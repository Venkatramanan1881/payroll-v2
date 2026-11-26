"use client";
import { useApi } from "./hooks";
import type { Opt } from "@/components/ui";

type Item = { id: string; code?: string; short_code?: string; name?: string; state?: string };

const label = (i: Item) => [i.short_code ?? i.code, i.name].filter(Boolean).join(" · ");

/** Active branches as select options. */
export function useBranches() {
  const r = useApi<{ items: Item[] }>("/org/branches", { state: "active" });
  return { ...r, options: (r.data?.items ?? []).map((i) => ({ value: i.id, label: label(i) })) as Opt[] };
}

export type ListName = "departments" | "designations" | "categories" | "employee-types" | "exit-reasons";

/** Active items of one organisation list as select options. */
export function useList(list: ListName) {
  const r = useApi<{ items: Item[] }>(`/org/lists/${list}`, { state: "active" });
  return { ...r, options: (r.data?.items ?? []).map((i) => ({ value: i.id, label: label(i) })) as Opt[] };
}

export function useContractors() {
  const r = useApi<{ items: Item[] }>("/org/contractors", { state: "active" });
  return { ...r, options: (r.data?.items ?? []).map((i) => ({ value: i.id, label: label(i) })) as Opt[] };
}

export function usePayrollGroups() {
  const r = useApi<{ items: Item[] }>("/org/payroll-groups");
  return { ...r, options: (r.data?.items ?? []).filter((i) => i.state !== "inactive").map((i) => ({ value: i.id, label: i.name ?? i.id })) as Opt[] };
}

export function useShifts() {
  const r = useApi<{ items: Item[] }>("/attendance/shifts", { state: "active" });
  return { ...r, options: (r.data?.items ?? []).map((i) => ({ value: i.id, label: label(i) })) as Opt[] };
}

/** Employees matching q, as options (first page only). */
export function useEmployeeOptions(q?: string) {
  const r = useApi<{ items: { id: string; employee_code: string; full_name: string }[] }>("/employees", { q, limit: 50 });
  return { ...r, options: (r.data?.items ?? []).map((e) => ({ value: e.id, label: `${e.employee_code} · ${e.full_name}` })) as Opt[] };
}
