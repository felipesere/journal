use serde::{Serialize, Deserialize};
use indoc::indoc;
use anyhow::Result;


use crate::Clock;
use crate::config::Section;
use crate::storage::Journal;

#[derive(Serialize, Deserialize, Clone)]
pub struct NotesConfig {
    #[serde(default = "default_note_template")]
    pub template: String,
}

impl Default for NotesConfig {
    fn default() -> Self {
        Self {
            template: default_note_template(),
        }
    }
}

fn default_note_template() -> String {
    indoc! {r#"
  ## Notes

  > This is where your notes will go!

  "#}
    .to_string()
}

#[async_trait::async_trait]
impl Section for NotesConfig {
    async fn render(&self, _: &Journal, _: &dyn Clock) -> Result<String> {
        Ok(self.template.clone())
    }
}

