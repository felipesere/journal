#![allow(dead_code)]
use figment::{
    providers::{Env, Format, Yaml},
    Figment,
};
use pulldown_cmark::{Event, Options, Parser as MdParser, Tag};
use serde::Deserialize;
use std::{ops::Range, path::PathBuf};

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
}

impl JournalParser {
    fn new() -> Self {
        JournalParser {
            found_todos: Vec::new(),
        }
    }

    fn process(&mut self, markdown: &str) -> () {
        let mut options = Options::empty();
        options.insert(Options::ENABLE_TASKLISTS);
        let parser = MdParser::new_ext(markdown, options);

        let mut state = ParserState::Initial;

        let mut md_iterator = parser.into_offset_iter();

        while let Some((event, range)) = md_iterator.next() {
            match event {
                Event::Start(Tag::Heading(2)) => {
                    if let Some((heading_title, _)) = md_iterator.next() {
                        match heading_title {
                            Event::Text(ref t) if t.to_string() == "TODOs" => {
                                state = ParserState::FoundTODOHeader
                            }
                            _ => state = ParserState::Done,
                        }
                    }
                },
                Event::Start(Tag::List(_)) if state == ParserState::FoundTODOHeader => {
                    state = ParserState::GettingTODOs;
                },
                Event::End(Tag::List(None)) if state == ParserState::GettingTODOs => {
                    state = ParserState::Done;
                }
                Event::Start(Tag::Item) if state == ParserState::GettingTODOs => {
                    match md_iterator.next() {
                        Some((Event::TaskListMarker(done), _)) if !done => {
                            self.found_todos.push(range);
                        },
                        _ => {},
                    };
                },
                _ => {},
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
        use crate::JournalParser;
        use indoc::indoc;

        #[test]
        fn finds_incomplete_todos() {
            let markdown = indoc! {r#"
                # Something

                ## TODOs

                * [ ] first
                  * [ ] middle
                  * Random text!

                * [x] second

                * [ ] third

                ## Other thing
                "#};

            let mut parser = JournalParser::new();
            parser.process(markdown);

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
