import * as React from "react";

import { useSpacetimeDB, useTable, useReducer } from "spacetimedb/react";
import { reducers, tables } from "@/module_bindings";

import { clearStoredToken, identityHex, storeToken } from "@/lib/stdb";

export interface AuthState {
  status: "connecting" | "connected" | "error";
  identityHex: string;
  isAuthenticated: boolean;
  displayName: string | null;
  error?: string;
}

export interface AuthContextValue extends AuthState {
  signUp: (displayName?: string) => Promise<void>;
  signIn: () => Promise<void>;
  logout: () => void;
}

const AuthContext = React.createContext<AuthContextValue | null>(null);

/**
 * Holds a session nonce used to force the SpacetimeDBProvider to remount with a
 * fresh anonymous identity on logout. Lives above the connection provider so it
 * does not depend on an active connection.
 */
const SessionKeyContext = React.createContext<{
  nonce: number;
  bump: () => void;
} | null>(null);

export function SessionKeyProvider({ children }: { children: React.ReactNode }) {
  const [nonce, setNonce] = React.useState(0);
  const value = React.useMemo(
    () => ({ nonce, bump: () => setNonce((n) => n + 1) }),
    [nonce]
  );
  return <SessionKeyContext.Provider value={value}>{children}</SessionKeyContext.Provider>;
}

export function useSessionKey() {
  const ctx = React.useContext(SessionKeyContext);
  if (!ctx) throw new Error("useSessionKey must be used within SessionKeyProvider");
  return ctx;
}

export function AuthProvider({ children }: { children: React.ReactNode }) {
  const conn = useSpacetimeDB();
  const [rows] = useTable(tables.my_user);
  const { bump } = useSessionKey();

  const signUp = useReducer(reducers.signUp);
  const signIn = useReducer(reducers.signIn);

  const user = rows[0];
  const identityHexVal = identityHex(conn.identity);

  React.useEffect(() => {
    if (conn.token) storeToken(conn.token);
  }, [conn.token]);

  const status: AuthState["status"] = conn.connectionError
    ? "error"
    : conn.isActive
      ? "connected"
      : "connecting";

  const value: AuthContextValue = {
    status,
    identityHex: identityHexVal,
    isAuthenticated: Boolean(user),
    displayName: user?.displayName ?? null,
    error: conn.connectionError?.message,
    signUp: async (displayName?: string) => {
      await signUp({ displayName: displayName });
    },
    signIn: async () => {
      await signIn();
    },
    logout: () => {
      clearStoredToken();
      try {
        conn.getConnection()?.disconnect();
      } catch {
        /* ignore */
      }
      bump();
    },
  };

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuth(): AuthContextValue {
  const ctx = React.useContext(AuthContext);
  if (!ctx) throw new Error("useAuth must be used within AuthProvider");
  return ctx;
}
