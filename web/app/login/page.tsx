"use client";
import { useRouter } from "next/navigation";
import { useState } from "react";
import { ThemeToggle } from "@/components/Shell";
import { Button, ErrorBox, Field, Icon, Tabs } from "@/components/ui";
import { api, tokens } from "@/lib/api";
import { useAction } from "@/lib/hooks";
import FluidOrb from "@/components/FluidOrb";

type Who = "admin" | "employee" | "vendor";
const SUB: Record<Who, string> = {
  admin: "Sign in to manage employees, attendance and settings.",
  employee: "Sign in with your organisation code, employee code and PIN.",
  vendor: "Sign in to the vendor console.",
};
const POINTS = [
  { i: "group", t: "Employee master", d: "One record per person: job history, status changes and portal access." },
  { i: "fingerprint", t: "Attendance", d: "Punches, shifts, rosters and policies worked out into daily results." },
  { i: "admin_panel_settings", t: "Roles and settings", d: "Control who sees what, and how your organisation's rules apply." },
];

// Sign-in for all three kinds of account: admin (A5, A6), employee (A1–A3) and vendor (§9).
export default function Login() {
  const [who, setWho] = useState<Who>("admin");
  return (
    <div className="auth">
      <section className="auth-brand" aria-hidden>
        <div className="brand-row">
          <span className="logo">PL</span>
          PayLedger
        </div>
        <div>
          <h2>Payroll, attendance and people, in one place.</h2>
          <p className="lead">The workspace your HR and operations teams use every day.</p>
          <ul className="auth-points">
            {POINTS.map((p) => (
              <li key={p.t}>
                <span className="tile"><Icon name={p.i} fill /></span>
                <span><b>{p.t}</b>{p.d}</span>
              </li>
            ))}
          </ul>
        </div>
        <footer>© {new Date().getFullYear()} PayLedger</footer>
        <FluidOrb className="auth-orb" size={440} color="#4f6bff" />
      </section>

      <section className="auth-form">
        <ThemeToggle />
        <div className="auth-box">
          <div>
            <h1>Welcome back</h1>
            <p className="muted" style={{ margin: "6px 0 0", fontSize: 14 }}>{SUB[who]}</p>
          </div>
          <Tabs
            value={who}
            onChange={setWho}
            tabs={[
              { value: "admin", label: "Admin" },
              { value: "employee", label: "Employee" },
              { value: "vendor", label: "Vendor" },
            ]}
          />
          {who === "employee" ? <EmployeeSignIn key="e" /> : <StaffSignIn key={who} who={who} />}
          <p className="auth-help">Trouble signing in? Ask your HR or system administrator.</p>
        </div>
      </section>
    </div>
  );
}

/** Text input with a leading icon; `secret` adds a show/hide button. */
function IconInput({ icon, secret, ...p }: { icon: string; secret?: boolean } & React.InputHTMLAttributes<HTMLInputElement>) {
  const [show, setShow] = useState(false);
  return (
    <div className="with-icon">
      <Icon name={icon} />
      <input {...p} type={secret && !show ? "password" : p.type ?? "text"} style={secret ? { paddingRight: 42 } : undefined} />
      {secret && (
        <button type="button" className="icon-btn reveal" onClick={() => setShow(!show)} aria-label={show ? "Hide" : "Show"} title={show ? "Hide" : "Show"}>
          <Icon name={show ? "visibility_off" : "visibility"} />
        </button>
      )}
    </div>
  );
}

/** Email + password; then the authenticator code, unless the server signs in straight away (MFA_DISABLED). */
function StaffSignIn({ who }: { who: "admin" | "vendor" }) {
  const router = useRouter();
  const act = useAction();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [mfa, setMfa] = useState<{ mfa_token: string; mfa_setup_required?: boolean } | null>(null);
  const [code, setCode] = useState("");
  const base = who === "admin" ? "/auth/admin" : "/vendor/auth";
  const done = (t: { access_token: string; refresh_token: string }) => {
    tokens.set(t);
    router.replace(who === "admin" ? "/" : "/vendor");
  };

  const step1 = (e: React.FormEvent) => {
    e.preventDefault();
    act.run(() => api(`${base}/sign-in`, { body: { email: email.trim(), password }, anonymous: true })).then((r) => r && (r.access_token ? done(r) : setMfa(r)));
  };
  const step2 = (e: React.FormEvent) => {
    e.preventDefault();
    act.run(() => api(`${base}/mfa`, { body: { mfa_token: mfa!.mfa_token, totp_code: code }, anonymous: true })).then((t) => t && done(t));
  };

  return !mfa ? (
    <form className="stack" onSubmit={step1}>
      <Field label="Email">
        <IconInput icon="mail" type="email" autoComplete="username" placeholder="you@company.com" value={email} onChange={(e) => setEmail(e.target.value)} required autoFocus />
      </Field>
      <Field label="Password">
        <IconInput icon="lock" secret autoComplete="current-password" placeholder="Your password" value={password} onChange={(e) => setPassword(e.target.value)} required />
      </Field>
      <ErrorBox error={act.error} />
      <Button kind="main" type="submit" className="block" disabled={act.busy}>
        {act.busy ? "Signing in…" : "Sign in"}
      </Button>
    </form>
  ) : (
    <form className="stack" onSubmit={step2}>
      <p className="muted" style={{ margin: 0 }}>Enter the 6-digit code from your authenticator app.</p>
      {mfa.mfa_setup_required && <div className="note">This account has no authenticator set up yet. Use the link from your invitation email.</div>}
      <Field label="Authenticator code">
        <IconInput icon="pin" inputMode="numeric" autoComplete="one-time-code" maxLength={6} value={code} onChange={(e) => setCode(e.target.value.replace(/\D/g, ""))} autoFocus required />
      </Field>
      <ErrorBox error={act.error} />
      <div className="spread">
        <Button kind="link" onClick={() => { setMfa(null); setCode(""); }}>Back</Button>
        <Button kind="main" type="submit" disabled={act.busy}>Sign in</Button>
      </div>
    </form>
  );
}

/** A browser id kept across visits, so HR sees "new device" only for a new browser (AC-EM-19.7). */
function deviceId() {
  try {
    let id = localStorage.getItem("payroll.device");
    if (!id) localStorage.setItem("payroll.device", (id = crypto.randomUUID()));
    return id;
  } catch {
    return crypto.randomUUID();
  }
}

/** Organisation code + employee code, then the PIN or a code sent by email. */
function EmployeeSignIn() {
  const router = useRouter();
  const act = useAction();
  const [f, setF] = useState(() => {
    try {
      return { org: localStorage.getItem("payroll.org") ?? "", code: "", secret: "" };
    } catch {
      return { org: "", code: "", secret: "" };
    }
  });
  const [byEmail, setByEmail] = useState(false);
  const [sent, setSent] = useState(false);
  const ids = { org_short_code: f.org.trim().toUpperCase(), employee_code: f.code.trim().toUpperCase() };
  const digits = (v: string) => v.replace(/\D/g, "").slice(0, 6);

  const sendCode = () =>
    act.run(() => api("/auth/employee/otp/request", { body: ids, anonymous: true })).then((r) => r !== undefined && setSent(true));
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (byEmail && !sent) return void sendCode();
    const device = { id: deviceId(), platform: "web" };
    const body = byEmail ? { ...ids, code: f.secret, device } : { ...ids, pin: f.secret, device };
    act.run(() => api(byEmail ? "/auth/employee/otp/verify" : "/auth/employee/sign-in", { body, anonymous: true })).then((t) => {
      if (!t) return;
      tokens.set(t);
      try {
        localStorage.setItem("payroll.org", ids.org_short_code);
      } catch {}
      router.replace("/portal");
    });
  };
  const locked = act.error?.code === "account_locked" ? String(act.error.meta?.locked_until ?? "") : "";

  return (
    <form className="stack" onSubmit={submit}>
      <div className="grid2" style={{ gridTemplateColumns: "1fr 1fr" }}>
        <Field label="Organisation code">
          <IconInput icon="domain" placeholder="e.g. DEV1" value={f.org} autoCapitalize="characters" onChange={(e) => setF({ ...f, org: e.target.value.toUpperCase() })} required disabled={sent} />
        </Field>
        <Field label="Employee code">
          <IconInput icon="badge" placeholder="Your code" value={f.code} autoCapitalize="characters" autoComplete="username" onChange={(e) => setF({ ...f, code: e.target.value.toUpperCase() })} required disabled={sent} />
        </Field>
      </div>
      {!byEmail ? (
        <Field label="PIN">
          <IconInput icon="lock" secret inputMode="numeric" autoComplete="current-password" placeholder="6 digits" value={f.secret} onChange={(e) => setF({ ...f, secret: digits(e.target.value) })} required />
        </Field>
      ) : sent ? (
        <Field label="Code from your email" hint="Valid for 10 minutes.">
          <IconInput icon="pin" inputMode="numeric" autoComplete="one-time-code" value={f.secret} onChange={(e) => setF({ ...f, secret: digits(e.target.value) })} autoFocus required />
        </Field>
      ) : (
        <p className="muted small" style={{ margin: 0 }}>We'll email a 6-digit code to the address HR has on file.</p>
      )}
      {locked && <div className="note">Too many wrong tries. Try again after {new Date(locked).toLocaleTimeString("en-IN", { hour: "2-digit", minute: "2-digit" })}, or ask HR to unlock you.</div>}
      <ErrorBox error={locked ? null : act.error} />
      <Button kind="main" type="submit" className="block" disabled={act.busy}>
        {act.busy ? "Please wait…" : byEmail && !sent ? "Email me a code" : "Sign in"}
      </Button>
      <Button
        kind="link"
        onClick={() => {
          setByEmail(!byEmail);
          setSent(false);
          setF({ ...f, secret: "" });
          act.setError(null);
        }}
      >
        {byEmail ? "Use my PIN instead" : "Forgot PIN? Sign in with an email code"}
      </Button>
    </form>
  );
}
