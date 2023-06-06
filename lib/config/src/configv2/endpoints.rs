#![allow(dead_code)]

use super::{
    common::{Duration, Headers},
    load_pattern::LoadPattern,
    templating::{Bool, False, Regular, Template, True, VarsOnly},
    PropagateVars,
};
use derive_more::{Deref, FromStr};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap},
    convert::TryFrom,
    str::FromStr,
};
use thiserror::Error;

#[derive(Debug, Deserialize)]
pub struct Endpoint<VD: Bool> {
    #[serde(default)]
    declare: BTreeMap<String, String>, // expressions are still Strings for now
    #[serde(default = "BTreeMap::new")]
    headers: Headers<VD>,
    body: Option<EndPointBody<VD>>,
    #[serde(bound = "LoadPattern<VD>: serde::de::DeserializeOwned")]
    load_pattern: Option<LoadPattern<VD>>,
    #[serde(default)]
    method: Method,
    peak_load: Option<Template<HitsPerMinute, VarsOnly, VD>>,
    #[serde(default = "BTreeMap::new")]
    tags: BTreeMap<String, Template<String, Regular, VD>>,
    url: Template<String, Regular, VD>,
    #[serde(default)]
    provides: BTreeMap<String, EndpointProvides>,
    // book says optional, check what the behavior should be and if this
    // should default
    on_demand: Option<bool>,
    #[serde(default)]
    logs: BTreeMap<String, EndpointLogs>,
    max_parallel_requests: Option<u64>,
    #[serde(default)]
    no_auto_returns: bool,
    request_timeout: Option<Duration>,
}

impl PropagateVars for Endpoint<False> {
    type Residual = Endpoint<True>;

    fn insert_vars(self, vars: &super::VarValue<True>) -> Result<Self::Residual, super::VarsError> {
        Ok(Endpoint {
            declare: self.declare,
            headers: self.headers.insert_vars(vars)?,
            body: self.body.insert_vars(vars)?,
            load_pattern: self.load_pattern.insert_vars(vars)?,
            method: self.method,
            peak_load: self.peak_load.insert_vars(vars)?,
            tags: self.tags.insert_vars(vars)?,
            url: self.url.insert_vars(vars)?,
            provides: self.provides,
            on_demand: self.on_demand,
            logs: self.logs,
            max_parallel_requests: self.max_parallel_requests,
            no_auto_returns: self.no_auto_returns,
            request_timeout: self.request_timeout,
        })
    }
}

/// Newtype wrapper around [`http::Method`] for implementing [`serde::Deserialize`].
#[derive(Deserialize, Debug, Default, Deref, FromStr, PartialEq, Eq)]
#[serde(try_from = "&str")]
struct Method(http::Method);

impl TryFrom<&str> for Method {
    type Error = <Self as FromStr>::Err;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        value.parse()
    }
}

// Error("deserializing nested enum in EndPointBody::str from YAML is not supported yet", line: 1, column: 1)
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "type", content = "content")]
enum EndPointBody<VD: Bool> {
    #[serde(rename = "str")]
    String(Template<String, Regular, VD>),
    File(Template<String, Regular, VD>),
    Multipart(HashMap<String, MultiPartBodySection<VD>>),
}

impl PropagateVars for EndPointBody<False> {
    type Residual = EndPointBody<True>;

    fn insert_vars(self, vars: &super::VarValue<True>) -> Result<Self::Residual, super::VarsError> {
        use EndPointBody::*;
        match self {
            String(s) => s.insert_vars(vars).map(String),
            File(f) => f.insert_vars(vars).map(File),
            Multipart(mp) => mp.insert_vars(vars).map(Multipart),
        }
    }
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct MultiPartBodySection<VD: Bool> {
    #[serde(default = "BTreeMap::new")]
    headers: Headers<VD>,
    body: EndPointBody<VD>,
}

impl PropagateVars for MultiPartBodySection<False> {
    type Residual = MultiPartBodySection<True>;

    fn insert_vars(self, vars: &super::VarValue<True>) -> Result<Self::Residual, super::VarsError> {
        let Self { headers, body } = self;
        Ok(MultiPartBodySection {
            headers: headers.insert_vars(vars)?,
            body: body.insert_vars(vars)?,
        })
    }
}

#[derive(Debug, Deserialize, PartialEq, PartialOrd, Deref)]
#[serde(try_from = "&str")]
struct HitsPerMinute(f64);

#[derive(Debug, Error, PartialEq, Eq)]
enum ParseHitsPerError {
    #[error("invalid hits per minute")]
    Invalid,
    #[error("hits per minute value too large")]
    TooBig,
}

impl FromStr for HitsPerMinute {
    type Err = ParseHitsPerError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use crate::shared::Per;
        let (n, tag) = crate::shared::get_hits_per(s).ok_or(ParseHitsPerError::Invalid)?;
        // Highly doubt anyone will do this, but you never know.
        let n = n
            .is_finite()
            .then_some(n)
            .ok_or(ParseHitsPerError::TooBig)?;
        Ok(Self(
            n * match tag {
                Per::Minute => 1.0,
                Per::Second => 60.0,
            },
        ))
    }
}

impl TryFrom<&str> for HitsPerMinute {
    type Error = <Self as FromStr>::Err;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        value.parse()
    }
}

#[derive(Debug, Deserialize)]
struct EndpointProvides {
    select: String, // Expressions are Strings for now
    for_each: String,
    r#where: String,
    send: super::common::ProviderSend,
}

#[derive(Debug, Deserialize)]
struct EndpointLogs {
    select: String, // Expressions are Strings for now
    for_each: String,
    r#where: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configv2::False;
    use serde_yaml::from_str as from_yaml;

    #[test]
    fn test_hits_per_minute() {
        assert_eq!("15hpm".parse(), Ok(HitsPerMinute(15.0)));
        assert_eq!("22 hpm".parse(), Ok(HitsPerMinute(22.0)));
        assert_eq!("1hps".parse(), Ok(HitsPerMinute(60.0)));

        assert_eq!("1.5 hpm".parse(), Ok(HitsPerMinute(1.5)));
        assert_eq!("0.5 hps".parse(), Ok(HitsPerMinute(30.0)));

        // Allowed, but should it be?
        assert_eq!("0hps".parse(), Ok(HitsPerMinute(0.0)));

        // Even though these are valid values for parsing a float, the regex won't catch them (and
        // shouldn't)
        assert_eq!(
            "NaN hpm".parse::<HitsPerMinute>(),
            Err(super::ParseHitsPerError::Invalid)
        );
        assert_eq!(
            "infinity hpm".parse::<HitsPerMinute>(),
            Err(super::ParseHitsPerError::Invalid)
        );
        assert_eq!(
            "-3.0 hpm".parse::<HitsPerMinute>(),
            Err(super::ParseHitsPerError::Invalid)
        );
    }

    #[test]
    fn test_body() {
        let EndPointBody::<False>::String(body) = from_yaml("type: str\ncontent: !l my text").unwrap() else {
            panic!("was not template variant")
        };
        assert_eq!(
            body,
            Template::Literal {
                value: "my text".to_owned()
            }
        );

        let EndPointBody::<False>::File(file) = from_yaml("type: file\ncontent: !l body.txt").unwrap() else {
            panic!("was not file variant")
        };
        assert_eq!(
            file,
            Template::Literal {
                value: "body.txt".to_owned()
            }
        );

        static TEST: &str = r#"
type: multipart
content:
  foo:
    headers:
      Content-Type: !l image/jpeg
    body:
      type: file
      content: !l foo.jpg
  bar:
    body:
      type: str
      content: !l some text"#;
        let EndPointBody::<False>::Multipart(multipart) = from_yaml(TEST).unwrap() else {
            panic!("was not multipart variant")
        };
        assert_eq!(multipart.len(), 2);
        assert_eq!(
            multipart["foo"],
            MultiPartBodySection {
                headers: [(
                    "Content-Type".to_owned(),
                    Template::Literal {
                        value: "image/jpeg".to_owned()
                    }
                )]
                .into(),
                body: EndPointBody::File(Template::Literal {
                    value: "foo.jpg".to_owned()
                })
            }
        );
        assert_eq!(
            multipart["bar"],
            MultiPartBodySection {
                headers: Default::default(),
                body: EndPointBody::String(Template::Literal {
                    value: "some text".to_owned()
                })
            }
        );
    }

    #[test]
    fn test_method_default() {
        // The Default impl for the local Method is forwarded to http::Method::default()
        // in current version, that default is GET. This test is to check if that changes between
        // versions.
        assert_eq!(Method::default(), Method(http::Method::GET));
    }

    #[test]
    fn test_method() {
        // The pewpew book does not specify a valid subset, so assuming all should be tested.
        let Method(method) = from_yaml("GET").unwrap();
        assert_eq!(method, http::Method::GET);
        let Method(method) = from_yaml("CONNECT").unwrap();
        assert_eq!(method, http::Method::CONNECT);
        let Method(method) = from_yaml("DELETE").unwrap();
        assert_eq!(method, http::Method::DELETE);
        let Method(method) = from_yaml("HEAD").unwrap();
        assert_eq!(method, http::Method::HEAD);
        let Method(method) = from_yaml("OPTIONS").unwrap();
        assert_eq!(method, http::Method::OPTIONS);
        let Method(method) = from_yaml("PATCH").unwrap();
        assert_eq!(method, http::Method::PATCH);
        let Method(method) = from_yaml("POST").unwrap();
        assert_eq!(method, http::Method::POST);
        let Method(method) = from_yaml("PUT").unwrap();
        assert_eq!(method, http::Method::PUT);
        let Method(method) = from_yaml("TRACE").unwrap();
        assert_eq!(method, http::Method::TRACE);
    }

    #[test]
    fn test_endpoint() {
        static TEST: &str = r#"url: !l example.com"#;
        let Endpoint::<False> {
            declare,
            headers,
            body,
            load_pattern,
            method,
            peak_load,
            tags,
            url,
            provides,
            on_demand,
            logs,
            max_parallel_requests,
            no_auto_returns,
            request_timeout,
        } = from_yaml(TEST).unwrap();
        assert!(declare.is_empty());
        assert!(headers.is_empty());
        assert_eq!(body, None);
        assert_eq!(load_pattern, None);
        assert_eq!(*method, http::Method::GET);
        assert_eq!(peak_load, None);
        assert!(tags.is_empty());
        assert_eq!(
            url,
            Template::Literal {
                value: "example.com".to_owned()
            }
        );
        assert!(provides.is_empty());
        assert_eq!(on_demand, None);
        assert!(logs.is_empty());
        assert_eq!(max_parallel_requests, None);
        assert_eq!(no_auto_returns, false);
        assert_eq!(request_timeout, None);
    }
}
