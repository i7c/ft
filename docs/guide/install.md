# Install

`ft` is a single statically-linked Rust binary. No daemon, no database,
no language runtime to install. The minimum supported Rust version is
pinned in `rust-toolchain.toml`; if you use `rustup`, it'll honor the
toolchain file automatically.

## Build from source

```sh
git clone <repo-url> ft && cd ft
cargo install --path ft
```

This drops `ft` into `~/.cargo/bin/` (or whatever your
`CARGO_INSTALL_ROOT` points at). Make sure that directory is on your
`PATH`.

Verify the install:

```sh
ft --version
ft --help
```

## Shell completions

Generate completions for your shell. The exact destination depends on
your distro and shell config:

```sh
# Bash
ft completions bash > ~/.local/share/bash-completion/completions/ft

# Zsh — drop into any directory on $fpath that you control
ft completions zsh  > "${fpath[1]}/_ft"

# Fish
ft completions fish > ~/.config/fish/completions/ft.fish
```

Re-source your shell config (or open a fresh shell) to pick the
completions up. The completion script is regenerated from the clap
definition, so it stays in sync with whatever `ft --help` shows.

## Man pages

```sh
mkdir -p ~/.local/share/man/man1
ft man --out ~/.local/share/man/man1
```

Then `man ft`, `man ft-tasks`, `man ft-notes`, etc. work the way you'd
expect. The output reflects the current binary, not a packaged
snapshot, so it tracks the version you have installed.

## First run

`ft vault` is the introspection command. It tells you where `ft`
thinks your vault is, which config files it loaded, and what the
merged configuration looks like:

```sh
cd ~/my-vault
ft vault
```

A working install with a discoverable vault prints something like:

```
Vault: /home/you/my-vault

Config files (lowest → highest precedence):
  [1] /home/you/.config/ft/config.toml (user): present
  [2] /home/you/my-vault/.ft/config.toml (vault): not found

Merged config:
  default_vault = "~/my-vault"
  …
```

If you see an error instead, the next chapter walks through vault
discovery and the first config block to write:
[vault-and-config.md](vault-and-config.md).

## Updating

Re-run `cargo install --path ft` from the updated source tree;
`--force` is implied for an existing install. After any update, run
`ft commands docs --check` against this repo's `docs/keybindings.md`
if you want to verify that the shipped reference matches your binary's
command registry (and regenerate if not — see
[tui.md](tui.md#commands-and-keymaps)).
