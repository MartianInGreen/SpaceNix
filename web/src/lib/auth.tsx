import * as React from "react";

import { useSpacetimeDB, useTable, useReducer } from "spacetimedb/react";
import { reducers, tables } from "@/module_bindings";

import { identityHex } from "@/lib/stdb";

export interface AuthState {
  status: "connecting" | "connected" | "error";
  identityHex: string;
  isAuthenticated: boolean;
  email: string | null;
  displayName: string | null;
  role: string | null;
  isAdmin: boolean;
  error?: string;
}

export interface AuthContextValue extends AuthState {
  signIn: (email: string, password: string) => Promise<void>;
  signUp: (
    email: string,
    password: string,
    displayName?: string
  ) => Promise<void>;
  signOut: () => Promise<void>;
  /** Force a hard disconnect and reconnect with a fresh anonymous identity. */
  hardReset: () => void;
}

const AuthContext = React.createContext<AuthContextValue | null>(null);

/**
 * Holds a session nonce used to force the SpacetimeDBProvider to remount with a
 * fresh anonymous identity on sign-out. Lives above the connection provider so
 * it does not depend on an active connection.
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

  const signInReducer = useReducer(reducers.signIn);
  const signUpReducer = useReducer(reducers.signUp);
  const signOutReducer = useReducer(reducers.signOut);

  const user = rows[0];
  const identityHexVal = identityHex(conn.identity);

  const status: AuthState["status"] = conn.connectionError
    ? "error"
    : conn.isActive
      ? "connected"
      : "connecting";

  const value: AuthContextValue = {
    status,
    identityHex: identityHexVal,
    isAuthenticated: Boolean(user),
    email: user?.email ?? null,
    displayName: user?.displayName ?? null,
    role: user?.role ?? null,
    isAdmin: user?.role === "admin",
    error: conn.connectionError?.message,
    signIn: async (email, password) => {
      await signInReducer({ email, password });
    },
    signUp: async (email, password, displayName) => {
      await signUpReducer({ email, password, displayName: displayName ?? undefined });
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
      bump();
    },
    hardReset: () => {
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
