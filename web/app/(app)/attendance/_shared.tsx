"use client";
import { useState } from "react";
import { Field, Select } from "@/components/ui";
import { useEmployeeOptions } from "@/lib/lookups";

/** <input type=time> gives HH:MM; the API wants a full time. */
export const toTime = (v: string) => (v ? (v.length === 5 ? `${v}:00` : v) : null);
export const hhmm = (v?: string | null) => (v ? v.slice(0, 5) : "");

/** YYYY-MM-DD plus n days. */
export const addDays = (d: string, n: number) => {
  const x = new Date(d + "T00:00:00");
  x.setDate(x.getDate() + n);
  return x.toLocaleDateString("en-CA");
};

export const DAY_STATUSES = ["present", "half_day", "absent", "absent_pending", "weekly_off", "holiday", "holiday_unpaid", "long_leave"];

export const Json = ({ value }: { value: unknown }) => <pre className="json">{JSON.stringify(value, null, 2)}</pre>;

/** Outcome of roster/manual batch writes: applied / unchanged / failed list. */
export function Outcome({ value }: { value: { applied?: number; unchanged?: number; created?: number; removed?: number; failed?: { employee_id: string; work_date?: string; code: string }[] } | null }) {
  if (!value) return null;
  return (
    <div className="note">
      {value.applied !== undefined && <>Applied {value.applied} · unchanged {value.unchanged} · </>}
      {value.created !== undefined && <>Created {value.created} · </>}
      {value.removed !== undefined && <>Removed {value.removed} · </>}
      failed {value.failed?.length ?? 0}
      {!!value.failed?.length && (
        <ul>
          {value.failed.slice(0, 50).map((f, i) => (
            <li key={i}>
              <code>{f.employee_id.slice(0, 8)}</code> {f.work_date} — {f.code.replace(/_/g, " ")}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/** One employee, searched on the server as you type. */
export function EmployeePicker({ label = "Employee", value, onChange, name, error }: { label?: string; value: string; onChange: (v: string) => void; name?: string; error?: any }) {
  const [q, setQ] = useState("");
  const emp = useEmployeeOptions(q || undefined);
  return (
    <Field label={label} name={name} error={error}>
      <Select value={value} onChange={onChange} options={emp.options} onSearch={setQ} placeholder="Choose…" />
    </Field>
  );
}
