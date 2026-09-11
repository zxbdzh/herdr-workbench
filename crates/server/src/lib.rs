use std::{env, fs, net::SocketAddr, path::PathBuf, sync::Arc};

use herdr_workbench_adapters_herdr::{HerdrCliHost, HerdrNamedPipeEventSource};
use herdr_workbench_adapters_sqlite::{FilesystemScreenshotStore, SqliteWorkspaceRepository};
use herdr_workbench_app_core::{
    EventBus, EventPublisher, HerdrEventSyncLoop, HerdrReconcileLoop, InMemoryPreviewDiagnostics,
    PreviewAdapter, PreviewDiagnosticsSink, SyncHerdrWorkspaces, TokioReconcileSleeper,
    WorkspaceRepository,
};
use herdr_workbench_transport::{AppState, router};
use thiserror::Error;

pub const DEFAULT_ADDRESS: &str = "127.0.0.1:17321";
pub const LAN_ADDRESS: &str = "0.0.0.0:17321";

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("failed to create Workbench data directory: {0}")]
    CreateDataDirectory(#[source] std::io::Error),
    #[error("failed to connect Workbench database: {0}")]
    ConnectDatabase(#[source] sqlx::Error),
    #[error("failed to migrate Workbench database: {0}")]
    MigrateDatabase(#[source] sqlx::migrate::MigrateError),
    #[error("failed to bind Workbench localhost port: {0}")]
    Bind(#[source] std::io::Error),
    #[error("Workbench HTTP server failed: {0}")]
    Serve(#[source] std::io::Error),
}

pub async fn connect_repository() -> Result<Arc<SqliteWorkspaceRepository>, ServerError> {
    let data_dir = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("HerdrWorkbench");
    fs::create_dir_all(&data_dir).map_err(ServerError::CreateDataDirectory)?;

    let database_path = data_dir.join("workbench.db");
    let database_url = format!(
        "sqlite:///{}",
        database_path.to_string_lossy().replace('\\', "/")
    );
    let repository = Arc::new(
        SqliteWorkspaceRepository::connect(&database_url)
            .await
            .map_err(ServerError::ConnectDatabase)?,
    );
    repository
        .migrate()
        .await
        .map_err(ServerError::MigrateDatabase)?;
    Ok(repository)
}

fn screenshot_store() -> Arc<FilesystemScreenshotStore> {
    let data_dir = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("HerdrWorkbench")
        .join("screenshots");
    Arc::new(FilesystemScreenshotStore::new(data_dir))
}

pub fn shared_event_bus() -> Arc<EventBus> {
    Arc::new(EventBus::new(256))
}

pub fn diagnostics_with_bus(events: Arc<EventBus>) -> Arc<dyn PreviewDiagnosticsSink> {
    Arc::new(InMemoryPreviewDiagnostics::with_publisher(
        events as Arc<dyn EventPublisher>,
    ))
}

pub async fn sync_herdr_workspaces<R>(repository: &R)
where
    R: WorkspaceRepository,
{
    let host = HerdrCliHost::from_env();
    match SyncHerdrWorkspaces::new(repository, &host).execute().await {
        Ok(report) => {
            println!(
                "Herdr workspace sync bound {} workspace(s), skipped {}",
                report.bound.len(),
                report.skipped
            );
        }
        Err(error) => eprintln!("Herdr workspace sync skipped: {error}"),
    }
}

pub fn spawn_herdr_reconcile<R>(repository: Arc<R>)
where
    R: WorkspaceRepository + 'static,
{
    tokio::spawn(async move {
        HerdrReconcileLoop::new(repository, HerdrCliHost::from_env(), TokioReconcileSleeper)
            .run()
            .await;
    });
}

pub fn spawn_herdr_event_sync<R>(repository: Arc<R>)
where
    R: WorkspaceRepository + 'static,
{
    tokio::spawn(async move {
        HerdrEventSyncLoop::new(
            repository,
            HerdrCliHost::from_env(),
            HerdrNamedPipeEventSource::from_env(),
        )
        .run()
        .await;
    });
}

pub async fn serve<A>(preview_adapter: Arc<A>) -> Result<(), ServerError>
where
    A: PreviewAdapter + 'static,
{
    let repository = connect_repository().await?;
    spawn_herdr_reconcile(Arc::clone(&repository));
    spawn_herdr_event_sync(Arc::clone(&repository));
    let sync_repository = Arc::clone(&repository);
    tokio::spawn(async move {
        sync_herdr_workspaces(sync_repository.as_ref()).await;
    });
    let events = shared_event_bus();
    let diagnostics = diagnostics_with_bus(Arc::clone(&events));
    serve_with_repository(repository, preview_adapter, diagnostics, events).await
}

pub async fn serve_with_repository<A>(
    repository: Arc<SqliteWorkspaceRepository>,
    preview_adapter: Arc<A>,
    diagnostics: Arc<dyn PreviewDiagnosticsSink>,
    events: Arc<EventBus>,
) -> Result<(), ServerError>
where
    A: PreviewAdapter + 'static,
{
    serve_with_repository_ready(repository, preview_adapter, diagnostics, events, || {}).await
}

pub async fn serve_with_repository_ready<A, F>(
    repository: Arc<SqliteWorkspaceRepository>,
    preview_adapter: Arc<A>,
    diagnostics: Arc<dyn PreviewDiagnosticsSink>,
    events: Arc<EventBus>,
    on_ready: F,
) -> Result<(), ServerError>
where
    A: PreviewAdapter + 'static,
    F: FnOnce() + Send + 'static,
{
    let mut on_ready = Some(on_ready);
    let (lan_tx, mut lan_rx) = tokio::sync::watch::channel(false);
    let mut state = AppState::new(
        Arc::clone(&repository),
        repository,
        preview_adapter,
        events,
        screenshot_store(),
        diagnostics,
        Arc::new(HerdrCliHost::from_env()),
    );
    state.lan_bind = Some(lan_tx);
    let mut announced = false;
    loop {
        let lan_enabled = *lan_rx.borrow_and_update();
        let address: SocketAddr = if lan_enabled {
            LAN_ADDRESS.parse().expect("valid lan address")
        } else {
            DEFAULT_ADDRESS.parse().expect("valid localhost address")
        };
        let listener = match tokio::net::TcpListener::bind(address).await {
            Ok(listener) => listener,
            Err(error) if lan_enabled => {
                eprintln!("LAN bind failed, staying on localhost: {error}");
                {
                    let mut lan = state
                        .lan
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    lan.disable();
                }
                let _ = state
                    .lan_bind
                    .as_ref()
                    .expect("lan bind channel")
                    .send(false);
                continue;
            }
            Err(error) => return Err(ServerError::Bind(error)),
        };
        println!("herdr-workbench listening on http://{address}");
        if !announced {
            if let Some(ready) = on_ready.take() {
                ready();
            }
            announced = true;
        }
        let mut shutdown_rx = lan_rx.clone();
        axum::serve(
            listener,
            router(state.clone()).into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            let _ = shutdown_rx.changed().await;
        })
        .await
        .map_err(ServerError::Serve)?;
    }
}
