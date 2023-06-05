use itertools::Itertools;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use std::{collections::BTreeMap, convert::TryFrom, fmt, str::FromStr};
use thiserror::Error;

use super::PropagateVars;

#[derive(Default, Deserialize, Debug, PartialEq, Eq, Clone, Copy)]
pub struct True;

#[derive(Deserialize, Debug, PartialEq, Eq, Clone, Copy)]
pub enum False {}

pub trait TryDefault: Sized + fmt::Debug {
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

pub trait Bool: fmt::Debug + TryDefault + Clone + Copy + PartialEq + Eq {
    type Inverse: Bool + fmt::Debug;

    const VALUE: bool;
}

pub trait OK: Default {}

impl OK for True {}

impl Bool for True {
    type Inverse = False;
    const VALUE: bool = true;
}

impl Bool for False {
    type Inverse = True;
    const VALUE: bool = false;
}

pub trait TemplateType: fmt::Debug {
    type EnvsAllowed: Bool;
    type VarsAllowed: Bool;
    type ProvAllowed: Bool;
}

#[derive(Deserialize, Debug)]
pub struct EnvsOnly;

impl TemplateType for EnvsOnly {
    type EnvsAllowed = True;
    type VarsAllowed = False;
    type ProvAllowed = False;
}

#[derive(Deserialize, Debug, PartialEq, Eq, Clone, Copy)]
pub struct VarsOnly;

impl TemplateType for VarsOnly {
    type EnvsAllowed = False;
    type VarsAllowed = True;
    type ProvAllowed = False;
}

#[derive(Deserialize, Debug, PartialEq, Eq, Clone, Copy)]
pub struct Regular;

impl TemplateType for Regular {
    type EnvsAllowed = False;
    type VarsAllowed = True;
    type ProvAllowed = True;
}

#[derive(Deserialize, Debug, PartialEq, Eq, Clone)]
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
        __dontuse: (T::EnvsAllowed, ED::Inverse),
    },
    PreVars {
        template: TemplatedString<T>,
        __dontuse: (T::VarsAllowed, VD::Inverse),
    },
    // needs more work done on this
    NeedsProviders {
        script: TemplatedString<T>,
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
/*
fn validate_template_types<T: TemplateType>(
    t: TemplatedString,
) -> Result<TemplatedString, TemplateError> {
    let error =
        t.0.iter().find_map(|ts| match ts {
            TemplatePiece::Raw(_) => None,
            TemplatePiece::Env(e) => (!T::EnvsAllowed::VALUE)
                .then(|| TemplateError::InvalidTemplateForType("e", e.clone())),
            TemplatePiece::Var(v) => (!T::VarsAllowed::VALUE)
                .then(|| TemplateError::InvalidTemplateForType("v", v.clone())),
            TemplatePiece::Provider(p) => (!T::ProvAllowed::VALUE)
                .then(|| TemplateError::InvalidTemplateForType("p", p.clone())),
        });
    match error {
        Some(e) => Err(e),
        None => Ok(t),
    }
}
*/
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

/*
#[test]
fn test_new_templates() {
    serde_yaml::from_str::<Template<i32, EnvsOnly, True, True>>("!l 5").unwrap();
    serde_yaml::from_str::<Template<i32, VarsOnly, False, False>>("!l 5").unwrap();
    serde_yaml::from_str::<Template<i32, EnvsOnly>>("!e ${e:HOME}").unwrap();
    serde_yaml::from_str::<Template<i32, VarsOnly>>("!e ${e:HOME}").unwrap_err();
    serde_yaml::from_str::<Template<i32, VarsOnly>>("!v ${v:x}").unwrap();
    serde_yaml::from_str::<Template<i32, Regular>>("!v ${v:x}").unwrap();
    serde_yaml::from_str::<Template<i32, Regular>>("!v ${e:HOME}").unwrap_err();

    let t = "hello".parse::<TemplatedString>().unwrap();
    assert_eq!(t.0, vec![TemplatePiece::Raw("hello".to_owned())]);

    let TemplatedString(v) = "${e:HOME}".parse().unwrap();
    assert_eq!(v, vec![TemplatePiece::Env("HOME".to_owned())]);
    let TemplatedString(v) = "${v:x}".parse().unwrap();
    assert_eq!(v, vec![TemplatePiece::Var("x".to_owned())]);
    let TemplatedString(v) = "${p:foobar}".parse().unwrap();
    assert_eq!(v, vec![TemplatePiece::Provider("foobar".to_owned())]);
}*/
