use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    let address: SocketAddr = "127.0.0.1:17321".parse().expect("valid localhost address");
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .expect("bind workbench localhost port");
    println!("herdr-workbench listening on http://{address}");
    axum::serve(listener, herdr_workbench_transport::empty_router())
        .await
        .expect("serve workbench HTTP API");
}
