import { toast } from "sonner";

export function reportError(err: unknown, fallback = "Something went wrong") {
  const msg = err instanceof Error ? err.message : typeof err === "string" ? err : fallback;
  toast.error(msg);
  console.error(err);
}

export function reportSuccess(msg: string) {
  toast.success(msg);
}
