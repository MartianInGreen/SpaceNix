import * as React from "react";
import { X, Plus } from "lucide-react";

import { cn } from "@/lib/utils";
import { Input } from "@/components/ui/input";

export function TagInput({
  values,
  onChange,
  placeholder = "Add and press Enter",
  emptyLabel,
  pattern,
  disabled,
}: {
  values: string[];
  onChange: (next: string[]) => void;
  placeholder?: string;
  emptyLabel?: string;
  /** Regex that each value must fully match, or undefined to allow anything. */
  pattern?: RegExp;
  disabled?: boolean;
}) {
  const [draft, setDraft] = React.useState("");
  const [error, setError] = React.useState<string | null>(null);

  const add = () => {
    const v = draft.trim();
    if (!v) return;
    if (pattern && !pattern.test(v)) {
      setError("Invalid characters.");
      return;
    }
    if (values.includes(v)) {
      setDraft("");
      setError(null);
      return;
    }
    onChange([...values, v]);
    setDraft("");
    setError(null);
  };

  return (
    <div className={cn("rounded-md border p-2", disabled && "opacity-60")}>
      <div className="flex flex-wrap gap-1.5">
        {values.length === 0 && emptyLabel ? (
          <span className="py-1 text-xs text-muted-foreground">{emptyLabel}</span>
        ) : null}
        {values.map((v) => (
          <span
            key={v}
            className="inline-flex items-center gap-1 rounded-md border bg-muted/40 px-1.5 py-0.5 font-mono text-[11px]"
          >
            {v}
            {!disabled && (
              <button
                type="button"
                className="text-muted-foreground hover:text-destructive"
                onClick={() => onChange(values.filter((x) => x !== v))}
                aria-label={`Remove ${v}`}
              >
                <X className="size-3" />
              </button>
            )}
          </span>
        ))}
        <Input
          value={draft}
          onChange={(e) => {
            setDraft(e.target.value);
            setError(null);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === ",") {
              e.preventDefault();
              add();
            } else if (e.key === "Backspace" && !draft && values.length > 0 && !disabled) {
              onChange(values.slice(0, -1));
            }
          }}
          onBlur={add}
          placeholder={placeholder}
          disabled={disabled}
          className="h-7 min-w-[140px] flex-1 border-0 bg-transparent px-1 shadow-none focus-visible:ring-0"
        />
      </div>
      {error ? <p className="mt-1 text-xs text-destructive">{error}</p> : null}
      <p className="mt-1 text-[11px] text-muted-foreground">
        Press <kbd className="rounded border px-1">Enter</kbd> to add. Wildcards like{" "}
        <code className="font-mono">files:*</code> are allowed for permissions.
      </p>
    </div>
  );
}

export function AddButton({ onClick, disabled }: { onClick: () => void; disabled?: boolean }) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className="inline-flex size-7 items-center justify-center rounded-md border text-muted-foreground hover:bg-accent disabled:opacity-50"
      aria-label="Add"
    >
      <Plus className="size-4" />
    </button>
  );
}
