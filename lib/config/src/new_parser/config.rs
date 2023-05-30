#![allow(dead_code)]

use super::common::{Duration, Headers};
use serde::Deserialize;

#[derive(Deserialize, Debug, PartialEq, Eq)]
pub struct Config {
    client: Client,
    general: General,
}

/// Customization Parameters for the HTTP client
#[derive(Deserialize, Debug, PartialEq, Eq)]
struct Client {
    #[serde(default = "default_timeout")]
    request_timeout: Duration,
    #[serde(default)]
    headers: Headers,
    #[serde(default = "default_keepalive")]
    keepalive: Duration,
}

#[derive(Deserialize, Debug, PartialEq, Eq)]
struct General {
    #[serde(default = "default_buffer_start_size")]
    auto_buffer_start_size: u64,
    #[serde(default = "default_bucket_size")]
    bucket_size: Duration,
    #[serde(default = "default_log_provider_stats")]
    log_provider_stats: bool,
    watch_transition_time: Option<Duration>,
}

fn default_timeout() -> Duration {
    Duration::from_secs(60)
}

fn default_keepalive() -> Duration {
    Duration::from_secs(90)
}

const fn default_buffer_start_size() -> u64 {
    5
}

fn default_bucket_size() -> Duration {
    Duration::from_secs(60)
}

const fn default_log_provider_stats() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::new_parser::OrTemplated;
    use serde_yaml::from_str as from_yaml;

    #[test]
    fn test_client() {
        static TEST1: &str = "";
        let Client {
            request_timeout,
            headers,
            keepalive,
        } = from_yaml(TEST1).unwrap();
        assert_eq!(request_timeout, Duration::from_secs(60));
        assert!(headers.is_empty());
        assert_eq!(keepalive, Duration::from_secs(90));

        static TEST2: &str = r#"
request_timeout: 23s
headers:
  one: two
keepalive: 19s
        "#;

        let Client {
            request_timeout,
            headers,
            keepalive,
        } = from_yaml(TEST2).unwrap();
        assert_eq!(request_timeout, Duration::from_secs(23));
        assert_eq!(headers.len(), 1);
        assert_eq!(headers["one"], OrTemplated::new_literal("two".to_owned()));
        assert_eq!(keepalive, Duration::from_secs(19));
    }

    #[test]
    fn test_general() {
        static TEST1: &str = "";
        let General {
            auto_buffer_start_size,
            bucket_size,
            log_provider_stats,
            watch_transition_time,
        } = from_yaml(TEST1).unwrap();
        assert_eq!(auto_buffer_start_size, 5);
        assert_eq!(bucket_size, Duration::from_secs(60));
        assert_eq!(log_provider_stats, true);
        assert_eq!(watch_transition_time, None);

        static TEST2: &str = r#"
auto_buffer_start_size: 100
bucket_size: 2m
log_provider_stats: false
watch_transition_time: 23s
        "#;
        let General {
            auto_buffer_start_size,
            bucket_size,
            log_provider_stats,
            watch_transition_time,
        } = from_yaml(TEST2).unwrap();
        assert_eq!(auto_buffer_start_size, 100);
        assert_eq!(bucket_size, Duration::from_secs(120));
        assert_eq!(log_provider_stats, false);
        assert_eq!(watch_transition_time, Some(Duration::from_secs(23)));
    }

    #[test]
    fn test_config() {
        static TEST1: &str = "client: {}\ngeneral: {}";
        let Config { client, general } = from_yaml(TEST1).unwrap();
        assert_eq!(client, from_yaml::<Client>("").unwrap());
        assert_eq!(general, from_yaml::<General>("").unwrap());

        static TEST2: &str = r#"
client:
  request_timeout: 89s
general:
  bucket_size: 1 hour
        "#;
        let Config { client, general } = from_yaml(TEST2).unwrap();
        assert_eq!(client.request_timeout, Duration::from_secs(89));
        assert_eq!(general.bucket_size, Duration::from_secs(3600));
    }
}