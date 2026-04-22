use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "mcpctl",
    version,
    about = "the MCP control utility for AI agents",
    long_about = "mcpctl lets agents (and humans) discover and invoke MCP tools from any\n\
                  configured server — stdio or HTTP — without restarting Claude Code,\n\
                  Claude Desktop, or Cursor.\n\n\
                  Invoke a tool directly (either form works):\n\
                  \x20 mcpctl mcp://<server>/<tool> --arg k=v\n\
                  \x20 mcpctl <server>/<tool>        --arg k=v\n"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Cmd>,

    /// Direct mcp://server/tool URI invocation (positional shortcut).
    #[arg(value_name = "URI")]
    pub uri: Option<String>,

    #[command(flatten)]
    pub call: CallArgs,

    /// Forward child-process stderr to the terminal (stdio MCP servers).
    #[arg(short = 'v', long = "verbose", global = true)]
    pub verbose: bool,

    /// Override the stdio `command` for this invocation (stdio servers only).
    /// Useful for testing a local rebuild without editing config.
    #[arg(long = "override-cmd", value_name = "PATH", global = true)]
    pub override_cmd: Option<String>,

    /// Output all results as JSON (machine-readable for agents).
    /// Errors are also emitted as {"error":"...","code":N} on stderr.
    #[arg(long, global = true)]
    pub json: bool,
}

#[derive(Debug, Subcommand)]
pub enum Cmd {
    /// Manage & inspect configured MCP servers.
    Server {
        #[command(subcommand)]
        cmd: ServerCmd,
    },
    /// List / show tools exposed by an MCP server.
    Tool {
        #[command(subcommand)]
        cmd: ToolCmd,
    },
    /// Inspect discovered config sources.
    Config {
        #[command(subcommand)]
        cmd: ConfigCmd,
    },
    /// Call a tool via explicit subcommand (equivalent to the positional form).
    Call {
        /// mcp://<server>/<tool>
        uri: String,

        #[command(flatten)]
        call: CallArgs,
    },
    /// Subscribe to a stdio MCP server and print each server-push notification
    /// (progress, log, resource/tool/prompt list_changed, resource_updated,
    /// cancelled, custom) as they arrive. Long-running; stop with Ctrl-C.
    Tail {
        server: String,
        /// Keep only notifications whose kind matches. Repeatable. Kinds:
        /// progress, log, resource_updated, resource_list_changed,
        /// tool_list_changed, prompt_list_changed, cancelled, custom.
        #[arg(long = "filter", value_name = "KIND")]
        filters: Vec<String>,
        /// Stop after N notifications (0 = unlimited).
        #[arg(long, default_value_t = 0)]
        count: u64,
        /// Handshake timeout (seconds). The tail itself has no duration limit.
        #[arg(long, default_value_t = 15)]
        timeout: u64,
    },
    /// List or get prompts exposed by an MCP server.
    Prompt {
        #[command(subcommand)]
        cmd: PromptCmd,
    },
    /// List or read resources exposed by an MCP server.
    Resource {
        #[command(subcommand)]
        cmd: ResourceCmd,
    },
    /// Single-session dump of server info + tools + schemas. Avoids three
    /// separate handshakes that `check` + `tool list` + N×`tool show` would cost.
    Introspect {
        server: String,
        /// Compact `name(args)` signatures instead of full schemas.
        #[arg(long)]
        signature: bool,
        /// Truncate each description to N chars (TSV mode). 0 disables.
        #[arg(long = "desc-chars", default_value_t = 80)]
        desc_chars: usize,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
}

#[derive(Debug, Subcommand)]
pub enum ServerCmd {
    /// List all discovered MCP servers.
    List {
        /// Parallel probe each server (handshake + list_tools). Adds a
        /// `status` column (ok | error | timeout) + an optional error tail.
        #[arg(long)]
        probe: bool,
        /// Per-probe timeout (seconds) when --probe is set.
        #[arg(long, default_value_t = 10)]
        probe_timeout: u64,
    },
    /// Show one server's resolved config.
    Show {
        name: String,
        /// Reveal redacted env / header values.
        #[arg(long)]
        reveal: bool,
    },
    /// Add or overwrite a server in mcpctl's own config (`~/.config/mcpctl/mcp.json`).
    Add(ServerAddArgs),
    /// Remove a server from mcpctl's own config. Read-only sources cannot be edited.
    Remove { name: String },
    /// Open mcpctl's own config in `$EDITOR`, creating an empty skeleton if missing.
    Edit,
    /// Probe a server end-to-end: handshake, tools/list, ping.
    Check {
        name: String,
        #[arg(long, default_value_t = 15)]
        timeout: u64,
    },
}

#[derive(Debug, Args)]
pub struct ServerAddArgs {
    /// Server name (the part after `mcp://` in invocations).
    pub name: String,

    /// stdio: executable to launch.
    #[arg(long, group = "transport")]
    pub command: Option<String>,
    /// stdio: repeatable argument to pass to the command.
    #[arg(long = "arg", value_name = "VALUE", requires = "command")]
    pub args: Vec<String>,
    /// stdio: repeatable `KEY=VALUE` environment variable.
    #[arg(long = "env", value_name = "KEY=VALUE", requires = "command")]
    pub env: Vec<String>,

    /// http: URL of the MCP streamable-http endpoint.
    #[arg(long, group = "transport")]
    pub url: Option<String>,
    /// http: repeatable `Name: Value` header.
    #[arg(long = "header", value_name = "NAME=VALUE", requires = "url")]
    pub headers: Vec<String>,

    /// Overwrite an existing entry in mcpctl's own config.
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Subcommand)]
pub enum ToolCmd {
    /// List tools exposed by a server.
    List {
        server: String,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
        /// Truncate each description to N chars (TSV mode). 0 disables.
        #[arg(long = "desc-chars", default_value_t = 80)]
        desc_chars: usize,
    },
    /// Show a tool's schema / description.
    ///
    /// Accepts either two positionals (`tool show <server> <tool>`) or a single
    /// URI (`tool show synap/query` or `tool show mcp://synap/query`), matching
    /// the call-side syntax.
    Show {
        /// `<server>` or `<server>/<tool>` or `mcp://<server>/<tool>`.
        target: String,
        /// When `target` is just `<server>`, the tool name goes here.
        tool: Option<String>,
        /// Render as a compact signature `name(a: type, b?: type=default)`
        /// instead of pretty-printed JSON Schema.
        #[arg(long)]
        signature: bool,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
}

#[derive(Debug, Subcommand)]
pub enum PromptCmd {
    /// List prompts exposed by a server.
    List {
        server: String,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Get a prompt's rendered messages. Arguments passed as --arg key=value.
    Get {
        /// `<server>/<prompt>` or two positionals `<server> <prompt>`.
        server: String,
        prompt: String,
        /// Prompt arguments as key=value pairs.
        #[arg(long = "arg", value_name = "KEY=VALUE")]
        args: Vec<String>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
}

#[derive(Debug, Subcommand)]
pub enum ResourceCmd {
    /// List resources exposed by a server.
    List {
        server: String,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Read a resource by URI.
    Read {
        server: String,
        uri: String,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigCmd {
    /// Print discovered config source paths & whether they exist.
    Sources,
}

#[derive(Debug, Args, Default, Clone)]
pub struct CallArgs {
    /// --arg key=value (repeatable). Values are parsed as JSON when possible, else string.
    #[arg(long = "arg", value_name = "KEY=VALUE")]
    pub args: Vec<String>,

    /// JSON object used as base arguments; individual --arg entries override matching keys.
    ///
    /// Three forms, to spare you shell-quoting hell:
    ///   --args-json '{"k":1}'   — inline
    ///   --args-json @file.json  — read from file
    ///   --args-json -           — read from stdin
    #[arg(long = "args-json", value_name = "JSON|@FILE|-")]
    pub args_json: Option<String>,

    /// Emit only `structured_content` (if the tool returned any), else fall
    /// back to the text content. Most machine-readable signal for agents.
    #[arg(long)]
    pub structured: bool,

    /// Per-call timeout in seconds.
    #[arg(long, default_value_t = 30)]
    pub timeout: u64,

    /// Retry handshake up to N extra times on transport errors (not on
    /// timeouts). Useful when a server has slow cold-start.
    #[arg(long, default_value_t = 0)]
    pub retry: u32,
}
