//! Generic Types for values that can be evaluated at a later point with config variables.
//!
//! Currently, only string key -> string value replacement is implemented; nested mapped values and
//! expression functions need to be done still.

use once_cell::{sync::Lazy, unsync::OnceCell};
use regex::Regex;
use serde::Deserialize;
use std::{borrow::Cow, collections::BTreeMap, convert::TryFrom, fmt::Display, str::FromStr};
use thiserror::Error;

// just using pure Strings for now
pub type Vars = BTreeMap<String, String>;

static TEMPLATE_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"\$\{(.*?)}").unwrap());

// TODO: handle providers and expressions (or just go straight to scripting)
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
struct Template(String);

impl Template {
    #[allow(dead_code, unused_variables)]
    fn new(s: &str) -> Result<Self, ()> {
        // if template is valid, return Self, else Err
        todo!()
    }

    fn eval(&self, vars: &Vars) -> Result<Cow<str>, String> {
        // Replace ${var_name} patterns with literal value of var_name
        // will be unchanged if no patterns are found

        // TODO: Very bad temporary solution, need to fix
        TEMPLATE_REGEX
            .captures_iter(&self.0)
            .all(|c| vars.contains_key(c.get(1).unwrap().as_str()))
            .then(|| {
                TEMPLATE_REGEX.replace_all(&self.0, |c: &regex::Captures| {
                    vars.get(c.get(1).unwrap().as_str()).unwrap()
                })
            })
            .ok_or_else(|| {
                TEMPLATE_REGEX
                    .captures_iter(&self.0)
                    .map(|c| c.get(1).unwrap().as_str())
                    .find(|k| !vars.contains_key(*k))
                    .unwrap()
                    .to_owned()
            })
    }
}

/// Contains a value of type `T` that can be represented by a templated string to be evaluated
/// later. Since the vars are meant to be unchanged for the duration of the program runtime
/// (at least not without refreshing the entire config file), passing in different vars later
/// will not update the contained value.
///
/// # [`PartialEq`], [`Eq`] behavior
///
/// Two [`OrTemplated`] are considered equal if:
///  - They have both been evaluated, and their values are equal.
///  - Neither has been evaluated, and the template strings are identical.
/// In any other case, the two are not considered equal.
///
/// ## Examples
/// ```
/// # use config::new_parser::templating::{OrTemplated, FromTemplatedStr};
/// let x = OrTemplated::<u8>::new_literal(27);
///
/// let y = u8::from_raw_str("${count}").unwrap();
/// let _ = y.evaluate(&[("count".to_owned(), "27".to_owned())].into());
///
/// assert_eq!(x, y);
/// ```
#[derive(Debug, Deserialize, Clone, Eq)]
#[serde(try_from = "&str")]
pub struct OrTemplated<T: FromTemplatedStr>(Template, OnceCell<T>);

impl<T, U> PartialEq<OrTemplated<U>> for OrTemplated<T>
where
    T: FromTemplatedStr + PartialEq<U>,
    U: FromTemplatedStr,
{
    fn eq(&self, other: &OrTemplated<U>) -> bool {
        match (self.try_get(), other.try_get()) {
            (Some(x), Some(y)) => x == y,
            (None, None) => self.0 == other.0,
            _ => false,
        }
    }
}

impl<T: FromTemplatedStr> OrTemplated<T> {
    /// Attempt to directly get the represented `T` value.
    /// Returns `Some` if the value has previously been evaluated, or was not templated. Returns
    /// `None` if the template has not been evaluated.
    ///
    /// # Examples
    ///
    /// ```
    /// # use config::new_parser::templating::OrTemplated;
    /// // Value is present.
    /// let val = "5".parse::<OrTemplated<u8>>().unwrap();
    /// assert_eq!(val.try_get(), Some(&5));
    /// ```
    ///
    /// ```
    /// # use config::new_parser::templating::OrTemplated;
    /// // Value is not present.
    /// let val = "${count}".parse::<OrTemplated<u8>>().unwrap();
    /// assert_eq!(val.try_get(), None);
    /// ```
    pub fn try_get(&self) -> Option<&T> {
        self.1.get()
    }

    /// Attempt to evaluate the value with the provided vars, or return the existing value if it
    /// has already been evaluated.
    ///
    /// # Errors
    ///
    /// Returns an `Err` if that value has not already been evaluated, and could not be with the
    /// given `vars`.
    ///
    /// # Examples
    /// ```
    /// # use config::new_parser::templating::{OrTemplated, TemplateError};
    /// // Value is already present, no need to eval.
    /// let val = OrTemplated::new_literal(5u8);
    /// assert_eq!(val.evaluate(&[].into()), Ok(&5));
    ///
    /// // Template needs a var `count`.
    /// let val = "${count}".parse::<OrTemplated<u8>>().unwrap();
    ///
    /// // The needed var was not provided; an error is returned.
    /// assert_eq!(val.evaluate(&[].into()), Err(TemplateError::VarsNotFound("count".to_owned())));
    ///
    /// // The needed var is provided, so that value is evaluated.
    /// assert_eq!(val.evaluate(&[("count".to_owned(), "19".to_owned())].into()), Ok(&19));
    ///
    /// // Now that the value has been evaluated, vars are no longer needed.
    /// assert_eq!(val.evaluate(&[].into()), Ok(&19));
    ///
    /// // The values are not evaluated again if different vars are passed in.
    /// assert_eq!(val.evaluate(&[("count".to_owned(), "255".to_owned())].into()), Ok(&19));
    /// ```
    pub fn evaluate(&self, vars: &Vars) -> Result<&T, TemplateError<T>> {
        self.1.get_or_try_init(|| {
            T::from_final_str(&self.0.eval(vars).map_err(TemplateError::VarsNotFound)?)
        })
    }

    /// Create a new non-templated value from the provided.
    ///
    /// # Examples
    ///
    /// ```
    /// # use config::new_parser::templating::OrTemplated;
    /// let val = OrTemplated::new_literal(5u8);
    ///
    /// assert_eq!(val.try_get(), Some(&5));
    /// ```
    pub fn new_literal(literal: T) -> Self {
        Self(Template("".to_owned()), OnceCell::with_value(literal))
    }
}

impl<T: FromTemplatedStr> FromStr for OrTemplated<T> {
    type Err = TemplateError<T>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        T::from_raw_str(s)
    }
}

impl<T: FromTemplatedStr> TryFrom<&str> for OrTemplated<T> {
    type Error = <Self as FromStr>::Err;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        value.parse()
    }
}

/// Trait for generating types from templated strings.
pub trait FromTemplatedStr: Sized {
    type Error: Display;

    /// Generate a `T` from the string, similar to [`FromStr::from_str`].
    fn from_final_str(s: &str) -> Result<Self, TemplateError<Self>>;

    /// Possibly generate a `T` from the templated string. The [`OrTemplated`] may contain the
    /// final value if the input string is not templated.
    fn from_raw_str(s: &str) -> Result<OrTemplated<Self>, TemplateError<Self>> {
        if TEMPLATE_REGEX.is_match(s) {
            Ok(OrTemplated(Template(s.to_owned()), OnceCell::new()))
        } else {
            Self::from_final_str(s).map(OrTemplated::new_literal)
        }
    }
}

/// Error in generating a `T` from a templated string.
#[derive(Debug, PartialEq, Eq, Error)]
pub enum TemplateError<T: FromTemplatedStr>
where
    T::Error: Display,
{
    #[error("temple string invalid")]
    InvalidTemplate,
    #[error("var {0} not found")]
    /// Some variable that the templated string requires were not provided.
    VarsNotFound(String),
    #[error("from str error: {0}")]
    /// An error occurred converting the final string to a `T`.
    FromStrErr(T::Error),
}

impl<T> FromTemplatedStr for T
where
    T: FromStr,
    T::Err: Display,
{
    type Error = T::Err;

    fn from_final_str(s: &str) -> Result<Self, TemplateError<Self>> {
        s.parse().map_err(TemplateError::FromStrErr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_literal_templated() {
        assert_eq!(i32::from_raw_str("5").unwrap().try_get(), Some(&5));
        assert_eq!(i32::from_raw_str("-5").unwrap().try_get(), Some(&-5));
        assert_eq!(i32::from_raw_str("-5").unwrap().try_get(), Some(&-5));
    }

    #[test]
    fn test_lazy_template() {
        let temp = i32::from_raw_str("${some_var}").unwrap();
        assert_eq!(temp.try_get(), None);
        assert_eq!(temp.0, Template("${some_var}".to_owned()));
        assert_eq!(temp.1, OnceCell::new());
    }

    #[test]
    fn test_replace_var_not_found() {
        let temp = i32::from_raw_str("${hello}").unwrap();
        assert_eq!(temp.try_get(), None);
        assert_eq!(temp.0, Template("${hello}".to_owned()));
        assert_eq!(temp.1, OnceCell::new());
        assert_eq!(
            temp.evaluate(&BTreeMap::new()),
            Err(TemplateError::VarsNotFound("hello".to_owned()))
        );
    }

    #[test]
    fn test_replace() {
        let temp = i32::from_raw_str("${hello}").unwrap();
        assert_eq!(temp.try_get(), None);
        assert_eq!(temp.0, Template("${hello}".to_owned()));
        assert_eq!(temp.1, OnceCell::new());
        assert_eq!(
            temp.evaluate(&BTreeMap::from([("hello".to_owned(), "23".to_owned())])),
            Ok(&23)
        );
    }

    #[test]
    fn test_replace_multi() {
        let temp = i32::from_raw_str("${hello}3${world}").unwrap();
        assert_eq!(temp.try_get(), None);
        assert_eq!(temp.0, Template("${hello}3${world}".to_owned()));
        assert_eq!(temp.1, OnceCell::new());
        assert_eq!(
            temp.evaluate(&BTreeMap::from([
                ("hello".to_owned(), "12".to_owned()),
                ("world".to_owned(), "45".to_owned())
            ])),
            Ok(&12345)
        );
    }
}
