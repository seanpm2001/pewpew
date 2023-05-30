use super::common::Duration;
use super::OrTemplated;
use itertools::Itertools;
use serde::Deserialize;
use std::{
    convert::{TryFrom, TryInto},
    str::FromStr,
};
use thiserror::Error;

/// Percentage type used for pewpew config files. Percentages can be zero, greater than 100, or
/// fractional, but cannot be negatives, nans, or infinities.
#[derive(Debug, Deserialize, PartialEq, Clone, Copy)]
pub struct Percent(f64);

#[derive(Debug, PartialEq, Eq, Error)]
pub enum PercentErr {
    #[error("missing '%' on the percent")]
    NoPercentSign,
    #[error("invalid float ({0})")]
    InvalidFloat(#[from] std::num::ParseFloatError),
    #[error("negative values not allowed")]
    NegativePercent,
    #[error("abnormal floats (infinity, NaN, etc.) are not valid Percents")]
    AbnormalFloat,
}

impl TryFrom<f64> for Percent {
    type Error = PercentErr;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        use PercentErr::*;

        Ok(value)
            .and_then(|p| {
                // is_normal() checks for nan, inf, subnormals, and 0, but 0 should be allowed
                (p.is_normal() || p == 0.0)
                    .then_some(p)
                    .ok_or(AbnormalFloat)
            })
            .and_then(|p| (p >= 0.0).then_some(p).ok_or(NegativePercent))
            .map(Self)
    }
}

impl FromStr for Percent {
    type Err = PercentErr;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use PercentErr::*;

        let base = s.strip_suffix('%').ok_or(NoPercentSign)?;

        (base.parse::<f64>()? / 100.0).try_into()
    }
}

/// Defines the load pattern of how heavily pewpew should be hitting the endpoints over time.
#[derive(Deserialize, Debug)]
#[serde(from = "Vec<LoadPatternTemp>")]
pub struct LoadPattern(Vec<LoadPatternSingle>);

impl From<Vec<LoadPatternTemp>> for LoadPattern {
    fn from(value: Vec<LoadPatternTemp>) -> Self {
        Self(
            // Dummy value at the start is because `from` defaults to 0 if there is no previous
            vec![LoadPatternTemp::Linear {
                from: None,
                // This is the important part
                to: OrTemplated::new_literal(Percent(0.0)),
                over: "1s".parse().unwrap(),
            }]
            .into_iter()
            .chain(value.into_iter())
            .tuple_windows()
            .map(|(prev, curr)| match curr {
                // if `curr` has no `from` defined, take the `to` value of `prev`
                LoadPatternTemp::Linear { from, to, over } => LoadPatternSingle::Linear {
                    from: from.unwrap_or_else(|| prev.into_end()),
                    to,
                    over,
                },
            })
            .collect_vec(),
        )
    }
}

/// Single segment of a [`LoadPattern`], defining the shape and duration.
#[derive(Debug, Clone)]
pub enum LoadPatternSingle {
    Linear {
        from: OrTemplated<Percent>,
        to: OrTemplated<Percent>,
        over: OrTemplated<Duration>,
    },
}

/// This temporary is used because `from` defaults to the `to` value of the previous, and that
/// cannot be acquired in the initial deserialization from the raw components
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
enum LoadPatternTemp {
    Linear {
        from: Option<OrTemplated<Percent>>,
        to: OrTemplated<Percent>,
        over: OrTemplated<Duration>,
    },
}

impl LoadPatternTemp {
    fn into_end(self) -> OrTemplated<Percent> {
        match self {
            Self::Linear { to, .. } => to,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_yaml::from_str as from_yaml;

    #[test]
    fn test_single_values() {
        // Percents
        type OTP = OrTemplated<Percent>;
        let per = from_yaml::<OTP>("1%").unwrap();
        assert_eq!(per.try_get(), Some(&Percent(0.01)));

        // Test fractional percentages
        // Using a sum of powers of 2 for `to` here to prevent float imprecision.
        let per = from_yaml::<OTP>("106.25%").unwrap();
        assert_eq!(per.try_get(), Some(&Percent(1.0625)));

        // Probably shouldn't, but you can
        let per = from_yaml::<OTP>("1e2%").unwrap();
        assert_eq!(per.try_get(), Some(&Percent(1.0)));

        // Valid floats, but not valid Percents

        // No negatives
        assert_eq!(
            from_yaml::<OTP>("-100%").unwrap_err().to_string(),
            "from str error: negative values not allowed"
        );

        // No infinities, NaNs, or subnormals
        assert_eq!(
            from_yaml::<OTP>("NAN%").unwrap_err().to_string(),
            "from str error: abnormal floats (infinity, NaN, etc.) are not valid Percents"
        );
        assert_eq!(
            from_yaml::<OTP>("infinity%").unwrap_err().to_string(),
            "from str error: abnormal floats (infinity, NaN, etc.) are not valid Percents"
        );
        assert_eq!(
            from_yaml::<OTP>("1e-308%").unwrap_err().to_string(),
            "from str error: abnormal floats (infinity, NaN, etc.) are not valid Percents"
        );

        // Zero is ok though
        let per = from_yaml::<OTP>("0%").unwrap();
        assert_eq!(per.try_get(), Some(&Percent(0.0)));

        // `%` is required
        assert_eq!(
            from_yaml::<OTP>("50").unwrap_err().to_string(),
            "from str error: missing '%' on the percent"
        )
    }

    #[test]
    fn test_single_load_pattern() {
        let LoadPatternTemp::Linear { from, to, over } =
            from_yaml("!linear\n  from: 50%\n  to: 100%\n  over: 5m").unwrap();
        assert_eq!(
            from.as_ref().map(OrTemplated::try_get),
            Some(Some(&Percent(0.5)))
        );
        assert_eq!(to.try_get(), Some(&Percent(1.0)));
        assert_eq!(over.try_get(), Some(&Duration::from_secs(5 * 60)));

        let LoadPatternTemp::Linear { from, to, over } =
            from_yaml("!linear\n  to: 20%\n  over: 1s").unwrap();
        assert!(matches!(from, None));
        assert_eq!(to.try_get(), Some(&Percent(0.2)));
        assert_eq!(over.try_get(), Some(&Duration::from_secs(1)));
    }

    #[test]
    fn test_full_load_pattern() {
        static TEST1: &str = r#"
- !linear
    from: 25%
    to: 100%
    over: 1h
        "#;

        let load = from_yaml::<LoadPattern>(TEST1).unwrap();
        assert_eq!(load.0.len(), 1);
        let LoadPatternSingle::Linear { from, to, over } = load.0[0].clone();
        assert_eq!(from.try_get(), Some(&Percent(0.25)));
        assert_eq!(to.try_get(), Some(&Percent(1.0)));
        assert_eq!(over.try_get(), Some(&Duration::from_secs(60 * 60)));

        static TEST2: &str = r#"
 - !linear
     to: 300%
     over: 5m
        "#;

        let LoadPattern(load) = from_yaml(TEST2).unwrap();
        assert_eq!(load.len(), 1);
        let LoadPatternSingle::Linear { from, to, over } = load[0].clone();
        assert_eq!(from.try_get(), Some(&Percent(0.0)));
        assert_eq!(to.try_get(), Some(&Percent(3.0)));
        assert_eq!(over.try_get(), Some(&Duration::from_secs(5 * 60)));

        static TEST3: &str = r#"
 - !linear
     to: 62.5%
     over: 59s
 - !linear
     to: 87.5%
     over: 22s
        "#;

        let LoadPattern(load) = from_yaml(TEST3).unwrap();
        let LoadPatternSingle::Linear { from, to, over } = load[0].clone();
        assert_eq!(from.try_get(), Some(&Percent(0.0)));
        assert_eq!(to.try_get(), Some(&Percent(0.625)));
        assert_eq!(over.try_get(), Some(&Duration::from_secs(59)));

        let LoadPatternSingle::Linear { from, to, over } = load[1].clone();
        assert_eq!(from.try_get(), Some(&Percent(0.625)));
        assert_eq!(to.try_get(), Some(&Percent(0.875)));
        assert_eq!(over.try_get(), Some(&Duration::from_secs(22)));
    }
}