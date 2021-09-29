use async_trait::async_trait;

use super::ElasticsearchStorage;
use crate::domain::model::status::StorageStatus;
use crate::domain::ports::secondary::status::{Error as StatusError, Status};

#[async_trait]
impl Status for ElasticsearchStorage {
    /// Returns the status of the Elasticsearch Backend
    ///
    /// The status is a combination of the cluster's health, and its version.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use mimir2::domain::ports::secondary::remote::Remote;
    /// use mimir2::adapters::secondary::elasticsearch;
    /// use mimir2::domain::ports::primary::status::Status;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///   let pool = elasticsearch::remote::connection_pool().await.unwrap();
    ///   let client = pool.conn(500u64, ">=7.13.0").await.unwrap();
    ///
    ///   let status = client.status().await.unwrap();
    /// }
    /// ```
    async fn status(&self) -> Result<StorageStatus, StatusError> {
        let cluster_health =
            self.cluster_health()
                .await
                .map_err(|err| StatusError::HealthRetrievalError {
                    source: Box::new(err),
                })?;
        let cluster_version =
            self.cluster_version()
                .await
                .map_err(|err| StatusError::VersionRetrievalError {
                    source: Box::new(err),
                })?;

        Ok(StorageStatus {
            health: cluster_health,
            version: cluster_version,
        })
    }
}