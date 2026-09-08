use std::{env, fs, net::SocketAddr, path::PathBuf, sync::Arc};

use herdr_workbench_adapters_sqlite::{FilesystemScreenshotStore, SqliteWorkspaceRepository};
use herdr_workbench_app_core::{EventBus, PreviewAdapter};
use herdr_workbench_transport::{AppState, router};
use thiserror::Error;

pub const DEFAULT_ADDRESS: &str = "127.0.0.1:17321";

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

pub async fn serve<A>(preview_adapter: Arc<A>) -> Result<(), ServerError>
where
    A: PreviewAdapter + 'static,
{
    let repository = connect_repository().await?;
    serve_with_repository(repository, preview_adapter).await
}

pub async fn serve_with_repository<A>(
    repository: Arc<SqliteWorkspaceRepository>,
    preview_adapter: Arc<A>,
) -> Result<(), ServerError>
where
    A: PreviewAdapter + 'static,
{
    let events = Arc::new(EventBus::new(256));
    let address: SocketAddr = DEFAULT_ADDRESS.parse().expect("valid localhost address");
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(ServerError::Bind)?;
    println!("herdr-workbench listening on http://{address}");
    axum::serve(
        listener,
        router(AppState::new(
            Arc::clone(&repository),
            repository,
            preview_adapter,
            events,
            screenshot_store(),
        )),
    )
    .await
    .map_err(ServerError::Serve)
}
