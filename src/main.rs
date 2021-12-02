#![allow(dead_code)]

use anyhow::{bail, Context, Result};
use clap::{AppSettings, Parser};
use figment::{
    providers::{Env, Format, Yaml},
    Figment,
};

use serde::Deserialize;
use std::{path::PathBuf, str::FromStr};
use time::{OffsetDateTime, format_description};
use tera::{Context as TeraContext, Tera};
use tracing::Level;

const DAY_TEMPLATE: &str = include_str!("../template/day.md");

mod todo;

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

        tracing::info!("Loading configfrom {:?}", config_path);
        Figment::new()
            .merge(Yaml::file(config_path))
            .merge(Env::prefixed("JOURNAL_"))
            .extract()
    }
}

/// Commands and arguments passed via the command line
#[derive(Debug, Parser)]
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

#[derive(Debug, Parser)]
enum Cmd {
    New { title: String },
}

fn to_level<S: AsRef<str>>(level: S) -> Result<Level, ()> {
    Level::from_str(level.as_ref()).map_err(|_| ())
}

fn init_logs() {
    let level = std::env::var("JOURNAL_LOG_LEVEL")
        .map_err(|_| ())
        .and_then(to_level)
        .unwrap_or_else(|_| Level::ERROR);

    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_max_level(level)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
}

struct Entry {
    path: PathBuf,
    markdown: String,
}

struct Journal {
    location: PathBuf,
}

impl Journal {
    fn new_at<P: Into<PathBuf>>(location: P) -> Journal {
        Journal {
            location: location.into(),
        }
    }

    fn latest_entry(&self) -> Result<Entry> {
        // Would still need a filter that matches naming convention
        let mut entries = std::fs::read_dir(&self.location)?
            .map(|res| res.map(|e| e.path()).unwrap())
            .collect::<Vec<_>>();

        // The order in which `read_dir` returns entries is not guaranteed. If reproducible
        // ordering is required the entries should be explicitly sorted.
        entries.sort();

        if let Some(path) = entries.pop() {
            let markdown = std::fs::read_to_string(&path)?;

            return Ok(Entry { path, markdown });
        }

        bail!("No journal entries found in {:?}", self.location);
    }
}

fn main() -> Result<()> {
    init_logs();

    let config = Config::load().context("Failed to load configuration")?;
    let cli = Cli::parse();

    let journal = Journal::new_at(config.dir);

    match cli.cmd {
        Cmd::New { title } => {
            let latest_entry = journal.latest_entry()?;

            let mut finder = todo::FindTodos::new();
            finder.process(&latest_entry.markdown);

            let open_todos = finder
                .found_todos
                .into_iter()
                .map(|todo| latest_entry.markdown[todo].to_string())
                .collect::<Vec<_>>();

            let mut tera = Tera::default();
            tera.add_raw_template("day.md", DAY_TEMPLATE).unwrap();

            let year_month_day = format_description::parse("[year]-[month]-[day]").unwrap();
            let today = OffsetDateTime::now_utc().format(&year_month_day)?;

            let mut context = TeraContext::new();
            context.insert("title", &title);
            context.insert("date", &today);
            context.insert("open_todos", &open_todos);

            let out = tera.render("day.md", &context).unwrap();

            print!("{}", out);
        }
    }

    Ok(())
}

#[cfg(test)]
mod test {
    mod journal {
        use crate::Journal;
        use assert_fs::{prelude::*, TempDir};
        use predicates::prelude::*;
        use predicates::str::contains;

        #[test]
        fn empty_journal() {
            let location = TempDir::new().unwrap();

            let journal = Journal::new_at(location.path());

            let entry = journal.latest_entry();

            assert!(entry.is_err());
        }

        #[test]
        fn single_journal_entry() {
            let dir = TempDir::new().unwrap();
            dir.child("2021-08-23-first_entry.md")
                .write_str("first content")
                .unwrap();

            let journal = Journal::new_at(dir.path());

            let entry = journal.latest_entry();

            assert!(entry.is_ok());
            let entry = entry.unwrap();
            assert_eq!(
                true,
                contains("2021-08-23-first_entry.md").eval(&entry.path.to_string_lossy())
            );
            assert_eq!(entry.markdown, "first content");
        }

        #[test]
        fn returns_the_latest_entry() {
            let dir = TempDir::new().unwrap();
            dir.child("2021-07-03-older_entry.md")
                .write_str("older content")
                .unwrap();
            dir.child("2021-08-23-first_entry.md")
                .write_str("first content")
                .unwrap();

            let journal = Journal::new_at(dir.path());

            let entry = journal.latest_entry();

            assert!(entry.is_ok());
            let entry = entry.unwrap();
            assert_eq!(
                true,
                contains("2021-08-23-first_entry.md").eval(&entry.path.to_string_lossy())
            );
            assert_eq!(entry.markdown, "first content");
        }
    }

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
