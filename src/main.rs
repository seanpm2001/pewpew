#![feature(drain_filter, no_more_cas)]

mod body_reader;
mod channel;
mod config;
mod for_each_parallel;
mod load_test;
mod mod_interval;
mod providers;
mod request;
mod stats;
mod util;
mod zip_all;

use std::{fs::File, path::PathBuf};

use crate::load_test::LoadTest;
use clap::{crate_version, App, Arg};
use futures::future::lazy;
use serde_yaml;
use tokio;

fn main() {
    #[cfg(target_os = "windows")]
    {
        let _ = ansi_term::enable_ansi_support();
    }
    let matches = App::new("pewpew")
        .version(crate_version!())
        .arg(
            Arg::with_name("CONFIG")
                .help("the load test config file to use")
                .index(1)
                .default_value("loadtest.yaml"),
        )
        .get_matches();
    let load_test_config_file: PathBuf = matches.value_of("CONFIG").unwrap().into();
    tokio::run(lazy(move || {
        let file = File::open(&load_test_config_file)
            .unwrap_or_else(|_| panic!("error opening `{:?}`", load_test_config_file));
        let config = serde_yaml::from_reader(file).expect("couldn't parse yaml");
        LoadTest::new(config, load_test_config_file).run()
    }));
}
