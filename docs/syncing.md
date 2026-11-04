# Sharing files between accounts

`swipe sync` builds each profile's config directory from one source tree, so a command or hook you
fix once is fixed for every account.

## Layout

```text
~/.config/swipe/
├── profiles.json
├── shared/                  # every profile gets these
│   ├── CLAUDE.md
│   ├── commands/review.md
│   ├── hooks/notify.sh
│   └── settings.json
└── profiles/
    ├── personal/            # only the personal profile
    │   └── commands/journal.md
    └── work/                # only the work profile
        ├── CLAUDE.md        # replaces shared/CLAUDE.md for work
        └── settings.json    # merged over shared/settings.json for work
```

Run `swipe sync` after adding, removing or renaming files. Edits to a file that's already synced
show up straight away, because the config directory links to the source.

```text
$ swipe sync
personal (/Users/you/.claude)
  link /Users/you/.claude/commands/review.md -> /Users/you/.config/swipe/shared/commands/review.md
work (/Users/you/.claude-work)
  link /Users/you/.claude-work/CLAUDE.md -> /Users/you/.config/swipe/profiles/work/CLAUDE.md
  write /Users/you/.claude-work/settings.json
```

A sync with nothing to do prints only the profile names. Running it twice in a row is always safe.

## The rules

**Files.** Every file under `shared/` goes into every profile at the same relative path. A file
at the same path under `profiles/<name>/` replaces it for that profile. On macOS and Windows,
paths that differ only in letter case count as the same path.

**Links or copies.** Files are symlinked to the source. Where symlinks aren't allowed, `swipe`
copies them instead (see [Windows](#windows)). Copies keep the source's executable bit, so hook
scripts still run.

**Symlinks in the source** are followed, so you can point `shared/skills` at a folder in another
repo and its files are synced like any other.

**Ignored names:** `.DS_Store`, and the `.swipe-manifest.json` file `swipe` keeps in each config
directory.

**Removing.** When a file leaves the source, the next sync removes the link or copy it made, and any
folder that's left empty.

## settings.json

`settings.json` is merged instead of linked, because each profile usually needs a little of its
own. `swipe` merges `profiles/<name>/settings.json` over `shared/settings.json`:

- objects are merged key by key;
- lists are combined, keeping the shared items first and adding the profile's new ones;
- anything else from the profile replaces the shared value.

```json title="shared/settings.json"
{ "model": "opus", "permissions": { "allow": ["Read", "Bash(ls:*)"] } }
```

```json title="profiles/work/settings.json"
{ "model": "sonnet", "permissions": { "allow": ["Bash(git status)"] } }
```

```json title="~/.claude-work/settings.json (written by swipe)"
{ "model": "sonnet", "permissions": { "allow": ["Read", "Bash(ls:*)", "Bash(git status)"] } }
```

The result is written as a real file. When both sources are removed, `swipe` removes the
`settings.json` it wrote.

!!! warning "Edits made directly to settings.json"
    If anything edits the config directory's `settings.json` directly (you, Claude Code, or a tool
    that installs plugins), the next `swipe sync` notices and stops with a conflict for that file
    instead of overwriting it. Copy what you want to keep into your source `settings.json`, delete
    the config directory's `settings.json`, and sync again.

## Conflicts

`swipe` never overwrites or deletes something it didn't create. The one exception is a file that is
byte-for-byte identical to its source (or a `settings.json` with the same JSON content): replacing
it with a link loses nothing, so `swipe` does. Even then it moves the original aside first, and
only deletes it once the link works; if anything fails, the original goes back. When something is
in the way, it
reports a conflict for that file, syncs everything else, and exits with status 1:

```text
swipe: /Users/you/.claude/hooks/notify.sh: already exists or was edited in place; move what you
want to keep into /Users/you/.config/swipe, delete it, then re-run
```

| Message | Meaning | Fix |
|---|---|---|
| `already exists or was edited in place` | A file is there that `swipe` didn't write, or a copy was edited | Move it into the source tree (or delete it), then sync |
| `is a symlink not made by swipe` | Your own symlink is in the way | Remove it, or move its target into the source tree |
| `is inside …, which is a symlink` | A folder in the config directory is a symlink; writing into it would change files elsewhere | Replace the symlinked folder with a real one, or stop managing that folder |
| `is a directory` | A folder sits where a file should go | Move or delete the folder |
| `was changed outside swipe` | `settings.json` was edited, by you or by Claude Code | Fold the change into the source and delete the file |
| `leads back to this file` | The source file is a link to the very file in the config directory | Replace the link in the source tree with the real file |
| `….swipe-replacing is in the way` | A sync was interrupted while it replaced a file; this is the original | Check it, keep what you need, delete it, then sync |

## Windows

Creating symlinks on Windows needs Developer Mode (Settings → System → For developers) or an
administrator shell. Without either, `swipe` copies files instead and says so once:

```text
  symlinks are not allowed here, so swipe will copy files instead
```

Copies behave like links except that edits to the source only arrive on the next `swipe sync`.
`swipe` remembers what it copied, so it updates its copies but never overwrites one you edited in
place. Set `SWIPE_COPY=1` to get copies on any platform.

## Stopping { #stopping }

To stop syncing a profile, delete `.swipe-manifest.json` from its config directory. Its links
still point into the source tree, so either keep the source tree or replace the links with real
files first:

```sh
# macOS and Linux: turn every link into the source tree into a real copy
SRC="$HOME/.config/swipe"
find ~/.claude -type l -lname "$SRC/*" -exec sh -c \
  'target=$(readlink "$1") && rm "$1" && cp -p "$target" "$1"' _ {} \;
rm ~/.claude/.swipe-manifest.json
```

Repeat for each config directory. On Windows without Developer Mode the files are already copies,
so deleting `.swipe-manifest.json` is enough.
