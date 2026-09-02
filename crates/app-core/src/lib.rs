use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use herdr_workbench_domain::{DomainError, HerdrWorkspaceId, PreviewSession, Workspace};
use thiserror::Error;
use tokio::sync::RwLock;

pub use herdr_workbench_domain::HerdrWorkspaceContext;

#[async_trait]
pub trait WorkspaceRepository: Send + Sync {
    async fn find_by_herdr_id(
        &self,
        id: &HerdrWorkspaceId,
    ) -> Result<Option<Workspace>, RepositoryError>;

    async fn insert(&self, workspace: Workspace) -> Result<(), RepositoryError>;
}

pub struct BindWorkspace<'a, R> {
    repository: &'a R,
}

impl<'a, R> BindWorkspace<'a, R>
where
    R: WorkspaceRepository,
{
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }

    pub async fn execute(
        &self,
        context: HerdrWorkspaceContext,
    ) -> Result<Workspace, ApplicationError> {
        if let Some(existing) = self
            .repository
            .find_by_herdr_id(&context.herdr_workspace_id)
            .await?
        {
            return Ok(existing);
        }

        let workspace = Workspace::bind(context);
        self.repository.insert(workspace.clone()).await?;
        Ok(workspace)
    }
}

#[async_trait]
pub trait PreviewRepository: Send + Sync {
    async fn find_by_workspace(
        &self,
        workspace_id: &herdr_workbench_domain::WorkbenchWorkspaceId,
    ) -> Result<Option<PreviewSession>, RepositoryError>;

    async fn insert(&self, session: PreviewSession) -> Result<(), RepositoryError>;
}

#[async_trait]
pub trait PreviewAdapter: Send + Sync {
    async fn open(
        &self,
        workspace: &Workspace,
        url: Option<String>,
    ) -> Result<PreviewSession, PreviewError>;
}

#[async_trait]
pub trait EventPublisher: Send + Sync {
    async fn publish(&self, event: AppEvent);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppEvent {
    PreviewOpened(PreviewSession),
}

pub struct OpenPreview<'a, R, P, E> {
    previews: &'a R,
    adapter: &'a P,
    events: &'a E,
}

impl<'a, R, P, E> OpenPreview<'a, R, P, E>
where
    R: PreviewRepository,
    P: PreviewAdapter,
    E: EventPublisher,
{
    pub fn new(previews: &'a R, adapter: &'a P, events: &'a E) -> Self {
        Self {
            previews,
            adapter,
            events,
        }
    }

    pub async fn execute(
        &self,
        workspace: &Workspace,
        url: Option<String>,
    ) -> Result<PreviewSession, ApplicationError> {
        if let Some(existing) = self
            .previews
            .find_by_workspace(&workspace.workspace_id)
            .await?
        {
            return Ok(existing);
        }

        let session = self.adapter.open(workspace, url).await?;
        self.previews.insert(session.clone()).await?;
        self.events
            .publish(AppEvent::PreviewOpened(session.clone()))
            .await;
        Ok(session)
    }
}

#[derive(Clone, Default)]
pub struct InMemoryWorkspaceRepository {
    workspaces: Arc<RwLock<HashMap<HerdrWorkspaceId, Workspace>>>,
}

impl InMemoryWorkspaceRepository {
    pub async fn workspace_count(&self) -> usize {
        self.workspaces.read().await.len()
    }
}

#[async_trait]
impl WorkspaceRepository for InMemoryWorkspaceRepository {
    async fn find_by_herdr_id(
        &self,
        id: &HerdrWorkspaceId,
    ) -> Result<Option<Workspace>, RepositoryError> {
        Ok(self.workspaces.read().await.get(id).cloned())
    }

    async fn insert(&self, workspace: Workspace) -> Result<(), RepositoryError> {
        self.workspaces
            .write()
            .await
            .entry(workspace.herdr_workspace_id.clone())
            .or_insert(workspace);
        Ok(())
    }
}

#[derive(Clone, Default)]
pub struct InMemoryPreviewRepository {
    sessions: Arc<RwLock<HashMap<herdr_workbench_domain::WorkbenchWorkspaceId, PreviewSession>>>,
}

impl InMemoryPreviewRepository {
    pub async fn session_count(&self) -> usize {
        self.sessions.read().await.len()
    }
}

#[async_trait]
impl PreviewRepository for InMemoryPreviewRepository {
    async fn find_by_workspace(
        &self,
        workspace_id: &herdr_workbench_domain::WorkbenchWorkspaceId,
    ) -> Result<Option<PreviewSession>, RepositoryError> {
        Ok(self.sessions.read().await.get(workspace_id).cloned())
    }

    async fn insert(&self, session: PreviewSession) -> Result<(), RepositoryError> {
        self.sessions
            .write()
            .await
            .entry(session.workspace_id.clone())
            .or_insert(session);
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum ApplicationError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
    #[error(transparent)]
    Preview(#[from] PreviewError),
}

#[derive(Clone, Debug, Error)]
#[error("workspace repository failed: {message}")]
pub struct RepositoryError {
    message: String,
}

impl RepositoryError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("preview is unavailable: {message}")]
pub struct PreviewError {
    message: String,
}

impl PreviewError {
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        AppEvent, BindWorkspace, EventPublisher, HerdrWorkspaceContext, InMemoryPreviewRepository,
        InMemoryWorkspaceRepository, OpenPreview, PreviewAdapter, PreviewError,
    };
    use async_trait::async_trait;
    use herdr_workbench_domain::{PreviewSession, PreviewStatus};

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
    }

    #[derive(Default)]
    struct RecordingEventBus {
        events: tokio::sync::Mutex<Vec<AppEvent>>,
    }

    #[async_trait]
    impl EventPublisher for RecordingEventBus {
        async fn publish(&self, event: AppEvent) {
            self.events.lock().await.push(event);
        }
    }

    #[tokio::test]
    async fn opening_preview_persists_an_open_session_and_publishes_event() {
        let workspaces = InMemoryWorkspaceRepository::default();
        let workspace = BindWorkspace::new(&workspaces)
            .execute(
                HerdrWorkspaceContext::new(
                    "herdr-workspace-1",
                    "Siftmark",
                    PathBuf::from(r"C:\projects\siftmark"),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let previews = InMemoryPreviewRepository::default();
        let events = RecordingEventBus::default();
        let use_case = OpenPreview::new(&previews, &FakePreviewAdapter, &events);

        let session = use_case
            .execute(&workspace, Some("http://localhost:3000".into()))
            .await
            .unwrap();

        assert_eq!(session.status, PreviewStatus::Open);
        assert_eq!(session.workspace_id, workspace.workspace_id);
        assert_eq!(previews.session_count().await, 1);
        assert!(matches!(
            events.events.lock().await.as_slice(),
            [AppEvent::PreviewOpened(_)]
        ));
    }

    #[tokio::test]
    async fn opening_preview_again_reuses_the_existing_session() {
        let workspaces = InMemoryWorkspaceRepository::default();
        let workspace = BindWorkspace::new(&workspaces)
            .execute(
                HerdrWorkspaceContext::new(
                    "herdr-workspace-1",
                    "Siftmark",
                    PathBuf::from(r"C:\projects\siftmark"),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let previews = InMemoryPreviewRepository::default();
        let events = RecordingEventBus::default();
        let use_case = OpenPreview::new(&previews, &FakePreviewAdapter, &events);

        let first = use_case.execute(&workspace, None).await.unwrap();
        let second = use_case
            .execute(&workspace, Some("http://localhost:4000".into()))
            .await
            .unwrap();

        assert_eq!(first.session_id, second.session_id);
        assert_eq!(previews.session_count().await, 1);
        assert_eq!(events.events.lock().await.len(), 1);
    }

    #[tokio::test]
    async fn repeated_binding_reuses_the_existing_workbench_workspace() {
        let repository = InMemoryWorkspaceRepository::default();
        let use_case = BindWorkspace::new(&repository);
        let context = HerdrWorkspaceContext::new(
            "herdr-workspace-1",
            "Siftmark",
            PathBuf::from(r"C:\projects\siftmark"),
        )
        .unwrap();

        let first = use_case.execute(context.clone()).await.unwrap();
        let second = use_case.execute(context).await.unwrap();

        assert_eq!(first.workspace_id, second.workspace_id);
        assert_eq!(repository.workspace_count().await, 1);
    }

    #[tokio::test]
    async fn distinct_herdr_workspaces_remain_isolated_when_their_cwd_matches() {
        let repository = InMemoryWorkspaceRepository::default();
        let use_case = BindWorkspace::new(&repository);
        let cwd = PathBuf::from(r"C:\projects\shared");

        let first = use_case
            .execute(HerdrWorkspaceContext::new("herdr-a", "A", cwd.clone()).unwrap())
            .await
            .unwrap();
        let second = use_case
            .execute(HerdrWorkspaceContext::new("herdr-b", "B", cwd).unwrap())
            .await
            .unwrap();

        assert_ne!(first.workspace_id, second.workspace_id);
        assert_eq!(repository.workspace_count().await, 2);
    }
}
