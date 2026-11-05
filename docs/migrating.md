# Moving an existing setup

You already have a `~/.claude` and maybe a `~/.claude-work`, each with its own commands, hooks and
settings, and an alias to switch between them. This guide moves both into `swipe` without logging
out or losing anything.

## 1. Back up

`swipe import` checks everything before it changes anything, but these are your working configs.
Take a copy first:

=== "macOS / Linux"

    ```sh
    tar -czf ~/claude-config-backup.tgz -C ~ .claude .claude-work
    ```

=== "Windows (PowerShell)"

    ```powershell
    Compress-Archive -Path $HOME\.claude, $HOME\.claude-work -DestinationPath $HOME\claude-config-backup.zip
    ```

## 2. Describe your profiles

Install `swipe` and write `profiles.json` as in [Install](install.md). Use the **same config
directories you use today**, spelled the way your alias spells them. On macOS the login is tied to
that exact path:

```json
{
  "default": "personal",
  "profiles": {
    "personal": { "config_dir": "~/.claude" },
    "work": { "config_dir": "~/.claude-work", "paths": ["~/code/acme"] }
  }
}
```

At this point `claude` already picks the right account. Steps 3 and 4 are only needed if you want
`swipe` to manage your files as well.

## 3. Import each profile

```sh
swipe import personal
swipe import work
```

For each profile, `import` takes the files you wrote yourself:

`CLAUDE.md` · `settings.json` · `keybindings.json` · `agents/` · `commands/` · `hooks/` ·
`output-styles/` · `skills/`

It copies them into `profiles/<name>/` in the source tree, then syncs: each original that is
identical to its new copy is swapped for a link to it, so Claude Code sees exactly what it saw
before. Everything else in the config directory stays where it is: the login, history, projects,
plugins and caches.

```text
$ swipe import work
import /Users/you/.claude-work/CLAUDE.md
import /Users/you/.claude-work/commands
import /Users/you/.claude-work/settings.json
work (/Users/you/.claude-work)
  link /Users/you/.claude-work/CLAUDE.md -> /Users/you/.config/swipe/profiles/work/CLAUDE.md
  …
```

**What makes it safe:**

- **Nothing is deleted.** An original is only ever replaced by a link to an identical copy. If the
  sync stops partway, for any reason, every file you haven't seen replaced is still there.
- Before copying, it checks that your `settings.json` is valid JSON and a real file (not a
  symlink), and that nothing it would create in the source tree already exists. A broken symlink or unreadable file stops the copy.
  In every one of these cases nothing is changed at all.
- **Links to files** stay links to the same file. **Links to folders**, such as a skill linked
  from another repo, are left exactly where they are, so they stay live. The import lists them as
  "left in place".
- **Git repos** (any folder with a `.git` inside) are left in place too, so git keeps working.
- **Folders Claude Code fills in itself** stay where they are: `skills/synced/` (skills from
  claude.ai, which differ between accounts) and `skills/.trash/`. So do folders a tool rebuilds on
  each machine: `.venv`, `node_modules` and `__pycache__`.
- A link from one of your files to another one in the same config directory is pointed at the
  imported copy. A link to something that isn't imported, such as `history.jsonl`, is left in
  place.
- It refuses a profile that `swipe` has already placed files in.

!!! note "When `shared/settings.json` already exists"
    If you import a profile after creating `shared/settings.json`, the merged result differs from
    the profile's current `settings.json`, so the sync reports a conflict and keeps the current
    file. Your settings are already safe in `profiles/<name>/settings.json`: delete the config
    directory's `settings.json` and run `swipe sync` to write the merged version. Importing every
    profile before creating `shared/settings.json` avoids this.

## 4. Share what's identical

After importing, both profiles have their own copy of everything. Move what's the same into
`shared/`. This lists the files that are byte-for-byte identical in both:

```sh
cd ~/.config/swipe/profiles/personal
find . -type f -exec sh -c 'cmp -s "$1" "../work/$1" && echo "$1"' _ {} \;
cd ..
```

For each one, move a copy to `shared/`, delete both profile copies, then sync:

```sh
mkdir -p ../shared/commands
mv personal/commands/review.md ../shared/commands/review.md
rm work/commands/review.md
swipe sync
```

Files that differ stay in each profile. That's the right home for anything tied to one account,
such as a hook that writes to a work-only folder.

## 5. Remove your old aliases

Delete `alias claude-work=...` and similar lines from your shell startup file and open a new
terminal. Check both accounts:

```sh
cd ~/code/acme && claude    # swipe: work (...)
cd ~ && claude              # swipe: personal (...)
```

## 6. Keep the source tree somewhere safe

The source tree is now where your commands and settings live. Putting it under version control is
a good idea:

```sh
cd ~/.config/swipe && git init && git add . && git commit -m "Claude Code profiles"
```

Before committing, check that no file contains secrets (API keys in `settings.json` `env`, for
example).

## Undoing it

Restore the backup from step 1:

```sh
rm -rf ~/.claude-work && tar -xzf ~/claude-config-backup.tgz -C ~ .claude-work
```

Or keep the files and stop syncing, as described in
[Sharing files between accounts](syncing.md#stopping).
