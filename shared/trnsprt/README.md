# `trnsprt`

Model Context Protocol client. Two transports, one surface.

- **Stdio** (`ChildStdio`): JSON-RPC 2.0 newline-delimited UTF-8 frames over a
  child process's `stdin`/`stdout`. One complete JSON object per line;
  embedded newlines not permitted.
- **In-proc** (`InProcTransport` + `InProcServer`): same JSON-RPC 2.0 shape,
  no process, no serialization cost across a pipe. For hot-path plugins (LLM
  provider, context assembler) where stdio latency is a material cost.

## Scope

Minimal:

- spawn a child process (program + args) **or** wrap a crate-local
  `InProcServer`,
- run the `initialize` handshake + `initialized` notification,
- list tools (`tools/list`),
- invoke tools (`tools/call`),
- reap the child on shutdown (explicit or `Drop`) — in-proc `kill()` just
  flips an internal flag.

## Registry

- `ServerId` — key.
- `Registry` — `HashMap<ServerId, …>`. Exposes `list_tools`,
  `call_tool`. Used by the agent loop / recipe engine.
- Registration:
  - `spawn_stdio(id, program, args)` — child process transport.
  - `register_inproc(id, Box<dyn InProcServer>)` — in-process transport.
- Callers dispatch via `call_tool(id, name, args)` — identical regardless of
  transport.

## Framing

Line-delimited JSON-RPC 2.0. **Not** LSP's `Content-Length` framing.

## Performance

In-proc per-call dispatch avoids the kernel pipe round-trip and UTF-8
re-parsing across a process boundary. Inline test
`inproc_transport_call_latency_under_ceiling` asserts a generous 5 ms
per-call ceiling (observed locally sub-100 µs) — a child-process stdio
round-trip is typically two-to-three orders of magnitude slower due to
syscalls + scheduler wake-up. Benchmarking against a real stdio server
belongs to the plugin that opts in, not this crate.

## Extending

- New transports → implement `Transport` (read/write/kill) and register a
  `Client` around it. `InProcTransport` is the reference in-process impl.
- Streaming tool output → stay within MCP spec; add decoder at the
  transport layer, not here.
