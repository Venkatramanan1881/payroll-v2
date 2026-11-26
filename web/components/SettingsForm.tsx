"use client";
import { useEffect, useState } from "react";
import { ApiError } from "@/lib/api";
import { Field } from "./ui";

/** One setting's description: from the policy JSON schema, or guessed from its current value. */
export type SettingMeta = { title?: string; type?: string; enum?: string[]; "x-code"?: string; default?: unknown };

const guess = (v: unknown): string => (typeof v === "boolean" ? "boolean" : typeof v === "number" ? "integer" : typeof v === "string" ? "string" : "object");

/**
 * Edits a flat settings document. Booleans, numbers, strings and enums get inputs;
 * arrays and objects (slabs, patterns) get a JSON box. Validation is the server's (invalid_setting per key).
 */
export function SettingsForm({
  values,
  onChange,
  meta = {},
  error,
  readOnly,
}: {
  values: Record<string, unknown>;
  onChange: (v: Record<string, unknown>) => void;
  meta?: Record<string, SettingMeta>;
  error?: ApiError | null;
  readOnly?: boolean;
}) {
  const keys = Array.from(new Set([...Object.keys(meta), ...Object.keys(values)]));
  const set = (k: string, v: unknown) => onChange({ ...values, [k]: v });
  return (
    <div className="grid2">
      {keys.map((k) => {
        const m = meta[k] ?? {};
        const v = k in values ? values[k] : m.default;
        const type = m.type ?? guess(v);
        const label = `${m["x-code"] ? m["x-code"] + " · " : ""}${m.title ?? k.replace(/_/g, " ")}`;
        if (m.enum)
          return (
            <Field key={k} label={label} name={`values.${k}`} error={error}>
              <select disabled={readOnly} value={String(v ?? "")} onChange={(e) => set(k, e.target.value)}>
                {m.enum.map((o) => (
                  <option key={o} value={o}>
                    {o.replace(/_/g, " ")}
                  </option>
                ))}
              </select>
            </Field>
          );
        if (type === "boolean")
          return (
            <Field key={k} label={label} name={`values.${k}`} error={error}>
              <span className="check">
                <input type="checkbox" disabled={readOnly} checked={!!v} onChange={(e) => set(k, e.target.checked)} /> {v ? "On" : "Off"}
              </span>
            </Field>
          );
        if (type === "integer" || type === "number")
          return (
            <Field key={k} label={label} name={`values.${k}`} error={error}>
              <input type="number" disabled={readOnly} value={v == null ? "" : String(v)} onChange={(e) => set(k, e.target.value === "" ? null : Number(e.target.value))} />
            </Field>
          );
        if (type === "string" || type === "null")
          return (
            <Field key={k} label={label} name={`values.${k}`} error={error}>
              <input disabled={readOnly} value={v == null ? "" : String(v)} onChange={(e) => set(k, e.target.value === "" && type === "null" ? null : e.target.value)} />
            </Field>
          );
        return <JsonField key={k} label={label} name={`values.${k}`} value={v} error={error} readOnly={readOnly} onChange={(x) => set(k, x)} />;
      })}
    </div>
  );
}

function JsonField({ label, name, value, onChange, error, readOnly }: { label: string; name: string; value: unknown; onChange: (v: unknown) => void; error?: ApiError | null; readOnly?: boolean }) {
  const [text, setText] = useState(() => JSON.stringify(value ?? null, null, 2));
  const [bad, setBad] = useState(false);
  // Follow values that arrive or change from outside (data loading, reset), but not while the text already says the same.
  useEffect(() => {
    try {
      if (JSON.stringify(JSON.parse(text)) === JSON.stringify(value ?? null)) return;
    } catch {}
    setText(JSON.stringify(value ?? null, null, 2));
    setBad(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [JSON.stringify(value ?? null)]);
  return (
    <Field label={label} name={name} error={error} wide hint={bad ? "Not valid JSON yet" : "JSON"}>
      <textarea
        className="mono"
        rows={Math.min(10, text.split("\n").length + 1)}
        disabled={readOnly}
        value={text}
        onChange={(e) => {
          setText(e.target.value);
          try {
            onChange(JSON.parse(e.target.value));
            setBad(false);
          } catch {
            setBad(true);
          }
        }}
      />
    </Field>
  );
}
