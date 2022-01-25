use anyhow::Result;
use clap::StructOpt;
use figment::{
    providers::{Env, Format, Yaml},
    value::{Uncased, UncasedStr},
    Figment,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::{
    github::PullRequestConfig, jira::JiraConfig, reminders::ReminderConfig, todo::TodoConfig,
};

#[derive(Debug, StructOpt)]
pub enum ConfigCmd {
    /// Show the current configuration that is loaded
    Show,
}

impl ConfigCmd {
    pub fn execute(&self, config: &Config) -> Result<()> {
        match self {
            ConfigCmd::Show => {
                serde_yaml::to_writer(std::io::stdout(), config).map_err(|e| anyhow::anyhow!(e))
            }
        }
    }
}

/// Configuration we can get either from a file or from ENV variables
#[derive(Serialize, Deserialize)]
pub struct Config {
    pub dir: PathBuf,
    pub pull_requests: Option<PullRequestConfig>,
    pub reminders: Option<ReminderConfig>,
    pub jira: Option<JiraConfig>,
    #[serde(default)]
    pub todo: TodoConfig,
    #[serde(default = "default_order")]
    pub sections: Vec<SectionName>,
    pub notes: Option<NotesConfig>,
}

#[derive(Serialize, Deserialize)]
pub struct NotesConfig {
    pub enabled: bool,
    pub template: Option<String>,
}

impl Default for NotesConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            template: None,
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Hash)]
pub enum SectionName {
    #[serde(rename = "notes")]
    Notes,
    #[serde(rename = "todos")]
    Todos,
    #[serde(rename = "pull_requests")]
    Prs,
    #[serde(rename = "jira")]
    Tasks,
    #[serde(rename = "reminders")]
    Reminders,
}

pub fn default_order() -> Vec<SectionName> {
    use SectionName::*;
    vec![Notes, Todos, Prs, Tasks, Reminders]
}

fn double_underscore_separated(input: &UncasedStr) -> Uncased<'_> {
    Uncased::new(input.as_str().replace("__", "."))
}

impl Config {
    pub fn load() -> Result<Self, figment::Error> {
        let config_path = std::env::var("JOURNAL__CONFIG").map_or_else(
            |_| {
                let home = dirs::home_dir().expect("Unable to get the the users 'home' directory");
                home.join(".journal.yaml")
            },
            PathBuf::from,
        );

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

#[cfg(test)]
mod tests {
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
}
