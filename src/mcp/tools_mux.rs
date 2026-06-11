//! Mux communication tools, served by [`Server`] only when it hosts a live pane
//! registry (mux mode — `Server::mux` is `Some`). Handlers operate on the
//! in-process registry and reuse [`Server::tool_ingest`] / [`Server::tool_query`]
//! — no socket, no `KernClient`. When `mux` is `None` (headless daemon) these
//! tools are neither advertised (see `mcp::Server::tools_list`) nor dispatchable.
//!
//! Tool names are kern-native (no `mux_` prefix): `delegate`, `collect`,
//! `spawn`, `send`, `panes`, `status`.

use serde::Deserialize;
use serde_json::json;

use super::{tool_error, tool_result_json, Server};

// ── Schemas ─────────────────────────────────────────────────────────────────

/// Schemas for the six comms tools. Appended to the catalogue by
/// `mcp::Server::tools_list` only when a pane registry is present.
pub fn tool_schemas() -> Vec<serde_json::Value> {
    vec![
        json!({
            "name": "delegate",
            "description": "Store a task in kern and spawn a fresh worker pane that boots by querying kern for its assignment. Returns session_id, task_key, and result_key.",
            "inputSchema": {
                "type": "object",
                "required": ["label", "task"],
                "properties": {
                    "label": { "type": "string", "description": "Human label for the new pane (e.g. 'worker-1')" },
                    "task":  { "type": "string", "description": "Full task description, stored in kern under mux:task:<session_id>" },
                    "cmd":   { "type": "string", "description": "Command to run in the pane (defaults to configured agent_cmd)" },
                },
            },
        }),
        json!({
            "name": "collect",
            "description": "Query kern for the result a worker published under mux:result:<session_id>. Empty string if not yet published.",
            "inputSchema": {
                "type": "object",
                "required": ["session_id"],
                "properties": {
                    "session_id": { "type": "string", "description": "Session id returned by delegate" },
                },
            },
        }),
        json!({
            "name": "spawn",
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
        json!({
            "name": "send",
            "description": "Write text to a pane's PTY stdin.",
            "inputSchema": {
                "type": "object",
                "required": ["session_id", "text"],
                "properties": {
                    "session_id": { "type": "string", "description": "Session id from spawn or panes" },
                    "text":       { "type": "string", "description": "Text to write to the pane's PTY stdin" },
                },
            },
        }),
        json!({
            "name": "panes",
            "description": "List all active panes.",
            "inputSchema": { "type": "object", "properties": {} },
        }),
        json!({
            "name": "status",
            "description": "Get the current visible screen content of a pane.",
            "inputSchema": {
                "type": "object",
                "required": ["session_id"],
                "properties": {
                    "session_id": { "type": "string", "description": "Session id of the pane to inspect" },
                },
            },
        }),
    ]
}

// ── Arg structs ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SpawnArgs { label: String, cmd: Option<String> }

#[derive(Deserialize)]
struct SendArgs { session_id: String, text: String }

#[derive(Deserialize)]
struct IdArgs { session_id: String }

#[derive(Deserialize)]
struct DelegateArgs { label: String, task: String, cmd: Option<String> }

#[derive(Deserialize)]
struct CollectArgs { session_id: String }

// ── Handlers (Server methods) ───────────────────────────────────────────────

impl Server {
    /// `spawn` — create a new sub-pane.
    pub(crate) fn tool_spawn(&self, args: &serde_json::Value) -> serde_json::Value {
        let Some(reg) = self.mux.as_ref() else { return tool_error("not running under a mux") };
        let p: SpawnArgs = match serde_json::from_value(args.clone()) {
            Ok(v) => v,
            Err(e) => return tool_error(&format!("invalid args: {e}")),
        };
        let cmd = p.cmd.unwrap_or_else(|| self.cfg.mux.agent_cmd.clone());
        let mut reg = match reg.lock() {
            Ok(g) => g,
            Err(_) => return tool_error("registry lock poisoned"),
        };
        let (cols, rows) = (reg.cols, reg.rows);
        match reg.spawn_pane(p.label, cmd, cols, rows) {
            Ok(id) => tool_result_json(&json!({ "session_id": id })),
            Err(e) => tool_error(&format!("spawn failed: {e}")),
        }
    }

    /// `send` — write text to a pane's stdin.
    pub(crate) fn tool_send(&self, args: &serde_json::Value) -> serde_json::Value {
        let Some(reg) = self.mux.as_ref() else { return tool_error("not running under a mux") };
        let p: SendArgs = match serde_json::from_value(args.clone()) {
            Ok(v) => v,
            Err(e) => return tool_error(&format!("invalid args: {e}")),
        };
        let mut reg = match reg.lock() {
            Ok(g) => g,
            Err(_) => return tool_error("registry lock poisoned"),
        };
        if reg.send_to(&p.session_id, &p.text) {
            tool_result_json(&json!({}))
        } else {
            tool_error(&format!("no pane with id: {}", p.session_id))
        }
    }

    /// `panes` — list active panes.
    pub(crate) fn tool_panes(&self, _args: &serde_json::Value) -> serde_json::Value {
        let Some(reg) = self.mux.as_ref() else { return tool_error("not running under a mux") };
        let reg = match reg.lock() {
            Ok(g) => g,
            Err(_) => return tool_error("registry lock poisoned"),
        };
        let panes: Vec<serde_json::Value> = reg.panes.iter().map(|p| json!({
            "session_id": p.id,
            "label":      p.label,
            "exited":     p.exited,
        })).collect();
        tool_result_json(&json!(panes))
    }

    /// `status` — current visible screen content of a pane.
    pub(crate) fn tool_status(&self, args: &serde_json::Value) -> serde_json::Value {
        let Some(reg) = self.mux.as_ref() else { return tool_error("not running under a mux") };
        let p: IdArgs = match serde_json::from_value(args.clone()) {
            Ok(v) => v,
            Err(e) => return tool_error(&format!("invalid args: {e}")),
        };
        let reg = match reg.lock() {
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
        tool_result_json(&json!({
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

    /// `delegate` — ingest a task into kern (in-process), spawn a worker pane,
    /// and send it the boot message that points it at its kern task key.
    pub(crate) fn tool_delegate(&self, args: &serde_json::Value) -> serde_json::Value {
        let Some(reg) = self.mux.as_ref() else { return tool_error("not running under a mux") };
        let p: DelegateArgs = match serde_json::from_value(args.clone()) {
            Ok(v) => v,
            Err(e) => return tool_error(&format!("invalid args: {e}")),
        };
        let cmd = p.cmd.unwrap_or_else(|| self.cfg.mux.agent_cmd.clone());

        // Spawn first so we have a session_id to key the task on.
        let id = {
            let mut r = match reg.lock() {
                Ok(g) => g,
                Err(_) => return tool_error("registry lock poisoned"),
            };
            let (cols, rows) = (r.cols, r.rows);
            match r.spawn_pane(p.label, cmd, cols, rows) {
                Ok(id) => id,
                Err(e) => return tool_error(&format!("spawn failed: {e}")),
            }
        };

        let task_key   = crate::mux::task_key(&id);
        let result_key = crate::mux::result_key(&id);

        // In-process ingest — the same path Claude's mcp__kern__ingest drives.
        // `sync: true` so the task is searchable before the worker queries it.
        let ingest_text = crate::mux::delegate::kern_ingest_text(&id, &p.task);
        let _ = self.tool_ingest(&json!({
            "text":      ingest_text,
            "source":    "agent",
            "object_id": task_key,
            "sync":      true,
        }));

        // Boot the worker. The addr arg is vestigial (workers reach kern via
        // mcp__kern__query automatically); it is removed when boot_message is
        // simplified alongside the mux/mcp.rs deletion.
        let boot = crate::mux::boot_message(&id, "");
        if let Ok(mut r) = reg.lock() {
            if !r.send_to(&id, &boot) {
                tracing::warn!(target: "kern.mux", session_id = %id, "pane vanished before boot message");
            }
        }

        tool_result_json(&json!({
            "session_id": id,
            "task_key":   task_key,
            "result_key": result_key,
        }))
    }

    /// `collect` — in-process query for the worker's published result.
    pub(crate) fn tool_collect(&self, args: &serde_json::Value) -> serde_json::Value {
        if self.mux.is_none() {
            return tool_error("not running under a mux");
        }
        let p: CollectArgs = match serde_json::from_value(args.clone()) {
            Ok(v) => v,
            Err(e) => return tool_error(&format!("invalid args: {e}")),
        };
        let result_key = crate::mux::result_key(&p.session_id);
        let res = self.tool_query(&json!({ "text": result_key, "k": 3 }));
        // tool_query wraps its JSON payload as a single text content block;
        // surface that inner text as `result`.
        let result_text = res
            .get("content")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first())
            .and_then(|b| b.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        tool_result_json(&json!({
            "session_id": p.session_id,
            "result_key": result_key,
            "result":     result_text,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_names_are_kern_native() {
        let defs = tool_schemas();
        let names: Vec<&str> = defs.iter().map(|d| d["name"].as_str().expect("name")).collect();
        assert_eq!(names, ["delegate", "collect", "spawn", "send", "panes", "status"]);
        // No legacy mux_ prefix survives.
        assert!(names.iter().all(|n| !n.starts_with("mux_")), "no mux_ prefix: {names:?}");
    }

    #[test]
    fn delegate_requires_label_and_task() {
        let defs = tool_schemas();
        let d = defs.iter().find(|d| d["name"] == "delegate").expect("delegate present");
        let req: Vec<&str> = d["inputSchema"]["required"].as_array().unwrap()
            .iter().filter_map(|v| v.as_str()).collect();
        assert!(req.contains(&"label") && req.contains(&"task"), "got {req:?}");
    }

    #[test]
    fn collect_and_status_require_session_id() {
        let defs = tool_schemas();
        for name in ["collect", "status", "send"] {
            let d = defs.iter().find(|d| d["name"] == name).unwrap();
            let req: Vec<&str> = d["inputSchema"]["required"].as_array().unwrap()
                .iter().filter_map(|v| v.as_str()).collect();
            assert!(req.contains(&"session_id"), "{name} must require session_id, got {req:?}");
        }
    }

    #[test]
    fn every_schema_is_well_formed() {
        for d in tool_schemas() {
            let name = d["name"].as_str().expect("name");
            assert!(d["inputSchema"].is_object(), "{name}: needs inputSchema");
            assert_eq!(d["inputSchema"]["type"], "object", "{name}: type must be object");
        }
    }
}
