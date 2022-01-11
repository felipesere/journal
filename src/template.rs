use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::Value;
use time::{format_description, Date};
use tinytemplate::TinyTemplate;

pub const DAY_TEMPLATE: &str = include_str!("../template/day.md");

pub struct Template {
    pub title: String,
    pub today: Date,
    pub todos: Option<String>,
    pub prs: Option<String>,
    pub reminders: Option<String>,
    pub tasks: Option<String>,
}

// TODO: replace this with a simple map
#[derive(Serialize)]
pub struct C {
    title: String,
    today: String,
    todos: Option<String>,
    prs: Option<String>,
    reminders: Option<String>,
    tasks: Option<String>,
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
            tasks: self.tasks,
        };

        tt.render("day.md", &c).map_err(|e| anyhow!(e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use pretty_assertions::assert_eq;
    use time::macros::date;

    #[test]
    fn title_and_todos_for_today() -> Result<()> {
        let template = Template {
            title: "Some title".to_string(),
            today: date!(2021 - 12 - 24),
            todos: Some(
                indoc::indoc! {r"
                ## TODOs

                * [] a todo
                * [] another one
                "}
                .to_string(),
            ),
            prs: None,
            reminders: None,
            tasks: None,
        };

        let expected = indoc! {r"
        # Some title on 2021-12-24

        ## Notes

        > This is where your notes will go!

        ## TODOs

        * [] a todo
        * [] another one
        "}
        .to_string();

        assert_eq!(expected, template.render()?);
        Ok(())
    }

    #[test]
    fn title_todos_and_prs_for_today() -> Result<()> {
        let template = Template {
            title: "Some title".to_string(),
            today: date!(2021 - 12 - 24),
            todos: Some(indoc::indoc! {r"
                ## TODOs

                * [ ] a todo
                * [ ] another one

                "}.to_string(),
            ),
            prs: Some(indoc::indoc! {r"
                ## Pull Requests

                * [ ] Fix the thingon [felipesere/journal](https://github.com/felipesere/journal) by felipe
                "}.to_string(),
            ),
            reminders: None,
            tasks: None,
        };

        let expected = indoc! {r#"
        # Some title on 2021-12-24

        ## Notes

        > This is where your notes will go!

        ## TODOs

        * [ ] a todo
        * [ ] another one

        ## Pull Requests

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
            todos: Some(
                indoc::indoc! {r"
                ## TODOs

                * [ ] a todo
                * [ ] another one

                "}
                .to_string(),
            ),
            prs: None,
            reminders: Some(
                indoc::indoc! {r"
                ## Your reminders for today:

                * [ ] Buy milk
                * [ ] Send email
            "}
                .to_string(),
            ),
            tasks: None,
        };

        let expected = indoc! {r#"
        # Some title on 2021-12-24

        ## Notes

        > This is where your notes will go!

        ## TODOs

        * [ ] a todo
        * [ ] another one

        ## Your reminders for today:

        * [ ] Buy milk
        * [ ] Send email
        "#}
        .to_string();

        assert_eq!(expected, template.render()?);
        Ok(())
    }
}
