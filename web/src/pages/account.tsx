import * as React from "react";
import { Mail, KeyRound, ShieldCheck, ShieldAlert } from "lucide-react";
import { useProcedure, useReducer, useTable } from "spacetimedb/react";

import { procedures, reducers, tables } from "@/module_bindings";
import type { FileMetadata, ReplaceTicket } from "@/module_bindings/types";
import { reportError, reportSuccess } from "@/lib/toast";
import { useAuth } from "@/lib/auth-context";
import { unwrap } from "@/lib/stdb";
import {
  decryptFileContent,
  deriveFileEncryptionKey,
  encryptFileContent,
} from "@/lib/file-crypto";
import { PageHeader, Spinner } from "@/components/common";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";

const EMAIL_PATTERN = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

export function AccountPage() {
  const {
    displayName,
    email,
    identityHex,
    role,
    isAdmin,
    fileEncryptionKey,
    updateLocalPassword,
  } = useAuth();
  const [files] = useTable(tables.my_files);
  const requestDownload = useProcedure(procedures.requestDownloadUrl);
  const replaceContent = useProcedure(procedures.replaceFileContent);
  const verifyPasswordReducer = useReducer(reducers.signIn);
  const updateEmailReducer = useReducer(reducers.updateEmail);
  const updatePasswordReducer = useReducer(reducers.updatePassword);
  const finalizeUpload = useReducer(reducers.finalizeUpload);

  const reencryptFile = React.useCallback(
    async (file: FileMetadata, fromKey: CryptoKey, toKey: CryptoKey) => {
      const downloadRes = await requestDownload({ fileId: file.id });
      const downloadUrl = unwrap<string>(downloadRes);
      const getRes = await fetch(downloadUrl, { mode: "cors" });
      if (!getRes.ok) throw new Error(`Download failed: ${getRes.status}`);

      const encryptedBody = await getRes.arrayBuffer();
      const body = await decryptFileContent(fromKey, encryptedBody);
      const nextEncryptedBody = await encryptFileContent(toKey, body);
      const replaceRes = await replaceContent({
        fileId: file.id,
        contentType: file.contentType ?? undefined,
      });
      const ticket = unwrap<ReplaceTicket>(replaceRes);
      const putRes = await fetch(ticket.uploadUrl, {
        method: "PUT",
        mode: "cors",
        body: nextEncryptedBody,
      });
      if (!putRes.ok) throw new Error(`Upload failed: ${putRes.status} ${putRes.statusText}`);
      await finalizeUpload({ fileId: file.id, hash: file.hash, sizeBytes: file.sizeBytes });
    },
    [finalizeUpload, replaceContent, requestDownload]
  );

  return (
    <div className="mx-auto w-full max-w-2xl space-y-6">
      <PageHeader
        title="Account"
        description="Update your email, password, and view account details."
      />

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <ShieldCheck className="size-4" /> Profile
          </CardTitle>
          <CardDescription>Information tied to your account.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-2 text-sm">
          <Row label="Display name" value={displayName ?? <Empty />} />
          <Row label="Email" value={email ?? <Empty />} />
          <Row label="Role" value={role ?? <Empty />} />
          {isAdmin ? (
            <p className="text-xs text-muted-foreground">
              You can administer the SpaceNix instance.
            </p>
          ) : null}
          <Separator className="my-2" />
          <Row
            label="Identity"
            value={
              <code className="break-all font-mono text-xs text-muted-foreground">
                {identityHex}
              </code>
            }
          />
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Mail className="size-4" /> Email
          </CardTitle>
          <CardDescription>
            Change the email you use to sign in. Confirm with your current password.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <EmailForm
            currentEmail={email}
            onSubmit={async (newEmail, currentPassword) => {
              await updateEmailReducer({ newEmail, currentPassword });
            }}
          />
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <KeyRound className="size-4" /> Password
          </CardTitle>
          <CardDescription>
            Choose a new password (8 characters or more). You'll need your current password to confirm.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <PasswordForm
            onSubmit={async (currentPassword, newPassword) => {
              if (!email) throw new Error("Cannot change password before sign in completes.");
              if (!fileEncryptionKey) throw new Error("File encryption key is not ready.");

              await verifyPasswordReducer({ email, password: currentPassword });
              const newFileEncryptionKey = await deriveFileEncryptionKey(newPassword, identityHex);
              const filesToMigrate = files.filter(
                (file) => !file.isDirectory && file.hash.length > 0
              );
              const migrated: FileMetadata[] = [];

              try {
                for (const file of filesToMigrate) {
                  await reencryptFile(file, fileEncryptionKey, newFileEncryptionKey);
                  migrated.push(file);
                }

                await updatePasswordReducer({ currentPassword, newPassword });
                updateLocalPassword(newPassword, newFileEncryptionKey);
              } catch (err) {
                for (const file of migrated.reverse()) {
                  try {
                    await reencryptFile(file, newFileEncryptionKey, fileEncryptionKey);
                  } catch {
                    // Preserve the original password-change error; rollback is best effort.
                  }
                }
                throw err;
              }
            }}
          />
        </CardContent>
      </Card>
    </div>
  );
}

function Row({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4">
      <span className="text-muted-foreground">{label}</span>
      <span className="text-right font-medium">{value}</span>
    </div>
  );
}

function Empty() {
  return <span className="text-muted-foreground">—</span>;
}

function EmailForm({
  currentEmail,
  onSubmit,
}: {
  currentEmail: string | null;
  onSubmit: (newEmail: string, currentPassword: string) => Promise<void>;
}) {
  const [newEmail, setNewEmail] = React.useState("");
  const [currentPassword, setCurrentPassword] = React.useState("");
  const [busy, setBusy] = React.useState(false);

  const submit = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmed = newEmail.trim();
    if (!EMAIL_PATTERN.test(trimmed)) {
      reportError(new Error("Please enter a valid email address."));
      return;
    }
    if (currentEmail && trimmed === currentEmail) {
      reportError(new Error("New email is the same as the current email."));
      return;
    }
    if (!currentPassword) {
      reportError(new Error("Enter your current password to confirm."));
      return;
    }
    setBusy(true);
    try {
      await onSubmit(trimmed, currentPassword);
      reportSuccess("Email updated.");
      setNewEmail("");
      setCurrentPassword("");
    } catch (err) {
      reportError(err);
    } finally {
      setBusy(false);
    }
  };

  return (
    <form onSubmit={submit} className="space-y-4" autoComplete="off">
      <div className="space-y-2">
        <Label htmlFor="new-email">New email</Label>
        <Input
          id="new-email"
          name="newEmail"
          type="email"
          autoComplete="email"
          required
          value={newEmail}
          onChange={(e) => setNewEmail(e.target.value)}
          disabled={busy}
          placeholder="you@example.com"
        />
      </div>
      <div className="space-y-2">
        <Label htmlFor="current-password-email">Current password</Label>
        <Input
          id="current-password-email"
          name="currentPassword"
          type="password"
          autoComplete="current-password"
          required
          value={currentPassword}
          onChange={(e) => setCurrentPassword(e.target.value)}
          disabled={busy}
        />
      </div>
      <div className="flex justify-end">
        <Button type="submit" disabled={busy}>
          {busy ? <Spinner className="size-4" /> : null}
          Update email
        </Button>
      </div>
    </form>
  );
}

function PasswordForm({
  onSubmit,
}: {
  onSubmit: (currentPassword: string, newPassword: string) => Promise<void>;
}) {
  const [currentPassword, setCurrentPassword] = React.useState("");
  const [newPassword, setNewPassword] = React.useState("");
  const [confirmPassword, setConfirmPassword] = React.useState("");
  const [busy, setBusy] = React.useState(false);
  const [showMismatchWarning, setShowMismatchWarning] = React.useState(false);

  const passwordsMatch = newPassword === confirmPassword;

  const submit = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (newPassword.length < 8) {
      reportError(new Error("New password must be at least 8 characters."));
      return;
    }
    if (!passwordsMatch) {
      reportError(new Error("New passwords do not match."));
      return;
    }
    if (newPassword === currentPassword) {
      reportError(new Error("New password must be different from the current one."));
      return;
    }
    setBusy(true);
    try {
      await onSubmit(currentPassword, newPassword);
      reportSuccess("Password updated.");
      setCurrentPassword("");
      setNewPassword("");
      setConfirmPassword("");
    } catch (err) {
      reportError(err);
    } finally {
      setBusy(false);
    }
  };

  return (
    <form onSubmit={submit} className="space-y-4" autoComplete="off">
      <div className="space-y-2">
        <Label htmlFor="current-password">Current password</Label>
        <Input
          id="current-password"
          name="currentPassword"
          type="password"
          autoComplete="current-password"
          required
          value={currentPassword}
          onChange={(e) => setCurrentPassword(e.target.value)}
          disabled={busy}
        />
      </div>
      <Separator />
      <div className="space-y-2">
        <Label htmlFor="new-password">New password</Label>
        <Input
          id="new-password"
          name="newPassword"
          type="password"
          autoComplete="new-password"
          required
          minLength={8}
          value={newPassword}
          onChange={(e) => {
            setNewPassword(e.target.value);
            setShowMismatchWarning(true);
          }}
          disabled={busy}
        />
      </div>
      <div className="space-y-2">
        <Label htmlFor="confirm-password">Confirm new password</Label>
        <Input
          id="confirm-password"
          name="confirmPassword"
          type="password"
          autoComplete="new-password"
          required
          minLength={8}
          value={confirmPassword}
          onChange={(e) => {
            setConfirmPassword(e.target.value);
            setShowMismatchWarning(true);
          }}
          disabled={busy}
        />
        {showMismatchWarning && confirmPassword.length > 0 && !passwordsMatch ? (
          <p className="flex items-center gap-1 text-xs text-destructive">
            <ShieldAlert className="size-3" /> Passwords do not match.
          </p>
        ) : null}
      </div>
      <div className="flex justify-end">
        <Button type="submit" disabled={busy}>
          {busy ? <Spinner className="size-4" /> : null}
          Update password
        </Button>
      </div>
    </form>
  );
}
