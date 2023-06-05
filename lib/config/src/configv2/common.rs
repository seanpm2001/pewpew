use super::templating::{Regular, Template};
use serde::Deserialize;
use std::{collections::BTreeMap, convert::TryFrom, str::FromStr, time::Duration as SDur};

pub type Headers = BTreeMap<String, Template<String, Regular>>;

/// Newtype wrapper around [`std::time::Duration`] that allows implementing the needed traits.
#[derive(Debug, Deserialize, PartialEq, Clone, Copy, Eq)]
#[serde(try_from = "&str")]
pub struct Duration(SDur);

impl FromStr for Duration {
    // TODO: better error reporting for Duration
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        crate::shared::duration_from_string(s)
            .map(Self)
            .ok_or("invalid duration")
    }
}

impl TryFrom<&str> for Duration {
    type Error = <Self as FromStr>::Err;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl Duration {
    pub fn from_secs(secs: u64) -> Self {
        Self(SDur::from_secs(secs))
    }
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSend {
    Block,
    Force,
    IfNotFull,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_yaml::from_str as from_yaml;

    #[test]
    fn basic_test_duration() {
        assert_eq!("1m".parse(), Ok(Duration::from_secs(60)));
        assert_eq!(
            "3h1m22s".parse(),
            Ok(Duration::from_secs(3 * 60 * 60 + 60 + 22))
        );
        assert_eq!("5hrs".parse(), Ok(Duration::from_secs(5 * 60 * 60)));
    }

    #[test]
    fn basic_test_provider_send() {
        // provider send
        let ps: ProviderSend = from_yaml("!block").unwrap();
        assert_eq!(ps, ProviderSend::Block);
        let ps: ProviderSend = from_yaml("!force").unwrap();
        assert_eq!(ps, ProviderSend::Force);
        let ps: ProviderSend = from_yaml("!if_not_full").unwrap();
        assert_eq!(ps, ProviderSend::IfNotFull);
    }
}
