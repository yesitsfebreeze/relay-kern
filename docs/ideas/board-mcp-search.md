# MCP `search` — fuzzy ticket search

> **Parked** with the rest of the board. Code lives under `planned/board/`
> and still uses the crate name `cranyum/` internally.

Fast fuzzy search over the kanban board. Exposed to AI agents, other MCP
clients, and external tools via the board MCP server.

## Request

Tool name: `search`

Arguments:

| field   | type    | required | default | notes                                      |
|---------|---------|----------|---------|--------------------------------------------|
| `query` | string  | yes      | —       | fuzzy, case-insensitive                    |
| `limit` | integer | no       | 16      | upper bound on returned hits               |

## Response

Array of hit objects, sorted by descending `score` (higher = better match):

```json
[
  { "id": "16851333-3b48-402f-bd68-3ce4349743dc",
    "short_id": "16851333",
    "title": "Implement Fuzzy Find Search Feature for MCP",
    "score": 412 },
  ...
]
```

Empty query (or all-whitespace) returns `[]`. Archived tickets are
excluded.

## Ranking

Each ticket is scored independently against three fields using fzf-style
fuzzy matching (`cranyum::fuzzy::fuzzy_score`):

- **title** — weighted 3x
- **description** — weighted 1x
- **context** — weighted 1x

The body contribution uses the max of description and context score (not
the sum) so a term appearing in both fields doesn't double-count. The
final score is `title*3 + max(description, context)`; tickets with no
match on any field are dropped.

Scoring rewards:

- consecutive matched characters
- matches at word boundaries (`-`, `_`, `/`, `.`, space, start-of-string)
- camelCase transitions
- leading characters

## Indexing

No secondary index. `search_tasks` scans the full ticket list per call
and scores in-memory. For current board sizes (hundreds of tickets) this
is sub-millisecond and keeps the implementation trivially correct — no
index rebuild on mutation, no drift between the live board and the
search view. If the board scales past ~10k rows, swap the linear scan
for SQLite FTS5 over `title || description || context` while keeping the
same response shape.

## Usage examples

**MCP JSON-RPC (stdio or websocket):**

```json
{"jsonrpc":"2.0","id":1,"method":"tools/call",
 "params":{"name":"search","arguments":{"items":[{"query":"fuzzy","limit":16}]}}}
```

**Rust (in-process):**

```rust
let hits = cranyum::board_ops::search_tasks(&conn, "fuzzy", 16)?;
for h in hits {
    println!("{}  {:>4}  {}", h.short_id, h.score, h.title);
}
```

## Performance

- Linear scan over all non-archived tickets.
- Three `fuzzy_score` calls per ticket (title + description + context).
- `fuzzy_score` is O(n·m) DP on two rows; empty or non-matching fields
  short-circuit via the pre-pass subsequence check.
- No cache — results reflect the board state at call time.
