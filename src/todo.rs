use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag};
use tracing::Level;

#[derive(Debug, PartialEq, Eq)]
enum State {
    Initial,
    GettingTodos,
    Done,
}

pub(crate) struct FindTodos {
    state: State,
}

#[derive(Debug, PartialEq, Eq)]
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

    fn find_todo_section<'a>(&self, parser: &mut impl Iterator<Item = Event<'a>>) -> bool {
        let mut todo_header = TodoHeader::NotFound;

        while let Some(event) = parser.next() {
            let span = tracing::span!(
                Level::INFO,
                "looking_for_todo_section",
                ?event,
                ?todo_header,
            );
            let _entered = span.enter();

            match (&event, &todo_header) {
                (Event::Start(Tag::Heading(2)), _) => {
                    todo_header = TodoHeader::Found;
                }
                (Event::Text(ref text), TodoHeader::Found) => {
                    if text.to_string() == "TODOs" {
                        todo_header = TodoHeader::ProcessedTitle;
                        tracing::info!("Found a TODO header");
                    }
                }
                (Event::End(Tag::Heading(2)), TodoHeader::ProcessedTitle) => return true,
                _ => {
                    tracing::trace!("Ignoring event");
                }
            }
        }

        false
    }

    fn gather_open_todos<'a>(
        &mut self,
        parser: &mut impl Iterator<Item = (Event<'a>, Range<usize>)>,
    ) -> Vec<Range<usize>> {
        let mut found_top_level_item = false;
        let mut range_of_todo_item = None;
        let mut depth = 0;
        let mut todos = Vec::new();

        while let Some((event, range)) = parser.next() {
            let span = tracing::span!(Level::INFO, "processing_todos", ?event, ?depth);
            let _entered = span.enter();
            match event {
                Event::Start(Tag::Heading(_)) => {
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
                    if !done {
                        tracing::info!("Storing incomplete TODO item");
                        todos.push(range_of_todo_item.take().unwrap());
                    } else {
                        tracing::info!("Skipping completed TODO");
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

        let found = self.find_todo_section(&mut parser);

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
