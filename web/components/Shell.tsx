"use client";
import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { useEffect, useRef, useState } from "react";
import { api, tokens } from "@/lib/api";
import { useAccess, useApi, useCan } from "@/lib/hooks";
import { locate, NAV } from "@/lib/nav";
import { setTheme as saveTheme } from "@/lib/theme";
import { Icon } from "./ui";

/** Closes when a click lands outside `ref`, or on Escape. */
function useDismiss(open: boolean, close: () => void, ref: React.RefObject<HTMLElement | null>, keep?: string) {
  useEffect(() => {
    if (!open) return;
    const down = (e: MouseEvent) => {
      const t = e.target as HTMLElement;
      if (!ref.current?.contains(t) && !(keep && t.closest(keep))) close();
    };
    const key = (e: KeyboardEvent) => e.key === "Escape" && close();
    document.addEventListener("mousedown", down);
    window.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("mousedown", down);
      window.removeEventListener("keydown", key);
    };
  }, [open, close, ref, keep]);
}

export async function signOut(router: ReturnType<typeof useRouter>) {
  try {
    await api("/auth/sign-out", { method: "POST" });
  } catch {}
  tokens.set(null);
  router.replace("/login");
}

/** Account menu with sign-out; shared by the admin shell and the employee portal. */
export function AccountMenu({ name, sub }: { name: string; sub: string }) {
  const router = useRouter();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useDismiss(open, () => setOpen(false), ref);
  return (
    <div className="menu-wrap" ref={ref}>
      <button className="menu-btn" onClick={() => setOpen(!open)} aria-expanded={open} aria-haspopup="menu">
        <Icon name="account_circle" fill size={18} />
        <span className="name">{name}</span>
        <Icon name="expand_more" size={16} />
      </button>
      {open && (
        <div className="card menu" role="menu">
          <div className="menu-who">
            <b>{name}</b>
            <span>{sub}</span>
          </div>
          <div className="menu-sep" />
          <button className="menu-item" role="menuitem" onClick={() => signOut(router)}>
            <Icon name="logout" /> Sign out
          </button>
        </div>
      )}
    </div>
  );
}

export function ThemeToggle() {
  const [theme, setTheme] = useState<"dark" | "light">("dark");
  useEffect(() => setTheme(document.documentElement.dataset.theme === "light" ? "light" : "dark"), []);
  const flip = () => {
    const next = theme === "dark" ? "light" : "dark";
    setTheme(next);
    saveTheme(next);
  };
  return (
    <button className="icon-btn boxed theme-toggle" onClick={flip} aria-label="Toggle light or dark theme" title={theme === "dark" ? "Light theme" : "Dark theme"}>
      <Icon name={theme === "dark" ? "light_mode" : "dark_mode"} />
    </button>
  );
}

export function Shell({ children }: { children: React.ReactNode }) {
  const path = usePathname();
  const router = useRouter();
  const access = useAccess();
  const can = useCan();
  const [open, setOpen] = useState<string | null>(null);
  const [drawer, setDrawer] = useState(false);
  const fly = useRef<HTMLDivElement>(null);
  const here = locate(path);
  const visible = NAV.map((s) => ({ ...s, pages: s.pages.filter((p) => !p.perm || can(...p.perm)) })).filter((s) => s.pages.length);
  const unread = useApi<{ unread_count: number }>("/attendance/me/notifications", { unread: true });
  const close = () => setOpen(null);
  useDismiss(!!open, close, fly, ".rail");
  useEffect(() => {
    setOpen(null);
    setDrawer(false);
  }, [path]);

  const section = visible.find((s) => s.key === open);
  const pick = (key: string, pages: { href: string }[]) => (pages.length === 1 ? router.push(pages[0].href) : setOpen(open === key ? null : key));

  return (
    <div className="shell">
      <nav className="rail" aria-label="Sections">
        <Link href="/" className="logo" aria-label="PayLedger overview">PL</Link>
        <div className="rail-sep" />
        <div className="rail-group">
          {visible.map((s) => {
            const on = here?.section.key === s.key;
            return (
              <button
                key={s.key}
                className={`rail-btn${on ? " on" : ""}${open === s.key ? " open" : ""}`}
                aria-label={s.label}
                aria-expanded={s.pages.length > 1 ? open === s.key : undefined}
                onClick={() => pick(s.key, s.pages)}
              >
                <Icon name={s.icon} fill={on} />
                {open !== s.key && <span className="tip">{s.label}</span>}
              </button>
            );
          })}
        </div>
        <div className="rail-group bottom">
          <button className="rail-btn" aria-label="Sign out" onClick={() => signOut(router)}>
            <Icon name="logout" />
            <span className="tip">Sign out</span>
          </button>
        </div>
      </nav>

      {section && (
        <div className="flyout" ref={fly} role="dialog" aria-label={`${section.label} navigation`}>
          <div className="flyout-head">
            <b><Icon name={section.icon} fill />{section.label}</b>
            <button className="icon-btn" onClick={close} aria-label="Close panel">
              <Icon name="chevron_left" />
            </button>
          </div>
          <nav>
            {section.pages.map((p) => {
              const on = here?.page.href === p.href;
              return (
                <Link key={p.href} href={p.href} className={on ? "on" : ""}>
                  <span>{p.label}</span>
                  {on && <Icon name="chevron_right" />}
                </Link>
              );
            })}
          </nav>
        </div>
      )}

      {drawer && (
        <>
          <div className="drawer-bg" onClick={() => setDrawer(false)} aria-hidden />
          <aside className="drawer-nav" aria-label="Menu">
            <div className="flyout-head">
              <b><span className="logo" style={{ width: 30, height: 30, fontSize: 12 }}>PL</span>PayLedger</b>
              <button className="icon-btn" onClick={() => setDrawer(false)} aria-label="Close menu"><Icon name="close" /></button>
            </div>
            <nav>
              {visible.map((s) => (
                <div key={s.key}>
                  <div className="grp"><Icon name={s.icon} />{s.label}</div>
                  {s.pages.map((p) => (
                    <Link key={p.href} href={p.href} className={here?.page.href === p.href ? "on" : ""}>
                      <span>{p.label}</span>
                    </Link>
                  ))}
                </div>
              ))}
            </nav>
          </aside>
        </>
      )}

      <div className="shell-main">
        <header className="topbar">
          <button className="icon-btn mobile-only" onClick={() => setDrawer(true)} aria-label="Open menu">
            <Icon name="menu" />
          </button>
          <div className="crumb">
            {here ? (
              <>
                <span>{here.section.label}</span>
                <span aria-hidden>/</span>
                <b>{here.page.label}</b>
              </>
            ) : (
              <b>{access.org.name}</b>
            )}
          </div>
          <ThemeToggle />
          <Link className="icon-btn boxed" href="/notifications" aria-label="Notifications">
            <Icon name="notifications" />
            {!!unread.data?.unread_count && <span className="badge">{unread.data.unread_count}</span>}
          </Link>
          <AccountMenu name={access.user.name} sub={`${access.org.name}${access.user.email ? ` · ${access.user.email}` : ""}`} />
        </header>
        <main className="content">{children}</main>
      </div>
    </div>
  );
}
