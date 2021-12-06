#![allow(dead_code)]

use anyhow::{bail, Context, Result};
use clap::{AppSettings, Parser};
use figment::{
    providers::{Env, Format, Yaml},
    value::{Uncased, UncasedStr},
    Figment,
};

use serde::Deserialize;
use std::{path::PathBuf, str::FromStr};
use tera::{Context as TeraContext, Tera};
use time::{format_description, OffsetDateTime};
use tracing::Level;

use github::PullRequestConfig;

const DAY_TEMPLATE: &str = include_str!("../template/day.md");

mod github;
mod todo;

/// Configuration we can get either from a file or from ENV variables
#[derive(Deserialize)]
struct Config {
    dir: String,
    pull_requests: PullRequestConfig,
}

fn double_underscore_separated(input: &UncasedStr) -> Uncased<'_> {
    Uncased::new(input.as_str().replace("__", "."))
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
            .merge(Env::prefixed("JOURNAL_").map(double_underscore_separated))
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
    New {
        title: String,
        #[clap(short = 's', long = "stdout")]
        write_to_stdout: bool,
    },
}

fn to_level<S: AsRef<str>>(level: S) -> Result<Level, ()> {
    Level::from_str(level.as_ref()).map_err(|_| ())
}

fn init_logs() {
    let level = std::env::var("JOURNAL_LOG_LEVEL")
        .map_err(|_| ())
        .and_then(to_level)
        .unwrap_or(Level::ERROR);

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

fn normalize_filename(raw: &str) -> String {
    let r = regex::Regex::new(r#"[\(\)\[\]?']"#).unwrap();
    let lower = raw.to_lowercase().replace(" ", "-");
    r.replace_all(&lower, "").to_string()
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
            tracing::info!("Lastest entry found at {:?}", path);

            return Ok(Entry { path, markdown });
        }

        bail!("No journal entries found in {:?}", self.location);
    }

    fn add_entry(&self, name: &str, data: &str) -> Result<()> {
        let path = self.location.join(name);
        std::fs::write(path, data)?;
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logs();

    let config = Config::load().context("Failed to load configuration")?;
    let cli = Cli::parse();
    let journal = Journal::new_at(config.dir);

    match cli.cmd {
        Cmd::New {
            title,
            write_to_stdout,
        } => {
            let latest_entry = journal.latest_entry()?;

            let mut finder = todo::FindTodos::new();
            let open_todos = finder.process(&latest_entry.markdown);

            let prs = config.pull_requests.get_matching_prs().await?;

            let mut tera = Tera::default();
            tera.add_raw_template("day.md", DAY_TEMPLATE).unwrap();

            let year_month_day = format_description::parse("[year]-[month]-[day]").unwrap();
            let today = OffsetDateTime::now_utc().format(&year_month_day)?;

            let mut context = TeraContext::new();
            context.insert("title", &title);
            context.insert("date", &today);
            context.insert("open_todos", &open_todos);
            context.insert("prs", &prs);

            let out = tera.render("day.md", &context).unwrap();

            if write_to_stdout {
                print!("{}", out);
            } else {
                let file_title = normalize_filename(&title);
                let new_filename = format!("{}-{}.md", today, file_title);

                journal.add_entry(&new_filename, &out)?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod test {
    mod title {
        use data_test::data_test;

        data_test! {
            fn title_for_filename(input, expected) => {
                assert_eq!(crate::normalize_filename(input), expected);
            }
            - a ("Easy simple lowercase", "easy-simple-lowercase")
            - b ("What's the plan?", "whats-the-plan")
            - c ("What's ([)the] plan?", "whats-the-plan")
        }
    }
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
        use crate::github::Auth;
        use crate::Config;

        #[test]
        fn config_read_from_yml() {
            figment::Jail::expect_with(|jail| {
                let config_path = jail.directory().join(".journal.yml");
                jail.set_env("JOURNAL_CONFIG", config_path.to_string_lossy());

                jail.create_file(
                    ".journal.yml",
                    indoc::indoc! { r#"
                        dir: file/from/yaml
                        pull_requests:
                          auth:
                            personal_access_token: "my-access-token"
                          select:
                            - repo: felipesere/sane-flags
                              authors:
                                - felipesere
                        "#
                    },
                )?;

                let config = Config::load()?;
                assert_eq!(config.dir, "file/from/yaml");
                assert!(config.pull_requests.is_some());

                Ok(())
            });
        }

        #[ignore] // temporary, while I iterate
        #[test]
        fn config_read_from_env() {
            figment::Jail::expect_with(|jail| {
                let config_path = jail.directory().join(".journal.yml");
                jail.set_env("JOURNAL_CONFIG", config_path.to_string_lossy());

                jail.create_file(".journal.yml", r#"dir: file/from/yaml"#)?;
                jail.set_env("JOURNAL_DIR", "env/set/the/dir");
                jail.set_env(
                    "JOURNAL_PULL_REQUESTS__AUTH__PERSONAL_ACCESS_TOKEN",
                    "my-access-token",
                );

                let config = Config::load()?;

                assert_eq!(config.dir, "env/set/the/dir");
                assert!(config.pull_requests.is_some());

                Ok(())
            });
        }
    }
}
