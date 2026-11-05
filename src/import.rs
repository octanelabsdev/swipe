//! Copies the files you wrote yourself from an existing config dir into the source tree, then
//! syncs. Nothing in the config dir is deleted: sync swaps each original for a link only when the
//! two are identical, so a failure at any point leaves every original in place.

use crate::config::Config;
use crate::sync;
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::{fs, io};

/// What Claude Code reads that you write yourself. Everything else in a config dir (the login,
/// history, projects, caches, plugins) belongs to Claude Code and stays where it is.
const IMPORTABLE: &[&str] = &[
    "CLAUDE.md",
    "settings.json",
    "keybindings.json",
    "agents",
    "commands",
    "hooks",
    "output-styles",
    "skills",
];

/// Folders inside those that Claude Code fills in itself, for one account only.
const CLAUDE_MANAGED: &[&str] = &["skills/synced", "skills/.trash"];

/// Folders a tool rebuilds for each machine.
const REBUILT: &[&str] = &[".venv", "node_modules", "__pycache__"];

/// Returns false if anything was left behind or the follow-up sync had problems.
pub fn import(config: &Config, name: &str) -> Result<bool> {
    let profile = config
        .profiles
        .iter()
        .find(|p| p.name == name)
        .with_context(|| format!("no profile named \"{name}\" in profiles.json"))?;
    let target = &profile.config_dir;
    let into = config.home.join("profiles").join(name);

    if sync::placed_anything(target)? {
        bail!(
            "{} is already synced by swipe; if an earlier import stopped with problems, fix them and run `swipe sync`, otherwise copy any remaining files into {} by hand",
            target.display(),
            into.display()
        );
    }
    let items: Vec<&str> = IMPORTABLE
        .iter()
        .copied()
        .filter(|item| fs::symlink_metadata(target.join(item)).is_ok())
        .collect();
    for item in &items {
        let to = into.join(item);
        if fs::symlink_metadata(&to).is_ok() {
            bail!(
                "{} already exists, so nothing was imported; if an earlier import got this far, run `swipe sync` to finish it",
                to.display()
            );
        }
    }
    let settings = target.join("settings.json");
    if fs::symlink_metadata(&settings).is_ok_and(|m| m.is_symlink()) {
        bail!(
            "{} is a symlink; swipe writes settings.json as a real file, so replace the link with a copy of its file and re-run; nothing was imported",
            settings.display()
        );
    }
    if settings.exists() {
        let value: Value =
            serde_json::from_str(&fs::read_to_string(&settings)?).with_context(|| {
                format!(
                    "{} is not valid JSON; nothing was imported",
                    settings.display()
                )
            })?;
        if !value.is_object() {
            bail!(
                "{} must contain a JSON object; nothing was imported",
                settings.display()
            );
        }
    }

    // Copy into a staging folder first, so a failed copy leaves the source tree as it was.
    let staging = config
        .home
        .join("profiles")
        .join(format!(".{name}.importing"));
    if staging.exists() {
        fs::remove_dir_all(&staging)?; // left behind by an earlier failed import
    }
    let mut copier = Copier {
        target,
        config_dir: fs::canonicalize(target).unwrap_or_else(|_| target.clone()),
        into: &into,
        staging: &staging,
        items: &items,
        notes: Vec::new(),
        rewritten: Vec::new(),
    };
    for item in &items {
        if let Err(e) = copier.copy(&target.join(item), &staging.join(item)) {
            let _ = fs::remove_dir_all(&staging);
            return Err(e.context(format!("could not copy {item}; nothing was imported")));
        }
    }
    if let Err(e) = copier.drop_links_to_files_left_behind() {
        let _ = fs::remove_dir_all(&staging);
        return Err(e.context("nothing was imported"));
    }
    fs::create_dir_all(&into)?;
    for item in &items {
        if staging.join(item).exists() {
            println!("import {}", target.join(item).display());
            fs::rename(staging.join(item), into.join(item)).with_context(|| {
                format!(
                    "could not move {item} into {}; move what is left in {} there by hand, then run `swipe sync`",
                    into.display(),
                    staging.display()
                )
            })?;
        }
    }
    let _ = fs::remove_dir_all(&staging);
    if items.is_empty() {
        println!("nothing to import from {}", target.display());
    }
    for note in &copier.notes {
        println!("{note}");
    }

    Ok(sync::sync(config, Some(name)))
}

struct Copier<'a> {
    /// The config dir as profiles.json spells it, which every copied path starts with.
    target: &'a Path,
    config_dir: PathBuf,
    into: &'a Path,
    staging: &'a Path,
    items: &'a [&'a str],
    /// One line for each thing left in place or copied instead of linked.
    notes: Vec<String>,
    /// Links pointed at an imported copy: (the original, the staged link, the staged copy).
    rewritten: Vec<(PathBuf, PathBuf, PathBuf)>,
}

impl Copier<'_> {
    /// Copies a file or folder. A symlink to a file is kept as a symlink to the same real file, or
    /// to its imported copy if that file is in the config dir. A symlink to a folder, or a folder
    /// that is a git repo, is not copied: it stays where it is, so the link stays live and git
    /// keeps working.
    fn copy(&mut self, from: &Path, to: &Path) -> Result<()> {
        if from.to_string_lossy().ends_with(sync::ASIDE) {
            self.notes.push(format!(
                "left in place: {} (left by an interrupted sync; check it and delete it)",
                from.display()
            ));
            return Ok(());
        }
        let meta = fs::symlink_metadata(from)?;
        if meta.is_symlink() {
            return self.copy_link(from, to);
        }
        if meta.is_dir() && fs::symlink_metadata(from.join(".git")).is_ok() {
            self.notes.push(format!(
                "left in place: {} (a git repo; copy it into the source tree by hand if you want it shared)",
                from.display()
            ));
            return Ok(());
        }
        if meta.is_dir() {
            if let Some(why) = self.not_yours(from) {
                self.notes
                    .push(format!("left in place: {} ({why})", from.display()));
                return Ok(());
            }
            for entry in fs::read_dir(from)? {
                let entry = entry?;
                self.copy(&entry.path(), &to.join(entry.file_name()))?;
            }
            return Ok(());
        }
        create_parent(to)?;
        fs::copy(from, to).with_context(|| format!("{} can't be read", from.display()))?;
        Ok(())
    }

    fn not_yours(&self, dir: &Path) -> Option<&'static str> {
        let rel = dir.strip_prefix(self.target).ok()?;
        if CLAUDE_MANAGED.iter().any(|m| rel == Path::new(m)) {
            return Some("Claude Code fills it in for this account");
        }
        let name = dir.file_name()?;
        REBUILT
            .iter()
            .any(|r| name == *r)
            .then_some("rebuilt on each machine")
    }

    fn copy_link(&mut self, from: &Path, to: &Path) -> Result<()> {
        let mut points_to = fs::canonicalize(from).map_err(|e| {
            let problem = if e.kind() == io::ErrorKind::NotFound {
                "is a broken symlink"
            } else {
                "is a symlink that can't be followed"
            };
            anyhow::Error::new(e).context(format!("{} {problem}", from.display()))
        })?;
        if points_to.is_dir() {
            self.notes.push(format!(
                "left in place: {} (a link to a folder; it stays live)",
                from.display()
            ));
            return Ok(());
        }
        // A link to another file in the config dir must point at that file's imported copy, so the
        // source tree doesn't depend on the config dir it feeds.
        if let Ok(rel) = points_to.strip_prefix(&self.config_dir) {
            let imported = rel
                .components()
                .next()
                .is_some_and(|first| self.items.iter().any(|item| first.as_os_str() == *item));
            if !imported {
                self.notes.push(format!(
                    "left in place: {} (a link to {}, which is not imported)",
                    from.display(),
                    points_to.display()
                ));
                return Ok(());
            }
            self.rewritten
                .push((from.to_path_buf(), to.to_path_buf(), self.staging.join(rel)));
            points_to = self.into.join(rel);
        }
        create_parent(to)?;
        match sync::symlink(&points_to, to) {
            Err(e) if sync::symlinks_not_allowed(&e) => {
                fs::copy(from, to)?;
                self.notes.push(format!(
                    "copied: {} (symlinks are not allowed here, so the copy no longer follows {})",
                    from.display(),
                    points_to.display()
                ));
                Ok(())
            }
            result => Ok(result?),
        }
    }

    /// A link can point at a file that was not copied after all, such as one in a git repo that
    /// stays in place. Those links stay in place too.
    fn drop_links_to_files_left_behind(&mut self) -> Result<()> {
        for (from, staged_link, staged_copy) in &self.rewritten {
            if fs::symlink_metadata(staged_copy).is_err() {
                fs::remove_file(staged_link)?;
                self.notes.push(format!(
                    "left in place: {} (it links to a file that stays in place)",
                    from.display()
                ));
            }
        }
        Ok(())
    }
}

fn create_parent(path: &Path) -> io::Result<()> {
    match path.parent() {
        Some(parent) => fs::create_dir_all(parent),
        None => Ok(()),
    }
}
