import * as React from "react";
import { Cloud, KeyRound, Loader2 } from "lucide-react";
import { toast } from "sonner";

import { useAuth } from "@/lib/auth";
import { reportError } from "@/lib/toast";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";

export function LoginPage() {
  const { status, identityHex, isAuthenticated, signUp, signIn } = useAuth();
  const [name, setName] = React.useState("");
  const [busy, setBusy] = React.useState(false);

  React.useEffect(() => {
    if (isAuthenticated) toast.success("Welcome back!");
  }, [isAuthenticated]);

  const handleSignUp = async () => {
    setBusy(true);
    try {
      await signUp(name.trim() || undefined);
      toast.success("Account created.");
    } catch (err) {
      reportError(err, "Sign up failed");
    } finally {
      setBusy(false);
    }
  };

  const handleSignIn = async () => {
    setBusy(true);
    try {
      await signIn();
      toast.success("Signed in.");
    } catch (err) {
      reportError(err, "Sign in failed");
    } finally {
      setBusy(false);
    }
  };

  if (status === "connecting") {
    return (
      <div className="flex h-full items-center justify-center">
        <Loader2 className="size-6 animate-spin text-muted-foreground" />
      </div>
    );
  }

  return (
    <div className="flex h-full items-center justify-center p-4">
      <Card className="w-full max-w-md">
        <CardHeader className="text-center">
          <div className="mx-auto mb-2 flex size-10 items-center justify-center rounded-lg bg-primary text-primary-foreground">
            <Cloud className="size-5" />
          </div>
          <CardTitle>SpaceNix</CardTitle>
          <CardDescription>
            Sync files, configs, and secrets across your devices.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="name">Display name (optional)</Label>
            <Input
              id="name"
              placeholder="hannah"
              value={name}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void handleSignUp();
              }}
              maxLength={128}
            />
          </div>
          <div className="rounded-md border bg-muted/40 p-3 text-xs text-muted-foreground">
            <div className="flex items-center gap-1.5 font-medium text-foreground">
              <KeyRound className="size-3.5" /> Your identity
            </div>
            <div className="mt-1 break-all font-mono text-[11px]">{identityHex || "—"}</div>
            <p className="mt-2">
              SpacetimeDB identifies you by a key generated in your browser. Create an account to
              bind this identity, or sign in if it already exists.
            </p>
          </div>
        </CardContent>
        <Separator />
        <CardFooter className="flex flex-col gap-2 p-6">
          <Button className="w-full" disabled={busy} onClick={handleSignUp}>
            {busy ? <Loader2 className="size-4 animate-spin" /> : null}
            Create account
          </Button>
          <Button className="w-full" variant="outline" disabled={busy} onClick={handleSignIn}>
            I already have an account
          </Button>
        </CardFooter>
      </Card>
    </div>
  );
}
