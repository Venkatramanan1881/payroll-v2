"use client";
import { createContext, useCallback, useContext, useEffect, useRef, useState } from "react";
import { api, ApiError, cache, Query } from "./api";

/** GET a route; reload() refetches. Pass null to skip. Revisits show the cached result at once and refresh quietly. */
export function useApi<T = any>(path: string | null, query?: Query) {
  const key = path ? path + JSON.stringify(query ?? {}) : null;
  const [data, setState] = useState<T | null>(() => (key ? ((cache.get(key) as T) ?? null) : null));
  const [error, setError] = useState<ApiError | null>(null);
  const [loading, setLoading] = useState(!!key && !cache.has(key));
  // Only the latest request may land: an older, slower response must not overwrite newer data or a user's edits.
  const seq = useRef(0);
  const load = useCallback(async () => {
    if (!path || !key) return;
    const n = ++seq.current;
    const hit = cache.has(key);
    if (hit) setState(cache.get(key) as T);
    setLoading(!hit);
    setError(null);
    try {
      const d = await api<T>(path, { query });
      cache.set(key, d);
      if (n === seq.current) setState(d);
    } catch (e) {
      if (n === seq.current) setError(e instanceof ApiError ? e : new ApiError(0, "network", "Network error", String(e)));
    } finally {
      if (n === seq.current) setLoading(false);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  useEffect(() => {
    load();
  }, [load]);
  const setData = useCallback(
    (v: T | null | ((p: T | null) => T | null)) =>
      setState((p) => {
        const next = typeof v === "function" ? (v as (p: T | null) => T | null)(p) : v;
        if (key) cache.set(key, next);
        return next;
      }),
    [key],
  );
  return { data, error, loading, reload: load, setData };
}

/** Runs a write and keeps its error for display. */
export function useAction() {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<ApiError | null>(null);
  const run = async <T,>(fn: () => Promise<T>): Promise<T | undefined> => {
    setBusy(true);
    setError(null);
    try {
      return await fn();
    } catch (e) {
      setError(e instanceof ApiError ? e : new ApiError(0, "network", "Network error", String(e)));
    } finally {
      setBusy(false);
    }
  };
  return { busy, error, setError, run };
}

export type Access = {
  user: { id: string; name: string; kind: string; email: string | null };
  org: { name: string; short_code: string; includes_mobile_app: boolean; state: string };
  permissions: string[];
  scope: { kind: string; branch_ids: string[]; department_ids: string[] };
  employee_id: string | null;
};

export const AccessContext = createContext<Access | null>(null);

export function useAccess() {
  return useContext(AccessContext)!;
}

/** True when the user holds any of the codes. UI hint only; the server checks again. */
export function useCan() {
  const a = useContext(AccessContext);
  return (...codes: string[]) => !!a && codes.some((c) => a.permissions.includes(c));
}

/** Today as YYYY-MM-DD in the browser's zone. */
export const today = () => new Date().toLocaleDateString("en-CA");
