use anyhow::Result;
use clap::{AppSettings, StructOpt};

use std::collections::HashMap;
use std::path::Path;

use config::ConfigCmd;
pub use reminders::{Clock, ReminderCmd, ReminderConfig, Reminders, WallClock};
use storage::Journal;
use template::Template;

pub use config::Config;

mod config;
mod github;
mod jira;
mod reminders;
mod storage;
mod template;
mod todo;

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

    #[clap(subcommand)]
    Config(ConfigCmd),
}

fn normalize_filename(raw: &str) -> String {
    let r = regex::Regex::new(r#"[\(\)\[\]?']"#).unwrap();
    let lower = raw.to_lowercase().replace(" ", "-");
    r.replace_all(&lower, "").to_string()
}

pub async fn run<O>(cli: Cli, config: &Config, clock: &impl Clock, open: O) -> Result<()>
where
    O: FnOnce(&Path) -> Result<()>,
{
    let journal = Journal::new_at(config.dir.clone());

    match cli.cmd {
        Cmd::Config(cmd) => cmd.execute(config)?,
        Cmd::Reminder(cmd) => {
            let with_reminders = config.reminders.as_ref().map_or(false, |c| c.is_enabled());

            if with_reminders {
                cmd.execute(config, clock)?;
            } else {
                println!("No reminder configuration set. Please add it first");
            }
        }
        Cmd::New {
            title,
            write_to_stdout,
        } => {
            let mut sections = HashMap::new();

            let enabled_sections = config.enabled_sections();

            for (name, section) in &enabled_sections {
                sections.insert(name.clone(), section.render(&journal, clock).await?);
            }

            let today = clock.today();

            let template = Template {
                title: title.clone(),
                today,
                sections,
            };

            let out = template.render(config.sections.clone())?;

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
#[path = "controlled_clock.rs"]
mod controlled_clock;

#[cfg(test)]
mod test {
    use std::sync::{Arc, Mutex};

    use crate::config::{Enabled, NotesConfig};
    use crate::todo::TodoConfig;

    use super::controlled_clock::ControlledClock;
    use super::*;
    use assert_fs::{prelude::*, TempDir};
    use predicates::{path::exists, str::diff};
    use time::ext::NumericalDuration;
    use time::Month::April;

    #[ignore]
    #[tokio::test]
    async fn creats_various_entries_on_the_filesystem() -> Result<()> {
        let journal_home = TempDir::new()?;
        let config = Config {
            dir: journal_home.to_path_buf(),
            pull_requests: None,
            reminders: None,
            jira: None,
            todo: TodoConfig::default(),
            sections: Vec::new(),
            notes: Some(Enabled::new(NotesConfig::default())),
        };
        let open_was_called = Arc::new(Mutex::new(false));
        let open = |_: &Path| {
            *open_was_called.lock().unwrap() = true;

            Ok(())
        };
        let mut clock = ControlledClock::new(2020, April, 22)?;

        let cli = Cli::parse_from(&["journal", "new", "This is great"]);
        run(cli, &config, &clock, open).await?;
        assert!(*open_was_called.lock().unwrap());
        journal_home
            .child("2020-04-22-this-is-great.md")
            .assert(exists());

        clock.advance_by(1.days());
        let cli = Cli::parse_from(&["journal", "new", "The Next One"]);
        run(cli, &config, &clock, open).await?;
        journal_home
            .child("2020-04-23-the-next-one.md")
            .assert(exists())
            .assert(diff(indoc::indoc! {r#"
                # The Next One on 2020-04-23

                ## Notes


                > This is where your notes will go!

                ## TODOs

                "#}));
        Ok(())
    }

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
}
