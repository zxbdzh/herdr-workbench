use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};

use async_trait::async_trait;
use herdr_workbench_domain::{
    AppEvent, DomainError, HerdrWorkspaceId, PreviewDiagnostic, PreviewScreenshot, PreviewSession,
    PreviewStatus, WorkbenchWorkspaceId, Workspace,
};
use thiserror::Error;
use tokio::sync::{RwLock, broadcast};

pub use herdr_workbench_domain::HerdrWorkspaceContext;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HerdrWorkspaceInfo {
    pub workspace_id: String,
    pub label: String,
    pub worktree_checkout_path: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HerdrPaneInfo {
    pub workspace_id: String,
    pub cwd: Option<PathBuf>,
    pub focused: bool,
}

#[async_trait]
pub trait HerdrHost: Send + Sync {
    async fn list_workspaces(&self) -> Result<Vec<HerdrWorkspaceInfo>, HerdrHostError>;
    async fn list_panes(&self) -> Result<Vec<HerdrPaneInfo>, HerdrHostError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HerdrAgentInfo {
    pub workspace_id: String,
    pub pane_id: String,
    pub agent: String,
    pub status: String,
    pub focused: bool,
}

#[async_trait]
pub trait HerdrAgentBridge: Send + Sync {
    async fn list_agents(&self) -> Result<Vec<HerdrAgentInfo>, HerdrHostError>;
    async fn prompt_agent(&self, target: &str, text: &str) -> Result<(), HerdrHostError>;
    async fn read_agent(&self, target: &str) -> Result<String, HerdrHostError>;
    async fn send_agent_keys(&self, target: &str, keys: &[&str]) -> Result<(), HerdrHostError>;
}

#[derive(Debug, Default)]
pub struct UnavailableAgentBridge;

#[async_trait]
impl HerdrAgentBridge for UnavailableAgentBridge {
    async fn list_agents(&self) -> Result<Vec<HerdrAgentInfo>, HerdrHostError> {
        Ok(Vec::new())
    }

    async fn prompt_agent(&self, _: &str, _: &str) -> Result<(), HerdrHostError> {
        Err(HerdrHostError::unavailable(
            "Herdr agent bridge is not configured",
        ))
    }

    async fn read_agent(&self, _: &str) -> Result<String, HerdrHostError> {
        Ok(String::new())
    }

    async fn send_agent_keys(&self, _: &str, _: &[&str]) -> Result<(), HerdrHostError> {
        Err(HerdrHostError::unavailable(
            "Herdr agent bridge is not configured",
        ))
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("Herdr host is unavailable: {message}")]
pub struct HerdrHostError {
    message: String,
}

impl HerdrHostError {
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

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

pub struct SyncHerdrWorkspaces<'a, R, H> {
    repository: &'a R,
    host: &'a H,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HerdrWorkspaceSyncReport {
    pub bound: Vec<Workspace>,
    pub skipped: usize,
}

impl<'a, R, H> SyncHerdrWorkspaces<'a, R, H>
where
    R: WorkspaceRepository,
    H: HerdrHost,
{
    pub fn new(repository: &'a R, host: &'a H) -> Self {
        Self { repository, host }
    }

    pub async fn execute(&self) -> Result<HerdrWorkspaceSyncReport, ApplicationError> {
        let workspaces = self.host.list_workspaces().await?;
        let panes = self.host.list_panes().await?;
        let mut bound = Vec::new();
        let mut skipped = 0;

        for workspace in workspaces {
            match derive_workspace_context(&workspace, &panes) {
                Ok(context) => {
                    bound.push(BindWorkspace::new(self.repository).execute(context).await?);
                }
                Err(_) => skipped += 1,
            }
        }

        Ok(HerdrWorkspaceSyncReport { bound, skipped })
    }
}

fn derive_workspace_context(
    workspace: &HerdrWorkspaceInfo,
    panes: &[HerdrPaneInfo],
) -> Result<HerdrWorkspaceContext, DomainError> {
    let owned: Vec<&HerdrPaneInfo> = panes
        .iter()
        .filter(|pane| pane.workspace_id == workspace.workspace_id)
        .collect();
    if let Some(context) = owned.iter().find(|pane| pane.focused).and_then(|pane| {
        bindable_context(&workspace.workspace_id, &workspace.label, pane.cwd.clone())
    }) {
        return Ok(context);
    }
    if let Some(context) = owned.iter().find_map(|pane| {
        bindable_context(&workspace.workspace_id, &workspace.label, pane.cwd.clone())
    }) {
        return Ok(context);
    }
    bindable_context(
        &workspace.workspace_id,
        &workspace.label,
        workspace.worktree_checkout_path.clone(),
    )
    .ok_or(DomainError::WorkspaceRootMustBeAbsolute)
}

fn bindable_context(
    workspace_id: &str,
    label: &str,
    cwd: Option<PathBuf>,
) -> Option<HerdrWorkspaceContext> {
    cwd.and_then(|cwd| HerdrWorkspaceContext::new(workspace_id, label, cwd).ok())
}

pub const HERDR_RECONCILE_OK_INTERVAL: Duration = Duration::from_secs(30);
pub const HERDR_RECONCILE_BACKOFF_INTERVAL: Duration = Duration::from_secs(120);

#[async_trait]
pub trait ReconcileSleeper: Send + Sync {
    async fn sleep(&self, duration: Duration);
}

pub struct TokioReconcileSleeper;

#[async_trait]
impl ReconcileSleeper for TokioReconcileSleeper {
    async fn sleep(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

pub struct HerdrReconcileLoop<R, H, S> {
    repository: Arc<R>,
    host: H,
    sleeper: S,
}

impl<R, H, S> HerdrReconcileLoop<R, H, S>
where
    R: WorkspaceRepository,
    H: HerdrHost,
    S: ReconcileSleeper,
{
    pub fn new(repository: Arc<R>, host: H, sleeper: S) -> Self {
        Self {
            repository,
            host,
            sleeper,
        }
    }

    pub async fn step(&self) -> Duration {
        let interval = match SyncHerdrWorkspaces::new(self.repository.as_ref(), &self.host)
            .execute()
            .await
        {
            Ok(_) => HERDR_RECONCILE_OK_INTERVAL,
            Err(error) => {
                eprintln!("Herdr workspace reconcile skipped: {error}");
                HERDR_RECONCILE_BACKOFF_INTERVAL
            }
        };
        self.sleeper.sleep(interval).await;
        interval
    }

    pub async fn run(&self) {
        loop {
            self.step().await;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HerdrLifecycleEvent {
    WorkspaceCreated,
    WorkspaceClosed,
    PaneCreated,
    PaneUpdated,
}

impl HerdrLifecycleEvent {
    pub fn should_sync(self) -> bool {
        matches!(
            self,
            Self::WorkspaceCreated | Self::PaneCreated | Self::PaneUpdated
        )
    }
}

#[async_trait]
pub trait HerdrEventSource: Send + Sync {
    async fn next_event(&self) -> Result<HerdrLifecycleEvent, HerdrHostError>;
}

pub struct HerdrEventSyncLoop<R, H, E> {
    repository: Arc<R>,
    host: H,
    events: E,
}

impl<R, H, E> HerdrEventSyncLoop<R, H, E>
where
    R: WorkspaceRepository,
    H: HerdrHost,
    E: HerdrEventSource,
{
    pub fn new(repository: Arc<R>, host: H, events: E) -> Self {
        Self {
            repository,
            host,
            events,
        }
    }

    pub async fn step(&self) -> Result<bool, ApplicationError> {
        let event = self.events.next_event().await?;
        if !event.should_sync() {
            return Ok(false);
        }
        SyncHerdrWorkspaces::new(self.repository.as_ref(), &self.host)
            .execute()
            .await?;
        Ok(true)
    }

    pub async fn run(&self) {
        loop {
            if let Err(error) = self.step().await {
                eprintln!("Herdr event sync skipped: {error}");
                tokio::time::sleep(HERDR_RECONCILE_BACKOFF_INTERVAL).await;
            }
        }
    }
}

#[async_trait]
pub trait PreviewAdapter: Send + Sync {
    async fn open(
        &self,
        workspace: &Workspace,
        url: Option<String>,
    ) -> Result<PreviewSession, PreviewError>;

    async fn capture_screenshot(
        &self,
        workspace: &Workspace,
    ) -> Result<CapturedPreviewImage, PreviewError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedPreviewImage {
    pub png: Vec<u8>,
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

#[async_trait]
pub trait PreviewScreenshotRepository: Send + Sync {
    async fn find_latest(
        &self,
        workspace_id: &WorkbenchWorkspaceId,
    ) -> Result<Option<PreviewScreenshot>, RepositoryError>;

    async fn commit_screenshot(
        &self,
        screenshot: PreviewScreenshot,
    ) -> Result<DurableScreenshotCommit, RepositoryError>;
}

#[async_trait]
pub trait PreviewStateUpdater: Send + Sync {
    async fn update_preview_state(
        &self,
        workspace_id: &WorkbenchWorkspaceId,
        status: PreviewStatus,
        url: Option<String>,
        title: Option<String>,
    ) -> Result<PreviewSession, RepositoryError>;
}

#[async_trait]
pub trait EventPublisher: Send + Sync {
    async fn publish(&self, event: AppEvent);
}

pub const PREVIEW_DIAGNOSTIC_LIMIT: usize = 50;

#[async_trait]
pub trait PreviewDiagnosticsSink: Send + Sync {
    async fn record(&self, diagnostic: PreviewDiagnostic);
    async fn list(&self, workspace_id: &WorkbenchWorkspaceId) -> Vec<PreviewDiagnostic>;
    async fn clear(&self, workspace_id: &WorkbenchWorkspaceId);
}

#[derive(Clone, Default)]
pub struct InMemoryPreviewDiagnostics {
    entries: Arc<RwLock<HashMap<WorkbenchWorkspaceId, Vec<PreviewDiagnostic>>>>,
    events: Option<Arc<dyn EventPublisher>>,
}

impl InMemoryPreviewDiagnostics {
    pub fn with_publisher(events: Arc<dyn EventPublisher>) -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            events: Some(events),
        }
    }
}

#[async_trait]
impl PreviewDiagnosticsSink for InMemoryPreviewDiagnostics {
    async fn record(&self, diagnostic: PreviewDiagnostic) {
        let mut entries = self.entries.write().await;
        let buffer = entries.entry(diagnostic.workspace_id.clone()).or_default();
        buffer.push(diagnostic.clone());
        let overflow = buffer.len().saturating_sub(PREVIEW_DIAGNOSTIC_LIMIT);
        if overflow > 0 {
            buffer.drain(0..overflow);
        }
        drop(entries);
        if let Some(events) = &self.events {
            events
                .publish(AppEvent::preview_diagnostics_updated(diagnostic))
                .await;
        }
    }

    async fn list(&self, workspace_id: &WorkbenchWorkspaceId) -> Vec<PreviewDiagnostic> {
        self.entries
            .read()
            .await
            .get(workspace_id)
            .cloned()
            .unwrap_or_default()
    }

    async fn clear(&self, workspace_id: &WorkbenchWorkspaceId) {
        self.entries.write().await.remove(workspace_id);
    }
}

#[derive(Clone, Debug)]
pub struct DurablePreviewCommit {
    pub session: PreviewSession,
    pub event: AppEvent,
}

#[derive(Clone, Debug)]
pub struct DurableScreenshotCommit {
    pub screenshot: PreviewScreenshot,
    pub event: AppEvent,
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

#[async_trait]
impl EventPublisher for Arc<EventBus> {
    async fn publish(&self, event: AppEvent) {
        EventBus::publish(self, event).await;
    }
}

#[async_trait]
impl EventPublisher for &EventBus {
    async fn publish(&self, event: AppEvent) {
        EventBus::publish(*self, event).await;
    }
}

#[async_trait]
impl<T> PreviewStateUpdater for Arc<T>
where
    T: PreviewStateUpdater + Send + Sync,
{
    async fn update_preview_state(
        &self,
        workspace_id: &WorkbenchWorkspaceId,
        status: PreviewStatus,
        url: Option<String>,
        title: Option<String>,
    ) -> Result<PreviewSession, RepositoryError> {
        T::update_preview_state(self, workspace_id, status, url, title).await
    }
}

pub struct PublishingPreviewStateUpdater<R, E> {
    inner: R,
    events: E,
}

impl<R, E> PublishingPreviewStateUpdater<R, E> {
    pub fn new(inner: R, events: E) -> Self {
        Self { inner, events }
    }
}

#[async_trait]
impl<R, E> PreviewStateUpdater for PublishingPreviewStateUpdater<R, E>
where
    R: PreviewStateUpdater,
    E: EventPublisher,
{
    async fn update_preview_state(
        &self,
        workspace_id: &WorkbenchWorkspaceId,
        status: PreviewStatus,
        url: Option<String>,
        title: Option<String>,
    ) -> Result<PreviewSession, RepositoryError> {
        let session = self
            .inner
            .update_preview_state(workspace_id, status, url, title)
            .await?;
        self.events
            .publish(AppEvent::preview_state_updated(session.clone()))
            .await;
        Ok(session)
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
        let session = self.adapter.open(workspace, url).await?;
        if let Some(existing) = self
            .previews
            .find_by_workspace(&workspace.workspace_id)
            .await?
        {
            return Ok(existing);
        }

        let commit = self
            .previews
            .commit_preview_open(&workspace.workspace_id, session)
            .await?;
        self.events.publish(commit.event).await;
        Ok(commit.session)
    }
}

pub struct CapturePreview<'a, R, P, E, S> {
    screenshots: &'a R,
    adapter: &'a P,
    events: &'a E,
    store: &'a S,
}

impl<'a, R, P, E, S> CapturePreview<'a, R, P, E, S>
where
    R: PreviewScreenshotRepository,
    P: PreviewAdapter,
    E: EventPublisher,
    S: ScreenshotStore,
{
    pub fn new(screenshots: &'a R, adapter: &'a P, events: &'a E, store: &'a S) -> Self {
        Self {
            screenshots,
            adapter,
            events,
            store,
        }
    }

    pub async fn execute(
        &self,
        workspace: &Workspace,
    ) -> Result<PreviewScreenshot, ApplicationError> {
        let image = match self.adapter.capture_screenshot(workspace).await {
            Ok(image) => image,
            Err(_) => {
                self.adapter.open(workspace, None).await?;
                self.adapter.capture_screenshot(workspace).await?
            }
        };
        let path = self.store.save(&workspace.workspace_id, &image.png).await?;
        let screenshot = PreviewScreenshot {
            screenshot_id: uuid::Uuid::now_v7(),
            workspace_id: workspace.workspace_id.clone(),
            path,
            sha256: sha256_hex(&image.png),
            byte_size: image.png.len() as u64,
            revision: 0,
        };
        let commit = self.screenshots.commit_screenshot(screenshot).await?;
        self.events.publish(commit.event).await;
        Ok(commit.screenshot)
    }
}

pub const PREVIEW_CONTEXT_DIAGNOSTIC_LIMIT: usize = 20;
pub const PREVIEW_CONTEXT_NOTE_LIMIT: usize = 500;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewContextReceipt {
    pub pane_id: String,
    pub agent: String,
}

pub struct SendPreviewContext<'a, P, D: ?Sized, A: ?Sized> {
    previews: &'a P,
    diagnostics: &'a D,
    agents: &'a A,
}

impl<'a, P, D, A> SendPreviewContext<'a, P, D, A>
where
    P: PreviewTransactionRepository + PreviewScreenshotRepository,
    D: PreviewDiagnosticsSink + ?Sized,
    A: HerdrAgentBridge + ?Sized,
{
    pub fn new(previews: &'a P, diagnostics: &'a D, agents: &'a A) -> Self {
        Self {
            previews,
            diagnostics,
            agents,
        }
    }

    pub async fn execute(
        &self,
        workspace: &Workspace,
        note: Option<String>,
    ) -> Result<PreviewContextReceipt, ApplicationError> {
        let session = self
            .previews
            .find_by_workspace(&workspace.workspace_id)
            .await?;
        let screenshot = self.previews.find_latest(&workspace.workspace_id).await?;
        let diagnostics = self.diagnostics.list(&workspace.workspace_id).await;
        let agents = self.agents.list_agents().await?;
        let selected = select_agent_for_workspace(workspace.herdr_workspace_id.as_str(), &agents)
            .ok_or_else(|| {
            HerdrHostError::unavailable("no Herdr agent is running in this workspace")
        })?;
        let text = render_preview_context(
            workspace,
            session.as_ref(),
            screenshot.as_ref(),
            &diagnostics,
            note.as_deref(),
        );
        self.agents.prompt_agent(&selected.pane_id, &text).await?;
        Ok(PreviewContextReceipt {
            pane_id: selected.pane_id.clone(),
            agent: selected.agent.clone(),
        })
    }
}

pub const AGENT_TRANSCRIPT_LINES: usize = 80;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentDecision {
    Yes,
    No,
}

impl AgentDecision {
    pub fn parse(value: &str) -> Result<Self, ApplicationError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "yes" | "y" | "approve" => Ok(Self::Yes),
            "no" | "n" | "reject" => Ok(Self::No),
            _ => Err(DomainError::InvalidAgentDecision.into()),
        }
    }

    pub fn keys(self) -> &'static [&'static str] {
        match self {
            Self::Yes => &["y", "enter"],
            Self::No => &["n", "enter"],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceAgentSession {
    pub pane_id: String,
    pub agent: String,
    pub status: String,
    pub focused: bool,
    pub transcript: String,
}

pub fn agent_is_blocked(status: &str) -> bool {
    status.eq_ignore_ascii_case("blocked")
}

pub fn strip_agent_transcript(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let Some(index) = trimmed.find('\u{2500}') else {
        return trimmed.to_owned();
    };
    let stripped = trimmed[..index].trim_end();
    if stripped.is_empty() {
        trimmed.to_owned()
    } else {
        stripped.to_owned()
    }
}

fn workspace_agents<'a>(
    herdr_workspace_id: &str,
    agents: &'a [HerdrAgentInfo],
) -> Vec<&'a HerdrAgentInfo> {
    agents
        .iter()
        .filter(|agent| agent.workspace_id == herdr_workspace_id)
        .collect()
}

fn find_workspace_agent<'a>(
    herdr_workspace_id: &str,
    pane_id: &str,
    agents: &'a [HerdrAgentInfo],
) -> Result<&'a HerdrAgentInfo, ApplicationError> {
    workspace_agents(herdr_workspace_id, agents)
        .into_iter()
        .find(|agent| agent.pane_id == pane_id)
        .ok_or_else(|| {
            HerdrHostError::unavailable(
                "no Herdr agent with that pane is running in this workspace",
            )
            .into()
        })
}

pub struct ListWorkspaceAgents<'a, A: ?Sized> {
    agents: &'a A,
}

impl<'a, A> ListWorkspaceAgents<'a, A>
where
    A: HerdrAgentBridge + ?Sized,
{
    pub fn new(agents: &'a A) -> Self {
        Self { agents }
    }

    pub async fn execute(
        &self,
        workspace: &Workspace,
    ) -> Result<Vec<HerdrAgentInfo>, ApplicationError> {
        let agents = self.agents.list_agents().await?;
        Ok(
            workspace_agents(workspace.herdr_workspace_id.as_str(), &agents)
                .into_iter()
                .cloned()
                .collect(),
        )
    }
}

pub struct ReadWorkspaceAgent<'a, A: ?Sized> {
    agents: &'a A,
}

impl<'a, A> ReadWorkspaceAgent<'a, A>
where
    A: HerdrAgentBridge + ?Sized,
{
    pub fn new(agents: &'a A) -> Self {
        Self { agents }
    }

    pub async fn execute(
        &self,
        workspace: &Workspace,
        pane_id: &str,
    ) -> Result<WorkspaceAgentSession, ApplicationError> {
        let agents = self.agents.list_agents().await?;
        let selected =
            find_workspace_agent(workspace.herdr_workspace_id.as_str(), pane_id, &agents)?;
        let transcript = strip_agent_transcript(&self.agents.read_agent(&selected.pane_id).await?);
        Ok(WorkspaceAgentSession {
            pane_id: selected.pane_id.clone(),
            agent: selected.agent.clone(),
            status: selected.status.clone(),
            focused: selected.focused,
            transcript,
        })
    }
}

pub struct PromptWorkspaceAgent<'a, A: ?Sized> {
    agents: &'a A,
}

impl<'a, A> PromptWorkspaceAgent<'a, A>
where
    A: HerdrAgentBridge + ?Sized,
{
    pub fn new(agents: &'a A) -> Self {
        Self { agents }
    }

    pub async fn execute(
        &self,
        workspace: &Workspace,
        pane_id: &str,
        text: &str,
    ) -> Result<WorkspaceAgentSession, ApplicationError> {
        let agents = self.agents.list_agents().await?;
        let selected =
            find_workspace_agent(workspace.herdr_workspace_id.as_str(), pane_id, &agents)?;
        if agent_is_blocked(&selected.status) {
            return Err(DomainError::AgentBlockedForPrompt.into());
        }
        self.agents.prompt_agent(&selected.pane_id, text).await?;
        Ok(WorkspaceAgentSession {
            pane_id: selected.pane_id.clone(),
            agent: selected.agent.clone(),
            status: selected.status.clone(),
            focused: selected.focused,
            transcript: String::new(),
        })
    }
}

pub struct ApproveWorkspaceAgent<'a, A: ?Sized> {
    agents: &'a A,
}

impl<'a, A> ApproveWorkspaceAgent<'a, A>
where
    A: HerdrAgentBridge + ?Sized,
{
    pub fn new(agents: &'a A) -> Self {
        Self { agents }
    }

    pub async fn execute(
        &self,
        workspace: &Workspace,
        pane_id: &str,
        decision: AgentDecision,
    ) -> Result<WorkspaceAgentSession, ApplicationError> {
        let agents = self.agents.list_agents().await?;
        let selected =
            find_workspace_agent(workspace.herdr_workspace_id.as_str(), pane_id, &agents)?;
        if !agent_is_blocked(&selected.status) {
            return Err(DomainError::AgentNotBlockedForApproval.into());
        }
        self.agents
            .send_agent_keys(&selected.pane_id, decision.keys())
            .await?;
        Ok(WorkspaceAgentSession {
            pane_id: selected.pane_id.clone(),
            agent: selected.agent.clone(),
            status: selected.status.clone(),
            focused: selected.focused,
            transcript: String::new(),
        })
    }
}

pub fn select_agent_for_workspace<'a>(
    herdr_workspace_id: &str,
    agents: &'a [HerdrAgentInfo],
) -> Option<&'a HerdrAgentInfo> {
    let local: Vec<&HerdrAgentInfo> = agents
        .iter()
        .filter(|agent| agent.workspace_id == herdr_workspace_id)
        .collect();
    local
        .iter()
        .copied()
        .find(|agent| agent.focused)
        .or_else(|| {
            local
                .iter()
                .copied()
                .find(|agent| agent.status.eq_ignore_ascii_case("idle"))
        })
        .or_else(|| local.first().copied())
}

pub fn render_preview_context(
    workspace: &Workspace,
    session: Option<&PreviewSession>,
    screenshot: Option<&PreviewScreenshot>,
    diagnostics: &[PreviewDiagnostic],
    note: Option<&str>,
) -> String {
    let mut lines = vec![
        "[Herdr Workbench Preview feedback]".to_owned(),
        format!("workspace: {}", workspace.label),
        format!("cwd: {}", workspace.cwd.display()),
        format!(
            "url: {}",
            session.and_then(|item| item.url.as_deref()).unwrap_or("-")
        ),
        format!(
            "title: {}",
            session
                .and_then(|item| item.title.as_deref())
                .unwrap_or("-")
        ),
        format!(
            "status: {}",
            session
                .map(|item| format!("{:?}", item.status))
                .unwrap_or_else(|| "-".into())
        ),
    ];
    match screenshot {
        Some(shot) => lines.push(format!(
            "screenshot: sha256={} bytes={} path={}",
            shot.sha256, shot.byte_size, shot.path
        )),
        None => lines.push("screenshot: none".into()),
    }
    if let Some(note) = note.map(str::trim).filter(|value| !value.is_empty()) {
        let clipped: String = note.chars().take(PREVIEW_CONTEXT_NOTE_LIMIT).collect();
        lines.push(format!("note: {clipped}"));
    }
    lines.push("diagnostics:".into());
    if diagnostics.is_empty() {
        lines.push("- none".into());
    } else {
        for item in diagnostics.iter().take(PREVIEW_CONTEXT_DIAGNOSTIC_LIMIT) {
            lines.push(format!(
                "- {:?} {:?} {}",
                item.level, item.kind, item.message
            ));
        }
    }
    lines.join("\n")
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[async_trait]
pub trait ScreenshotStore: Send + Sync {
    async fn save(
        &self,
        workspace_id: &WorkbenchWorkspaceId,
        png: &[u8],
    ) -> Result<String, RepositoryError>;

    async fn load(&self, path: &str) -> Result<Vec<u8>, RepositoryError>;
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
    screenshots: Arc<RwLock<HashMap<WorkbenchWorkspaceId, PreviewScreenshot>>>,
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

#[async_trait]
impl PreviewStateUpdater for InMemoryPreviewRepository {
    async fn update_preview_state(
        &self,
        workspace_id: &WorkbenchWorkspaceId,
        status: PreviewStatus,
        url: Option<String>,
        title: Option<String>,
    ) -> Result<PreviewSession, RepositoryError> {
        let mut sessions = self.sessions.write().await;
        let session = sessions
            .get_mut(workspace_id)
            .ok_or_else(|| RepositoryError::new("preview session not found"))?;
        session.status = status;
        if let Some(url) = url {
            session.url = Some(url);
        }
        if let Some(title) = title {
            session.title = Some(title);
        }
        Ok(session.clone())
    }
}

#[async_trait]
impl PreviewScreenshotRepository for InMemoryPreviewRepository {
    async fn find_latest(
        &self,
        workspace_id: &WorkbenchWorkspaceId,
    ) -> Result<Option<PreviewScreenshot>, RepositoryError> {
        Ok(self.screenshots.read().await.get(workspace_id).cloned())
    }

    async fn commit_screenshot(
        &self,
        mut screenshot: PreviewScreenshot,
    ) -> Result<DurableScreenshotCommit, RepositoryError> {
        let mut revisions = self.revisions.write().await;
        let revision = revisions
            .entry(screenshot.workspace_id.clone())
            .or_insert(0);
        *revision += 1;
        screenshot.revision = *revision;
        self.screenshots
            .write()
            .await
            .insert(screenshot.workspace_id.clone(), screenshot.clone());
        Ok(DurableScreenshotCommit {
            event: AppEvent::preview_screenshot_captured(screenshot.clone(), screenshot.revision),
            screenshot,
        })
    }
}

#[derive(Clone, Default)]
pub struct InMemoryScreenshotStore {
    files: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

#[async_trait]
impl ScreenshotStore for InMemoryScreenshotStore {
    async fn save(
        &self,
        workspace_id: &WorkbenchWorkspaceId,
        png: &[u8],
    ) -> Result<String, RepositoryError> {
        let path = format!("memory://{}", workspace_id.as_uuid());
        self.files.write().await.insert(path.clone(), png.to_vec());
        Ok(path)
    }

    async fn load(&self, path: &str) -> Result<Vec<u8>, RepositoryError> {
        self.files
            .read()
            .await
            .get(path)
            .cloned()
            .ok_or_else(|| RepositoryError::new("screenshot file not found"))
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
    #[error(transparent)]
    Herdr(#[from] HerdrHostError),
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanStatus {
    pub enabled: bool,
    pub listen: String,
    pub urls: Vec<String>,
    pub pairing_code: String,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("LAN access denied: {message}")]
pub struct LanDenied {
    message: String,
}

impl LanDenied {
    pub fn pairing_required() -> Self {
        Self {
            message: "pairing code required".into(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct LanAccess {
    enabled: bool,
    pairing_code: Option<String>,
    urls: Vec<String>,
}

impl LanAccess {
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn pairing_code(&self) -> Option<&str> {
        self.pairing_code.as_deref()
    }

    pub fn status(&self) -> LanStatus {
        LanStatus {
            enabled: self.enabled,
            listen: if self.enabled {
                "0.0.0.0:17321".into()
            } else {
                "127.0.0.1:17321".into()
            },
            urls: self.urls.clone(),
            pairing_code: self.pairing_code.clone().unwrap_or_default(),
        }
    }

    pub fn enable(&mut self, addresses: impl IntoIterator<Item = String>) -> LanStatus {
        self.enabled = true;
        self.pairing_code = Some(generate_pairing_code());
        self.urls = addresses
            .into_iter()
            .map(|ip| format!("http://{ip}:17321/"))
            .collect();
        self.status()
    }

    pub fn disable(&mut self) {
        self.enabled = false;
        self.pairing_code = None;
        self.urls.clear();
    }

    pub fn authorize(&self, peer_ip: &str, pairing_code: Option<&str>) -> Result<(), LanDenied> {
        if is_loopback(peer_ip) {
            return Ok(());
        }
        if !self.enabled {
            return Err(LanDenied::pairing_required());
        }
        match (self.pairing_code.as_deref(), pairing_code) {
            (Some(expected), Some(actual)) if expected == actual => Ok(()),
            _ => Err(LanDenied::pairing_required()),
        }
    }
}

fn is_loopback(peer_ip: &str) -> bool {
    peer_ip == "127.0.0.1"
        || peer_ip == "::1"
        || peer_ip == "localhost"
        || peer_ip == "::ffff:127.0.0.1"
}

fn generate_pairing_code() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    format!("{:06}", rng.gen_range(0..1_000_000))
}

pub fn lan_ipv4_addresses() -> Vec<String> {
    use std::collections::BTreeSet;
    use std::net::{IpAddr, ToSocketAddrs, UdpSocket};

    let mut ips = BTreeSet::new();
    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0")
        && socket.connect("1.1.1.1:80").is_ok()
        && let Ok(addr) = socket.local_addr()
        && let IpAddr::V4(ip) = addr.ip()
        && is_advertisable_lan_ip(ip)
    {
        ips.insert(ip.to_string());
    }
    for host in [
        std::env::var("COMPUTERNAME").ok(),
        std::env::var("HOSTNAME").ok(),
    ]
    .into_iter()
    .flatten()
    {
        if let Ok(addrs) = (host.as_str(), 0u16).to_socket_addrs() {
            for addr in addrs {
                if let IpAddr::V4(ip) = addr.ip()
                    && is_advertisable_lan_ip(ip)
                {
                    ips.insert(ip.to_string());
                }
            }
        }
    }
    ips.into_iter().collect()
}

fn is_advertisable_lan_ip(ip: std::net::Ipv4Addr) -> bool {
    !ip.is_loopback() && !ip.is_unspecified() && !ip.is_link_local() && !ip.is_multicast()
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};

    use super::{
        AgentDecision, ApproveWorkspaceAgent, BindWorkspace, CapturePreview, CapturedPreviewImage,
        EventBus, HERDR_RECONCILE_BACKOFF_INTERVAL, HERDR_RECONCILE_OK_INTERVAL, HerdrAgentBridge,
        HerdrAgentInfo, HerdrEventSource, HerdrEventSyncLoop, HerdrHost, HerdrHostError,
        HerdrLifecycleEvent, HerdrPaneInfo, HerdrReconcileLoop, HerdrWorkspaceContext,
        HerdrWorkspaceInfo, InMemoryPreviewDiagnostics, InMemoryPreviewRepository,
        InMemoryScreenshotStore, InMemoryWorkspaceRepository, LanAccess, ListWorkspaceAgents,
        OpenPreview, PreviewAdapter, PreviewDiagnosticsSink, PreviewError,
        PreviewScreenshotRepository, PreviewStateUpdater, PromptWorkspaceAgent,
        PublishingPreviewStateUpdater, ReadWorkspaceAgent, ReconcileSleeper, ScreenshotStore,
        SendPreviewContext, SyncHerdrWorkspaces, WorkspaceRepository,
    };
    use async_trait::async_trait;
    use herdr_workbench_domain::{
        EventPayload, EventType, PreviewDiagnostic, PreviewDiagnosticKind, PreviewDiagnosticLevel,
        PreviewSession,
    };

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
        ) -> Result<CapturedPreviewImage, PreviewError> {
            Ok(CapturedPreviewImage {
                png: vec![137, 80, 78, 71, 13, 10, 26, 10],
            })
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

    struct CountingPreviewAdapter {
        opens: std::sync::Mutex<u32>,
        window_open: std::sync::Mutex<bool>,
    }

    impl Default for CountingPreviewAdapter {
        fn default() -> Self {
            Self {
                opens: std::sync::Mutex::new(0),
                window_open: std::sync::Mutex::new(false),
            }
        }
    }

    #[async_trait]
    impl PreviewAdapter for CountingPreviewAdapter {
        async fn open(
            &self,
            workspace: &herdr_workbench_domain::Workspace,
            url: Option<String>,
        ) -> Result<PreviewSession, PreviewError> {
            *self.opens.lock().unwrap() += 1;
            *self.window_open.lock().unwrap() = true;
            Ok(PreviewSession::opening(workspace, url).mark_open())
        }

        async fn capture_screenshot(
            &self,
            _: &herdr_workbench_domain::Workspace,
        ) -> Result<CapturedPreviewImage, PreviewError> {
            if !*self.window_open.lock().unwrap() {
                return Err(PreviewError::unavailable("preview window is not open"));
            }
            Ok(CapturedPreviewImage {
                png: vec![137, 80, 78, 71, 13, 10, 26, 10],
            })
        }
    }

    #[tokio::test]
    async fn opening_preview_again_still_asks_the_adapter_to_show_the_window() {
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
        let adapter = CountingPreviewAdapter::default();
        let use_case = OpenPreview::new(&previews, &adapter, &events);
        let first = use_case
            .execute(&workspace, Some("http://localhost:3000".into()))
            .await
            .unwrap();
        *adapter.window_open.lock().unwrap() = false;
        let second = use_case
            .execute(&workspace, Some("http://localhost:3000".into()))
            .await
            .unwrap();
        assert_eq!(first.session_id, second.session_id);
        assert_eq!(*adapter.opens.lock().unwrap(), 2);
        assert!(*adapter.window_open.lock().unwrap());
        assert_eq!(previews.revision(&workspace.workspace_id).await, 1);
    }

    #[tokio::test]
    async fn capturing_screenshot_reopens_the_preview_window_when_it_is_missing() {
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
        let store = InMemoryScreenshotStore::default();
        let events = EventBus::new(16);
        let adapter = CountingPreviewAdapter::default();
        OpenPreview::new(&previews, &adapter, &events)
            .execute(&workspace, Some("http://localhost:3000".into()))
            .await
            .unwrap();
        *adapter.window_open.lock().unwrap() = false;
        let screenshot = CapturePreview::new(&previews, &adapter, &events, &store)
            .execute(&workspace)
            .await
            .unwrap();
        assert_eq!(screenshot.byte_size, 8);
        assert_eq!(*adapter.opens.lock().unwrap(), 2);
        assert!(*adapter.window_open.lock().unwrap());
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

    #[tokio::test]
    async fn updating_status_preserves_existing_url_and_title() {
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
        OpenPreview::new(&previews, &FakePreviewAdapter, &events)
            .execute(&workspace, Some("http://localhost:3000".into()))
            .await
            .unwrap();

        previews
            .update_preview_state(
                &workspace.workspace_id,
                herdr_workbench_domain::PreviewStatus::Open,
                Some("http://localhost:3000/app".into()),
                Some("App".into()),
            )
            .await
            .unwrap();

        let closed = previews
            .update_preview_state(
                &workspace.workspace_id,
                herdr_workbench_domain::PreviewStatus::Unavailable,
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(
            closed.status,
            herdr_workbench_domain::PreviewStatus::Unavailable
        );
        assert_eq!(closed.url.as_deref(), Some("http://localhost:3000/app"));
        assert_eq!(closed.title.as_deref(), Some("App"));
        assert_eq!(previews.revision(&workspace.workspace_id).await, 1);
    }

    #[tokio::test]
    async fn updating_preview_state_publishes_without_advancing_revision() {
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
        OpenPreview::new(&previews, &FakePreviewAdapter, &events)
            .execute(&workspace, Some("http://localhost:3000".into()))
            .await
            .unwrap();
        let mut receiver = events.subscribe();
        let updater = PublishingPreviewStateUpdater::new(previews.clone(), events.clone());

        let session = updater
            .update_preview_state(
                &workspace.workspace_id,
                herdr_workbench_domain::PreviewStatus::Open,
                Some("http://localhost:3000/app".into()),
                Some("App".into()),
            )
            .await
            .unwrap();
        let closed = updater
            .update_preview_state(
                &workspace.workspace_id,
                herdr_workbench_domain::PreviewStatus::Unavailable,
                None,
                None,
            )
            .await
            .unwrap();

        let opened = receiver.recv().await.unwrap();
        assert_eq!(opened.event_type, EventType::PreviewStateUpdated);
        assert_eq!(opened.revision, 0);
        assert_eq!(
            opened.payload,
            EventPayload::PreviewStateUpdated(herdr_workbench_domain::PreviewStateUpdated {
                session: session.clone()
            })
        );
        let closed_event = receiver.recv().await.unwrap();
        assert_eq!(closed_event.event_type, EventType::PreviewStateUpdated);
        assert_eq!(closed_event.revision, 0);
        assert_eq!(closed.url.as_deref(), Some("http://localhost:3000/app"));
        assert_eq!(closed.title.as_deref(), Some("App"));
        assert_eq!(previews.revision(&workspace.workspace_id).await, 1);
    }

    #[tokio::test]
    async fn capturing_screenshot_persists_metadata_and_advances_revision() {
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
        let store = InMemoryScreenshotStore::default();
        let events = EventBus::new(16);
        let mut receiver = events.subscribe();
        OpenPreview::new(&previews, &FakePreviewAdapter, &events)
            .execute(&workspace, Some("http://localhost:3000".into()))
            .await
            .unwrap();
        let _ = receiver.recv().await.unwrap();

        let screenshot = CapturePreview::new(&previews, &FakePreviewAdapter, &events, &store)
            .execute(&workspace)
            .await
            .unwrap();
        let event = receiver.recv().await.unwrap();
        let stored = store.load(&screenshot.path).await.unwrap();

        assert_eq!(screenshot.workspace_id, workspace.workspace_id);
        assert_eq!(screenshot.revision, 2);
        assert_eq!(screenshot.byte_size, 8);
        assert_eq!(stored, vec![137, 80, 78, 71, 13, 10, 26, 10]);
        assert_eq!(event.event_type, EventType::PreviewScreenshotCaptured);
        assert_eq!(event.revision, 2);
        assert_eq!(previews.revision(&workspace.workspace_id).await, 2);
        assert!(
            previews
                .find_latest(&workspace.workspace_id)
                .await
                .unwrap()
                .is_some()
        );
    }

    fn sample_diagnostic(
        workspace: &herdr_workbench_domain::Workspace,
        message: &str,
    ) -> PreviewDiagnostic {
        PreviewDiagnostic {
            workspace_id: workspace.workspace_id.clone(),
            kind: PreviewDiagnosticKind::Console,
            level: PreviewDiagnosticLevel::Error,
            message: message.to_owned(),
            source: Some("http://localhost:3000/app.js".into()),
            status: None,
            occurred_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn recording_diagnostics_keeps_the_latest_fifty_and_can_be_cleared() {
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
        let sink = InMemoryPreviewDiagnostics::default();
        sink.record(sample_diagnostic(&workspace, "first")).await;
        for index in 0..super::PREVIEW_DIAGNOSTIC_LIMIT {
            sink.record(sample_diagnostic(&workspace, &format!("msg-{index}")))
                .await;
        }
        let listed = sink.list(&workspace.workspace_id).await;
        assert_eq!(listed.len(), super::PREVIEW_DIAGNOSTIC_LIMIT);
        assert_eq!(listed[0].message, "msg-0");
        assert_eq!(listed.last().unwrap().message, "msg-49");
        sink.clear(&workspace.workspace_id).await;
        assert!(sink.list(&workspace.workspace_id).await.is_empty());
    }

    #[tokio::test]
    async fn recording_diagnostics_publishes_without_a_revision() {
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
        let events = EventBus::new(8);
        let mut receiver = events.subscribe();
        let sink = InMemoryPreviewDiagnostics::with_publisher(Arc::new(events));
        sink.record(sample_diagnostic(&workspace, "boom")).await;
        let event = receiver.recv().await.unwrap();
        assert_eq!(event.event_type, EventType::PreviewDiagnosticsUpdated);
        assert_eq!(event.revision, 0);
        assert_eq!(event.workspace_id, workspace.workspace_id);
    }

    #[derive(Clone, Default)]
    struct FakeHerdrHost {
        workspaces: Vec<HerdrWorkspaceInfo>,
        panes: Vec<HerdrPaneInfo>,
        error: Option<HerdrHostError>,
    }

    #[async_trait]
    impl HerdrHost for FakeHerdrHost {
        async fn list_workspaces(&self) -> Result<Vec<HerdrWorkspaceInfo>, HerdrHostError> {
            if let Some(error) = &self.error {
                return Err(error.clone());
            }
            Ok(self.workspaces.clone())
        }

        async fn list_panes(&self) -> Result<Vec<HerdrPaneInfo>, HerdrHostError> {
            if let Some(error) = &self.error {
                return Err(error.clone());
            }
            Ok(self.panes.clone())
        }
    }

    fn sample_host_workspace(id: &str, label: &str) -> HerdrWorkspaceInfo {
        HerdrWorkspaceInfo {
            workspace_id: id.into(),
            label: label.into(),
            worktree_checkout_path: None,
        }
    }

    fn sample_pane(workspace_id: &str, cwd: &str, focused: bool) -> HerdrPaneInfo {
        HerdrPaneInfo {
            workspace_id: workspace_id.into(),
            cwd: Some(PathBuf::from(cwd)),
            focused,
        }
    }

    #[tokio::test]
    async fn syncing_herdr_workspaces_binds_each_workspace_from_its_pane_cwd() {
        let repository = InMemoryWorkspaceRepository::default();
        let host = FakeHerdrHost {
            workspaces: vec![
                sample_host_workspace("wD", "code"),
                sample_host_workspace("w9", "other"),
            ],
            panes: vec![
                sample_pane("wD", r"D:\Code\huajingweb", true),
                sample_pane("w9", r"F:\github\Chronos", false),
                sample_pane("w9", r"F:\github\QuickPane", true),
            ],
            error: None,
        };

        let report = SyncHerdrWorkspaces::new(&repository, &host)
            .execute()
            .await
            .unwrap();

        assert_eq!(report.bound.len(), 2);
        assert_eq!(report.skipped, 0);
        assert_eq!(repository.workspace_count().await, 2);
        let listed = repository.list().await.unwrap();
        let code = listed
            .iter()
            .find(|workspace| workspace.herdr_workspace_id.as_str() == "wD")
            .unwrap();
        let other = listed
            .iter()
            .find(|workspace| workspace.herdr_workspace_id.as_str() == "w9")
            .unwrap();
        assert_eq!(code.label, "code");
        assert_eq!(code.cwd, PathBuf::from(r"D:\Code\huajingweb"));
        assert_eq!(other.label, "other");
        assert_eq!(other.cwd, PathBuf::from(r"F:\github\QuickPane"));
    }

    #[tokio::test]
    async fn syncing_the_same_herdr_workspaces_again_reuses_existing_bindings() {
        let repository = InMemoryWorkspaceRepository::default();
        let host = FakeHerdrHost {
            workspaces: vec![sample_host_workspace("wD", "code")],
            panes: vec![sample_pane("wD", r"D:\Code\huajingweb", true)],
            error: None,
        };
        let use_case = SyncHerdrWorkspaces::new(&repository, &host);
        let first = use_case.execute().await.unwrap();
        let second = use_case.execute().await.unwrap();

        assert_eq!(first.bound[0].workspace_id, second.bound[0].workspace_id);
        assert_eq!(repository.workspace_count().await, 1);
    }

    #[tokio::test]
    async fn syncing_skips_workspaces_without_a_usable_cwd() {
        let repository = InMemoryWorkspaceRepository::default();
        let host = FakeHerdrHost {
            workspaces: vec![
                sample_host_workspace("wD", "code"),
                sample_host_workspace("wH", "empty"),
            ],
            panes: vec![sample_pane("wD", r"D:\Code\huajingweb", true)],
            error: None,
        };

        let report = SyncHerdrWorkspaces::new(&repository, &host)
            .execute()
            .await
            .unwrap();

        assert_eq!(report.bound.len(), 1);
        assert_eq!(report.skipped, 1);
        assert_eq!(repository.workspace_count().await, 1);
    }

    #[tokio::test]
    async fn syncing_skips_invalid_cwd_and_still_binds_the_rest() {
        let repository = InMemoryWorkspaceRepository::default();
        let host = FakeHerdrHost {
            workspaces: vec![
                sample_host_workspace("bad", "unc"),
                sample_host_workspace("wD", "code"),
            ],
            panes: vec![
                sample_pane("bad", r"\\nas\share", true),
                sample_pane("wD", r"D:\Code\huajingweb", true),
            ],
            error: None,
        };

        let report = SyncHerdrWorkspaces::new(&repository, &host)
            .execute()
            .await
            .unwrap();

        assert_eq!(report.bound.len(), 1);
        assert_eq!(report.skipped, 1);
        assert_eq!(report.bound[0].herdr_workspace_id.as_str(), "wD");
    }

    #[tokio::test]
    async fn syncing_falls_back_to_worktree_checkout_when_panes_have_no_cwd() {
        let repository = InMemoryWorkspaceRepository::default();
        let host = FakeHerdrHost {
            workspaces: vec![HerdrWorkspaceInfo {
                workspace_id: "wT".into(),
                label: "tree".into(),
                worktree_checkout_path: Some(PathBuf::from(r"C:\projects\siftmark")),
            }],
            panes: vec![HerdrPaneInfo {
                workspace_id: "wT".into(),
                cwd: None,
                focused: true,
            }],
            error: None,
        };

        let report = SyncHerdrWorkspaces::new(&repository, &host)
            .execute()
            .await
            .unwrap();

        assert_eq!(report.bound.len(), 1);
        assert_eq!(report.bound[0].cwd, PathBuf::from(r"C:\projects\siftmark"));
    }

    #[tokio::test]
    async fn host_failure_leaves_the_repository_unchanged() {
        let repository = InMemoryWorkspaceRepository::default();
        let host = FakeHerdrHost {
            error: Some(HerdrHostError::unavailable("herdr not running")),
            ..FakeHerdrHost::default()
        };

        let error = SyncHerdrWorkspaces::new(&repository, &host)
            .execute()
            .await
            .unwrap_err();

        assert!(matches!(error, super::ApplicationError::Herdr(_)));
        assert_eq!(repository.workspace_count().await, 0);
    }

    #[tokio::test]
    async fn syncing_skips_a_relative_cwd_and_still_binds_the_rest() {
        let repository = InMemoryWorkspaceRepository::default();
        let host = FakeHerdrHost {
            workspaces: vec![
                sample_host_workspace("rel", "relative"),
                sample_host_workspace("wD", "code"),
            ],
            panes: vec![
                sample_pane("rel", r"projects\siftmark", true),
                sample_pane("wD", r"D:\Code\huajingweb", true),
            ],
            error: None,
        };

        let report = SyncHerdrWorkspaces::new(&repository, &host)
            .execute()
            .await
            .unwrap();

        assert_eq!(report.bound.len(), 1);
        assert_eq!(report.skipped, 1);
        assert_eq!(report.bound[0].herdr_workspace_id.as_str(), "wD");
    }

    #[tokio::test]
    async fn focused_relative_cwd_falls_back_to_the_first_absolute_pane() {
        let repository = InMemoryWorkspaceRepository::default();
        let host = FakeHerdrHost {
            workspaces: vec![sample_host_workspace("w9", "other")],
            panes: vec![
                sample_pane("w9", r"relative\path", true),
                sample_pane("w9", r"F:\github\QuickPane", false),
            ],
            error: None,
        };

        let report = SyncHerdrWorkspaces::new(&repository, &host)
            .execute()
            .await
            .unwrap();

        assert_eq!(report.bound.len(), 1);
        assert_eq!(report.bound[0].cwd, PathBuf::from(r"F:\github\QuickPane"));
    }

    #[tokio::test]
    async fn relative_pane_cwd_falls_back_to_worktree_checkout() {
        let repository = InMemoryWorkspaceRepository::default();
        let host = FakeHerdrHost {
            workspaces: vec![HerdrWorkspaceInfo {
                workspace_id: "wT".into(),
                label: "tree".into(),
                worktree_checkout_path: Some(PathBuf::from(r"C:\projects\siftmark")),
            }],
            panes: vec![sample_pane("wT", r"relative\path", true)],
            error: None,
        };

        let report = SyncHerdrWorkspaces::new(&repository, &host)
            .execute()
            .await
            .unwrap();

        assert_eq!(report.bound.len(), 1);
        assert_eq!(report.bound[0].cwd, PathBuf::from(r"C:\projects\siftmark"));
    }

    #[derive(Clone, Default)]
    struct RecordingSleeper {
        sleeps: Arc<std::sync::Mutex<Vec<Duration>>>,
    }

    impl RecordingSleeper {
        fn recorded(&self) -> Vec<Duration> {
            self.sleeps.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl ReconcileSleeper for RecordingSleeper {
        async fn sleep(&self, duration: Duration) {
            self.sleeps.lock().unwrap().push(duration);
        }
    }

    type HostSnapshot = Result<(Vec<HerdrWorkspaceInfo>, Vec<HerdrPaneInfo>), HerdrHostError>;

    #[derive(Clone)]
    struct SequenceHost {
        results: Arc<std::sync::Mutex<Vec<HostSnapshot>>>,
        current: Arc<std::sync::Mutex<Option<HostSnapshot>>>,
    }

    impl SequenceHost {
        fn snapshot(&self) -> HostSnapshot {
            let mut current = self.current.lock().unwrap();
            if current.is_none() {
                let mut results = self.results.lock().unwrap();
                *current = Some(if results.is_empty() {
                    Err(HerdrHostError::unavailable("no more host results"))
                } else {
                    results.remove(0)
                });
            }
            current.clone().expect("host snapshot")
        }

        fn finish_step(&self) {
            *self.current.lock().unwrap() = None;
        }
    }

    #[async_trait]
    impl HerdrHost for SequenceHost {
        async fn list_workspaces(&self) -> Result<Vec<HerdrWorkspaceInfo>, HerdrHostError> {
            self.snapshot().map(|(workspaces, _)| workspaces)
        }

        async fn list_panes(&self) -> Result<Vec<HerdrPaneInfo>, HerdrHostError> {
            self.snapshot().map(|(_, panes)| panes)
        }
    }

    #[tokio::test]
    async fn successful_reconcile_step_sleeps_thirty_seconds() {
        let repository = Arc::new(InMemoryWorkspaceRepository::default());
        let host = FakeHerdrHost {
            workspaces: vec![sample_host_workspace("wD", "code")],
            panes: vec![sample_pane("wD", r"D:\Code\huajingweb", true)],
            error: None,
        };
        let sleeper = RecordingSleeper::default();
        let interval = HerdrReconcileLoop::new(Arc::clone(&repository), host, sleeper.clone())
            .step()
            .await;
        assert_eq!(interval, HERDR_RECONCILE_OK_INTERVAL);
        assert_eq!(sleeper.recorded(), vec![HERDR_RECONCILE_OK_INTERVAL]);
        assert_eq!(repository.workspace_count().await, 1);
    }

    #[tokio::test]
    async fn failed_reconcile_step_backs_off_two_minutes_then_recovers() {
        let repository = Arc::new(InMemoryWorkspaceRepository::default());
        let host = SequenceHost {
            results: Arc::new(std::sync::Mutex::new(vec![
                Err(HerdrHostError::unavailable("herdr not running")),
                Ok((
                    vec![sample_host_workspace("wD", "code")],
                    vec![sample_pane("wD", r"D:\Code\huajingweb", true)],
                )),
            ])),
            current: Arc::new(std::sync::Mutex::new(None)),
        };
        let sleeper = RecordingSleeper::default();
        let loop_ = HerdrReconcileLoop::new(Arc::clone(&repository), host.clone(), sleeper.clone());
        let first = loop_.step().await;
        host.finish_step();
        let second = loop_.step().await;
        assert_eq!(first, HERDR_RECONCILE_BACKOFF_INTERVAL);
        assert_eq!(second, HERDR_RECONCILE_OK_INTERVAL);
        assert_eq!(
            sleeper.recorded(),
            vec![
                HERDR_RECONCILE_BACKOFF_INTERVAL,
                HERDR_RECONCILE_OK_INTERVAL
            ]
        );
        assert_eq!(repository.workspace_count().await, 1);
    }

    #[derive(Clone)]
    struct FakeEventSource {
        events: Arc<std::sync::Mutex<Vec<Result<HerdrLifecycleEvent, HerdrHostError>>>>,
    }

    #[async_trait]
    impl HerdrEventSource for FakeEventSource {
        async fn next_event(&self) -> Result<HerdrLifecycleEvent, HerdrHostError> {
            let mut events = self.events.lock().unwrap();
            if events.is_empty() {
                return Err(HerdrHostError::unavailable("no more herdr events"));
            }
            events.remove(0)
        }
    }

    fn sample_bindable_host() -> FakeHerdrHost {
        FakeHerdrHost {
            workspaces: vec![sample_host_workspace("wD", "code")],
            panes: vec![sample_pane("wD", r"D:\Code\huajingweb", true)],
            error: None,
        }
    }

    #[tokio::test]
    async fn workspace_created_event_syncs_new_bindings() {
        let repository = Arc::new(InMemoryWorkspaceRepository::default());
        let events = FakeEventSource {
            events: Arc::new(std::sync::Mutex::new(vec![Ok(
                HerdrLifecycleEvent::WorkspaceCreated,
            )])),
        };
        let synced =
            HerdrEventSyncLoop::new(Arc::clone(&repository), sample_bindable_host(), events)
                .step()
                .await
                .unwrap();
        assert!(synced);
        assert_eq!(repository.workspace_count().await, 1);
    }

    #[tokio::test]
    async fn workspace_closed_event_does_not_unbind_or_sync() {
        let repository = Arc::new(InMemoryWorkspaceRepository::default());
        let events = FakeEventSource {
            events: Arc::new(std::sync::Mutex::new(vec![Ok(
                HerdrLifecycleEvent::WorkspaceClosed,
            )])),
        };
        let synced =
            HerdrEventSyncLoop::new(Arc::clone(&repository), sample_bindable_host(), events)
                .step()
                .await
                .unwrap();
        assert!(!synced);
        assert_eq!(repository.workspace_count().await, 0);
    }

    #[tokio::test]
    async fn event_source_failure_does_not_panic_or_bind() {
        let repository = Arc::new(InMemoryWorkspaceRepository::default());
        let events = FakeEventSource {
            events: Arc::new(std::sync::Mutex::new(vec![Err(
                HerdrHostError::unavailable("pipe closed"),
            )])),
        };
        let error =
            HerdrEventSyncLoop::new(Arc::clone(&repository), sample_bindable_host(), events)
                .step()
                .await
                .unwrap_err();
        assert!(matches!(error, super::ApplicationError::Herdr(_)));
        assert_eq!(repository.workspace_count().await, 0);
    }

    type PromptLog = Arc<std::sync::Mutex<Vec<(String, String)>>>;
    type TranscriptMap = Arc<std::sync::Mutex<HashMap<String, String>>>;
    type KeyLog = Arc<std::sync::Mutex<Vec<(String, Vec<String>)>>>;

    #[derive(Default)]
    struct FakeAgentBridge {
        agents: Vec<HerdrAgentInfo>,
        prompts: PromptLog,
        transcripts: TranscriptMap,
        keys: KeyLog,
    }

    #[async_trait]
    impl HerdrAgentBridge for FakeAgentBridge {
        async fn list_agents(&self) -> Result<Vec<HerdrAgentInfo>, HerdrHostError> {
            Ok(self.agents.clone())
        }

        async fn prompt_agent(&self, target: &str, text: &str) -> Result<(), HerdrHostError> {
            self.prompts
                .lock()
                .unwrap()
                .push((target.to_owned(), text.to_owned()));
            Ok(())
        }

        async fn read_agent(&self, target: &str) -> Result<String, HerdrHostError> {
            Ok(self
                .transcripts
                .lock()
                .unwrap()
                .get(target)
                .cloned()
                .unwrap_or_default())
        }

        async fn send_agent_keys(&self, target: &str, keys: &[&str]) -> Result<(), HerdrHostError> {
            self.keys.lock().unwrap().push((
                target.to_owned(),
                keys.iter().map(|key| (*key).to_owned()).collect(),
            ));
            Ok(())
        }
    }

    fn sample_agent(
        workspace_id: &str,
        pane_id: &str,
        focused: bool,
        status: &str,
    ) -> HerdrAgentInfo {
        HerdrAgentInfo {
            workspace_id: workspace_id.into(),
            pane_id: pane_id.into(),
            agent: "pi".into(),
            status: status.into(),
            focused,
        }
    }

    #[tokio::test]
    async fn sending_preview_context_prompts_the_focused_agent() {
        let workspaces = InMemoryWorkspaceRepository::default();
        let workspace = BindWorkspace::new(&workspaces)
            .execute(
                HerdrWorkspaceContext::new(
                    "herdr-1",
                    "Siftmark",
                    PathBuf::from(r"C:\projects\siftmark"),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let previews = InMemoryPreviewRepository::default();
        let events = EventBus::new(8);
        OpenPreview::new(&previews, &FakePreviewAdapter, &events)
            .execute(&workspace, Some("http://localhost:3000".into()))
            .await
            .unwrap();
        CapturePreview::new(
            &previews,
            &FakePreviewAdapter,
            &events,
            &InMemoryScreenshotStore::default(),
        )
        .execute(&workspace)
        .await
        .unwrap();
        let diagnostics = InMemoryPreviewDiagnostics::default();
        diagnostics
            .record(PreviewDiagnostic {
                workspace_id: workspace.workspace_id.clone(),
                kind: PreviewDiagnosticKind::Console,
                level: PreviewDiagnosticLevel::Error,
                message: "boom".into(),
                source: None,
                status: None,
                occurred_at: chrono::Utc::now(),
            })
            .await;
        let prompts = Arc::new(std::sync::Mutex::new(Vec::new()));
        let agents = FakeAgentBridge {
            agents: vec![
                sample_agent("other", "wX:p1", true, "idle"),
                sample_agent("herdr-1", "w1:p2", false, "working"),
                sample_agent("herdr-1", "w1:p1", true, "idle"),
            ],
            prompts: Arc::clone(&prompts),
            ..FakeAgentBridge::default()
        };
        let receipt = SendPreviewContext::new(&previews, &diagnostics, &agents)
            .execute(&workspace, Some("please check the header".into()))
            .await
            .unwrap();
        assert_eq!(receipt.pane_id, "w1:p1");
        assert_eq!(receipt.agent, "pi");
        let recorded = prompts.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].0, "w1:p1");
        assert!(recorded[0].1.contains("http://localhost:3000"));
        assert!(recorded[0].1.contains("boom"));
        assert!(recorded[0].1.contains("please check the header"));
        assert!(!recorded[0].1.contains('\u{89}'));
    }

    #[tokio::test]
    async fn sending_preview_context_without_an_agent_does_not_prompt() {
        let workspaces = InMemoryWorkspaceRepository::default();
        let workspace = BindWorkspace::new(&workspaces)
            .execute(
                HerdrWorkspaceContext::new(
                    "herdr-1",
                    "Siftmark",
                    PathBuf::from(r"C:\projects\siftmark"),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let previews = InMemoryPreviewRepository::default();
        let diagnostics = InMemoryPreviewDiagnostics::default();
        let prompts = Arc::new(std::sync::Mutex::new(Vec::new()));
        let agents = FakeAgentBridge {
            agents: vec![sample_agent("other", "wX:p1", true, "idle")],
            prompts: Arc::clone(&prompts),
            ..FakeAgentBridge::default()
        };
        let error = SendPreviewContext::new(&previews, &diagnostics, &agents)
            .execute(&workspace, None)
            .await
            .unwrap_err();
        assert!(matches!(error, super::ApplicationError::Herdr(_)));
        assert!(prompts.lock().unwrap().is_empty());
    }

    #[test]
    fn lan_access_is_off_by_default() {
        let lan = LanAccess::default();
        assert!(!lan.enabled());
        assert!(lan.pairing_code().is_none());
        assert!(lan.authorize("10.0.0.8", None).is_err());
        assert!(lan.authorize("127.0.0.1", None).is_ok());
    }

    #[test]
    fn enabling_lan_issues_a_pairing_code_required_off_localhost() {
        let mut lan = LanAccess::default();
        let enabled = lan.enable(["192.168.1.20".into()]);
        assert!(enabled.enabled);
        assert_eq!(enabled.listen, "0.0.0.0:17321");
        assert!(
            enabled
                .urls
                .iter()
                .any(|url| url.contains("192.168.1.20:17321"))
        );
        assert_eq!(enabled.pairing_code.len(), 6);
        assert!(lan.authorize("10.0.0.8", None).is_err());
        assert!(
            lan.authorize("10.0.0.8", Some(&enabled.pairing_code))
                .is_ok()
        );
        assert!(lan.authorize("127.0.0.1", None).is_ok());
        lan.disable();
        assert!(!lan.enabled());
        assert!(
            lan.authorize("10.0.0.8", Some(&enabled.pairing_code))
                .is_err()
        );
    }

    #[tokio::test]
    async fn listing_workspace_agents_hides_other_workspaces() {
        let workspaces = InMemoryWorkspaceRepository::default();
        let workspace = BindWorkspace::new(&workspaces)
            .execute(
                HerdrWorkspaceContext::new(
                    "herdr-1",
                    "Siftmark",
                    PathBuf::from(r"C:\projects\siftmark"),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let agents = FakeAgentBridge {
            agents: vec![
                sample_agent("other", "wX:p1", true, "idle"),
                sample_agent("herdr-1", "w1:p2", false, "working"),
                sample_agent("herdr-1", "w1:p1", true, "idle"),
            ],
            ..FakeAgentBridge::default()
        };
        let listed = ListWorkspaceAgents::new(&agents)
            .execute(&workspace)
            .await
            .unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].pane_id, "w1:p2");
        assert_eq!(listed[1].pane_id, "w1:p1");
        assert!(listed.iter().all(|agent| agent.workspace_id == "herdr-1"));
    }

    #[tokio::test]
    async fn reading_a_workspace_agent_returns_recent_transcript() {
        let workspaces = InMemoryWorkspaceRepository::default();
        let workspace = BindWorkspace::new(&workspaces)
            .execute(
                HerdrWorkspaceContext::new(
                    "herdr-1",
                    "Siftmark",
                    PathBuf::from(r"C:\projects\siftmark"),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let agents = FakeAgentBridge {
            agents: vec![sample_agent("herdr-1", "w1:p1", true, "idle")],
            transcripts: Arc::new(std::sync::Mutex::new(
                [("w1:p1".into(), "please approve the install".into())]
                    .into_iter()
                    .collect(),
            )),
            ..FakeAgentBridge::default()
        };
        let session = ReadWorkspaceAgent::new(&agents)
            .execute(&workspace, "w1:p1")
            .await
            .unwrap();
        assert_eq!(session.pane_id, "w1:p1");
        assert_eq!(session.agent, "pi");
        assert_eq!(session.status, "idle");
        assert_eq!(session.transcript, "please approve the install");
    }

    #[test]
    fn stripping_agent_transcript_drops_the_tui_footer() {
        let raw = "please approve the install\n\
\n\
────────────────────────────────────────────────────────────────\n\
\n\
 grok-4.6  Sylorapi Copy 2  xhigh\n\
────────────────────────────────────────────────────────────────\n\
 Huajingflow in demo on * feat/demo\n\
 [####------] 41.3%/500k (auto) | $0.000\n";
        assert_eq!(
            super::strip_agent_transcript(raw),
            "please approve the install"
        );
        assert_eq!(
            super::strip_agent_transcript("please approve the install"),
            "please approve the install"
        );
    }

    #[tokio::test]
    async fn prompting_a_ready_agent_sends_the_user_text() {
        let workspaces = InMemoryWorkspaceRepository::default();
        let workspace = BindWorkspace::new(&workspaces)
            .execute(
                HerdrWorkspaceContext::new(
                    "herdr-1",
                    "Siftmark",
                    PathBuf::from(r"C:\projects\siftmark"),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let prompts = Arc::new(std::sync::Mutex::new(Vec::new()));
        let agents = FakeAgentBridge {
            agents: vec![sample_agent("herdr-1", "w1:p1", true, "idle")],
            prompts: Arc::clone(&prompts),
            ..FakeAgentBridge::default()
        };
        PromptWorkspaceAgent::new(&agents)
            .execute(&workspace, "w1:p1", "fix the header")
            .await
            .unwrap();
        assert_eq!(
            *prompts.lock().unwrap(),
            vec![("w1:p1".into(), "fix the header".into())]
        );
    }

    #[tokio::test]
    async fn prompting_a_blocked_agent_is_rejected_without_sending_text() {
        let workspaces = InMemoryWorkspaceRepository::default();
        let workspace = BindWorkspace::new(&workspaces)
            .execute(
                HerdrWorkspaceContext::new(
                    "herdr-1",
                    "Siftmark",
                    PathBuf::from(r"C:\projects\siftmark"),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let prompts = Arc::new(std::sync::Mutex::new(Vec::new()));
        let agents = FakeAgentBridge {
            agents: vec![sample_agent("herdr-1", "w1:p1", true, "blocked")],
            prompts: Arc::clone(&prompts),
            ..FakeAgentBridge::default()
        };
        let error = PromptWorkspaceAgent::new(&agents)
            .execute(&workspace, "w1:p1", "keep going")
            .await
            .unwrap_err();
        assert!(error.to_string().contains("blocked"));
        assert!(prompts.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn approving_a_blocked_agent_sends_yes_keys() {
        let workspaces = InMemoryWorkspaceRepository::default();
        let workspace = BindWorkspace::new(&workspaces)
            .execute(
                HerdrWorkspaceContext::new(
                    "herdr-1",
                    "Siftmark",
                    PathBuf::from(r"C:\projects\siftmark"),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let keys = Arc::new(std::sync::Mutex::new(Vec::new()));
        let agents = FakeAgentBridge {
            agents: vec![sample_agent("herdr-1", "w1:p1", true, "blocked")],
            keys: Arc::clone(&keys),
            ..FakeAgentBridge::default()
        };
        ApproveWorkspaceAgent::new(&agents)
            .execute(&workspace, "w1:p1", AgentDecision::Yes)
            .await
            .unwrap();
        assert_eq!(
            *keys.lock().unwrap(),
            vec![("w1:p1".into(), vec!["y".into(), "enter".into()])]
        );
    }

    #[tokio::test]
    async fn rejecting_a_blocked_agent_sends_no_keys() {
        let workspaces = InMemoryWorkspaceRepository::default();
        let workspace = BindWorkspace::new(&workspaces)
            .execute(
                HerdrWorkspaceContext::new(
                    "herdr-1",
                    "Siftmark",
                    PathBuf::from(r"C:\projects\siftmark"),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let keys = Arc::new(std::sync::Mutex::new(Vec::new()));
        let agents = FakeAgentBridge {
            agents: vec![sample_agent("herdr-1", "w1:p1", true, "blocked")],
            keys: Arc::clone(&keys),
            ..FakeAgentBridge::default()
        };
        ApproveWorkspaceAgent::new(&agents)
            .execute(&workspace, "w1:p1", AgentDecision::No)
            .await
            .unwrap();
        assert_eq!(
            *keys.lock().unwrap(),
            vec![("w1:p1".into(), vec!["n".into(), "enter".into()])]
        );
    }

    #[tokio::test]
    async fn approving_an_idle_agent_is_rejected() {
        let workspaces = InMemoryWorkspaceRepository::default();
        let workspace = BindWorkspace::new(&workspaces)
            .execute(
                HerdrWorkspaceContext::new(
                    "herdr-1",
                    "Siftmark",
                    PathBuf::from(r"C:\projects\siftmark"),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let keys = Arc::new(std::sync::Mutex::new(Vec::new()));
        let agents = FakeAgentBridge {
            agents: vec![sample_agent("herdr-1", "w1:p1", true, "idle")],
            keys: Arc::clone(&keys),
            ..FakeAgentBridge::default()
        };
        let error = ApproveWorkspaceAgent::new(&agents)
            .execute(&workspace, "w1:p1", AgentDecision::Yes)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("blocked"));
        assert!(keys.lock().unwrap().is_empty());
    }
}
