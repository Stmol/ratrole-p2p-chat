# Acceptance: nonblocking direct path upgrade

## Acceptance completed

Manual two-peer validation was completed on 2026-08-04 in both `auto` and `relay-only` modes. The code audit and automated checks for the implementation are recorded in the completed tasks. The auto-mode result below is based on the reported manual run; the relay-only result is additionally backed by the paired JSONL logs.

## Auto mode

The two peers initially connected through the relay and then selected a direct IP path almost immediately. The transition was not visible as a user-facing delay.

- Both peers reached `Connected` quickly.
- The first messages were delivered immediately after connection.
- No message remained pending while the path changed.
- The selected path changed from relay to `Direct IP` without a reconnect or a second user-visible session.

This satisfies the intended user flow for automatic path selection.

## Relay-only mode

The evidence files are:

- `target/debug/rathole-logs/rathole-relay-20260804-232403.jsonl`
- `/Users/stmol/Downloads/rathole-relay-20260804-232344.jsonl`

Both peers reported `path_mode: relay-only`. Every message event reported the same relay path, `relay:https://euc1-1.relay.n0.iroh.link./`. No Direct IP path was selected during the run. The UI's `Connected` state was accurate: an authenticated Iroh connection over the relay existed.

The first peer's initial outbound dial succeeded in about 0.43 seconds. The second peer had an earlier outbound dial timeout because the other peer was not yet available, then accepted the inbound relay connection and reached `Connected` immediately after that connection arrived. This startup detail is separate from the message-delivery delays.

All times below are from the logs' `ts_utc` field:

| Message | Local write | Remote receive | Sender settled | Result |
|---|---|---|---:|---|
| `40512a9c...` | 20:24:11.844 | 20:24:26.374 | 14.586 s | Delivered, but delayed before remote receipt |
| `d056a3cc...` | 20:25:05.157 | 20:25:07.213 | 2.162 s | Delivered |
| `f191e24a...` | 20:25:24.277 | 20:25:24.461 | 2.077 s | Remote receipt was fast; return confirmation was delayed |
| `cc6e5b88...` | 20:25:35.740 | 20:25:44.793 | 10.729 s | Delivered, but delayed before remote receipt |

The relay-only path remained functional, but its quality was inconsistent. The path statistics show RTT values roughly between 120 and 360 ms and growing `lost_packets` counters, reaching 53 later in the run. Those counters are local path statistics, not proof that the relay server itself was overloaded, but they are strong evidence of packet loss or retransmission somewhere along the relay route.

## Acceptance conclusion

- The Direct IP migration theory is not supported by this relay-only run: Direct IP was disabled and the same long delays still occurred.
- The automatic mode satisfies the intended product behavior: relay first, quick Direct IP upgrade, and no visible message delay.
- The relay-only run confirms a separate relay-path quality problem. It does not identify whether the cause is the relay server, the route to the relay, or one peer's network.
- Messages retained one final delivery outcome and were not duplicated or silently discarded.

The relay-only latency issue should be tracked as a separate relay/network investigation unless a product requirement is added that demands a fixed latency bound for relay traffic.

## Correlation fields

The paired logs correlate `message_id`, `stream_id`, and per-process `connection_id` with `path_event_kind`, `path_id`, `path_kind`, and `path_remote`. Message bodies and secrets are not logged.
