// One client for every /v1 call: bearer token, refresh rotation, If-Match, Idempotency-Key, problem-details errors.
// The server enforces every rule; this file only carries requests.

export type FieldError = { field: string; code: string; message: string; meta?: Record<string, unknown> };

export class ApiError extends Error {
  constructor(
    public status: number,
    public code: string,
    public title: string,
    public detail: string,
    public errors: FieldError[] = [],
    public meta?: Record<string, unknown>,
  ) {
    super(detail || title || code);
  }
  /** Message for one form field, if the server named it. */
  field(name: string) {
    return this.errors.find((e) => e.field === name)?.message;
  }
}

type Tokens = { access_token: string; refresh_token: string; expires_in?: number };

const KEY = "payroll.tokens";
// shortcut: tokens in localStorage are readable by any script on the page; move the refresh token to an httpOnly cookie via a BFF route before production.
export const tokens = {
  get(): Tokens | null {
    try {
      return JSON.parse(localStorage.getItem(KEY) || "null");
    } catch {
      return null;
    }
  },
  set(t: Tokens | null) {
    if (!t) cache.clear(); // signed out: the next user must not see this user's data
    try {
      if (t) localStorage.setItem(KEY, JSON.stringify(t));
      else localStorage.removeItem(KEY);
    } catch {}
  },
};

export type Query = Record<string, string | number | boolean | null | undefined | (string | number)[]>;

export function qs(q?: Query) {
  if (!q) return "";
  const p = new URLSearchParams();
  for (const [k, v] of Object.entries(q)) {
    if (v === undefined || v === null || v === "") continue;
    if (Array.isArray(v)) v.forEach((x) => p.append(k, String(x)));
    else p.append(k, String(v));
  }
  const s = p.toString();
  return s ? `?${s}` : "";
}

type Opts = {
  method?: string;
  body?: unknown;
  query?: Query;
  /** Record version for If-Match. */
  version?: number | string;
  /** Skip the bearer token (sign-in routes). */
  anonymous?: boolean;
};

// Routes that send the record version only as an ETag (e.g. /me/profile); the latest seen per path.
const etags = new Map<string, string>();
export const etagOf = (path: string) => etags.get(path);

/** Last GET result per path + query, shown instantly on revisit while useApi refetches. Any successful write clears it. */
export const cache = new Map<string, unknown>();

let refreshing: Promise<boolean> | null = null;

async function refresh(): Promise<boolean> {
  const t = tokens.get();
  if (!t?.refresh_token) return false;
  // Refresh tokens rotate on use and reuse revokes the session, so only one refresh may be in flight.
  refreshing ??= fetch("/v1/auth/refresh", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ refresh_token: t.refresh_token }),
  })
    .then(async (r) => {
      if (!r.ok) {
        tokens.set(null);
        return false;
      }
      tokens.set(await r.json());
      return true;
    })
    .catch(() => false)
    .finally(() => {
      refreshing = null;
    });
  return refreshing;
}

async function toError(r: Response): Promise<ApiError> {
  let b: any = {};
  try {
    b = await r.json();
  } catch {}
  return new ApiError(r.status, b.code ?? `http_${r.status}`, b.title ?? r.statusText, b.detail ?? "", b.errors ?? [], b.meta);
}

export async function api<T = any>(path: string, opts: Opts = {}, retried = false): Promise<T> {
  const method = opts.method ?? (opts.body !== undefined ? "POST" : "GET");
  const headers: Record<string, string> = {};
  const isForm = typeof FormData !== "undefined" && opts.body instanceof FormData;
  if (opts.body !== undefined && !isForm) headers["Content-Type"] = "application/json";
  if (!opts.anonymous) {
    const t = tokens.get();
    if (t) headers.Authorization = `Bearer ${t.access_token}`;
  }
  // Every write gets a fresh key: routes that need one are covered, routes that don't ignore it.
  if (method !== "GET") headers["Idempotency-Key"] = crypto.randomUUID();
  if (opts.version !== undefined) headers["If-Match"] = `"${opts.version}"`;

  const r = await fetch(`/v1${path}${qs(opts.query)}`, {
    method,
    headers,
    body: opts.body === undefined ? undefined : isForm ? (opts.body as FormData) : JSON.stringify(opts.body),
  });
  if (r.status === 401 && !opts.anonymous && !retried && (await refresh())) return api<T>(path, opts, true);
  if (r.status === 401 && !opts.anonymous) {
    tokens.set(null);
    if (typeof window !== "undefined" && !location.pathname.startsWith("/login")) location.href = "/login";
  }
  if (!r.ok) throw await toError(r);
  if (method !== "GET") cache.clear();
  const tag = r.headers.get("etag");
  if (tag) etags.set(path, tag.replace(/^W\//, "").replace(/"/g, ""));
  if (r.status === 204) return undefined as T;
  const ct = r.headers.get("content-type") ?? "";
  return (ct.includes("json") ? r.json() : r.blob()) as Promise<T>;
}

/** Opens a file route (template, export) with the bearer token and saves it. */
export async function download(path: string, name: string, query?: Query) {
  const blob = await api<Blob>(path, { query });
  const url = URL.createObjectURL(blob instanceof Blob ? blob : new Blob([JSON.stringify(blob)]));
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  URL.revokeObjectURL(url);
}
