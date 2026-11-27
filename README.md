# televisor.sh

IPTV from the terminal: browse Xtream Codes panels and play live TV, movies (VOD), and series in your media player — as a CLI, or as an MCP server for AI agents.

## Install

```bash
git clone https://github.com/lgalabru/televisor && cd televisor
just install     # builds and installs `televisor` to ~/.cargo/bin
televisor setup  # interactive first run: account + MCP install
```

## Requirements

- A media player — [VLC](https://www.videolan.org) by default (or `VLC_PATH` to its binary). mpv, IINA, and ffplay work too; see [Player](#player).
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
televisor play fox sports 1          # fuzzy-resolve and open in your player
televisor play 429586 --fullscreen   # by stream id

televisor guide fox sports 1         # EPG: what's on now/next (--full for the day)
televisor play bbc one --at "2026-07-15 20:00" --duration 90   # catch-up replay
televisor channels --archive         # channels with replay (shown as [replay 3d])

televisor movies dune                # search VOD
televisor movies --category comedy  # list every movie in a matching VOD category
televisor movies dune --category 4k # search only within a matching VOD category
televisor movies --recent            # latest additions
televisor info dune                  # plot, rating, cast, duration, trailer
televisor play --movie dune          # play a movie
televisor series breaking bad        # search shows (--recent for latest)
televisor info --series 48173        # show details
televisor episodes 48173             # list a show's episodes
televisor play --series 48173 -s 1 -e 2   # play S01E02

televisor favorites add fox sports 1 # ★ favorites (add/remove/list)
televisor history                    # recently played
televisor play --last                # replay the most recent item

televisor open <url-or-file>         # raw player passthrough (any URL/file)
```

Ambiguous `play` queries print ranked candidates and exit with code 2 — pass a numeric id to disambiguate. Try `--format ts` if a live channel won't play as m3u8. Live TV, movies, and series each have their own `--category` lists. Catch-up `--at` times are interpreted in the panel's timezone (`televisor status` shows it).

### Plain M3U providers

If your provider only hands out a playlist URL (no working API), add it with `--m3u`:

```bash
televisor account add mylist --m3u --host "http://host/get.php?username=u&password=p&type=m3u_plus"
```

Live TV (search, categories, play, favorites) works from the parsed playlist; VOD, series, EPG, catch-up, and status need a full Xtream login and say so.

## MCP server (Claude / Codex)

`televisor mcp` serves MCP on stdio with twelve tools: `list_accounts`, `get_status`, `list_categories`, `search_channels` (with archive flags), `play_channel` (live or catch-up via `at`), `get_epg`, `search_movies`, `get_movie_info`, `play_movie`, `search_series`, `get_series_episodes`, `play_episode`. `search_movies` accepts a VOD category id/name keyword; use an empty query plus `all: true` to return that category's complete movie list, or `offset`/`max_results` to page through it. Playback launches your player locally — stream URLs (which embed panel credentials) are never returned to the model, and account credentials can only be managed via the CLI.

`televisor setup` installs the server into `~/.claude.json` (Claude Code), Claude Desktop, and `~/.codex/config.toml` (Codex) — idempotent, additive, and pointing at the current binary. Manual config, if you prefer:

```json
{ "mcpServers": { "televisor": { "command": "televisor", "args": ["mcp"] } } }
```

This repo also ships a `.mcp.json` so Claude Code picks it up automatically inside the repo (via `cargo run`).

## Player

VLC is the default but not required. Pick another with `--player` on `open`/`play`, the `TELEVISOR_PLAYER` env var, or `player = "…"` in config:

```bash
televisor play fox sports 1 --player mpv
televisor play --movie dune --player iina        # macOS
TELEVISOR_PLAYER='mpv --force-media-title={title} {url}' televisor play bbc one
```

Built-in presets: `vlc`, `mpv`, `iina`, `ffplay`. Any other value is a command template — `{url}` and `{title}` are substituted (no shell, so credentials never hit one), and a bare command gets the URL appended. `--player system` hands the URL to the OS default handler (`open`/`xdg-open`/`start`) — convenient if you've associated a player, but it can't set the window title (so a credentialed URL may show) and for `http` streams often opens a browser, so it's opt-in.

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
├── core/   # Xtream client, config, catalog cache, player launcher, redaction (televisor-core)
├── cli/    # Clap frontend, one module per command (televisor binary)
└── mcp/    # MCP server, thin dispatch over core (televisor-mcp)
```

Credential hygiene is enforced by construction: passwords are `Redacted` (masked `Debug`/`Display`), URLs pass through `redact_url` before any log line, reqwest errors are stripped of their URL, and the server's echo of the password in `user_info` is never deserialized.

## License

MIT
