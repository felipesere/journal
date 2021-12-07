use anyhow::{bail, Result};
use std::path::PathBuf;

pub struct Entry {
    pub markdown: String,
}

pub struct Journal {
    location: PathBuf,
}

impl Journal {
    pub fn new_at<P: Into<PathBuf>>(location: P) -> Journal {
        Journal {
            location: location.into(),
        }
    }

    pub fn latest_entry(&self) -> Result<Entry> {
        // Would still need a filter that matches naming convention
        let mut entries = std::fs::read_dir(&self.location)?
            .map(|res| res.map(|e| e.path()).unwrap())
            .collect::<Vec<_>>();

        // The order in which `read_dir` returns entries is not guaranteed. If reproducible
        // ordering is required the entries should be explicitly sorted.
        entries.sort();

        if let Some(path) = entries.pop() {
            let markdown = std::fs::read_to_string(&path)?;
            tracing::info!("Lastest entry found at {:?}", path);

            return Ok(Entry { markdown });
        }

        bail!("No journal entries found in {:?}", self.location);
    }

    pub fn add_entry(&self, name: &str, data: &str) -> Result<()> {
        let path = self.location.join(name);
        std::fs::write(path, data)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::{prelude::*, TempDir};

    #[test]
    fn empty_journal() {
        let location = TempDir::new().unwrap();

        let journal = Journal::new_at(location.path());

        let entry = journal.latest_entry();

        assert!(entry.is_err());
    }

    #[test]
    fn single_journal_entry() {
        let dir = TempDir::new().unwrap();
        dir.child("2021-08-23-first_entry.md")
            .write_str("first content")
            .unwrap();

        let journal = Journal::new_at(dir.path());

        let entry = journal.latest_entry();

        assert!(entry.is_ok());
        let entry = entry.unwrap();
        assert_eq!(entry.markdown, "first content");
    }

    #[test]
    fn returns_the_latest_entry() {
        let dir = TempDir::new().unwrap();
        dir.child("2021-07-03-older_entry.md")
            .write_str("older content")
            .unwrap();
        dir.child("2021-08-23-first_entry.md")
            .write_str("first content")
            .unwrap();

        let journal = Journal::new_at(dir.path());

        let entry = journal.latest_entry();

        assert!(entry.is_ok());
        let entry = entry.unwrap();
        assert_eq!(entry.markdown, "first content");
    }
}