//! Optional co-located import adapter. Never accepts executable/root paths from a browser.
use crate::{ApiError, AppState};
use axum::{
    Extension, Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::StatusCode,
    response::Response,
    routing::{get, post, put},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
    sync::Arc,
};

#[derive(Clone, Serialize)]
pub struct GameImportConfig {
    pub python: String,
    pub worker: String,
    pub mame: String,
    pub ini: String,
    pub rom_root: String,
    pub sample_root: String,
    pub stage_root: String,
    pub manifest: String,
    pub host_id: String,
    pub chdman: Option<String>,
}

#[derive(Clone)]
struct ImportService(Option<GameImportConfig>, Arc<tokio::sync::Semaphore>);

pub fn with_game_imports(app: Router, config: Option<GameImportConfig>) -> Router {
    let service = ImportService(config, Arc::new(tokio::sync::Semaphore::new(1)));
    app.layer(Extension(Arc::new(service)))
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/admin/game-imports", get(status).post(start))
        .route("/api/v1/admin/game-imports/{job}", get(job_status))
        .route("/api/v1/admin/game-imports/{job}/files", post(add_file))
        .route(
            "/api/v1/admin/game-imports/{job}/files/{file}/{offset}",
            put(chunk).layer(DefaultBodyLimit::max(4 * 1024 * 1024)),
        )
        .route("/api/v1/admin/game-imports/{job}/{action}", post(action))
}

pub(crate) async fn script() -> Response {
    crate::static_response(
        "text/javascript; charset=utf-8",
        include_str!("../static/admin/game-import.js"),
    )
}

type Service = Option<Extension<Arc<ImportService>>>;
fn config(service: Service) -> Result<GameImportConfig, ApiError> {
    service.and_then(|service| service.0.0.clone()).ok_or_else(|| ApiError::new(StatusCode::SERVICE_UNAVAILABLE,
        "game_import_disabled", "Game imports are not configured. Ask the operator to configure the co-located MAME import directories."))
}

async fn run(
    config: GameImportConfig,
    action: &'static str,
    value: Value,
    data: Vec<u8>,
) -> Result<Value, ApiError> {
    tokio::task::spawn_blocking(move || {
        let mut command = Command::new(&config.python);
        command.env_clear();
        // Audit children need filesystem paths, not service/API/provider secrets.
        for name in ["PATH", "HOME", "LANG", "TMPDIR", "SYSTEMROOT"] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        let mut child = command
            .arg(&config.worker)
            .arg(serde_json::to_string(&config).unwrap())
            .arg(action)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "Could not start the configured import worker".to_owned())?;
        let mut input = child.stdin.take().unwrap();
        let write = (|| -> std::io::Result<()> {
            input.write_all(value.to_string().as_bytes())?;
            input.write_all(b"\n")?;
            input.write_all(&data)
        })();
        drop(input);
        if write.is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Import worker input failed".to_owned());
        }
        let output = child
            .wait_with_output()
            .map_err(|_| "Import worker failed".to_owned())?;
        if output.stdout.len() > 1024 * 1024 {
            return Err("Import worker output exceeded limit".to_owned());
        }
        let value: Value = serde_json::from_slice(&output.stdout)
            .map_err(|_| "Import worker returned invalid JSON".to_owned())?;
        if !output.status.success() {
            return Err(value["error"]
                .as_str()
                .unwrap_or("Import failed")
                .to_owned());
        }
        Ok(value)
    })
    .await
    .map_err(|_| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "game_import_failed",
            "Import worker task failed",
        )
    })?
    .map_err(|message| ApiError::new(StatusCode::CONFLICT, "game_import_failed", &message))
}

async fn status(service: Service) -> Json<Value> {
    let config = config(service).ok();
    Json(
        json!({"enabled":config.is_some(),"host_id":config.map(|c|c.host_id),"chunk_bytes":4*1024*1024,"max_file_bytes":16_u64*1024*1024*1024}),
    )
}
async fn start(
    State(state): State<AppState>,
    service: Service,
    Json(value): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let config = config(service)?;
    state
        .runtime_hosts
        .get(
            config.host_id.clone(),
            crate::unix_time_ms(),
            state.offline_after_ms,
        )
        .await
        .map_err(ApiError::store)?;
    Ok(Json(run(config, "start", value, vec![]).await?))
}
async fn job_status(service: Service, Path(job): Path<String>) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        run(config(service)?, "status", json!({"job":job}), vec![]).await?,
    ))
}
async fn add_file(
    service: Service,
    Path(job): Path<String>,
    Json(mut value): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    if !value.is_object() {
        return Err(ApiError::bad_request(
            "invalid_import_file",
            "Expected a file object",
        ));
    }
    value["job"] = json!(job);
    Ok(Json(run(config(service)?, "add", value, vec![]).await?))
}
async fn chunk(
    service: Service,
    Path((job, file, offset)): Path<(String, String, u64)>,
    data: Bytes,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        run(
            config(service)?,
            "chunk",
            json!({"job":job,"file":file,"offset":offset}),
            data.to_vec(),
        )
        .await?,
    ))
}
async fn action(
    service: Service,
    Path((job, action)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    let gate = service.as_ref().map(|service| service.0.1.clone());
    let config = config(service)?;
    if action == "discard" {
        return Ok(Json(
            run(config, "discard", json!({"job":job}), vec![]).await?,
        ));
    }
    let operation = match action.as_str() {
        "audit" => "audit",
        "install" => "install",
        _ => {
            return Err(ApiError::bad_request(
                "invalid_import_action",
                "Unknown import action",
            ));
        }
    };
    let result = run(
        config.clone(),
        "begin",
        json!({"job":job,"action":operation}),
        vec![],
    )
    .await?;
    tokio::spawn(async move {
        let Ok(_permit) = gate.unwrap().acquire_owned().await else {
            return;
        };
        if run(config, operation, json!({"job":job}), vec![])
            .await
            .is_err()
        {
            eprintln!(
                "Game import background operation did not complete; inspect the import status."
            );
        }
    });
    Ok(Json(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Method, Request},
    };
    use tower::ServiceExt;
    #[tokio::test]
    async fn imports_fail_closed_and_require_existing_admin_bearer() {
        let app = crate::app().await.unwrap();
        for path in [
            "/api/v1/admin/game-imports",
            "/api/v1/admin/game-imports/invalid",
        ] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/game-imports")
                    .header("authorization", "Bearer test-seat-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap()["enabled"],
            false
        );
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/game-imports")
                    .method(Method::POST)
                    .header("authorization", "Bearer test-seat-token")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"game":"aof"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
