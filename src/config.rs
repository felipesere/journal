use anyhow::Result;
use clap::StructOpt;
use figment::{
    providers::{Env, Format, Yaml},
    value::{Uncased, UncasedStr},
    Figment,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::github::PullRequestConfig;
use crate::reminders::ReminderConfig;

#[derive(Debug, StructOpt)]
pub enum ConfigCmd {
    /// Show the current configuration that is loaded
    Show,
}

impl ConfigCmd {
    pub fn execute(&self, config: Config) -> Result<()> {
        match self {
            ConfigCmd::Show => {
                serde_yaml::to_writer(std::io::stdout(), &config).map_err(|e| anyhow::anyhow!(e))
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::github::Auth;
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

    #[ignore]
    #[test]
    fn config_read_from_env() {
        figment::Jail::expect_with(|jail| {
            let config_path = jail.directory().join(".journal.yml");
            jail.set_env("JOURNAL__CONFIG", config_path.to_string_lossy());

            jail.create_file(".journal.yml", r#"dir: file/from/yaml"#)?;
            jail.set_env("JOURNAL__DIR", "env/set/the/dir");
            jail.set_env(
                "JOURNAL_PULL_REQUESTS__AUTH_PERSONAL_ACCESS_TOKEN",
                "my-access-token",
            );

            jail.set_env("JOURNAL_PULL_REQUESTS__ENABLED", "true");

            let config = Config::load()?;

            assert_eq!(config.dir, PathBuf::from("env/set/the/dir"));

            assert!(config.pull_requests.is_some());
            let pull_requests = config.pull_requests.unwrap();
            assert_eq!(
                pull_requests.auth,
                Auth::PersonalAccessToken("my-access-token".into())
            );

            Ok(())
        });
    }
}
