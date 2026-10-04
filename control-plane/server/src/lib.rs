use axum::{Json, Router, routing::get};
use control_protocol::{ApiInfo, ServiceStatus, StatusResponse};

pub fn app() -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(readiness))
        .route("/api/v1", get(api_info))
}

async fn health() -> Json<StatusResponse> {
    Json(StatusResponse {
        status: ServiceStatus::Ok,
    })
}

async fn readiness() -> Json<StatusResponse> {
    Json(StatusResponse {
        status: ServiceStatus::Ready,
    })
}

async fn api_info() -> Json<ApiInfo> {
    Json(ApiInfo::control_plane())
}

#[cfg(test)]
mod tests {
    use super::app;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use serde_json::{Value, json};
    use tower::ServiceExt;

    async fn get_json(path: &str) -> (u16, Value) {
        let response = app()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status().as_u16();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&body).unwrap())
    }

    #[tokio::test]
    async fn health_endpoint_reports_ok() {
        let (status, body) = get_json("/health").await;
        assert_eq!(status, 200);
        assert_eq!(body, json!({ "status": "ok" }));
    }

    #[tokio::test]
    async fn readiness_endpoint_reports_ready() {
        let (status, body) = get_json("/ready").await;
        assert_eq!(status, 200);
        assert_eq!(body, json!({ "status": "ready" }));
    }

    #[tokio::test]
    async fn versioned_api_root_describes_the_service() {
        let (status, body) = get_json("/api/v1").await;
        assert_eq!(status, 200);
        assert_eq!(
            body,
            json!({
                "service": "4-play-control-plane",
                "api_version": "v1"
            })
        );
    }

    #[tokio::test]
    async fn unknown_routes_return_not_found() {
        let response = app()
            .oneshot(Request::get("/api/v2").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), 404);
    }
}
