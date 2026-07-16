# televisor.sh

IPTV from the terminal: browse Xtream Codes panels and play channels in VLC — as a CLI, or as an MCP server for AI agents.

## Install

```bash
git clone https://github.com/lgalabru/televisor && cd televisor
just install     # builds and installs `televisor` to ~/.cargo/bin
televisor setup  # interactive first run: account + MCP install
```

## Requirements

- [VLC](https://www.videolan.org) installed (or `VLC_PATH` pointing at the binary)
- [Just](https://github.com/casey/just) as the task runner

## Quick start

```bash
# Interactive first-run: prompts for your panel account (pasting the full
# playlist URL your provider sent also works — embedded credentials are
# extracted), verifies it against the panel, and installs the MCP server
# into Claude Code, Claude Desktop, and Codex.
televisor setup

# Or non-interactively:
televisor account add home --host http://host:port --username u   # password prompted
televisor setup --update             # (re-)install the MCP configs only

televisor status                     # subscription state, expiry, connections
televisor categories                 # live categories
televisor channels fox sports        # search 37k+ channels (local cache, ~ms)
televisor play fox sports 1          # fuzzy-resolve and open in VLC
televisor play 429586 --fullscreen   # by stream id
televisor open <url-or-file>         # raw VLC passthrough
```

Ambiguous `play` queries print ranked candidates and exit with code 2 — pass a stream id to disambiguate. Try `--format ts` if a channel won't play as m3u8.

## MCP server (Claude / Codex)

`televisor mcp` serves MCP on stdio with five tools: `list_accounts`, `get_status`, `list_categories`, `search_channels`, `play_channel`. Playback launches VLC locally — stream URLs (which embed panel credentials) are never returned to the model, and account credentials can only be managed via the CLI.

`televisor setup` installs the server into `~/.claude.json` (Claude Code), Claude Desktop, and `~/.codex/config.toml` (Codex) — idempotent, additive, and pointing at the current binary. Manual config, if you prefer:

```json
{ "mcpServers": { "televisor": { "command": "televisor", "args": ["mcp"] } } }
```

This repo also ships a `.mcp.json` so Claude Code picks it up automatically inside the repo (via `cargo run`).

## Configuration

Accounts live in `~/.config/televisor/config.toml` (created `0600`). Multiple accounts are supported; the first added becomes the default (`televisor account set-default` to change, `--account` to override per command).

For ephemeral/CI use, `TELEVISOR_HOST` + `TELEVISOR_USERNAME` + `TELEVISOR_PASSWORD` synthesize an account that takes precedence over the config file; `TELEVISOR_ACCOUNT` selects a configured account by name.

The channel catalog is cached per account under `~/.cache/televisor/` for 24 h — pass `--refresh` to refetch.

## Development

```bash
just setup          # build the workspace
just install        # install `televisor` to ~/.cargo/bin and repoint MCP configs at it
just fmt            # check formatting
just lint           # clippy, warnings as errors
just test           # unit + mock-server tests
just test-network   # ignored smoke tests against a real panel (TELEVISOR_TEST_* via Doppler)
```

## Workspace layout

```
crates/
├── core/   # Xtream client, config, catalog cache, VLC launcher, redaction (televisor-core)
├── cli/    # Clap frontend, one module per command (televisor binary)
└── mcp/    # MCP server, thin dispatch over core (televisor-mcp)
```

Credential hygiene is enforced by construction: passwords are `Redacted` (masked `Debug`/`Display`), URLs pass through `redact_url` before any log line, reqwest errors are stripped of their URL, and the server's echo of the password in `user_info` is never deserialized.

## License

MIT
