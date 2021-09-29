use super::ElasticsearchStorage;
use crate::domain::model::configuration::root_doctype;
use crate::domain::ports::secondary::list::{Error, List, Parameters};
use async_trait::async_trait;
use futures::stream::Stream;
use std::pin::Pin;

#[async_trait]
impl List for ElasticsearchStorage {
    type Doc = serde_json::Value;

    async fn list_documents(
        &self,
        parameters: Parameters,
    ) -> Result<Pin<Box<dyn Stream<Item = Self::Doc> + Send + 'static>>, Error> {
        self.list_documents(root_doctype(&parameters.doc_type))
            .await
            .map_err(|err| Error::DocumentRetrievalError { source: err.into() })
    }
}