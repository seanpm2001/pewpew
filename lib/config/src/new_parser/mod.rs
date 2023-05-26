pub mod templating;
pub use templating::OrTemplated;

pub mod config;
pub mod endpoints;
pub mod load_pattern;
pub mod loggers;
pub mod providers;

pub mod common {
    use super::OrTemplated;
    use serde::Deserialize;
    use std::{collections::BTreeMap, convert::TryFrom, str::FromStr, time::Duration as SDur};

    pub type Headers = BTreeMap<String, OrTemplated<String>>;

    /// Newtype wrapper around [`std::time::Duration`] that allows implementing the needed traits.
    #[derive(Debug, Deserialize, PartialEq, Clone, Copy, Eq)]
    #[serde(try_from = "&str")]
    pub struct Duration(SDur);

    impl FromStr for Duration {
        // TODO: better error reporting for Duration
        type Err = &'static str;

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            crate::duration_from_string(s.to_owned())
                .map_err(|_| "invalid duration")
                .map(Self)
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

    #[cfg(test)]
    mod tests {
        use super::super::OrTemplated;
        use super::*;
        use serde_yaml::from_str as from_yaml;

        #[test]
        fn basic_test_duration() {
            // Durations
            type OTD = OrTemplated<Duration>;
            let dur = from_yaml::<OTD>("1m").unwrap();
            assert_eq!(dur.try_get(), Some(&Duration::from_secs(60)));
            let dur = from_yaml::<OTD>("3h1m22s").unwrap();
            assert_eq!(
                dur.try_get(),
                Some(&Duration::from_secs(3 * 60 * 60 + 60 + 22))
            );
            let dur = from_yaml::<OTD>("5 hrs").unwrap();
            assert_eq!(dur.try_get(), Some(&Duration::from_secs(5 * 60 * 60)));
        }
    }
}
