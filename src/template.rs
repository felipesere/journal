use anyhow::{anyhow, Result};
use tera::{Context as TeraContext, Tera};
use time::{format_description, OffsetDateTime};

use crate::github::Pr;

pub const DAY_TEMPLATE: &str = include_str!("../template/day.md");

pub struct Template {
    pub title: String,
    pub today: OffsetDateTime,
    pub todos: Vec<String>,
    pub prs: Option<Vec<Pr>>,
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

        tera.render("day.md", &context).map_err(|e| anyhow!(e))
    }
}
