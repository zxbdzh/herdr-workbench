use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};
use herdr_workbench_app_core::WorkspaceRepository;
use serde::Serialize;

pub struct AppState<R> {
    pub workspaces: Arc<R>,
}

impl<R> Clone for AppState<R> {
    fn clone(&self) -> Self {
        Self {
            workspaces: Arc::clone(&self.workspaces),
        }
    }
}
impl<R> AppState<R> {
    pub fn new(workspaces: Arc<R>) -> Self {
        Self { workspaces }
    }
}

pub fn router<R>(state: AppState<R>) -> Router
where
    R: WorkspaceRepository + 'static,
{
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/workspaces", get(workspaces::<R>))
        .route("/api/v1/workspaces/{id}/state", get(workspace_state))
        .with_state(state)
}

pub fn empty_router() -> Router {
    router(AppState::new(Arc::new(EmptyWorkspaceRepository)))
}

async fn health() -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::OK, Json(serde_json::json!({"status": "ready"})))
}

#[derive(Serialize)]
struct WorkspaceListResponse {
    workspaces: Vec<herdr_workbench_domain::Workspace>,
}

async fn workspaces<R>(
    State(state): State<AppState<R>>,
) -> Result<Json<WorkspaceListResponse>, ApiError>
where
    R: WorkspaceRepository + 'static,
{
    state
        .workspaces
        .list()
        .await
        .map(|workspaces| Json(WorkspaceListResponse { workspaces }))
        .map_err(ApiError::from)
}

async fn workspace_state(Path(id): Path<String>) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "error": {
                "code": "workspace_not_found",
                "message": "workspace was not found",
                "request_id": null,
                "details": {"workspace_id": id}
            }
        })),
    )
}

#[derive(Debug)]
struct EmptyWorkspaceRepository;

#[async_trait::async_trait]
impl WorkspaceRepository for EmptyWorkspaceRepository {
    async fn find_by_herdr_id(
        &self,
        _id: &herdr_workbench_domain::HerdrWorkspaceId,
    ) -> Result<Option<herdr_workbench_domain::Workspace>, herdr_workbench_app_core::RepositoryError>
    {
        Ok(None)
    }

    async fn list(
        &self,
    ) -> Result<Vec<herdr_workbench_domain::Workspace>, herdr_workbench_app_core::RepositoryError>
    {
        Ok(Vec::new())
    }

    async fn insert(
        &self,
        _workspace: herdr_workbench_domain::Workspace,
    ) -> Result<(), herdr_workbench_app_core::RepositoryError> {
        Ok(())
    }

    async fn increment_revision(
        &self,
        _workspace_id: &herdr_workbench_domain::WorkbenchWorkspaceId,
    ) -> Result<u64, herdr_workbench_app_core::RepositoryError> {
        Ok(0)
    }
}

#[derive(Debug)]
struct ApiError;

impl From<herdr_workbench_app_core::RepositoryError> for ApiError {
    fn from(_: herdr_workbench_app_core::RepositoryError) -> Self {
        Self
    }
}

impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": {"code": "internal_error", "message": "internal server error"}})),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use herdr_workbench_app_core::{BindWorkspace, InMemoryWorkspaceRepository};
    use herdr_workbench_domain::HerdrWorkspaceContext;
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_endpoint_reports_ready() {
        let response = empty_router()
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
    async fn workspace_list_returns_repository_state() {
        let repository = Arc::new(InMemoryWorkspaceRepository::default());
        BindWorkspace::new(repository.as_ref())
            .execute(
                HerdrWorkspaceContext::new(
                    "herdr-1",
                    "Siftmark",
                    std::path::PathBuf::from(r"C:\projects\siftmark"),
                )
                .unwrap(),
            )
            .await
            .unwrap();

        let response = router(AppState::new(repository))
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["workspaces"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn missing_workspace_returns_the_stable_error_envelope() {
        let response = empty_router()
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
