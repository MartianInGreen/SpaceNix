import * as React from "react";
import { Link, NavLink, Outlet, useLocation } from "react-router-dom";
import { useReducer } from "spacetimedb/react";
import {
  Cloud,
  FilesIcon,
  KeyRound,
  Laptop,
  LogOut,
  Moon,
  Sun,
  Terminal,
  UserRound,
} from "lucide-react";

import { cn } from "@/lib/utils";
import { reducers } from "@/module_bindings";
import { reportError, reportSuccess } from "@/lib/toast";
import { useAuth } from "@/lib/auth-context";
import { useTheme } from "@/lib/use-theme";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Separator } from "@/components/ui/separator";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";

const NAV = [
  { to: "/files", label: "Files", icon: FilesIcon },
  { to: "/secrets", label: "Secrets", icon: KeyRound },
  { to: "/ssh", label: "SSH", icon: Terminal },
  { to: "/pats", label: "PATs", icon: KeyRound },
  { to: "/devices", label: "Devices", icon: Laptop },
  { to: "/account", label: "Account", icon: UserRound },
] as const;

export function AppShell() {
  const { displayName, email, identityHex, role, signOut } = useAuth();
  const { theme, toggle } = useTheme();
  const location = useLocation();
  const sendUiEvent = useReducer(reducers.sendUiEvent);

  const sendCurrentPageToTui = async () => {
    const screen = routeToTuiScreen(location.pathname);
    try {
      await sendUiEvent({
        targetDeviceId: undefined,
        kind: "screen:open",
        payloadJson: JSON.stringify({ screen }),
      });
      reportSuccess(`Sent ${screen} to TUI.`);
    } catch (err) {
      reportError(err);
    }
  };

  return (
    <div className="flex h-full w-full">
      <aside className="hidden w-60 shrink-0 flex-col border-r bg-sidebar text-sidebar-foreground md:flex">
        <div className="flex h-14 items-center gap-2 border-b px-4">
          <Cloud className="size-5" />
          <span className="font-semibold tracking-tight">SpaceNix</span>
        </div>
        <nav className="flex flex-1 flex-col gap-1 p-3">
          {NAV.map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              className={({ isActive }) =>
                cn(
                  "flex items-center gap-2 rounded-md px-3 py-2 text-sm font-medium transition-colors",
                  isActive
                    ? "bg-sidebar-accent text-sidebar-accent-foreground"
                    : "text-sidebar-foreground/70 hover:bg-sidebar-accent/60 hover:text-sidebar-accent-foreground"
                )
              }
            >
              <item.icon className="size-4" />
              {item.label}
            </NavLink>
          ))}
        </nav>
        <Separator />
        <div className="p-3">
          <div className="rounded-md border bg-card p-3 text-xs">
            <div className="flex items-center justify-between">
              <span className="text-muted-foreground">Signed in</span>
              <Badge variant={role === "admin" ? "default" : "secondary"}>{role ?? "user"}</Badge>
            </div>
            <div className="mt-1 truncate font-medium">{displayName ?? email ?? "anonymous"}</div>
            {email ? (
              <div className="mt-1 truncate text-[11px] text-muted-foreground">
                {email}
              </div>
            ) : null}
            <div className="mt-1 truncate font-mono text-[10px] text-muted-foreground">
              {identityHex}
            </div>
          </div>
        </div>
      </aside>

      <div className="flex flex-1 flex-col overflow-hidden">
        <header className="flex h-14 shrink-0 items-center gap-2 border-b px-4">
          <MobileNav current={location.pathname} />
          <div className="flex-1" />
          <Button variant="ghost" size="icon" onClick={toggle} aria-label="Toggle theme">
            {theme === "dark" ? <Sun /> : <Moon />}
          </Button>
          <Button variant="ghost" size="sm" className="gap-2" onClick={sendCurrentPageToTui}>
            <Terminal className="size-4" />
            <span className="hidden sm:inline">Open in TUI</span>
          </Button>
          <AlertDialog>
            <AlertDialogTrigger asChild>
              <Button variant="ghost" size="sm" className="gap-2">
                <LogOut className="size-4" />
                <span className="hidden sm:inline">Logout</span>
              </Button>
            </AlertDialogTrigger>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>Log out?</AlertDialogTitle>
                <AlertDialogDescription>
                  This disconnects from SpacetimeDB and ends your current session on this device.
                  Your data stays on the server, tied to your account.
                </AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>Cancel</AlertDialogCancel>
                <AlertDialogAction onClick={() => void signOut()}>Log out</AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        </header>

        <main className="flex-1 overflow-y-auto">
          <div className="mx-auto w-full max-w-6xl p-4 md:p-6">
            <Outlet />
          </div>
        </main>
      </div>
    </div>
  );
}

function MobileNav({ current }: { current: string }) {
  return (
    <div className="flex items-center gap-1 md:hidden">
      <span className="font-semibold tracking-tight">SpaceNix</span>
      <Badge variant="secondary" className="ml-2 font-mono text-[10px]">
        {current.replace("/", "") || "home"}
      </Badge>
    </div>
  );
}

export { Link };

function routeToTuiScreen(pathname: string): string {
  const segment = pathname.split("/").filter(Boolean)[0] ?? "files";
  if (segment === "ssh") return "ssh_keys";
  if (segment === "pats") return "tokens";
  return segment;
}
