use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use std::{convert::TryFrom, fmt, str::FromStr};
use thiserror::Error;

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

pub trait Bool: fmt::Debug + TryDefault {
    type Inverse: Bool + fmt::Debug;

    const VALUE: bool;
}

trait OK: Default {}

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
#[serde(try_from = "TemplateTmp<V>")]
pub enum Template<
    V,
    T: TemplateType,
    ED: Bool = <<T as TemplateType>::EnvsAllowed as Bool>::Inverse,
    VD: Bool = <<T as TemplateType>::VarsAllowed as Bool>::Inverse,
> {
    Literal {
        value: V,
    },
    Env {
        template: TemplatedString,
        __dontuse: (T::EnvsAllowed, ED::Inverse),
    },
    PreVars {
        template: TemplatedString,
        __dontuse: (T::VarsAllowed, VD::Inverse),
    },
    // needs more work done on this
    NeedsProviders {
        script: TemplatedString,
        __dontuse: (ED, VD, T::ProvAllowed),
    },
}

#[derive(Debug, PartialEq, Eq, Deserialize, Clone)]
#[serde(try_from = "&str")]
#[serde(bound = "")]
pub struct TemplatedString(Vec<TemplatePiece>);

impl FromStr for TemplatedString {
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

            let r#type = match caps.get(1).map(|c| c.as_str()).unwrap_or("") {
                "v" => TemplatePiece::Var,
                "p" => TemplatePiece::Provider,
                "e" => TemplatePiece::Env,
                _ => unreachable!(),
            };

            let segment = caps.get(0).unwrap().as_str();
            let path = caps.get(2).unwrap().as_str();
            pieces.push(r#type(path.to_owned()));
            s = s.strip_prefix(segment).unwrap();
        }
        Ok(Self(pieces))
    }
}

impl TryFrom<&str> for TemplatedString {
    type Error = <Self as FromStr>::Err;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        value.parse()
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
enum TemplatePiece {
    Raw(String),
    Env(String),
    Var(String),
    Provider(String),
}

#[derive(Debug, Deserialize, Clone)]
enum TemplateTmp<V> {
    #[serde(rename = "l")]
    Literal(V),
    #[serde(rename = "e")]
    Env(TemplatedString),
    #[serde(rename = "v")]
    Vars(TemplatedString),
    #[serde(rename = "s")]
    Script(TemplatedString),
}

#[derive(Debug, PartialEq, Eq, Hash, Error)]
enum TemplateError {
    #[error(r#"invalid type tag "{0}" for Template"#)]
    InvalidTypeTag(&'static str),
    #[error(r#"invalid template type "{0}" with value "{1}""#)]
    InvalidTemplateForType(&'static str, String),
}

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

impl<V, T: TemplateType, ED: Bool, VD: Bool> TryFrom<TemplateTmp<V>> for Template<V, T, ED, VD> {
    type Error = TemplateError;

    fn try_from(value: TemplateTmp<V>) -> Result<Self, Self::Error> {
        Ok(match value {
            TemplateTmp::Literal(x) => Self::Literal { value: x },
            TemplateTmp::Env(template) => Self::Env {
                template: validate_template_types::<T>(template)?,
                __dontuse: TryDefault::try_default()
                    .map_err(|_| TemplateError::InvalidTypeTag("e"))?,
            },
            TemplateTmp::Vars(template) => Self::PreVars {
                template: validate_template_types::<T>(template)?,
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
}
