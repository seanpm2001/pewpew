//! Functions and types that are shared by both v1 and v2 configs.

use once_cell::sync::Lazy;
use regex::Regex;
use std::time::Duration;

/// Returns a [`Duration`] based on the input [`str`], based on the config format, or `None` if the
/// string does not match the pattern.
///
/// # Panics
/// Can panic if the duration string contains a value larger than [`u64::MAX`].
pub(crate) fn duration_from_string(dur: &str) -> Option<Duration> {
    const BASE_RE: &str = r"(?i)(\d+)\s*(d|h|m|s|days?|hrs?|mins?|secs?|hours?|minutes?|seconds?)";
    static SANITY_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(&format!(r"^(?:{BASE_RE}\s*)+$")).expect("should be a valid regex")
    });
    SANITY_RE
        .is_match(dur)
        .then(|| {
            static RE: Lazy<Regex> =
                Lazy::new(|| Regex::new(BASE_RE).expect("should be a valid regex"));
            RE.captures_iter(&dur)
                .map(|captures| {
                    // shouldn't panic due to how regex is set up
                    // unless a value greater then u64::MAX is used
                    let [n, unit] = (1..=2)
                        .map(|i| captures.get(i).expect("should have capture group").as_str())
                        .collect::<Vec<_>>()[..] else {
                        unreachable!()
                    };
                    n.parse::<u64>().unwrap()
                        * match &unit[0..1] {
                            "d" | "D" => 60 * 60 * 24, // days
                            "h" | "H" => 60 * 60,      // hours
                            "m" | "M" => 60,           // minutes
                            "s" | "S" => 1,            // seconds
                            _ => unreachable!(),       // regex shouldn't capture anything else
                        }
                })
                .sum()
        })
        .map(Duration::from_secs)
}
