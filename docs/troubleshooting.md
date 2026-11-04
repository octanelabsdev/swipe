# Troubleshooting & FAQ

## Claude opened with the wrong account

Run `swipe which` in the same folder. If it names the wrong profile:

- Check `paths` in `profiles.json`. The most specific matching path wins; a folder outside every
  path uses `default`.
- `swipe` matches the folder's **real** location. If you reached it through a symlink, the link's
  target is what's matched.

If `swipe which` is right but Claude still uses the wrong account, the hook isn't active:

```sh
type claude      # bash/zsh: should say "claude is a shell function"
```

```powershell
Get-Command claude   # PowerShell: CommandType should be Function
```

If it isn't, check that the `swipe init` line is in your startup file and open a new terminal.

## Claude asks me to log in again

Claude Code ties each login to its config directory, and on macOS to the directory's exact
spelling. Make `config_dir` match what you used before: `~/.claude-work`, not a symlinked or
differently cased path. For the `~/.claude` profile, `swipe` leaves `CLAUDE_CONFIG_DIR` unset, which
is what Claude Code expects for its default.

Claude Code's docs note one exception: Claude Console sign-ins made **without** an API key are stored
outside the config directory, so separate directories don't keep two of them apart.

## The VS Code / JetBrains extension, or the desktop app, uses the wrong account

Those start Claude Code themselves and don't go through your shell, so the hook never runs. Set
`CLAUDE_CONFIG_DIR` in the editor's environment for that workspace if the editor supports it, or
launch the editor from a terminal where you've set it:

```sh
CLAUDE_CONFIG_DIR=$(swipe which | cut -f2) code .
```

For the `~/.claude` profile, launch without setting it.

## "conflict" or "problem(s)" after `swipe sync`

Something `swipe` didn't create is where it wants to put a file. Each line names the file and what
to do. See the [conflict table](syncing.md#conflicts). Everything else was still synced.

## "broken symlink in source"

A symlink in your source tree points at something that doesn't exist. `swipe` stops syncing that
profile rather than guess. Fix or remove the link and sync again.

## "profiles … all match"

Two profiles list paths that are the same folder on disk, often one through a symlink. Remove one,
or make one more specific.

## Windows: files are copies, not links

Without Developer Mode, Windows doesn't let normal users create symlinks, so `swipe` copies files.
Turn on Developer Mode (Settings → System → For developers) and the next sync uses links for new
files. Files that `swipe` already copied stay copies until they leave the source tree.

## cmd.exe

cmd.exe has no shell functions, so there's no hook for it. Use PowerShell, Windows Terminal or Git
Bash, or run `swipe run` directly.

## Can I keep the source tree in my dotfiles repo?

Yes. Set `SWIPE_HOME` to the folder in your shell startup file, before the `swipe init` line:

```sh
export SWIPE_HOME="$HOME/dotfiles/swipe"
```

## Does swipe send anything anywhere?

No. It reads `profiles.json` and your source tree, and writes only inside the config directories it
manages and, for `import`, the source tree.
