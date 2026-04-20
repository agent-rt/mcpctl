# cmcp — curl for MCP

A one-shot CLI for [Model Context Protocol](https://modelcontextprotocol.io/) that
reuses your existing Agent configs. Talk to any MCP server — stdio or HTTP — from
the terminal without restarting Claude Code, Claude Desktop, or Cursor.

Built for the *edge case that happens every ten minutes*: you're iterating on an
MCP server and don't want each rebuild to blow away your Agent's prompt cache.

## Install

**Prebuilt binary** (macOS arm64 / macOS x86_64 / Linux x86_64):

```bash
# replace <VERSION> and <TARGET> (e.g. aarch64-apple-darwin)
curl -L https://github.com/<owner>/cmcp/releases/download/v<VERSION>/cmcp-<VERSION>-<TARGET>.tar.gz \
  | tar -xz --strip-components=1 -C /usr/local/bin cmcp-<VERSION>-<TARGET>/cmcp
```

**From source**:

```bash
just install                 # → cargo install --path . --locked --force
# or
cargo install --path .
```

Requires Rust 1.75+.

## Quick start

```bash
# see where cmcp reads config from
cmcp config sources

# list every MCP server discovered across your Agents
cmcp server list

# inspect one server (env / headers are redacted — pass --reveal to unmask)
cmcp server show cortex

# add or remove entries in cmcp's own config (~/.config/cmcp/mcp.json)
cmcp server add my-dev --command node --arg /path/to/server.js --env DEBUG=1
cmcp server add my-api --url https://mcp.example.com/mcp --header 'Authorization: Bearer xxx'
cmcp server remove my-dev
cmcp server edit                     # open $EDITOR on the config file

# list tools exposed by a server
cmcp tool list cortex

# show one tool's JSON schema
cmcp tool show cortex list_nodes

# call a tool — either scheme works (like `curl google.com` vs `curl https://google.com`)
cmcp cortex/list_projects
cmcp mcp://cortex/list_projects

# arguments: --arg key=value (repeatable) or --args-json '{...}'
cmcp github/search_repos --arg query=rust --arg limit=5
cmcp github/search_repos --args-json '{"query":"rust","limit":5}'

# raw JSON output for piping into jq
cmcp mcp://cortex/list_projects --json | jq '.content[0].text'

# forward the server's stderr (useful while developing your own MCP server)
cmcp -v mcp://my-server/my_tool
```

## Config sources

`cmcp` auto-discovers MCP server definitions from any of the following files and
merges them. On a collision the higher-priority source wins:

| Priority | Source         | Path                                                                        | Writable |
|---------:|----------------|-----------------------------------------------------------------------------|:--------:|
| 1        | cmcp           | `$XDG_CONFIG_HOME/cmcp/mcp.json` (or `~/.config/cmcp/mcp.json`)             | ✅       |
| 2        | Claude Code    | `~/.claude.json`                                                            | read-only |
| 3        | Claude Desktop | `~/Library/Application Support/Claude/claude_desktop_config.json` (macOS)   | read-only |
| 4        | Cursor         | `~/.cursor/mcp.json`                                                         | read-only |

Only cmcp's own config is writable — `server add` / `remove` / `edit` edit that
file. Entries defined there win over Agent-provided ones with the same name, so
you can override a Claude-Code server locally without touching Claude's config.

Override the config root for tests or sandboxing:

```bash
CMCP_CONFIG_DIR=/tmp/my-cfg cmcp server list
```

## Transports

| Transport       | Supported | Notes                                               |
|-----------------|:---------:|-----------------------------------------------------|
| stdio (child)   | ✅        | `command` + `args` + `env`                          |
| streamable-http | ✅        | `url` + `headers` (Bearer auth picked up)           |
| OAuth 2.0 flow  | ❌ (planned) |                                                   |
| SSE             | ❌ (out of scope for MVP) |                                      |

## Exit codes

| Code | Meaning                              |
|-----:|--------------------------------------|
| 0    | success                              |
| 1    | generic IO / JSON error              |
| 2    | config or invalid-URI error          |
| 3    | server not found / transport failure |
| 4    | tool not found / tool reported error |
| 5    | timeout                              |

## Development

```bash
just check           # cargo check
just test            # fast, hermetic unit + integration tests
just test-e2e        # live tests against @modelcontextprotocol/server-everything (requires npx)
just lint            # clippy -D warnings
just fmt             # rustfmt
just ci              # fmt-check + lint + test
```

## Roadmap

- Silent daemon / connection pool for sub-100ms repeated calls
- OAuth 2.0 flow for remote servers
- `prompts` / `resources` surfaces
- Windows path discovery
- Config mutation (`cmcp server add` / `remove`)

## License

Apache-2.0
