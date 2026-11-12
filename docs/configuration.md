# profiles.json

`swipe` reads `profiles.json` from its source tree:

| Platform | Default location |
|---|---|
| macOS, Linux | `~/.config/swipe/profiles.json` |
| Windows | `%USERPROFILE%\.config\swipe\profiles.json` |

Set `SWIPE_HOME` to use a different folder, for example a dotfiles repo. If `SWIPE_HOME` isn't set
and `XDG_CONFIG_HOME` is, `swipe` uses `$XDG_CONFIG_HOME/swipe`.

You can edit the file by hand, or with [`swipe edit`](commands.md#swipe-edit).

## Example

```json
{
  "default": "personal",
  "profiles": {
    "personal": { "config_dir": "~/.claude" },
    "work": {
      "config_dir": "~/.claude-work",
      "paths": ["~/code/acme", "~/code/acme-infra"],
      "op_account": "acme"
    },
    "client": {
      "config_dir": "~/.claude-client",
      "paths": ["~/code/acme/client-portal"]
    }
  }
}
```

Here `~/code/acme/api` uses **work**, `~/code/acme/client-portal/web` uses **client** (its path is
more specific), and everything else uses **personal**.

## Fields

### `default`

*Optional.* The profile to use when no profile's `paths` contain the current directory. It must
name a profile defined below. Without it, `claude` refuses to start outside a listed folder.

### `profiles`

*Required.* An object mapping each profile name to its settings.

#### `config_dir`

*Required.* The Claude Code config directory for this profile. It must be absolute or start with
`~`. No two profiles may share one.

!!! warning "Use the same spelling you already log in with"
    On macOS, Claude Code ties the login to the exact `CLAUDE_CONFIG_DIR` value. If you've been
    using `CLAUDE_CONFIG_DIR=$HOME/.claude-work`, write `~/.claude-work`, not a symlinked or
    differently cased path, or Claude Code will ask you to log in again.

#### `paths`

*Optional.* Folders that use this profile, including everything under them. Each must be absolute
or start with `~`.

- The most specific match wins.
- Paths are matched by real location: symlinks are followed, and letter case is ignored on macOS
  and Windows.
- A path that doesn't exist, or can't be read, is skipped.
- Two profiles can't list the same path.

#### `op_account`

*Optional.* The 1Password account that the `op` CLI uses by default inside this profile's Claude
sessions. `swipe run` passes it to `claude` as `OP_ACCOUNT`, so `op` commands don't pick another
account or ask for approval on the wrong one. Use any form `op` accepts: the shorthand, the sign-in
address (`acme.1password.com`), or the account ID. Run `op account list` to see yours.

- The profile's value replaces an `OP_ACCOUNT` already set in your shell.
- Without it, `swipe` leaves `OP_ACCOUNT` as it is, so nothing changes if you don't use 1Password.
- It only sets the default for `op`. It doesn't sign you in, and it doesn't affect 1Password in the
  browser.

## Validation

`swipe` checks the whole file on every run and stops with an error naming the problem: unknown keys
(so a typo like `"path"` can't silently switch accounts), wrong types, relative paths, a shared
`config_dir`, a duplicate path, an empty `op_account`, or a `default` that doesn't exist.

## Environment variables

| Variable | Effect |
|---|---|
| `SWIPE_HOME` | Source tree location (contains `profiles.json`, `shared/`, `profiles/`) |
| `XDG_CONFIG_HOME` | Used as `$XDG_CONFIG_HOME/swipe` when `SWIPE_HOME` isn't set |
| `SWIPE_COPY=1` | `swipe sync` copies files instead of symlinking them |
