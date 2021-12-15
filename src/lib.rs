use anyhow::Result;
use clap::{AppSettings, StructOpt};
use figment::{
    providers::{Env, Format, Yaml},
    value::{Uncased, UncasedStr},
    Figment,
};
use serde::Deserialize;
use std::path::{Path, PathBuf};

use github::PullRequestConfig;
pub use reminders::{Clock, ReminderCmd, ReminderConfig, Reminders, WallClock};
use storage::Journal;
use template::Template;

mod github;
mod reminders;
mod storage;
mod template;
mod todo;

/// Configuration we can get either from a file or from ENV variables
#[derive(Deserialize)]
pub struct Config {
    dir: PathBuf,
    pull_requests: Option<PullRequestConfig>,
    reminders: Option<ReminderConfig>,
}

fn double_underscore_separated(input: &UncasedStr) -> Uncased<'_> {
    Uncased::new(input.as_str().replace("__", "."))
}

impl Config {
    pub fn load() -> Result<Self, figment::Error> {
        let config_path = std::env::var("JOURNAL__CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = dirs::home_dir().expect("Unable to get the the users 'home' directory");
                home.join(".journal.yaml")
            });

        if !config_path.exists() {
            return Err(figment::Error::from(format!("{} does not exist. We need a configuration file to work.\nYou can either use a '.journal.yaml' file in your HOME directory or configure it with the JOURNAL__CONFIG environment variable", config_path.to_string_lossy())));
        }

        tracing::info!("Loading config from {:?}", config_path);
        Figment::new()
            .merge(Yaml::file(config_path))
            .merge(Env::prefixed("JOURNAL__").map(double_underscore_separated))
            .extract()
    }
}

/// Commands and arguments passed via the command line
#[derive(Debug, StructOpt)]
#[clap(
    author = "Felipe Sere <journal@felipesere.com>",
    version,
    setting = AppSettings::DeriveDisplayOrder,
)]
pub struct Cli {
    #[clap(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, StructOpt)]
enum Cmd {
    New {
        title: String,
        #[clap(short = 's', long = "stdout")]
        write_to_stdout: bool,
    },
    #[clap(subcommand)]
    Reminder(ReminderCmd),
}

fn normalize_filename(raw: &str) -> String {
    let r = regex::Regex::new(r#"[\(\)\[\]?']"#).unwrap();
    let lower = raw.to_lowercase().replace(" ", "-");
    r.replace_all(&lower, "").to_string()
}

pub async fn run<O>(cli: Cli, config: Config, clock: impl Clock, open: O) -> Result<()>
where
    O: FnOnce(&Path) -> Result<()>,
{
    let journal = Journal::new_at(config.dir.clone());

    match cli.cmd {
        Cmd::Reminder(cmd) => {
            let with_reminders = config
                .reminders
                .as_ref()
                .map(|c| c.enabled)
                .unwrap_or(false);

            if !with_reminders {
                println!("No reminder configuration set. Please add it first");
            } else {
                cmd.execute(config, &clock)?;
            }
        }
        Cmd::New {
            title,
            write_to_stdout,
        } => {
            let todos = match journal.latest_entry() {
                Ok(Some(latest_entry)) => {
                    let mut finder = todo::FindTodos::new();
                    finder.process(&latest_entry.markdown)
                }
                Ok(None) => Vec::new(),
                Err(e) => return Err(anyhow::anyhow!(e)),
            };

            let prs = if let Some(config) = config.pull_requests {
                let prs = config.get_matching_prs().await?;
                Some(prs)
            } else {
                None
            };

            let reminders = if let Some(ReminderConfig { enabled: true }) = config.reminders {
                let location = config.dir.join("reminders.json");
                let reminders = Reminders::load(&location)?;

                Some(reminders.for_today(&clock))
            } else {
                None
            };

            let today = clock.today();

            let template = Template {
                title: title.clone(),
                today,
                todos,
                prs,
                reminders,
            };

            let out = template.render()?;

            if write_to_stdout {
                print!("{}", out);
            } else {
                let file_title = normalize_filename(&title);
                let new_filename = format!("{}-{}.md", today, file_title);

                let stored = journal.add_entry(&new_filename, &out)?;

                open(&stored)?;
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

    mod config {
        use std::path::PathBuf;

        use crate::Config;

        #[test]
        fn config_read_from_yml() {
            figment::Jail::expect_with(|jail| {
                let config_path = jail.directory().join(".journal.yml");
                jail.set_env("JOURNAL__CONFIG", config_path.to_string_lossy());

                jail.create_file(
                    ".journal.yml",
                    indoc::indoc! { r#"
                        dir: file/from/yaml
                        pull_requests:
                          enabled: true
                          auth:
                            personal_access_token: "my-access-token"
                          select:
                            - repo: felipesere/sane-flags
                              authors:
                                - felipesere
                        reminders:
                            enabled: true
                        "#
                    },
                )?;

                let config = Config::load()?;
                assert_eq!(config.dir, PathBuf::from("file/from/yaml"));
                assert!(config.pull_requests.is_some());
                assert!(config.reminders.is_some());

                Ok(())
            });
        }

        #[ignore] // temporary, while I iterate
        #[test]
        fn config_read_from_env() {
            figment::Jail::expect_with(|jail| {
                let config_path = jail.directory().join(".journal.yml");
                jail.set_env("JOURNAL__CONFIG", config_path.to_string_lossy());

                jail.create_file(".journal.yml", r#"dir: file/from/yaml"#)?;
                jail.set_env("JOURNAL_DIR", "env/set/the/dir");
                jail.set_env(
                    "JOURNAL_PULL_REQUESTS__AUTH__PERSONAL_ACCESS_TOKEN",
                    "my-access-token",
                );

                let config = Config::load()?;

                assert_eq!(config.dir, PathBuf::from("env/set/the/dir"));
                assert!(config.pull_requests.is_some());

                Ok(())
            });
        }
    }
}
