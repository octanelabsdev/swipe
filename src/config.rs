use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::{env, fs};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    default: Option<String>,
    profiles: BTreeMap<String, RawProfile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProfile {
    config_dir: String,
    #[serde(default)]
    paths: Vec<String>,
    op_account: Option<String>,
}

pub struct Profile {
    pub name: String,
    /// As configured, with `~` expanded and any trailing separator dropped. Not resolved further,
    /// because Claude Code keys its macOS login to this exact value.
    pub config_dir: PathBuf,
    paths: Vec<PathBuf>,
    /// The 1Password account `op` should default to, passed through untouched as OP_ACCOUNT.
    pub op_account: Option<String>,
}

pub struct Config {
    /// The source tree: `profiles.json`, `shared/` and `profiles/<name>/`.
    pub home: PathBuf,
    pub profiles: Vec<Profile>,
    default: Option<String>,
    user_home: PathBuf,
}

impl Config {
    pub fn load() -> Result<Config> {
        let (home, user_home) = locate()?;
        Config::load_from(home, user_home)
    }

    pub fn load_from(home: PathBuf, user_home: PathBuf) -> Result<Config> {
        let file = home.join("profiles.json");
        let text = fs::read_to_string(&file)
            .with_context(|| format!("could not read {}", file.display()))?;
        let raw: RawConfig =
            serde_json::from_str(&text).with_context(|| format!("invalid {}", file.display()))?;
        Config::from_raw(raw, home, user_home)
            .with_context(|| format!("invalid {}", file.display()))
    }

    /// Runs every check `load` makes, on a profiles.json that hasn't been written yet.
    pub fn check(value: &Value, home: &Path, user_home: &Path) -> Result<()> {
        let raw: RawConfig = serde_json::from_value(value.clone())?;
        Config::from_raw(raw, home.to_path_buf(), user_home.to_path_buf()).map(|_| ())
    }

    fn from_raw(raw: RawConfig, home: PathBuf, user_home: PathBuf) -> Result<Config> {
        let mut profiles: Vec<Profile> = Vec::new();
        for (name, p) in raw.profiles {
            let config_dir = expand(&p.config_dir, &user_home)
                .with_context(|| format!("profile \"{name}\": config_dir"))?;
            if let Some(other) = profiles
                .iter()
                .find(|o| same_path(&o.config_dir, &config_dir))
            {
                bail!(
                    "profiles \"{}\" and \"{name}\" share the config_dir {}",
                    other.name,
                    config_dir.display()
                );
            }
            let paths: Vec<PathBuf> = p
                .paths
                .iter()
                .map(|path| expand(path, &user_home))
                .collect::<Result<_>>()
                .with_context(|| format!("profile \"{name}\": paths"))?;
            for path in &paths {
                if let Some(other) = profiles
                    .iter()
                    .find(|o| o.paths.iter().any(|q| same_path(q, path)))
                {
                    bail!(
                        "profiles \"{}\" and \"{name}\" both list the path {}",
                        other.name,
                        path.display()
                    );
                }
            }
            if p.op_account.as_deref() == Some("") {
                bail!("profile \"{name}\": op_account must not be empty");
            }
            profiles.push(Profile {
                name,
                config_dir,
                paths,
                op_account: p.op_account,
            });
        }
        if let Some(default) = &raw.default
            && !profiles.iter().any(|p| &p.name == default)
        {
            bail!("default profile \"{default}\" is not defined");
        }
        Ok(Config {
            home,
            profiles,
            default: raw.default,
            user_home,
        })
    }

    /// The profile whose most specific path contains `dir`, else the default.
    pub fn profile_for(&self, dir: &Path) -> Result<&Profile> {
        let dir = fs::canonicalize(dir)
            .with_context(|| format!("no such directory: {}", dir.display()))?;
        let mut matches: Vec<(&Profile, usize)> = Vec::new();
        for profile in &self.profiles {
            for path in &profile.paths {
                // A path that doesn't exist (or can't be read) can't contain the working directory.
                if let Ok(path) = fs::canonicalize(path)
                    && dir.starts_with(&path)
                {
                    matches.push((profile, path.components().count()));
                }
            }
        }
        if let Some(deepest) = matches.iter().map(|&(_, depth)| depth).max() {
            let mut winners: Vec<&Profile> = matches
                .into_iter()
                .filter(|&(_, depth)| depth == deepest)
                .map(|(profile, _)| profile)
                .collect();
            winners.dedup_by(|a, b| a.name == b.name);
            if let [profile] = winners[..] {
                return Ok(profile);
            }
            let names: Vec<&str> = winners.iter().map(|p| p.name.as_str()).collect();
            bail!(
                "profiles {} all match {}; give one a more specific path",
                names.join(", "),
                dir.display()
            );
        }
        match &self.default {
            Some(name) => Ok(self
                .profiles
                .iter()
                .find(|p| &p.name == name)
                .expect("validated on load")),
            None => bail!(
                "no profile's paths contain {} and no \"default\" is set",
                dir.display()
            ),
        }
    }

    /// Claude Code's own default. Running with this dir means leaving CLAUDE_CONFIG_DIR unset,
    /// because setting it explicitly would point at a different (logged-out) account.
    pub fn is_default_dir(&self, dir: &Path) -> bool {
        is_default_dir(dir, &self.user_home)
    }

    pub fn login_email(&self, profile: &Profile) -> Option<String> {
        login_email(&profile.config_dir, &self.user_home)
    }
}

fn is_default_dir(dir: &Path, user_home: &Path) -> bool {
    same_path(dir, &user_home.join(".claude"))
}

/// Claude Code's `.claude.json` sits in the home dir, not in it, for the default `~/.claude`.
pub fn login_email(config_dir: &Path, user_home: &Path) -> Option<String> {
    let file = if is_default_dir(config_dir, user_home) {
        user_home.join(".claude.json")
    } else {
        config_dir.join(".claude.json")
    };
    let value: Value = serde_json::from_str(&fs::read_to_string(file).ok()?).ok()?;
    value["oauthAccount"]["emailAddress"]
        .as_str()
        .map(String::from)
}

/// The source tree (where `profiles.json` lives) and the user's home directory.
pub fn locate() -> Result<(PathBuf, PathBuf)> {
    let user_home = env::home_dir().context("could not determine your home directory")?;
    let home = match env::var_os("SWIPE_HOME") {
        Some(dir) => PathBuf::from(dir),
        None => env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| user_home.join(".config"))
            .join("swipe"),
    };
    Ok((std::path::absolute(home)?, user_home))
}

pub fn expand(path: &str, user_home: &Path) -> Result<PathBuf> {
    let expanded = if path == "~" {
        user_home.to_path_buf()
    } else if let Some(rest) = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        user_home.join(rest)
    } else {
        PathBuf::from(path)
    };
    if !expanded.is_absolute() {
        bail!("\"{path}\" must be an absolute path or start with ~");
    }
    // Rebuilding from components drops a trailing separator but keeps a root like `/` or `C:\`.
    Ok(expanded.components().collect())
}

/// macOS and Windows file systems ignore letter case by default.
pub const IGNORES_CASE: bool = cfg!(any(windows, target_os = "macos"));

/// Compares component by component, so `/` and `\` are equivalent on Windows.
fn same_path(a: &Path, b: &Path) -> bool {
    let parts = |p: &Path| -> Vec<String> {
        p.components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .map(|s| if IGNORES_CASE { s.to_lowercase() } else { s })
            .collect()
    };
    parts(a) == parts(b)
}
