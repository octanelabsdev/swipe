# How it works

## One config directory per account

Claude Code stores each account's login, settings and history in a config directory. The default is
`~/.claude`; setting the `CLAUDE_CONFIG_DIR` environment variable points Claude Code at a different
one. Claude Code's [authentication docs](https://code.claude.com/docs/en/iam#log-in-with-multiple-accounts)
recommend exactly this for keeping a work and a personal account signed in side by side.

Where the login is kept depends on the platform:

| Platform | Login stored in |
|---|---|
| macOS | The Keychain, in an entry tied to the config directory's path |
| Linux | `.credentials.json` inside the config directory |
| Windows | `.credentials.json` inside the config directory |

`swipe` never reads or moves any of this. It only decides which directory Claude Code gets.

## Picking a profile

When you run `claude`, the shell hook runs `swipe run`, which:

1. resolves the current directory to its real path, following symlinks and using the folder's real
   letter case;
2. finds every profile with a `paths` entry containing it, and picks the most specific one;
3. falls back to `default` if nothing matches;
4. starts the real `claude` with `CLAUDE_CONFIG_DIR` set to that profile's `config_dir`.

!!! info "The `~/.claude` profile"
    For a profile whose `config_dir` is `~/.claude`, `swipe` **unsets** `CLAUDE_CONFIG_DIR` instead
    of setting it. On macOS, the login is tied to whether the variable is set and to its exact
    value. Setting it to `~/.claude` explicitly would make Claude Code look for a different login,
    and you'd appear logged out.

If two profiles match equally well, for example because one lists a folder and another lists a
symlink to the same folder, `swipe` refuses to guess and reports both. If the configuration is
invalid or no profile matches and there's no default, `claude` isn't started at all, so you never
end up on the wrong account by accident.

On macOS and Linux, `swipe run` replaces itself with `claude`, so signals, exit codes and the
terminal behave exactly as if you'd started `claude` directly. On Windows it starts `claude`, waits
for it, and passes its exit code through.

## The source tree (optional)

Choosing the account is all most people need. If you also want your commands, hooks and settings
kept in one place, `swipe sync` builds each config directory from a source tree:

```text
~/.config/swipe/
├── profiles.json
├── shared/                 # goes into every profile
│   ├── commands/review.md
│   ├── hooks/notify.sh
│   └── settings.json
└── profiles/
    └── work/               # only for the work profile; wins over shared/
        ├── CLAUDE.md
        └── settings.json
```

Details in [Sharing files between accounts](syncing.md).

## What swipe will never do

- Read, copy or move your logins, history, projects, or anything else Claude Code writes for
  itself.
- Overwrite or delete a file it didn't create. It reports a conflict instead and leaves the file
  alone.
- Write or delete through a symlinked folder inside a config directory.
