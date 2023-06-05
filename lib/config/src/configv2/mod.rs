#![allow(dead_code)]

use self::templating::{Bool, EnvsOnly, False, MissingEnvVar, Template, True};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap},
    hash::Hash,
};
use thiserror::Error;

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

impl VarValue<True> {
    fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Terminal(..) => None,
            Self::Nested(v) => v.get(key),
        }
    }

    fn finish(&self) -> Option<&VarTerminal<True>> {
        match self {
            Self::Terminal(v) => Some(v),
            Self::Nested(..) => None,
        }
    }
}

fn insert_env_vars(
    v: Vars<False>,
    evars: &BTreeMap<String, String>,
) -> Result<Vars<True>, MissingEnvVar> {
    v.into_iter()
        .map(|(k, v)| Ok((k, v.insert_env_vars(evars)?)))
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
    fn insert_env_vars(
        self,
        evars: &BTreeMap<String, String>,
    ) -> Result<VarTerminal<True>, MissingEnvVar> {
        match self {
            Self::Num(n) => Ok(VarTerminal::Num(n)),
            Self::Bool(b) => Ok(VarTerminal::Bool(b)),
            Self::Str(template) => template.insert_env_vars(evars).map(VarTerminal::Str),
        }
    }
}

impl VarValue<False> {
    fn insert_env_vars(
        self,
        evars: &BTreeMap<String, String>,
    ) -> Result<VarValue<True>, MissingEnvVar> {
        match self {
            Self::Nested(v) => insert_env_vars(v, evars).map(VarValue::Nested),
            Self::Terminal(t) => t.insert_env_vars(evars).map(VarValue::Terminal),
        }
    }
}

#[derive(Debug, Error)]
pub enum LoadTestGenError {
    #[error("error parsing yaml: {0}")]
    YamlParse(#[from] serde_yaml::Error),
    #[error("{0}")]
    MissingEnvVar(#[from] MissingEnvVar),
}

impl LoadTest<True, True> {
    pub fn from_yaml(yaml: &str) -> Result<Self, LoadTestGenError> {
        let pre_envs: LoadTest<False, False> = serde_yaml::from_str(yaml)?;
        let env_vars = std::env::vars().collect::<BTreeMap<_, _>>();
        let pre_vars = pre_envs.insert_env_vars(&env_vars)?;
        todo!()
    }
}

impl LoadTest<False, False> {
    fn insert_env_vars(
        self,
        evars: &BTreeMap<String, String>,
    ) -> Result<LoadTest<False, True>, MissingEnvVar> {
        let Self {
            config,
            load_pattern,
            vars,
            providers,
            loggers,
            endpoints,
        } = self;
        Ok(LoadTest {
            config,
            load_pattern,
            vars: insert_env_vars(vars, evars)?,
            providers,
            loggers,
            endpoints,
        })
    }
}

#[derive(Debug, Error)]
enum VarsError {
    #[error("var at path \"{0}\" not found")]
    VarNotFound(String),
    #[error("resulting string \"{from}\", was not a valid {typename} ({error})")]
    InvalidString {
        typename: &'static str,
        from: String,
        #[source]
        error: Box<dyn std::error::Error>,
    },
}

trait PropagateVars {
    // should be same generic type, but with VD as True
    type Residual;

    fn insert_vars(self, vars: &VarValue<True>) -> Result<Self::Residual, VarsError>;
}

impl PropagateVars for LoadTest<False, True> {
    type Residual = LoadTest<True, True>;

    fn insert_vars(self, vars: &VarValue<True>) -> Result<Self::Residual, VarsError> {
        let Self {
            config,
            load_pattern,
            vars,
            providers,
            loggers,
            endpoints,
        } = self;

        todo!()
    }
}

impl<K, V> PropagateVars for BTreeMap<K, V>
where
    K: Ord,
    V: PropagateVars,
{
    type Residual = BTreeMap<K, V::Residual>;

    fn insert_vars(self, vars: &VarValue<True>) -> Result<Self::Residual, VarsError> {
        self.into_iter()
            .map(|(k, v)| Ok((k, v.insert_vars(vars)?)))
            .collect()
    }
}

impl<K, V> PropagateVars for HashMap<K, V>
where
    K: Eq + Hash,
    V: PropagateVars,
{
    type Residual = HashMap<K, V::Residual>;

    fn insert_vars(self, vars: &VarValue<True>) -> Result<Self::Residual, VarsError> {
        self.into_iter()
            .map(|(k, v)| Ok((k, v.insert_vars(vars)?)))
            .collect()
    }
}

impl<T> PropagateVars for Vec<T>
where
    T: PropagateVars,
{
    type Residual = Vec<T::Residual>;

    fn insert_vars(self, vars: &VarValue<True>) -> Result<Self::Residual, VarsError> {
        self.into_iter().map(|x| x.insert_vars(vars)).collect()
    }
}
