use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use rmcp::model::{
    CancelledNotificationParam, CustomNotification, ElicitationResponseNotificationParam,
    LoggingMessageNotificationParam, ProgressNotificationParam, ResourceUpdatedNotificationParam,
};
use rmcp::service::NotificationContext;
use rmcp::{ClientHandler, RoleClient, ServiceExt};
use tokio::sync::mpsc;

use crate::config::{apply_override, load_all, McpTransport, ServerId};
use crate::error::{CmcpError, Result};
use crate::output::{print_tail_event, TailEvent, TailKind};
use crate::session;

/// Notification fan-in: each `on_*` callback forwards to this channel so the
/// main loop can print on its own schedule without blocking the rmcp handler.
type TailTx = mpsc::UnboundedSender<TailEvent>;

struct TailHandler {
    info: rmcp::model::ClientInfo,
    tx: TailTx,
}

impl ClientHandler for TailHandler {
    fn get_info(&self) -> rmcp::model::ClientInfo {
        self.info.clone()
    }

    fn on_progress(
        &self,
        params: ProgressNotificationParam,
        _ctx: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        let payload = serde_json::to_value(&params).unwrap_or_default();
        let _ = self.tx.send(TailEvent {
            kind: TailKind::Progress,
            payload,
        });
        std::future::ready(())
    }

    fn on_logging_message(
        &self,
        params: LoggingMessageNotificationParam,
        _ctx: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        let payload = serde_json::to_value(&params).unwrap_or_default();
        let _ = self.tx.send(TailEvent {
            kind: TailKind::Log,
            payload,
        });
        std::future::ready(())
    }

    fn on_resource_updated(
        &self,
        params: ResourceUpdatedNotificationParam,
        _ctx: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        let payload = serde_json::to_value(&params).unwrap_or_default();
        let _ = self.tx.send(TailEvent {
            kind: TailKind::ResourceUpdated,
            payload,
        });
        std::future::ready(())
    }

    fn on_resource_list_changed(
        &self,
        _ctx: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        let _ = self.tx.send(TailEvent {
            kind: TailKind::ResourceListChanged,
            payload: serde_json::Value::Null,
        });
        std::future::ready(())
    }

    fn on_tool_list_changed(
        &self,
        _ctx: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        let _ = self.tx.send(TailEvent {
            kind: TailKind::ToolListChanged,
            payload: serde_json::Value::Null,
        });
        std::future::ready(())
    }

    fn on_prompt_list_changed(
        &self,
        _ctx: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        let _ = self.tx.send(TailEvent {
            kind: TailKind::PromptListChanged,
            payload: serde_json::Value::Null,
        });
        std::future::ready(())
    }

    fn on_cancelled(
        &self,
        params: CancelledNotificationParam,
        _ctx: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        let payload = serde_json::to_value(&params).unwrap_or_default();
        let _ = self.tx.send(TailEvent {
            kind: TailKind::Cancelled,
            payload,
        });
        std::future::ready(())
    }

    fn on_url_elicitation_notification_complete(
        &self,
        params: ElicitationResponseNotificationParam,
        _ctx: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        // Rare enough that we lump this under `custom` with an explicit method.
        let payload = serde_json::json!({
            "method": "notifications/elicitation/complete",
            "params": params,
        });
        let _ = self.tx.send(TailEvent {
            kind: TailKind::Custom,
            payload,
        });
        std::future::ready(())
    }

    fn on_custom_notification(
        &self,
        notification: CustomNotification,
        _ctx: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        let payload = serde_json::to_value(&notification).unwrap_or_default();
        let _ = self.tx.send(TailEvent {
            kind: TailKind::Custom,
            payload,
        });
        std::future::ready(())
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn run(
    server: &str,
    filters: Vec<String>,
    count: u64,
    timeout_secs: u64,
    json: bool,
    verbose: bool,
    override_cmd: Option<&str>,
) -> Result<()> {
    let filter_set = parse_filters(&filters)?;

    let servers = load_all()?;
    let id = ServerId(server.to_string());
    let cfg = servers
        .get(&id)
        .map(|r| &r.active)
        .ok_or_else(|| CmcpError::ServerNotFound(id.clone()))?;
    let cfg = apply_override(cfg, override_cmd)?;

    // MVP: stdio only. http streamable pushes notifications the same way at
    // the rmcp layer, but plumbing a different transport through here needs
    // a generic `connect_http_with_handler`; deferred until we have a real
    // use case.
    let (command, args, env) = match &cfg.transport {
        McpTransport::Stdio { command, args, env } => (command, args, env),
        McpTransport::Http { .. } => {
            return Err(CmcpError::InvalidArg {
                arg: server.into(),
                reason: "tail only supports stdio servers for now".into(),
            });
        }
    };

    let spawned = session::spawn_stdio_child(command, args, env, verbose)?;
    let pgid = spawned.pgid;
    let stderr_ring = Arc::clone(&spawned.stderr_ring);
    let child = spawned.child;

    let (tx, mut rx) = mpsc::unbounded_channel::<TailEvent>();
    let handler = TailHandler {
        info: session::client_info(),
        tx,
    };

    // Handshake with the same budget contract as `connect`.
    let handshake = handler.serve(child);
    let running = match tokio::time::timeout(Duration::from_secs(timeout_secs), handshake).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            session::kill_process_group_now(pgid);
            tokio::time::sleep(Duration::from_millis(150)).await;
            let tail = drain_tail(&stderr_ring);
            let hint = "hint: rerun with -v/--verbose to stream server stderr";
            let msg = if tail.is_empty() {
                format!("stdio handshake failed: {e}\n{hint}")
            } else {
                format!("stdio handshake failed: {e}\n--- last stderr lines ---\n{tail}\n{hint}")
            };
            return Err(CmcpError::Transport(msg));
        }
        Err(_) => {
            session::kill_process_group_now(pgid);
            tokio::time::sleep(Duration::from_millis(150)).await;
            let tail = drain_tail(&stderr_ring);
            return Err(CmcpError::HandshakeTimeout {
                secs: timeout_secs,
                tail: if tail.is_empty() { None } else { Some(tail) },
            });
        }
    };

    if verbose {
        eprintln!("[mcpctl] tailing '{server}' — stop with Ctrl-C");
    }

    let mut seen: u64 = 0;
    let mut ctrl_c = Box::pin(tokio::signal::ctrl_c());

    loop {
        tokio::select! {
            // Ctrl-C: graceful shutdown.
            _ = &mut ctrl_c => {
                if verbose {
                    eprintln!("[mcpctl] SIGINT received, closing session");
                }
                break;
            }
            maybe_event = rx.recv() => {
                let Some(event) = maybe_event else {
                    // Handler dropped the sender — should not happen while
                    // running holds it. Treat as EOF.
                    break;
                };
                if !filter_set.is_empty() && !filter_set.contains(&event.kind) {
                    continue;
                }
                print_tail_event(&event, json);
                seen += 1;
                if count > 0 && seen >= count {
                    break;
                }
            }
        }
    }

    let _ = running.cancel().await;
    session::kill_process_group(pgid).await;
    Ok(())
}

fn parse_filters(filters: &[String]) -> Result<HashSet<TailKind>> {
    let mut out = HashSet::new();
    for f in filters {
        let kind = TailKind::from_cli_label(f).ok_or_else(|| CmcpError::InvalidArg {
            arg: format!("--filter {f}"),
            reason: "unknown kind; expected one of: progress, log, resource_updated, \
                     resource_list_changed, tool_list_changed, prompt_list_changed, \
                     cancelled, custom"
                .into(),
        })?;
        out.insert(kind);
    }
    Ok(out)
}

fn drain_tail(ring: &Arc<std::sync::Mutex<std::collections::VecDeque<String>>>) -> String {
    ring.lock()
        .map(|g| g.iter().cloned().collect::<Vec<_>>().join("\n"))
        .unwrap_or_default()
}
