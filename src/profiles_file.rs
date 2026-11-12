//! Edits profiles.json in memory and saves it. The file is kept as JSON rather than parsed into
//! `Config`, so a save keeps the order of profiles and of the keys inside each one. Nothing
//! touches the disk until `save`, and a save that wouldn't load is refused.

use crate::config::{self, Config};
use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

pub struct ProfilesFile {
    home: PathBuf,
    user_home: PathBuf,
    saved: Value,
    current: Value,
    /// Current name -> name on disk, for every profile that was in the saved file.
    origin: HashMap<String, String>,
}

/// One profile as written in profiles.json, before `~` is expanded.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub name: String,
    pub config_dir: String,
    pub paths: Vec<String>,
    pub op_account: Option<String>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Saved {
    /// Profiles taken out of profiles.json: name, config dir and source folder, all left on disk.
    pub removed: Vec<(String, PathBuf, PathBuf)>,
    /// A profile was added or renamed, or a config dir changed, so its files need syncing.
    pub needs_sync: bool,
}

impl ProfilesFile {
    /// A missing profiles.json opens as an empty one; the first save creates it.
    pub fn open_in(home: PathBuf, user_home: PathBuf) -> Result<ProfilesFile> {
        let file = home.join("profiles.json");
        let saved = match fs::read_to_string(&file) {
            Ok(text) => {
                let value: Value = serde_json::from_str(&text)
                    .with_context(|| format!("invalid {}", file.display()))?;
                if !value.get("profiles").is_some_and(Value::is_object) {
                    bail!("invalid {}: no \"profiles\" object", file.display());
                }
                value
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({ "profiles": {} }),
            Err(e) => {
                return Err(e).with_context(|| format!("could not read {}", file.display()));
            }
        };
        let mut file = ProfilesFile {
            home,
            user_home,
            current: saved.clone(),
            saved,
            origin: HashMap::new(),
        };
        file.origin = file.names().into_iter().map(|n| (n.clone(), n)).collect();
        Ok(file)
    }

    pub fn path(&self) -> PathBuf {
        self.home.join("profiles.json")
    }

    pub fn entries(&self) -> Vec<Entry> {
        entries_in(&self.current)
    }

    pub fn default(&self) -> Option<&str> {
        self.current["default"].as_str()
    }

    pub fn is_changed(&self) -> bool {
        self.current != self.saved
    }

    /// The first error loading would report for the edited file, if any.
    pub fn problem(&self) -> Option<String> {
        Config::check(&self.current, &self.home, &self.user_home)
            .err()
            .map(|e| format!("{e:#}"))
    }

    pub fn add_profile(&mut self, name: &str, config_dir: &str) -> Result<()> {
        check_name(name)?;
        if self.profiles().contains_key(name) {
            bail!("a profile named \"{name}\" already exists");
        }
        let first = self.profiles().is_empty();
        self.profiles_mut()
            .insert(name.to_string(), json!({ "config_dir": config_dir }));
        if first && self.default().is_none() {
            self.set_default(Some(name))?;
        }
        Ok(())
    }

    /// Takes the profile out of profiles.json only; `save` reports what it leaves on disk.
    pub fn remove_profile(&mut self, name: &str) -> Result<()> {
        self.profiles_mut()
            .shift_remove(name)
            .with_context(|| format!("no profile named \"{name}\""))?;
        self.origin.remove(name);
        if self.default() == Some(name) {
            self.set_default(None)?;
        }
        Ok(())
    }

    pub fn rename_profile(&mut self, old: &str, new: &str) -> Result<()> {
        if old == new {
            return Ok(());
        }
        check_name(new)?;
        if self.profiles().contains_key(new) {
            bail!("a profile named \"{new}\" already exists");
        }
        let on_disk = self.origin.get(old).map(String::as_str).unwrap_or(old);
        let folder = self.source_folder(new);
        if new != on_disk && fs::symlink_metadata(&folder).is_ok() {
            bail!(
                "profiles/{new} already exists; move it away before renaming ({})",
                folder.display()
            );
        }
        let profiles = self.profiles_mut();
        let index = profiles
            .keys()
            .position(|k| k == old)
            .with_context(|| format!("no profile named \"{old}\""))?;
        let value = profiles.shift_remove(old).expect("found above");
        profiles.shift_insert(index, new.to_string(), value);
        if let Some(on_disk) = self.origin.remove(old) {
            self.origin.insert(new.to_string(), on_disk);
        }
        if self.default() == Some(old) {
            self.set_default(Some(new))?;
        }
        Ok(())
    }

    pub fn set_config_dir(&mut self, name: &str, config_dir: &str) -> Result<()> {
        self.profile_mut(name)?
            .insert("config_dir".into(), config_dir.into());
        Ok(())
    }

    pub fn add_path(&mut self, name: &str, path: &str) -> Result<()> {
        let profile = self.profile_mut(name)?;
        let paths = profile
            .entry("paths")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .context("\"paths\" is not a list")?;
        if paths.iter().any(|p| p == path) {
            bail!("\"{name}\" already lists {path}");
        }
        paths.push(path.into());
        Ok(())
    }

    pub fn remove_path(&mut self, name: &str, path: &str) -> Result<()> {
        let profile = self.profile_mut(name)?;
        let paths = profile
            .get_mut("paths")
            .and_then(Value::as_array_mut)
            .with_context(|| format!("\"{name}\" has no paths"))?;
        let before = paths.len();
        paths.retain(|p| p != path);
        if paths.len() == before {
            bail!("\"{name}\" doesn't list {path}");
        }
        if paths.is_empty() {
            profile.shift_remove("paths");
        }
        Ok(())
    }

    pub fn set_op_account(&mut self, name: &str, account: Option<&str>) -> Result<()> {
        let profile = self.profile_mut(name)?;
        match account {
            Some(account) => {
                profile.insert("op_account".into(), account.into());
            }
            None => {
                profile.shift_remove("op_account");
            }
        }
        Ok(())
    }

    pub fn set_default(&mut self, name: Option<&str>) -> Result<()> {
        let root = self.current.as_object_mut().expect("checked on open");
        match name {
            Some(name) => {
                if !root["profiles"]
                    .as_object()
                    .is_some_and(|p| p.contains_key(name))
                {
                    bail!("no profile named \"{name}\"");
                }
                match root.get_mut("default") {
                    Some(default) => *default = name.into(),
                    None => {
                        root.shift_insert(0, "default".into(), name.into());
                    }
                }
            }
            None => {
                root.shift_remove("default");
            }
        }
        Ok(())
    }

    /// Checks the edited file, renames source folders, then replaces profiles.json in one step.
    pub fn save(&mut self) -> Result<Saved> {
        if let Some(problem) = self.problem() {
            bail!("not saved: {problem}");
        }
        let entries = self.entries();
        let saved_entries: HashMap<String, Entry> = entries_in(&self.saved)
            .into_iter()
            .map(|e| (e.name.clone(), e))
            .collect();

        let mut report = Saved::default();
        let kept: Vec<&String> = self.origin.values().collect();
        for (name, entry) in &saved_entries {
            if !kept.contains(&name) && !entries.iter().any(|e| &e.name == name) {
                // The old entry may be the invalid one that was removed to fix the file.
                let config_dir = config::expand(&entry.config_dir, &self.user_home)
                    .unwrap_or_else(|_| PathBuf::from(&entry.config_dir));
                report
                    .removed
                    .push((name.clone(), config_dir, self.source_folder(name)));
            }
        }
        report.removed.sort();
        report.needs_sync = entries.iter().any(|e| match self.origin.get(&e.name) {
            None => true,
            Some(on_disk) => {
                on_disk != &e.name || saved_entries[on_disk].config_dir != e.config_dir
            }
        });

        let moves: Vec<(PathBuf, PathBuf)> = self
            .origin
            .iter()
            .filter(|(now, before)| now != before)
            .map(|(now, before)| (self.source_folder(before), self.source_folder(now)))
            .filter(|(from, _)| fs::symlink_metadata(from).is_ok())
            .collect();
        for (_, to) in &moves {
            if fs::symlink_metadata(to).is_ok() {
                let name = to.file_name().unwrap_or_default().to_string_lossy();
                bail!(
                    "not saved: profiles/{name} already exists ({})",
                    to.display()
                );
            }
        }
        let mut done: Vec<&(PathBuf, PathBuf)> = Vec::new();
        for pair in &moves {
            if let Err(e) = fs::rename(&pair.0, &pair.1) {
                undo(&done);
                return Err(e)
                    .with_context(|| format!("not saved: could not rename {}", pair.0.display()));
            }
            done.push(pair);
        }
        if let Err(e) = self.write() {
            undo(&done);
            return Err(e);
        }

        self.saved = self.current.clone();
        self.origin = self.names().into_iter().map(|n| (n.clone(), n)).collect();
        Ok(report)
    }

    /// Writes beside the file and renames over it, so profiles.json is never half written.
    fn write(&self) -> Result<()> {
        fs::create_dir_all(&self.home)
            .with_context(|| format!("not saved: could not create {}", self.home.display()))?;
        let file = self.path();
        let temp = self.home.join(".profiles.json.tmp");
        let text = serde_json::to_string_pretty(&self.current)? + "\n";
        fs::write(&temp, text)
            .and_then(|()| fs::rename(&temp, &file))
            .inspect_err(|_| {
                let _ = fs::remove_file(&temp);
            })
            .with_context(|| format!("not saved: could not write {}", file.display()))
    }

    fn source_folder(&self, name: &str) -> PathBuf {
        self.home.join("profiles").join(name)
    }

    fn names(&self) -> Vec<String> {
        self.profiles().keys().cloned().collect()
    }

    fn profiles(&self) -> &Map<String, Value> {
        self.current["profiles"]
            .as_object()
            .expect("checked on open")
    }

    fn profiles_mut(&mut self) -> &mut Map<String, Value> {
        self.current["profiles"]
            .as_object_mut()
            .expect("checked on open")
    }

    fn profile_mut(&mut self, name: &str) -> Result<&mut Map<String, Value>> {
        self.profiles_mut()
            .get_mut(name)
            .and_then(Value::as_object_mut)
            .with_context(|| format!("no profile named \"{name}\""))
    }
}

fn entries_in(file: &Value) -> Vec<Entry> {
    let Some(profiles) = file["profiles"].as_object() else {
        return Vec::new();
    };
    profiles
        .iter()
        .map(|(name, p)| Entry {
            name: name.clone(),
            config_dir: p["config_dir"].as_str().unwrap_or_default().to_string(),
            paths: p["paths"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            op_account: p["op_account"].as_str().map(String::from),
        })
        .collect()
}

fn undo(done: &[&(PathBuf, PathBuf)]) {
    for (from, to) in done.iter().rev() {
        let _ = fs::rename(to, from);
    }
}

/// A profile's name is also its folder under `profiles/`, so it must be a plain folder name.
fn check_name(name: &str) -> Result<()> {
    if name.trim().is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
        bail!("\"{name}\" can't be a profile name; it's also a folder name under profiles/");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"{
  "default": "personal",
  "profiles": {
    "work": { "paths": ["~/code/acme"], "config_dir": "~/.claude-work" },
    "personal": { "config_dir": "~/.claude" }
  }
}
"#;

    struct Setup {
        _dir: tempfile::TempDir,
        home: PathBuf,
        user_home: PathBuf,
    }

    impl Setup {
        fn new(contents: Option<&str>) -> Setup {
            let dir = tempfile::tempdir().unwrap();
            let home = dir.path().join("swipe");
            let user_home = dir.path().join("user");
            fs::create_dir_all(user_home.join("code/acme")).unwrap();
            fs::create_dir_all(user_home.join("code/client")).unwrap();
            if let Some(contents) = contents {
                fs::create_dir_all(&home).unwrap();
                fs::write(home.join("profiles.json"), contents).unwrap();
            }
            Setup {
                _dir: dir,
                home,
                user_home,
            }
        }

        fn open(&self) -> ProfilesFile {
            ProfilesFile::open_in(self.home.clone(), self.user_home.clone()).unwrap()
        }

        fn text(&self) -> String {
            fs::read_to_string(self.home.join("profiles.json")).unwrap()
        }

        /// The profile `swipe which` would pick in `dir` under the user's home.
        fn which(&self, dir: &str) -> (String, Option<String>) {
            let config = Config::load_from(self.home.clone(), self.user_home.clone()).unwrap();
            let p = config.profile_for(&self.user_home.join(dir)).unwrap();
            (p.name.clone(), p.op_account.clone())
        }
    }

    #[test]
    fn adding_a_profile_saves_it_last_and_swipe_then_uses_it() {
        let s = Setup::new(Some(FILE));
        let mut file = s.open();
        file.add_profile("client", "~/.claude-client").unwrap();
        file.add_path("client", "~/code/client").unwrap();
        file.set_op_account("client", Some("client.1password.com"))
            .unwrap();
        file.save().unwrap();

        let text = s.text();
        let order: Vec<usize> = [
            "\"default\"",
            "\"work\": {",
            "\"paths\"",
            "\"config_dir\"",
            "\"personal\": {",
            "\"client\": {",
        ]
        .iter()
        .map(|k| {
            text.find(k)
                .unwrap_or_else(|| panic!("{k} missing from\n{text}"))
        })
        .collect();
        assert!(
            order.windows(2).all(|w| w[0] < w[1]),
            "order changed:\n{text}"
        );
        assert_eq!(
            s.which("code/client"),
            ("client".into(), Some("client.1password.com".into()))
        );
    }

    #[test]
    fn a_save_that_would_not_load_is_refused_and_the_file_is_untouched() {
        let s = Setup::new(Some(FILE));
        let mut file = s.open();
        file.add_profile("copy", "~/.claude-work").unwrap();
        assert!(file.problem().unwrap().contains("share the config_dir"));
        let err = file.save().unwrap_err().to_string();
        assert!(err.contains("share the config_dir"), "{err}");
        assert_eq!(s.text(), FILE);
        assert!(file.is_changed());
    }

    #[test]
    fn edits_reach_the_disk_only_on_save() {
        let s = Setup::new(Some(FILE));
        let mut file = s.open();
        assert!(!file.is_changed());
        file.remove_profile("work").unwrap();
        file.set_default(None).unwrap();
        assert!(file.is_changed());
        drop(file);
        assert_eq!(s.text(), FILE);
    }

    #[test]
    fn the_first_save_creates_profiles_json_with_the_first_profile_as_default() {
        let s = Setup::new(None);
        let mut file = s.open();
        assert!(file.entries().is_empty());
        file.add_profile("personal", "~/.claude").unwrap();
        file.save().unwrap();
        assert!(
            s.text().starts_with("{\n  \"default\": \"personal\""),
            "{}",
            s.text()
        );
        assert_eq!(s.which("code/acme").0, "personal");
    }

    #[test]
    fn removing_a_profile_leaves_its_folders_and_says_where_they_are() {
        let s = Setup::new(Some(FILE));
        let config_dir = s.user_home.join(".claude-work");
        let source = s.home.join("profiles/work");
        fs::create_dir_all(&config_dir).unwrap();
        fs::create_dir_all(&source).unwrap();
        let mut file = s.open();
        file.remove_profile("work").unwrap();
        let saved = file.save().unwrap();
        assert_eq!(
            saved.removed,
            vec![("work".into(), config_dir.clone(), source.clone())]
        );
        assert!(config_dir.is_dir() && source.is_dir());
        assert!(!s.text().contains("work"), "{}", s.text());
    }

    #[test]
    fn removing_the_default_profile_clears_the_default() {
        let s = Setup::new(Some(FILE));
        let mut file = s.open();
        file.remove_profile("personal").unwrap();
        assert_eq!(file.default(), None);
        file.save().unwrap();
        assert!(!s.text().contains("default"), "{}", s.text());
    }

    #[test]
    fn renaming_a_profile_moves_its_source_folder_and_the_default() {
        let s = Setup::new(Some(FILE));
        fs::create_dir_all(s.home.join("profiles/personal")).unwrap();
        fs::write(s.home.join("profiles/personal/CLAUDE.md"), "mine").unwrap();
        let mut file = s.open();
        file.rename_profile("personal", "home").unwrap();
        let saved = file.save().unwrap();
        assert!(saved.removed.is_empty());
        assert_eq!(file.default(), Some("home"));
        assert_eq!(
            fs::read_to_string(s.home.join("profiles/home/CLAUDE.md")).unwrap(),
            "mine"
        );
        assert!(!s.home.join("profiles/personal").exists());
        assert_eq!(s.which("code/client").0, "home");
        let text = s.text();
        assert!(
            text.find("\"work\": {") < text.find("\"home\": {"),
            "{text}"
        );
    }

    #[test]
    fn renaming_onto_an_existing_source_folder_is_refused() {
        let s = Setup::new(Some(FILE));
        fs::create_dir_all(s.home.join("profiles/work")).unwrap();
        fs::create_dir_all(s.home.join("profiles/job")).unwrap();
        let mut file = s.open();
        let err = file.rename_profile("work", "job").unwrap_err().to_string();
        assert!(err.contains("profiles") && err.contains("job"), "{err}");
        assert!(!file.is_changed());
        assert!(s.home.join("profiles/work").is_dir());
    }

    #[test]
    fn renaming_back_and_forth_moves_nothing() {
        let s = Setup::new(Some(FILE));
        fs::create_dir_all(s.home.join("profiles/work")).unwrap();
        let mut file = s.open();
        file.rename_profile("work", "job").unwrap();
        file.rename_profile("job", "work").unwrap();
        assert!(!file.is_changed());
        assert!(!file.save().unwrap().needs_sync);
        assert!(s.home.join("profiles/work").is_dir());
    }

    #[test]
    fn only_adds_renames_and_config_dir_changes_need_a_sync() {
        let s = Setup::new(Some(FILE));
        let mut file = s.open();
        file.add_path("work", "~/code/client").unwrap();
        file.set_op_account("work", Some("acme")).unwrap();
        file.set_default(Some("work")).unwrap();
        assert!(!file.save().unwrap().needs_sync);

        file.set_config_dir("work", "~/.claude-job").unwrap();
        assert!(file.save().unwrap().needs_sync);
        file.rename_profile("work", "job").unwrap();
        assert!(file.save().unwrap().needs_sync);
        file.add_profile("client", "~/.claude-client").unwrap();
        assert!(file.save().unwrap().needs_sync);
    }

    #[test]
    fn paths_accounts_and_the_default_can_be_set_and_cleared() {
        let s = Setup::new(Some(FILE));
        let mut file = s.open();
        file.add_path("work", "~/code/client").unwrap();
        file.set_op_account("work", Some("acme")).unwrap();
        file.set_default(Some("work")).unwrap();
        file.save().unwrap();
        assert_eq!(s.which("code/client"), ("work".into(), Some("acme".into())));

        file.remove_path("work", "~/code/client").unwrap();
        file.remove_path("work", "~/code/acme").unwrap();
        file.set_op_account("work", None).unwrap();
        file.set_default(Some("personal")).unwrap();
        file.save().unwrap();
        let work = &file.entries()[0];
        assert_eq!(work.paths, Vec::<String>::new());
        assert_eq!(work.op_account, None);
        assert!(!s.text().contains("paths"), "{}", s.text());
        assert_eq!(s.which("code/acme").0, "personal");
    }

    #[test]
    fn names_that_are_not_plain_folder_names_are_refused() {
        let s = Setup::new(Some(FILE));
        let mut file = s.open();
        for name in ["", " ", ".", "..", "a/b", "a\\b"] {
            assert!(file.add_profile(name, "~/.x").is_err(), "{name:?}");
            assert!(file.rename_profile("work", name).is_err(), "{name:?}");
        }
        assert!(file.add_profile("work", "~/.x").is_err());
        assert!(!file.is_changed());
    }

    #[test]
    fn a_file_that_does_not_load_still_opens_so_it_can_be_fixed() {
        let s = Setup::new(Some(
            r#"{"default": "gone", "profiles": {"a": {"config_dir": "~/.a"}}}"#,
        ));
        let mut file = s.open();
        assert!(file.problem().unwrap().contains("\"gone\" is not defined"));
        file.set_default(Some("a")).unwrap();
        assert_eq!(file.problem(), None);
        file.save().unwrap();
    }

    #[test]
    fn removing_a_broken_profile_saves_the_fixed_file() {
        let s = Setup::new(Some(
            r#"{"profiles": {"bad": {"config_dir": "rel"}, "a": {"config_dir": "~/.a"}}}"#,
        ));
        let mut file = s.open();
        file.remove_profile("bad").unwrap();
        let saved = file.save().unwrap();
        assert_eq!(saved.removed[0].1, PathBuf::from("rel"));
    }

    #[test]
    fn a_profile_removed_and_added_again_is_not_reported_removed() {
        let s = Setup::new(Some(FILE));
        let mut file = s.open();
        file.remove_profile("work").unwrap();
        file.add_profile("work", "~/.claude-job").unwrap();
        let saved = file.save().unwrap();
        assert!(saved.removed.is_empty());
        assert!(saved.needs_sync);
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_write_keeps_the_previous_file_whole() {
        use std::os::unix::fs::PermissionsExt;
        let s = Setup::new(Some(FILE));
        let mut file = s.open();
        file.set_default(Some("work")).unwrap();
        fs::set_permissions(&s.home, fs::Permissions::from_mode(0o555)).unwrap();
        let result = file.save();
        fs::set_permissions(&s.home, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(result.is_err());
        assert_eq!(s.text(), FILE);
        assert!(!s.home.join(".profiles.json.tmp").exists());
    }
}
