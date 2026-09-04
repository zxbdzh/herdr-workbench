use std::sync::Arc;

use herdr_workbench_server::serve;
use herdr_workbench_transport::UnavailablePreviewAdapter;

#[tokio::main]
async fn main() {
    serve(Arc::new(UnavailablePreviewAdapter))
        .await
        .expect("serve Workbench HTTP API");
}
