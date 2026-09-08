use std::sync::Arc;

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::{HeaderValue, StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use herdr_workbench_app_core::{
    CapturePreview, EventPublisher, OpenPreview, PreviewAdapter, PreviewDiagnosticsSink,
    PreviewError, PreviewScreenshotRepository, PreviewTransactionRepository, ScreenshotStore,
    WorkspaceRepository,
};
use herdr_workbench_contracts::{
    ApiDoc, PreviewDiagnosticDto, PreviewDiagnosticsResponse, PreviewOpenRequest,
    PreviewScreenshotResponse, PreviewStateResponse, WorkspaceDto, WorkspaceListResponse,
};
use rust_embed::RustEmbed;
use utoipa::OpenApi;

#[derive(RustEmbed)]
#[folder = "../../web/dist/"]
struct WebAssets;

pub struct AppState<W, P, A, E, S> {
    pub workspaces: Arc<W>,
    pub previews: Arc<P>,
    pub preview_adapter: Arc<A>,
    pub events: Arc<E>,
    pub screenshots: Arc<S>,
    pub diagnostics: Arc<dyn PreviewDiagnosticsSink>,
}

impl<W, P, A, E, S> Clone for AppState<W, P, A, E, S> {
    fn clone(&self) -> Self {
        Self {
            workspaces: Arc::clone(&self.workspaces),
            previews: Arc::clone(&self.previews),
            preview_adapter: Arc::clone(&self.preview_adapter),
            events: Arc::clone(&self.events),
            screenshots: Arc::clone(&self.screenshots),
            diagnostics: Arc::clone(&self.diagnostics),
        }
    }
}

impl<W, P, A, E, S> AppState<W, P, A, E, S> {
    pub fn new(
        workspaces: Arc<W>,
        previews: Arc<P>,
        preview_adapter: Arc<A>,
        events: Arc<E>,
        screenshots: Arc<S>,
        diagnostics: Arc<dyn PreviewDiagnosticsSink>,
    ) -> Self {
        Self {
            workspaces,
            previews,
            preview_adapter,
            events,
            screenshots,
            diagnostics,
        }
    }
}

pub fn router<W, P, A, E, S>(state: AppState<W, P, A, E, S>) -> Router
where
    W: WorkspaceRepository + 'static,
    P: PreviewTransactionRepository + PreviewScreenshotRepository + 'static,
    A: PreviewAdapter + 'static,
    E: EventPublisher + 'static,
    S: ScreenshotStore + 'static,
{
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/openapi.json", get(openapi))
        .route("/app", get(web_index))
        .route("/app/", get(web_index))
        .route("/app/{*path}", get(web_asset))
        .route("/api/v1/workspaces", get(workspaces::<W, P, A, E, S>))
        .route(
            "/api/v1/workspaces/{id}/state",
            get(workspace_state::<W, P, A, E, S>),
        )
        .route(
            "/api/v1/workspaces/{id}/preview/open",
            post(open_preview::<W, P, A, E, S>),
        )
        .route(
            "/api/v1/workspaces/{id}/preview/screenshot",
            post(capture_screenshot::<W, P, A, E, S>).get(get_screenshot::<W, P, A, E, S>),
        )
        .route(
            "/api/v1/workspaces/{id}/preview/diagnostics",
            get(get_diagnostics::<W, P, A, E, S>),
        )
        .with_state(state)
}

pub fn empty_router() -> Router {
    router(AppState::new(
        Arc::new(EmptyWorkspaceRepository),
        Arc::new(EmptyPreviewRepository),
        Arc::new(UnavailablePreviewAdapter),
        Arc::new(herdr_workbench_app_core::EventBus::new(16)),
        Arc::new(EmptyScreenshotStore),
        Arc::new(herdr_workbench_app_core::InMemoryPreviewDiagnostics::default()),
    ))
}

async fn health() -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::OK, Json(serde_json::json!({"status": "ready"})))
}

async fn openapi() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}

async fn web_index() -> Response {
    web_asset(Path(String::from("index.html"))).await
}

async fn web_asset(Path(path): Path<String>) -> Response {
    let path = path.trim_start_matches('/');
    let asset = WebAssets::get(path).or_else(|| WebAssets::get("index.html"));
    let Some(asset) = asset else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let content_type = match path.rsplit('.').next().unwrap_or_default() {
        "css" => "text/css; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "html" => "text/html; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    };

    let mut response = Response::new(Body::from(asset.data.into_owned()));
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
}

async fn workspaces<W, P, A, E, S>(
    State(state): State<AppState<W, P, A, E, S>>,
) -> Result<Json<WorkspaceListResponse>, ApiError>
where
    W: WorkspaceRepository + 'static,
    P: PreviewTransactionRepository + PreviewScreenshotRepository + 'static,
    A: PreviewAdapter + 'static,
    E: EventPublisher + 'static,
    S: ScreenshotStore + 'static,
{
    let items = state
        .workspaces
        .list()
        .await
        .map_err(ApiError::repository)?;
    Ok(Json(WorkspaceListResponse {
        workspaces: items.into_iter().map(WorkspaceDto::from).collect(),
    }))
}

async fn workspace_state<W, P, A, E, S>(
    Path(id): Path<String>,
    State(state): State<AppState<W, P, A, E, S>>,
) -> Result<Json<PreviewStateResponse>, ApiError>
where
    W: WorkspaceRepository + 'static,
    P: PreviewTransactionRepository + PreviewScreenshotRepository + 'static,
    A: PreviewAdapter + 'static,
    E: EventPublisher + 'static,
    S: ScreenshotStore + 'static,
{
    let workspace_id = parse_workspace_id(&id)?;
    let workspace = state
        .workspaces
        .find_by_id(&workspace_id)
        .await
        .map_err(ApiError::repository)?
        .ok_or_else(|| ApiError::workspace_not_found(id.clone()))?;
    let preview = state
        .previews
        .find_by_workspace(&workspace_id)
        .await
        .map_err(ApiError::repository)?;

    Ok(Json(PreviewStateResponse::from_parts(workspace, preview)))
}

async fn open_preview<W, P, A, E, S>(
    Path(id): Path<String>,
    State(state): State<AppState<W, P, A, E, S>>,
    Json(request): Json<PreviewOpenRequest>,
) -> Result<Json<PreviewStateResponse>, ApiError>
where
    W: WorkspaceRepository + 'static,
    P: PreviewTransactionRepository + PreviewScreenshotRepository + 'static,
    A: PreviewAdapter + 'static,
    E: EventPublisher + 'static,
    S: ScreenshotStore + 'static,
{
    let workspace_id = parse_workspace_id(&id)?;
    let workspace = state
        .workspaces
        .find_by_id(&workspace_id)
        .await
        .map_err(ApiError::repository)?
        .ok_or_else(|| ApiError::workspace_not_found(id.clone()))?;
    let session = OpenPreview::new(
        state.previews.as_ref(),
        state.preview_adapter.as_ref(),
        state.events.as_ref(),
    )
    .execute(&workspace, request.url)
    .await
    .map_err(ApiError::application)?;

    Ok(Json(PreviewStateResponse::from_parts(
        workspace,
        Some(session),
    )))
}

async fn capture_screenshot<W, P, A, E, S>(
    Path(id): Path<String>,
    State(state): State<AppState<W, P, A, E, S>>,
) -> Result<Json<PreviewScreenshotResponse>, ApiError>
where
    W: WorkspaceRepository + 'static,
    P: PreviewTransactionRepository + PreviewScreenshotRepository + 'static,
    A: PreviewAdapter + 'static,
    E: EventPublisher + 'static,
    S: ScreenshotStore + 'static,
{
    let workspace_id = parse_workspace_id(&id)?;
    let workspace = state
        .workspaces
        .find_by_id(&workspace_id)
        .await
        .map_err(ApiError::repository)?
        .ok_or_else(|| ApiError::workspace_not_found(id.clone()))?;
    let screenshot = CapturePreview::new(
        state.previews.as_ref(),
        state.preview_adapter.as_ref(),
        state.events.as_ref(),
        state.screenshots.as_ref(),
    )
    .execute(&workspace)
    .await
    .map_err(ApiError::application)?;
    Ok(Json(PreviewScreenshotResponse::from(screenshot)))
}

async fn get_screenshot<W, P, A, E, S>(
    Path(id): Path<String>,
    State(state): State<AppState<W, P, A, E, S>>,
) -> Result<Response, ApiError>
where
    W: WorkspaceRepository + 'static,
    P: PreviewTransactionRepository + PreviewScreenshotRepository + 'static,
    A: PreviewAdapter + 'static,
    E: EventPublisher + 'static,
    S: ScreenshotStore + 'static,
{
    let workspace_id = parse_workspace_id(&id)?;
    let screenshot = state
        .previews
        .find_latest(&workspace_id)
        .await
        .map_err(ApiError::repository)?
        .ok_or(ApiError::screenshot_not_found())?;
    let png = state
        .screenshots
        .load(&screenshot.path)
        .await
        .map_err(ApiError::repository)?;
    let mut response = Response::new(Body::from(png));
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("image/png"));
    Ok(response)
}

async fn get_diagnostics<W, P, A, E, S>(
    Path(id): Path<String>,
    State(state): State<AppState<W, P, A, E, S>>,
) -> Result<Json<PreviewDiagnosticsResponse>, ApiError>
where
    W: WorkspaceRepository + 'static,
    P: PreviewTransactionRepository + PreviewScreenshotRepository + 'static,
    A: PreviewAdapter + 'static,
    E: EventPublisher + 'static,
    S: ScreenshotStore + 'static,
{
    let workspace_id = parse_workspace_id(&id)?;
    let workspace = state
        .workspaces
        .find_by_id(&workspace_id)
        .await
        .map_err(ApiError::repository)?
        .ok_or_else(|| ApiError::workspace_not_found(id.clone()))?;
    let _ = workspace;
    let diagnostics = state.diagnostics.list(&workspace_id).await;
    Ok(Json(PreviewDiagnosticsResponse {
        diagnostics: diagnostics
            .into_iter()
            .map(PreviewDiagnosticDto::from)
            .collect(),
    }))
}

fn parse_workspace_id(id: &str) -> Result<herdr_workbench_domain::WorkbenchWorkspaceId, ApiError> {
    uuid::Uuid::parse_str(id)
        .map(herdr_workbench_domain::WorkbenchWorkspaceId::from_uuid)
        .map_err(|_| ApiError::workspace_not_found(id.to_owned()))
}

#[derive(Debug)]
pub struct EmptyWorkspaceRepository;

#[async_trait::async_trait]
impl WorkspaceRepository for EmptyWorkspaceRepository {
    async fn find_by_herdr_id(
        &self,
        _: &herdr_workbench_domain::HerdrWorkspaceId,
    ) -> Result<Option<herdr_workbench_domain::Workspace>, herdr_workbench_app_core::RepositoryError>
    {
        Ok(None)
    }

    async fn find_by_id(
        &self,
        _: &herdr_workbench_domain::WorkbenchWorkspaceId,
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
        _: herdr_workbench_domain::Workspace,
    ) -> Result<(), herdr_workbench_app_core::RepositoryError> {
        Ok(())
    }
}

#[derive(Debug)]
pub struct EmptyPreviewRepository;

#[async_trait::async_trait]
impl PreviewTransactionRepository for EmptyPreviewRepository {
    async fn find_by_workspace(
        &self,
        _: &herdr_workbench_domain::WorkbenchWorkspaceId,
    ) -> Result<
        Option<herdr_workbench_domain::PreviewSession>,
        herdr_workbench_app_core::RepositoryError,
    > {
        Ok(None)
    }

    async fn commit_preview_open(
        &self,
        _: &herdr_workbench_domain::WorkbenchWorkspaceId,
        _: herdr_workbench_domain::PreviewSession,
    ) -> Result<
        herdr_workbench_app_core::DurablePreviewCommit,
        herdr_workbench_app_core::RepositoryError,
    > {
        Err(herdr_workbench_app_core::RepositoryError::new(
            "preview repository unavailable",
        ))
    }
}

#[async_trait::async_trait]
impl PreviewScreenshotRepository for EmptyPreviewRepository {
    async fn find_latest(
        &self,
        _: &herdr_workbench_domain::WorkbenchWorkspaceId,
    ) -> Result<
        Option<herdr_workbench_domain::PreviewScreenshot>,
        herdr_workbench_app_core::RepositoryError,
    > {
        Ok(None)
    }

    async fn commit_screenshot(
        &self,
        _: herdr_workbench_domain::PreviewScreenshot,
    ) -> Result<
        herdr_workbench_app_core::DurableScreenshotCommit,
        herdr_workbench_app_core::RepositoryError,
    > {
        Err(herdr_workbench_app_core::RepositoryError::new(
            "preview repository unavailable",
        ))
    }
}

#[derive(Debug)]
pub struct EmptyScreenshotStore;

#[async_trait::async_trait]
impl ScreenshotStore for EmptyScreenshotStore {
    async fn save(
        &self,
        _: &herdr_workbench_domain::WorkbenchWorkspaceId,
        _: &[u8],
    ) -> Result<String, herdr_workbench_app_core::RepositoryError> {
        Err(herdr_workbench_app_core::RepositoryError::new(
            "screenshot store unavailable",
        ))
    }

    async fn load(&self, _: &str) -> Result<Vec<u8>, herdr_workbench_app_core::RepositoryError> {
        Err(herdr_workbench_app_core::RepositoryError::new(
            "screenshot store unavailable",
        ))
    }
}

#[derive(Debug)]
pub struct UnavailablePreviewAdapter;

#[async_trait::async_trait]
impl PreviewAdapter for UnavailablePreviewAdapter {
    async fn open(
        &self,
        _: &herdr_workbench_domain::Workspace,
        _: Option<String>,
    ) -> Result<herdr_workbench_domain::PreviewSession, PreviewError> {
        Err(PreviewError::unavailable(
            "preview adapter is not configured",
        ))
    }

    async fn capture_screenshot(
        &self,
        _: &herdr_workbench_domain::Workspace,
    ) -> Result<herdr_workbench_app_core::CapturedPreviewImage, PreviewError> {
        Err(PreviewError::unavailable(
            "preview adapter is not configured",
        ))
    }
}

#[derive(Debug)]
pub struct ApiError {
    code: &'static str,
    message: String,
    status: StatusCode,
}

impl ApiError {
    fn workspace_not_found(_: String) -> Self {
        Self {
            code: "workspace_not_found",
            message: "workspace was not found".to_owned(),
            status: StatusCode::NOT_FOUND,
        }
    }

    fn screenshot_not_found() -> Self {
        Self {
            code: "screenshot_not_found",
            message: "preview screenshot was not found".to_owned(),
            status: StatusCode::NOT_FOUND,
        }
    }

    fn repository(error: herdr_workbench_app_core::RepositoryError) -> Self {
        Self {
            code: "internal_error",
            message: error.to_string(),
            status: StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn application(error: herdr_workbench_app_core::ApplicationError) -> Self {
        match error {
            herdr_workbench_app_core::ApplicationError::Preview(error) => Self {
                code: "preview_unavailable",
                message: error.to_string(),
                status: StatusCode::SERVICE_UNAVAILABLE,
            },
            herdr_workbench_app_core::ApplicationError::Repository(error) => {
                Self::repository(error)
            }
            herdr_workbench_app_core::ApplicationError::Domain(error) => Self {
                code: "invalid_request",
                message: error.to_string(),
                status: StatusCode::BAD_REQUEST,
            },
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({"error": {"code": self.code, "message": self.message, "request_id": null}})),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::{body::Body, http::Request};
    use herdr_workbench_app_core::{
        BindWorkspace, EventBus, InMemoryPreviewDiagnostics, InMemoryPreviewRepository,
        InMemoryScreenshotStore, InMemoryWorkspaceRepository, PreviewDiagnosticsSink,
    };
    use herdr_workbench_domain::{HerdrWorkspaceContext, PreviewSession};
    use tower::ServiceExt;

    struct FakePreviewAdapter;

    #[async_trait]
    impl PreviewAdapter for FakePreviewAdapter {
        async fn open(
            &self,
            workspace: &herdr_workbench_domain::Workspace,
            url: Option<String>,
        ) -> Result<PreviewSession, PreviewError> {
            Ok(PreviewSession::opening(workspace, url).mark_open())
        }

        async fn capture_screenshot(
            &self,
            _: &herdr_workbench_domain::Workspace,
        ) -> Result<herdr_workbench_app_core::CapturedPreviewImage, PreviewError> {
            Ok(herdr_workbench_app_core::CapturedPreviewImage {
                png: vec![137, 80, 78, 71, 13, 10, 26, 10],
            })
        }
    }

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

    #[tokio::test]
    async fn app_route_serves_the_embedded_react_page() {
        let response = empty_router()
            .oneshot(Request::builder().uri("/app/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn preview_open_and_state_routes_use_the_application_core() {
        let workspaces = Arc::new(InMemoryWorkspaceRepository::default());
        let workspace = BindWorkspace::new(workspaces.as_ref())
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
        let state = AppState::new(
            workspaces,
            Arc::new(InMemoryPreviewRepository::default()),
            Arc::new(FakePreviewAdapter),
            Arc::new(EventBus::new(8)),
            Arc::new(InMemoryScreenshotStore::default()),
            Arc::new(InMemoryPreviewDiagnostics::default()),
        );
        let app = router(state);
        let open_request = Request::builder()
            .method("POST")
            .uri(format!(
                "/api/v1/workspaces/{}/preview/open",
                workspace.workspace_id.as_uuid()
            ))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"url":"http://localhost:3000"}"#))
            .unwrap();
        let open_response = app.clone().oneshot(open_request).await.unwrap();
        assert_eq!(open_response.status(), StatusCode::OK);
        let state_response = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/workspaces/{}/state",
                        workspace.workspace_id.as_uuid()
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(state_response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(state_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["preview_url"], "http://localhost:3000");
    }

    #[tokio::test]
    async fn preview_screenshot_routes_capture_and_return_png() {
        let workspaces = Arc::new(InMemoryWorkspaceRepository::default());
        let workspace = BindWorkspace::new(workspaces.as_ref())
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
        let state = AppState::new(
            workspaces,
            Arc::new(InMemoryPreviewRepository::default()),
            Arc::new(FakePreviewAdapter),
            Arc::new(EventBus::new(8)),
            Arc::new(InMemoryScreenshotStore::default()),
            Arc::new(InMemoryPreviewDiagnostics::default()),
        );
        let app = router(state);
        let capture_request = Request::builder()
            .method("POST")
            .uri(format!(
                "/api/v1/workspaces/{}/preview/screenshot",
                workspace.workspace_id.as_uuid()
            ))
            .body(Body::empty())
            .unwrap();
        let capture_response = app.clone().oneshot(capture_request).await.unwrap();
        assert_eq!(capture_response.status(), StatusCode::OK);
        let screenshot_response = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/workspaces/{}/preview/screenshot",
                        workspace.workspace_id.as_uuid()
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(screenshot_response.status(), StatusCode::OK);
        assert_eq!(
            screenshot_response.headers().get(CONTENT_TYPE).unwrap(),
            "image/png"
        );
        let body = axum::body::to_bytes(screenshot_response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(body.as_ref(), [137, 80, 78, 71, 13, 10, 26, 10]);
    }

    #[tokio::test]
    async fn preview_diagnostics_route_returns_recorded_events() {
        let workspaces = Arc::new(InMemoryWorkspaceRepository::default());
        let workspace = BindWorkspace::new(workspaces.as_ref())
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
        let diagnostics = Arc::new(InMemoryPreviewDiagnostics::default());
        diagnostics
            .record(herdr_workbench_domain::PreviewDiagnostic {
                workspace_id: workspace.workspace_id.clone(),
                kind: herdr_workbench_domain::PreviewDiagnosticKind::Console,
                level: herdr_workbench_domain::PreviewDiagnosticLevel::Error,
                message: "boom".into(),
                source: Some("http://localhost:3000/app.js".into()),
                status: None,
                occurred_at: chrono::Utc::now(),
            })
            .await;
        let state = AppState::new(
            workspaces,
            Arc::new(InMemoryPreviewRepository::default()),
            Arc::new(FakePreviewAdapter),
            Arc::new(EventBus::new(8)),
            Arc::new(InMemoryScreenshotStore::default()),
            diagnostics,
        );
        let app = router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/workspaces/{}/preview/diagnostics",
                        workspace.workspace_id.as_uuid()
                    ))
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
        assert_eq!(json["diagnostics"][0]["kind"], "console");
        assert_eq!(json["diagnostics"][0]["level"], "error");
        assert_eq!(json["diagnostics"][0]["message"], "boom");
    }
}
