use assert_cmd::prelude::*;
use std::process::Command;

fn journal() -> Command {
    Command::cargo_bin("journal").unwrap()
}

#[test]
fn it_prints_the_version() {
        journal()
        .args(&["--version"])
        .assert()
        .stdout("journal 0.1.0\n");
}
