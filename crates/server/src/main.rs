use std::{env, fs, net::SocketAddr, path::PathBuf, sync::Arc};

use herdr_workbench_adapters_sqlite::SqliteWorkspaceRepository;
use herdr_workbench_transport::{AppState, router};

#[tokio::main]
async fn main() {
    let data_dir = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("HerdrWorkbench");
    fs::create_dir_all(&data_dir).expect("create Workbench data directory");

    let database_path = data_dir.join("workbench.db");
    let database_url = format!(
        "sqlite:///{}",
        database_path.to_string_lossy().replace('\\', "/")
    );
    println!("workbench database: {database_url}");
    let repository = SqliteWorkspaceRepository::connect(&database_url)
        .await
        .expect("connect Workbench database");
    repository
        .migrate()
        .await
        .expect("migrate Workbench database");

    let address: SocketAddr = "127.0.0.1:17321".parse().expect("valid localhost address");
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .expect("bind Workbench localhost port");
    println!("herdr-workbench listening on http://{address}");
    axum::serve(listener, router(AppState::new(Arc::new(repository))))
        .await
        .expect("serve Workbench HTTP API");
}
