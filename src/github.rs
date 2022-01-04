use std::collections::HashSet;
use std::str::FromStr;

use anyhow::Result;
use futures::future::join_all;
use octocrab::{
    models::{pulls::PullRequest, Repository},
    Octocrab, OctocrabBuilder, Page,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use tokio::task::JoinHandle;
use tracing::{instrument, Instrument};

/// Configuration for how journal should get outstanding Pull/Merge requests
#[derive(Debug, Deserialize, Serialize)]
pub struct PullRequestConfig {
    pub(crate) auth: Auth,
    select: Vec<PrSelector>,
    enabled: bool,
}

impl PullRequestConfig {
    pub async fn get_matching_prs(&self) -> Result<Vec<Pr>> {
        if !self.enabled {
            return Ok(Vec::new());
        }

        let Auth::PersonalAccessToken(ref token) = self.auth;

        let octocrab = OctocrabBuilder::new()
            .personal_token(token.clone())
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
                    let octocrab = OctocrabBuilder::new().personal_token(token).build()?;
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
    #[serde(flatten)]
    origin: Origin,
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
        let repos = self.origin.repos(octocrab).await?;

        let mut prs = Vec::new();
        for Repo { owner, name } in repos {
            tracing::info!("Getting PRs for org={} repo={}", owner, name);
            let mut current_page = octocrab
                .pulls(&owner, &name)
                .list()
                .state(octocrab::params::State::Open)
                .per_page(50)
                .send()
                .await?;

            prs.extend(self.extract_prs(&mut current_page));

            while let Ok(Some(mut next_page)) = octocrab.get_page(&current_page.next).await {
                tracing::info!("Getting next page of PRs for org={} repo={}", owner, name);
                prs.extend(self.extract_prs(&mut next_page));

                current_page = next_page;
            }
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

#[derive(Clone, Debug, Deserialize, Serialize)]
enum Origin {
    #[serde(rename = "org")]
    Organisation(String),
    #[serde(rename = "repo")]
    Repository(Repo),
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

impl Origin {
    #[instrument(skip(octocrab))]
    async fn repos(&self, octocrab: &Octocrab) -> Result<Vec<Repo>> {
        match self {
            Origin::Organisation(org) => {
                tracing::info!("Getting repos for org={}", org);
                let mut current_page = octocrab.orgs(org).list_repos().per_page(50).send().await?;
                let mut repos: Vec<Repo> = extract_repo(org, &mut current_page);

                while let Ok(Some(mut next_page)) = octocrab.get_page(&current_page.next).await {
                    tracing::info!("Getting next page of repos for org={}", org);
                    repos.extend(extract_repo(org, &mut next_page));

                    current_page = next_page;
                }

                Ok(repos)
            }
            Origin::Repository(repo) => Ok(vec![repo.clone()]),
        }
    }
}

fn extract_repo(org: &str, page: &mut Page<Repository>) -> Vec<Repo> {
    page.take_items()
        .iter()
        .map(|repo: &Repository| Repo {
            owner: org.to_string(),
            name: repo.name.clone(),
        })
        .collect()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct LocalFilter {
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub(crate) authors: HashSet<String>,

    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub(crate) labels: HashSet<String>,
}

#[derive(Deserialize, PartialEq, Eq, Serialize)]
pub(crate) enum Auth {
    #[serde(rename = "personal_access_token")]
    PersonalAccessToken(String),
}

impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Auth::PersonalAccessToken(token) => {
                write!(f, "{}", &token[0..3])
            }
        }
    }
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
            repo: raw.base.repo.as_ref().unwrap().full_name.as_ref().unwrap().to_string(),
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
