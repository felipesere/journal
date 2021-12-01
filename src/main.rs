#![allow(dead_code)]
use figment::{
    providers::{Env, Format, Yaml},
    Figment,
};
use pulldown_cmark::{Event, Options, Parser as MdParser, Tag};
use serde::Deserialize;
use std::{ops::Range, path::PathBuf};
use tracing::Level;

use clap::{AppSettings, Parser};

/// Configuration we can get either from a file or from ENV variables
#[derive(Deserialize)]
struct Config {
    dir: String,
}

impl Config {
    fn load() -> Result<Self, figment::Error> {
        let config_path = std::env::var("JOURNAL_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = dirs::home_dir().expect("Unable to get the the users 'home' directory");
                home.join(".journal.yaml")
            });

        Figment::new()
            .merge(Yaml::file(config_path))
            .merge(Env::prefixed("JOURNAL_"))
            .extract()
    }
}

/// Commands and arguments passed via the command line
#[derive(Debug, Parser)]
#[clap(
    name = "fern",
    version = "0.0.3",
    author = "Felipe Sere <journal@felipesere.com>",
    setting = AppSettings::DeriveDisplayOrder,
)]
struct Cli {
    #[clap(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Parser)]
enum Cmd {
    New { title: String },
}

#[derive(Debug, PartialEq, Eq)]
enum ParserState {
    Initial,
    FoundTODOHeader,
    GettingTODOs,
    Done,
}

struct JournalParser {
    found_todos: Vec<Range<usize>>,
    state: ParserState,
}

#[derive(Eq, PartialEq)]
enum TodoHeader {
    NotFound,
    Found,
    ProcessedTitle,
}

impl JournalParser {
    fn new() -> Self {
        JournalParser {
            found_todos: Vec::new(),
            state: ParserState::Initial,
        }
    }

    fn process(&mut self, markdown: &str) {
        let mut options = Options::empty();
        options.insert(Options::ENABLE_TASKLISTS);
        let mut parser = MdParser::new_ext(markdown, options).into_offset_iter();

        let mut found_top_level_item = false;
        let mut range_of_todo_item = None;

        let mut depth = 0;
        let mut todo_header = TodoHeader::NotFound;

        while let Some((event, range)) = parser.next() {
            let span =
                tracing::span!(Level::INFO, "processing_events", ?event, ?self.state, ?depth);
            let _entered = span.enter();

            // Pulled this out of the match statement below to make it
            // easier to express: Found a header, and now we've processed it
            match (&event, &todo_header) {
                (Event::Start(Tag::Heading(2)), _) => {
                    todo_header = TodoHeader::Found;
                }
                (Event::Text(ref text), TodoHeader::Found) => {
                    todo_header = TodoHeader::ProcessedTitle;
                    if text.to_string() == "TODOs" {
                        tracing::info!("Found a TODO header");
                        self.state = ParserState::FoundTODOHeader;
                    } else {
                        tracing::info!("New section, done with TODOs");
                        self.state = ParserState::Done;
                    }
                }
                (_, TodoHeader::ProcessedTitle) => {
                    todo_header = TodoHeader::NotFound;
                }
                (Event::End(Tag::Heading(2)), _) => {
                    // move on to next phase?
                }
                _ => {}
            }

            match event {
                Event::Start(Tag::List(_)) if self.state == ParserState::FoundTODOHeader => {
                    tracing::info!("Processing list within TODO header");
                    self.state = ParserState::GettingTODOs;
                }
                Event::End(Tag::List(_)) => {
                    tracing::info!("Found the end of a list");
                }
                Event::Start(Tag::Item)
                    if self.state == ParserState::GettingTODOs && depth == 0 =>
                {
                    tracing::info!("Found the beginning of an item");
                    depth += 1;
                    found_top_level_item = true;
                    range_of_todo_item = Some(range);
                }
                Event::Start(Tag::Item) if self.state == ParserState::GettingTODOs => {
                    depth += 1;
                }
                Event::TaskListMarker(done) if found_top_level_item => {
                    tracing::info!("Found a TODO item.");
                    found_top_level_item = false;
                    if !done {
                        tracing::info!("Storing incomplete TODO item");
                        self.found_todos.push(range_of_todo_item.take().unwrap());
                    } else {
                        tracing::info!("Skipping completed TODO");
                    }
                }
                Event::End(Tag::Item) if self.state == ParserState::GettingTODOs => {
                    depth -= 1;
                    tracing::info!("End of an item");
                }
                _ => {
                    tracing::info!("Ignoring event");
                }
            }
        }
    }
}

fn main() {
    std::env::var("JOURNAL_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = dirs::home_dir().expect("Unable to get the the users 'home' directory");
            home.join(".journal.yaml")
        });

    let cli = Cli::parse();

    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_max_level(Level::TRACE)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    match cli.cmd {
        Cmd::New { title: _title } => {
            let markdown = include_str!("../example/full.md");

            let mut parser = JournalParser::new();
            parser.process(markdown);

            for todo in parser.found_todos {
                println!("---------------");
                println!("{}", &markdown[todo]);
                println!("---------------");
            }
        }
    }
}

#[cfg(test)]
mod test {
    mod parsing {
        use crate::{JournalParser, ParserState};
        use indoc::indoc;
        use tracing_test::traced_test;

        #[test]
        #[traced_test]
        fn parser_knows_when_found_the_todo_header() {
            let markdown = indoc! {r#"
                # Something

                ## TODOs

                abc
                "#};

            let mut parser = JournalParser::new();
            parser.process(markdown);

            assert_eq!(parser.state, ParserState::FoundTODOHeader,);
        }

        #[test]
        #[traced_test]
        fn parser_knows_when_it_is_looking_at_a_todo_list() {
            let markdown = indoc! {r#"
                # Something

                ## TODOs

                * [ ] abc
                "#};

            let mut parser = JournalParser::new();
            parser.process(markdown);

            assert_eq!(parser.state, ParserState::GettingTODOs);
            assert_eq!(parser.found_todos.len(), 1);
        }

        #[test]
        #[traced_test]
        fn parser_knows_when_its_done_with_todos() {
            let markdown = indoc! {r#"
                # Something

                ## TODOs

                ## Not TODOs

                "#};

            let mut parser = JournalParser::new();
            parser.process(markdown);

            assert_eq!(parser.state, ParserState::Done);
            assert_eq!(parser.found_todos.len(), 0);
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

            let mut parser = JournalParser::new();
            parser.process(markdown);

            for todo in &parser.found_todos {
                println!("---------------");
                println!("{}", &markdown[todo.start..todo.end]);
                println!("---------------");
            }

            assert_eq!(parser.found_todos.len(), 3);
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

            let mut parser = JournalParser::new();
            parser.process(markdown);

            for todo in &parser.found_todos {
                println!("---------------");
                println!("{}", &markdown[todo.start..todo.end]);
                println!("---------------");
            }

            assert_eq!(parser.found_todos.len(), 2);
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

            let mut parser = JournalParser::new();
            parser.process(markdown);

            for todo in &parser.found_todos {
                println!("---------------");
                println!("{}", &markdown[todo.start..todo.end]);
                println!("---------------");
            }

            assert_eq!(parser.found_todos.len(), 2);
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

            let mut parser = JournalParser::new();
            parser.process(markdown);

            for todo in &parser.found_todos {
                println!("---------------");
                println!("{}", &markdown[todo.start..todo.end]);
                println!("---------------");
            }

            assert_eq!(parser.found_todos.len(), 2);
        }
    }

    mod config {
        use crate::Config;

        #[test]
        fn config_read_from_yml() {
            figment::Jail::expect_with(|jail| {
                let config_path = jail.directory().join(".journal.yml");
                jail.set_env("JOURNAL_CONFIG", config_path.to_string_lossy());

                jail.create_file(".journal.yml", r#"dir: file/from/yaml"#)?;

                let config = Config::load()?;

                assert_eq!(config.dir, "file/from/yaml");

                Ok(())
            });
        }

        #[test]
        fn config_read_from_env() {
            figment::Jail::expect_with(|jail| {
                let config_path = jail.directory().join(".journal.yml");
                jail.set_env("JOURNAL_CONFIG", config_path.to_string_lossy());

                jail.create_file(".journal.yml", r#"dir: file/from/yaml"#)?;
                jail.set_env("JOURNAL_DIR", "env/set/the/dir");

                let config = Config::load()?;

                assert_eq!(config.dir, "env/set/the/dir");

                Ok(())
            });
        }
    }
}
