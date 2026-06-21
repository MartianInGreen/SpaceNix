import * as React from "react";
import { Shield, ShieldAlert, Sparkles, X } from "lucide-react";

import { cn } from "@/lib/utils";
import { ChipList } from "@/components/common";
import { TagInput } from "@/components/tag-input";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";

export const PERMISSION_PATTERN = /^[A-Za-z0-9:._\-*]+$/;

const PERMISSION_SCOPES = [
  {
    scope: "files",
    label: "Files",
    description: "Browse, upload, rename, and remove synced files.",
    actions: [
      { action: "read", label: "Read" },
      { action: "write", label: "Write" },
      { action: "delete", label: "Delete" },
      { action: "*", label: "All files" },
    ],
  },
  {
    scope: "secrets",
    label: "Secrets",
    description: "List, reveal, create, and update environment secrets.",
    actions: [
      { action: "read", label: "Read/reveal" },
      { action: "write", label: "Create/update" },
      { action: "delete", label: "Delete" },
      { action: "*", label: "All secrets" },
    ],
  },
  {
    scope: "devices",
    label: "Devices",
    description: "Register, rename, and manage device records.",
    actions: [
      { action: "read", label: "Read" },
      { action: "write", label: "Register/update" },
      { action: "delete", label: "Delete" },
      { action: "*", label: "All devices" },
    ],
  },
  {
    scope: "sync",
    label: "Sync",
    description: "Manage local sync selections and automation.",
    actions: [
      { action: "read", label: "Read status" },
      { action: "write", label: "Change sync" },
      { action: "*", label: "All sync" },
    ],
  },
] as const;

type PermissionOverviewItem = {
  name: string;
  permissions: string[];
};

function normalizePermissions(values: string[]) {
  return [...new Set(values.map((v) => v.trim()).filter(Boolean))].sort();
}

function isBroadPermission(permission: string) {
  return permission === "*" || permission.endsWith(":*");
}

function permissionGrant(scope: string, action: string) {
  return `${scope}:${action}`;
}

function removeScopePermissions(values: string[], scope: string) {
  return values.filter((value) => !value.startsWith(`${scope}:`));
}

export function PermissionEditor({
  values,
  onChange,
  emptyLabel = "No permissions selected.",
}: {
  values: string[];
  onChange: (next: string[]) => void;
  emptyLabel?: string;
}) {
  const selected = normalizePermissions(values);
  const hasFullAccess = selected.includes("*");

  const setValues = (next: string[]) => onChange(normalizePermissions(next));
  const toggleFullAccess = () => setValues(hasFullAccess ? selected.filter((value) => value !== "*") : ["*"]);

  const toggleGrant = (scope: string, action: string) => {
    const grant = permissionGrant(scope, action);
    if (selected.includes(grant)) {
      setValues(selected.filter((value) => value !== grant));
      return;
    }

    if (action === "*") {
      setValues([...removeScopePermissions(selected.filter((value) => value !== "*"), scope), grant]);
      return;
    }

    setValues([...selected.filter((value) => value !== "*" && value !== permissionGrant(scope, "*")), grant]);
  };

  const activate = (event: React.KeyboardEvent, action: () => void, disabled?: boolean) => {
    if (disabled || (event.key !== "Enter" && event.key !== " ")) return;
    event.preventDefault();
    action();
  };

  return (
    <div className="space-y-4">
      <div className="rounded-lg border bg-muted/20 p-3">
        <div
          role="button"
          tabIndex={0}
          onClick={toggleFullAccess}
          onKeyDown={(event) => activate(event, toggleFullAccess)}
          className={cn(
            "flex w-full cursor-pointer items-start gap-3 rounded-md p-2 text-left transition-colors hover:bg-accent",
            hasFullAccess && "bg-accent/70"
          )}
        >
          <Checkbox
            checked={hasFullAccess}
            onClick={(event) => event.stopPropagation()}
            onCheckedChange={toggleFullAccess}
            className="mt-0.5"
          />
          <span className="min-w-0 flex-1">
            <span className="flex items-center gap-2 font-medium">
              Full access
              <Badge variant="warning">*</Badge>
            </span>
            <span className="mt-0.5 block text-sm text-muted-foreground">
              Grants every current and future permission. Use this only for trusted tokens or broad internal scopes.
            </span>
          </span>
        </div>
      </div>

      <div className="grid gap-3 sm:grid-cols-2">
        {PERMISSION_SCOPES.map((group) => {
          const scopeWildcard = permissionGrant(group.scope, "*");
          const scopeLocked = hasFullAccess || selected.includes(scopeWildcard);

          return (
            <div key={group.scope} className="rounded-lg border p-3">
              <div className="mb-3">
                <div className="font-medium">{group.label}</div>
                <div className="text-xs text-muted-foreground">{group.description}</div>
              </div>
              <div className="grid gap-2">
                {group.actions.map((item) => {
                  const grant = permissionGrant(group.scope, item.action);
                  const checked = hasFullAccess || selected.includes(grant);
                  const disabled = hasFullAccess || (scopeLocked && item.action !== "*");

                  const toggle = () => toggleGrant(group.scope, item.action);

                  return (
                    <div
                      key={grant}
                      role="button"
                      tabIndex={disabled ? -1 : 0}
                      aria-disabled={disabled}
                      onClick={() => {
                        if (!disabled) toggle();
                      }}
                      onKeyDown={(event) => activate(event, toggle, disabled)}
                      className={cn(
                        "flex items-center gap-2 rounded-md border px-2 py-1.5 text-left text-sm transition-colors hover:bg-accent",
                        disabled ? "cursor-not-allowed opacity-55" : "cursor-pointer",
                        checked && "border-primary/50 bg-primary/5"
                      )}
                    >
                      <Checkbox
                        checked={checked}
                        disabled={disabled}
                        onClick={(event) => event.stopPropagation()}
                        onCheckedChange={toggle}
                      />
                      <span className="flex-1">{item.label}</span>
                      <code className="font-mono text-[11px] text-muted-foreground">{grant}</code>
                    </div>
                  );
                })}
              </div>
            </div>
          );
        })}
      </div>

      <div className="space-y-2">
        <div className="flex items-center justify-between gap-3">
          <div>
            <div className="text-sm font-medium">Selected grants</div>
            <div className="text-xs text-muted-foreground">Add a custom grant if the preset list does not cover it.</div>
          </div>
          {selected.length > 0 ? (
            <Button type="button" variant="ghost" size="sm" className="h-7 gap-1" onClick={() => onChange([])}>
              <X className="size-3.5" /> Clear
            </Button>
          ) : null}
        </div>
        <TagInput
          values={selected}
          onChange={setValues}
          pattern={PERMISSION_PATTERN}
          placeholder="custom:grant"
          emptyLabel={emptyLabel}
        />
      </div>
    </div>
  );
}

export function PermissionOverview({
  items,
  emptyItemsLabel = "Items without grants",
}: {
  items: PermissionOverviewItem[];
  emptyItemsLabel?: string;
}) {
  const stats = React.useMemo(() => {
    const counts = new Map<string, number>();
    let broad = 0;
    let empty = 0;

    for (const item of items) {
      if (item.permissions.length === 0) empty += 1;
      for (const permission of item.permissions) {
        counts.set(permission, (counts.get(permission) ?? 0) + 1);
        if (isBroadPermission(permission)) broad += 1;
      }
    }

    const permissions = [...counts.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
    return {
      broad,
      empty,
      permissions,
      unique: permissions.length,
      total: items.reduce((sum, item) => sum + item.permissions.length, 0),
    };
  }, [items]);

  return (
    <div className="mb-5 grid gap-3 lg:grid-cols-3">
      <Card>
        <CardHeader className="pb-3">
          <CardTitle className="flex items-center gap-2 text-sm">
            <Shield className="size-4 text-primary" /> Permission overview
          </CardTitle>
          <CardDescription>{items.length} scoped item{items.length === 1 ? "" : "s"}</CardDescription>
        </CardHeader>
        <CardContent className="grid grid-cols-3 gap-2 text-center">
          <div className="rounded-md bg-muted/40 p-2">
            <div className="text-lg font-semibold">{stats.unique}</div>
            <div className="text-[11px] text-muted-foreground">Unique</div>
          </div>
          <div className="rounded-md bg-muted/40 p-2">
            <div className="text-lg font-semibold">{stats.total}</div>
            <div className="text-[11px] text-muted-foreground">Assigned</div>
          </div>
          <div className="rounded-md bg-muted/40 p-2">
            <div className="text-lg font-semibold">{stats.empty}</div>
            <div className="text-[11px] text-muted-foreground">Empty</div>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="pb-3">
          <CardTitle className="flex items-center gap-2 text-sm">
            <ShieldAlert className="size-4 text-amber-500" /> Broad grants
          </CardTitle>
          <CardDescription>Wildcards that cover many actions</CardDescription>
        </CardHeader>
        <CardContent>
          {stats.broad === 0 ? (
            <p className="text-sm text-muted-foreground">No wildcard grants in use.</p>
          ) : (
            <div className="flex items-center gap-2">
              <Badge variant="warning">{stats.broad} wildcard grant{stats.broad === 1 ? "" : "s"}</Badge>
              <span className="text-xs text-muted-foreground">Review regularly.</span>
            </div>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="pb-3">
          <CardTitle className="flex items-center gap-2 text-sm">
            <Sparkles className="size-4 text-primary" /> Most used
          </CardTitle>
          <CardDescription>{stats.empty} {emptyItemsLabel.toLowerCase()}</CardDescription>
        </CardHeader>
        <CardContent>
          {stats.permissions.length === 0 ? (
            <p className="text-sm text-muted-foreground">No permissions assigned yet.</p>
          ) : (
            <div className="space-y-2">
              {stats.permissions.slice(0, 4).map(([permission, count]) => (
                <div key={permission} className="flex items-center justify-between gap-3 text-sm">
                  <code className="truncate font-mono text-xs">{permission}</code>
                  <Badge variant="secondary">{count}</Badge>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}

export function PermissionChips({ permissions }: { permissions: string[] }) {
  return <ChipList items={permissions} empty="No grants" />;
}
