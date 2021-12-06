use std::collections::HashSet;
use std::str::FromStr;

use anyhow::Result;
use octocrab::models::pulls::PullRequest;
use octocrab::models::Repository;
use octocrab::{Octocrab, Page};
use serde::{Deserialize, Deserializer, Serialize};
use tracing::instrument;

/// Configuration for how journal should get outstanding Pull/Merge requests
#[derive(Deserialize)]
pub struct PullRequestConfig {
    pub auth: Auth,
    #[serde(rename = "select")]
    pub selections: Vec<PrSelector>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PrSelector {
    #[serde(flatten)]
    origin: Origin,
    #[serde(flatten)]
    filter: LocalFilter,
}

impl LocalFilter {
    fn apply(&self, pr: &Pr) -> bool {
        let mut applies = true;
        if !self.authors.is_empty() {
            applies = applies && self.authors.contains(&pr.author)
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

#[derive(Clone, Debug, Deserialize)]
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
                let mut current_page = octocrab.orgs(org).list_repos().send().await?;
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

#[derive(Clone, Debug, Deserialize)]
struct LocalFilter {
    #[serde(default)]
    authors: HashSet<String>,
    #[serde(default)]
    labels: HashSet<String>,
}

#[derive(Deserialize)]
pub enum Auth {
    #[serde(rename = "personal_access_token")]
    PersonalAccessToken(String),
}

#[derive(Debug, Serialize)]
pub struct Pr {
    author: String,
    labels: HashSet<String>,
    repo: String,
    title: String,
    url: String,
}

impl From<&octocrab::models::pulls::PullRequest> for Pr {
    fn from(raw: &octocrab::models::pulls::PullRequest) -> Self {
        Pr {
            author: raw.user.login.clone(),
            labels: raw
                .labels
                .clone()
                .unwrap_or_default()
                .iter()
                .map(|l| l.name.clone())
                .collect(),
            repo: raw.base.repo.clone().unwrap().full_name,
            title: raw.title.clone(),
            url: raw.html_url.to_string(),
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
        fn parse_localfilter_with_multiple_values() -> Result<()> {
            let input = indoc! { r#"
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
            assert_eq!(pr_config.selections.len(), 1);
            let selection = &pr_config.selections[0];

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
            input.into_iter().map(ToString::to_string).collect()
        }
    }
}
