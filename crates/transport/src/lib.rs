use axum::{Router, http::StatusCode, routing::get};

#[derive(Clone)]
pub struct AppState;

pub fn router() -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/workspaces", get(workspaces))
        .route("/api/v1/workspaces/{id}/state", get(workspace_state))
}

async fn health() -> (StatusCode, axum::Json<serde_json::Value>) {
    (
        StatusCode::OK,
        axum::Json(serde_json::json!({"status": "ready"})),
    )
}

async fn workspaces() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({"workspaces": []}))
}

async fn workspace_state(
    axum::extract::Path(id): axum::extract::Path<String>,
) -> (StatusCode, axum::Json<serde_json::Value>) {
    (
        StatusCode::NOT_FOUND,
        axum::Json(serde_json::json!({
            "error": {
                "code": "workspace_not_found",
                "message": "workspace was not found",
                "request_id": null,
                "details": {"workspace_id": id}
            }
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_endpoint_reports_ready() {
        let response = router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn missing_workspace_returns_the_stable_error_envelope() {
        let response = router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces/missing/state")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
