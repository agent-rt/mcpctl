use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "cmcp",
    version,
    about = "curl for MCP — one-shot CLI for Model Context Protocol tools",
    long_about = "cmcp reuses MCP server configs from Claude Code, Claude Desktop and Cursor\n\
                  so you can invoke tools from the terminal without restarting your Agent.\n\n\
                  Invoke a tool directly (either form works):\n\
                  \x20 cmcp mcp://<server>/<tool> --arg k=v\n\
                  \x20 cmcp <server>/<tool>        --arg k=v\n"
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
}

#[derive(Debug, Subcommand)]
pub enum ServerCmd {
    /// List all discovered MCP servers.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Show one server's resolved config.
    Show {
        name: String,
        /// Reveal redacted env / header values.
        #[arg(long)]
        reveal: bool,
        #[arg(long)]
        json: bool,
    },
    /// Add or overwrite a server in cmcp's own config (`~/.config/cmcp/mcp.json`).
    Add(ServerAddArgs),
    /// Remove a server from cmcp's own config. Read-only sources cannot be edited.
    Remove { name: String },
    /// Open cmcp's own config in `$EDITOR`, creating an empty skeleton if missing.
    Edit,
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

    /// Overwrite an existing entry in cmcp's own config.
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Subcommand)]
pub enum ToolCmd {
    /// List tools exposed by a server.
    List {
        server: String,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Show a tool's schema / description.
    Show {
        server: String,
        tool: String,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigCmd {
    /// Print discovered config source paths & whether they exist.
    Sources {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Args, Default, Clone)]
pub struct CallArgs {
    /// --arg key=value (repeatable). Values are parsed as JSON when possible, else string.
    #[arg(long = "arg", value_name = "KEY=VALUE")]
    pub args: Vec<String>,

    /// JSON object used as base arguments; individual --arg entries override matching keys.
    #[arg(long = "args-json", value_name = "JSON")]
    pub args_json: Option<String>,

    /// Output raw CallToolResult JSON.
    #[arg(long)]
    pub json: bool,

    /// Per-call timeout in seconds.
    #[arg(long, default_value_t = 30)]
    pub timeout: u64,
}
