#![allow(dead_code)]

use super::OrTemplated;
use serde::Deserialize;
// Queries/expressions are handled as regular String values for now.

// TODO: handle the queries better.

#[derive(Debug, Deserialize)]
pub struct Logger {
    select: Option<String>,
    for_each: Option<String>,
    r#where: Option<String>,
    to: LogTo,
    #[serde(default)]
    pretty: bool,
    #[serde(default)]
    limit: u64,
    #[serde(default)]
    kill: bool,
}

#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LogTo {
    Stdout,
    Stderr,
    File(OrTemplated<String>),
    /// Allows templating of non-file paths, similar to the legacy parser. Literal string values of
    /// "stdout" and "stderr" will redirect to the corresponding target, where anything else will
    /// be a file of that name.
    ///
    /// Make sure to be extra cautious about spelling the sentinel values correctly.
    Raw(OrTemplated<String>),
}

impl LogTo {
    /// "Flattens" a [`LogTo::Raw`] into one of the other options by evaluating the template.
    fn flatten_raw(
        &self,
        vars: &super::templating::Vars,
    ) -> Result<Self, super::templating::TemplateError<String>> {
        match self {
            Self::Raw(ots) => match ots.evaluate(vars)?.as_str() {
                "stdout" => Ok(Self::Stdout),
                "stderr" => Ok(Self::Stderr),
                other => Ok(Self::File(OrTemplated::new_literal(other.to_owned()))),
            },
            other => Ok(other.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_yaml::from_str as from_yaml;

    #[test]
    fn test_log_to_basic() {
        let to = from_yaml::<LogTo>("!stdout").unwrap();
        assert_eq!(to, LogTo::Stdout);
        let to = from_yaml::<LogTo>("!stderr").unwrap();
        assert_eq!(to, LogTo::Stderr);
        let to = from_yaml::<LogTo>("!file out.txt").unwrap();
        assert_eq!(
            to,
            LogTo::File(OrTemplated::new_literal("out.txt".to_owned()))
        );
        assert!(from_yaml::<LogTo>("!stder").is_err());
    }

    // This test may need to be rewritten when the templating/vars structure is changed
    #[test]
    fn test_log_to_raw() {
        let to = from_yaml::<LogTo>("!raw stdout").unwrap();
        assert_eq!(to.flatten_raw(&[].into()), Ok(LogTo::Stdout));
        let to = from_yaml::<LogTo>("!raw stderr").unwrap();
        assert_eq!(to.flatten_raw(&[].into()), Ok(LogTo::Stderr));
        let to = from_yaml::<LogTo>("!raw out.txt").unwrap();
        assert_eq!(
            to.flatten_raw(&[].into()),
            Ok(LogTo::File(OrTemplated::new_literal("out.txt".to_owned())))
        );
        let to = from_yaml::<LogTo>("!raw stder").unwrap();
        assert_eq!(
            to.flatten_raw(&[].into()),
            Ok(LogTo::File(OrTemplated::new_literal("stder".to_owned())))
        );
    }

    #[test]
    fn test_logger_defaults() {
        let logger = from_yaml::<Logger>("to: !stdout").unwrap();
        assert_eq!(logger.select, None);
        assert_eq!(logger.for_each, None);
        assert_eq!(logger.r#where, None);
        assert_eq!(logger.pretty, false);
        assert_eq!(logger.limit, 0);
        assert_eq!(logger.kill, false);

        assert_eq!(logger.to, LogTo::Stdout);
    }
}
