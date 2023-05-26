#![allow(dead_code)]

use super::{
    common::{Duration, Headers},
    load_pattern::LoadPattern,
    OrTemplated,
};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap},
    convert::TryFrom,
    str::FromStr,
};

#[derive(Deserialize)]
pub struct Endpoint {
    declare: Option<()>,
    headers: Headers,
    body: EndPointBody,
    load_pattern: LoadPattern,
    method: (),
    peak_load: OrTemplated<HitsPerMinute>,
    #[serde(default)]
    tags: BTreeMap<String, OrTemplated<String>>,
    url: OrTemplated<String>,
    provides: (),
    on_demand: bool,
    logs: (),
    max_parallel_requests: Option<u64>,
    #[serde(default)]
    no_auto_returns: bool,
    request_timeout: Option<Duration>,
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

#[derive(Debug, Deserialize, PartialEq, PartialOrd)]
#[serde(try_from = "&str")]
struct HitsPerMinute(f64);

impl FromStr for HitsPerMinute {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        static REGEX: Lazy<Regex> = Lazy::new(|| {
            Regex::new(r"^(?i)(\d+(?:\.\d+)?)\s*hp([ms])$").expect("should be a valid regex")
        });
        let captures = REGEX.captures(s).ok_or("invalid")?;
        // None of this should ever panic due to how the regex is formed.
        let [n, tag] = (1..=2)
            .map(|i| captures.get(i).unwrap().as_str())
            .collect::<Vec<_>>()[..] else {
                unreachable!()
            };

        let n: f64 = n.parse().unwrap();
        // Highly doubt anyone will do this, but you never know.
        let n = n.is_finite().then_some(n).ok_or("hits per is too big")?;
        Ok(Self(
            n * match tag {
                "m" => 1.0,
                "s" => 60.0,
                _ => unreachable!("regex should only catch 'h' or 'm'"),
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
}
