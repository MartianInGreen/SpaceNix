import { DbConnection } from "@/module_bindings";
import type { Identity } from "spacetimedb";
const env = (import.meta as { env?: Record<string, string | undefined> }).env ?? {};

export const STDB_URI = env.VITE_STDB_URI ?? "http://127.0.0.1:3000";
export const STDB_MODULE = env.VITE_STDB_MODULE ?? "spacenix-9wfd4";

const TOKEN_KEY = "spacenix.stdb.token";
const CREDS_KEY = "spacenix.stdb.credentials";
const CALLBACK_KEY = "spacenix.cli.callback";

export interface PendingCallback {
  /** Local URL the TUI is waiting on, e.g. `http://127.0.0.1:7711/oauth/callback`. */
  url: string;
}

export function readPendingCallback(): PendingCallback | undefined {
  try {
    const raw = sessionStorage.getItem(CALLBACK_KEY);
    if (!raw) return undefined;
    const parsed = JSON.parse(raw) as Partial<PendingCallback>;
    if (typeof parsed.url === "string" && parsed.url.length > 0) {
      return { url: parsed.url };
    }
    return undefined;
  } catch {
    return undefined;
  }
}

export function storePendingCallback(cb: PendingCallback | undefined) {
  try {
    if (cb && cb.url) {
      sessionStorage.setItem(CALLBACK_KEY, JSON.stringify(cb));
    } else {
      sessionStorage.removeItem(CALLBACK_KEY);
    }
  } catch {
    /* ignore */
  }
}

export function clearPendingCallback() {
  storePendingCallback(undefined);
}

/**
 * Build the redirect URL the browser should navigate to after a successful
 * sign-in completes inside the TUI's `?callback=` flow. We attach the
 * current connection token and identity hex as query parameters so the
 * local server the TUI is running can pick them up.
 */
export function buildCallbackRedirectUrl(
  callbackUrl: string,
  token: string,
  identity: string,
): string {
  const url = new URL(callbackUrl);
  // The TUI's local server reads `?token=…&identity=…`. We do not use
  // `URLSearchParams` here because we want to keep things simple and the
  // inputs are well-formed.
  url.searchParams.set("token", token);
  if (identity) {
    url.searchParams.set("identity", identity);
  }
  return url.toString();
}

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
