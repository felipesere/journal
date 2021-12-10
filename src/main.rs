use anyhow::{Context, Result};
use clap::{AppSettings, Parser};
use figment::{
    providers::{Env, Format, Yaml},
    value::{Uncased, UncasedStr},
    Figment,
};

use serde::Deserialize;
use std::{path::PathBuf, str::FromStr};
use time::{format_description::FormatItem, OffsetDateTime};
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
    dir: PathBuf,
    pull_requests: Option<PullRequestConfig>,
    reminders: Option<ReminderConfig>,
}

fn double_underscore_separated(input: &UncasedStr) -> Uncased<'_> {
    Uncased::new(input.as_str().replace("__", "."))
}

const YEAR_MONTH_DAY: &'static [FormatItem] =
    time::macros::format_description!("[year]-[month]-[day]");

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
            .merge(Env::prefixed("JOURNAL__").map(double_underscore_separated))
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
    /// Add a new reminder, either on a specific date or recurring.
    New {
        #[clap(long = "on", group = "date_selection")]
        on_date: Option<SpecificDate>,

        #[clap(long = "every", group = "date_selection")]
        every: Option<RepeatingDate>,

        #[clap(takes_value(true))]
        reminder: String,
    },
    /// List all existing reminders
    List,
    /// Delete a reminder
    Delete {
        /// The number to delete
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

fn execute_reminder(cmd: ReminderCmd, config: Config, clock: &impl Clock) -> Result<()> {
    let with_reminders = config
        .reminders
        .as_ref()
        .map(|c| c.enabled)
        .unwrap_or(false);

    if !with_reminders {
        println!("No reminder configuration set. Please add it first");
        return Ok(());
    }

    let location = config.dir.join("reminders.json");
    let mut reminders_storage = Reminders::load(&location)?;

    match cmd {
        ReminderCmd::Delete { nr } => {
            tracing::info!("intention to delete reminder");

            reminders_storage.delete(nr)?;

            println!("Deleted {}", nr,);
        }
        ReminderCmd::List => {
            tracing::info!("intention to list reminders");

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
        ReminderCmd::New {
            on_date: specific_date_spec,
            every: interval_spec,
            reminder,
        } => {
            tracing::info!("intention to create a new reminder");

            if let Some(date_spec) = specific_date_spec {
                let next = date_spec.next_date(clock.today());

                reminders_storage.on_date(next, reminder.clone());

                println!(
                    "Added a reminder for '{}' on '{}'",
                    reminder,
                    next.format(YEAR_MONTH_DAY)?
                );
            }

            if let Some(interval_spec) = interval_spec {
                reminders_storage.every(clock, &interval_spec, &reminder);

                println!(
                    "Added a reminder for '{}' every '{}'",
                    reminder, interval_spec,
                );
            }
        }
    }

    reminders_storage
        .save(&location)
        .context("Failed to save reminders")?;

    tracing::info!("Saved reminders");

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logs();

    let config = Config::load().context("Failed to load configuration")?;
    let cli = Cli::parse();
    let journal = Journal::new_at(config.dir.clone());
    let clock = WallClock;

    match cli.cmd {
        Cmd::Reminder(cmd) => {
            execute_reminder(cmd, config, &clock)?;
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

            let reminders = if let Some(ReminderConfig { enabled: true }) = config.reminders {
                let location = config.dir.join("reminders.json");
                let reminders = Reminders::load(&location)?;

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
        use std::path::PathBuf;

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
                jail.set_env("JOURNAL_CONFIG", config_path.to_string_lossy());

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
