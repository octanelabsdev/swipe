# Install

Setup has three steps: install the `swipe` binary, add the shell hook, and write a `profiles.json`.

## 1. Install the binary

`swipe` is a single binary with no dependencies.

=== "macOS and Linux"

    ```sh
    curl -fsSL https://raw.githubusercontent.com/octanelabsdev/swipe/main/install.sh | sh
    ```

    This installs `swipe` in `~/.local/bin`. If that folder is not on your `PATH`, the script
    tells you the line to add.

=== "Windows"

    ```powershell
    irm https://raw.githubusercontent.com/octanelabsdev/swipe/main/install.ps1 | iex
    ```

    This installs `swipe.exe` in `%LOCALAPPDATA%\Programs\swipe` and adds that folder to your
    user `PATH`.

=== "With Rust"

    ```sh
    cargo install --git https://github.com/octanelabsdev/swipe
    ```

    This needs Rust 1.88 or newer, and puts `swipe` in `~/.cargo/bin`.

The scripts download the latest release from
[GitHub](https://github.com/octanelabsdev/swipe/releases) and check its SHA-256 checksum before
they install anything. They don't need admin rights. To install a specific release, set
`SWIPE_VERSION` (for example `SWIPE_VERSION=v0.1.0`). To use a different folder, set
`SWIPE_INSTALL_DIR`.

Check that it works:

```sh
swipe --version
```

## 2. Add the shell hook

The hook defines a `claude` command that runs through `swipe`. `swipe` then starts the real
`claude` with the right account. Add one line to your shell's startup file:

=== "zsh"

    ```sh
    echo 'eval "$(swipe init zsh)"' >> ~/.zshrc
    ```

=== "bash"

    ```sh
    echo 'eval "$(swipe init bash)"' >> ~/.bashrc
    ```

=== "PowerShell"

    ```powershell
    Add-Content $PROFILE 'swipe init powershell | Out-String | Invoke-Expression'
    ```

Open a new terminal afterwards. If you had an alias such as `alias claude-work=...` or
`alias claude=...`, you can delete it. The hook removes any existing `claude` alias so it can't
shadow the hook.

!!! note "Git Bash on Windows"
    Git Bash uses the bash hook. cmd.exe has no shell functions, so it isn't supported; use
    PowerShell or Windows Terminal instead.

## 3. Set up your profiles

Run the editor:

```sh
swipe edit
```

Press `a` to add a profile. Give it a name and a config directory, then press `p` to add the
folders that use it. Press `w` to save and `q` to quit. See [`swipe edit`](commands.md#swipe-edit)
for every key.

Or write `~/.config/swipe/profiles.json` yourself:

```json
{
  "default": "personal",
  "profiles": {
    "personal": { "config_dir": "~/.claude" },
    "work": { "config_dir": "~/.claude-work", "paths": ["~/code/acme"] }
  }
}
```

- **`config_dir`** is the Claude Code config directory for that account. `~/.claude` is Claude
  Code's own default.
- **`paths`** are the folders, and everything under them, that use this profile.
- **`default`** is used everywhere else.

Every field is described in [profiles.json](configuration.md).

## 4. Check it

```sh
cd ~/code/acme && swipe which
# work	/Users/you/.claude-work

cd ~ && swipe which
# personal	/Users/you/.claude
```

Then run `claude` as usual. A one-line banner names the profile it picked:

```text
swipe: work (/Users/you/.claude-work)
```

The first time you use a new config directory, Claude Code asks you to log in to that account.
After that, each directory keeps its own login.

## Updating and uninstalling

To update, run the install command again. It replaces the binary with the latest release.

To uninstall, delete the binary: `~/.local/bin/swipe`, or `%LOCALAPPDATA%\Programs\swipe` on
Windows. If you installed with Rust, run `cargo uninstall swipe`.

To uninstall, also remove the hook line from your shell startup file. Your config directories are
left exactly as they are; if you used `swipe sync`, see
[Sharing files between accounts](syncing.md#stopping) first.
