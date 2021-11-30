#![allow(dead_code)]
use figment::{
    providers::{Env, Format, Yaml},
    Figment,
};
use serde::Deserialize;
use std::path::PathBuf;

use clap::{AppSettings, Parser};

/// Configuration we can get either from a file or from ENV variables
#[derive(Deserialize)]
struct Config {
    dir: String,
}

impl Config {
    fn load() -> Result<Self, figment::Error> {
        let config_path = std::env::var("JOURNAL_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = dirs::home_dir().expect("Unable to get the the users 'home' directory");
                home.join(".journal.yaml")
            });

        Figment::new()
            .merge(Yaml::file(config_path))
            .merge(Env::prefixed("JOURNAL_"))
            .extract()
    }
}

/// Commands and arguments passed via the command line
#[derive(Parser)]
#[clap(
    name = "fern",
    version = "0.0.3",
    author = "Felipe Sere <journal@felipesere.com>",
    setting = AppSettings::DeriveDisplayOrder,
)]
struct Cli {
    #[clap(subcommand)]
    cmd: Cmd,
}

#[derive(Parser)]
enum Cmd {
    New { title: String },
}

fn main() {
    std::env::var("JOURNAL_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = dirs::home_dir().expect("Unable to get the the users 'home' directory");
            home.join(".journal.yaml")
        });

    let _ = Cli::parse();
}

#[cfg(test)]
mod test {

    mod config {
        use crate::Config;

        #[test]
        fn config_read_from_yml() {
            figment::Jail::expect_with(|jail| {
                let config_path = jail.directory().join(".journal.yml");
                jail.set_env("JOURNAL_CONFIG", config_path.to_string_lossy());

                jail.create_file(".journal.yml", r#"dir: file/from/yaml"#)?;

                let config = Config::load()?;

                assert_eq!(config.dir, "file/from/yaml");

                Ok(())
            });
        }

        #[test]
        fn config_read_from_env() {
            figment::Jail::expect_with(|jail| {
                let config_path = jail.directory().join(".journal.yml");
                jail.set_env("JOURNAL_CONFIG", config_path.to_string_lossy());

                jail.create_file(".journal.yml", r#"dir: file/from/yaml"#)?;
                jail.set_env("JOURNAL_DIR", "env/set/the/dir");

                let config = Config::load()?;

                assert_eq!(config.dir, "env/set/the/dir");

                Ok(())
            });
        }
    }
}
