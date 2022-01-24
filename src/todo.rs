use std::ops::Range;

use anyhow::Result;
use handlebars::Handlebars;
use pulldown_cmark::{Event, HeadingLevel::H2, Options, Parser, Tag};
use serde::{Deserialize, Serialize};
use tracing::Level;

use crate::storage::Journal;

const TODO: &str = indoc::indoc! {r#"
## TODOs
{{#each todos as |todo| }}
{{~todo~}}
{{/each}}
"#};

#[derive(Default, Debug, Deserialize, Serialize)]
pub struct TodoConfig {
    template: Option<String>,
}

impl TodoConfig {
    pub async fn render(&self, journal: &Journal) -> Result<String> {
        let todos = match journal.latest_entry() {
            Ok(None) => Vec::new(),
            Ok(Some(last_entry)) => {
                let mut finder = FindTodos::new();
                finder.process(&last_entry.markdown)
            }
            Err(e) => return Err(anyhow::anyhow!(e)),
        };

        #[derive(Serialize)]
        struct C {
            todos: Vec<String>,
        }

        let template = self.template.clone().unwrap_or_else(|| TODO.to_string());

        let mut tt = Handlebars::new();
        tt.register_template_string("todos", template)?;
        tt.register_escape_fn(handlebars::no_escape);
        tt.render("todos", &C { todos })
            .map_err(|e| anyhow::anyhow!(e))
    }
}

#[derive(Debug, Eq, PartialEq)]
enum State {
    Initial,
    GettingTodos,
    Done,
}

pub(crate) struct FindTodos {
    state: State,
}

#[derive(Debug, Eq, PartialEq)]
enum TodoHeader {
    NotFound,
    Found,
    ProcessedTitle,
}

impl FindTodos {
    pub(crate) fn new() -> Self {
        FindTodos {
            state: State::Initial,
        }
    }

    fn gather_open_todos<'a>(
        &mut self,
        parser: &mut impl Iterator<Item = (Event<'a>, Range<usize>)>,
    ) -> Vec<Range<usize>> {
        let mut found_top_level_item = false;
        let mut range_of_todo_item = None;
        let mut depth = 0;
        let mut todos = Vec::new();

        for (event, range) in parser {
            let span = tracing::span!(Level::INFO, "processing_todos", ?event, ?depth);
            let _entered = span.enter();
            match event {
                Event::Start(Tag::Heading(_, _, _)) => {
                    // Found a new section, leaving!
                    self.state = State::Done;
                    break;
                }
                Event::Start(Tag::Item) if depth == 0 => {
                    tracing::info!("Found the beginning of a top-level item");
                    depth += 1;
                    found_top_level_item = true;
                    range_of_todo_item = Some(range);
                }
                Event::Start(Tag::Item) => {
                    depth += 1;
                    tracing::info!("Beginning of an item");
                }
                Event::End(Tag::Item) => {
                    depth -= 1;
                    tracing::info!("End of an item");
                }
                Event::TaskListMarker(done) if found_top_level_item => {
                    tracing::info!("Found a TODO item.");
                    found_top_level_item = false;
                    if done {
                        tracing::info!("Skipping completed TODO");
                    } else {
                        tracing::info!("Storing incomplete TODO item");
                        todos.push(range_of_todo_item.take().unwrap());
                    }
                }
                _ => {
                    tracing::trace!("Ignoring event");
                }
            }
        }

        todos
    }

    pub fn process(&mut self, markdown: &str) -> Vec<String> {
        let mut options = Options::empty();
        options.insert(Options::ENABLE_TASKLISTS);
        let mut parser = Parser::new_ext(markdown, options);

        let found = find_todo_section(&mut parser);

        let todo_text = Vec::new();
        if !found {
            self.state = State::Done;
            return todo_text;
        }

        let mut parser = parser.into_offset_iter();
        self.state = State::GettingTodos;

        let ranges = self.gather_open_todos(&mut parser);

        ranges
            .into_iter()
            .map(|todo| markdown[todo].to_string())
            .collect::<Vec<_>>()
    }
}

fn find_todo_section<'a>(parser: &mut impl Iterator<Item = Event<'a>>) -> bool {
    let mut todo_header = TodoHeader::NotFound;

    for event in parser {
        let span = tracing::span!(
            Level::INFO,
            "looking_for_todo_section",
            ?event,
            ?todo_header,
        );
        let _entered = span.enter();

        match (&event, &todo_header) {
            (Event::Start(Tag::Heading(H2, _, _)), _) => {
                todo_header = TodoHeader::Found;
            }
            (Event::Text(ref text), TodoHeader::Found) => {
                if text.to_string() == "TODOs" {
                    todo_header = TodoHeader::ProcessedTitle;
                    tracing::info!("Found a TODO header");
                }
            }
            (Event::End(Tag::Heading(H2, _, _)), TodoHeader::ProcessedTitle) => return true,
            _ => {
                tracing::trace!("Ignoring event");
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::{FindTodos, State};
    use indoc::indoc;
    use tracing_test::traced_test;

    #[test]
    #[traced_test]
    fn there_were_no_todos() {
        let markdown = indoc! {r#"
                # Something

                "#};

        let mut parser = FindTodos::new();
        let found_todos = parser.process(markdown);

        assert_eq!(parser.state, State::Done);
        assert_eq!(found_todos.len(), 0);
    }

    #[test]
    #[traced_test]
    fn parser_knows_when_found_the_todo_header() {
        let markdown = indoc! {r#"
                # Something

                ## TODOs

                abc
                "#};

        let mut parser = FindTodos::new();
        parser.process(markdown);

        assert_eq!(parser.state, State::GettingTodos,);
    }

    #[test]
    #[traced_test]
    fn parser_knows_when_it_is_looking_at_a_todo_list() {
        let markdown = indoc! {r#"
                # Something

                ## TODOs

                * [ ] abc
                "#};

        let mut parser = FindTodos::new();
        let found_todos = parser.process(markdown);

        assert_eq!(parser.state, State::GettingTodos);
        assert_eq!(found_todos.len(), 1);
    }

    #[test]
    #[traced_test]
    fn parser_knows_when_its_done_with_todos() {
        let markdown = indoc! {r#"
                # Something

                ## TODOs

                ## Not TODOs

                "#};

        let mut parser = FindTodos::new();
        let found_todos = parser.process(markdown);

        assert_eq!(parser.state, State::Done);
        assert_eq!(found_todos.len(), 0);
    }

    #[test]
    #[traced_test]
    fn finds_multiple_todos() {
        let markdown = indoc! {r#"
                # Something

                ## TODOs

                * [ ] first

                * [ ] second

                * [ ] third

                ## Other thing
                "#};

        let mut parser = FindTodos::new();
        let found_todos = parser.process(markdown);

        for todo in &found_todos {
            println!("---------------");
            println!("{}", todo);
            println!("---------------");
        }

        assert_eq!(found_todos.len(), 3);
    }

    #[test]
    #[traced_test]
    fn skips_completed_todos() {
        let markdown = indoc! {r#"
                # Something

                ## TODOs

                * [ ] first

                * [x] second

                * [ ] third

                ## Other thing
                "#};

        let mut parser = FindTodos::new();
        let found_todos = parser.process(markdown);

        for todo in &found_todos {
            println!("---------------");
            println!("{}", todo);
            println!("---------------");
        }

        assert_eq!(found_todos.len(), 2);
    }

    #[test]
    #[traced_test]
    fn ignores_todos_beneath_a_completed_one() {
        let markdown = indoc! {r#"
                # Something

                ## TODOs

                * [ ] first

                * [x] second
                    * [ ] second.dot.one

                * [ ] third

                ## Other thing
                "#};

        let mut parser = FindTodos::new();
        let found_todos = parser.process(markdown);

        for todo in &found_todos {
            println!("---------------");
            println!("{}", todo);
            println!("---------------");
        }

        assert_eq!(found_todos.len(), 2);
    }

    #[test]
    #[traced_test]
    fn ignores_normal_bullet_lists_within_completed_ones() {
        let markdown = indoc! {r#"
                # Something

                ## TODOs

                * [ ] first

                * [x] second
                    * second.dot.one

                * [ ] third

                ## Other thing
                "#};

        let mut parser = FindTodos::new();
        let found_todos = parser.process(markdown);

        for todo in &found_todos {
            println!("---------------");
            println!("{}", todo);
            println!("---------------");
        }

        assert_eq!(found_todos.len(), 2);
    }
}
