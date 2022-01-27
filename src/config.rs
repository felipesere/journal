use anyhow::{bail, Result};
use clap::StructOpt;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, io::Read, path::PathBuf};

use crate::notes::NotesConfig;
use crate::{
    github::PullRequestConfig, jira::JiraConfig, reminders::ReminderConfig, storage::Journal,
    todo::TodoConfig, Clock,
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
    #[serde(default = "default_order")]
    pub sections: Vec<SectionName>,
    pub dir: PathBuf,

    #[serde(default)]
    pub todos: Enabled<TodoConfig>,
    #[serde(default)]
    pub notes: Enabled<NotesConfig>,
    #[serde(default)]
    pub reminders: Enabled<ReminderConfig>,

    pub jira: Option<Enabled<JiraConfig>>,

    pub pull_requests: Option<Enabled<PullRequestConfig>>,
}

#[derive(Serialize, Deserialize)]
pub struct Enabled<T> {
    enabled: bool,
    #[serde(flatten)]
    inner: T,
}

impl<T: Default> Default for Enabled<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T> Enabled<T> {
    pub fn new(inner: T) -> Enabled<T> {
        Self {
            enabled: true,
            inner,
        }
    }
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

impl Config {
    pub fn enabled_sections(&self) -> HashMap<SectionName, Box<dyn Section>> {
        let mut sections = HashMap::new();

        if self.todos.is_enabled() {
            sections.insert(
                SectionName::Todos,
                Box::new(self.todos.inner.clone()) as Box<dyn Section>,
            );
        }

        if self.notes.is_enabled() {
            sections.insert(
                SectionName::Notes,
                Box::new(self.notes.inner.clone()) as Box<dyn Section>,
            );
        }

        if self.reminders.is_enabled() {
            sections.insert(
                SectionName::Reminders,
                Box::new(self.reminders.inner.clone()) as Box<dyn Section>,
            );
        }

        if let Some(ref jira) = self.jira {
            if jira.is_enabled() {
                sections.insert(
                    SectionName::Tasks,
                    Box::new(jira.inner.clone()) as Box<dyn Section>,
                );
            }
        }

        if let Some(ref pull_requests) = &self.pull_requests {
            if pull_requests.enabled {
                sections.insert(
                    SectionName::Prs,
                    Box::new(pull_requests.inner.clone()) as Box<dyn Section>,
                );
            }
        }

        sections
    }
}

#[async_trait::async_trait]
pub trait Section {
    async fn render(&self, journal: &Journal, clock: &dyn Clock) -> Result<String>;
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Debug, Hash)]
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

impl Config {
    pub fn config_path() -> Result<PathBuf> {
        let config_path = std::env::var("JOURNAL__CONFIG").map_or_else(
            |_| {
                let home = dirs::home_dir().expect("Unable to get the the users 'home' directory");
                home.join(".journal.yaml")
            },
            PathBuf::from,
        );

        if !config_path.exists() {
            bail!(format!("{} does not exist. We need a configuration file to work.\nYou can either use a '.journal.yaml' file in your HOME directory or configure it with the JOURNAL__CONFIG environment variable", config_path.to_string_lossy()));
        }

        Ok(config_path)
    }

    pub fn from_reader(reader: impl Read) -> Result<Self> {
        serde_yaml::from_reader(reader).map_err(|e| anyhow::anyhow!(e))
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;
    use std::collections::HashSet;
    use std::path::PathBuf;

    use crate::config::SectionName::*;
    use crate::Config;

    #[test]
    fn minimal_config() {
        let r = indoc! { r#"
                    dir: file/from/yaml
                    "#
        };

        let config = Config::from_reader(r.as_bytes()).unwrap();
        assert_eq!(config.dir, PathBuf::from("file/from/yaml"));

        let sections: HashSet<_> = config.enabled_sections().into_keys().collect();
        assert_eq!(sections, set(vec![Todos, Notes, Reminders]));
    }

    #[test]
    fn minimal_config_with_all_defaults_disabled() {
        let r = indoc! { r#"
                     dir: file/from/yaml

                     reminders:
                         enabled: false

                     notes:
                         enabled: false

                     todos:
                         enabled: false
                    "#
        };

        let config = Config::from_reader(r.as_bytes()).unwrap();
        assert_eq!(config.dir, PathBuf::from("file/from/yaml"));

        let sections: HashSet<_> = config.enabled_sections().into_keys().collect();
        assert_eq!(sections, set(vec![]));
    }

    #[test]
    fn config_read_from_yml() {
        let r = indoc! { r#"
                    dir: file/from/yaml

                    pull_requests:
                      enabled: true
                      auth:
                        personal_access_token: "my-access-token"
                      select:
                        - repo: felipesere/sane-flags
                          authors:
                            - felipesere
                    "#
        };

        let config = Config::from_reader(r.as_bytes()).unwrap();
        assert_eq!(config.dir, PathBuf::from("file/from/yaml"));

        let sections: HashSet<_> = config.enabled_sections().into_keys().collect();
        assert_eq!(sections, set(vec![Prs, Todos, Notes, Reminders]));
    }

    fn set<T: std::hash::Hash + std::cmp::Eq>(elements: Vec<T>) -> HashSet<T> {
        HashSet::from_iter(elements)
    }
}
