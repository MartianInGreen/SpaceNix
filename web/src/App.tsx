import * as React from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { SpacetimeDBProvider } from "spacetimedb/react";
import { Toaster } from "sonner";
import { BrowserRouter, Routes, Route, Navigate, useLocation } from "react-router-dom";

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
import { AuthProvider, SessionKeyProvider, useAuth, useSessionKey } from "@/lib/auth";
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
  const { status, isAuthenticated, restoring, fileEncryptionKey, fileEncryptionError } = useAuth();

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
