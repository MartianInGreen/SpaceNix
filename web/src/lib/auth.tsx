import * as React from "react";

import { useSpacetimeDB, useTable, useReducer } from "spacetimedb/react";
import { reducers, tables } from "@/module_bindings";

import {
  clearStoredCredentials,
  clearStoredToken,
  identityHex,
  loadStoredCredentials,
  storeCredentials,
} from "@/lib/stdb";
import { deriveFileEncryptionKey } from "@/lib/file-crypto";
import {
  AuthContext,
  SessionKeyContext,
  useSessionKey,
  type AuthContextValue,
  type AuthState,
} from "@/lib/auth-context";

/**
 * Holds a session nonce used to force the SpacetimeDBProvider to remount with a
 * fresh anonymous identity on sign-out. Lives above the connection provider so
 * it does not depend on an active connection.
 */
export function SessionKeyProvider({ children }: { children: React.ReactNode }) {
  const [nonce, setNonce] = React.useState(0);
  const value = React.useMemo(
    () => ({ nonce, bump: () => setNonce((n) => n + 1) }),
    [nonce]
  );
  return <SessionKeyContext.Provider value={value}>{children}</SessionKeyContext.Provider>;
}

export function AuthProvider({ children }: { children: React.ReactNode }) {
  const conn = useSpacetimeDB();
  const [rows] = useTable(tables.my_user);
  const { bump } = useSessionKey();

  const signInReducer = useReducer(reducers.signIn);
  const signUpReducer = useReducer(reducers.signUp);
  const signOutReducer = useReducer(reducers.signOut);

  const user = rows[0];
  const connectionIdentityHex = identityHex(conn.identity);
  const userIdentityHex = user ? identityHex(user.identity) : connectionIdentityHex;
  const hasUser = Boolean(user);
  const [restoring, setRestoring] = React.useState<boolean>(
    () => loadStoredCredentials() !== undefined
  );
  const [fileEncryptionPassword, setFileEncryptionPassword] = React.useState<string | null>(
    () => loadStoredCredentials()?.password ?? null
  );
  const [fileEncryptionKey, setFileEncryptionKey] = React.useState<CryptoKey | null>(null);
  const [fileEncryptionError, setFileEncryptionError] = React.useState<string | undefined>();

  React.useEffect(() => {
    if (!userIdentityHex || !fileEncryptionPassword || !hasUser) {
      setFileEncryptionKey(null);
      setFileEncryptionError(undefined);
      return;
    }

    let cancelled = false;
    setFileEncryptionError(undefined);
    deriveFileEncryptionKey(fileEncryptionPassword, userIdentityHex)
      .then((key) => {
        if (!cancelled) setFileEncryptionKey(key);
      })
      .catch((err) => {
        if (!cancelled) {
          setFileEncryptionKey(null);
          setFileEncryptionError(
            err instanceof Error ? err.message : "Could not derive file encryption key."
          );
        }
      });

    return () => {
      cancelled = true;
    };
  }, [fileEncryptionPassword, userIdentityHex, hasUser]);

  React.useEffect(() => {
    if (!conn.isActive) return;
    if (user) {
      setRestoring(false);
      return;
    }
    const stored = loadStoredCredentials();
    if (!stored) {
      setRestoring(false);
      return;
    }
    setRestoring(true);
    let cancelled = false;
    signInReducer({ email: stored.email, password: stored.password })
      .then(() => {
        if (!cancelled) {
          storeCredentials(stored);
          setFileEncryptionPassword(stored.password);
          setRestoring(false);
        }
      })
      .catch(() => {
        if (!cancelled) {
          clearStoredCredentials();
          setRestoring(false);
        }
      });
    return () => {
      cancelled = true;
    };
    // Re-run only when the connection identity changes, not on every render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [conn.isActive, connectionIdentityHex, user]);

  const status: AuthState["status"] = conn.connectionError
    ? "error"
    : conn.isActive
      ? "connected"
      : "connecting";

  const value: AuthContextValue = {
    status,
    identityHex: userIdentityHex,
    isAuthenticated: Boolean(user),
    email: user?.email ?? null,
    displayName: user?.displayName ?? null,
    role: user?.role ?? null,
    isAdmin: user?.role === "admin",
    emailVerified: user?.emailVerified ?? false,
    restoring,
    fileEncryptionKey,
    fileEncryptionError,
    error: conn.connectionError?.message,
    signIn: async (email, password) => {
      await signInReducer({ email, password });
      storeCredentials({ email, password });
      setFileEncryptionPassword(password);
    },
    signUp: async (email, password, displayName) => {
      await signUpReducer({ email, password, displayName: displayName ?? undefined });
      storeCredentials({ email, password });
      setFileEncryptionPassword(password);
    },
    updateLocalPassword: (password, fileKey) => {
      if (!user?.email) {
        throw new Error("Cannot update local password before sign in completes.");
      }
      storeCredentials({ email: user.email, password });
      setFileEncryptionPassword(password);
      setFileEncryptionError(undefined);
      if (fileKey) setFileEncryptionKey(fileKey);
    },
    signOut: async () => {
      try {
        await signOutReducer();
      } catch {
        // If the server-side sign-out fails (e.g. we were never signed in),
        // still drop the local connection.
      }
      try {
        conn.getConnection()?.disconnect();
      } catch {
        /* ignore */
      }
      clearStoredToken();
      clearStoredCredentials();
      setFileEncryptionPassword(null);
      setFileEncryptionKey(null);
      bump();
    },
    hardReset: () => {
      try {
        conn.getConnection()?.disconnect();
      } catch {
        /* ignore */
      }
      clearStoredToken();
      clearStoredCredentials();
      setFileEncryptionPassword(null);
      setFileEncryptionKey(null);
      bump();
    },
  };

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}
