use utoipa::OpenApi;

fn main() {
    println!(
        "{}",
        herdr_workbench_contracts::ApiDoc::openapi()
            .to_pretty_json()
            .unwrap()
    );
}
