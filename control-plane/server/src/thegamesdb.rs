//! Server-only, explicitly reviewed metadata acquisition. No runtime metadata imports.
use crate::{ApiError, AppState, unix_time_ms};
use axum::{
    Extension, Json, Router,
    body::Body,
    extract::State,
    http::{StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use control_protocol::GameMetadata;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{io::AsyncWriteExt, sync::Mutex};

const API: &str = "https://api.thegamesdb.net";
const TTL_MS: u64 = 7 * 24 * 60 * 60 * 1000;
const JSON_LIMIT: usize = 4 * 1024 * 1024;
const IMAGE_LIMIT: usize = 10 * 1024 * 1024;
const FIELDS: &[&str] = &[
    "sort_title",
    "description",
    "genre",
    "release_year",
    "manufacturer",
];
const IMAGE_FIELDS: &[&str] = &[
    "artwork_path",
    "screenshot_path",
    "marquee_path",
    "logo_path",
];

pub fn with_thegamesdb(
    app: Router,
    key: Option<String>,
    cache: PathBuf,
) -> Result<Router, &'static str> {
    Ok(app.layer(Extension(Arc::new(Provider::new(
        key,
        cache,
        API.to_owned(),
    )?))))
}

struct Provider {
    // Never Debug-format this struct or reqwest errors: the API requires a query key.
    key: Option<String>,
    client: reqwest::Client,
    base: String,
    cache: PathBuf,
    gate: Mutex<()>,
    allowance: AtomicU64,
    backoff_until: AtomicU64,
    last_request: Mutex<tokio::time::Instant>,
    #[cfg(test)]
    test_image_base: Option<String>,
}

impl Provider {
    fn new(key: Option<String>, cache: PathBuf, base: String) -> Result<Self, &'static str> {
        let key = key
            .map(|key| key.trim().to_owned())
            .filter(|key| !key.is_empty());
        if key.as_ref().is_some_and(|key| {
            key.len() < 16 || key.len() > 256 || key.chars().any(char::is_control)
        }) {
            return Err(
                "FOURPLAY_THEGAMESDB_API_KEY must be a nonempty key of 16-256 characters without control characters",
            );
        }
        Ok(Self {
            key,
            base,
            cache,
            gate: Mutex::new(()),
            allowance: AtomicU64::new(u64::MAX),
            backoff_until: AtomicU64::new(0),
            last_request: Mutex::new(tokio::time::Instant::now() - Duration::from_millis(250)),
            #[cfg(test)]
            test_image_base: None,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .connect_timeout(Duration::from_secs(5))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|_| "metadata HTTP client could not be initialized")?,
        })
    }

    async fn data(&self, path: &str, params: &[(&str, String)]) -> Result<Value, ApiError> {
        if self
            .key
            .as_ref()
            .is_some_and(|key| params.iter().any(|(_, value)| value.contains(key)))
        {
            return Err(failure(
                "Do not use the provider credential as a search term",
            ));
        }
        let identity =
            serde_json::to_string(&(path, params)).map_err(|_| failure("cache identity failed"))?;
        let mut hasher = DefaultHasher::new();
        identity.hash(&mut hasher);
        let cache_file = self.cache.join(format!("{:016x}.json", hasher.finish()));
        if let Ok(bytes) = tokio::fs::read(&cache_file).await
            && bytes.len() <= JSON_LIMIT
            && let Ok(cached) = serde_json::from_slice::<Value>(&bytes)
            && cached["identity"].as_str() == Some(&identity)
            && cached["fetched_ms"]
                .as_u64()
                .is_some_and(|when| when <= unix_time_ms() && unix_time_ms() - when < TTL_MS)
        {
            return Ok(cached["data"].clone());
        }
        let key = self.key.as_ref().ok_or_else(disabled)?;
        if self.backoff_until.load(Ordering::Relaxed) > unix_time_ms() {
            return Err(ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "metadata_provider_backoff",
                "TheGamesDB requests are paused after a provider limit; retry later",
            ));
        }
        {
            let mut last = self.last_request.lock().await;
            tokio::time::sleep_until(*last + Duration::from_millis(250)).await;
            *last = tokio::time::Instant::now();
        }
        let response = self
            .client
            .get(format!("{}{path}", self.base))
            .query(params)
            .query(&[("apikey", key)])
            .send()
            .await
            .map_err(|_| failure("TheGamesDB request failed; check server connectivity"))?;
        if response.status().as_u16() == 429 || response.status().as_u16() == 403 {
            let delay = response
                .headers()
                .get("retry-after")
                .and_then(|header| header.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(60)
                .clamp(60, 3600);
            self.backoff_until
                .store(unix_time_ms() + delay * 1000, Ordering::Relaxed);
            return Err(ApiError::new(
                StatusCode::BAD_GATEWAY,
                "metadata_provider_key_or_limit",
                "TheGamesDB rejected the server key or request allowance; no automatic retry was attempted",
            ));
        }
        if !response.status().is_success() {
            return Err(failure("TheGamesDB returned an unsuccessful response"));
        }
        let bytes = bounded(response, JSON_LIMIT).await?;
        let mut value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| failure("TheGamesDB returned invalid JSON"))?;
        if value["code"].as_u64() != Some(200) || !value["data"].is_object() {
            return Err(failure(
                "TheGamesDB response did not contain successful data",
            ));
        }
        if let Some(remaining) = value["remaining_monthly_allowance"].as_u64() {
            self.allowance.store(remaining, Ordering::Relaxed);
        }
        // Drop paging URLs (which may contain the key) and redact any reflected key.
        let data = value.get_mut("data").expect("checked object");
        redact(data, key);
        let result = data.clone();
        tokio::fs::create_dir_all(&self.cache)
            .await
            .map_err(|_| failure("metadata cache directory is not writable"))?;
        let content = serde_json::to_vec(
            &json!({"identity": identity, "fetched_ms": unix_time_ms(), "data": result}),
        )
        .map_err(|_| failure("metadata cache serialization failed"))?;
        // Unique temporary + rename; no API key or paging URL is stored.
        let temporary = self.cache.join(format!("{}.tmp", uuid::Uuid::new_v4()));
        tokio::fs::write(&temporary, content)
            .await
            .map_err(|_| failure("metadata cache could not be written"))?;
        tokio::fs::rename(&temporary, &cache_file)
            .await
            .map_err(|_| failure("metadata cache could not be published"))?;
        Ok(result)
    }

    async fn arcade(&self) -> Result<u64, ApiError> {
        let platforms = self
            .data(
                "/v1/Platforms/ByPlatformName",
                &[("name", "Arcade".to_owned())],
            )
            .await?;
        let matches: Vec<u64> = records(&platforms["platforms"])
            .into_iter()
            .filter(|v| {
                v["name"]
                    .as_str()
                    .is_some_and(|name| name.eq_ignore_ascii_case("Arcade"))
            })
            .filter_map(|v| number(&v["id"]))
            .collect();
        if matches.len() != 1 {
            return Err(failure(
                "TheGamesDB Arcade platform could not be uniquely identified",
            ));
        }
        Ok(matches[0])
    }

    async fn review(&self, id: u64) -> Result<Review, ApiError> {
        if id == 0 {
            return Err(ApiError::bad_request(
                "invalid_provider_id",
                "Choose a valid TheGamesDB game ID",
            ));
        }
        let arcade = self.arcade().await?;
        let data = self
            .data(
                "/v1/Games/ByGameID",
                &[
                    ("id", id.to_string()),
                    ("fields", "overview,genres,publishers,platform".to_owned()),
                ],
            )
            .await?;
        let game = records(&data["games"])
            .into_iter()
            .find(|v| number(&v["id"]) == Some(id) && number(&v["platform"]) == Some(arcade))
            .ok_or_else(|| {
                ApiError::bad_request(
                    "not_arcade_match",
                    "The selected match is not an Arcade game",
                )
            })?;
        let genres = self.data("/v1/Genres", &[]).await?;
        let publishers = self.data("/v1/Publishers", &[]).await?;
        let images = self
            .data("/v1/Games/Images", &[("games_id", id.to_string())])
            .await?;
        let title = clip(text(&game["game_title"]), 256)
            .ok_or_else(|| failure("Provider game has no title"))?;
        let mut artwork = Vec::new();
        for image in records(&images["images"][id.to_string()])
            .into_iter()
            .take(50)
        {
            let Some(filename) = text(&image["filename"]) else {
                continue;
            };
            if image_url(&filename).is_err() {
                continue;
            }
            artwork.push(Image {
                filename,
                kind: text(&image["type"]).unwrap_or_else(|| "artwork".to_owned()),
                side: text(&image["side"]),
            });
        }
        Ok(Review {
            id,
            title: title.clone(),
            metadata: GameMetadata {
                sort_title: Some(title),
                description: clip(text(&game["overview"]), 4096),
                release_year: year(&game["release_date"]),
                genre: clip(dictionary_names(&game["genres"], &genres["genres"]), 128),
                manufacturer: clip(
                    dictionary_names(&game["publishers"], &publishers["publishers"]),
                    256,
                ),
                ..GameMetadata::default()
            },
            images: artwork,
        })
    }

    async fn image(
        &self,
        filename: &str,
    ) -> Result<(Vec<u8>, &'static str, &'static str), ApiError> {
        let url = image_url(filename)?;
        #[cfg(test)]
        let url = match &self.test_image_base {
            Some(base) => {
                reqwest::Url::parse(&format!("{base}/{filename}")).expect("test fixture URL")
            }
            None => url,
        };
        let mut hasher = DefaultHasher::new();
        filename.hash(&mut hasher);
        let cached_path = self
            .cache
            .join("images")
            .join(format!("{:016x}.bin", hasher.finish()));
        if let Ok(metadata) = tokio::fs::metadata(&cached_path).await
            && metadata.len() <= IMAGE_LIMIT as u64
            && metadata
                .modified()
                .ok()
                .and_then(|when| when.elapsed().ok())
                .is_some_and(|age| age.as_millis() < TTL_MS as u128)
            && let Ok(bytes) = tokio::fs::read(&cached_path).await
            && let Some((mime, extension)) = image_format(&bytes)
        {
            return Ok((bytes, mime, extension));
        }
        // No API key on CDN requests; no redirects to another host.
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|_| failure("TheGamesDB image could not be downloaded"))?;
        if !response.status().is_success() {
            return Err(failure("TheGamesDB image download failed"));
        }
        let bytes = bounded(response, IMAGE_LIMIT).await?;
        let (mime, extension) = image_format(&bytes)
            .ok_or_else(|| failure("Provider image is not a supported PNG, JPEG, GIF or WebP"))?;
        tokio::fs::create_dir_all(cached_path.parent().expect("cache parent"))
            .await
            .map_err(|_| failure("Image cache directory is not writable"))?;
        let temporary = self
            .cache
            .join("images")
            .join(format!("{}.tmp", uuid::Uuid::new_v4()));
        tokio::fs::write(&temporary, &bytes)
            .await
            .map_err(|_| failure("Image cache could not be written"))?;
        tokio::fs::rename(temporary, cached_path)
            .await
            .map_err(|_| failure("Image cache could not be published"))?;
        Ok((bytes, mime, extension))
    }
}

#[derive(Serialize, Deserialize)]
struct Candidate {
    id: u64,
    title: String,
    release_year: Option<u16>,
}
#[derive(Serialize, Deserialize)]
struct Image {
    filename: String,
    kind: String,
    side: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Review {
    id: u64,
    title: String,
    metadata: GameMetadata,
    images: Vec<Image>,
}
#[derive(Deserialize)]
struct Search {
    query: String,
}
#[derive(Deserialize)]
struct Choice {
    id: u64,
}
#[derive(Deserialize)]
struct ImageChoice {
    id: u64,
    index: usize,
}
#[derive(Deserialize)]
struct ImageImport {
    index: usize,
    field: String,
}
#[derive(Deserialize)]
struct Stage {
    id: u64,
    fields: Vec<String>,
    images: Vec<ImageImport>,
}
type OptionalProvider = Option<Extension<Arc<Provider>>>;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/metadata/thegamesdb/status", get(status))
        .route("/api/v1/metadata/thegamesdb/search", post(search))
        .route("/api/v1/metadata/thegamesdb/review", post(review))
        .route("/api/v1/metadata/thegamesdb/image", post(image))
        .route(
            "/api/v1/games/{game_id}/metadata/thegamesdb/stage",
            post(stage),
        )
}
async fn status(provider: OptionalProvider) -> Json<Value> {
    let provider = provider.map(|Extension(provider)| provider);
    let allowance = provider
        .as_ref()
        .map(|p| p.allowance.load(Ordering::Relaxed))
        .filter(|v| *v != u64::MAX);
    Json(
        json!({"provider": "thegamesdb", "configured": provider.is_some_and(|p| p.key.is_some()), "last_live_monthly_allowance": allowance, "cache_days": 7}),
    )
}
fn configured(provider: OptionalProvider) -> Result<Arc<Provider>, ApiError> {
    provider
        .map(|Extension(p)| p)
        .filter(|p| p.key.is_some())
        .ok_or_else(disabled)
}
fn disabled() -> ApiError {
    ApiError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "metadata_provider_not_configured",
        "Set FOURPLAY_THEGAMESDB_API_KEY on the server; never enter it in the browser",
    )
}
fn failure(message: &str) -> ApiError {
    ApiError::new(StatusCode::BAD_GATEWAY, "metadata_provider_failed", message)
}
fn busy() -> ApiError {
    ApiError::new(
        StatusCode::TOO_MANY_REQUESTS,
        "metadata_provider_busy",
        "Another metadata request is running; try again after it finishes",
    )
}

async fn search(
    provider: OptionalProvider,
    Json(request): Json<Search>,
) -> Result<Json<Value>, ApiError> {
    let query = request.query.trim();
    if query.len() < 2 || query.len() > 120 || query.chars().any(char::is_control) {
        return Err(ApiError::bad_request(
            "invalid_metadata_query",
            "Search must contain 2-120 characters without control characters",
        ));
    }
    let provider = configured(provider)?;
    let _guard = provider.gate.try_lock().map_err(|_| busy())?;
    let arcade = provider.arcade().await?;
    let data = provider
        .data(
            "/v1.1/Games/ByGameName",
            &[
                ("name", query.to_owned()),
                ("filter[platform]", arcade.to_string()),
            ],
        )
        .await?;
    let candidates: Vec<Candidate> = records(&data["games"])
        .into_iter()
        .filter(|v| number(&v["platform"]) == Some(arcade))
        .take(25)
        .filter_map(|v| {
            Some(Candidate {
                id: number(&v["id"])?,
                title: text(&v["game_title"])?,
                release_year: year(&v["release_date"]),
            })
        })
        .collect();
    Ok(Json(
        json!({"candidates": candidates, "platform": "Arcade", "note": "First page only. Refine the search if the desired match is missing."}),
    ))
}
async fn review(
    provider: OptionalProvider,
    Json(request): Json<Choice>,
) -> Result<Json<Review>, ApiError> {
    let provider = configured(provider)?;
    let _guard = provider.gate.try_lock().map_err(|_| busy())?;
    Ok(Json(provider.review(request.id).await?))
}
async fn image(
    provider: OptionalProvider,
    Json(request): Json<ImageChoice>,
) -> Result<Response, ApiError> {
    let provider = configured(provider)?;
    let _guard = provider.gate.try_lock().map_err(|_| busy())?;
    let review = provider.review(request.id).await?;
    let filename = &review
        .images
        .get(request.index)
        .ok_or_else(|| ApiError::bad_request("invalid_image", "Choose a listed provider image"))?
        .filename;
    let (bytes, mime, _) = provider.image(filename).await?;
    Ok(([(CONTENT_TYPE, mime)], Body::from(bytes)).into_response())
}
async fn stage(
    State(state): State<AppState>,
    axum::extract::Path(game_id): axum::extract::Path<String>,
    provider: OptionalProvider,
    Json(request): Json<Stage>,
) -> Result<Json<Value>, ApiError> {
    validate_stage(&request)?;
    state
        .runtime_hosts
        .get_game_metadata(game_id.clone())
        .await
        .map_err(ApiError::store)?;
    if !game_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(ApiError::bad_request(
            "invalid_game",
            "Game ID cannot be used as an asset directory",
        ));
    }
    let provider = configured(provider)?;
    let _guard = provider.gate.try_lock().map_err(|_| busy())?;
    let reviewed = provider.review(request.id).await?;
    let source = serde_json::to_value(&reviewed.metadata)
        .map_err(|_| failure("metadata serialization failed"))?;
    let mut patch = serde_json::Map::new();
    for field in &request.fields {
        if !source[field].is_null() {
            patch.insert(field.clone(), source[field].clone());
        }
    }
    // Download and validate ALL chosen images before creating any local asset.
    let mut downloads = Vec::new();
    for choice in &request.images {
        let image = reviewed.images.get(choice.index).ok_or_else(|| {
            ApiError::bad_request("invalid_image", "Choose a listed provider image")
        })?;
        let (bytes, _, extension) = provider.image(&image.filename).await?;
        downloads.push((
            choice.field.clone(),
            bytes,
            extension,
            image.filename.clone(),
        ));
    }
    let import_id = uuid::Uuid::new_v4().to_string();
    let mut created = Vec::new();
    let result = async {
        for (field, bytes, extension, _) in &downloads {
            let root = state.asset_root.as_ref().ok_or_else(|| failure("Asset root must be configured to import artwork"))?;
            let relative = format!("media/{game_id}/thegamesdb/{}/{import_id}-{field}.{extension}", request.id);
            let path = root.join(&relative);
            tokio::fs::create_dir_all(path.parent().expect("constructed parent")).await.map_err(|_| failure("Imported asset directory could not be created"))?;
            let mut file = tokio::fs::OpenOptions::new().write(true).create_new(true).open(&path).await.map_err(|_| failure("Imported asset could not be created without overwrite"))?;
            created.push(path);
            file.write_all(bytes).await.map_err(|_| failure("Imported asset could not be written"))?;
            patch.insert(field.clone(), json!(relative));
        }
        let imports = provider.cache.join("imports");
        tokio::fs::create_dir_all(&imports).await.map_err(|_| failure("Import provenance directory is not writable"))?;
        let provenance = json!({"provider": "thegamesdb", "provider_id": request.id, "game_id": game_id, "staged_ms": unix_time_ms(), "fields": request.fields, "images": downloads.iter().map(|(field, _, _, filename)| json!({"field":field,"filename":filename})).collect::<Vec<_>>(), "patch": patch, "published": false});
        tokio::fs::write(imports.join(format!("{import_id}.json")), serde_json::to_vec(&provenance).map_err(|_| failure("Provenance serialization failed"))?).await.map_err(|_| failure("Import provenance could not be written"))?;
        Ok(Json(json!({"patch": patch, "import_id": import_id, "provider_id": request.id, "message": "Staged only. Save metadata in the editor to publish; existing catalog values and files were not replaced."})))
    }.await;
    if result.is_err() {
        for path in created {
            let _ = tokio::fs::remove_file(path).await;
        }
    }
    result
}

fn validate_stage(request: &Stage) -> Result<(), ApiError> {
    if request.id == 0
        || request.fields.len() > FIELDS.len()
        || request.images.len() > IMAGE_FIELDS.len()
        || request.fields.iter().any(|f| !FIELDS.contains(&f.as_str()))
        || request
            .images
            .iter()
            .any(|image| !IMAGE_FIELDS.contains(&image.field.as_str()))
        || request
            .fields
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != request.fields.len()
        || request
            .images
            .iter()
            .map(|i| &i.field)
            .collect::<std::collections::HashSet<_>>()
            .len()
            != request.images.len()
        || (request.fields.is_empty() && request.images.is_empty())
    {
        return Err(ApiError::bad_request(
            "invalid_metadata_import",
            "Select unique descriptive fields and at most one image per artwork field; runtime fields are never imported",
        ));
    }
    Ok(())
}
fn records(value: &Value) -> Vec<&Value> {
    match value {
        Value::Array(values) => values.iter().collect(),
        Value::Object(values) => values.values().collect(),
        _ => Vec::new(),
    }
}
fn number(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str()?.parse().ok())
}
fn text(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}
fn clip(value: Option<String>, max_bytes: usize) -> Option<String> {
    value.map(|mut value| {
        let mut end = value.len().min(max_bytes);
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        value.truncate(end);
        value
    })
}
fn year(value: &Value) -> Option<u16> {
    value
        .as_str()?
        .get(..4)?
        .parse::<u16>()
        .ok()
        .filter(|y| (1900..=2100).contains(y))
}
fn dictionary_names(ids: &Value, dictionary: &Value) -> Option<String> {
    let names: Vec<_> = records(ids)
        .into_iter()
        .filter_map(number)
        .filter_map(|id| {
            records(dictionary)
                .into_iter()
                .find(|v| number(&v["id"]) == Some(id))
                .and_then(|v| text(&v["name"]))
        })
        .collect();
    (!names.is_empty()).then(|| names.join(" / "))
}
fn redact(value: &mut Value, key: &str) {
    match value {
        Value::String(v) => *v = v.replace(key, "[redacted]"),
        Value::Array(v) => v.iter_mut().for_each(|v| redact(v, key)),
        Value::Object(v) => v.values_mut().for_each(|v| redact(v, key)),
        _ => {}
    }
}
fn image_url(filename: &str) -> Result<reqwest::Url, ApiError> {
    if filename.len() > 512
        || filename.is_empty()
        || filename
            .split('/')
            .any(|v| v.is_empty() || v == "." || v == "..")
        || filename.contains("..")
        || filename
            .chars()
            .any(|c| c.is_control() || matches!(c, ':' | '\\' | '?' | '#' | '%'))
    {
        return Err(failure("Provider image path is unsafe"));
    }
    let mut url =
        reqwest::Url::parse("https://cdn.thegamesdb.net/images/original/").expect("constant URL");
    url.path_segments_mut()
        .expect("constant path")
        .pop_if_empty()
        .extend(filename.split('/'));
    Ok(url)
}
fn image_format(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(("image/png", "png"))
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some(("image/jpeg", "jpg"))
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(("image/gif", "gif"))
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some(("image/webp", "webp"))
    } else {
        None
    }
}
async fn bounded(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>, ApiError> {
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        return Err(failure(
            "Provider response exceeds the configured size limit",
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| failure("Provider response could not be read"))?
    {
        if bytes.len() + chunk.len() > limit {
            return Err(failure(
                "Provider response exceeds the configured size limit",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        extract::{OriginalUri, Query},
        http::{Method, Request},
    };
    use http_body_util::BodyExt;
    use std::collections::HashMap;
    use tower::ServiceExt;

    const KEY: &str = "fixture-only-key-not-a-real-secret";
    async fn fixture(
        State(count): State<Arc<AtomicU64>>,
        OriginalUri(uri): OriginalUri,
        Query(query): Query<HashMap<String, String>>,
    ) -> Response {
        count.fetch_add(1, Ordering::Relaxed);
        if uri.path() == "/images/test.png" {
            return (
                [(CONTENT_TYPE, "image/png")],
                b"\x89PNG\r\n\x1a\nfixture".to_vec(),
            )
                .into_response();
        }
        if query.get("name").is_some_and(|q| q == "fail") {
            return (StatusCode::FORBIDDEN, KEY).into_response();
        }
        let game = json!({"id": 1, "game_title": "Aliens", "platform": 23, "overview": "Provider description", "release_date": "1990-01-01", "genres": [3], "publishers": [4], "players": 99});
        let console = json!({"id": 2, "game_title": "Aliens console", "platform": 2});
        let data = match uri.path() {
            "/v1/Platforms/ByPlatformName" => {
                json!({"platforms": [{"id":23,"name":"Arcade"},{"id":2,"name":"Console"}]})
            }
            "/v1.1/Games/ByGameName" => json!({"games": [game, console]}),
            "/v1/Games/ByGameID" => {
                json!({"games": [if query.get("id").is_some_and(|id| id == "2") {console} else {game}]})
            }
            "/v1/Genres" => json!({"genres": {"3": {"id":3,"name":"Action"}}}),
            "/v1/Publishers" => json!({"publishers": {"4": {"id":4,"name":"Konami"}}}),
            "/v1/Games/Images" => {
                json!({"images": {"1": [{"filename":"test.png","type":"screenshot"},{"filename":"../evil.png","type":"boxart"}]}})
            }
            _ => return StatusCode::NOT_FOUND.into_response(),
        };
        Json(json!({"code":200,"data":data,"remaining_monthly_allowance":123,"pages":{"next":format!("https://api.example/?apikey={KEY}")}})).into_response()
    }
    async fn mock() -> (String, Arc<AtomicU64>, tokio::task::JoinHandle<()>) {
        let count = Arc::new(AtomicU64::new(0));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = Router::new()
            .fallback(get(fixture))
            .with_state(count.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, server).await.unwrap();
        });
        (base, count, task)
    }
    async fn call(
        app: Router,
        method: Method,
        path: &str,
        body: Value,
        authenticated: bool,
    ) -> (u16, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(CONTENT_TYPE, "application/json");
        if authenticated {
            request = request.header("authorization", "Bearer test-seat-token");
        }
        let response = app
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = response.status().as_u16();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap())
    }
    #[test]
    fn source_paths_formats_and_import_fields_are_restricted() {
        for path in [
            "../bad.png",
            "https://evil/image.jpg",
            "a%2fb.png",
            "a//b",
            "a?key=x",
            "a\\b",
        ] {
            assert!(image_url(path).is_err());
        }
        assert_eq!(
            image_url("boxart/front/game 1.jpg")
                .ok()
                .unwrap()
                .host_str(),
            Some("cdn.thegamesdb.net")
        );
        assert_eq!(
            image_format(b"\xff\xd8\xffrest"),
            Some(("image/jpeg", "jpg"))
        );
        assert!(image_format(b"<script>bad</script>").is_none());
        assert_eq!(clip(Some("ééé".into()), 3).as_deref(), Some("é"));
        for fields in [
            vec!["player_count"],
            vec!["control_notes"],
            vec!["description", "description"],
        ] {
            assert!(
                validate_stage(&Stage {
                    id: 1,
                    fields: fields.into_iter().map(str::to_owned).collect(),
                    images: vec![]
                })
                .is_err()
            );
        }
        assert!(
            validate_stage(&Stage {
                id: 1,
                fields: vec![],
                images: vec![
                    ImageImport {
                        index: 0,
                        field: "logo_path".into()
                    },
                    ImageImport {
                        index: 1,
                        field: "logo_path".into()
                    }
                ]
            })
            .is_err()
        );
        assert!(Provider::new(Some("short".into()), PathBuf::new(), API.into()).is_err());
        let mut value = json!({"data":[format!("prefix-{KEY}")]});
        redact(&mut value, KEY);
        assert!(!value.to_string().contains(KEY));
    }
    #[tokio::test]
    async fn routes_require_authentication_and_missing_key_fails_closed() {
        let app = crate::app().await.unwrap();
        assert_eq!(
            call(
                app.clone(),
                Method::POST,
                "/api/v1/metadata/thegamesdb/search",
                json!({"query":"Aliens"}),
                false
            )
            .await
            .0,
            401
        );
        let (status, response) = call(
            app.clone(),
            Method::POST,
            "/api/v1/metadata/thegamesdb/search",
            json!({"query":"Aliens"}),
            true,
        )
        .await;
        assert_eq!(status, 503);
        assert_eq!(response["code"], "metadata_provider_not_configured");
        assert_eq!(
            call(
                app.clone(),
                Method::POST,
                "/api/v1/metadata/thegamesdb/search",
                json!({"query":"x"}),
                true
            )
            .await
            .0,
            400
        );
        assert_eq!(
            call(
                app,
                Method::GET,
                "/api/v1/metadata/thegamesdb/status",
                Value::Null,
                true
            )
            .await
            .1["configured"],
            false
        );
    }
    #[tokio::test]
    async fn cache_survives_restart_and_never_contains_key_or_provider_paging_urls() {
        let directory = tempfile::tempdir().unwrap();
        let (base, count, server) = mock().await;
        let provider =
            Provider::new(Some(KEY.into()), directory.path().into(), base.clone()).unwrap();
        let reviewed = provider
            .review(1)
            .await
            .unwrap_or_else(|error| panic!("{}", error.body.message));
        assert_eq!(
            reviewed.metadata.description.as_deref(),
            Some("Provider description")
        );
        assert_eq!(reviewed.metadata.manufacturer.as_deref(), Some("Konami"));
        assert_eq!(reviewed.metadata.genre.as_deref(), Some("Action"));
        assert!(reviewed.metadata.player_count.is_none());
        assert!(reviewed.metadata.control_notes.is_none());
        assert_eq!(reviewed.images.len(), 1);
        assert!(provider.review(2).await.is_err());
        let before = count.load(Ordering::Relaxed);
        server.abort();
        let restarted = Provider::new(Some(KEY.into()), directory.path().into(), base).unwrap();
        restarted
            .review(1)
            .await
            .unwrap_or_else(|error| panic!("{}", error.body.message));
        assert_eq!(count.load(Ordering::Relaxed), before);
        for file in std::fs::read_dir(directory.path()).unwrap() {
            let text = std::fs::read_to_string(file.unwrap().path()).unwrap();
            assert!(!text.contains(KEY));
            assert!(!text.contains("apikey"));
            assert!(!text.contains("paging"));
        }
    }
    #[tokio::test]
    async fn search_filters_console_matches_and_provider_failure_is_sanitized_and_backed_off() {
        let directory = tempfile::tempdir().unwrap();
        let (base, count, server) = mock().await;
        let provider =
            Arc::new(Provider::new(Some(KEY.into()), directory.path().into(), base).unwrap());
        let app = crate::app()
            .await
            .unwrap()
            .layer(Extension(provider.clone()));
        let (status, result) = call(
            app.clone(),
            Method::POST,
            "/api/v1/metadata/thegamesdb/search",
            json!({"query":"Aliens"}),
            true,
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(result["candidates"].as_array().unwrap().len(), 1);
        let before = count.load(Ordering::Relaxed);
        assert_eq!(
            call(
                app.clone(),
                Method::POST,
                "/api/v1/metadata/thegamesdb/search",
                json!({"query":"Aliens"}),
                true
            )
            .await
            .0,
            200
        );
        assert_eq!(count.load(Ordering::Relaxed), before);
        let (status, error) = call(
            app.clone(),
            Method::POST,
            "/api/v1/metadata/thegamesdb/search",
            json!({"query":"fail"}),
            true,
        )
        .await;
        assert_eq!(status, 502);
        assert!(!error.to_string().contains(KEY));
        let before = count.load(Ordering::Relaxed);
        assert_eq!(
            call(
                app.clone(),
                Method::POST,
                "/api/v1/metadata/thegamesdb/search",
                json!({"query":"other"}),
                true
            )
            .await
            .0,
            429
        );
        assert_eq!(count.load(Ordering::Relaxed), before);
        let _guard = provider.gate.lock().await;
        assert_eq!(
            call(
                app,
                Method::POST,
                "/api/v1/metadata/thegamesdb/search",
                json!({"query":"Aliens"}),
                true
            )
            .await
            .0,
            429
        );
        server.abort();
    }
    #[tokio::test]
    async fn stage_preserves_catalog_and_manual_assets_and_caches_images() {
        let directory = tempfile::tempdir().unwrap();
        let (base, count, server) = mock().await;
        let mut provider = Provider::new(
            Some(KEY.into()),
            directory.path().join("cache"),
            base.clone(),
        )
        .unwrap();
        provider.test_image_base = Some(format!("{base}/images"));
        let state = AppState::in_memory(Duration::from_secs(15)).await.unwrap();
        state.runtime_hosts.upsert_registration("host".into(), serde_json::from_value(json!({"display_name":"Test","agent_version":"1","capabilities":{"operating_system":"linux","architecture":"x86_64","logical_cpu_count":4,"memory_bytes":1024,"encoder_names":[],"emulator_adapters":["mame"]}})).unwrap(), unix_time_ms()).await.unwrap();
        state.runtime_hosts.replace_host_catalog("host".into(), serde_json::from_value(json!({"games":[{"id":"aliens","display_name":"Aliens","rom_name":"aliens","profile":{"width":288,"height":224,"refresh_hz":60.0,"rotation_degrees":0,"max_players":2,"buttons_per_player":2,"supports_save_state":true}}]})).unwrap()).await.unwrap();
        let metadata = GameMetadata {
            description: Some("My manual description".into()),
            screenshot_path: Some("media/manual.jpg".into()),
            player_count: Some(2),
            ..GameMetadata::default()
        };
        state
            .runtime_hosts
            .update_game_metadata("aliens".into(), metadata.clone())
            .await
            .unwrap();
        let mut state = state;
        let root = directory.path().join("assets");
        tokio::fs::create_dir_all(root.join("media")).await.unwrap();
        tokio::fs::write(root.join("media/manual.jpg"), b"original")
            .await
            .unwrap();
        state.asset_root = Some(root.clone());
        let app = crate::app_with_state(state.clone()).layer(Extension(Arc::new(provider)));
        let request = json!({"id":1,"fields":["description"],"images":[{"index":0,"field":"screenshot_path"}]});
        let (status, first) = call(
            app.clone(),
            Method::POST,
            "/api/v1/games/aliens/metadata/thegamesdb/stage",
            request.clone(),
            true,
        )
        .await;
        assert_eq!(status, 200);
        let before = count.load(Ordering::Relaxed);
        let (status, second) = call(
            app.clone(),
            Method::POST,
            "/api/v1/games/aliens/metadata/thegamesdb/stage",
            request,
            true,
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(count.load(Ordering::Relaxed), before);
        assert_ne!(
            first["patch"]["screenshot_path"],
            second["patch"]["screenshot_path"]
        );
        assert_eq!(
            state
                .runtime_hosts
                .get_game_metadata("aliens".into())
                .await
                .unwrap(),
            metadata
        );
        assert_eq!(
            std::fs::read(root.join("media/manual.jpg")).unwrap(),
            b"original"
        );
        assert!(
            std::fs::read(root.join(first["patch"]["screenshot_path"].as_str().unwrap()))
                .unwrap()
                .starts_with(b"\x89PNG")
        );
        assert!(
            directory
                .path()
                .join("cache/imports")
                .join(format!("{}.json", first["import_id"].as_str().unwrap()))
                .exists()
        );
        // Provenance publication failure must roll back only this request's
        // unique files, leaving earlier imports and the manual asset alone.
        let failed_cache = directory.path().join("failed-cache");
        std::fs::create_dir_all(&failed_cache).unwrap();
        std::fs::write(failed_cache.join("imports"), b"blocked directory").unwrap();
        let mut failed = Provider::new(Some(KEY.into()), failed_cache, base.clone()).unwrap();
        failed.test_image_base = Some(format!("{base}/images"));
        let failed_app = crate::app_with_state(state.clone()).layer(Extension(Arc::new(failed)));
        let images_dir = root.join("media/aliens/thegamesdb/1");
        let before_files = std::fs::read_dir(&images_dir).unwrap().count();
        let (status, _) = call(
            failed_app,
            Method::POST,
            "/api/v1/games/aliens/metadata/thegamesdb/stage",
            json!({"id":1,"fields":[],"images":[{"index":0,"field":"logo_path"}]}),
            true,
        )
        .await;
        assert_eq!(status, 502);
        assert_eq!(std::fs::read_dir(images_dir).unwrap().count(), before_files);
        assert_eq!(
            state
                .runtime_hosts
                .get_game_metadata("aliens".into())
                .await
                .unwrap(),
            metadata
        );
        let response = reqwest::get(format!("{base}/images/test.png"))
            .await
            .unwrap();
        assert!(bounded(response, 4).await.is_err());
        server.abort();
    }
}
