"use client";
import { useState } from "react";
import { Button, ErrorBox, Field } from "@/components/ui";
import { api } from "@/lib/api";
import { useAction } from "@/lib/hooks";

const digits = (v: string) => v.replace(/\D/g, "").slice(0, 6);

/** A4: current PIN (or the temporary one) and a new 6-digit PIN. The server rejects weak ones. */
export function ChangePin({ onDone }: { onDone: () => void }) {
  const act = useAction();
  const [f, setF] = useState({ current: "", next: "", again: "" });
  const [done, setDone] = useState(false);
  const mismatch = f.again.length === 6 && f.again !== f.next;
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (f.next !== f.again) return;
    act.run(async () => (await api("/auth/employee/pin", { body: { current_pin: f.current, new_pin: f.next } }), true)).then((ok) => {
      if (!ok) return;
      setF({ current: "", next: "", again: "" });
      setDone(true);
      onDone();
    });
  };
  const pin = (k: keyof typeof f, label: string, hint?: string) => (
    <Field label={label} hint={hint}>
      <input type="password" inputMode="numeric" autoComplete={k === "current" ? "current-password" : "new-password"} value={f[k]} onChange={(e) => { setDone(false); setF({ ...f, [k]: digits(e.target.value) }); }} required minLength={6} />
    </Field>
  );
  return (
    <form className="stack" onSubmit={submit}>
      {pin("current", "Current PIN")}
      {pin("next", "New PIN", "6 digits. Not all the same, and not a run like 123456.")}
      {pin("again", "New PIN again")}
      {mismatch && <small className="err">The two new PINs don't match.</small>}
      {done && <div className="note">PIN changed.</div>}
      <ErrorBox error={act.error} />
      <Button kind="main" type="submit" disabled={act.busy || mismatch}>
        Change PIN
      </Button>
    </form>
  );
}
