import * as React from "react";
import { Cloud, Loader2, Terminal } from "lucide-react";

import { reportError } from "@/lib/toast";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { useAuth } from "@/lib/auth-context";
import { readPendingCallback } from "@/lib/stdb";

const EMAIL_PATTERN = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

export function LoginPage() {
  const { signIn, signUp } = useAuth();

  const [mode, setMode] = React.useState<"sign-in" | "sign-up">("sign-in");
  const [email, setEmail] = React.useState("");
  const [password, setPassword] = React.useState("");
  const [displayName, setDisplayName] = React.useState("");
  const [submitting, setSubmitting] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [pendingCallback] = React.useState<string | null>(() => {
    const cb = readPendingCallback();
    if (!cb) return null;
    try {
      const url = new URL(cb.url);
      return `${url.hostname}:${url.port || "(default port)"}`;
    } catch {
      return cb.url;
    }
  });

  const isSignUp = mode === "sign-up";

  const onSubmit = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setError(null);

    const trimmedEmail = email.trim();
    if (!EMAIL_PATTERN.test(trimmedEmail)) {
      setError("Please enter a valid email address.");
      return;
    }
    if (password.length < 8) {
      setError("Password must be at least 8 characters.");
      return;
    }

    setSubmitting(true);
    try {
      if (isSignUp) {
        const trimmedName = displayName.trim();
        await signUp(
          trimmedEmail,
          password,
          trimmedName.length > 0 ? trimmedName : undefined
        );
      } else {
        await signIn(trimmedEmail, password);
      }
    } catch (err) {
      const message =
        err instanceof Error ? err.message : typeof err === "string" ? err : null;
      setError(message ?? "Sign in failed.");
      reportError(err, "Sign in failed");
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="flex h-full items-center justify-center p-4">
      <Card className="w-full max-w-md">
        <CardHeader className="text-center">
          <div className="mx-auto mb-2 flex size-10 items-center justify-center rounded-lg bg-primary text-primary-foreground">
            <Cloud className="size-5" />
          </div>
          <CardTitle>SpaceNix</CardTitle>
          <CardDescription>
            Sync files and secrets across your devices.
          </CardDescription>
        </CardHeader>
        <CardContent>
          {pendingCallback ? (
            <div
              role="status"
              className="mb-4 flex items-start gap-2 rounded-md border border-primary/30 bg-primary/5 p-3 text-xs text-foreground"
            >
              <Terminal className="mt-0.5 size-4 shrink-0 text-primary" />
              <div className="space-y-0.5">
                <p className="font-medium">The SpaceNix TUI is waiting for you.</p>
                <p className="text-muted-foreground">
                  After you sign in, this browser will redirect back to{" "}
                  <code className="font-mono">{pendingCallback}</code> with your
                  connection token. You can close this tab once you're redirected.
                </p>
              </div>
            </div>
          ) : null}
          <form
            id="credentials-form"
            onSubmit={onSubmit}
            className="space-y-4"
            autoComplete="off"
          >
            <div className="space-y-2">
              <Label htmlFor="email">Email</Label>
              <Input
                id="email"
                name="email"
                type="email"
                autoComplete="username"
                required
                value={email}
                onChange={(event) => setEmail(event.target.value)}
                disabled={submitting}
                placeholder="you@example.com"
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="password">Password</Label>
              <Input
                id="password"
                name="password"
                type="password"
                autoComplete={isSignUp ? "new-password" : "current-password"}
                required
                minLength={8}
                value={password}
                onChange={(event) => setPassword(event.target.value)}
                disabled={submitting}
                placeholder="Your data will be encrypted using this password, please choose a secure password."
              />
            </div>
            {isSignUp ? (
              <div className="space-y-2">
                <Label htmlFor="display-name">Display name (optional)</Label>
                <Input
                  id="display-name"
                  name="displayName"
                  type="text"
                  autoComplete="nickname"
                  value={displayName}
                  onChange={(event) => setDisplayName(event.target.value)}
                  disabled={submitting}
                  maxLength={128}
                  placeholder="What should we call you?"
                />
              </div>
            ) : null}
            {error ? (
              <p className="text-sm text-destructive" role="alert">
                {error}
              </p>
            ) : null}
          </form>
        </CardContent>
        <Separator />
        <CardFooter className="flex flex-col gap-2 p-6">
          <Button
            type="submit"
            form="credentials-form"
            className="w-full"
            disabled={submitting}
          >
            {submitting ? <Loader2 className="size-4 animate-spin" /> : null}
            {isSignUp ? "Create account" : "Sign in"}
          </Button>
          <p className="text-center text-xs text-muted-foreground">
            {isSignUp ? (
              <>
                Already have an account?{" "}
                <button
                  type="button"
                  className="font-medium text-foreground underline-offset-2 hover:underline"
                  onClick={() => {
                    setMode("sign-in");
                    setError(null);
                  }}
                  disabled={submitting}
                >
                  Sign in
                </button>
              </>
            ) : (
              <>
                New to SpaceNix?{" "}
                <button
                  type="button"
                  className="font-medium text-foreground underline-offset-2 hover:underline"
                  onClick={() => {
                    setMode("sign-up");
                    setError(null);
                  }}
                  disabled={submitting}
                >
                  Create an account
                </button>
              </>
            )}
          </p>
        </CardFooter>
      </Card>
    </div>
  );
}
