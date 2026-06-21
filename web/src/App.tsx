import * as React from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { SpacetimeDBProvider } from "spacetimedb/react";
import { Toaster } from "sonner";
import { BrowserRouter, Routes, Route, Navigate } from "react-router-dom";

import { DbConnection } from "@/module_bindings";
import {
  STDB_MODULE,
  STDB_URI,
  buildCallbackRedirectUrl,
  clearPendingCallback,
  loadStoredToken,
  readPendingCallback,
  storePendingCallback,
  storeToken,
} from "@/lib/stdb";
import { AuthProvider, SessionKeyProvider } from "@/lib/auth";
import { useAuth, useSessionKey } from "@/lib/auth-context";
import { AppShell } from "@/components/app-shell";
import { LoginPage } from "@/pages/login";
import { FilesPage } from "@/pages/files";
import { SecretsPage } from "@/pages/secrets";
import { SshPage } from "@/pages/ssh";
import { PatsPage } from "@/pages/pats";
import { DevicesPage } from "@/pages/devices";
import { AccountPage } from "@/pages/account";

const queryClient = new QueryClient({
  defaultOptions: { queries: { refetchOnWindowFocus: false, retry: false } },
});

function buildConnectionBuilder() {
  return DbConnection.builder()
    .withUri(STDB_URI)
    .withDatabaseName(STDB_MODULE)
    .withToken(loadStoredToken())
    .onConnect((_conn, _identity, token) => {
      storeToken(token);
    });
}

function ConnectionRoot() {
  const { nonce } = useSessionKey();
  return (
    <SpacetimeDBProvider
      key={nonce}
      connectionBuilder={buildConnectionBuilder()}
    >
      <AuthProvider>
        <Router />
      </AuthProvider>
    </SpacetimeDBProvider>
  );
}

function Router() {
  const { status, isAuthenticated, identityHex, restoring, fileEncryptionKey, fileEncryptionError } =
    useAuth();

  // Capture a `?callback=…` query param from the URL (set by the SpaceNix
  // CLI / TUI when it spawns the browser) and stash it in sessionStorage so
  // the redirect effect below can pick it up once auth completes.
  React.useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const callback = params.get("callback");
    if (callback) {
      try {
        const parsed = new URL(callback);
        if (parsed.protocol === "http:" && parsed.hostname === "127.0.0.1") {
          storePendingCallback({ url: callback });
          // Strip the query string from the address bar so a refresh
          // doesn't re-apply it.
          params.delete("callback");
          const next =
            window.location.pathname +
            (params.toString() ? `?${params.toString()}` : "") +
            window.location.hash;
          window.history.replaceState({}, "", next);
        }
      } catch {
        // Ignore malformed callback URLs.
      }
    }
  }, []);

  // Once the user is fully signed in (and the connection token has landed
  // in localStorage via `onConnect`), redirect the browser to the TUI's
  // local callback URL so the CLI can pick the token up.
  React.useEffect(() => {
    if (!isAuthenticated) return;
    const pending = readPendingCallback();
    if (!pending) return;
    const token = loadStoredToken();
    if (!token) return;
    const redirect = buildCallbackRedirectUrl(pending.url, token, identityHex);
    clearPendingCallback();
    // Replace instead of assign so the user doesn't have a confusing
    // back-button history entry pointing at the callback URL.
    window.location.replace(redirect);
  }, [isAuthenticated, identityHex]);

  if (status === "connecting" || restoring) {
    return (
      <div className="flex h-full items-center justify-center text-muted-foreground">
        {restoring ? "Signing you in…" : "Connecting to SpacetimeDB…"}
      </div>
    );
  }

  if (status === "error") {
    return (
      <div className="flex h-full items-center justify-center p-6">
        <div className="max-w-md text-center">
          <h2 className="text-lg font-semibold text-destructive">Connection failed</h2>
          <p className="mt-2 text-sm text-muted-foreground">
            Could not reach SpacetimeDB at <code className="font-mono">{STDB_URI}</code> for module{" "}
            <code className="font-mono">{STDB_MODULE}</code>. Start the server with{" "}
            <code className="font-mono">spacetime start</code> and ensure the module is published.
          </p>
        </div>
      </div>
    );
  }

  if (!isAuthenticated) {
    return (
      <Routes>
        <Route path="*" element={<LoginPage />} />
      </Routes>
    );
  }

  if (fileEncryptionError) {
    return (
      <div className="flex h-full items-center justify-center p-6">
        <div className="max-w-md text-center">
          <h2 className="text-lg font-semibold text-destructive">Encryption setup failed</h2>
          <p className="mt-2 text-sm text-muted-foreground">{fileEncryptionError}</p>
        </div>
      </div>
    );
  }

  if (!fileEncryptionKey) {
    return (
      <div className="flex h-full items-center justify-center text-muted-foreground">
        Preparing file encryption…
      </div>
    );
  }

  return (
    <Routes>
      <Route element={<AppShell />}>
        <Route index element={<Navigate to="/files" replace />} />
        <Route path="files" element={<FilesPage />} />
        <Route path="secrets" element={<SecretsPage />} />
        <Route path="ssh" element={<SshPage />} />
        <Route path="pats" element={<PatsPage />} />
        <Route path="devices" element={<DevicesPage />} />
        <Route path="account" element={<AccountPage />} />
        <Route path="*" element={<Navigate to="/files" replace />} />
      </Route>
    </Routes>
  );
}

export default function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <SessionKeyProvider>
          <ConnectionRoot />
        </SessionKeyProvider>
      </BrowserRouter>
      <Toaster richColors position="bottom-right" />
    </QueryClientProvider>
  );
}
