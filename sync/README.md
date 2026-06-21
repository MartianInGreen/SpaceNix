# `sync/` — SpacetimeDB server module

This directory contains the SpacetimeDB module for SpaceNix: the Rust source
that compiles to a WebAssembly bundle and is published into the SpacetimeDB
server. The module owns all persistent state (users, sessions, files, secrets,
SSH keys, devices, API keys, and the S3 configuration) and exposes the
reducers, procedures, and views that every client uses.

## Layout

```
sync/
├── AGENTS.md              Working notes for AI agents: SpacetimeDB rules,
│                          CLI cheatsheet, and Rust SDK reference.
├── spacetime.json         Default `spacetime` CLI config (module path,
│                          target server).
├── spacetime.local.json   Local override (typically holds the database name
│                          used by `scripts/upload-s3-env.sh`).
├── spacetimedb/           The actual Rust module (crate type `cdylib`).
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs         Module entry: lifecycle hooks (`init`,
│       │                  `client_connected`, `client_disconnected`).
│       ├── user.rs        Users, sessions, password hashes (Argon2).
│       ├── device.rs      Device registry per user.
│       ├── secret.rs      Per-environment secret values + permissions.
│       ├── ssh.rs         SSH keypairs and SSH endpoints.
│       ├── file.rs        UserFile rows + S3 presigned-URL procedures.
│       ├── api_key.rs     Personal access tokens (`snx_…`).
│       └── config.rs      Singleton S3 config row + admin reducers.
└── src/
    └── module_bindings/   Generated TypeScript bindings (regenerated via
                           `pnpm --dir web gen:bindings`).
```

## Module responsibilities

- **Auth** (`user.rs`) — `sign_up` / `sign_in` reducers create Argon2 password
  hashes, mint a `Session` keyed by `ctx.sender()`, and gate all other
  reducers through `require_registered_user`.
- **Files** (`file.rs`) — `user_file` table stores metadata; encrypt-on-upload
  / decrypt-on-download happens client-side. The module exposes
  `create_file_upload_url`, `create_file_download_url`, `replace_file` and
  companion delete / rename / move reducers. URLs are HMAC-signed and time-
  limited (15 min for upload, 5 min for download).
- **Secrets** (`secret.rs`) — values are stored encrypted in the database and
  only revealed through the `reveal_secret` procedure, which checks the
  caller's permissions and device allow-list.
- **SSH** (`ssh.rs`) — full keypairs plus endpoint records; both support
  device allow-lists, tags, and enable / disable.
- **Browser SSH relay** (`ssh.rs`) — the user picks one of their devices
  as a relay (`ssh_relay_device`); opening an endpoint from the web
  app mints a `SshRelaySession` row. The TUI service on the relay
  device attaches a per-session bearer token; the browser opens a
  WebSocket to the service, presents the token, and the service
  spawns `ssh(1)` in a pty.
- **API keys** (`api_key.rs`) — `snx_…` prefixed tokens, hashed at rest, with
  permission scopes and revocation.
- **S3 config** (`config.rs`) — a single row (id `1`) holding bucket, region,
  optional endpoint / path prefix / public base URL, and access credentials.
  Updated by an admin-only `update_s3_config` reducer (or its
  `update_s_3_config_with_credentials` companion used by
  `scripts/upload-s3-env.sh`).

## Build

```bash
cd sync/spacetimedb
spacetime build                # release WASM bundle
spacetime build --debug        # faster iteration, slower runtime
```

## Publish

**The user publishes.** The agent (and CI by default) must not run
`spacetime publish`, `spacetime call`, `spacetime sql`, `spacetime logs`,
`spacetime subscribe`, or `spacetime delete`. The full rule and the rationale
are in `sync/AGENTS.md:1`. To publish, the user runs:

```bash
spacetime publish spacenix --server local --yes
# or
spacetime publish spacenix --server maincloud --yes
```

## Regenerate client bindings

```bash
# TypeScript (web/)
pnpm --dir web gen:bindings

# Rust (tui/) — see tui/scripts/gen-bindings.sh
./tui/scripts/gen-bindings.sh            # uses cached WASM if present
./tui/scripts/gen-bindings.sh --build    # rebuilds WASM first
```

Both commands are local codegen — they do not touch a running server.

## Configuration files

- `spacetime.json` — checked-in defaults (`module-path`, `server`).
- `spacetime.local.json` — gitignored local override. The
  `scripts/upload-s3-env.sh` script reads `.database` / `.module` from this
  file when deciding which database to push S3 credentials to.
