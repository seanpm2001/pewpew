#![allow(dead_code)]

use super::common::ProviderSend;
use serde::Deserialize;

mod file;
mod list;
mod range;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderType {
    File(file::FileProvider),
    Response {
        auto_return: Option<ProviderSend>,
        #[serde(default)]
        buffer: BufferLimit,
        #[serde(default)]
        unique: bool,
    },
    List(list::ListProvider),
    Range(range::RangeProvider),
}

#[derive(Debug, Deserialize, PartialEq, Eq, Default, Clone, Copy)]
#[serde(from = "BufferLimitTmp")]
pub enum BufferLimit {
    Limit(u64),
    #[default]
    Auto,
}

impl From<BufferLimitTmp> for BufferLimit {
    fn from(value: BufferLimitTmp) -> Self {
        match value {
            BufferLimitTmp::Limit(x) => Self::Limit(x),
            BufferLimitTmp::Auto(Auto::Auto) => Self::Auto,
        }
    }
}

/// Limit is supposed to be a number or the literal keyword "auto"
/// This slightly redundant setup allows for that, but gets converted into the "real" limit struct
/// after.
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
enum BufferLimitTmp {
    Limit(u64),
    Auto(Auto),
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Auto {
    Auto,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_yaml::from_str as from_yaml;

    #[test]
    fn test_basic_types() {
        // buffer limit
        let bl: BufferLimit = from_yaml("43").unwrap();
        assert_eq!(bl, BufferLimit::Limit(43));
        let bl: BufferLimit = from_yaml("auto").unwrap();
        assert_eq!(bl, BufferLimit::Auto);
    }

    #[test]
    fn test_provider_type_response() {
        static TEST1: &str = "!response";

        let ProviderType::Response {
            auto_return,
            buffer,
            unique,
        } = from_yaml(TEST1).unwrap() else {
            panic!("was not response")
        };
        assert_eq!(auto_return, None);
        assert_eq!(buffer, BufferLimit::Auto);
        assert_eq!(unique, false);

        static TEST2: &str = r#"
!response
  buffer: auto
  auto_return: block
  unique: true
        "#;

        let ProviderType::Response {
            auto_return,
            buffer,
            unique,
        } = from_yaml(TEST2).unwrap() else {
            panic!("was not response")
        };
        assert_eq!(auto_return, Some(ProviderSend::Block));
        assert_eq!(buffer, BufferLimit::Auto);
        assert_eq!(unique, true);
    }

    #[test]
    fn test_provider_type_other() {
        // just one quick check on each type
        // more detailed testing on specific properties should be handled in the dedicated modules

        static TEST_FILE: &str = r##"
!file
  path: !l file.csv
  repeat: true
  unique: true
  auto_return: force
  buffer: 27
  format: !csv
    comment: "#"
    headers: true"##;

        let ProviderType::File(_) = from_yaml(TEST_FILE).unwrap() else {
            panic!("was not file provider")
        };

        static TEST_LIST: &str = r##"
!list
  - a
  - b
        "##;

        let ProviderType::List(_) = from_yaml(TEST_LIST).unwrap() else {
            panic!("was not list provider")
        };

        static TEST_RANGE: &str = r#"
!range
  start: 15
        "#;

        let ProviderType::Range(_) = from_yaml(TEST_RANGE).unwrap() else {
            panic!("was not range")
        };
    }
}
