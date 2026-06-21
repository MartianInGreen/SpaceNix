import * as React from "react";
import { Shield, X } from "lucide-react";

import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { TagInput } from "@/components/tag-input";

const GRANT_PATTERN = /^(secrets:[a-z0-9_*-]+|secrets:\*)$/;

const PRESETS: { grant: string; label: string; description: string }[] = [
  {
    grant: "secrets:read",
    label: "Read/reveal",
    description: "Token can read this secret's value.",
  },
  {
    grant: "secrets:write",
    label: "Create/update",
    description: "Token can change this secret's value.",
  },
  {
    grant: "secrets:*",
    label: "All secrets",
    description: "Token gets full access to this secret.",
  },
];

function normalize(values: string[]) {
  return [...new Set(values.map((v) => v.trim()).filter(Boolean))].sort();
}

export function SecretPermissionGate({
  values,
  onChange,
  collapsedLabel = "Add a permission gate",
}: {
  values: string[];
  onChange: (next: string[]) => void;
  collapsedLabel?: string;
}) {
  const selected = React.useMemo(() => normalize(values), [values]);
  const [enabled, setEnabled] = React.useState(selected.length > 0);
  const [custom, setCustom] = React.useState("");

  React.useEffect(() => {
    if (selected.length > 0 && !enabled) setEnabled(true);
  }, [selected.length, enabled]);

  const setSelected = (next: string[]) => onChange(normalize(next));

  const togglePreset = (grant: string) => {
    setSelected(
      selected.includes(grant) ? selected.filter((v) => v !== grant) : [...selected, grant]
    );
  };

  const addCustom = () => {
    const v = custom.trim();
    if (!v) return;
    if (!GRANT_PATTERN.test(v)) return;
    if (selected.includes(v)) {
      setCustom("");
      return;
    }
    setSelected([...selected, v]);
    setCustom("");
  };

  if (!enabled) {
    return (
      <div className="rounded-md border border-dashed bg-muted/20 p-3">
        <div className="flex items-center justify-between gap-3">
          <div className="flex min-w-0 items-center gap-2 text-sm text-muted-foreground">
            <Shield className="size-3.5" />
            <span>No permission gate. Any matching device scope can receive this secret.</span>
          </div>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="h-7 gap-1"
            onClick={() => setEnabled(true)}
          >
            {collapsedLabel}
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="space-y-3 rounded-md border p-3">
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-2 text-sm font-medium">
          <Shield className="size-3.5 text-primary" />
          Permission gate
        </div>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="h-7 gap-1"
          onClick={() => {
            setEnabled(false);
            setSelected([]);
          }}
        >
          <X className="size-3.5" /> Remove
        </Button>
      </div>

      <div className="grid gap-1.5 sm:grid-cols-3">
        {PRESETS.map((p) => {
          const active = selected.includes(p.grant);
          return (
            <button
              key={p.grant}
              type="button"
              onClick={() => togglePreset(p.grant)}
              className={cn(
                "rounded-md border px-2 py-1.5 text-left text-xs transition-colors hover:bg-accent",
                active && "border-primary/50 bg-primary/5"
              )}
            >
              <div className="flex items-center justify-between gap-1.5">
                <span className="font-medium">{p.label}</span>
                <code className="font-mono text-[10px] text-muted-foreground">{p.grant}</code>
              </div>
              <p className="mt-0.5 text-[11px] text-muted-foreground">{p.description}</p>
            </button>
          );
        })}
      </div>

      <div className="flex items-center gap-2">
        <Input
          value={custom}
          onChange={(e) => setCustom(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              addCustom();
            }
          }}
          placeholder="secrets:custom-grant"
          className="h-8 font-mono text-xs"
        />
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="h-8"
          onClick={addCustom}
          disabled={!GRANT_PATTERN.test(custom.trim())}
        >
          Add
        </Button>
      </div>

      {selected.length > 0 ? (
        <TagInput
          values={selected}
          onChange={setSelected}
          pattern={GRANT_PATTERN}
          placeholder=""
          emptyLabel=""
        />
      ) : (
        <p className="text-[11px] text-muted-foreground">
          Empty gate — no token is restricted from this secret.
        </p>
      )}
    </div>
  );
}
