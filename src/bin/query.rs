use mimir2::{
    adapters::primary::bragi::autocomplete::{build_query, Filters},
    adapters::primary::bragi::settings::QuerySettings,
    adapters::secondary::elasticsearch::remote::connection_test_pool,
    domain::ports::remote::Remote,
    domain::ports::search::SearchParameters,
    domain::usecases::search_documents::{SearchDocuments, SearchDocumentsParameters},
    domain::usecases::UseCase,
};
use places::{admin::Admin, MimirObject};
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let q = &args[0];

    let (pool, _) = connection_test_pool()
        .await
        .expect("Elasticsearch Connection Pool");
    let client = pool
        .conn()
        .await
        .expect("Elasticsearch Connection Established");

    let search_documents = SearchDocuments::new(Box::new(client));

    let filters = Filters::default();

    let mut query_settings_file = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    query_settings_file.push("config");
    query_settings_file.push("query");
    query_settings_file.push("settings.toml");
    let query_settings = QuerySettings::new_from_file(query_settings_file)
        .await
        .expect("query settings");

    let query = build_query(&q, filters, &["fr"], &query_settings);

    let parameters = SearchDocumentsParameters {
        parameters: SearchParameters {
            dsl: query,
            doc_types: vec![String::from(Admin::doc_type())],
        },
    };

    let search_result = search_documents.execute(parameters).await.unwrap();
    let search_result = serde_json::to_string_pretty(&search_result).unwrap();

    println!("Result: {}", search_result);
}