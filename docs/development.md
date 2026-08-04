# Development Guide

This guide covers local development, repository validation, and the optional OpenSpec workflow. The product overview and quick start are in [`README.md`](../README.md). Engineering and TUI implementation rules are in [`AGENTS.md`](../AGENTS.md).

## Requirements

- Stable Rust with `rust-version = 1.91` or newer.
- `just` for the repository shortcuts; the equivalent `cargo` commands remain available.
- Node.js 20.19.0 or newer only when using the repository's OpenSpec workflow.

The project has no Node.js runtime dependency. OpenSpec is a development-time planning tool and is not linked into the Rathole binary.

## Run and validate

Run the TUI directly:

```sh
cargo run
```

The repository shortcuts also configure diagnostic log locations:

```sh
just run          # alias: just r
just dev          # alias: just d; build, sign, and run the dev binary
just run-relay    # alias: just rr; force relay-only Iroh paths
```

### Signed development launch on macOS

`just dev` is the normal local development command. It builds
`target/debug/rathole`, signs it, and starts the same file with the file-backed
development identity. Signing prevents macOS from treating every rebuilt
debug executable as a new network process because its code hash changes.

The direct `cargo run` and `just run` commands remain raw Cargo launches. Use
`just dev` for routine local iteration with the macOS Application Firewall or
LuLu enabled.

Create a persistent local code-signing identity in Keychain Access with the
certificate type `Code Signing` and the name `Rathole Local Development`, or
use an existing Apple signing identity. Verify the available identities with:

```sh
security find-identity -v -p codesigning
```

The default certificate name and code-signing identifier can be overridden when
needed:

```sh
APPLE_CODESIGN_IDENTITY="Apple Development: Developer Name (TEAMID)" just dev
RATHOLE_CODESIGN_IDENTIFIER="org.rathole.rathole.dev" just dev
```

Approve the first signed `just dev` run in the macOS firewall and create the
matching allow rule in LuLu. Keep the signing certificate and identifier stable
across rebuilds; do not replace this with an ad hoc `codesign --sign -`
signature, because that does not provide a persistent code-signing identity.

Use the standard repository checks before handing off a change:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Development storage profile

The normal profile uses the operating-system keychain for the device secret. `just dev` sets `RATHOLE_STORAGE_PROFILE=dev`, avoids the OS keychain, and uses a separate file-backed development identity.

The development profile stores its identity and contacts under:

```text
~/.config/rathole/device.key
~/.config/rathole/contacts.toml
```

`device.key` contains exactly 32 raw bytes. On Unix it is created with owner-only `0600` permissions. It is intentionally not encrypted: never copy, share, commit, or synchronize it. Deleting the file creates a new development peer ID. Development logs created by `just dev` remain under `target/debug/rathole-logs/`.

The normal profile uses the platform-specific Rathole data directory for contacts and logs. The application prints the exact diagnostic log path before entering the TUI.

## OpenSpec workflow

OpenSpec is used for changes that need durable requirements, design decisions, and implementation tasks. The repository contains generated integrations for Codex, Cursor, and Claude Code.

Install the CLI once on a development machine:

```sh
npm install -g @fission-ai/openspec@latest
openspec --version
```

The checked-in tool integrations are refreshed after an OpenSpec CLI upgrade:

```sh
openspec update
```

Use `openspec init --tools codex,cursor,claude --profile core` only when setting up or regenerating the selected tool tree in a fresh project. Useful terminal checks are:

```sh
openspec doctor --json
openspec list --json
openspec list --specs --json
openspec validate --all --strict --no-interactive
```

The supported AI-chat entry points are:

| Tool | Propose | Apply | Archive |
| --- | --- | --- | --- |
| Codex | `$openspec-propose` | `$openspec-apply-change` | `$openspec-archive-change` |
| Cursor | `/opsx-propose` | `/opsx-apply` | `/opsx-archive` |
| Claude Code | `/opsx:propose` | `/opsx:apply` | `/opsx:archive` |

The normal loop is: optionally explore an idea, propose a change, review its artifacts, apply approved tasks, sync the resulting specs, and archive the completed change. Create a capability spec when a real product change needs a durable requirement; do not create a speculative specification inventory for the entire existing application.

Files under `.codex/skills/openspec-*`, `.cursor/skills/openspec-*`, `.cursor/commands/opsx-*`, `.claude/skills/openspec-*`, and `.claude/commands/opsx/` are generated integrations. Do not edit them by hand; run `openspec update` after changing the CLI or selected profile.

OpenSpec artifacts do not replace `AGENTS.md`, code review, Rust tests, live network acceptance, or the security rules. The source-of-truth boundary is:

- `AGENTS.md` for project-wide engineering and security rules;
- relevant `openspec/specs/` and active change artifacts for approved capability requirements and decisions;
- module README files and `docs/` for stable technical and operational documentation;
- `README.md` for orientation and quick start.
