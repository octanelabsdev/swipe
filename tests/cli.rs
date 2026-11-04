use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const CONFIG: &str = r#"{
  "default": "personal",
  "profiles": {
    "personal": { "config_dir": "~/.claude" },
    "work": { "config_dir": "~/.claude-work", "paths": ["~/IOU"] },
    "special": { "config_dir": "~/.claude-special/", "paths": ["~/IOU/special"] }
  }
}"#;

/// A throwaway home directory (with a space in its path) holding a cprof source tree.
struct Sandbox {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    src: PathBuf,
}

impl Sandbox {
    fn new() -> Sandbox {
        let dir = tempfile::Builder::new()
            .prefix("cprof test")
            .tempdir()
            .unwrap();
        let root = dir.path().to_path_buf();
        let home = root.join("home");
        let src = root.join("src");
        for d in ["IOU/app/deep", "IOU/special", "elsewhere"] {
            fs::create_dir_all(home.join(d)).unwrap();
        }
        fs::create_dir_all(src.join("shared")).unwrap();
        fs::write(src.join("profiles.json"), CONFIG).unwrap();
        Sandbox {
            _dir: dir,
            root,
            home,
            src,
        }
    }

    fn cprof(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_cprof"));
        cmd.args(args)
            .current_dir(&self.home)
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("CPROF_HOME", &self.src)
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("CPROF_COPY");
        cmd
    }

    fn run(&self, args: &[&str]) -> Output {
        self.cprof(args).output().unwrap()
    }

    fn which(&self, dir: &Path) -> String {
        stdout(&self.run(&["which", dir.to_str().unwrap()]))
    }

    fn write(&self, rel: &str, contents: &str) {
        let path = self.src.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn config(&self, json: &str) {
        fs::write(self.src.join("profiles.json"), json).unwrap();
    }

    fn sync(&self) -> Output {
        self.run(&["sync"])
    }

    fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.home.join(rel)).unwrap_or_else(|e| format!("<{e}>"))
    }

    fn expect(&self, profile: &str, dir: &str) -> String {
        format!("{profile}\t{}\n", self.home.join(dir).display())
    }
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn assert_ok(o: &Output) {
    assert!(
        o.status.success(),
        "expected success\nstdout: {}\nstderr: {}",
        stdout(o),
        stderr(o)
    );
}

fn assert_fails(o: &Output, message: &str) {
    assert!(
        !o.status.success(),
        "expected failure\nstdout: {}",
        stdout(o)
    );
    assert!(
        stderr(o).contains(message),
        "stderr should contain {message:?}\nstderr: {}",
        stderr(o)
    );
}

// --- which -----------------------------------------------------------------

#[test]
fn which_picks_the_profile_whose_path_contains_the_dir() {
    let s = Sandbox::new();
    assert_eq!(
        s.which(&s.home.join("IOU/app/deep")),
        s.expect("work", ".claude-work")
    );
}

#[test]
fn which_prefers_the_longest_matching_path() {
    let s = Sandbox::new();
    assert_eq!(
        s.which(&s.home.join("IOU/special")),
        s.expect("special", ".claude-special")
    );
}

#[test]
fn which_falls_back_to_the_default() {
    let s = Sandbox::new();
    assert_eq!(
        s.which(&s.home.join("elsewhere")),
        s.expect("personal", ".claude")
    );
}

#[test]
fn which_uses_the_current_dir() {
    let s = Sandbox::new();
    let out = s
        .cprof(&["which"])
        .current_dir(s.home.join("IOU"))
        .output()
        .unwrap();
    assert_eq!(stdout(&out), s.expect("work", ".claude-work"));
}

#[test]
fn which_does_not_match_a_sibling_sharing_a_prefix() {
    let s = Sandbox::new();
    fs::create_dir_all(s.home.join("IOUX")).unwrap();
    assert_eq!(
        s.which(&s.home.join("IOUX")),
        s.expect("personal", ".claude")
    );
}

#[test]
fn which_ignores_letter_case_where_the_file_system_does() {
    let s = Sandbox::new();
    if !s.home.join("iou").exists() {
        return; // case-sensitive file system
    }
    assert_eq!(
        s.which(&s.home.join("iou/app")),
        s.expect("work", ".claude-work")
    );
    s.config(&CONFIG.replace("~/IOU\"", "~/iou\""));
    assert_eq!(
        s.which(&s.home.join("IOU/app")),
        s.expect("work", ".claude-work")
    );
}

#[cfg(unix)]
#[test]
fn which_follows_symlinked_dirs() {
    let s = Sandbox::new();
    std::os::unix::fs::symlink(s.home.join("IOU/app"), s.home.join("applink")).unwrap();
    assert_eq!(
        s.which(&s.home.join("applink")),
        s.expect("work", ".claude-work")
    );
}

#[test]
fn which_ignores_configured_paths_that_do_not_exist() {
    let s = Sandbox::new();
    s.config(
        r#"{"default": "personal", "profiles": {
        "personal": {"config_dir": "~/.claude"},
        "gone": {"config_dir": "~/.claude-gone", "paths": ["~/missing"]}}}"#,
    );
    assert_eq!(
        s.which(&s.home.join("elsewhere")),
        s.expect("personal", ".claude")
    );
}

#[cfg(unix)]
#[test]
fn which_ignores_configured_paths_it_cannot_read() {
    use std::os::unix::fs::PermissionsExt;
    let s = Sandbox::new();
    fs::create_dir_all(s.home.join("locked")).unwrap();
    fs::set_permissions(s.home.join("locked"), fs::Permissions::from_mode(0o000)).unwrap();
    s.config(
        r#"{"default": "personal", "profiles": {
        "personal": {"config_dir": "~/.claude"},
        "vault": {"config_dir": "~/.claude-vault", "paths": ["~/locked"]}}}"#,
    );
    let result = s.which(&s.home.join("elsewhere"));
    fs::set_permissions(s.home.join("locked"), fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(result, s.expect("personal", ".claude"));
}

#[test]
fn which_fails_without_a_match_or_default() {
    let s = Sandbox::new();
    s.config(&CONFIG.replace("\"default\": \"personal\",", ""));
    assert_fails(
        &s.run(&["which", s.home.join("elsewhere").to_str().unwrap()]),
        "no \"default\" is set",
    );
}

#[test]
fn which_fails_without_a_config() {
    let s = Sandbox::new();
    fs::remove_file(s.src.join("profiles.json")).unwrap();
    assert_fails(&s.run(&["which"]), "could not read");
}

#[test]
fn which_uses_xdg_config_home_when_cprof_home_is_unset() {
    let s = Sandbox::new();
    let xdg = s.root.join("xdg");
    fs::create_dir_all(xdg.join("claude-profiles")).unwrap();
    fs::write(xdg.join("claude-profiles/profiles.json"), CONFIG).unwrap();
    let out = s
        .cprof(&["which"])
        .env_remove("CPROF_HOME")
        .env("XDG_CONFIG_HOME", &xdg)
        .current_dir(s.home.join("IOU"))
        .output()
        .unwrap();
    assert_eq!(stdout(&out), s.expect("work", ".claude-work"));
}

// --- config validation -----------------------------------------------------

fn assert_config_rejected(json: &str, message: &str) {
    let s = Sandbox::new();
    s.config(json);
    assert_fails(&s.run(&["which"]), message);
}

#[test]
fn rejects_paths_that_are_not_a_list() {
    assert_config_rejected(
        r#"{"profiles": {"w": {"config_dir": "~/.w", "paths": "~/IOU"}}}"#,
        "invalid type",
    );
}

#[test]
fn rejects_unknown_keys_so_typos_do_not_silently_change_accounts() {
    assert_config_rejected(
        r#"{"profiles": {"w": {"config_dir": "~/.w", "path": ["~/IOU"]}}}"#,
        "unknown field `path`",
    );
}

#[test]
fn rejects_empty_and_relative_paths() {
    assert_config_rejected(
        r#"{"profiles": {"w": {"config_dir": "~/.w", "paths": [""]}}}"#,
        "must be an absolute path",
    );
    assert_config_rejected(
        r#"{"profiles": {"w": {"config_dir": "rel-dir"}}}"#,
        "must be an absolute path",
    );
}

#[test]
fn rejects_two_profiles_sharing_a_config_dir() {
    assert_config_rejected(
        r#"{"profiles": {"a": {"config_dir": "~/.x"}, "b": {"config_dir": "~/.x/"}}}"#,
        "share the config_dir",
    );
}

#[test]
fn rejects_two_profiles_listing_the_same_path() {
    assert_config_rejected(
        r#"{"profiles": {"a": {"config_dir": "~/.a", "paths": ["~/w"]}, "b": {"config_dir": "~/.b", "paths": ["~/w/"]}}}"#,
        "both list the path",
    );
}

#[cfg(unix)]
#[test]
fn accepts_the_root_as_a_path() {
    let s = Sandbox::new();
    s.config(r#"{"profiles": {"all": {"config_dir": "~/.claude-all", "paths": ["/"]}}}"#);
    assert_eq!(
        s.which(&s.home.join("elsewhere")),
        s.expect("all", ".claude-all")
    );
}

#[test]
fn rejects_an_undefined_default() {
    assert_config_rejected(
        r#"{"default": "nope", "profiles": {"a": {"config_dir": "~/.x"}}}"#,
        "\"nope\" is not defined",
    );
}

// --- run and init ----------------------------------------------------------

/// Puts a fake `claude` that prints its environment and arguments first on PATH.
fn with_fake_claude(s: &Sandbox, cmd: &mut Command) {
    let bin = s.root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    if cfg!(windows) {
        fs::write(
            bin.join("claude.cmd"),
            "@echo CLAUDE_CONFIG_DIR=%CLAUDE_CONFIG_DIR% args=%*\r\n",
        )
        .unwrap();
    } else {
        let script = bin.join("claude");
        fs::write(
            &script,
            "#!/bin/sh\necho \"CLAUDE_CONFIG_DIR=${CLAUDE_CONFIG_DIR-<unset>} args=$*\"\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    let cprof_dir = Path::new(env!("CARGO_BIN_EXE_cprof")).parent().unwrap();
    let path = std::env::join_paths(
        [bin.as_path(), cprof_dir]
            .into_iter()
            .map(PathBuf::from)
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    cmd.env("PATH", path).env("CLAUDE_CONFIG_DIR", "/stale");
}

fn unset() -> &'static str {
    if cfg!(windows) {
        "" // cmd expands an unset variable to nothing inside a batch file
    } else {
        "<unset>"
    }
}

#[test]
fn run_sets_the_config_dir_and_forwards_arguments() {
    let s = Sandbox::new();
    let mut cmd = s.cprof(&["run", "--resume", "two words"]);
    cmd.current_dir(s.home.join("IOU/app"));
    with_fake_claude(&s, &mut cmd);
    let out = cmd.output().unwrap();
    assert_ok(&out);
    let expected = format!(
        "CLAUDE_CONFIG_DIR={} args=--resume",
        s.home.join(".claude-work").display()
    );
    assert!(stdout(&out).starts_with(&expected), "{}", stdout(&out));
    assert!(stdout(&out).contains("two words"), "{}", stdout(&out));
}

#[test]
fn run_unsets_the_config_dir_for_the_default_claude_dir() {
    let s = Sandbox::new();
    let mut cmd = s.cprof(&["run", "-p", "hi"]);
    cmd.current_dir(s.home.join("elsewhere"));
    with_fake_claude(&s, &mut cmd);
    let out = cmd.output().unwrap();
    assert_eq!(
        stdout(&out).trim(),
        format!("CLAUDE_CONFIG_DIR={} args=-p hi", unset())
    );
}

#[test]
fn run_does_not_start_claude_when_the_profile_cannot_be_resolved() {
    let s = Sandbox::new();
    s.config("{ not json");
    let mut cmd = s.cprof(&["run"]);
    with_fake_claude(&s, &mut cmd);
    let out = cmd.output().unwrap();
    assert_fails(&out, "invalid");
    assert_eq!(stdout(&out), "");
}

#[test]
fn run_explains_a_missing_claude() {
    let s = Sandbox::new();
    let out = s
        .cprof(&["run"])
        .env("PATH", s.root.join("empty"))
        .output()
        .unwrap();
    assert_fails(&out, "could not find `claude`");
}

#[cfg(windows)]
#[test]
fn run_treats_forward_slashes_as_the_default_dir_on_windows() {
    let s = Sandbox::new();
    let home = s.home.display().to_string().replace('\\', "/");
    s.config(&format!(
        r#"{{"default": "p", "profiles": {{"p": {{"config_dir": "{home}/.claude"}}}}}}"#
    ));
    let mut cmd = s.cprof(&["run"]);
    cmd.current_dir(s.home.join("elsewhere"));
    with_fake_claude(&s, &mut cmd);
    let out = cmd.output().unwrap();
    assert_eq!(
        stdout(&out).trim(),
        format!("CLAUDE_CONFIG_DIR={} args=", unset())
    );
}

#[test]
fn init_rejects_an_unknown_shell() {
    let s = Sandbox::new();
    assert_eq!(s.run(&["init", "tcsh"]).status.code(), Some(2));
}

#[cfg(unix)]
#[test]
fn posix_hook_works_in_bash_and_zsh_even_over_an_alias() {
    let s = Sandbox::new();
    for shell in ["bash", "zsh"] {
        if Command::new(shell).arg("-c").arg("true").output().is_err() {
            continue; // not installed
        }
        let script = format!(
            "{} alias claude=/nonexistent/claude\neval \"$(cprof init {shell})\"\nclaude --resume",
            if shell == "bash" {
                "shopt -s expand_aliases;"
            } else {
                ""
            }
        );
        let mut cmd = Command::new(shell);
        cmd.arg("-c")
            .arg(script)
            .current_dir(s.home.join("IOU/app"))
            .env("HOME", &s.home)
            .env("CPROF_HOME", &s.src);
        with_fake_claude(&s, &mut cmd);
        let out = cmd.output().unwrap();
        assert_eq!(
            stdout(&out).trim(),
            format!(
                "CLAUDE_CONFIG_DIR={} args=--resume",
                s.home.join(".claude-work").display()
            ),
            "{shell}: {}",
            stderr(&out)
        );
    }
}

#[cfg(windows)]
#[test]
fn powershell_hook_runs_claude_through_cprof() {
    let s = Sandbox::new();
    let mut cmd = Command::new("pwsh");
    cmd.args([
        "-NoProfile",
        "-Command",
        "cprof init powershell | Out-String | Invoke-Expression; claude --resume",
    ])
    .current_dir(s.home.join("IOU/app"))
    .env("USERPROFILE", &s.home)
    .env("CPROF_HOME", &s.src);
    with_fake_claude(&s, &mut cmd);
    let out = cmd.output().unwrap();
    assert_eq!(
        stdout(&out).trim(),
        format!(
            "CLAUDE_CONFIG_DIR={} args=--resume",
            s.home.join(".claude-work").display()
        ),
        "{}",
        stderr(&out)
    );
}

// --- sync: files -----------------------------------------------------------

#[test]
fn sync_puts_shared_files_in_every_profile_and_profile_files_win() {
    let s = Sandbox::new();
    s.write("shared/commands/a.md", "shared a");
    s.write("shared/commands/b.md", "shared b");
    s.write("profiles/work/commands/b.md", "work b");
    s.write("profiles/work/CLAUDE.md", "work rules");
    assert_ok(&s.sync());
    assert_eq!(s.read(".claude/commands/b.md"), "shared b");
    assert_eq!(s.read(".claude-work/commands/b.md"), "work b");
    assert_eq!(s.read(".claude-work/commands/a.md"), "shared a");
    assert_eq!(s.read(".claude-work/CLAUDE.md"), "work rules");
    assert!(!s.home.join(".claude/CLAUDE.md").exists());
}

#[test]
fn sync_is_idempotent() {
    let s = Sandbox::new();
    s.write("shared/commands/a.md", "a");
    s.write("shared/settings.json", r#"{"model": "x"}"#);
    assert_ok(&s.sync());
    let out = s.sync();
    assert_ok(&out);
    assert!(
        !stdout(&out).contains("  "),
        "second sync should change nothing:\n{}",
        stdout(&out)
    );
}

#[cfg(unix)]
#[test]
fn linked_files_pick_up_source_edits_without_a_sync() {
    let s = Sandbox::new();
    s.write("shared/commands/a.md", "before");
    assert_ok(&s.sync());
    s.write("shared/commands/a.md", "after");
    assert_eq!(s.read(".claude/commands/a.md"), "after");
    assert_eq!(s.read(".claude-work/commands/a.md"), "after");
}

#[test]
fn sync_never_overwrites_a_file_it_did_not_create() {
    let s = Sandbox::new();
    s.write("shared/hooks/h.sh", "source");
    fs::create_dir_all(s.home.join(".claude/hooks")).unwrap();
    fs::write(s.home.join(".claude/hooks/h.sh"), "mine").unwrap();
    let out = s.sync();
    assert_fails(&out, "already exists");
    assert_eq!(s.read(".claude/hooks/h.sh"), "mine");
    assert_eq!(
        s.read(".claude-work/hooks/h.sh"),
        "source",
        "other profiles still sync"
    );
}

#[cfg(unix)]
#[test]
fn sync_never_replaces_a_symlink_it_did_not_create() {
    let s = Sandbox::new();
    s.write("shared/hooks/h.sh", "source");
    fs::create_dir_all(s.home.join(".claude/hooks")).unwrap();
    std::os::unix::fs::symlink("/etc/hosts", s.home.join(".claude/hooks/h.sh")).unwrap();
    assert_fails(&s.sync(), "symlink not made by cprof");
    assert_eq!(
        fs::read_link(s.home.join(".claude/hooks/h.sh")).unwrap(),
        Path::new("/etc/hosts")
    );
}

#[test]
fn sync_removes_only_its_own_files_when_they_leave_the_source() {
    let s = Sandbox::new();
    s.write("shared/commands/gone.md", "x");
    s.write("shared/commands/kept.md", "y");
    s.write("shared/old/only.md", "z");
    assert_ok(&s.sync());
    fs::write(s.home.join(".claude/commands/own.md"), "own").unwrap();

    fs::remove_file(s.src.join("shared/commands/gone.md")).unwrap();
    fs::remove_dir_all(s.src.join("shared/old")).unwrap();
    assert_ok(&s.sync());
    assert!(fs::symlink_metadata(s.home.join(".claude/commands/gone.md")).is_err());
    assert!(
        !s.home.join(".claude/old").exists(),
        "directories emptied by a removal are removed"
    );
    assert_eq!(s.read(".claude/commands/kept.md"), "y");
    assert_eq!(s.read(".claude/commands/own.md"), "own");
}

#[test]
fn sync_handles_names_starting_with_a_dash() {
    let s = Sandbox::new();
    s.write("shared/-dash.md", "x");
    assert_ok(&s.sync());
    let out = s.sync();
    assert_ok(&out);
    assert!(!stdout(&out).contains("remove"), "{}", stdout(&out));
    assert_eq!(s.read(".claude/-dash.md"), "x");
}

#[test]
fn sync_handles_a_file_turning_into_a_directory_and_back() {
    let s = Sandbox::new();
    s.write("shared/agents", "file");
    assert_ok(&s.sync());
    fs::remove_file(s.src.join("shared/agents")).unwrap();
    s.write("shared/agents/a.md", "a");
    assert_ok(&s.sync());
    assert_eq!(s.read(".claude/agents/a.md"), "a");

    fs::remove_dir_all(s.src.join("shared/agents")).unwrap();
    s.write("shared/agents", "file again");
    assert_ok(&s.sync());
    assert_eq!(s.read(".claude/agents"), "file again");
}

#[cfg(unix)]
#[test]
fn sync_follows_symlinks_in_the_source() {
    let s = Sandbox::new();
    fs::create_dir_all(s.root.join("ext/skills")).unwrap();
    fs::write(s.root.join("ext/skills/base.md"), "base").unwrap();
    fs::write(s.root.join("real.md"), "real").unwrap();
    std::os::unix::fs::symlink(s.root.join("ext/skills"), s.src.join("shared/skills")).unwrap();
    std::os::unix::fs::symlink(s.root.join("real.md"), s.src.join("shared/linked.md")).unwrap();
    s.write("profiles/work/skills/extra.md", "extra");
    assert_ok(&s.sync());
    assert_eq!(s.read(".claude/linked.md"), "real");
    assert_eq!(s.read(".claude/skills/base.md"), "base");
    assert_eq!(s.read(".claude-work/skills/extra.md"), "extra");
    assert!(
        !s.home.join(".claude/skills/extra.md").exists(),
        "a profile file must not leak into other profiles"
    );
    assert!(
        !s.root.join("ext/skills/extra.md").exists(),
        "nothing is written into the linked directory"
    );
}

#[cfg(unix)]
#[test]
fn sync_refuses_to_write_through_a_symlinked_directory_in_the_config_dir() {
    let s = Sandbox::new();
    fs::create_dir_all(s.root.join("elsewhere-plugins")).unwrap();
    fs::create_dir_all(s.home.join(".claude")).unwrap();
    std::os::unix::fs::symlink(
        s.root.join("elsewhere-plugins"),
        s.home.join(".claude/plugins"),
    )
    .unwrap();
    s.write("shared/plugins/p.json", "{}");
    assert_fails(&s.sync(), "which is a symlink");
    assert!(!s.root.join("elsewhere-plugins/p.json").exists());
}

#[test]
fn sync_makes_a_relative_cprof_home_absolute() {
    let s = Sandbox::new();
    s.write("shared/commands/a.md", "a");
    let out = s
        .cprof(&["sync"])
        .env("CPROF_HOME", "src")
        .current_dir(&s.root)
        .output()
        .unwrap();
    assert_ok(&out);
    assert_eq!(s.read(".claude/commands/a.md"), "a");
}

#[test]
fn sync_takes_no_arguments() {
    let s = Sandbox::new();
    assert_eq!(s.run(&["sync", "--dry-run"]).status.code(), Some(2));
}

#[cfg(unix)]
#[test]
fn sync_follows_two_links_to_the_same_directory() {
    let s = Sandbox::new();
    fs::create_dir_all(s.root.join("ext")).unwrap();
    fs::write(s.root.join("ext/x.md"), "x").unwrap();
    std::os::unix::fs::symlink(s.root.join("ext"), s.src.join("shared/a")).unwrap();
    std::os::unix::fs::symlink(s.root.join("ext"), s.src.join("shared/b")).unwrap();
    assert_ok(&s.sync());
    assert_eq!(s.read(".claude/a/x.md"), "x");
    assert_eq!(s.read(".claude/b/x.md"), "x");
}

#[test]
fn sync_ignores_a_source_file_named_like_its_manifest() {
    let s = Sandbox::new();
    s.write("shared/.cprof-manifest.json", "{\"precious\": 1}");
    assert_ok(&s.sync());
    assert_eq!(
        fs::read_to_string(s.src.join("shared/.cprof-manifest.json")).unwrap(),
        "{\"precious\": 1}"
    );
}

#[test]
fn a_blocked_file_does_not_stop_the_rest_of_the_profile() {
    let s = Sandbox::new();
    s.write("shared/commands/a.md", "a");
    s.write("shared/z.md", "z");
    fs::create_dir_all(s.home.join(".claude")).unwrap();
    fs::write(
        s.home.join(".claude/commands"),
        "a file where cprof needs a directory",
    )
    .unwrap();
    let out = s.sync();
    assert_fails(
        &out,
        &s.home.join(".claude/commands/a.md").display().to_string(),
    );
    assert_eq!(s.read(".claude/z.md"), "z");
    assert_eq!(s.read(".claude-work/commands/a.md"), "a");
}

#[cfg(unix)]
#[test]
fn an_unreadable_source_file_does_not_orphan_earlier_copies() {
    use std::os::unix::fs::PermissionsExt;
    let s = Sandbox::new();
    let sync = || s.cprof(&["sync"]).env("CPROF_COPY", "1").output().unwrap();
    s.write("shared/a.md", "a");
    s.write("shared/b.md", "b");
    fs::set_permissions(s.src.join("shared/b.md"), fs::Permissions::from_mode(0o000)).unwrap();
    let first = sync();
    fs::set_permissions(s.src.join("shared/b.md"), fs::Permissions::from_mode(0o644)).unwrap();
    assert_fails(&first, "b.md");
    assert_eq!(s.read(".claude/a.md"), "a");
    assert_ok(&sync());
    assert_eq!(s.read(".claude/b.md"), "b");
}

#[cfg(unix)]
#[test]
fn sync_never_removes_a_symlink_the_user_put_in_place_of_its_own() {
    let s = Sandbox::new();
    s.write("shared/commands/a.md", "a");
    assert_ok(&s.sync());
    fs::remove_file(s.home.join(".claude/commands/a.md")).unwrap();
    fs::write(s.root.join("mine.md"), "mine").unwrap();
    std::os::unix::fs::symlink(s.root.join("mine.md"), s.home.join(".claude/commands/a.md"))
        .unwrap();
    fs::remove_file(s.src.join("shared/commands/a.md")).unwrap();
    assert_ok(&s.sync());
    assert_eq!(s.read(".claude/commands/a.md"), "mine");
}

#[cfg(unix)]
#[test]
fn sync_never_removes_through_a_symlinked_directory_in_the_config_dir() {
    let s = Sandbox::new();
    let sync = || s.cprof(&["sync"]).env("CPROF_COPY", "1").output().unwrap();
    s.write("shared/plugins/p.json", "{}");
    assert_ok(&sync());
    fs::rename(s.home.join(".claude/plugins"), s.root.join("outside")).unwrap();
    std::os::unix::fs::symlink(s.root.join("outside"), s.home.join(".claude/plugins")).unwrap();
    fs::remove_dir_all(s.src.join("shared/plugins")).unwrap();
    assert_fails(&sync(), "which is a symlink");
    assert!(s.root.join("outside/p.json").exists());
    assert!(
        fs::symlink_metadata(s.home.join(".claude/plugins"))
            .unwrap()
            .is_symlink()
    );
}

#[test]
fn profile_files_override_shared_files_whose_names_differ_only_in_case() {
    let s = Sandbox::new();
    s.write("shared/Commands/a.md", "shared");
    s.write("profiles/work/commands/a.md", "work");
    if !s.src.join("profiles/work/COMMANDS").exists() {
        return; // case-sensitive file system: these really are two different files
    }
    for copy in ["0", "1"] {
        let sync = || s.cprof(&["sync"]).env("CPROF_COPY", copy).output().unwrap();
        assert_ok(&sync());
        assert_eq!(s.read(".claude-work/commands/a.md"), "work");
        let again = sync();
        assert_ok(&again);
        assert!(
            !stdout(&again).contains("  "),
            "re-sync should change nothing:\n{}",
            stdout(&again)
        );
        fs::remove_dir_all(&s.home).unwrap();
        fs::create_dir_all(&s.home).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn a_failed_update_keeps_cprofs_claim_on_its_file() {
    use std::os::unix::fs::PermissionsExt;
    let s = Sandbox::new();
    let sync = || s.cprof(&["sync"]).env("CPROF_COPY", "1").output().unwrap();
    s.write("shared/a.md", "v1");
    assert_ok(&sync());
    s.write("shared/a.md", "v2");
    fs::set_permissions(s.src.join("shared/a.md"), fs::Permissions::from_mode(0o000)).unwrap();
    let failed = sync();
    fs::set_permissions(s.src.join("shared/a.md"), fs::Permissions::from_mode(0o644)).unwrap();
    assert_fails(&failed, "a.md");
    assert_ok(&sync());
    assert_eq!(s.read(".claude/a.md"), "v2");
    fs::remove_file(s.src.join("shared/a.md")).unwrap();
    assert_ok(&sync());
    assert!(
        !s.home.join(".claude/a.md").exists(),
        "cprof still removes its own copy"
    );
}

#[test]
fn reserved_names_are_skipped_whatever_their_case() {
    let s = Sandbox::new();
    s.write("shared/.CPROF-MANIFEST.json", "precious");
    if !s.src.join("shared/.cprof-manifest.json").exists() {
        return; // case-sensitive file system: a different name, synced like any other file
    }
    s.write("shared/Settings.json", "{\"model\": \"raw\"}");
    assert_ok(&s.sync());
    assert_eq!(
        fs::read_to_string(s.src.join("shared/.CPROF-MANIFEST.json")).unwrap(),
        "precious"
    );
    // On this file system Settings.json *is* settings.json: it is merged and written as a real file.
    assert!(
        !fs::symlink_metadata(s.home.join(".claude/settings.json"))
            .unwrap()
            .is_symlink()
    );
    assert!(s.read(".claude/settings.json").contains("\"raw\""));
}

#[cfg(unix)]
#[test]
fn manifest_entries_outside_the_config_dir_are_ignored() {
    let s = Sandbox::new();
    s.write("shared/a.md", "a");
    assert_ok(&s.sync());
    // Something outside the config dir that would pass cprof's ownership check.
    fs::create_dir_all(s.root.join("outside")).unwrap();
    std::os::unix::fs::symlink(s.src.join("shared/a.md"), s.root.join("outside/victim.md"))
        .unwrap();
    let manifest = s.home.join(".claude/.cprof-manifest.json");
    let tampered = fs::read_to_string(&manifest).unwrap().replace(
        "\"files\": {",
        "\"files\": {\n    \"../../outside/victim.md\": \"link\",",
    );
    fs::write(&manifest, tampered).unwrap();
    assert_ok(&s.sync());
    assert!(fs::symlink_metadata(s.root.join("outside/victim.md")).is_ok());
}

// --- sync: copy mode (what Windows uses without Developer Mode) -------------

#[test]
fn copy_mode_copies_updates_and_protects_local_edits() {
    let s = Sandbox::new();
    let sync = || s.cprof(&["sync"]).env("CPROF_COPY", "1").output().unwrap();
    s.write("shared/commands/a.md", "v1");
    s.write("shared/commands/b.md", "b");
    assert_ok(&sync());
    assert!(
        !fs::symlink_metadata(s.home.join(".claude/commands/a.md"))
            .unwrap()
            .is_symlink()
    );
    assert_eq!(s.read(".claude/commands/a.md"), "v1");

    s.write("shared/commands/a.md", "v2");
    assert_ok(&sync());
    assert_eq!(
        s.read(".claude/commands/a.md"),
        "v2",
        "source edits are re-copied"
    );

    fs::write(s.home.join(".claude/commands/a.md"), "edited in place").unwrap();
    s.write("shared/commands/a.md", "v3");
    assert_fails(&sync(), "edited in place");
    assert_eq!(s.read(".claude/commands/a.md"), "edited in place");

    fs::remove_file(s.home.join(".claude/commands/a.md")).unwrap();
    fs::remove_file(s.src.join("shared/commands/b.md")).unwrap();
    assert_ok(&sync());
    assert_eq!(
        s.read(".claude/commands/a.md"),
        "v3",
        "a resolved conflict syncs again"
    );
    assert!(
        !s.home.join(".claude-work/commands/b.md").exists(),
        "removed source files remove their copies"
    );
}

// --- sync: settings.json ---------------------------------------------------

#[test]
fn settings_merge_shared_and_profile_sources() {
    let s = Sandbox::new();
    s.write("shared/settings.json", r#"{"model": "base", "permissions": {"allow": ["Bash(ls)", "Read"], "deny": ["Bash(rm)"]}, "env": {"A": "1"}}"#);
    s.write("profiles/work/settings.json", r#"{"model": "work", "permissions": {"allow": ["Read", "Bash(git status)"]}, "env": {"B": "2"}}"#);
    assert_ok(&s.sync());
    let work: serde_json::Value =
        serde_json::from_str(&s.read(".claude-work/settings.json")).unwrap();
    assert_eq!(
        work,
        serde_json::json!({"model": "work", "permissions": {"allow": ["Bash(ls)", "Read", "Bash(git status)"], "deny": ["Bash(rm)"]}, "env": {"A": "1", "B": "2"}})
    );
    let personal: serde_json::Value =
        serde_json::from_str(&s.read(".claude/settings.json")).unwrap();
    assert_eq!(personal["model"], "base");
    assert!(
        !fs::symlink_metadata(s.home.join(".claude/settings.json"))
            .unwrap()
            .is_symlink()
    );
}

#[test]
fn settings_are_rewritten_when_the_source_changes() {
    let s = Sandbox::new();
    s.write("shared/settings.json", r#"{"model": "a"}"#);
    assert_ok(&s.sync());
    s.write("shared/settings.json", r#"{"model": "b"}"#);
    assert_ok(&s.sync());
    assert!(s.read(".claude/settings.json").contains("\"b\""));
}

#[test]
fn settings_edited_outside_cprof_are_kept() {
    let s = Sandbox::new();
    s.write("shared/settings.json", r#"{"model": "a"}"#);
    assert_ok(&s.sync());
    fs::write(
        s.home.join(".claude/settings.json"),
        r#"{"model": "a", "theme": "dark"}"#,
    )
    .unwrap();
    s.write("shared/settings.json", r#"{"model": "b"}"#);
    assert_fails(&s.sync(), "was changed outside cprof");
    assert_eq!(
        s.read(".claude/settings.json"),
        r#"{"model": "a", "theme": "dark"}"#
    );
}

#[test]
fn settings_that_cprof_never_wrote_are_kept() {
    let s = Sandbox::new();
    s.write("shared/settings.json", r#"{"model": "a"}"#);
    fs::create_dir_all(s.home.join(".claude")).unwrap();
    fs::write(
        s.home.join(".claude/settings.json"),
        "{\"theme\": \"light\"}",
    )
    .unwrap();
    assert_fails(&s.sync(), "was changed outside cprof");
    assert_eq!(s.read(".claude/settings.json"), "{\"theme\": \"light\"}");
}

#[test]
fn settings_sources_must_be_json_objects() {
    let s = Sandbox::new();
    s.write("shared/settings.json", "{\"model\": ");
    assert_fails(&s.sync(), "is not valid JSON");
    s.write("shared/settings.json", "[]");
    assert_fails(&s.sync(), "must contain a JSON object");
    assert!(!s.home.join(".claude/settings.json").exists());
}

#[test]
fn settings_are_removed_when_their_sources_are() {
    let s = Sandbox::new();
    s.write(
        "shared/settings.json",
        r#"{"permissions": {"allow": ["Bash(rm -rf *)"]}}"#,
    );
    assert_ok(&s.sync());
    fs::remove_file(s.src.join("shared/settings.json")).unwrap();
    assert_ok(&s.sync());
    assert!(!s.home.join(".claude/settings.json").exists());
    s.write("shared/settings.json", r#"{"model": "b"}"#);
    assert_ok(&s.sync());
    assert!(s.read(".claude/settings.json").contains("\"b\""));
}

#[test]
fn settings_edited_by_the_user_survive_their_sources_being_removed() {
    let s = Sandbox::new();
    s.write("shared/settings.json", r#"{"model": "a"}"#);
    assert_ok(&s.sync());
    fs::write(
        s.home.join(".claude/settings.json"),
        "{\"theme\": \"dark\"}",
    )
    .unwrap();
    fs::remove_file(s.src.join("shared/settings.json")).unwrap();
    assert_ok(&s.sync());
    assert_eq!(s.read(".claude/settings.json"), "{\"theme\": \"dark\"}");
}

#[cfg(unix)]
#[test]
fn settings_are_never_written_through_a_symlink() {
    let s = Sandbox::new();
    s.write("shared/settings.json", r#"{"model": "a"}"#);
    fs::create_dir_all(s.home.join(".claude")).unwrap();
    std::os::unix::fs::symlink(
        s.root.join("outside/settings.json"),
        s.home.join(".claude/settings.json"),
    )
    .unwrap();
    assert_fails(&s.sync(), "is a symlink");
    assert!(!s.root.join("outside/settings.json").exists());
}

#[cfg(unix)]
#[test]
fn sync_never_takes_over_a_users_own_link_into_the_source_tree() {
    let s = Sandbox::new();
    s.write("notes/CLAUDE.md", "my notes");
    fs::create_dir_all(s.home.join(".claude-work")).unwrap();
    std::os::unix::fs::symlink(
        s.src.join("notes/CLAUDE.md"),
        s.home.join(".claude-work/CLAUDE.md"),
    )
    .unwrap();
    s.write("shared/CLAUDE.md", "shared rules");
    assert_fails(&s.sync(), "symlink not made by cprof");
    assert_eq!(s.read(".claude-work/CLAUDE.md"), "my notes");
}

#[cfg(unix)]
#[test]
fn which_refuses_two_profiles_matching_the_same_directory() {
    let s = Sandbox::new();
    fs::create_dir_all(s.home.join("proj")).unwrap();
    std::os::unix::fs::symlink(s.home.join("proj"), s.home.join("projlink")).unwrap();
    s.config(
        r#"{"profiles": {
        "work": {"config_dir": "~/.claude-work", "paths": ["~/proj"]},
        "alpha": {"config_dir": "~/.claude-alpha", "paths": ["~/projlink"]}}}"#,
    );
    assert_fails(
        &s.run(&["which", s.home.join("proj").to_str().unwrap()]),
        "all match",
    );
}

#[cfg(unix)]
#[test]
fn copies_keep_the_executable_bit() {
    use std::os::unix::fs::PermissionsExt;
    let s = Sandbox::new();
    s.write("shared/hooks/h.sh", "#!/bin/sh\n");
    fs::set_permissions(
        s.src.join("shared/hooks/h.sh"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    assert_ok(&s.cprof(&["sync"]).env("CPROF_COPY", "1").output().unwrap());
    let mode = fs::metadata(s.home.join(".claude/hooks/h.sh"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o111, 0o111, "mode was {mode:o}");
}

#[cfg(unix)]
#[test]
fn a_tie_does_not_hide_a_deeper_match() {
    let s = Sandbox::new();
    fs::create_dir_all(s.home.join("proj/sub")).unwrap();
    std::os::unix::fs::symlink(s.home.join("proj"), s.home.join("projlink")).unwrap();
    s.config(
        r#"{"profiles": {
        "alpha": {"config_dir": "~/.claude-alpha", "paths": ["~/projlink"]},
        "beta": {"config_dir": "~/.claude-beta", "paths": ["~/proj"]},
        "zeta": {"config_dir": "~/.claude-zeta", "paths": ["~/proj/sub"]}}}"#,
    );
    assert_eq!(
        s.which(&s.home.join("proj/sub")),
        s.expect("zeta", ".claude-zeta")
    );
}

#[cfg(unix)]
#[test]
fn copies_follow_later_permission_changes_but_stay_writable() {
    use std::os::unix::fs::PermissionsExt;
    let s = Sandbox::new();
    let sync = || s.cprof(&["sync"]).env("CPROF_COPY", "1").output().unwrap();
    let mode = |rel: &str| fs::metadata(s.home.join(rel)).unwrap().permissions().mode();
    s.write("shared/h.sh", "#!/bin/sh\n");
    s.write("shared/ro.md", "a");
    fs::set_permissions(
        s.src.join("shared/ro.md"),
        fs::Permissions::from_mode(0o444),
    )
    .unwrap();
    assert_ok(&sync());
    assert_eq!(mode(".claude/h.sh") & 0o111, 0);

    fs::set_permissions(s.src.join("shared/h.sh"), fs::Permissions::from_mode(0o755)).unwrap();
    assert_ok(&sync());
    assert_eq!(
        mode(".claude/h.sh") & 0o111,
        0o111,
        "chmod +x reaches the copy"
    );

    fs::set_permissions(
        s.src.join("shared/ro.md"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    s.write("shared/ro.md", "b");
    assert_ok(&sync());
    assert_eq!(
        s.read(".claude/ro.md"),
        "b",
        "a read-only source still leaves an updatable copy"
    );
}
