"use client";
import { ReactNode, useEffect, useMemo, useRef, useState } from "react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { ApiError } from "@/lib/api";
import { useCan } from "@/lib/hooks";
import { locate } from "@/lib/nav";

export const Icon = ({ name, fill, size }: { name: string; fill?: boolean; size?: number }) => (
  <span className={`ms${fill ? " fill" : ""}`} style={size ? { fontSize: size } : undefined} aria-hidden>
    {name}
  </span>
);

export function PageHead({ title, sub, children }: { title: string; sub?: string; children?: ReactNode }) {
  return (
    <>
      <div className="page-head">
        <div>
          <h1>{title}</h1>
          {sub && <p className="muted">{sub}</p>}
        </div>
        <div className="row">{children}</div>
      </div>
      <SectionTabs />
    </>
  );
}

/** PayLedger-style tab bar under the page title: the other pages of the current rail section the user may open. */
function SectionTabs() {
  const path = usePathname();
  const can = useCan();
  const here = locate(path);
  const pages = here?.section.pages.filter((p) => !p.perm || can(...p.perm)) ?? [];
  if (pages.length < 2) return null;
  return (
    <nav className="tabs section-tabs" aria-label={here!.section.label}>
      {pages.map((p) => (
        <Link key={p.href} href={p.href} className={p.href === here!.page.href ? "on" : ""} aria-current={p.href === here!.page.href ? "page" : undefined}>
          {p.label}
        </Link>
      ))}
    </nav>
  );
}

type BtnProps = React.ButtonHTMLAttributes<HTMLButtonElement> & { kind?: "main" | "ghost" | "danger" | "link"; icon?: string };
export function Button({ kind = "ghost", icon, children, className, ...p }: BtnProps) {
  return (
    <button type="button" className={`btn ${kind} ${className ?? ""}`} {...p}>
      {icon && <Icon name={icon} />}
      {children}
    </button>
  );
}

export function Card({ title, actions, children, pad = true }: { title?: ReactNode; actions?: ReactNode; children: ReactNode; pad?: boolean }) {
  return (
    <section className="card">
      {(title || actions) && (
        <header className="card-head">
          <h2>{title}</h2>
          <div className="row">{actions}</div>
        </header>
      )}
      <div className={pad ? "card-body" : undefined}>{children}</div>
    </section>
  );
}

/** Problem details as a banner: title, detail, and field errors not shown beside an input. */
export function ErrorBox({ error, fields = [] }: { error: ApiError | null; fields?: string[] }) {
  if (!error) return null;
  const rest = error.errors.filter((e) => !fields.includes(e.field));
  const msg =
    error.status === 403 && error.code === "forbidden"
      ? "You don't have permission for this."
      : error.status === 412
        ? "Someone changed this record since you opened it. Reload and try again."
        : error.detail || error.title;
  return (
    <div className="alert" role="alert">
      <Icon name="error" />
      <div>
        <strong>{error.title || error.code}</strong>
        {msg && msg !== error.title && <div>{msg}</div>}
        {rest.length > 0 && (
          <ul>
            {rest.map((e, i) => (
              <li key={i}>
                {e.field && <code>{e.field}</code>} {e.message}
              </li>
            ))}
          </ul>
        )}
        <div className="muted small">
          {error.code}
          {error.status ? ` · ${error.status}` : ""}
        </div>
      </div>
    </div>
  );
}

export const Spinner = ({ small }: { small?: boolean }) => <span className={`spinner${small ? " sm" : ""}`} role="status" aria-label="Loading" />;

/** Spinner block; `full` centres it on the whole screen (first load). */
export function Loading({ full }: { full?: boolean }) {
  return (
    <div className={full ? "loading full" : "loading"}>
      <Spinner />
    </div>
  );
}

/**
 * Indian mobile: fixed +91 prefix, 10 digits only. Takes and returns E.164 ("+919876543210"), as the API expects.
 * shortcut: assumes +91; a stored number from another country shows its digits only. Add a country picker if needed.
 */
export function PhoneInput({ value, onChange, ...p }: { value?: string | null; onChange: (e164: string) => void } & Omit<React.InputHTMLAttributes<HTMLInputElement>, "value" | "onChange">) {
  const digits = (value ?? "").replace(/^\+91/, "").replace(/\D/g, "").slice(0, 10);
  return (
    <div className="phone">
      <span>+91</span>
      <input
        {...p}
        type="tel"
        inputMode="numeric"
        autoComplete="tel-national"
        pattern="[0-9]{10}"
        placeholder="10-digit number"
        value={digits}
        onChange={(e) => {
          const d = e.target.value.replace(/\D/g, "").slice(0, 10);
          onChange(d ? `+91${d}` : "");
        }}
      />
    </div>
  );
}

export function Empty({ children = "Nothing here yet." }: { children?: ReactNode }) {
  return (
    <div className="empty">
      <span className="tile"><Icon name="inbox" fill /></span>
      <span>{children}</span>
    </div>
  );
}

/** Labelled input; shows the server's message for `name` when error names it. */
export function Field({
  label,
  name,
  error,
  hint,
  children,
  wide,
}: {
  label: string;
  name?: string;
  error?: ApiError | null;
  hint?: ReactNode;
  children: ReactNode;
  wide?: boolean;
}) {
  const msg = name && error ? error.field(name) : undefined;
  return (
    <label className={`field${wide ? " wide" : ""}${msg ? " bad" : ""}`}>
      <span>{label}</span>
      {children}
      {msg ? <small className="err">{msg}</small> : hint ? <small className="muted">{hint}</small> : null}
    </label>
  );
}

export type Opt = { value: string; label: string };
export function Select({
  value,
  onChange,
  options,
  placeholder,
  ...p
}: { value: string | undefined | null; onChange: (v: string) => void; options: Opt[]; placeholder?: string } & Omit<
  React.SelectHTMLAttributes<HTMLSelectElement>,
  "onChange" | "value"
>) {
  return (
    <select value={value ?? ""} onChange={(e) => onChange(e.target.value)} {...p}>
      {placeholder !== undefined && <option value="">{placeholder}</option>}
      {options.map((o) => (
        <option key={o.value} value={o.value}>
          {o.label}
        </option>
      ))}
    </select>
  );
}

/** Right-side drawer for forms; `center` for short confirmations. */
export function Modal({ open, title, onClose, children, footer, wide, center }: { open: boolean; title: string; onClose: () => void; children: ReactNode; footer?: ReactNode; wide?: boolean; center?: boolean }) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const d = ref.current;
    if (!d) return;
    if (open && !d.open) d.showModal();
    if (!open && d.open) d.close();
  }, [open]);
  return (
    <dialog ref={ref} className={`modal${wide ? " wide" : ""}${center ? " center" : ""}`} onClose={onClose}>
      {open && (
        <>
          <header className="card-head">
            <h2>{title}</h2>
            <button className="icon-btn" onClick={onClose} aria-label="Close">
              <Icon name="close" />
            </button>
          </header>
          <div className="card-body">{children}</div>
          {footer && <footer className="modal-foot">{footer}</footer>}
        </>
      )}
    </dialog>
  );
}

const GOOD = ["active", "approved", "present", "enrolled", "finalised", "done", "effective", "completed", "employee_portal"];
const WARN = ["pending", "pending_approval", "probation", "notice", "draft", "scheduled", "provisional", "half_day", "processing", "queued"];
const BAD = ["inactive", "rejected", "exited", "absent", "suspended", "discarded", "failed", "locked", "voided", "withdrawn", "cancelled"];
export function Pill({ value }: { value?: string | null }) {
  if (!value) return <span className="muted">—</span>;
  const tone = GOOD.includes(value) ? "good" : WARN.includes(value) ? "warn" : BAD.includes(value) ? "bad" : "";
  return <span className={`pill ${tone}`}>{value.replace(/_/g, " ")}</span>;
}

export const Code = ({ children }: { children: ReactNode }) => <span className="code">{children}</span>;

export function Tabs<T extends string>({ value, onChange, tabs }: { value: T; onChange: (v: T) => void; tabs: { value: T; label: string }[] }) {
  return (
    <div className="tabs" role="tablist">
      {tabs.map((t) => (
        <button key={t.value} role="tab" aria-selected={t.value === value} className={t.value === value ? "on" : ""} onClick={() => onChange(t.value)}>
          {t.label}
        </button>
      ))}
    </div>
  );
}

type Col<T> = { h: ReactNode; c: (r: T) => ReactNode; w?: string; right?: boolean };
const text = (v: unknown): string | null => (typeof v === "string" || typeof v === "number" ? String(v) : null);
const label = (h: ReactNode, i: number) => text(h) || `Column ${i + 1}`;

/**
 * Data table (PayLedger style): search, sortable columns, column picker, CSV export and paging over the rows
 * already loaded; cards on phones. Columns whose cell is plain text or a number sort, search and export by it.
 * shortcut: search/sort/paging cover only the loaded rows; server-side filters stay above the table.
 */
export function Table<T>({ rows, cols, onRow, empty, name = "export" }: { rows: T[] | undefined | null; cols: Col<T>[]; onRow?: (r: T) => void; empty?: ReactNode; name?: string }) {
  const [q, setQ] = useState("");
  const [sort, setSort] = useState<{ i: number; asc: boolean } | null>(null);
  const [hidden, setHidden] = useState<Record<number, boolean>>({});
  const [menu, setMenu] = useState(false);
  const [page, setPage] = useState(1);
  const [size, setSize] = useState(25);
  const menuRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!menu) return;
    const down = (e: MouseEvent) => !menuRef.current?.contains(e.target as Node) && setMenu(false);
    document.addEventListener("mousedown", down);
    return () => document.removeEventListener("mousedown", down);
  }, [menu]);

  const list = useMemo(() => {
    let out = rows ?? [];
    const k = q.trim().toLowerCase();
    if (k) out = out.filter((r) => cols.some((c) => text(c.c(r))?.toLowerCase().includes(k)) || JSON.stringify(r).toLowerCase().includes(k));
    if (sort) {
      const c = cols[sort.i];
      out = [...out].sort((a, b) => {
        const x = text(c.c(a)) ?? "";
        const y = text(c.c(b)) ?? "";
        const n = x !== "" && y !== "" && !isNaN(+x) && !isNaN(+y) ? +x - +y : x.localeCompare(y, undefined, { numeric: true });
        return sort.asc ? n : -n;
      });
    }
    return out;
  }, [rows, cols, q, sort]);

  if (!rows) return <Loading />;
  const shown = cols.map((c, i) => ({ ...c, i })).filter((c) => !hidden[c.i]);
  const pages = Math.max(1, Math.ceil(list.length / size));
  const at = Math.min(page, pages);
  const slice = list.slice((at - 1) * size, at * size);
  const sortable = (c: Col<T>) => rows.length > 1 && text(c.h) !== null && text(c.c(rows[0])) !== null;
  const tools = rows.length > 0;
  const csv = () => {
    const line = (vals: string[]) => vals.map((v) => `"${v.replace(/"/g, '""')}"`).join(",");
    const useful = shown.filter((c) => text(c.h));
    const body = [line(useful.map((c) => label(c.h, c.i))), ...list.map((r) => line(useful.map((c) => text(c.c(r)) ?? "")))].join("\n");
    const a = document.createElement("a");
    a.href = URL.createObjectURL(new Blob([body], { type: "text/csv" }));
    a.download = `${name}.csv`;
    a.click();
    URL.revokeObjectURL(a.href);
  };
  const [first, second, ...rest] = shown;

  return (
    <div>
      {tools && (
        <div className="tbl-bar">
          <div className="search with-icon">
            <Icon name="search" />
            <input value={q} onChange={(e) => { setQ(e.target.value); setPage(1); }} placeholder="Search records…" aria-label="Search this table" />
          </div>
          <div className="row">
            <div className="menu-wrap" ref={menuRef}>
              <Button icon="view_column" onClick={() => setMenu(!menu)} aria-expanded={menu}>Columns</Button>
              {menu && (
                <div className="card menu cols-menu">
                  {cols.map((c, i) =>
                    text(c.h) ? (
                      <label key={i}>
                        <input type="checkbox" checked={!hidden[i]} onChange={() => setHidden({ ...hidden, [i]: !hidden[i] })} />
                        {label(c.h, i)}
                      </label>
                    ) : null,
                  )}
                </div>
              )}
            </div>
            <Button icon="download" onClick={csv}>Export CSV</Button>
          </div>
        </div>
      )}
      {!list.length ? (
        <div className="empty">
          <span className="tile"><Icon name="inbox" fill /></span>
          <span>{q ? "Nothing matches your search." : empty ?? "Nothing here yet."}</span>
        </div>
      ) : (
        <>
          <div className="table-wrap has-cards">
            <table className="table">
              <thead>
                <tr>
                  {shown.map((c) => (
                    <th key={c.i} style={{ width: c.w, textAlign: c.right ? "right" : undefined }} aria-sort={sort?.i === c.i ? (sort.asc ? "ascending" : "descending") : undefined}>
                      {sortable(c) ? (
                        <button onClick={() => setSort(sort?.i === c.i ? { i: c.i, asc: !sort.asc } : { i: c.i, asc: true })}>
                          {c.h}
                          <Icon name={sort?.i === c.i ? (sort.asc ? "arrow_upward" : "arrow_downward") : "swap_vert"} />
                        </button>
                      ) : (
                        c.h
                      )}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {slice.map((r, i) => (
                  <tr key={i} onClick={onRow ? () => onRow(r) : undefined} className={onRow ? "click" : undefined}>
                    {shown.map((c) => (
                      <td key={c.i} className={c.right ? "num" : undefined}>{c.c(r)}</td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <div className="tbl-cards">
            {slice.map((r, i) => (
              <div key={i} className="tbl-card" onClick={onRow ? () => onRow(r) : undefined}>
                <div className="t">{first?.c(r)}</div>
                {second && <div className="s">{second.c(r)}</div>}
                {rest.length > 0 && (
                  <dl>
                    {rest.map((c) => (
                      <div key={c.i}>
                        {text(c.h) && <dt>{c.h}</dt>}
                        <dd>{c.c(r)}</dd>
                      </div>
                    ))}
                  </dl>
                )}
              </div>
            ))}
          </div>
          {list.length > 10 && (
            <div className="tbl-foot">
              <div className="row">
                <span>Showing {(at - 1) * size + 1}–{Math.min(at * size, list.length)} of {list.length}</span>
                <select value={size} onChange={(e) => { setSize(+e.target.value); setPage(1); }} aria-label="Rows per page">
                  {[10, 25, 50, 100].map((n) => <option key={n} value={n}>{n} / page</option>)}
                </select>
              </div>
              <div className="row">
                <Button icon="chevron_left" disabled={at <= 1} onClick={() => setPage(at - 1)}>Prev</Button>
                <span>Page {at} of {pages}</span>
                <Button disabled={at >= pages} onClick={() => setPage(at + 1)}>Next <Icon name="chevron_right" /></Button>
              </div>
            </div>
          )}
        </>
      )}
    </div>
  );
}

/** Asks for a reason, then runs onConfirm(reason). Many state changes require one. */
export function ReasonDialog({
  open,
  title,
  label = "Reason",
  required = true,
  confirm = "Confirm",
  onClose,
  onConfirm,
  error,
  busy,
  children,
}: {
  open: boolean;
  title: string;
  label?: string;
  required?: boolean;
  confirm?: string;
  onClose: () => void;
  onConfirm: (reason: string) => void;
  error?: ApiError | null;
  busy?: boolean;
  children?: ReactNode;
}) {
  const ref = useRef<HTMLTextAreaElement>(null);
  return (
    <Modal
      open={open}
      center
      title={title}
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button kind="main" disabled={busy} onClick={() => onConfirm(ref.current?.value.trim() ?? "")}>
            {confirm}
          </Button>
        </>
      }
    >
      {children}
      <Field label={label + (required ? "" : " (optional)")} name="reason" error={error}>
        <textarea ref={ref} rows={3} />
      </Field>
      <ErrorBox error={error ?? null} fields={["reason"]} />
    </Modal>
  );
}

export const fmtDate = (d?: string | null) => (d ? new Date(d.length === 10 ? d + "T00:00:00" : d).toLocaleDateString("en-IN", { day: "2-digit", month: "short", year: "numeric" }) : "—");
export const fmtTime = (d?: string | null) => (d ? new Date(d).toLocaleString("en-IN", { day: "2-digit", month: "short", hour: "2-digit", minute: "2-digit" }) : "—");
export const mins = (m?: number | null) => (m == null ? "—" : `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, "0")}m`);
