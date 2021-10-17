// Copyright © 2016, Canal TP and/or its affiliates. All rights reserved.
//
// This file is part of Navitia,
//     the software to build cool stuff with public transport.
//
// Hope you'll enjoy and contribute to this project,
//     powered by Canal TP (www.canaltp.fr).
// Help us simplify mobility and open public transport:
//     a non ending quest to the responsive locomotion way of traveling!
//
// LICENCE: This program is free software; you can redistribute it
// and/or modify it under the terms of the GNU Affero General Public
// License as published by the Free Software Foundation, either
// version 3 of the License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful, but
// WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU
// Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public
// License along with this program. If not, see
// <http://www.gnu.org/licenses/>.
//
// Stay tuned using
// twitter @navitia
// IRC #navitia on freenode
// https://groups.google.com/d/forum/navitia
// www.navitia.io

use common::config::load_es_config_for;
use mimir2::adapters::secondary::elasticsearch;
use mimir2::domain::ports::secondary::remote::Remote;
use mimirsbrunn::settings::cosmogony2mimir as settings;
use places::admin::Admin;
use slog_scope::info;
use snafu::{ResultExt, Snafu};
use structopt::StructOpt;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Settings (Configuration or CLI) Error: {}", source))]
    Settings { source: settings::Error },

    #[snafu(display("Elasticsearch Connection Pool {}", source))]
    ElasticsearchPool {
        source: elasticsearch::remote::Error,
    },

    #[snafu(display("Elasticsearch Connection Pool {}", source))]
    ElasticsearchConnection {
        source: mimir2::domain::ports::secondary::remote::Error,
    },

    #[snafu(display("Execution Error {}", source))]
    Execution { source: Box<dyn std::error::Error> },

    #[snafu(display("Configuration Error {}", source))]
    Configuration { source: common::config::Error },

    #[snafu(display("Import Error {}", source))]
    Import { source: mimirsbrunn::admin::Error },
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let opts = settings::Opts::from_args();
    match opts.cmd {
        settings::Command::Run => mimirsbrunn::utils::launch::wrapped_launch_async(Box::new(run))
            .await
            .context(Execution),
        settings::Command::Config => {
            mimirsbrunn::utils::launch::wrapped_launch_async(Box::new(config))
                .await
                .context(Execution)
        }
    }
}

async fn config(opts: settings::Opts) -> Result<(), Box<dyn std::error::Error>> {
    let settings = settings::Settings::new(&opts).map_err(Box::new)?;
    println!("{}", serde_json::to_string_pretty(&settings).unwrap());
    Ok(())
}

async fn run(opts: settings::Opts) -> Result<(), Box<dyn std::error::Error>> {
    info!("importing BANO into Mimir");
    let input = opts.input.clone(); // we save the input, because opts will be consumed by settings.

    let settings = &settings::Settings::new(&opts)
        .context(Settings)
        .map_err(Box::new)?;

    let pool = elasticsearch::remote::connection_pool_url(&settings.elasticsearch.url)
        .await
        .context(ElasticsearchPool)
        .map_err(Box::new)?;

    let client = pool
        .conn(
            settings.elasticsearch.timeout,
            &settings.elasticsearch.version_req,
        )
        .await
        .context(ElasticsearchConnection)
        .map_err(Box::new)?;

    let config = load_es_config_for::<Admin>(
        opts.settings
            .iter()
            .filter_map(|s| {
                if s.starts_with("elasticsearch.admin") {
                    Some(s.to_string())
                } else {
                    None
                }
            })
            .collect(),
        settings.container.dataset.clone(),
    )
    .context(Configuration)
    .map_err(Box::new)?;

    mimirsbrunn::admin::index_cosmogony(&input, settings.langs.clone(), config, &client)
        .await
        .context(Import)
        .map_err(|err| Box::new(err) as Box<dyn snafu::Error>) // TODO Investigate why the need to cast?
}

#[cfg(test)]
mod tests {
    use approx::assert_relative_eq;
    use futures::TryStreamExt;
    use serial_test::serial;

    use super::*;
    //use common::document::ContainerDocument;
    //use mimir2::domain::model::query::Query;
    //use mimir2::domain::ports::primary::list_documents::ListDocuments;
    //use mimir2::domain::ports::primary::search_documents::SearchDocuments;
    //use mimir2::{adapters::secondary::elasticsearch::remote, utils::docker};
    //use places::admin::Admin;
    //use places::Place;

    //use super::*;
    //use futures::TryStreamExt;
    //use mimir2::domain::ports::primary::list_documents::ListDocuments;
    use mimir2::domain::ports::primary::list_documents::ListDocuments;
    use mimir2::{adapters::secondary::elasticsearch::remote, utils::docker};
    //use mimirsbrunn::settings::cosmogony2mimir as settings;
    //use places::addr::Addr;

    #[tokio::test]
    #[serial]
    async fn should_return_an_error_when_given_an_invalid_es_url() {
        docker::initialize()
            .await
            .expect("elasticsearch docker initialization");
        let opts = settings::Opts {
            config_dir: [env!("CARGO_MANIFEST_DIR"), "config"].iter().collect(),
            run_mode: Some("testing".to_string()),
            settings: vec![String::from("elasticsearch.url='http://example.com:demo'")],
            input: [
                env!("CARGO_MANIFEST_DIR"),
                "tests",
                "fixtures",
                "cosmogony.json",
            ]
            .iter()
            .collect(),
            cmd: settings::Command::Run,
        };

        let res = mimirsbrunn::utils::launch::launch_async_args(run, opts).await;
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("Invalid Elasticsearch URL"));
    }

    #[tokio::test]
    #[serial]
    async fn should_return_an_error_when_given_an_url_not_es() {
        docker::initialize()
            .await
            .expect("elasticsearch docker initialization");
        let opts = settings::Opts {
            config_dir: [env!("CARGO_MANIFEST_DIR"), "config"].iter().collect(),
            run_mode: Some("testing".to_string()),
            settings: vec![String::from("elasticsearch.url='http://no-es.test'")],
            input: [
                env!("CARGO_MANIFEST_DIR"),
                "tests",
                "fixtures",
                "cosmogony.json",
            ]
            .iter()
            .collect(),
            cmd: settings::Command::Run,
        };

        let res = mimirsbrunn::utils::launch::launch_async_args(run, opts).await;
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("Elasticsearch Connection Error"));
    }

    #[tokio::test]
    #[serial]
    async fn should_return_an_error_when_given_an_invalid_path_for_config() {
        docker::initialize()
            .await
            .expect("elasticsearch docker initialization");

        let opts = settings::Opts {
            config_dir: [env!("CARGO_MANIFEST_DIR")].iter().collect(), // Not a valid config base dir
            run_mode: Some("testing".to_string()),
            settings: vec![],
            input: [
                env!("CARGO_MANIFEST_DIR"),
                "tests",
                "fixtures",
                "cosmogony.json",
            ]
            .iter()
            .collect(),
            cmd: settings::Command::Run,
        };

        let res = mimirsbrunn::utils::launch::launch_async_args(run, opts).await;

        assert!(res.unwrap_err().to_string().contains("Config Source Error"));
    }

    #[tokio::test]
    #[serial]
    async fn should_return_an_error_when_given_an_invalid_path_for_input() {
        docker::initialize()
            .await
            .expect("elasticsearch docker initialization");

        let opts = settings::Opts {
            config_dir: [env!("CARGO_MANIFEST_DIR"), "config"].iter().collect(), // Not a valid config base dir
            run_mode: Some("testing".to_string()),
            settings: vec![],
            input: [env!("CARGO_MANIFEST_DIR"), "invalid.jsonl.gz"]
                .iter()
                .collect(),
            cmd: settings::Command::Run,
        };

        let res = mimirsbrunn::utils::launch::launch_async_args(run, opts).await;

        assert!(res
            .unwrap_err()
            .to_string()
            .contains("Cosmogony Error: could not read zones from file"));
    }

    #[tokio::test]
    #[serial]
    async fn should_correctly_index_a_small_cosmogony_file() {
        docker::initialize()
            .await
            .expect("elasticsearch docker initialization");

        let opts = settings::Opts {
            config_dir: [env!("CARGO_MANIFEST_DIR"), "config"].iter().collect(), // Not a valid config base dir
            run_mode: Some("testing".to_string()),
            settings: vec![],
            input: [
                env!("CARGO_MANIFEST_DIR"),
                "tests",
                "fixtures",
                "cosmogony",
                "bretagne.small.jsonl.gz",
            ]
            .iter()
            .collect(),
            cmd: settings::Command::Run,
        };

        let _res = mimirsbrunn::utils::launch::launch_async_args(run, opts).await;

        // Now we query the index we just created. Since it's a small cosmogony file with few entries,
        // we'll just list all the documents in the index, and check them.
        let config = docker::ConfigElasticsearchTesting::default();
        let pool = remote::connection_pool_url(&config.url)
            .await
            .expect("Elasticsearch Connection Pool");

        let client = pool
            .conn(config.timeout, &config.version_req)
            .await
            .expect("Elasticsearch Connection Established");

        let admins: Vec<Admin> = client
            .list_documents()
            .await
            .unwrap()
            .try_collect()
            .await
            .unwrap();

        assert_eq!(admins.len(), 8);
        assert!(admins.iter().all(|admin| admin.boundary.is_some()));
        assert!(admins.iter().all(|admin| admin.coord.is_valid()));
    }

    #[tokio::test]
    #[serial]
    async fn should_correctly_index_cosmogony_with_langs() {
        docker::initialize()
            .await
            .expect("elasticsearch docker initialization");

        let opts = settings::Opts {
            config_dir: [env!("CARGO_MANIFEST_DIR"), "config"].iter().collect(), // Not a valid config base dir
            run_mode: Some("testing".to_string()),
            settings: vec![String::from("langs=['fr', 'en']")],
            input: [
                env!("CARGO_MANIFEST_DIR"),
                "tests",
                "fixtures",
                "cosmogony",
                "bretagne.small.jsonl.gz",
            ]
            .iter()
            .collect(),
            cmd: settings::Command::Run,
        };

        let _res = mimirsbrunn::utils::launch::launch_async_args(run, opts).await;

        // Now we query the index we just created. Since it's a small cosmogony file with few entries,
        // we'll just list all the documents in the index, and check them.
        let config = docker::ConfigElasticsearchTesting::default();
        let pool = remote::connection_pool_url(&config.url)
            .await
            .expect("Elasticsearch Connection Pool");

        let client = pool
            .conn(config.timeout, &config.version_req)
            .await
            .expect("Elasticsearch Connection Established");

        let admins: Vec<Admin> = client
            .list_documents()
            .await
            .unwrap()
            .try_collect()
            .await
            .unwrap();

        let brittany = admins.iter().find(|a| a.name == "Bretagne").unwrap();
        assert_eq!(brittany.names.get("fr"), Some("Bretagne"));
        assert_eq!(brittany.names.get("en"), Some("Brittany"));
        assert_eq!(brittany.labels.get("en"), Some("Brittany"));
    }

    #[tokio::test]
    #[serial]
    async fn should_index_cosmogony_with_correct_values() {
        docker::initialize()
            .await
            .expect("elasticsearch docker initialization");

        let opts = settings::Opts {
            config_dir: [env!("CARGO_MANIFEST_DIR"), "config"].iter().collect(), // Not a valid config base dir
            run_mode: Some("testing".to_string()),
            settings: vec![String::from("langs=['fr', 'en']")],
            input: [
                env!("CARGO_MANIFEST_DIR"),
                "tests",
                "fixtures",
                "cosmogony",
                "bretagne.small.jsonl.gz",
            ]
            .iter()
            .collect(),
            cmd: settings::Command::Run,
        };

        let _res = mimirsbrunn::utils::launch::launch_async_args(run, opts).await;

        // Now we query the index we just created. Since it's a small cosmogony file with few entries,
        // we'll just list all the documents in the index, and check them.
        let config = docker::ConfigElasticsearchTesting::default();
        let pool = remote::connection_pool_url(&config.url)
            .await
            .expect("Elasticsearch Connection Pool");

        let client = pool
            .conn(config.timeout, &config.version_req)
            .await
            .expect("Elasticsearch Connection Established");

        let admins: Vec<Admin> = client
            .list_documents()
            .await
            .unwrap()
            .try_collect()
            .await
            .unwrap();

        let brittany = admins.iter().find(|a| a.name == "Bretagne").unwrap();
        assert_eq!(brittany.id, "admin:osm:relation:102740");
        assert_eq!(brittany.zone_type, Some(cosmogony::ZoneType::State));
        assert_relative_eq!(brittany.weight, 0.002_298, epsilon = 1e-6);
        assert_eq!(
            brittany.codes,
            vec![
                ("ISO3166-2", "FR-BRE"),
                ("ref:INSEE", "53"),
                ("ref:nuts", "FRH;FRH0"),
                ("ref:nuts:1", "FRH"),
                ("ref:nuts:2", "FRH0"),
                ("wikidata", "Q12130")
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
        )
    }
}
