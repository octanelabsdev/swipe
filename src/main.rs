mod config;
mod edit;
mod import;
mod profiles_file;
mod sync;

use anyhow::{Context, Result, bail};
use config::Config;
use std::ffi::OsString;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::{Command, ExitCode};
use std::{env, io};

const USAGE: &str = "\
usage: swipe run [args...]        run claude with the profile for the current directory
       swipe which [dir]          print the profile, config dir, 1Password account and login
                                  email for dir (default: current dir)
       swipe edit                 add, change and remove profiles in a full-screen editor
       swipe sync                 link shared and per-profile files into each config dir
       swipe import <profile>     copy a profile's existing commands, hooks and settings
                                  into the source tree, then sync them back
       swipe init <shell>         print the hook that makes `claude` run through swipe
                                  (bash, zsh or powershell)
       swipe --version            print the version";

const POSIX_HOOK: &str =
    "unalias claude 2>/dev/null || true\nfunction claude { command swipe run \"$@\"; }\n";
const POWERSHELL_HOOK: &str =
    "Remove-Item Alias:claude -ErrorAction SilentlyContinue\nfunction claude { swipe run @args }\n";

fn main() -> ExitCode {
    match run_command(env::args_os().skip(1).collect()) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("swipe: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run_command(args: Vec<OsString>) -> Result<ExitCode> {
    let command = args.first().and_then(|a| a.to_str());
    let rest = args.get(1..).unwrap_or_default();
    match command {
        Some("run") => run_claude(rest),
        Some("which") => {
            let dir = match rest {
                [] => env::current_dir()?,
                [dir] => PathBuf::from(dir),
                _ => return usage_error(),
            };
            let config = Config::load()?;
            let profile = config.profile_for(&dir)?;
            let mut fields = vec![
                profile.name.clone(),
                profile.config_dir.display().to_string(),
                profile.op_account.clone().unwrap_or_default(),
                config.login_email(profile).unwrap_or_default(),
            ];
            // Fields keep their positions for `cut`; only empty ones at the end are dropped.
            while fields.last().is_some_and(String::is_empty) {
                fields.pop();
            }
            println!("{}", fields.join("\t"));
            Ok(ExitCode::SUCCESS)
        }
        Some("edit") => {
            if !rest.is_empty() {
                return usage_error();
            }
            Ok(if edit::run()? {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        Some("sync") => {
            if !rest.is_empty() {
                return usage_error();
            }
            let clean = sync::sync(&Config::load()?, None);
            Ok(if clean {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        Some("import") => {
            let [name] = rest else {
                return usage_error();
            };
            let name = name.to_str().context("profile names are UTF-8")?;
            let clean = import::import(&Config::load()?, name)?;
            Ok(if clean {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        Some("init") => {
            match rest.first().and_then(|s| s.to_str()) {
                Some("bash" | "zsh") => print!("{POSIX_HOOK}"),
                Some("powershell" | "pwsh") => print!("{POWERSHELL_HOOK}"),
                _ => return usage_error(),
            }
            Ok(ExitCode::SUCCESS)
        }
        Some("version" | "-V" | "--version") => {
            println!("swipe {}", env!("CARGO_PKG_VERSION"));
            Ok(ExitCode::SUCCESS)
        }
        Some("help" | "-h" | "--help") => {
            println!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        _ => usage_error(),
    }
}

fn usage_error() -> Result<ExitCode> {
    eprintln!("{USAGE}");
    Ok(ExitCode::from(2))
}

fn run_claude(args: &[OsString]) -> Result<ExitCode> {
    let config = Config::load()?;
    let profile = config.profile_for(&env::current_dir()?)?;
    let claude = find_on_path("claude").context("could not find `claude` on your PATH")?;

    if io::stderr().is_terminal() {
        eprintln!("swipe: {} ({})", profile.name, profile.config_dir.display());
    }
    let mut command = Command::new(claude);
    command.args(args);
    if config.is_default_dir(&profile.config_dir) {
        command.env_remove("CLAUDE_CONFIG_DIR");
    } else {
        command.env("CLAUDE_CONFIG_DIR", &profile.config_dir);
    }
    // Without one, an OP_ACCOUNT the user exported still reaches claude.
    if let Some(account) = &profile.op_account {
        command.env("OP_ACCOUNT", account);
    }
    launch(command)
}

/// Replaces this process with claude, so signals and the terminal behave exactly as if
/// claude had been started directly.
#[cfg(unix)]
fn launch(mut command: Command) -> Result<ExitCode> {
    use std::os::unix::process::CommandExt;
    Err(command.exec()).context("could not start claude")
}

/// Windows can't replace a process, so wait for claude instead. Ctrl+C reaches claude directly
/// through the shared console; ignoring it here keeps swipe from exiting while claude still runs.
#[cfg(windows)]
fn launch(mut command: Command) -> Result<ExitCode> {
    ctrlc::set_handler(|| {})?;
    let status = command.status().context("could not start claude")?;
    // Windows exit codes are 32-bit; ExitCode::from(u8) would turn 256 into success.
    std::process::exit(status.code().unwrap_or(1))
}

/// Looks `name` up on PATH. On Windows this also tries PATHEXT, so an npm-installed
/// `claude.cmd` is found as well as the native `claude.exe`.
fn find_on_path(name: &str) -> Result<PathBuf> {
    let extensions: Vec<String> = if cfg!(windows) {
        env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".into())
            .split(';')
            .map(str::to_owned)
            .collect()
    } else {
        vec![String::new()]
    };
    let path = env::var_os("PATH").unwrap_or_default();
    for dir in env::split_paths(&path) {
        for ext in &extensions {
            let candidate = dir.join(format!("{name}{ext}"));
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    bail!("not found")
}
