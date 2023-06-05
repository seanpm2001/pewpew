#![allow(dead_code)]

use serde::Deserialize;
use std::collections::BTreeMap;

use self::templating::{Bool, EnvsOnly, False, Template, True};

pub mod config;
pub mod endpoints;
pub mod load_pattern;
pub mod loggers;
pub mod providers;
pub mod templating;

pub mod common;

#[derive(Debug, Deserialize)]
pub struct LoadTest<VD: Bool, ED: Bool> {
    config: config::Config<VD>,
    #[serde(bound = "load_pattern::LoadPattern<VD>: serde::de::DeserializeOwned")]
    load_pattern: load_pattern::LoadPattern<VD>,
    vars: Vars<ED>,
    providers: BTreeMap<String, providers::ProviderType<VD>>,
    loggers: BTreeMap<String, loggers::Logger<VD>>,
    endpoints: BTreeMap<String, endpoints::Endpoint<VD>>,
}

type Vars<ED> = BTreeMap<String, VarValue<ED>>;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum VarValue<ED: Bool> {
    Nested(Vars<ED>),
    Terminal(VarTerminal<ED>),
}

fn insert_env_vars(v: Vars<False>, evars: &BTreeMap<String, String>) -> Option<Vars<True>> {
    v.into_iter()
        .map(|(k, v)| Some((k, v.insert_env_vars(evars)?)))
        .collect()
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum VarTerminal<ED: Bool> {
    Num(i64),
    Bool(bool),
    Str(Template<String, EnvsOnly, True, ED>),
}

impl VarTerminal<False> {
    fn insert_env_vars(self, evars: &BTreeMap<String, String>) -> Option<VarTerminal<True>> {
        match self {
            Self::Num(n) => Some(VarTerminal::Num(n)),
            Self::Bool(b) => Some(VarTerminal::Bool(b)),
            Self::Str(template) => template.insert_env_vars(evars).map(VarTerminal::Str),
        }
    }
}

impl VarValue<False> {
    fn insert_env_vars(self, evars: &BTreeMap<String, String>) -> Option<VarValue<True>> {
        match self {
            Self::Nested(v) => insert_env_vars(v, evars).map(VarValue::Nested),
            Self::Terminal(t) => t.insert_env_vars(evars).map(VarValue::Terminal),
        }
    }
}

impl LoadTest<True, True> {
    pub fn from_yaml(yaml: &str) -> Result<Self, serde_yaml::Error> {
        let pre_envs: LoadTest<False, False> = serde_yaml::from_str(yaml)?;
        let env_vars = std::env::vars().collect::<BTreeMap<_, _>>();
        let pre_vars = pre_envs.insert_env_vars(&env_vars);
        todo!()
    }
}

impl LoadTest<False, False> {
    fn insert_env_vars(self, evars: &BTreeMap<String, String>) -> Option<LoadTest<False, True>> {
        let Self {
            config,
            load_pattern,
            vars,
            providers,
            loggers,
            endpoints,
        } = self;
        Some(LoadTest {
            config,
            load_pattern,
            vars: insert_env_vars(vars, evars)?,
            providers,
            loggers,
            endpoints,
        })
    }
}

trait PropagateVars {
    // should be same generic type, but with VD as True
    type Residual;

    fn insert_vars(self, vars: &Vars<True>) -> Self::Residual;
}
