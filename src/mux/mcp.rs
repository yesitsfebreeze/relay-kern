//! MCP server for the mux PTY multiplexer.
//!
//! Exposes six tools (mux_spawn, mux_send, mux_list, mux_status,
//! mux_delegate, mux_collect) that agents running inside panes can call to
//! manage sibling panes and delegate tasks via kern.
//! Only active in mux mode — never registered when running `--daemon`.
//!
//! Transport: TCP loopback, one thread per accepted connection (see run_mux).

use std::sync::{Arc, Mutex};

use serde::Deserialize;
use trnsprt::{McpError, McpServer, ToolResult, ToolSchema};

use crate::mux::registry::PaneRegistry;

pub struct MuxMcpServer {
    pub registry: Arc<Mutex<PaneRegistry>>,
    pub agent_cmd: String,
    /// Address of the running kern daemon MCP server, e.g. `"127.0.0.1:7778"`.
    /// Used by `mux_delegate` and `mux_collect` to ingest/query tasks.
    pub kern_mcp_addr: String,
}

// ── Tool schemas ──────────────────────────────────────────────────────────────

pub fn tool_schemas() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "mux_spawn",
            "description": "Spawn a new agent sub-pane in the mux TUI.",
            "inputSchema": {
                "type": "object",
                "required": ["label"],
                "properties": {
                    "label": { "type": "string", "description": "Human label for the new pane (e.g. 'worker-1')" },
                    "cmd":   { "type": "string", "description": "Command to run (defaults to configured agent_cmd)" },
                },
            },
        }),
        serde_json::json!({
            "name": "mux_send",
            "description": "Write text to a pane's PTY stdin.",
            "inputSchema": {
                "type": "object",
                "required": ["session_id", "text"],
                "properties": {
                    "session_id": { "type": "string", "description": "Session id from mux_spawn or mux_list" },
                    "text":       { "type": "string", "description": "Text to write to the pane's PTY stdin" },
                },
            },
        }),
        serde_json::json!({
            "name": "mux_list",
            "description": "List all active panes.",
            "inputSchema": {
                "type": "object",
                "properties": {},
            },
        }),
        serde_json::json!({
            "name": "mux_status",
            "description": "Get the current visible screen content of a pane.",
            "inputSchema": {
                "type": "object",
                "required": ["session_id"],
                "properties": {
                    "session_id": { "type": "string", "description": "Session id of the pane to inspect" },
                },
            },
        }),
        serde_json::json!({
            "name": "mux_delegate",
            "description": "Store a task description in kern and spawn a fresh worker pane that boots by querying kern for its assignment. Returns session_id, task_key, and result_key.",
            "inputSchema": {
                "type": "object",
                "required": ["label", "task"],
                "properties": {
                    "label": {
                        "type": "string",
                        "description": "Human label for the new pane (e.g. 'worker-1')"
                    },
                    "task": {
                        "type": "string",
                        "description": "Full task description stored in kern under mux:task:<session_id>"
                    },
                    "cmd": {
                        "type": "string",
                        "description": "Command to run in the pane (defaults to configured agent_cmd)"
                    },
                },
            },
        }),
        serde_json::json!({
            "name": "mux_collect",
            "description": "Query kern for the result a worker published under mux:result:<session_id>. Returns empty string if not yet published.",
            "inputSchema": {
                "type": "object",
                "required": ["session_id"],
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "Session id returned by mux_delegate"
                    },
                },
            },
        }),
    ]
}

// ── McpServer impl ────────────────────────────────────────────────────────────

impl McpServer for MuxMcpServer {
    fn server_name(&self)    -> &str { "kern-mux" }
    fn server_version(&self) -> &str { env!("CARGO_PKG_VERSION") }

    fn tools_list(&self) -> Vec<ToolSchema> {
        tool_schemas()
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect()
    }

    fn call_tool(&self, name: &str, args: &serde_json::Value) -> Result<ToolResult, McpError> {
        let result = match name {
            "mux_spawn"    => self.tool_spawn(args),
            "mux_send"     => self.tool_send(args),
            "mux_list"     => self.tool_list(),
            "mux_status"   => self.tool_status(args),
            "mux_delegate" => self.tool_delegate(args),
            "mux_collect"  => self.tool_collect(args),
            _              => tool_error(&format!("unknown mux tool: {name}")),
        };
        Ok(value_to_tool_result(result))
    }
}

// ── Tool handlers ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SpawnArgs { label: String, cmd: Option<String> }

#[derive(Deserialize)]
struct SendArgs { session_id: String, text: String }

#[derive(Deserialize)]
struct IdArgs { session_id: String }

#[derive(Deserialize)]
struct DelegateArgs {
    label: String,
    task:  String,
    cmd:   Option<String>,
}

#[derive(Deserialize)]
struct CollectArgs {
    session_id: String,
}

impl MuxMcpServer {
    fn tool_spawn(&self, args: &serde_json::Value) -> serde_json::Value {
        let p: SpawnArgs = match serde_json::from_value(args.clone()) {
            Ok(v) => v,
            Err(e) => return tool_error(&format!("invalid args: {e}")),
        };
        let cmd = p.cmd.unwrap_or_else(|| self.agent_cmd.clone());
        let mut reg = match self.registry.lock() {
            Ok(g) => g,
            Err(_) => return tool_error("registry lock poisoned"),
        };
        let cols = reg.cols;
        let rows = reg.rows;
        match reg.spawn_pane(p.label, cmd, cols, rows) {
            Ok(id)  => tool_ok(serde_json::json!({ "session_id": id })),
            Err(e)  => tool_error(&format!("spawn failed: {e}")),
        }
    }

    fn tool_send(&self, args: &serde_json::Value) -> serde_json::Value {
        let p: SendArgs = match serde_json::from_value(args.clone()) {
            Ok(v) => v,
            Err(e) => return tool_error(&format!("invalid args: {e}")),
        };
        let mut reg = match self.registry.lock() {
            Ok(g) => g,
            Err(_) => return tool_error("registry lock poisoned"),
        };
        if reg.send_to(&p.session_id, &p.text) {
            tool_ok(serde_json::json!({}))
        } else {
            tool_error(&format!("no pane with id: {}", p.session_id))
        }
    }

    fn tool_list(&self) -> serde_json::Value {
        let reg = match self.registry.lock() {
            Ok(g) => g,
            Err(_) => return tool_error("registry lock poisoned"),
        };
        let panes: Vec<serde_json::Value> = reg.panes.iter().map(|p| {
            serde_json::json!({
                "session_id": p.id,
                "label":      p.label,
                "exited":     p.exited,
            })
        }).collect();
        tool_ok(serde_json::json!(panes))
    }

    fn tool_status(&self, args: &serde_json::Value) -> serde_json::Value {
        let p: IdArgs = match serde_json::from_value(args.clone()) {
            Ok(v) => v,
            Err(e) => return tool_error(&format!("invalid args: {e}")),
        };
        let reg = match self.registry.lock() {
            Ok(g) => g,
            Err(_) => return tool_error("registry lock poisoned"),
        };
        let Some(pane) = reg.find(&p.session_id) else {
            return tool_error(&format!("no pane with id: {}", p.session_id));
        };
        let screen = pane.parser.screen();
        let (rows, cols) = screen.size();
        let (cursor_row, cursor_col) = screen.cursor_position();
        let screen_text = pane.screen_text();
        tool_ok(serde_json::json!({
            "session_id":  pane.id,
            "label":       pane.label,
            "exited":      pane.exited,
            "cols":        cols,
            "rows":        rows,
            "cursor_row":  cursor_row,
            "cursor_col":  cursor_col,
            "screen_text": screen_text,
        }))
    }

    fn tool_delegate(&self, args: &serde_json::Value) -> serde_json::Value {
        let p: DelegateArgs = match serde_json::from_value(args.clone()) {
            Ok(v)  => v,
            Err(e) => return tool_error(&format!("invalid args: {e}")),
        };
        let cmd = p.cmd.unwrap_or_else(|| self.agent_cmd.clone());

        // Spawn the pane first so we have a session_id to key on.
        let id = {
            let mut reg = match self.registry.lock() {
                Ok(g)  => g,
                Err(_) => return tool_error("registry lock poisoned"),
            };
            let cols = reg.cols;
            let rows = reg.rows;
            match reg.spawn_pane(p.label, cmd, cols, rows) {
                Ok(id)  => id,
                Err(e)  => return tool_error(&format!("spawn failed: {e}")),
            }
        };

        // Ingest the task into kern so the worker can retrieve it.
        // Non-fatal: if kern is down, boot message still names the key so
        // the worker can poll on its own.
        let task_key = crate::mux::delegate::task_key(&id);
        let kern     = crate::mux::KernClient::new(&self.kern_mcp_addr);
        if let Err(e) = kern.ingest(&task_key, &p.task) {
            tracing::warn!(
                target: "kern.mux",
                session_id = %id,
                error      = %e,
                "kern ingest failed for delegate task; worker will see empty query result",
            );
        }

        // Send the boot message to the spawned pane.
        let result_key = crate::mux::delegate::result_key(&id);
        let boot = crate::mux::delegate::boot_message(&id, &self.kern_mcp_addr);
        {
            let mut reg = match self.registry.lock() {
                Ok(g)  => g,
                Err(e) => {
                    tracing::error!(target: "kern.mux", session_id = %id, error = %e, "registry lock poisoned; boot message not sent");
                    return tool_ok(serde_json::json!({
                        "session_id": id,
                        "task_key":   task_key,
                        "result_key": result_key,
                        "warning":    "boot message not sent (registry lock poisoned)"
                    }));
                }
            };
            if !reg.send_to(&id, &boot) {
                tracing::warn!(target: "kern.mux", session_id = %id, "pane vanished before boot message");
            }
        }

        tool_ok(serde_json::json!({
            "session_id": id,
            "task_key":   task_key,
            "result_key": result_key,
        }))
    }

    fn tool_collect(&self, args: &serde_json::Value) -> serde_json::Value {
        let p: CollectArgs = match serde_json::from_value(args.clone()) {
            Ok(v)  => v,
            Err(e) => return tool_error(&format!("invalid args: {e}")),
        };
        let result_key = crate::mux::delegate::result_key(&p.session_id);
        let kern       = crate::mux::KernClient::new(&self.kern_mcp_addr);
        match kern.query(&result_key) {
            Ok(text) => tool_ok(serde_json::json!({
                "session_id": p.session_id,
                "result_key": result_key,
                "result":     text,
            })),
            Err(e) => {
                tracing::warn!(target: "kern.mux", session_id = %p.session_id, error = %e, "kern query failed in collect");
                tool_ok(serde_json::json!({
                    "session_id": p.session_id,
                    "result_key": result_key,
                    "result":     "",
                    "warning":    format!("kern unreachable: {e}"),
                }))
            }
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn tool_ok(v: serde_json::Value) -> serde_json::Value {
    let s = serde_json::to_string(&v).expect("Value serialization is infallible");
    serde_json::json!({ "content": [{ "type": "text", "text": s }] })
}

fn tool_error(msg: &str) -> serde_json::Value {
    serde_json::json!({
        "isError": true,
        "content": [{ "type": "text", "text": msg }],
    })
}

fn value_to_tool_result(v: serde_json::Value) -> ToolResult {
    let is_error = v.get("isError").and_then(|v| v.as_bool()).unwrap_or(false);
    let content  = v.get("content").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    ToolResult { content, is_error, structured_content: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_definitions_are_well_formed() {
        let defs  = tool_schemas();
        let names: Vec<&str> = defs.iter()
            .map(|d| d["name"].as_str().expect("name"))
            .collect();
        assert_eq!(names, ["mux_spawn", "mux_send", "mux_list", "mux_status", "mux_delegate", "mux_collect"]);
        for d in &defs {
            let name = d["name"].as_str().unwrap();
            assert!(d["inputSchema"].is_object(), "{name}: needs inputSchema");
            assert_eq!(d["inputSchema"]["type"], "object");
        }
    }

    #[test]
    fn mux_spawn_schema_requires_label() {
        let defs  = tool_schemas();
        let spawn = defs.iter().find(|d| d["name"] == "mux_spawn").unwrap();
        let req   = spawn["inputSchema"]["required"].as_array().unwrap();
        let strs: Vec<&str> = req.iter().filter_map(|v| v.as_str()).collect();
        assert!(strs.contains(&"label"));
    }

    #[test]
    fn mux_send_schema_requires_session_id_and_text() {
        let defs = tool_schemas();
        let send = defs.iter().find(|d| d["name"] == "mux_send").unwrap();
        let req  = send["inputSchema"]["required"].as_array().unwrap();
        let strs: Vec<&str> = req.iter().filter_map(|v| v.as_str()).collect();
        assert!(strs.contains(&"session_id"));
        assert!(strs.contains(&"text"));
    }

    #[test]
    fn mux_status_schema_requires_session_id() {
        let defs   = tool_schemas();
        let status = defs.iter().find(|d| d["name"] == "mux_status").unwrap();
        let req    = status["inputSchema"]["required"].as_array().unwrap();
        let strs: Vec<&str> = req.iter().filter_map(|v| v.as_str()).collect();
        assert!(strs.contains(&"session_id"));
    }

    #[test]
    fn tool_list_includes_delegate_and_collect() {
        let defs = tool_schemas();
        let names: Vec<&str> = defs.iter()
            .map(|d| d["name"].as_str().expect("name"))
            .collect();
        assert!(names.contains(&"mux_delegate"), "mux_delegate missing from tool list");
        assert!(names.contains(&"mux_collect"),  "mux_collect missing from tool list");
    }

    #[test]
    fn mux_delegate_schema_requires_label_and_task() {
        let defs   = tool_schemas();
        let d      = defs.iter().find(|d| d["name"] == "mux_delegate").expect("mux_delegate");
        let req    = d["inputSchema"]["required"].as_array().expect("required array");
        let strs: Vec<&str> = req.iter().filter_map(|v| v.as_str()).collect();
        assert!(strs.contains(&"label"), "mux_delegate requires label");
        assert!(strs.contains(&"task"),  "mux_delegate requires task");
    }

    #[test]
    fn mux_collect_schema_requires_session_id() {
        let defs   = tool_schemas();
        let d      = defs.iter().find(|d| d["name"] == "mux_collect").expect("mux_collect");
        let req    = d["inputSchema"]["required"].as_array().expect("required array");
        let strs: Vec<&str> = req.iter().filter_map(|v| v.as_str()).collect();
        assert!(strs.contains(&"session_id"), "mux_collect requires session_id");
    }
}
