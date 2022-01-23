use std::collections::HashMap;

use anyhow::Result;
use time::{format_description, Date};

use crate::config::{default_order, Sections};

pub struct Template {
    pub title: String,
    pub today: Date,
    pub sections: HashMap<Sections, String>,
}

impl Template {
    pub fn render(self, order: Vec<Sections>) -> Result<String> {
        let year_month_day = format_description::parse("[year]-[month]-[day]").unwrap();

        let Template {
            title,
            today,
            sections,
        } = self;

        let today = today.format(&year_month_day)?;

        let order = expand_with_defaults(order);

        let mut to_be_printed = vec![format!("# {title} on {today}")];

        for section in &order {
            if let Some(content) = sections.get(section) {
                to_be_printed.push(content.to_string());
            };
        }

        Ok(to_be_printed.join("\n\n"))
    }
}

fn expand_with_defaults(mut order: Vec<Sections>) -> Vec<Sections> {
    let mut df = default_order();

    for section in &order {
        df = df.into_iter().filter(|s| s != section).collect();
    }

    order.extend(df);
    order
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
            sections: maplit::hashmap! {
                Sections::Todos => indoc! {r"
                ## TODOs

                * [] a todo
                * [] another one
                "}.to_string(),
                Sections::Notes => indoc! {r"
                ## Notes

                > This is where your notes will go!
                "}.to_string(),
            },
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

        assert_eq!(
            expected,
            template.render(vec![Sections::Notes, Sections::Todos, Sections::Prs])?
        );
        Ok(())
    }

    #[test]
    fn title_todos_and_prs_for_today() -> Result<()> {
        let template = Template {
            title: "Some title".to_string(),
            today: date!(2021 - 12 - 24),
            sections: maplit::hashmap! {
                Sections::Notes => indoc! {r"
                ## Notes

                > This is where your notes will go!
                "}.to_string(),
                Sections::Todos => indoc::indoc! {r"
                ## TODOs

                * [ ] a todo
                * [ ] another one
                "}.to_string(),
                Sections::Prs => indoc::indoc! {r"
                ## Pull Requests

                * [ ] Fix the thingon [felipesere/journal](https://github.com/felipesere/journal) by felipe
                "}.to_string(),
            },
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

        assert_eq!(
            expected,
            template.render(vec![Sections::Notes, Sections::Todos, Sections::Prs])?
        );
        Ok(())
    }

    #[test]
    fn title_todos_and_reminders_for_today() -> Result<()> {
        let template = Template {
            title: "Some title".to_string(),
            today: date!(2021 - 12 - 24),
            sections: maplit::hashmap! {
                Sections::Notes => indoc! {r"
                ## Notes

                > This is where your notes will go!
                "}.to_string(),
                Sections::Todos => indoc::indoc! {r"
                ## TODOs

                * [ ] a todo
                * [ ] another one
                "}.to_string(),
                Sections::Reminders => indoc::indoc! {r"
                ## Your reminders for today:

                * [ ] Buy milk
                * [ ] Send email
                "}.to_string(),
            },
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

        assert_eq!(
            expected,
            template.render(vec![Sections::Notes, Sections::Todos, Sections::Reminders])?
        );
        Ok(())
    }
}
