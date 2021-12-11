use anyhow::{anyhow, Result};
use tera::{Context as TeraContext, Tera};
use time::{format_description, Date};

use crate::github::Pr;

pub const DAY_TEMPLATE: &str = include_str!("../template/day.md");

pub struct Template {
    pub title: String,
    pub today: Date,
    pub todos: Vec<String>,
    pub prs: Option<Vec<Pr>>,
    pub reminders: Option<Vec<String>>,
}

impl Template {
    pub fn render(self) -> Result<String> {
        let mut tera = Tera::default();
        tera.add_raw_template("day.md", DAY_TEMPLATE).unwrap();
        let year_month_day = format_description::parse("[year]-[month]-[day]").unwrap();
        let today = self.today.format(&year_month_day)?;

        let mut context = TeraContext::new();
        context.insert("title", &self.title);
        context.insert("date", &today);
        context.insert("open_todos", &self.todos);

        if let Some(ref prs) = self.prs {
            context.insert("prs", prs);
        }
        if let Some(ref reminders) = self.reminders {
            context.insert("reminders", reminders);
        }

        tera.render("day.md", &context).map_err(|e| anyhow!(e))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use indoc::indoc;
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
