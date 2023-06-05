#![allow(dead_code)]

use serde::Deserialize;
use std::collections::BTreeMap;

pub mod config;
pub mod endpoints;
pub mod load_pattern;
pub mod loggers;
pub mod providers;
pub mod templating;

pub mod common;

#[derive(Debug, Deserialize)]
pub struct LoadTest {
    config: config::Config,
    load_pattern: load_pattern::LoadPattern,
    //vars: templating::Vars,
    providers: BTreeMap<String, providers::ProviderType>,
    loggers: BTreeMap<String, loggers::Logger>,
    endpoints: BTreeMap<String, endpoints::Endpoint>,
}
