import { DbConnection } from "@/module_bindings";
import type { Identity } from "spacetimedb";
const env = (import.meta as { env?: Record<string, string | undefined> }).env ?? {};

export const STDB_URI = env.VITE_STDB_URI ?? "http://127.0.0.1:3000";
export const STDB_MODULE = env.VITE_STDB_MODULE ?? "spacenix-9wfd4";

const TOKEN_KEY = "spacenix.stdb.token";
const CREDS_KEY = "spacenix.stdb.credentials";

export function loadStoredToken(): string | undefined {
  try {
    const t = localStorage.getItem(TOKEN_KEY);
    return t && t.length > 0 ? t : undefined;
  } catch {
    return undefined;
  }
}

export function storeToken(token: string | undefined) {
  try {
    if (token && token.length > 0) {
      localStorage.setItem(TOKEN_KEY, token);
    } else {
      localStorage.removeItem(TOKEN_KEY);
    }
  } catch {
    /* ignore */
  }
}

export function clearStoredToken() {
  storeToken(undefined);
}

export interface StoredCredentials {
  email: string;
  password: string;
}

export function loadStoredCredentials(): StoredCredentials | undefined {
  try {
    const raw = localStorage.getItem(CREDS_KEY);
    if (!raw) return undefined;
    const parsed = JSON.parse(raw) as Partial<StoredCredentials>;
    if (typeof parsed.email === "string" && typeof parsed.password === "string") {
      return { email: parsed.email, password: parsed.password };
    }
    return undefined;
  } catch {
    return undefined;
  }
}

export function storeCredentials(creds: StoredCredentials | undefined) {
  try {
    if (creds) {
      localStorage.setItem(CREDS_KEY, JSON.stringify(creds));
    } else {
      localStorage.removeItem(CREDS_KEY);
    }
  } catch {
    /* ignore */
  }
}

export function clearStoredCredentials() {
  storeCredentials(undefined);
}

export function identityHex(id: Identity | undefined): string {
  if (!id) return "";
  try {
    return id.toHexString();
  } catch {
    try {
      return (id as any).toString();
    } catch {
      return "";
    }
  }
}

/**
 * SpacetimeDB procedure return values are encoded as a Result sum on the wire
 * and decode to `{ ok: T } | { err: E }` at runtime, but the generated TS type
 * is a loose union. This helper narrows the runtime shape and throws on errors.
 */
export type StdbResult<T> = { ok: T } | { err: string };

export function unwrap<T>(r: unknown): T {
  if (r && typeof r === "object" && "err" in (r as Record<string, unknown>)) {
    throw new Error(String((r as { err: unknown }).err ?? "procedure failed"));
  }
  if (r && typeof r === "object" && "ok" in (r as Record<string, unknown>)) {
    return (r as { ok: T }).ok;
  }
  return r as T;
}

export { DbConnection };
