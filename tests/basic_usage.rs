use assert_cmd::prelude::*;
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
