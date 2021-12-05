use std::str::FromStr;

use octocrab::Octocrab;
use octocrab::models::Repository;
use serde::{Deserialize, Deserializer, Serialize};
use tracing::instrument;
use anyhow::Result;

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
        if let Some(ref author) = self.author {
            return &pr.author == author;
        }
        if let Some(ref label) = self.label {
            return pr.labels.iter().any(|l| l == label);
        }

        true
    }
}

impl PrSelector {
    #[instrument(skip(octocrab))]
    pub async fn get_prs(&self, octocrab: &Octocrab) -> Result<Vec<Pr>> {
        let repos = self.origin.repos(octocrab).await?;

        let mut prs = Vec::new();
        for Repo {owner, name } in repos {
            tracing::info!("Getting PRs for org={} repo={}", owner, name);
            let mut current_page = octocrab
                .pulls(&owner, &name)
                .list()
                .state(octocrab::params::State::Open)
                .send()
                .await?;

            prs.extend(
                current_page
                    .take_items()
                    .iter()
                    .map(Pr::from)
                    .filter(|pr| self.filter.apply(pr))
                    .collect::<Vec<_>>(),
            );

            while let Ok(Some(mut next_page)) = octocrab.get_page(&current_page.next).await {
                tracing::info!("Getting next page of PRs for org={} repo={}", owner, name);
                prs.extend(
                    next_page
                        .take_items()
                        .iter()
                        .map(Pr::from)
                        .collect::<Vec<_>>(),
                );

                current_page = next_page;
            }
        }

        Ok(prs)
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
        where D: Deserializer<'de>
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

                let mut repos: Vec<Repo> = current_page
                    .take_items()
                    .iter()
                    .map(|repo| Repo{ owner: org.clone(), name:  repo.name.clone()})
                    .collect();

                while let Ok(Some(mut next_page)) = octocrab.get_page(&current_page.next).await {
                    tracing::info!("Getting next page of repos for org={}", org);
                    repos.extend(
                        next_page
                            .take_items()
                            .iter()
                            .map(|repo: &Repository| Repo{ owner: org.clone(), name:  repo.name.clone()})
                            .collect::<Vec<Repo>>(),
                    );

                    current_page = next_page;
                }

                Ok(repos)
            }
            Origin::Repository(repo) => Ok(vec![repo.clone()])
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
struct LocalFilter {
    author: Option<String>,
    label: Option<String>,
}

#[derive(Deserialize)]
pub enum Auth {
    #[serde(rename = "personal_access_token")]
    PersonalAccessToken(String),
}

#[derive(Debug, Serialize)]
pub struct Pr {
    author: String,
    labels: Vec<String>,
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
