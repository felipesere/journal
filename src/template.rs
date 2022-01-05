use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::Value;
use time::{format_description, Date};
use tinytemplate::TinyTemplate;

use crate::github::Pr;

pub const DAY_TEMPLATE: &str = include_str!("../template/day.md");

pub struct Template {
    pub title: String,
    pub today: Date,
    pub todos: Vec<String>,
    pub prs: Option<Vec<Pr>>,
    pub reminders: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct C {
    title: String,
    today: String,
    todos: Vec<String>,
    prs: Option<Vec<Pr>>,
    reminders: Option<Vec<String>>,
}

pub fn trim(value: &Value, output: &mut String) -> Result<(), tinytemplate::error::Error> {
    if let Value::String(val) = value {
        output.push_str(val.trim());
    }
    Ok(())
}

impl Template {
    pub fn render(self) -> Result<String> {
        let mut tt = TinyTemplate::new();
        tt.add_template("day.md", DAY_TEMPLATE)
            .expect("adding tempalte");
        tt.add_formatter("trim", trim);

        let year_month_day = format_description::parse("[year]-[month]-[day]").unwrap();
        let today = self.today.format(&year_month_day)?;

        let c = C {
            title: self.title,
            todos: self.todos,
            today,
            prs: self.prs,
            reminders: self.reminders,
        };

        tt.render("day.md", &c).map_err(|e| anyhow!(e))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use indoc::indoc;
    use pretty_assertions::assert_eq;
    use time::macros::date;

    #[test]
    fn title_and_todos_for_today() -> Result<()> {
        let template = Template {
            title: "Some title".to_string(),
            today: date!(2021 - 12 - 24),
            todos: vec![
                "* [] a todo\n".to_string(),
                "* [] another one\n".to_string(),
            ],
            prs: None,
            reminders: None,
        };

        let expected = indoc! {r#"
        # Some title on 2021-12-24

        ## Notes

        > This is where your notes will go!

        ## TODOs

        * [] a todo

        * [] another one





        "#}
        .to_string();

        assert_eq!(expected, template.render()?);
        Ok(())
    }

    #[test]
    fn title_todos_and_prs_for_today() -> Result<()> {
        let template = Template {
            title: "Some title".to_string(),
            today: date!(2021 - 12 - 24),
            todos: vec![
                "* [] a todo\n".to_string(),
                "* [] another one\n".to_string(),
            ],
            prs: Some(vec![Pr {
                author: "felipe".into(),
                labels: HashSet::new(),
                repo: "felipesere/journal".to_string(),
                title: "Fix the thing".to_string(),
                url: "https://github.com/felipesere/journal".into(),
            }]),
            reminders: None,
        };

        let expected = indoc! {r#"
        # Some title on 2021-12-24

        ## Notes

        > This is where your notes will go!

        ## TODOs

        * [] a todo

        * [] another one



        ## Pull Requests:

        * [ ] Fix the thingon [felipesere/journal](https://github.com/felipesere/journal) by felipe



        "#}
        .to_string();

        assert_eq!(expected, template.render()?);
        Ok(())
    }

    #[test]
    fn title_todos_and_reminders_for_today() -> Result<()> {
        let template = Template {
            title: "Some title".to_string(),
            today: date!(2021 - 12 - 24),
            todos: vec![
                "* [] a todo\n".to_string(),
                "* [] another one\n".to_string(),
            ],
            prs: None,
            reminders: Some(vec!["Buy milk".to_string(), "Send email".to_string()]),
        };

        let expected = indoc! {r#"
        # Some title on 2021-12-24

        ## Notes

        > This is where your notes will go!

        ## TODOs

        * [] a todo

        * [] another one





        ## Your reminders for today:

        * [ ] Buy milk
        * [ ] Send email


        "#}
        .to_string();

        assert_eq!(expected, template.render()?);
        Ok(())
    }
}
