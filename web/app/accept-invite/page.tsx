"use client";
import { useRouter, useSearchParams } from "next/navigation";
import { Suspense, useState } from "react";
import { Button, Code, ErrorBox, Field } from "@/components/ui";
import { api, tokens } from "@/lib/api";
import { useAction } from "@/lib/hooks";

// Invitation link: /invite?token=… → set a password, add the authenticator, then sign in (A7, A6).
function Invite() {
  const router = useRouter();
  const token = useSearchParams().get("token") ?? "";
  const act = useAction();
  const [password, setPassword] = useState("");
  const [setup, setSetup] = useState<{ mfa_setup: { secret: string; otpauth_uri: string }; mfa_token: string } | null>(null);
  const [code, setCode] = useState("");
  const accept = (e: React.FormEvent) => {
    e.preventDefault();
    act.run(() => api("/auth/admin/accept-invite", { body: { invite_token: token, password }, anonymous: true })).then((r) => r && setSetup(r));
  };
  const finish = (e: React.FormEvent) => {
    e.preventDefault();
    act.run(() => api("/auth/admin/mfa", { body: { mfa_token: setup!.mfa_token, totp_code: code }, anonymous: true })).then((t) => {
      if (!t) return;
      tokens.set(t);
      router.replace("/");
    });
  };
  return (
    <div className="auth">
      <div className="card">
        <div className="card-body stack">
          <h1>Accept invitation</h1>
          {!setup ? (
            <form className="stack" onSubmit={accept}>
              <Field label="Choose a password" hint="At least 12 characters">
                <input type="password" autoComplete="new-password" value={password} onChange={(e) => setPassword(e.target.value)} required />
              </Field>
              {!token && <div className="note">This page needs the link from your invitation email.</div>}
              <ErrorBox error={act.error} />
              <Button kind="main" type="submit" disabled={act.busy || !token}>
                Continue
              </Button>
            </form>
          ) : (
            <form className="stack" onSubmit={finish}>
              <p className="muted" style={{ margin: 0 }}>
                Add this key to your authenticator app, then enter the code it shows.
              </p>
              <Code>{setup.mfa_setup.secret}</Code>
              <a href={setup.mfa_setup.otpauth_uri}>Open in authenticator app</a>
              <Field label="Authenticator code">
                <input inputMode="numeric" maxLength={6} value={code} onChange={(e) => setCode(e.target.value)} required />
              </Field>
              <ErrorBox error={act.error} />
              <Button kind="main" type="submit" disabled={act.busy}>
                Finish
              </Button>
            </form>
          )}
        </div>
      </div>
    </div>
  );
}

export default function Page() {
  return (
    <Suspense>
      <Invite />
    </Suspense>
  );
}
