use futures::stream::{Stream, TryStreamExt};
use snafu::futures::TryFutureExt;
use snafu::futures::TryStreamExt as SnafuTryStreamExt;
use snafu::{ResultExt, Snafu};
use std::path::PathBuf;

use crate::domain::model::error::Error as ModelError;
use crate::domain::ports::primary::configure_backend::ConfigureBackend;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Config Merge Error: {} [{}]", details, source))]
    ConfigMerge {
        details: String,
        source: config::ConfigError,
    },

    #[snafu(display("IO Error: {} [{}]", source, details))]
    InvalidIO {
        details: String,
        source: std::io::Error,
    },

    #[snafu(display("Backend Error: {}", source))]
    Backend { source: ModelError },
}

#[derive(Debug, Clone, Copy)]
pub enum Template {
    Index,
    Component,
}

pub async fn import<C: Clone + ConfigureBackend>(
    client: C,
    path: PathBuf,
    template_type: Template,
) -> Result<(), Error> {
    dir_to_stream(path)
        .await?
        .try_for_each(|template| {
            let template_name = template
                .file_stem()
                .expect("file stem")
                .to_str()
                .expect("template_name")
                .to_string();
            let client = client.clone();
            async move {
                let config = config::Config::default()
                    .set_default("elasticsearch.name", template_name)
                    .unwrap()
                    .merge(config::File::new(
                        template.to_str().unwrap(),
                        config::FileFormat::Json,
                    ))
                    .context(ConfigMerge {
                        details: format!(
                            "could not read template configuration from {}",
                            template.display()
                        ),
                    })?
                    .clone();

                match template_type {
                    Template::Component => {
                        client
                            .configure(String::from("create component template"), config)
                            .context(Backend)
                            .await
                    }
                    Template::Index => {
                        client
                            .configure(String::from("create index template"), config)
                            .context(Backend)
                            .await
                    }
                }
            }
        })
        .await
}

// Turns a directory into a Stream of PathBuf
async fn dir_to_stream(
    dir: PathBuf,
) -> Result<impl Stream<Item = Result<PathBuf, Error>> + Unpin, Error> {
    let entries = tokio::fs::read_dir(dir.as_path())
        .await
        .context(InvalidIO {
            details: format!("{}", dir.display()),
        })?;

    let stream = tokio_stream::wrappers::ReadDirStream::new(entries);

    Ok(stream.map_ok(|entry| entry.path()).context(InvalidIO {
        details: "could not get path",
    }))
}