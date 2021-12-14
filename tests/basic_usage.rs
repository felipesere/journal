use assert_cmd::prelude::*;
use assert_fs::{prelude::*, TempDir};
use predicates::str::is_match;
use std::process::Command;

fn journal() -> Command {
    Command::cargo_bin("journal").unwrap()
}

#[test]
fn it_prints_the_version() {
    journal()
        .env_clear()
        .args(&["--version"])
        .assert()
        .stdout("journal 0.1.0\n");
}

#[test]
fn when_the_config_is_entirely_missing_it_reports_the_error() {
    journal()
        .env_clear()
        .env("HOME", "/home/example_user") // otherwise the message will vary by who is executing the test
        .args(&["new", "Through the Looking-Glass", "--stdout"])
        .assert()
        .failure()
        .stderr(indoc::indoc! {r#"
        Error: Failed to load configuration

        Caused by:
            /home/example_user/.journal.yaml does not exist. We need a configuration file to work.
            You can either use a '.journal.yaml' file in your HOME directory or configure it with the JOURNAL__CONFIG environment variable
        "#});
}

#[test]
fn reads_journal_config_from_the_home_directory() {
    let fake_journal_directory = TempDir::new().unwrap();

    let fake_home_dir = TempDir::new().unwrap();
    fake_home_dir
        .child(".journal.yaml")
        .write_str(&format!(
            "dir: {}",
            fake_journal_directory.to_str().unwrap()
        ))
        .unwrap();

    journal()
        .env_clear()
        .env("HOME", fake_home_dir.path())
        .args(&["new", "Through the Looking-Glass", "--stdout"])
        .assert()
        .success()
        .stdout(
            is_match(indoc::indoc! {r#"
        # Through the Looking-Glass on \d\d\d\d-\d\d-\d\d

        ## Notes

        > This is where your notes will go!

        ## TODOs

        "#})
            .unwrap(),
        );
}
