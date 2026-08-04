# Chat Diagnostics

Use this runbook when investigating a live chat problem, especially when a message is visible on the remote client but the sender does not reach `Delivered`.

## Log files

Every launch creates a flushed JSONL diagnostic file and prints its exact path before entering the TUI. Without an override, the file is stored under the Rathole local data directory in `logs/`. The repository shortcuts put timestamped files next to the debug binary under `target/debug/rathole-logs/`.

Set `RATHOLE_LOG_FILE` to make paired runs easy to identify:

```sh
RATHOLE_LOG_FILE=/tmp/rathole-client-a.jsonl cargo run
RATHOLE_LOG_FILE=/tmp/rathole-client-b.jsonl cargo run
```

For a relay-only comparison, set `RATHOLE_IROH_PATH_MODE=relay-only` or use:

```sh
just run-relay
```

The default path mode is `auto`. An invalid value stops startup with an explicit error. The configured path mode is a diagnostic input, not proof of the path Iroh actually selected.

## Correlating two clients

When reporting a problem, send both JSONL files unchanged. Records include:

- the local `local_peer_id`;
- per-run `instance_id`, `event_id`, and monotonic `seq`;
- wall-clock `ts_unix_ms` and `ts_utc`;
- local `monotonic_ms` for exact ordering within one process;
- the remote `peer_id` when known;
- shared `message_id` and, when relevant, local `connection_id` and `stream_id`;
- transport path metadata and counters when Iroh exposes them.

Use `ts_unix_ms` to align the two computers approximately. Use `seq` and `monotonic_ms` to establish exact order inside each client. `connection_id` is local to one process; correlate the two machines with `message_id` and `stream_id` instead.

Message bodies, private keys, and device secrets are not written to the diagnostic log. The logger records message sizes and protocol metadata rather than message content.

## Receipt events

`receipt_write_finished` means the local process finished writing its response to a stream. `receipt_received` means the sender's runtime read the remote response. A local stream `finish` is not proof that the remote runtime read the receipt. There is no third receipt acknowledgement in the current CBOR v1 protocol.

`Delivered` is therefore a transport/runtime acceptance state. It is not a read receipt and does not prove durable storage.

## Quick filters

Filter both files by a shared message ID:

```sh
jq 'select(.message_id == "<message-id>")' /tmp/rathole-client-a.jsonl
```

Focus on connection, receipt, and delivery events:

```sh
jq 'select(.event | test("connection|receipt|delivery"))' /tmp/rathole-client-b.jsonl
```
