import * as React from "react";
import { Mail, Loader2 } from "lucide-react";

import { reportError, reportSuccess } from "@/lib/toast";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useAuth } from "@/lib/auth-context";
import { useProcedure, useReducer } from "spacetimedb/react";
import { procedures, reducers } from "@/module_bindings";

const CODE_PATTERN = /^\d{6}$/;

export function VerifyEmailPage() {
  const { email, signOut } = useAuth();
  const requestCode = useReducer(reducers.requestEmailVerification);
  const verifyCode = useReducer(reducers.verifyEmail);
  const sendEmail = useProcedure(procedures.sendVerificationEmail);

  const [code, setCode] = React.useState("");
  const [sending, setSending] = React.useState(false);
  const [verifying, setVerifying] = React.useState(false);
  const [lastSentAt, setLastSentAt] = React.useState<number | null>(null);
  const [cooldownRemaining, setCooldownRemaining] = React.useState(0);

  React.useEffect(() => {
    if (!lastSentAt) {
      setCooldownRemaining(0);
      return;
    }
    const update = () => {
      const elapsed = Math.floor((Date.now() - lastSentAt) / 1000);
      setCooldownRemaining(Math.max(0, 60 - elapsed));
    };
    update();
    const id = setInterval(update, 1000);
    return () => clearInterval(id);
  }, [lastSentAt]);

  const sendVerificationEmail = React.useCallback(async () => {
    setSending(true);
    try {
      await requestCode();
      await sendEmail();
      setLastSentAt(Date.now());
      reportSuccess("Verification code sent.");
    } catch (err) {
      reportError(err, "Could not send verification code");
    } finally {
      setSending(false);
    }
  }, [requestCode, sendEmail]);

  const submit = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmed = code.trim();
    if (!CODE_PATTERN.test(trimmed)) {
      reportError(new Error("Enter the 6-digit code from your email."));
      return;
    }
    setVerifying(true);
    try {
      await verifyCode({ code: trimmed });
      reportSuccess("Email verified.");
    } catch (err) {
      reportError(err, "Verification failed");
    } finally {
      setVerifying(false);
    }
  };

  return (
    <div className="flex h-full items-center justify-center p-4">
      <Card className="w-full max-w-md">
        <CardHeader className="text-center">
          <div className="mx-auto mb-2 flex size-10 items-center justify-center rounded-lg bg-primary text-primary-foreground">
            <Mail className="size-5" />
          </div>
          <CardTitle>Verify your email</CardTitle>
          <CardDescription>
            We need to confirm <strong>{email ?? "your email"}</strong> before you
            can use SpaceNix.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-6">
          <Button
            type="button"
            variant="outline"
            className="w-full"
            disabled={sending || cooldownRemaining > 0}
            onClick={sendVerificationEmail}
          >
            {sending ? <Loader2 className="mr-2 size-4 animate-spin" /> : null}
            {cooldownRemaining > 0
              ? `Resend code in ${cooldownRemaining}s`
              : lastSentAt
                ? "Resend verification code"
                : "Send verification code"}
          </Button>

          <form onSubmit={submit} className="space-y-4" autoComplete="off">
            <div className="space-y-2">
              <Label htmlFor="verification-code">Verification code</Label>
              <Input
                id="verification-code"
                name="verificationCode"
                type="text"
                inputMode="numeric"
                autoComplete="one-time-code"
                maxLength={6}
                placeholder="000000"
                value={code}
                onChange={(event) => {
                  const value = event.target.value.replace(/\D/g, "").slice(0, 6);
                  setCode(value);
                }}
                disabled={verifying}
              />
            </div>
            <Button type="submit" className="w-full" disabled={verifying}>
              {verifying ? <Loader2 className="mr-2 size-4 animate-spin" /> : null}
              Verify email
            </Button>
          </form>

          <div className="flex justify-center">
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() => signOut()}
            >
              Sign out
            </Button>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
