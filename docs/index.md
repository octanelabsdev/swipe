# swipe

**Run Claude Code with the right account for every directory.** Work repos open with your work
login, everything else with your personal one, and no alias to remember. One source tree holds
your commands, hooks, agents and settings, so the two accounts stop drifting apart.

```sh
curl -fsSL https://raw.githubusercontent.com/octanelabsdev/swipe/main/install.sh | sh
echo 'eval "$(swipe init zsh)"' >> ~/.zshrc
```

Then tell it which folders belong to which account. Run `swipe edit`, or write
`~/.config/swipe/profiles.json` yourself:

```json
{
  "default": "personal",
  "profiles": {
    "personal": { "config_dir": "~/.claude" },
    "work": { "config_dir": "~/.claude-work", "paths": ["~/code/acme"] }
  }
}
```

Now `claude` in `~/code/acme` uses your work account, and `claude` anywhere else uses your
personal one. Full setup, including Windows and PowerShell, is in [Install](install.md).

## Why this exists

Claude Code keeps everything for an account in one config directory: the login, settings, history,
commands, hooks. Claude Code's docs recommend giving each account its own directory and pointing
`CLAUDE_CONFIG_DIR` at it. In practice that means:

- an alias like `claude-work` you have to remember, and forget, and end up on the wrong account;
- two copies of every command and hook, which quietly drift apart as you fix one and not the other;
- nothing that works the same way on a teammate's Windows laptop.

`swipe` fixes all three:

| Problem | What swipe does |
|---|---|
| Remembering which alias to use | `claude` picks the account from the folder you're in |
| Copies drifting apart | Files live once in a source tree, linked into each account's directory |
| Different machines and shells | One binary for macOS, Linux and Windows; hooks for bash, zsh and PowerShell |

## What it never touches

Your logins, history, projects and anything else Claude Code writes for itself stay exactly where
they are. `swipe` only manages files you put in its source tree, and it never overwrites or deletes
a file it didn't create. See [How it works](concepts.md).

## Where to go next

<div class="grid cards" markdown>

-   **[Install](install.md)**

    The binary, the shell hook, and your first `profiles.json`.

-   **[Moving an existing setup](migrating.md)**

    Already have `~/.claude` and `~/.claude-work`? Import them without losing anything.

-   **[Sharing files between accounts](syncing.md)**

    One copy of each command, hook and setting, with per-account overrides.

-   **[Troubleshooting & FAQ](troubleshooting.md)**

    Wrong account, conflicts, Windows symlinks, IDE extensions.

</div>
