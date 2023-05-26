#![allow(dead_code)]

use super::OrTemplated;
use serde::Deserialize;

mod file;
mod list;
mod range;

enum ProviderType {
    File(file::FileProvider),
    Response {
        auto_return: ProviderSend,
        buffer: BufferLimit,
        unique: bool,
    },
    List(list::ListProvider),
    Range(range::RangeProvider),
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum ProviderSend {
    Block,
    Force,
    IfNotFull,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Default, Clone, Copy)]
#[serde(from = "BufferLimitTmp")]
enum BufferLimit {
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
        // provider send
        let ps: ProviderSend = from_yaml("!block").unwrap();
        assert_eq!(ps, ProviderSend::Block);
        let ps: ProviderSend = from_yaml("!force").unwrap();
        assert_eq!(ps, ProviderSend::Force);
        let ps: ProviderSend = from_yaml("!if_not_full").unwrap();
        assert_eq!(ps, ProviderSend::IfNotFull);

        // buffer limit
        let bl: BufferLimit = from_yaml("43").unwrap();
        assert_eq!(bl, BufferLimit::Limit(43));
        let bl: BufferLimit = from_yaml("auto").unwrap();
        assert_eq!(bl, BufferLimit::Auto);
    }
}
