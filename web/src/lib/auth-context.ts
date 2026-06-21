import * as React from "react";

export interface AuthState {
  status: "connecting" | "connected" | "error";
  identityHex: string;
  isAuthenticated: boolean;
  email: string | null;
  displayName: string | null;
  role: string | null;
  isAdmin: boolean;
  emailVerified: boolean;
  /** True while a stored-credential auto sign-in is being replayed. */
  restoring: boolean;
  fileEncryptionKey: CryptoKey | null;
  fileEncryptionError?: string;
  error?: string;
}

export interface AuthContextValue extends AuthState {
  signIn: (email: string, password: string) => Promise<void>;
  signUp: (
    email: string,
    password: string,
    displayName?: string
  ) => Promise<void>;
  updateLocalPassword: (password: string, fileKey?: CryptoKey) => void;
  signOut: () => Promise<void>;
  /** Force a hard disconnect and reconnect with a fresh anonymous identity. */
  hardReset: () => void;
}

export const AuthContext = React.createContext<AuthContextValue | null>(null);

export const SessionKeyContext = React.createContext<{
  nonce: number;
  bump: () => void;
} | null>(null);

export function useSessionKey() {
  const ctx = React.useContext(SessionKeyContext);
  if (!ctx) throw new Error("useSessionKey must be used within SessionKeyProvider");
  return ctx;
}

export function useAuth(): AuthContextValue {
  const ctx = React.useContext(AuthContext);
  if (!ctx) throw new Error("useAuth must be used within AuthProvider");
  return ctx;
}
