# `web/` — SpaceNix web app

React 19 + Vite single-page app for SpaceNix. It connects to SpacetimeDB
directly through the official TypeScript SDK, and it also acts as the OAuth
callback for the `spacenix` TUI / CLI login flow.

## Stack

- **React 19** with **TypeScript 6** (strict).
- **Vite 8** for dev server and build (`pnpm dev`, `pnpm build`).
- **Tailwind CSS v4** via `@tailwindcss/vite`; the `components.json` file
  declares the shadcn-style primitive layout.
- **Radix UI** primitives (alert-dialog, dialog, dropdown, select, switch,
  tabs, tooltip, …) wrapped in `src/components/ui/`.
- **TanStack Query v5** for non-realtime background fetches and
  **Sonner** for toasts.
- **`spacetimedb` 2.4.1** client SDK (and its React bindings) for the data
  layer.

## Layout

```
web/
├── index.html
├── package.json
├── pnpm-lock.yaml
├── tsconfig*.json
├── vite.config.ts           @ alias → src/, tailwind plugin.
├── eslint.config.js         ESLint flat config (typescript-eslint, react
│                            hooks / refresh plugins).
├── components.json          shadcn-style primitive config.
├── public/                  Static assets (favicon, …).
└── src/
    ├── main.tsx             React root.
    ├── App.tsx              Provider stack, router, auth bootstrap.
    ├── index.css            Tailwind entry + design tokens.
    ├── components/
    │   ├── ui/              Radix-wrapped primitives.
    │   ├── app-shell.tsx    Sidebar + main content.
    │   ├── common.tsx       PageHeader, EmptyState, Spinner, ConfirmDelete.
    │   ├── file-tree.tsx    Hierarchical file / folder tree.
    │   ├── file-row.tsx     Single row in the tree.
    │   ├── file-tree-utils.ts
    │   ├── folder-picker.tsx
    │   ├── permission-editor.tsx
    │   └── tag-input.tsx
    ├── pages/
    │   ├── login.tsx        Sign-in / sign-up form.
    │   ├── files.tsx        Encrypted file browser + uploads.
    │   ├── secrets.tsx      Per-environment secret list / reveal.
    │   ├── ssh.tsx          SSH keys and endpoints.
    │   ├── pats.tsx         Personal access tokens.
    │   ├── devices.tsx      Device registry.
    │   └── account.tsx      Profile / email / password.
    ├── lib/
    │   ├── stdb.ts          SpacetimeDB URI / module defaults, token
    │   │                    storage, OAuth callback URL helpers.
    │   ├── auth.tsx         AuthProvider — connects to STDB, reads
    │   │                    `my_user`, derives file encryption key.
    │   ├── auth-context.ts  Context types and hooks.
    │   ├── file-crypto.ts   PBKDF2 → AES-GCM file encryption.
    │   ├── use-theme.ts     Dark / light theme hook.
    │   ├── toast.ts         Sonner wrapper.
    │   └── utils.ts         cn(), formatBytes(), …
    └── module_bindings/     Auto-generated TypeScript bindings
                             (regenerated via `pnpm gen:bindings`).
```

## Develop

```bash
pnpm install
pnpm dev          # http://localhost:5173
```

By default the app talks to a local SpacetimeDB server
(`http://127.0.0.1:3000`) and the `spacenix-9wfd4` database. Override with a
`.env` (gitignored):

```
VITE_STDB_URI=https://maincloud.spacetimedb.com
VITE_STDB_MODULE=spacenix
```

See `src/lib/stdb.ts:5` for the full default-resolution logic.

## Browser SSH terminal

The SSH page (`/ssh`) has a "play" button on each enabled endpoint
that opens an interactive terminal in the browser, driven by `xterm.js`.
The browser speaks WebSocket to one of the user's devices running
`spacenix service start --bind 0.0.0.0` (the "relay device"), and
the relay device spawns `ssh(1)` in a pty and bridges the bytes.

To set it up:

1. On the device that should run SSH (`spacenix login` first), start
   the service on a non-loopback address:
   ```bash
   spacenix service start --bind 0.0.0.0 --port 7770
   ```
   (Use `wss://` and a Tailscale address if the browser is on a
   different network. The same network + plain `ws://` also works.)
2. On the Devices page, click the star (☆) on that device's row to
   mark it as the SSH relay. The same row expands to show a
   "Relay URL" input — enter the address the browser can use to
   reach the service, e.g. `ws://laptop.lan:7770` or
   `wss://my-laptop.tail-net.ts.net:7770`, and click Save.
3. On the SSH page, click the play button on any enabled endpoint.

The per-session auth token is minted by the relay device and read
by the browser from the SpacetimeDB subscription — it never appears
in the URL, in localStorage, or in the UI. The SSH private key is
fetched from the database by the relay device, written to a 0600
tempfile, and removed when the session ends. The browser never sees
the key — it only sees the encrypted WebSocket stream.

## Build / preview / typecheck / lint

```bash
pnpm build        # tsc -b && vite build
pnpm preview      # serve the built dist/
pnpm typecheck    # tsc -b --pretty
pnpm lint         # eslint .
```

## Regenerate SpacetimeDB bindings

```bash
pnpm gen:bindings
```

This runs `spacetime generate --lang typescript --module-path ../sync/spacetimedb`
and writes the result to `src/module_bindings/`. It is a local codegen step
and does not touch a running server.

## Auth flow

1. The user lands on `/` and is shown `LoginPage` if no SpacetimeDB session
   exists.
2. On sign-in, `AuthProvider` (`src/lib/auth.tsx:36`) opens a connection
   through `SpacetimeDBProvider`, stores the issued token, and reads the
   `my_user` view.
3. Once the user is signed in, `App.tsx:90` checks for a pending CLI
   callback (set via the `?callback=…` query param the TUI adds when it
   spawns the browser) and, if present, redirects the browser to
   `http://127.0.0.1:<port>/oauth/callback` with the token so the TUI can
   capture it.
4. The session is preserved in `localStorage` under the `spacenix.stdb.token`
   key; signing out (`SessionKeyProvider.bump`) increments a nonce that
   forces `SpacetimeDBProvider` to remount with a fresh anonymous identity.

## File encryption

`src/lib/file-crypto.ts` derives a per-user `CryptoKey` from the user's
password and identity hex with PBKDF2 (310 000 iterations, SHA-256), then
encrypts file contents with AES-GCM before upload. The SpacetimeDB module
mints short-lived presigned URLs (15 min upload / 5 min download) so the
server never sees the plaintext. The encrypted blob is downloaded, decrypted
in the browser, and presented to the user.
