use anyhow::Result;
use std::collections::HashMap;

use handlebars::Handlebars;
use jsonpath::Selector;
use secrecy::{ExposeSecret, Secret};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;

use crate::config::Section;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct JiraAuth {
    user: String,
    #[serde(serialize_with = "only_asterisk")]
    personal_access_token: Secret<String>,
}
fn only_asterisk<S>(_: &Secret<String>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str("***")
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(transparent)]
struct Jql(HashMap<String, String>);

impl Jql {
    fn to_query(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        for (k, v) in &self.0 {
            parts.push(format!(r#"{}="{}""#, k, v));
        }

        parts.join(" and ")
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct JiraConfig {
    base_url: String,
    auth: JiraAuth,
    query: Jql,
    template: Option<String>,
}

#[async_trait::async_trait]
impl Section for JiraConfig {
    async fn render(&self, _: &crate::storage::Journal, _: &dyn crate::Clock) -> Result<String> {
        let tasks = self.get_matching_tasks().await?;

        #[derive(Serialize)]
        struct C {
            tasks: Vec<Task>,
        }

        let template = self.template.clone().unwrap_or_else(|| TASKS.to_string());

        let mut tt = Handlebars::new();
        tt.register_template_string("tasks", template)?;
        tt.register_escape_fn(handlebars::no_escape);
        tt.render("tasks", &C { tasks }).map_err(|e| e.into())
    }
}

#[derive(Debug, Serialize)]
pub struct Task {
    summary: String,
    href: String,
}

struct Selection {
    summary: Selector,
    href: Selector,
}

impl Selection {
    fn extract_from(&self, issue: &Value) -> Option<Task> {
        let summary: String = self.summary.find(issue).next()?.as_str()?.to_string();
        let href: String = self.href.find(issue).next()?.as_str()?.to_string();

        Some(Task { summary, href })
    }
}

const TASKS: &str = r#"
## Open tasks

{{#each tasks as | task | }}
* [ ] {{task.summary}} [here]({{task.task.href}})
{{/each }}
"#;

impl JiraConfig {
    pub async fn get_matching_tasks(&self) -> Result<Vec<Task>> {
        let params = [
            ("jql", self.query.to_query()),
            ("maxResults", "50".to_string()),
        ];
        let client = reqwest::Client::new();
        let res = client
            .get(&self.base_url)
            .basic_auth(
                self.auth.user.to_string(),
                Some(self.auth.personal_access_token.expose_secret()),
            )
            .query(&params)
            .send()
            .await?
            .error_for_status()?;

        let body: Value = res.json::<Value>().await?;

        let issues = Selector::new("$.issues")
            .unwrap()
            .find(&body)
            .next()
            .unwrap();

        let selection = Selection {
            summary: Selector::new("$.fields.summary").unwrap(),
            href: Selector::new("$.self").unwrap(),
        };

        let mut tasks = Vec::new();

        if let Some(array) = issues.as_array() {
            for issue in array {
                if let Some(task) = selection.extract_from(issue) {
                    tasks.push(task);
                }
            }
        };

        Ok(tasks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use maplit::hashmap;

    #[test]
    fn it_works() {
        let raw = indoc! {r#"
        auth:
          user: foo
          personal_access_token: bar
        base_url: "https://x.y/abc"
        query:
          project: EOPS
          status: "In Progress"
          assignee: 61ba1
        "#};

        let config: JiraConfig = serde_yaml::from_str(raw).unwrap();

        assert_eq!(config.base_url, "https://x.y/abc");

        let JiraAuth {
            user,
            personal_access_token,
        } = config.auth;

        assert_eq!(user, "foo".to_string(),);
        assert_eq!(*personal_access_token.expose_secret(), "bar".to_string(),);

        assert_eq!(
            config.query,
            Jql(hashmap! {
                "project".to_string() => "EOPS".to_string(),
                "status".to_string() => "In Progress".to_string(),
                "assignee".to_string() => "61ba1".to_string(),
            })
        );
    }
}
