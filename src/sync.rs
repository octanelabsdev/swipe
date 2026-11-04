//! Builds each profile's config dir from `shared/` plus `profiles/<name>/`.
//!
//! Files are symlinked to the source, or copied where symlinks aren't allowed (Windows without
//! Developer Mode) or `SWIPE_COPY=1` is set. `settings.json` is merged from both sources and
//! written as a real file. A manifest in each config dir records what swipe wrote, so it never
//! overwrites or deletes anything it didn't create.

use crate::config::{Config, IGNORES_CASE, Profile};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::{Component, Path, PathBuf};
use std::{env, fs, io};

pub const MANIFEST: &str = ".swipe-manifest.json";
const SETTINGS: &str = "settings.json";

#[derive(Default, Serialize, Deserialize)]
struct Manifest {
    files: BTreeMap<String, Entry>,
    settings: Option<String>,
}

/// How swipe placed a file: a symlink to the source, or a copy with the hash it had when written.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Entry {
    Link,
    Copy(String),
}

/// Syncs every profile, or just `only`. Returns false if anything was left unsynced.
pub fn sync(config: &Config, only: Option<&str>) -> bool {
    let mut syncer = Syncer {
        source_home: &config.home,
        copy: env::var_os("SWIPE_COPY").is_some_and(|v| v == "1"),
        problems: 0,
    };
    for profile in config
        .profiles
        .iter()
        .filter(|p| only.is_none_or(|n| p.name == n))
    {
        println!("{} ({})", profile.name, profile.config_dir.display());
        if let Err(e) = syncer.sync_profile(config, profile) {
            syncer.problem(&profile.config_dir, e);
        }
    }
    if syncer.problems > 0 {
        eprintln!(
            "swipe: {} problem(s); anything swipe did not create was left alone",
            syncer.problems
        );
    }
    syncer.problems == 0
}

struct Syncer<'a> {
    source_home: &'a Path,
    copy: bool,
    problems: usize,
}

impl Syncer<'_> {
    fn sync_profile(&mut self, config: &Config, profile: &Profile) -> Result<()> {
        let target = profile.config_dir.as_path();
        let old = read_manifest(target)?;
        let wanted = source_files(config, profile)?;
        let old_by_key: HashMap<String, &Entry> =
            old.files.iter().map(|(rel, e)| (key(rel), e)).collect();
        let mut new = Manifest::default();

        // Prune first, so a path that turned from a file into a directory (or back) is free.
        for (rel, entry) in &old.files {
            if wanted.contains_key(&key(rel)) {
                continue;
            }
            let dest = dest_path(target, rel);
            if let Err(e) = prune(self.source_home, target, &dest, entry) {
                self.problem(&dest, e);
                new.files.insert(rel.clone(), entry.clone()); // still ours; retry next time
            }
        }

        // Links already in the config dir go last: one may lead to another file here, which must
        // be placed first for the link to lead to the same source.
        let mut order: Vec<_> = wanted.iter().collect();
        order.sort_by_key(|(_, (rel, _))| {
            fs::symlink_metadata(dest_path(target, rel)).is_ok_and(|m| m.is_symlink())
        });
        for (k, (rel, source)) in order {
            let dest = dest_path(target, rel);
            match self.place(target, &dest, source, old_by_key.get(k).copied()) {
                Ok(entry) => {
                    new.files.insert(rel.clone(), entry);
                }
                Err(e) => {
                    self.problem(&dest, e);
                    if let Some(&entry) = old_by_key.get(k) {
                        new.files.insert(rel.clone(), entry.clone()); // still ours; retry next time
                    }
                }
            }
        }

        let settings = target.join(SETTINGS);
        new.settings = match sync_settings(config, profile, &settings, old.settings.as_deref()) {
            Ok(hash) => hash,
            Err(e) => {
                self.problem(&settings, e);
                old.settings
            }
        };

        fs::create_dir_all(target)?;
        fs::write(target.join(MANIFEST), serde_json::to_string_pretty(&new)?)?;
        Ok(())
    }

    /// Links or copies one file, unless something swipe didn't create is in the way.
    fn place(
        &mut self,
        target: &Path,
        dest: &Path,
        source: &Path,
        recorded: Option<&Entry>,
    ) -> Result<Entry> {
        if let Some(link) = symlinked_ancestor(target, dest) {
            bail!("is inside {}, which is a symlink", link.display());
        }
        let aside = aside_path(dest);
        if fs::symlink_metadata(&aside).is_ok() {
            bail!(
                "{} is in the way; it may be left from an interrupted sync, so check it and delete it",
                aside.display()
            );
        }
        match fs::symlink_metadata(dest) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
            Ok(meta) if meta.is_symlink() => {
                let points_to = fs::read_link(dest)?;
                let ours = points_to == source
                    || (recorded == Some(&Entry::Link) && points_to.starts_with(self.source_home))
                    || same_real_file(dest, source);
                if !ours {
                    bail!(
                        "is a symlink not made by swipe; remove it or move its target into {}",
                        self.source_home.display()
                    );
                }
                if points_to == source && !self.copy {
                    return Ok(Entry::Link);
                }
            }
            Ok(meta) if meta.is_file() => {
                let current = hash(&fs::read(dest)?);
                if recorded == Some(&Entry::Copy(current.clone())) {
                    // A file swipe copied stays a copy, so a re-run changes nothing.
                    return copy(dest, source, Some(&current));
                }
                if !same_content(dest, source) {
                    bail!(
                        "already exists or was edited in place; move what you want to keep into {}, delete it, then re-run",
                        self.source_home.display()
                    );
                }
                // Identical to its source, so replacing it below loses nothing.
            }
            Ok(_) => bail!(
                "is a directory; move it into {} or delete it",
                self.source_home.display()
            ),
        }

        fs::create_dir_all(dest.parent().expect("dest is inside the config dir"))?;
        // Move the original aside: a source that no longer reads once it is moved was this file.
        let aside = set_aside(dest, aside)?;
        let placed = if aside.is_some() && fs::metadata(source).is_err() {
            Err(anyhow::anyhow!(
                "its source {} leads back to this file; replace that link with the real file",
                source.display()
            ))
        } else {
            self.link_or_copy(dest, source)
        };
        if let Some(aside) = aside {
            if placed.is_ok() {
                fs::remove_file(&aside)?;
            } else {
                let _ = fs::remove_file(dest);
                fs::rename(&aside, dest)?;
            }
        }
        placed
    }

    fn link_or_copy(&mut self, dest: &Path, source: &Path) -> Result<Entry> {
        if !self.copy {
            match symlink(source, dest) {
                Ok(()) => {
                    println!("  link {} -> {}", dest.display(), source.display());
                    return Ok(Entry::Link);
                }
                Err(e) if symlinks_not_allowed(&e) => {
                    println!("  symlinks are not allowed here, so swipe will copy files instead");
                    self.copy = true;
                }
                Err(e) => return Err(e.into()),
            }
        }
        copy(dest, source, None)
    }

    fn problem(&mut self, path: &Path, error: anyhow::Error) {
        eprintln!("swipe: {}: {error:#}", path.display());
        self.problems += 1;
    }
}

/// Removes a file swipe placed, if it is still exactly what swipe put there.
fn prune(source_home: &Path, target: &Path, dest: &Path, recorded: &Entry) -> Result<()> {
    if let Some(link) = symlinked_ancestor(target, dest) {
        bail!(
            "is inside {}, which is a symlink, so swipe left it alone",
            link.display()
        );
    }
    let owned = match fs::symlink_metadata(dest) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        Err(e) => return Err(e.into()),
        Ok(meta) if meta.is_symlink() => {
            *recorded == Entry::Link && fs::read_link(dest)?.starts_with(source_home)
        }
        Ok(meta) if meta.is_file() => *recorded == Entry::Copy(hash(&fs::read(dest)?)),
        Ok(_) => false,
    };
    if owned {
        println!("  remove {} (no longer in source)", dest.display());
        fs::remove_file(dest)?;
        for dir in dest.ancestors().skip(1).take_while(|d| *d != target) {
            if fs::remove_dir(dir).is_err() {
                break; // not empty
            }
        }
    }
    Ok(())
}

/// Writes the merged settings, or removes swipe's settings.json once no source has any.
/// Returns the hash of what is now swipe's settings.json.
fn sync_settings(
    config: &Config,
    profile: &Profile,
    dest: &Path,
    recorded: Option<&str>,
) -> Result<Option<String>> {
    let merged = merged_settings(config, profile)?;
    let current = match fs::symlink_metadata(dest) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
        Ok(meta) if meta.is_symlink() => {
            bail!(
                "is a symlink; swipe writes settings.json as a real file, so remove the link and re-run"
            )
        }
        Ok(_) => Some(hash(&fs::read(dest)?)),
    };
    let ours = current.is_none() || current.as_deref() == recorded;

    let Some(text) = merged else {
        if current.is_some() && ours {
            println!("  remove {} (no longer in source)", dest.display());
            fs::remove_file(dest)?;
        }
        return Ok(None);
    };
    let new = hash(text.as_bytes());
    if current.as_deref() == Some(new.as_str()) {
        return Ok(Some(new));
    }
    // Same settings, only formatted differently: rewriting it loses nothing.
    let same_settings = || {
        let parse = |t: &str| serde_json::from_str::<Value>(t).ok();
        fs::read_to_string(dest).ok().and_then(|t| parse(&t)) == parse(&text)
    };
    if !ours && !same_settings() {
        bail!(
            "was changed outside swipe; copy what you want to keep into your source settings.json, delete this file, then re-run"
        );
    }
    println!("  write {}", dest.display());
    fs::create_dir_all(dest.parent().expect("dest is inside the config dir"))?;
    fs::write(dest, text)?;
    Ok(Some(new))
}

/// `shared/settings.json` with `profiles/<name>/settings.json` merged over it, if either exists.
fn merged_settings(config: &Config, profile: &Profile) -> Result<Option<String>> {
    let sources = [
        config.home.join("shared").join(SETTINGS),
        config
            .home
            .join("profiles")
            .join(&profile.name)
            .join(SETTINGS),
    ];
    let mut merged: Option<Value> = None;
    for source in sources.iter().filter(|s| s.exists()) {
        let value: Value = serde_json::from_str(&fs::read_to_string(source)?)
            .with_context(|| format!("{} is not valid JSON", source.display()))?;
        if !value.is_object() {
            bail!("{} must contain a JSON object", source.display());
        }
        match &mut merged {
            Some(base) => merge(base, value),
            None => merged = Some(value),
        }
    }
    Ok(match merged {
        Some(value) => Some(serde_json::to_string_pretty(&value)? + "\n"),
        None => None,
    })
}

/// Objects merge key by key, arrays gain the overlay's new items, anything else is replaced.
fn merge(base: &mut Value, overlay: Value) {
    match (base, overlay) {
        (Value::Object(base), Value::Object(overlay)) => {
            for (key, value) in overlay {
                match base.get_mut(&key) {
                    Some(existing) => merge(existing, value),
                    None => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (Value::Array(base), Value::Array(overlay)) => {
            for value in overlay {
                if !base.contains(&value) {
                    base.push(value);
                }
            }
        }
        (base, overlay) => *base = overlay,
    }
}

/// Every source file for a profile, as `key -> (relative path, source path)`. A file in
/// `profiles/<name>/` replaces the one at the same path in `shared/`. Symlinked directories are
/// followed, so every entry is a regular file.
fn source_files(config: &Config, profile: &Profile) -> Result<BTreeMap<String, (String, PathBuf)>> {
    let mut files = BTreeMap::new();
    for root in [
        config.home.join("shared"),
        config.home.join("profiles").join(&profile.name),
    ] {
        if root.is_dir() {
            walk(&root, &root, &mut files, &mut Vec::new())?;
        }
    }
    Ok(files)
}

fn walk(
    root: &Path,
    dir: &Path,
    files: &mut BTreeMap<String, (String, PathBuf)>,
    open_dirs: &mut Vec<PathBuf>,
) -> Result<()> {
    let real = fs::canonicalize(dir)?;
    if open_dirs.contains(&real) {
        return Ok(()); // a symlink loop back to a directory we're already inside
    }
    open_dirs.push(real);
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let meta = fs::metadata(&path)
            .with_context(|| format!("broken symlink in source: {}", path.display()))?;
        if meta.is_dir() {
            walk(root, &path, files, open_dirs)?;
            continue;
        }
        let rel = path
            .strip_prefix(root)?
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let k = key(&rel);
        if k == key(SETTINGS)
            || k == key(MANIFEST)
            || path.file_name().is_some_and(|n| n == ".DS_Store")
        {
            continue;
        }
        files.insert(k, (rel, path));
    }
    open_dirs.pop();
    Ok(())
}

/// True if swipe has placed anything in this config dir.
pub fn placed_anything(target: &Path) -> Result<bool> {
    let manifest = read_manifest(target)?;
    Ok(!manifest.files.is_empty() || manifest.settings.is_some())
}

fn read_manifest(target: &Path) -> Result<Manifest> {
    let path = target.join(MANIFEST);
    match fs::read_to_string(&path) {
        Ok(text) => {
            let mut manifest: Manifest = serde_json::from_str(&text)
                .with_context(|| format!("{} is corrupt; delete it and re-run", path.display()))?;
            // Entries are always plain relative paths; anything else wasn't written by swipe.
            manifest.files.retain(|rel, _| {
                Path::new(rel)
                    .components()
                    .all(|c| matches!(c, Component::Normal(_)))
            });
            Ok(manifest)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Manifest::default()),
        Err(e) => Err(e.into()),
    }
}

/// The first directory between the config dir and `dest` that is a symlink. Writing or deleting
/// through one would touch files outside the config dir.
fn symlinked_ancestor(target: &Path, dest: &Path) -> Option<PathBuf> {
    dest.ancestors()
        .skip(1)
        .take_while(|dir| *dir != target)
        .find(|dir| fs::symlink_metadata(dir).is_ok_and(|m| m.is_symlink()))
        .map(Path::to_path_buf)
}

/// Copies `source` over `dest` unless it already holds the same content.
fn copy(dest: &Path, source: &Path, current: Option<&str>) -> Result<Entry> {
    let contents = fs::read(source)?;
    let new = hash(&contents);
    if current != Some(new.as_str()) {
        println!("  copy {} -> {}", dest.display(), source.display());
        fs::write(dest, &contents)?;
    }
    keep_executable(dest, source)?;
    Ok(Entry::Copy(new))
}

/// Gives the copy the source's executable bits, so a copied hook script still runs. Only those
/// bits: a read-only source must not make a copy swipe can't update later.
#[cfg(unix)]
fn keep_executable(dest: &Path, source: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(dest)?.permissions().mode();
    let wanted = (mode & !0o111) | (fs::metadata(source)?.permissions().mode() & 0o111);
    if wanted != mode {
        fs::set_permissions(dest, fs::Permissions::from_mode(wanted))?;
    }
    Ok(())
}

#[cfg(windows)]
fn keep_executable(_dest: &Path, _source: &Path) -> Result<()> {
    Ok(())
}

/// Manifest paths always use `/`. Pushing their components one at a time gives native separators,
/// and they're the same components `read_manifest` checked are plain names.
fn dest_path(target: &Path, rel: &str) -> PathBuf {
    let mut path = target.to_path_buf();
    path.extend(Path::new(rel).components());
    path
}

/// True if both files can be read and hold the same bytes, so one can replace the other.
fn same_content(a: &Path, b: &Path) -> bool {
    matches!((fs::read(a), fs::read(b)), (Ok(x), Ok(y)) if x == y)
}

/// True if both paths lead to the same real file, as after `swipe import` copies a file link.
fn same_real_file(a: &Path, b: &Path) -> bool {
    matches!((fs::canonicalize(a), fs::canonicalize(b)), (Ok(x), Ok(y)) if x == y)
}

/// Paths that differ only in letter case are the same file on macOS and Windows.
fn key(rel: &str) -> String {
    if IGNORES_CASE {
        rel.to_lowercase()
    } else {
        rel.to_owned()
    }
}

/// FNV-1a: stable across Rust versions and platforms, which is all a change check needs.
fn hash(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf29ce484222325_u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    });
    format!("{hash:016x}")
}

pub const ASIDE: &str = ".swipe-replacing";

fn aside_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().expect("dest has a file name").to_owned();
    name.push(ASIDE);
    path.with_file_name(name)
}

/// Renames `path` to `aside`, if there is anything at `path`.
fn set_aside(path: &Path, aside: PathBuf) -> Result<Option<PathBuf>> {
    if fs::symlink_metadata(path).is_err() {
        return Ok(None);
    }
    fs::rename(path, &aside)?;
    Ok(Some(aside))
}

#[cfg(unix)]
pub fn symlink(source: &Path, dest: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(source, dest)
}

#[cfg(windows)]
pub fn symlink(source: &Path, dest: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_file(source, dest)
}

/// Windows refuses symlinks (ERROR_PRIVILEGE_NOT_HELD) unless Developer Mode is on or you're an admin.
pub fn symlinks_not_allowed(e: &io::Error) -> bool {
    cfg!(windows) && e.raw_os_error() == Some(1314)
}
