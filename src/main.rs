use anyhow::{Context, Result};
use clap::{AppSettings, Parser};
use figment::{
    providers::{Env, Format, Yaml},
    value::{Uncased, UncasedStr},
    Figment,
};

use serde::Deserialize;
use std::{path::PathBuf, str::FromStr};
use time::OffsetDateTime;
use tracing::Level;

use github::PullRequestConfig;
use storage::Journal;

use reminders::{Clock, ReminderConfig, Reminders, RepeatingDate, SpecificDate, WallClock};
use template::Template;

mod github;
mod reminders;
mod storage;
mod template;
mod todo;

/// Configuration we can get either from a file or from ENV variables
#[derive(Deserialize)]
struct Config {
    dir: String,
    pull_requests: Option<PullRequestConfig>,
    reminders: Option<ReminderConfig>,
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

        tracing::info!("Loading config from {:?}", config_path);
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
    #[clap(subcommand)]
    Reminder(ReminderCmd),
}

#[derive(Debug, Parser)]
#[clap(alias = "reminders")]
enum ReminderCmd {
    New {
        #[clap(long = "on", group = "date_selection")]
        on_date: Option<SpecificDate>,

        #[clap(long = "every", group = "date_selection")]
        every: Option<RepeatingDate>,

        #[clap(takes_value(true))]
        reminder: String,
    },
    List,
    Delete {
        nr: u32,
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

fn normalize_filename(raw: &str) -> String {
    let r = regex::Regex::new(r#"[\(\)\[\]?']"#).unwrap();
    let lower = raw.to_lowercase().replace(" ", "-");
    r.replace_all(&lower, "").to_string()
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logs();

    let config = Config::load().context("Failed to load configuration")?;
    let cli = Cli::parse();
    let journal = Journal::new_at(config.dir);

    match cli.cmd {
        Cmd::Reminder(ReminderCmd::Delete { nr }) => {
            if config.reminders.is_none() {
                println!("No reminder configuration set. Please add it first");
                return Ok(());
            }
            tracing::info!("intention to delete reminder");

            let location = config.reminders.unwrap().location;

            let mut reminders_storage = Reminders::load(&location)?;

            reminders_storage.delete(nr);

            reminders_storage
                .save(&location)
                .context("Failed to save reminders")?;

            tracing::info!("Saved reminders");
            println!("Deleted {}", nr,);
        }
        Cmd::Reminder(ReminderCmd::List) => {
            if config.reminders.is_none() {
                println!("No reminder configuration set. Please add it first");
                return Ok(());
            }
            tracing::info!("intention to list reminders");
            let location = config.reminders.unwrap().location;

            let reminders_storage = Reminders::load(&location)?;

            let reminders = reminders_storage.all();
            // temp:
            use comfy_table::{
                modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, ContentArrangement, Table,
            };
            let mut table = Table::new();
            table
                .load_preset(UTF8_FULL)
                .apply_modifier(UTF8_ROUND_CORNERS)
                .set_content_arrangement(ContentArrangement::Dynamic)
                .set_header(vec!["Nr", "Date", "Reminders"]);

            for reminder in reminders {
                table.add_row(vec![
                    reminder.nr.to_string(),
                    format!("{}", reminder.date),
                    reminder.reminder,
                ]);
            }

            println!("{}", table);
        }
        Cmd::Reminder(ReminderCmd::New {
            on_date: specific_date_spec,
            every: interval_spec,
            reminder,
        }) => {
            if config.reminders.is_none() {
                println!("No reminder configuration set. Please add it first");
                return Ok(());
            }
            tracing::info!("intention to create a new reminder");
            let location = config.reminders.unwrap().location;

            let mut reminders = Reminders::load(&location)?;

            let clock = WallClock;
            if let Some(date_spec) = specific_date_spec {
                let next = date_spec.next_date(clock.today());

                reminders.on_date(next, reminder.clone());

                reminders
                    .save(&location)
                    .context("Failed to save reminders")?;

                tracing::info!("Saved reminders");
                let year_month_day = time::macros::format_description!("[year]-[month]-[day]");
                println!(
                    "Added a reminder for '{}' on '{}'",
                    reminder,
                    next.format(&year_month_day)?
                );
            }

            if let Some(interval_spec) = interval_spec {
                reminders.every(&clock, &interval_spec, &reminder);

                reminders
                    .save(&location)
                    .context("Failed to save reminders")?;

                println!(
                    "Added a reminder for '{}' every '{}'",
                    reminder, interval_spec,
                );
            }
        }
        Cmd::New {
            title,
            write_to_stdout,
        } => {
            let latest_entry = journal.latest_entry()?;
            let mut finder = todo::FindTodos::new();
            let todos = finder.process(&latest_entry.markdown);

            let prs = if let Some(config) = config.pull_requests {
                let prs = config.get_matching_prs().await?;
                Some(prs)
            } else {
                None
            };

            let reminders = if let Some(ReminderConfig { location: dir }) = config.reminders {
                let clock = WallClock;
                let reminders = Reminders::load(&dir)?;

                Some(reminders.for_today(&clock))
            } else {
                None
            };

            let today = OffsetDateTime::now_utc();

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

    mod config {
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
                        reminders:
                            location: "abc"
                        "#
                    },
                )?;

                let config = Config::load()?;
                assert_eq!(config.dir, "file/from/yaml");
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
