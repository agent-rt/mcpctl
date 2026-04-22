# mcpctl — the MCP control utility for AI agents

`mcpctl` gives AI agents a CLI interface to **discover, inspect, and invoke** any MCP server that is already configured in their environment — without restarts, without extra daemons, without extra config.

```
Agent (Claude Code / Cursor / any shell tool user)
  └── mcpctl <server>/<tool> --args-json '{"q":"..."}'
        └── MCP Server (stdio or HTTP)
```

## Why agent-first?

`mcpctl` is infrastructure for agents, not a developer GUI:

- **Discovery**: `mcpctl server list` lets an agent enumerate all available MCP servers at runtime
- **Introspection**: `mcpctl introspect <server> --json` returns tools + prompts + resources in one round-trip, giving the agent a full capability map before deciding what to call
- **Invocation**: `mcpctl <server>/<tool> --args-json '...'` invokes any tool with a single shell command — no SDK, no handshake boilerplate
- **Machine-readable output**: `--json` on any command returns structured JSON; errors go to stderr as `{"error":"...","code":N}` so agents can parse and branch without string matching
- **Config reuse**: reads existing Claude Code, Claude Desktop, and Cursor configs — zero extra setup for the agent

## Install

**Prebuilt binary** (macOS arm64 / macOS x86_64 / Linux x86_64):

```bash
# replace <VERSION> and <TARGET> (e.g. aarch64-apple-darwin)
curl -L https://github.com/<owner>/mcpctl/releases/download/v<VERSION>/mcpctl-<VERSION>-<TARGET>.tar.gz \
  | tar -xz --strip-components=1 -C /usr/local/bin mcpctl-<VERSION>-<TARGET>/mcpctl
```

**From source**:

```bash
cargo install --path .
```

Requires Rust 1.75+.

## Quick start (agent perspective)

```bash
# 1. Discover all configured MCP servers
mcpctl server list --json

# 2. Get full capability map for a server in one shot
mcpctl introspect github --json

# 3. Call a tool
mcpctl github/search_repos --args-json '{"query":"mcp","language":"rust"}'

# 4. Call with --json for machine-readable output + JSON errors
mcpctl --json github/search_repos --args-json '{"query":"mcp"}'

# 5. List prompts and resources
mcpctl prompt list <server> --json
mcpctl resource list <server> --json
```

## Command reference

### Tool invocation

```bash
mcpctl <server>/<tool> [--arg key=value ...] [--args-json '{...}'|@file|-]
mcpctl mcp://<server>/<tool> ...          # explicit URI form
mcpctl call mcp://<server>/<tool> ...     # subcommand form
```

Options:
- `--json` — output raw `CallToolResult` JSON
- `--structured` — output only `structured_content` (most machine-readable)
- `--timeout N` — per-call timeout in seconds (default: 30)
- `--retry N` — retry handshake on transport errors (default: 0)

### Server commands

```bash
mcpctl server list                  # list all discovered servers (TSV / JSON)
mcpctl server list --probe          # parallel health check (handshake + list_tools)
mcpctl server show <name>           # resolved config with redacted secrets
mcpctl server check <name>          # per-stage diagnostic with timing
mcpctl server add <name> --command <cmd> [--arg ...] [--env KEY=VAL ...]
mcpctl server add <name> --url <url> [--header Name=Value ...]
mcpctl server remove <name>
mcpctl server edit                  # open ~/.config/mcpctl/mcp.json in $EDITOR
```

### Tool commands

```bash
mcpctl tool list <server>           # list tools (TSV / JSON)
mcpctl tool show <server>/<tool>    # full JSON schema
mcpctl tool show <server>/<tool> --signature  # compact signature
```

### Prompt commands

```bash
mcpctl prompt list <server>         # list prompts (TSV / JSON)
mcpctl prompt get <server> <prompt> [--arg key=value ...]
```

### Resource commands

```bash
mcpctl resource list <server>       # list resources (TSV / JSON)
mcpctl resource read <server> <uri> # read resource content
```

### Introspect (single-session full dump)

```bash
mcpctl introspect <server> --json   # tools + prompts + resources in one handshake
mcpctl introspect <server> --signature  # compact tool signatures
```

### Tail (real-time notifications)

```bash
mcpctl tail <server>                        # stream all server-push notifications
mcpctl tail <server> --filter progress      # filter by kind
mcpctl tail <server> --count 10 --json      # stop after 10, emit ndjson
```

### Config

```bash
mcpctl config sources               # show discovered config paths
```

## Global flags

| Flag | Effect |
|------|--------|
| `--json` | All output as JSON; errors as `{"error":"...","code":N}` on stderr |
| `--verbose` / `-v` | Forward server stderr to terminal |
| `--override-cmd <path>` | Test a local binary without editing config |

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | IO / JSON error |
| 2 | Config or URI error |
| 3 | Server not found / transport failure |
| 4 | Tool not found / tool returned `is_error=true` |
| 5 | Timeout |

## Config sources (priority order)

1. `~/.config/mcpctl/mcp.json` — mcpctl's own writable config (highest priority)
2. `~/.claude.json` — Claude Code
3. `~/Library/Application Support/Claude/claude_desktop_config.json` — Claude Desktop (macOS)
4. `~/.cursor/mcp.json` — Cursor

On first run, if `~/.config/cmcp/mcp.json` exists (old name), it is silently migrated to `~/.config/mcpctl/mcp.json`.

## Comparison with mcpd

[mcpd](https://github.com/mozilla-ai/mcpd) wraps MCP servers as a long-running HTTP daemon — useful for production deployments where downstream services need a stable HTTP endpoint.

`mcpctl` takes the opposite approach: **no daemon, no port, stateless**. Each invocation spawns, handshakes, calls, and exits. This is the right trade-off for agent tool use: agents call tools infrequently enough that per-call cold-start (~100–500ms) is acceptable, and the absence of a daemon to manage is a reliability advantage.

The two tools are complementary: `mcpctl` can call servers exposed by `mcpd` via `--url`.
