use std::path::{Component, Path, PathBuf};

pub mod events;
pub mod preview;

pub use events::{AppEvent, EventPayload, EventType, PreviewOpened, PreviewScreenshotCaptured};
pub use preview::{
    PreviewDiagnostic, PreviewDiagnosticKind, PreviewDiagnosticLevel, PreviewScreenshot,
    PreviewSession, PreviewSessionId, PreviewStatus,
};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HerdrWorkspaceId(String);

impl HerdrWorkspaceId {
    pub fn parse(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(DomainError::EmptyHerdrWorkspaceId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkbenchWorkspaceId(Uuid);

impl WorkbenchWorkspaceId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    pub fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for WorkbenchWorkspaceId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HerdrWorkspaceContext {
    pub herdr_workspace_id: HerdrWorkspaceId,
    pub label: String,
    pub cwd: PathBuf,
}

impl HerdrWorkspaceContext {
    pub fn new(
        herdr_workspace_id: impl Into<String>,
        label: impl Into<String>,
        cwd: PathBuf,
    ) -> Result<Self, DomainError> {
        validate_workspace_root(&cwd)?;
        Ok(Self {
            herdr_workspace_id: HerdrWorkspaceId::parse(herdr_workspace_id)?,
            label: label.into(),
            cwd,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub workspace_id: WorkbenchWorkspaceId,
    pub herdr_workspace_id: HerdrWorkspaceId,
    pub label: String,
    pub cwd: PathBuf,
    pub revision: u64,
}

impl Workspace {
    pub fn bind(context: HerdrWorkspaceContext) -> Self {
        Self {
            workspace_id: WorkbenchWorkspaceId::new(),
            herdr_workspace_id: context.herdr_workspace_id,
            label: context.label,
            cwd: context.cwd,
            revision: 0,
        }
    }
}

fn validate_workspace_root(path: &Path) -> Result<(), DomainError> {
    if !path.is_absolute() {
        return Err(DomainError::WorkspaceRootMustBeAbsolute);
    }
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(DomainError::WorkspaceRootContainsTraversal);
    }
    let text = path.to_string_lossy();
    if text.starts_with(r"\\") {
        return Err(DomainError::UncWorkspaceRootNotAllowed);
    }
    Ok(())
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DomainError {
    #[error("Herdr workspace id cannot be empty")]
    EmptyHerdrWorkspaceId,
    #[error("workspace root must be absolute")]
    WorkspaceRootMustBeAbsolute,
    #[error("workspace root cannot contain parent traversal")]
    WorkspaceRootContainsTraversal,
    #[error("UNC workspace roots are not allowed")]
    UncWorkspaceRootNotAllowed,
}
