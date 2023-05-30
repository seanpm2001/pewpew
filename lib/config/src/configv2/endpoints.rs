#![allow(dead_code)]

use super::{
    common::{Duration, Headers},
    load_pattern::LoadPattern,
    OrTemplated,
};
use derive_more::{Deref, FromStr};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap},
    convert::TryFrom,
    str::FromStr,
};

#[derive(Debug, Deserialize)]
pub struct Endpoint {
    #[serde(default)]
    declare: BTreeMap<String, String>, // expressions are still Strings for now
    #[serde(default)]
    headers: Headers,
    body: Option<EndPointBody>,
    load_pattern: Option<LoadPattern>,
    #[serde(default)]
    method: Method,
    peak_load: Option<OrTemplated<HitsPerMinute>>,
    #[serde(default)]
    tags: BTreeMap<String, OrTemplated<String>>,
    url: OrTemplated<String>,
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

// maybe make this one a Tmp, and the real one just has three tuple variants
// or turn the whole thing into a type-tagged, and get rid of the just a string deserialization
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
enum EndPointBody {
    Template(OrTemplated<String>),
    File {
        file: OrTemplated<String>,
    },
    Multipart {
        multipart: HashMap<String, MultiPartBodySection>,
    },
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct MultiPartBodySection {
    #[serde(default)]
    headers: Headers,
    body: EndPointBody,
}

#[derive(Debug, Deserialize, PartialEq, PartialOrd, Deref)]
#[serde(try_from = "&str")]
struct HitsPerMinute(f64);

impl FromStr for HitsPerMinute {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use crate::shared::Per;
        let (n, tag) = crate::shared::get_hits_per(s).ok_or("invalid")?;
        // Highly doubt anyone will do this, but you never know.
        let n = n.is_finite().then_some(n).ok_or("hits per is too big")?;
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
        assert_eq!("NaN hpm".parse::<HitsPerMinute>(), Err("invalid"));
        assert_eq!("infinity hpm".parse::<HitsPerMinute>(), Err("invalid"));
        assert_eq!("-3.0 hpm".parse::<HitsPerMinute>(), Err("invalid"));
    }

    #[test]
    fn test_body() {
        let EndPointBody::Template(body) = from_yaml("my text").unwrap() else {
            panic!("was not template variant")
        };
        assert_eq!(body, OrTemplated::new_literal("my text".to_owned()));

        let EndPointBody::File { file } = from_yaml("file: body.txt").unwrap() else {
            panic!("was not file variant")
        };
        assert_eq!(file, OrTemplated::new_literal("body.txt".to_owned()));

        static TEST: &str = r#"
multipart:
  foo:
    headers:
      Content-Type: image/jpeg
    body:
      file: foo.jpg
  bar:
    body: some text"#;
        let EndPointBody::Multipart { multipart } = from_yaml(TEST).unwrap() else {
            panic!("was not multipart variant")
        };
        assert_eq!(multipart.len(), 2);
        assert_eq!(
            multipart["foo"],
            MultiPartBodySection {
                headers: [(
                    "Content-Type".to_owned(),
                    OrTemplated::new_literal("image/jpeg".to_owned())
                )]
                .into(),
                body: EndPointBody::File {
                    file: OrTemplated::new_literal("foo.jpg".to_owned())
                }
            }
        );
        assert_eq!(
            multipart["bar"],
            MultiPartBodySection {
                headers: Default::default(),
                body: EndPointBody::Template(OrTemplated::new_literal("some text".to_owned()))
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
        static TEST: &str = r#"url: example.com"#;
        let Endpoint {
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
        assert_eq!(url, OrTemplated::new_literal("example.com".to_owned()));
        assert!(provides.is_empty());
        assert_eq!(on_demand, None);
        assert!(logs.is_empty());
        assert_eq!(max_parallel_requests, None);
        assert_eq!(no_auto_returns, false);
        assert_eq!(request_timeout, None);
    }
}
