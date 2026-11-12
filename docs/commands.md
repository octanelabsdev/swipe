# Commands

| Command | What it does |
|---|---|
| `swipe run [args…]` | Starts `claude` with the profile for the current directory, passing `args` through |
| `swipe which [dir]` | Prints the profile and config directory for `dir` (default: the current directory) |
| `swipe sync` | Builds every profile's config directory from the source tree |
| `swipe import <profile>` | Copies a profile's existing files into the source tree, then syncs them back |
| `swipe init <shell>` | Prints the shell hook: `bash`, `zsh` or `powershell` |
| `swipe help` | Prints usage |
| `swipe --version` | Prints the version |

Every command exits with status 0 on success, 1 on an error or conflict, and 2 for a usage error.
`swipe run` exits with `claude`'s own status.

## `swipe run`

```sh
swipe run --resume
```

The shell hook makes `claude` an alias for `swipe run`, so you normally never type it. It resolves
the profile, prints `swipe: <profile> (<config dir>)` to the terminal, and starts the first
`claude` on your `PATH` (on Windows, also `claude.cmd` and other `PATHEXT` extensions). If the
profile can't be resolved, `claude` isn't started.

## `swipe which`

```sh
$ swipe which ~/code/acme/api
work	/Users/you/.claude-work
```

Prints the profile name and config directory, separated by a tab. If the profile sets
[`op_account`](configuration.md#op_account), it follows as a third field. Useful in scripts:

```sh
CLAUDE_CONFIG_DIR=$(swipe which | cut -f2)
```

!!! warning
    For a profile whose `config_dir` is `~/.claude`, leave `CLAUDE_CONFIG_DIR` unset instead. On
    macOS, setting it explicitly points Claude Code at a different login.

## `swipe sync`

Links every file in `shared/` and `profiles/<name>/` into each profile's config directory, merges
`settings.json`, and removes what it placed that's no longer in the source. It reports conflicts
instead of overwriting anything it didn't create. See [Sharing files between accounts](syncing.md).

## `swipe import <profile>`

Copies `CLAUDE.md`, `settings.json`, `keybindings.json`, `agents/`, `commands/`, `hooks/`,
`output-styles/` and `skills/` from the profile's config directory into `profiles/<profile>/`, then
syncs that profile. Nothing is deleted. Links to folders, git repos, `skills/synced/`,
`skills/.trash/` and `.venv`, `node_modules` and `__pycache__` folders are left in place. See
[Moving an existing setup](migrating.md).

## `swipe init <shell>`

Prints the hook to add to your shell's startup file:

=== "bash / zsh"

    ```sh
    unalias claude 2>/dev/null || true
    function claude { command swipe run "$@"; }
    ```

=== "PowerShell"

    ```powershell
    Remove-Item Alias:claude -ErrorAction SilentlyContinue
    function claude { swipe run @args }
    ```
