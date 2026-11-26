"use client";
import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { useCallback, useEffect, useState } from "react";
import { Button, ErrorBox, Icon, Loading } from "@/components/ui";
import { api, ApiError, tokens } from "@/lib/api";
import { Access, AccessContext, useAccess, useApi } from "@/lib/hooks";
import { AccountMenu, signOut, ThemeToggle } from "@/components/Shell";
import { ChangePin } from "./ChangePin";

const NAV = [
  { href: "/portal", label: "Today", icon: "today" },
  { href: "/portal/attendance", label: "Attendance", icon: "calendar_month" },
  { href: "/portal/roster", label: "Roster", icon: "view_week" },
  { href: "/portal/requests", label: "Requests", icon: "assignment" },
  { href: "/portal/holidays", label: "Holidays", icon: "beach_access" },
  { href: "/portal/profile", label: "Profile", icon: "person" },
];

// Employee self-service. Staff accounts are sent back to the admin app.
export default function PortalLayout({ children }: { children: React.ReactNode }) {
  const router = useRouter();
  const [access, setAccess] = useState<Access | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  const load = useCallback(() => {
    setError(null);
    api<Access>("/me/access").then((a) => (a.user.kind === "employee" ? setAccess(a) : router.replace("/")), setError);
  }, [router]);
  useEffect(() => {
    if (!tokens.get()) return router.replace("/login");
    load();
  }, [router, load]);

  if (error?.code === "pin_change_required")
    return (
      <div className="auth-form pin-page">
        <ThemeToggle />
        <div className="card auth-box pin-card">
          <div className="brand-row">
            <span className="logo">PL</span>
            PayLedger
          </div>
          <span className="tile"><Icon name="lock_reset" fill /></span>
          <div>
            <h1>Set your PIN</h1>
            <p className="muted" style={{ margin: "6px 0 0", fontSize: 14 }}>You signed in with a temporary PIN. Choose your own 6-digit PIN to continue.</p>
          </div>
          <ChangePin onDone={load} />
          <Button kind="link" onClick={() => signOut(router)}>Not you? Sign out</Button>
        </div>
      </div>
    );
  if (error) return <div className="content"><ErrorBox error={error} /></div>;
  if (!access) return <Loading full />;
  return (
    <AccessContext.Provider value={access}>
      <PortalShell>{children}</PortalShell>
    </AccessContext.Provider>
  );
}

function PortalShell({ children }: { children: React.ReactNode }) {
  const path = usePathname();
  const access = useAccess();
  const unread = useApi<{ unread_count: number }>("/attendance/me/notifications", { unread: true });
  return (
    <div className="portal">
      <header className="topbar">
        <span className="logo">PL</span>
        <nav className="portal-nav" aria-label="Sections">
          {NAV.map((n) => (
            <Link key={n.href} href={n.href} className={path === n.href ? "on" : ""} aria-label={n.label}>
              <Icon name={n.icon} fill={path === n.href} />
              <span className="lbl">{n.label}</span>
            </Link>
          ))}
        </nav>
        <ThemeToggle />
        <Link className="icon-btn boxed" href="/portal/notifications" aria-label="Notifications">
          <Icon name="notifications" />
          {!!unread.data?.unread_count && <span className="badge">{unread.data.unread_count}</span>}
        </Link>
        <AccountMenu name={access.user.name} sub={access.org.name} />
      </header>
      <main className="content" style={{ maxWidth: 1040 }}>{children}</main>
    </div>
  );
}
