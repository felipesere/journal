use std::collections::HashSet;
use std::str::FromStr;

use anyhow::Result;
use futures::future::join_all;
use handlebars::Handlebars;
use octocrab::{models::pulls::PullRequest, Octocrab, OctocrabBuilder, Page};
use secrecy::{ExposeSecret, Secret};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use tokio::task::JoinHandle;
use tracing::{instrument, Instrument};

use crate::config::Section;

/// Configuration for how journal should get outstanding Pull/Merge requests
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PullRequestConfig {
    pub(crate) auth: Auth,
    select: Vec<PrSelector>,
    template: Option<String>,
}

const PRS: &str = r#"
## Pull Requests:

{{#each prs as | pr | }}
* [ ] `{{pr.title}}` on [{{pr.repo}}]({{pr.url}}) by {{pr.author}}
{{/each }}
"#;

#[async_trait::async_trait]
impl Section for PullRequestConfig {
    async fn render(&self, _: &crate::storage::Journal, _: &dyn crate::Clock) -> Result<String> {
        let prs = self.get_matching_prs().await?;

        #[derive(Serialize)]
        struct C {
            prs: Vec<Pr>,
        }

        let template = self.template.clone().unwrap_or_else(|| PRS.to_string());

        let mut tt = Handlebars::new();
        tt.register_template_string("prs", template)?;
        tt.register_escape_fn(handlebars::no_escape);
        tt.render("prs", &C { prs }).map_err(|e| anyhow::anyhow!(e))
    }
}

impl PullRequestConfig {
    pub async fn get_matching_prs(&self) -> Result<Vec<Pr>> {
        let Auth::PersonalAccessToken(ref token) = self.auth;

        let octocrab = OctocrabBuilder::new()
            .personal_token(token.expose_secret().to_string())
            .build()?;
        let user = octocrab.current().user().await?;
        tracing::info!("Logged into GitHub as {}", user.login);
        tracing::info!("Selections for PRs: {:?}", self.select);

        let mut join_handles = Vec::new();
        for selector in &self.select {
            let selector = selector.clone();
            let token = token.clone();
            let handle: JoinHandle<Result<Vec<Pr>>> = tokio::spawn(
                async move {
                    // Make life easy and just create multiple instances
                    let octocrab = OctocrabBuilder::new()
                        .personal_token(token.expose_secret().to_string())
                        .build()?;
                    selector.get_prs(&octocrab).await
                }
                .instrument(tracing::info_span!("getting prs")),
            );

            join_handles.push(handle);
        }

        let task_results = join_all(join_handles).await;
        let mut prs = Vec::new();
        for task in task_results {
            prs.extend(task??); // double unwrapping, facepalm
        }

        Ok(prs)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PrSelector {
    repo: Repo,
    #[serde(flatten)]
    filter: LocalFilter,
}

impl LocalFilter {
    fn apply(&self, pr: &Pr) -> bool {
        let mut applies = true;
        if !self.authors.is_empty() {
            applies = applies && self.authors.contains(&pr.author);
        }
        if !self.labels.is_empty() {
            applies = applies && self.labels.intersection(&pr.labels).count() > 0;
        }
        applies
    }
}
impl PrSelector {
    #[instrument(skip(octocrab))]
    pub async fn get_prs(&self, octocrab: &Octocrab) -> Result<Vec<Pr>> {
        let Repo { owner, name } = self.repo.clone();

        tracing::info!("Getting PRs for org={} repo={}", owner, name);
        let mut current_page = octocrab
            .pulls(&owner, &name)
            .list()
            .state(octocrab::params::State::Open)
            .per_page(50)
            .send()
            .await?;

        let mut prs = self.extract_prs(&mut current_page);

        while let Ok(Some(mut next_page)) = octocrab.get_page(&current_page.next).await {
            tracing::info!("Getting next page of PRs for org={} repo={}", owner, name);
            prs.extend(self.extract_prs(&mut next_page));

            current_page = next_page;
        }

        Ok(prs)
    }

    /// Converts the PullRequest to the internal format and applies the filters
    fn extract_prs(&self, page: &mut Page<PullRequest>) -> Vec<Pr> {
        page.take_items()
            .iter()
            .map(Pr::from)
            .filter(|pr| self.filter.apply(pr))
            .collect::<Vec<_>>()
    }
}

#[derive(Debug, Clone)]
struct Repo {
    owner: String,
    name: String,
}

impl Serialize for Repo {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format!("{}/{}", self.owner, self.name))
    }
}

impl FromStr for Repo {
    type Err = String;

    fn from_str(repo: &str) -> Result<Self, Self::Err> {
        let repo_components = repo.split('/').map(ToString::to_string).collect::<Vec<_>>();
        if repo_components.len() != 2 {
            return Result::Err(format!("\"{}\" did not have exactly 2 components", repo));
        }
        Ok(Repo {
            owner: repo_components[0].to_string(),
            name: repo_components[1].to_string(),
        })
    }
}
impl<'de> Deserialize<'de> for Repo {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        FromStr::from_str(&s).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct LocalFilter {
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub(crate) authors: HashSet<String>,

    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub(crate) labels: HashSet<String>,
}

#[derive(Deserialize, Serialize, Clone)]
pub(crate) enum Auth {
    #[serde(rename = "personal_access_token", serialize_with = "only_asterisk")]
    PersonalAccessToken(Secret<String>),
}

impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            &Self::PersonalAccessToken(_) => f.write_str("***"),
        }
    }
}

fn only_asterisk<S>(_: &Secret<String>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str("***")
}

#[derive(Debug, Serialize)]
pub struct Pr {
    pub(crate) author: String,
    pub(crate) labels: HashSet<String>,
    pub(crate) repo: String,
    pub(crate) title: String,
    pub(crate) url: String,
}

impl From<&PullRequest> for Pr {
    fn from(raw: &PullRequest) -> Self {
        Pr {
            author: raw.user.as_ref().unwrap().login.clone(),
            labels: raw
                .labels
                .clone()
                .unwrap_or_default()
                .iter()
                .map(|l| l.name.clone())
                .collect(),
            repo: raw
                .base
                .repo
                .as_ref()
                .unwrap()
                .full_name
                .as_ref()
                .unwrap()
                .to_string(),
            title: raw.title.clone().unwrap(),
            url: raw.html_url.as_ref().unwrap().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod config {
        use super::*;
        use anyhow::Result;
        use indoc::indoc;

        #[test]
        fn parse_config() -> Result<()> {
            let input = indoc! { r#"
            enabled: true
            auth:
              personal_access_token: abc
            select:
                - repo: felipesere/journal
                  labels:
                    - foo
                    - bar
            "#
            };

            let pr_config: PullRequestConfig = serde_yaml::from_str(input)?;
            assert_eq!(pr_config.select.len(), 1);
            let selection = &pr_config.select[0];

            assert!(selection.filter.labels.contains("foo"));
            assert!(selection.filter.labels.contains("bar"));

            Ok(())
        }

        #[test]
        fn filter_applies_when_author_matches() {
            let filter = LocalFilter {
                authors: set(&["felipe"]),
                labels: set(&[]),
            };

            let mut pr = Pr {
                author: "felipe".into(),
                labels: set(&[]),
                repo: "...".into(),
                title: "...".into(),
                url: "...".into(),
            };

            assert!(filter.apply(&pr));

            pr.author = "anna".into();
            assert!(!filter.apply(&pr))
        }

        #[test]
        fn filter_applies_at_least_one_label_matches() {
            let filter = LocalFilter {
                authors: set(&[]),
                labels: set(&["foo"]),
            };

            let mut pr = Pr {
                author: "...".into(),
                labels: set(&["foo", "bar"]),
                repo: "...".into(),
                title: "...".into(),
                url: "...".into(),
            };

            assert!(filter.apply(&pr));

            pr.labels = set(&["batz"]);
            assert!(!filter.apply(&pr))
        }

        #[test]
        fn filter_author_and_label_need_to_match() {
            let filter = LocalFilter {
                authors: set(&["felipe"]),
                labels: set(&["foo"]),
            };

            let pr = Pr {
                author: "felipe".into(),
                labels: set(&["foo", "bar"]),
                repo: "...".into(),
                title: "...".into(),
                url: "...".into(),
            };

            assert!(filter.apply(&pr));

            let pr = Pr {
                author: "felipe".into(),
                labels: set(&["batz"]),
                repo: "...".into(),
                title: "...".into(),
                url: "...".into(),
            };
            assert!(!filter.apply(&pr));

            let pr = Pr {
                author: "anna".into(),
                labels: set(&["foo"]),
                repo: "...".into(),
                title: "...".into(),
                url: "...".into(),
            };
            assert!(!filter.apply(&pr));

            let pr = Pr {
                author: "anna".into(),
                labels: set(&["batz"]),
                repo: "...".into(),
                title: "...".into(),
                url: "...".into(),
            };
            assert!(!filter.apply(&pr));
        }

        fn set(input: &[&str]) -> HashSet<String> {
            input.iter().map(ToString::to_string).collect()
        }
    }
}
