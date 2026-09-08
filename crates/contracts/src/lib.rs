use serde::{Deserialize, Serialize};
use utoipa::{OpenApi, ToSchema};

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct HealthResponse {
    pub status: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct WorkspaceDto {
    pub workspace_id: String,
    pub herdr_workspace_id: String,
    pub label: String,
    pub cwd: String,
    pub revision: u64,
}

impl From<herdr_workbench_domain::Workspace> for WorkspaceDto {
    fn from(workspace: herdr_workbench_domain::Workspace) -> Self {
        Self {
            workspace_id: workspace.workspace_id.as_uuid().to_string(),
            herdr_workspace_id: workspace.herdr_workspace_id.as_str().to_owned(),
            label: workspace.label,
            cwd: workspace.cwd.to_string_lossy().into_owned(),
            revision: workspace.revision,
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct WorkspaceListResponse {
    pub workspaces: Vec<WorkspaceDto>,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct PreviewOpenRequest {
    pub url: Option<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PreviewStateResponse {
    pub workspace: WorkspaceDto,
    pub preview_session_id: Option<String>,
    pub preview_url: Option<String>,
    pub preview_status: Option<String>,
    pub preview_title: Option<String>,
}

impl PreviewStateResponse {
    pub fn from_parts(
        workspace: herdr_workbench_domain::Workspace,
        preview: Option<herdr_workbench_domain::PreviewSession>,
    ) -> Self {
        Self {
            workspace: WorkspaceDto::from(workspace),
            preview_session_id: preview.as_ref().map(|p| p.session_id.as_uuid().to_string()),
            preview_url: preview.as_ref().and_then(|p| p.url.clone()),
            preview_status: preview.as_ref().map(|p| format!("{:?}", p.status)),
            preview_title: preview.and_then(|p| p.title),
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PreviewScreenshotResponse {
    pub screenshot_id: String,
    pub workspace_id: String,
    pub path: String,
    pub sha256: String,
    pub byte_size: u64,
    pub revision: u64,
}

impl From<herdr_workbench_domain::PreviewScreenshot> for PreviewScreenshotResponse {
    fn from(screenshot: herdr_workbench_domain::PreviewScreenshot) -> Self {
        Self {
            screenshot_id: screenshot.screenshot_id.to_string(),
            workspace_id: screenshot.workspace_id.as_uuid().to_string(),
            path: screenshot.path,
            sha256: screenshot.sha256,
            byte_size: screenshot.byte_size,
            revision: screenshot.revision,
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PreviewDiagnosticDto {
    pub kind: String,
    pub level: String,
    pub message: String,
    pub source: Option<String>,
    pub status: Option<u16>,
    pub occurred_at: String,
}

impl From<herdr_workbench_domain::PreviewDiagnostic> for PreviewDiagnosticDto {
    fn from(diagnostic: herdr_workbench_domain::PreviewDiagnostic) -> Self {
        Self {
            kind: match diagnostic.kind {
                herdr_workbench_domain::PreviewDiagnosticKind::Console => "console".into(),
                herdr_workbench_domain::PreviewDiagnosticKind::Exception => "exception".into(),
                herdr_workbench_domain::PreviewDiagnosticKind::Network => "network".into(),
            },
            level: match diagnostic.level {
                herdr_workbench_domain::PreviewDiagnosticLevel::Warning => "warning".into(),
                herdr_workbench_domain::PreviewDiagnosticLevel::Error => "error".into(),
            },
            message: diagnostic.message,
            source: diagnostic.source,
            status: diagnostic.status,
            occurred_at: diagnostic.occurred_at.to_rfc3339(),
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PreviewDiagnosticsResponse {
    pub diagnostics: Vec<PreviewDiagnosticDto>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ErrorResponse {
    pub error: ErrorBody,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    pub request_id: Option<String>,
}

#[derive(OpenApi)]
#[openapi(
    paths(health, list_workspaces, get_workspace_state, open_preview, capture_preview_screenshot, get_preview_screenshot, get_preview_diagnostics),
    components(schemas(
        HealthResponse, WorkspaceDto, WorkspaceListResponse, PreviewOpenRequest,
        PreviewStateResponse, PreviewScreenshotResponse, PreviewDiagnosticDto, PreviewDiagnosticsResponse, ErrorResponse, ErrorBody
    )),
    tags((name = "system", description = "Workbench system endpoints"))
)]
pub struct ApiDoc;

#[utoipa::path(get, path = "/api/v1/health", tag = "system", responses((status = 200, body = HealthResponse)))]
pub fn health() {}

#[utoipa::path(get, path = "/api/v1/workspaces", responses((status = 200, body = WorkspaceListResponse)))]
pub fn list_workspaces() {}

#[utoipa::path(
    get,
    path = "/api/v1/workspaces/{id}/state",
    params(("id" = String, Path, description = "Workbench workspace ID")),
    responses((status = 200, body = PreviewStateResponse), (status = 404, body = ErrorResponse))
)]
pub fn get_workspace_state() {}

#[utoipa::path(
    post,
    path = "/api/v1/workspaces/{id}/preview/open",
    params(("id" = String, Path, description = "Workbench workspace ID")),
    request_body = PreviewOpenRequest,
    responses((status = 200, body = PreviewStateResponse), (status = 404, body = ErrorResponse), (status = 503, body = ErrorResponse))
)]
pub fn open_preview() {}

#[utoipa::path(
    post,
    path = "/api/v1/workspaces/{id}/preview/screenshot",
    params(("id" = String, Path, description = "Workbench workspace ID")),
    responses((status = 200, body = PreviewScreenshotResponse), (status = 404, body = ErrorResponse), (status = 503, body = ErrorResponse))
)]
pub fn capture_preview_screenshot() {}

#[utoipa::path(
    get,
    path = "/api/v1/workspaces/{id}/preview/screenshot",
    params(("id" = String, Path, description = "Workbench workspace ID")),
    responses((status = 200), (status = 404, body = ErrorResponse))
)]
pub fn get_preview_screenshot() {}

#[utoipa::path(
    get,
    path = "/api/v1/workspaces/{id}/preview/diagnostics",
    params(("id" = String, Path, description = "Workbench workspace ID")),
    responses((status = 200, body = PreviewDiagnosticsResponse), (status = 404, body = ErrorResponse))
)]
pub fn get_preview_diagnostics() {}
