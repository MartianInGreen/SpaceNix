import * as React from "react";
import { useTable, useReducer } from "spacetimedb/react";
import { Link, useNavigate, useSearchParams } from "react-router-dom";
import {
  ArrowLeft,
  CircleAlert,
  Loader2,
  PowerOff,
  Server,
  Terminal as TerminalIcon,
} from "lucide-react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";

import { reducers, tables } from "@/module_bindings";
import { Spinner } from "@/components/common";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { reportError, reportSuccess } from "@/lib/toast";

type Status = "idle" | "preparing" | "connecting" | "connected" | "closed" | "error";

function buildWsUrl(listenUrl: string, sessionId: string, token: string): string {
  const trimmed = listenUrl.trim().replace(/\/+$/, "");
  // Accept ws://, wss://, http:// (→ ws://), https:// (→ wss://),
  // or a bare host:port (→ ws://).
  const proto = trimmed.startsWith("https://")
    ? "wss://"
    : trimmed.startsWith("http://")
      ? "ws://"
      : trimmed.startsWith("wss://") || trimmed.startsWith("ws://")
        ? ""
        : "ws://";
  const host = proto === "" ? trimmed : trimmed.replace(/^(https?|wss?):\/\//, "");
  const prefix = proto === "" ? "" : proto;
  return `${prefix}${host}/ssh/sessions/${encodeURIComponent(sessionId)}?token=${encodeURIComponent(token)}`;
}

export function TerminalPage() {
  const [searchParams] = useSearchParams();
  const navigate = useNavigate();
  const endpointId = searchParams.get("endpoint") ?? "";

  const [sessions] = useTable(tables.my_ssh_relay_sessions);
  const [relayDeviceRows] = useTable(tables.my_ssh_relay_device);
  const relayDevice = relayDeviceRows[0];
  const closeSshRelaySession = useReducer(reducers.closeSshRelaySession);

  // Pick the most recent non-Closed session for the requested
  // endpoint. (New sessions always have the largest id.)
  const session = React.useMemo(() => {
    const filtered = sessions
      .filter((s) => String(s.endpointId) === endpointId)
      .filter((s) => (s.status as { tag: string }).tag !== "Closed")
      .sort((a, b) => Number(b.id - a.id));
    return filtered[0];
  }, [sessions, endpointId]);

  const containerRef = React.useRef<HTMLDivElement | null>(null);
  const termRef = React.useRef<XTerm | null>(null);
  const fitRef = React.useRef<FitAddon | null>(null);
  const wsRef = React.useRef<WebSocket | null>(null);
  const keepaliveRef = React.useRef<number | null>(null);

  const [status, setStatus] = React.useState<Status>("idle");
  const [error, setError] = React.useState<string | null>(null);

  // Spin up xterm.js once on mount.
  React.useEffect(() => {
    if (!containerRef.current) return;
    const term = new XTerm({
      fontFamily:
        'ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace',
      fontSize: 13,
      cursorBlink: true,
      convertEol: true,
      theme:
        document.documentElement.classList.contains("dark")
          ? undefined
          : { background: "#ffffff" },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(containerRef.current);
    fit.fit();
    termRef.current = term;
    fitRef.current = fit;

    const onResize = () => {
      try {
        fit.fit();
      } catch {
        // ignore
      }
    };
    window.addEventListener("resize", onResize);
    const ro = new ResizeObserver(onResize);
    ro.observe(containerRef.current);
    return () => {
      window.removeEventListener("resize", onResize);
      ro.disconnect();
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
    };
  }, []);

  // If we have a session but no token yet, the relay device is still
  // minting one. Wait — the subscription will update.
  React.useEffect(() => {
    if (!session) return;
    if (session.authToken) {
      setError(null);
    } else if (status !== "connecting" && status !== "connected") {
      setStatus("preparing");
      setError(
        "Waiting for the relay device to mint a session token. Make sure `spacenix service start` is running on the chosen relay device.",
      );
    }
  }, [session, session?.authToken, status]);

  // Open the WebSocket once we have everything we need.
  React.useEffect(() => {
    if (!session || !session.authToken) return;
    if (!relayDevice?.listenUrl) {
      if (status !== "error") {
        setStatus("error");
        setError(
          "No relay listen URL is set. On the Devices page, mark a device as the SSH relay and set its address (e.g. ws://laptop.lan:7770).",
        );
      }
      return;
    }
    if (status === "connecting" || status === "connected") return;

    setStatus("connecting");
    setError(null);

    const url = buildWsUrl(relayDevice.listenUrl, String(session.id), session.authToken);
    let ws: WebSocket;
    try {
      ws = new WebSocket(url);
    } catch (err) {
      setStatus("error");
      setError(
        err instanceof Error ? err.message : "Failed to construct WebSocket URL",
      );
      return;
    }
    ws.binaryType = "arraybuffer";
    wsRef.current = ws;

    const term = termRef.current;
    if (!term) return;

    ws.addEventListener("open", () => {
      setStatus("connected");
      if (term) term.writeln("\x1b[2m── connected ─────────────────────────────\x1b[0m");
    });
    ws.addEventListener("message", (ev) => {
      const data =
        ev.data instanceof ArrayBuffer
          ? new Uint8Array(ev.data)
          : typeof ev.data === "string"
            ? new TextEncoder().encode(ev.data)
            : null;
      if (data) {
        term.write(data);
      }
    });
    ws.addEventListener("close", (ev) => {
      setStatus("closed");
      term.writeln(
        `\x1b[2m── session closed (code ${ev.code}) ─────────────\x1b[0m`,
      );
      if (keepaliveRef.current != null) {
        window.clearInterval(keepaliveRef.current);
        keepaliveRef.current = null;
      }
    });
    ws.addEventListener("error", () => {
      setStatus("error");
      setError(
        "WebSocket error. Check that the relay is reachable from this browser (same network or Tailscale) and that the listen URL is correct.",
      );
    });

    // Forward xterm input → ws binary frames.
    const dataDisp = term.onData((data) => {
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(new TextEncoder().encode(data));
      }
    });
    keepaliveRef.current = window.setInterval(() => {
      if (ws.readyState === WebSocket.OPEN) {
        try {
          ws.send(new Uint8Array(0));
        } catch {
          // ignore
        }
      }
    }, 30_000);

    return () => {
      dataDisp.dispose();
      if (keepaliveRef.current != null) {
        window.clearInterval(keepaliveRef.current);
        keepaliveRef.current = null;
      }
      try {
        ws.close(1000, "client closing");
      } catch {
        // ignore
      }
      wsRef.current = null;
    };
    // We intentionally only re-run the effect when the token, the
    // listen URL, or the session id change. The `status` and
    // `session` state changes are downstream of these and would
    // cause us to reconnect mid-session.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [session?.authToken, relayDevice?.listenUrl, session?.id]);

  const sessionGone = !!endpointId && !session && status === "idle";

  const endSession = async () => {
    if (!session) return;
    try {
      await closeSshRelaySession({ sessionId: session.id });
      reportSuccess("Session closed.");
      navigate("/ssh");
    } catch (err) {
      reportError(err);
    }
  };

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-3 border-b px-4 py-3">
        <Button variant="ghost" size="icon" asChild aria-label="Back to SSH">
          <Link to="/ssh">
            <ArrowLeft className="size-4" />
          </Link>
        </Button>
        <TerminalIcon className="size-4 text-muted-foreground" />
        <div className="flex-1">
          <h1 className="text-base font-semibold">Browser SSH terminal</h1>
          <p className="text-xs text-muted-foreground">
            {session ? (
              <>
                Session #{String(session.id)} · endpoint #{String(session.endpointId)} ·
                status {status}
              </>
            ) : endpointId ? (
              <>Waiting for a session on endpoint #{endpointId}…</>
            ) : (
              "No endpoint selected."
            )}
          </p>
        </div>
        <Button variant="outline" size="sm" onClick={endSession} disabled={!session}>
          <PowerOff className="mr-2 size-4" />
          End session
        </Button>
      </div>

      <div className="grid flex-1 grid-cols-1 gap-3 overflow-hidden p-3 lg:grid-cols-[1fr_280px]">
        <Card className="flex flex-col overflow-hidden p-0">
          <CardContent className="flex-1 overflow-hidden p-0">
            <div
              ref={containerRef}
              className="h-full min-h-[60vh] w-full overflow-hidden bg-black p-2"
            />
          </CardContent>
        </Card>

        <div className="flex flex-col gap-3">
          <Card>
            <CardHeader>
              <CardTitle className="text-sm">Relay</CardTitle>
              <CardDescription className="text-xs">
                The device running <code className="font-mono">spacenix service start</code>{" "}
                that will spawn <code className="font-mono">ssh(1)</code> on your behalf.
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-2 text-xs">
              <div className="rounded-md border bg-muted/40 p-2 font-mono">
                {relayDevice ? (
                  <>
                    device #{String(relayDevice.deviceId)}
                    <br />
                    <span className="text-muted-foreground">
                      {relayDevice.listenUrl ?? "no listen URL set"}
                    </span>
                  </>
                ) : (
                  <span className="text-muted-foreground">none set</span>
                )}
              </div>
              <p className="text-muted-foreground">
                Set the relay on the{" "}
                <Link to="/devices" className="underline">
                  Devices page
                </Link>
                .
              </p>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle className="text-sm">Status</CardTitle>
            </CardHeader>
            <CardContent className="space-y-2 text-xs">
              <div className="flex items-center gap-2">
                {status === "connecting" ? (
                  <Loader2 className="size-3.5 animate-spin" />
                ) : status === "connected" ? (
                  <Server className="size-3.5 text-emerald-500" />
                ) : status === "error" ? (
                  <CircleAlert className="size-3.5 text-destructive" />
                ) : status === "preparing" ? (
                  <Spinner className="size-3.5" />
                ) : (
                  <span className="size-3.5 rounded-full bg-muted-foreground/40" />
                )}
                <span className="font-mono">{status}</span>
              </div>
              {error ? (
                <p className="text-destructive">{error}</p>
              ) : sessionGone ? (
                <p className="text-muted-foreground">
                  The session has been closed or expired. Return to the SSH page and try again.
                </p>
              ) : null}
            </CardContent>
          </Card>
        </div>
      </div>
    </div>
  );
}
