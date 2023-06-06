//! Templating-related types. The generic type vars `VD` and `ED` correspond to "Vars Done", meaning
//! that static vars have been inserted, and "Envs Done", meaning that OS Environment variables have been
//! inserted.
//!
//! Rather than redefine nearly identical types multiple times for the same structure before and
//! after var processing, this module uses conditional enums to manage state, as well as which
//! template sources are allowed.
//!
//! Read here for more info: <https://rreverser.com/conditional-enum-variants-in-rust/>
//!
//! For example: `Template<_, EnvsOnly>` cannot be instantiated in the PreVars variant, because the
//! associated type is False.

use super::PropagateVars;
use derivative::Derivative;
pub use helpers::*;
use itertools::Itertools;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use std::{collections::BTreeMap, convert::TryFrom, str::FromStr};
use thiserror::Error;

#[derive(Deserialize, PartialEq, Eq, Clone, Derivative)]
#[derivative(Debug)]
#[serde(try_from = "TemplateTmp<V, T>")]
pub enum Template<
    V: FromStr,
    T: TemplateType,
    VD: Bool, /* = <<T as TemplateType>::VarsAllowed as Bool>::Inverse*/
    ED: Bool = <<T as TemplateType>::EnvsAllowed as Bool>::Inverse,
> {
    Literal {
        value: V,
    },
    Env {
        template: TemplatedString<T>,
        #[derivative(Debug = "ignore")]
        __dontuse: (T::EnvsAllowed, ED::Inverse),
    },
    PreVars {
        template: TemplatedString<T>,
        #[derivative(Debug = "ignore")]
        __dontuse: (T::VarsAllowed, VD::Inverse),
    },
    // needs more work done on this
    NeedsProviders {
        script: TemplatedString<T>,
        #[derivative(Debug = "ignore")]
        __dontuse: (ED, VD, T::ProvAllowed),
    },
}

impl<VD: Bool> Template<String, EnvsOnly, VD, False> {
    pub(crate) fn insert_env_vars(
        self,
        evars: &BTreeMap<String, String>,
    ) -> Result<Template<String, EnvsOnly, VD, True>, MissingEnvVar> {
        match self {
            Self::Literal { value } => Ok(Template::Literal { value }),
            Self::Env {
                template,
                __dontuse,
            } => Ok(Template::Literal {
                value: template
                    .insert_env_vars(evars)?
                    .try_collect()
                    .expect("EnvsOnly shouldn't have other types"),
            }),
            _ => unreachable!(),
        }
    }
}

impl<V: FromStr, T: TemplateType> Template<V, T, True, True>
where
    <T::ProvAllowed as Bool>::Inverse: OK,
{
    fn get(&self) -> &V {
        match self {
            Self::Literal { value } => value,
            _ => unreachable!(),
        }
    }
}

impl<V: FromStr, T: TemplateType> PropagateVars for Template<V, T, False, True>
where
    T::VarsAllowed: OK,
    V::Err: std::error::Error + 'static,
{
    type Residual = Template<V, T, True, True>;

    fn insert_vars(self, vars: &super::VarValue<True>) -> Result<Self::Residual, super::VarsError> {
        match self {
            Self::Literal { value } => Ok(Template::Literal { value }),
            Self::PreVars {
                template,
                __dontuse,
            } => {
                let s = template.insert_vars(vars)?;
                if T::ProvAllowed::VALUE {
                    Ok(Template::NeedsProviders {
                        script: s,
                        __dontuse: TryDefault::try_default().unwrap(),
                    })
                } else {
                    let s = s.try_collect().unwrap();
                    s.parse()
                        .map_err(|e: <V as FromStr>::Err| super::VarsError::InvalidString {
                            typename: std::any::type_name::<V>(),
                            from: s,
                            error: e.into(),
                        })
                        .map(|v| Template::Literal { value: v })
                }
            }
            _ => unreachable!(),
        }
    }
}

#[derive(Debug, Error)]
#[error("missing environment variable {0}")]
pub struct MissingEnvVar(String);

#[derive(Debug, PartialEq, Eq, Deserialize, Clone)]
#[serde(try_from = "&str")]
#[serde(bound = "")]
pub struct TemplatedString<T: TemplateType>(Vec<TemplatePiece<T>>);

impl<T: TemplateType> TemplatedString<T> {
    fn try_collect(self) -> Option<String> {
        self.0
            .into_iter()
            .map(|p| match p {
                TemplatePiece::Raw(s) => Some(s),
                _ => None,
            })
            .collect()
    }
}

impl<T: TemplateType> PropagateVars for TemplatedString<T>
where
    T::VarsAllowed: OK,
{
    type Residual = Self;

    fn insert_vars(self, vars: &super::VarValue<True>) -> Result<Self::Residual, super::VarsError> {
        self.0
            .into_iter()
            .map(|p| match p {
                TemplatePiece::Var(v, ..) => {
                    let path = v.split('.').collect_vec();
                    path.iter()
                        .fold(Some(vars), |vars, n| vars.and_then(|v| v.get(n)))
                        .and_then(super::VarValue::finish)
                        .map(|vt| match vt {
                            super::VarTerminal::Num(n) => n.to_string(),
                            super::VarTerminal::Str(s) => s.get().clone(),
                            super::VarTerminal::Bool(b) => b.to_string(),
                        })
                        .ok_or_else(|| super::VarsError::VarNotFound(v))
                        .map(TemplatePiece::Raw)
                }
                other => Ok(other),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Self)
    }
}

impl<T: TemplateType> TemplatedString<T>
where
    T::EnvsAllowed: OK,
{
    fn insert_env_vars(self, evars: &BTreeMap<String, String>) -> Result<Self, MissingEnvVar> {
        self.0
            .into_iter()
            .map(|p| match p {
                TemplatePiece::Env(e, ..) => evars
                    .get(&e)
                    .cloned()
                    .map(TemplatePiece::Raw)
                    .ok_or_else(|| MissingEnvVar(e)),
                other => Ok(other),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Self)
    }
}

impl<T: TemplateType> FromStr for TemplatedString<T> {
    type Err = &'static str;

    fn from_str(mut s: &str) -> Result<Self, Self::Err> {
        static REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"\$\{(?:([vpe]):)(.*?)}").unwrap());
        static REGEX2: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[^\$]*").unwrap());

        let mut pieces = Vec::new();
        while !s.is_empty() {
            let caps = REGEX2.captures(s).unwrap();
            let segment = caps.get(0).unwrap().as_str();
            if !segment.is_empty() {
                pieces.push(TemplatePiece::Raw(segment.to_owned()));
            }
            s = s.strip_prefix(segment).unwrap();

            if s.is_empty() {
                return Ok(Self(pieces));
            }

            let Some(caps) = REGEX.captures(s) else {
                return Err("mismatched template pattern");
            };

            let r#type = |x: String| -> Result<TemplatePiece<T>, &'static str> {
                Ok(match caps.get(1).map(|c| c.as_str()).unwrap_or("") {
                    "v" => TemplatePiece::Var(x, T::VarsAllowed::try_default()?),
                    "p" => TemplatePiece::Provider(x, T::ProvAllowed::try_default()?),
                    "e" => TemplatePiece::Env(x, T::EnvsAllowed::try_default()?),
                    _ => unreachable!(),
                })
            };

            let segment = caps.get(0).unwrap().as_str();
            let path = caps.get(2).unwrap().as_str();
            pieces.push(r#type(path.to_owned())?);
            s = s.strip_prefix(segment).unwrap();
        }
        Ok(Self(pieces))
    }
}

impl<T: TemplateType> TryFrom<&str> for TemplatedString<T> {
    type Error = <Self as FromStr>::Err;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        value.parse()
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
enum TemplatePiece<T: TemplateType> {
    Raw(String),
    Env(String, T::EnvsAllowed),
    Var(String, T::VarsAllowed),
    Provider(String, T::ProvAllowed),
}

/// Temporary Template type that allows all variants to exist, but can fail in conversion into the
/// main Template based on the marker type of `T`.
#[derive(Debug, Deserialize, Clone)]
enum TemplateTmp<V, T: TemplateType> {
    #[serde(rename = "l")]
    Literal(V),
    #[serde(rename = "e")]
    Env(TemplatedString<T>),
    #[serde(rename = "v")]
    Vars(TemplatedString<T>),
    #[serde(rename = "s")]
    Script(TemplatedString<T>),
}

#[derive(Debug, PartialEq, Eq, Hash, Error)]
enum TemplateError {
    #[error(r#"invalid type tag "{0}" for Template"#)]
    InvalidTypeTag(&'static str),
    #[error(r#"invalid template type "{0}" with value "{1}""#)]
    InvalidTemplateForType(&'static str, String),
}

impl<V: FromStr, T: TemplateType, ED: Bool, VD: Bool> TryFrom<TemplateTmp<V, T>>
    for Template<V, T, ED, VD>
{
    type Error = TemplateError;

    fn try_from(value: TemplateTmp<V, T>) -> Result<Self, Self::Error> {
        Ok(match value {
            TemplateTmp::Literal(x) => Self::Literal { value: x },
            TemplateTmp::Env(template) => Self::Env {
                template,
                __dontuse: TryDefault::try_default()
                    .map_err(|_| TemplateError::InvalidTypeTag("e"))?,
            },
            TemplateTmp::Vars(template) => Self::PreVars {
                template,
                __dontuse: TryDefault::try_default()
                    .map_err(|_| TemplateError::InvalidTypeTag("v"))?,
            },
            _ => todo!(),
        })
    }
}

impl<T, U> TryDefault for (T, U)
where
    T: TryDefault,
    U: TryDefault,
{
    fn try_default() -> Result<Self, &'static str> {
        Ok((T::try_default()?, U::try_default()?))
    }
}

impl<T, U, V> TryDefault for (T, U, V)
where
    T: TryDefault,
    U: TryDefault,
    V: TryDefault,
{
    fn try_default() -> Result<Self, &'static str> {
        Ok((T::try_default()?, U::try_default()?, V::try_default()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templated_strings_raw() {
        let TemplatedString::<EnvsOnly>(t) = "hello".parse().unwrap();
        assert_eq!(t, vec![TemplatePiece::Raw("hello".to_owned())]);
        let TemplatedString::<VarsOnly>(t) = "hello".parse().unwrap();
        assert_eq!(t, vec![TemplatePiece::Raw("hello".to_owned())]);
        let TemplatedString::<Regular>(t) = "hello".parse().unwrap();
        assert_eq!(t, vec![TemplatePiece::Raw("hello".to_owned())]);
    }

    #[test]
    fn templated_strings_single() {
        let TemplatedString::<EnvsOnly>(v) = "${e:HOME}".parse().unwrap();
        assert_eq!(
            v,
            vec![TemplatePiece::Env(
                "HOME".to_owned(),
                TryDefault::try_default().unwrap()
            )]
        );
        let TemplatedString::<VarsOnly>(v) = "${v:x}".parse().unwrap();
        assert_eq!(
            v,
            vec![TemplatePiece::Var(
                "x".to_owned(),
                TryDefault::try_default().unwrap()
            )]
        );
        let TemplatedString::<Regular>(v) = "${v:x}".parse().unwrap();
        assert_eq!(
            v,
            vec![TemplatePiece::Var(
                "x".to_owned(),
                TryDefault::try_default().unwrap()
            )]
        );
        let TemplatedString::<Regular>(v) = "${p:foobar}".parse().unwrap();
        assert_eq!(
            v,
            vec![TemplatePiece::Provider(
                "foobar".to_owned(),
                TryDefault::try_default().unwrap()
            )]
        );
    }

    #[test]
    fn template_strings_vec() {
        assert_eq!(
            "foo${v:bar}".parse::<TemplatedString<VarsOnly>>(),
            Ok(TemplatedString(vec![
                TemplatePiece::Raw("foo".to_owned()),
                TemplatePiece::Var("bar".to_owned(), TryDefault::try_default().unwrap())
            ]))
        );

        assert_eq!(
            "${e:HOME}/file.txt".parse::<TemplatedString<EnvsOnly>>(),
            Ok(TemplatedString(vec![
                TemplatePiece::Env("HOME".to_owned(), TryDefault::try_default().unwrap()),
                TemplatePiece::Raw("/file.txt".to_owned()),
            ]))
        );
    }
}

mod helpers {
    use serde::Deserialize;
    use std::fmt;

    mod private {
        pub trait Seal {}

        impl Seal for super::True {}
        impl Seal for super::False {}
        impl Seal for super::EnvsOnly {}
        impl Seal for super::VarsOnly {}
        impl Seal for super::Regular {}
        impl<T, U> Seal for (T, U)
        where
            T: Seal,
            U: Seal,
        {
        }
        impl<T, U, V> Seal for (T, U, V)
        where
            T: Seal,
            U: Seal,
            V: Seal,
        {
        }
    }

    /// Unit type that only exists to allow enum variants containing to be made.
    #[derive(Default, Deserialize, Debug, PartialEq, Eq, Clone, Copy)]
    pub struct True;

    /// Uninhabited type that makes enum variants containing it to be inaccessible.
    #[derive(Deserialize, Debug, PartialEq, Eq, Clone, Copy)]
    pub enum False {}

    /// Trait for trying to get a Default value. Serde itself has no solution (that I could find)
    /// that directly allows making specific enum variants inaccessible, so this is to make
    /// generating a Default value fallible based on the type. If an invaild variant is used (for
    /// example, an env variant for a template outside of the vars section), then
    /// `False::try_default()` will be called, and an error will be forwarded and Deserialize will
    /// fail.
    pub trait TryDefault: Sized + fmt::Debug + private::Seal {
        fn try_default() -> Result<Self, &'static str>;
    }

    impl TryDefault for True {
        fn try_default() -> Result<Self, &'static str> {
            Ok(Self)
        }
    }

    impl TryDefault for False {
        fn try_default() -> Result<Self, &'static str> {
            Err("uninhabited type")
        }
    }

    /// Trait for a type that represents a boolean state for if a value can be constructed.
    pub trait Bool:
        fmt::Debug + TryDefault + Clone + Copy + PartialEq + Eq + private::Seal
    {
        type Inverse: Bool + fmt::Debug;

        const VALUE: bool;
    }

    /// Trait meaning that the Boolean type specifically can be created.
    pub trait OK: Default + Bool + private::Seal {}

    impl OK for True {}

    impl Bool for True {
        type Inverse = False;
        const VALUE: bool = true;
    }

    impl Bool for False {
        type Inverse = True;
        const VALUE: bool = false;
    }

    /// Trait for types of templatings allowed. It's not an enumeration of variants, because
    /// Template needs to be generic over a type of this trait.
    pub trait TemplateType: fmt::Debug + private::Seal + PartialEq + Eq {
        type EnvsAllowed: Bool;
        type VarsAllowed: Bool;
        type ProvAllowed: Bool;
    }

    /// Marker struct to indicate that this template can only read from OS environment variables as
    /// a source.
    #[derive(Deserialize, Debug, PartialEq, Eq, Clone, Copy)]
    pub struct EnvsOnly;

    impl TemplateType for EnvsOnly {
        type EnvsAllowed = True;
        type VarsAllowed = False;
        type ProvAllowed = False;
    }

    /// Marker struct to indicate that this template can only read from static Vars as a source.
    #[derive(Deserialize, Debug, PartialEq, Eq, Clone, Copy)]
    pub struct VarsOnly;

    impl TemplateType for VarsOnly {
        type EnvsAllowed = False;
        type VarsAllowed = True;
        type ProvAllowed = False;
    }

    /// Marker struct to indicate that this template can read from vars or providers.
    #[derive(Deserialize, Debug, PartialEq, Eq, Clone, Copy)]
    pub struct Regular;

    impl TemplateType for Regular {
        type EnvsAllowed = False;
        type VarsAllowed = True;
        type ProvAllowed = True;
    }
}
