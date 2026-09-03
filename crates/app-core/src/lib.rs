use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use herdr_workbench_domain::{
    AppEvent, DomainError, HerdrWorkspaceId, PreviewSession, WorkbenchWorkspaceId, Workspace,
};
use thiserror::Error;
use tokio::sync::{RwLock, broadcast};

pub use herdr_workbench_domain::HerdrWorkspaceContext;

#[async_trait]
pub trait WorkspaceRepository: Send + Sync {
    async fn find_by_herdr_id(
        &self,
        id: &HerdrWorkspaceId,
    ) -> Result<Option<Workspace>, RepositoryError>;

    async fn find_by_id(
        &self,
        id: &WorkbenchWorkspaceId,
    ) -> Result<Option<Workspace>, RepositoryError>;

    async fn list(&self) -> Result<Vec<Workspace>, RepositoryError>;

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
pub trait PreviewAdapter: Send + Sync {
    async fn open(
        &self,
        workspace: &Workspace,
        url: Option<String>,
    ) -> Result<PreviewSession, PreviewError>;
}

#[async_trait]
pub trait PreviewTransactionRepository: Send + Sync {
    async fn find_by_workspace(
        &self,
        workspace_id: &WorkbenchWorkspaceId,
    ) -> Result<Option<PreviewSession>, RepositoryError>;

    async fn commit_preview_open(
        &self,
        workspace_id: &WorkbenchWorkspaceId,
        session: PreviewSession,
    ) -> Result<DurablePreviewCommit, RepositoryError>;
}

#[derive(Clone, Debug)]
pub struct DurablePreviewCommit {
    pub session: PreviewSession,
    pub event: AppEvent,
}

#[async_trait]
pub trait EventPublisher: Send + Sync {
    async fn publish(&self, event: AppEvent);
}

#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<AppEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.sender.subscribe()
    }
}

#[async_trait]
impl EventPublisher for EventBus {
    async fn publish(&self, event: AppEvent) {
        let _ = self.sender.send(event);
    }
}

pub struct OpenPreview<'a, R, P, E> {
    previews: &'a R,
    adapter: &'a P,
    events: &'a E,
}

impl<'a, R, P, E> OpenPreview<'a, R, P, E>
where
    R: PreviewTransactionRepository,
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
        let commit = self
            .previews
            .commit_preview_open(&workspace.workspace_id, session)
            .await?;
        self.events.publish(commit.event).await;
        Ok(commit.session)
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

    async fn find_by_id(
        &self,
        id: &WorkbenchWorkspaceId,
    ) -> Result<Option<Workspace>, RepositoryError> {
        Ok(self
            .workspaces
            .read()
            .await
            .values()
            .find(|workspace| &workspace.workspace_id == id)
            .cloned())
    }

    async fn list(&self) -> Result<Vec<Workspace>, RepositoryError> {
        Ok(self.workspaces.read().await.values().cloned().collect())
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
    sessions: Arc<RwLock<HashMap<WorkbenchWorkspaceId, PreviewSession>>>,
    revisions: Arc<RwLock<HashMap<WorkbenchWorkspaceId, u64>>>,
}

impl InMemoryPreviewRepository {
    pub async fn session_count(&self) -> usize {
        self.sessions.read().await.len()
    }

    pub async fn revision(&self, workspace_id: &WorkbenchWorkspaceId) -> u64 {
        *self.revisions.read().await.get(workspace_id).unwrap_or(&0)
    }
}

#[async_trait]
impl PreviewTransactionRepository for InMemoryPreviewRepository {
    async fn find_by_workspace(
        &self,
        workspace_id: &WorkbenchWorkspaceId,
    ) -> Result<Option<PreviewSession>, RepositoryError> {
        Ok(self.sessions.read().await.get(workspace_id).cloned())
    }

    async fn commit_preview_open(
        &self,
        workspace_id: &WorkbenchWorkspaceId,
        session: PreviewSession,
    ) -> Result<DurablePreviewCommit, RepositoryError> {
        let mut sessions = self.sessions.write().await;
        if let Some(existing) = sessions.get(workspace_id).cloned() {
            return Ok(DurablePreviewCommit {
                session: existing.clone(),
                event: AppEvent::preview_opened(
                    existing,
                    *self.revisions.read().await.get(workspace_id).unwrap_or(&0),
                ),
            });
        }

        let mut revisions = self.revisions.write().await;
        let revision = revisions.entry(workspace_id.clone()).or_insert(0);
        *revision += 1;
        sessions.insert(workspace_id.clone(), session.clone());
        Ok(DurablePreviewCommit {
            event: AppEvent::preview_opened(session.clone(), *revision),
            session,
        })
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
#[error("repository operation failed: {message}")]
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
        BindWorkspace, EventBus, HerdrWorkspaceContext, InMemoryPreviewRepository,
        InMemoryWorkspaceRepository, OpenPreview, PreviewAdapter, PreviewError,
    };
    use async_trait::async_trait;
    use herdr_workbench_domain::{EventPayload, EventType, PreviewSession};

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

    #[tokio::test]
    async fn opening_preview_commits_state_and_publishes_the_same_revision() {
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
        let events = EventBus::new(16);
        let mut receiver = events.subscribe();
        let use_case = OpenPreview::new(&previews, &FakePreviewAdapter, &events);

        let session = use_case
            .execute(&workspace, Some("http://localhost:3000".into()))
            .await
            .unwrap();
        let event = receiver.recv().await.unwrap();

        assert_eq!(session.workspace_id, workspace.workspace_id);
        assert_eq!(previews.session_count().await, 1);
        assert_eq!(previews.revision(&workspace.workspace_id).await, 1);
        assert_eq!(event.event_type, EventType::PreviewOpened);
        assert_eq!(event.revision, 1);
        assert!(matches!(event.payload, EventPayload::PreviewOpened(_)));
    }

    #[tokio::test]
    async fn opening_preview_again_is_idempotent_and_does_not_advance_revision() {
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
        let events = EventBus::new(16);
        let mut receiver = events.subscribe();
        let use_case = OpenPreview::new(&previews, &FakePreviewAdapter, &events);

        let first = use_case.execute(&workspace, None).await.unwrap();
        let second = use_case.execute(&workspace, None).await.unwrap();

        assert_eq!(first.session_id, second.session_id);
        assert_eq!(previews.revision(&workspace.workspace_id).await, 1);
        assert!(receiver.try_recv().is_ok());
        assert!(receiver.try_recv().is_err());
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
