#![allow(dead_code)]

//! Some types in the shared code are tied to the [`yaml_rust::scanner::Marker`] type, which is not
//! used for serde-based deserialization. The [`MaybeMarked`] struct defined here allows a type to
//! be generic over whether it it tied with a [`Marker`] (for `configv1`) or not (for `configv2`).
//!
//! See [https://rreverser.com/conditional-enum-variants-in-rust/] for a more technical
//! explanation, but to summarize, `MaybeMarked<T, True>` is a pair of `T` and `Marker`, while
//! `MaybeMarked<T, False>` is just a `T`. This way, the same generic interface can be used for the
//! most part, while `configv1` code can insert markers as needed.
//!
//! In the event that the configv1 parser is wholly deprecated and removed, this whole module can
//! be gotten rid of as well, and just use a `T` instead of a `MaybeMarked<T, False>`

use derivative::Derivative;
use std::{
    fmt::{self, Display},
    hint::unreachable_unchecked,
    ops::Deref,
};
use yaml_rust::scanner::Marker;

pub type MM<T, B> = MaybeMarked<T, B>;

pub trait AllowMarkers: Copy {
    type Inverse: AllowMarkers;
}

#[derive(Debug, Clone, Copy)]
pub struct True;

impl AllowMarkers for True {
    type Inverse = False;
}

#[derive(Clone, Copy)]
pub enum False {}

impl AllowMarkers for False {
    type Inverse = True;
}

// // Uncomment if it's needed to determine "markedness" at runtime
//#[derive(Debug, Clone, Copy)]
//pub(crate) struct Either;
//
//impl AllowMarkers for Either {
//    type Inverse = Either;
//}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaybeMarked<T, B: AllowMarkers = False>(MaybeMarkedInner<T, B>);

// The __dontuse property of each variant prevents that variant from being constructed if the type
// is False. For example, MaybeMarkedInner<T, True> cannot be an Unmarked variant, because that
// requires a value of type True::Inverse, which is False, which has no values.
#[derive(Derivative, Clone, Copy)]
#[derivative(Debug, PartialEq, Eq)]
enum MaybeMarkedInner<T, B: AllowMarkers> {
    Marked {
        value: T,
        marker: Marker,
        #[derivative(PartialEq = "ignore")]
        #[derivative(Debug = "ignore")]
        __dontuse: B,
    },
    Unmarked {
        value: T,
        #[derivative(Debug = "ignore")]
        #[derivative(PartialEq = "ignore")]
        __dontuse: B::Inverse,
    },
}

impl<T> MaybeMarked<T, True> {
    pub(crate) fn new_marked(value: T, marker: Marker) -> Self {
        Self(MaybeMarkedInner::Marked {
            value,
            marker,
            __dontuse: True,
        })
    }

    pub(crate) fn get_marker(&self) -> &Marker {
        match &self.0 {
            MaybeMarkedInner::Marked { marker, .. } => marker,
            // Unmarked variant cannot be constructed when generic over True, so this cannot be
            // reached.
            MaybeMarkedInner::Unmarked { .. } => unsafe { unreachable_unchecked() },
        }
    }
}

impl<T> MaybeMarked<T, False> {
    pub(crate) fn new_unmarked(value: T) -> Self {
        Self(MaybeMarkedInner::Unmarked {
            value,
            __dontuse: True,
        })
    }

    pub(crate) fn into_marked(self, marker: Marker) -> MaybeMarked<T, True> {
        MaybeMarked::new_marked(self.into_inner(), marker)
    }

    /// Inserts the marker and "markedness" from `other` into self
    pub fn zip<B: AllowMarkers, U>(self, other: &MaybeMarked<U, B>) -> MaybeMarked<T, B> {
        match other.0 {
            MaybeMarkedInner::Marked {
                marker, __dontuse, ..
            } => MaybeMarked(MaybeMarkedInner::Marked {
                value: self.into_inner(),
                marker,
                __dontuse,
            }),
            MaybeMarkedInner::Unmarked { __dontuse, .. } => {
                MaybeMarked(MaybeMarkedInner::Unmarked {
                    value: self.into_inner(),
                    __dontuse,
                })
            }
        }
    }
}

impl<T> From<T> for MaybeMarked<T, False> {
    fn from(value: T) -> Self {
        Self::new_unmarked(value)
    }
}

impl<T, B: AllowMarkers> MaybeMarked<T, B> {
    pub fn get(&self) -> &T {
        match &self.0 {
            MaybeMarkedInner::Unmarked { value, .. } => value,
            MaybeMarkedInner::Marked { value, .. } => value,
        }
    }

    pub(crate) fn into_inner(self) -> T {
        match self.0 {
            MaybeMarkedInner::Unmarked { value, .. } => value,
            MaybeMarkedInner::Marked { value, .. } => value,
        }
    }

    pub(crate) fn try_get_marker(&self) -> Option<&Marker> {
        match &self.0 {
            MaybeMarkedInner::Unmarked { .. } => None,
            MaybeMarkedInner::Marked { marker, .. } => Some(marker),
        }
    }

    pub(crate) fn as_marker(&self) -> MaybeMarked<(), B> {
        self.as_ref().map_value(|_| ())
    }

    pub(crate) fn as_ref(&self) -> MaybeMarked<&T, B> {
        match &self.0 {
            MaybeMarkedInner::Marked {
                value,
                marker,
                __dontuse,
            } => MaybeMarked(MaybeMarkedInner::Marked {
                value,
                marker: *marker,
                __dontuse: *__dontuse,
            }),
            MaybeMarkedInner::Unmarked { value, __dontuse } => {
                MaybeMarked(MaybeMarkedInner::Unmarked {
                    value,
                    __dontuse: *__dontuse,
                })
            }
        }
    }

    pub fn extract(self) -> (T, MaybeMarked<(), B>) {
        let marker = self.as_marker();
        (self.into_inner(), marker)
    }

    pub fn map_value<U, F>(self, f: F) -> MaybeMarked<U, B>
    where
        F: FnOnce(T) -> U,
    {
        match self.0 {
            MaybeMarkedInner::Marked {
                value,
                marker,
                __dontuse,
            } => MaybeMarked(MaybeMarkedInner::Marked {
                value: f(value),
                marker,
                __dontuse,
            }),
            MaybeMarkedInner::Unmarked { value, __dontuse } => {
                MaybeMarked(MaybeMarkedInner::Unmarked {
                    value: f(value),
                    __dontuse,
                })
            }
        }
    }

    pub(crate) fn map_into<U>(self) -> MaybeMarked<U, B>
    where
        U: From<T>,
    {
        self.map_value(From::from)
    }
}

impl<T, B: AllowMarkers> Display for MaybeMarked<T, B>
where
    T: Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            MaybeMarkedInner::Marked { value, marker, .. } => write!(
                f,
                "{} at line {} col {}",
                value,
                marker.line(),
                marker.col()
            ),
            MaybeMarkedInner::Unmarked { value, .. } => write!(f, "{value}"),
        }
    }
}

impl<T: std::error::Error, B: AllowMarkers> std::error::Error for MaybeMarked<T, B>
where
    Self: Display + fmt::Debug,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.get().source()
    }
}

impl<T, B: AllowMarkers> Deref for MaybeMarked<T, B> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.get()
    }
}
