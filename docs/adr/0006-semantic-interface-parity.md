# CLI and MCP share semantic Interface Parity

Rivets defines **Interface Parity** as equivalent observable domain behavior across CLI and MCP operations that represent the same intent: the same invariants, defaults, ordering, validation, error meaning, and result meaning. We chose semantic parity over identical surfaces because CLI prompts, confirmations, text rendering, and partial-success batching are useful human adapter mechanics, while MCP context selection, protocol envelopes, and loop-equivalent single-target calls are useful agent adapter mechanics; we rejected outcome-only parity because it permits silent drift in query results, accepted inputs, and returned information.

## Consequences

Accepted domain decisions in `CONTEXT.md` and ADRs define the target even when both adapters currently agree on legacy behavior. Every current CLI leaf operation and MCP tool, plus every required intent implied by accepted decisions, must be classified in the machine-readable parity registry. Differences are either explicit adapter mechanics, known gaps with a required resolution, or legacy surfaces awaiting canonical cutover. Workspace initialization and permanent Issue deletion remain CLI-only; MCP context selection and inspection remain MCP-only. CLI batches need no matching MCP batch tool when repeated single-target calls preserve the same partial-success semantics.

`docs/cli-mcp-parity.json` is the source of truth for classifications and observable contracts. `docs/cli-mcp-parity.md` is its rendered reference. A contract test fails when the current CLI or MCP inventory changes without an explicit registry classification; behavioral tests continue to defend the conforming contracts themselves.
