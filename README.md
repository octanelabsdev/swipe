<p align="center"><img src="assets/mark.svg" width="72" alt=""></p>
<h1 align="center">swipe</h1>
<p align="center"><b>Run Claude Code with the right account for every directory.</b></p>
<p align="center"><a href="https://swipe.octanelabs.dev">Docs</a> · <a href="https://github.com/octanelabsdev/swipe/releases">Releases</a></p>

Work repos open with your work login. Everything else opens with your personal one. You don't
need an alias. One source tree holds your commands, hooks, agents and settings, so your accounts
stop drifting apart. It works on macOS, Linux and Windows.

## Install

macOS and Linux:

```sh
curl -fsSL https://raw.githubusercontent.com/octanelabsdev/swipe/main/install.sh | sh
```

Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/octanelabsdev/swipe/main/install.ps1 | iex
```

Both scripts download the latest release, check its SHA-256 checksum, and install one binary.
They don't need admin rights. If you have Rust, you can use
`cargo install --git https://github.com/octanelabsdev/swipe` instead.

## Set up

1. Add the shell hook. It makes `claude` run through swipe:

   ```sh
   echo 'eval "$(swipe init zsh)"' >> ~/.zshrc     # bash: swipe init bash, in ~/.bashrc
   ```

   ```powershell
   Add-Content $PROFILE 'swipe init powershell | Out-String | Invoke-Expression'
   ```

2. Tell swipe which folders belong to which account. Run `swipe edit` and press `a` to add a
   profile, or write `~/.config/swipe/profiles.json` yourself:

   ```json
   {
     "default": "personal",
     "profiles": {
       "personal": { "config_dir": "~/.claude" },
       "work": { "config_dir": "~/.claude-work", "paths": ["~/code/acme"] }
     }
   }
   ```

3. Open a new terminal and check it:

   ```sh
   cd ~/code/acme && swipe which      # work	/Users/you/.claude-work
   ```

Now `claude` in `~/code/acme` uses your work account, and `claude` anywhere else uses your
personal one. The first time you use a new config directory, Claude Code asks you to log in.

Use more than one 1Password account? Add `"op_account"` to a profile, and the `op` CLI defaults to
that account inside its Claude sessions. See the
[configuration reference](https://swipe.octanelabs.dev/configuration/#op_account).

## Commands

| Command | What it does |
|---|---|
| `swipe run [args...]` | Runs `claude` with the profile for the current directory. The hook calls this for you. |
| `swipe which [dir]` | Prints the profile, config directory and login email for a directory. |
| `swipe edit` | Opens a full-screen editor to add, change and remove profiles. |
| `swipe sync` | Links your shared and per-profile files into each config directory. |
| `swipe import <profile>` | Copies an existing profile's commands, hooks and settings into the source tree, then syncs. |
| `swipe init <shell>` | Prints the hook for bash, zsh or PowerShell. |

## What it never touches

Your logins, history, projects and anything else Claude Code writes for itself stay where they
are. swipe only manages files you put in its source tree. It never overwrites or deletes a file
it didn't create. Read [How it works](https://swipe.octanelabs.dev/concepts/) for the details.

## License

[MIT](LICENSE)
