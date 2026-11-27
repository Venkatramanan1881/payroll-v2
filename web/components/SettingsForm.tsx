"use client";
import { useEffect, useState } from "react";
import { ApiError } from "@/lib/api";
import { optLabel, Select } from "./ui";

/** One setting's description: from the policy JSON schema, or guessed from its current value. */
export type SettingMeta = {
  title?: string;
  type?: string;
  enum?: string[];
  "x-code"?: string;
  default?: unknown;
  /** One line under the title saying what the setting does. */
  desc?: string;
  /** Settings with the same section are shown together under that heading. */
  section?: string;
  /** Checkbox list for an array setting (instead of a JSON box). */
  options?: string[];
  /** Hide the row unless this returns true for the current values. */
  when?: (values: Record<string, unknown>) => boolean;
  /** Own editor for a structured value (shown full width). */
  custom?: (value: unknown, set: (v: unknown) => void, readOnly?: boolean) => React.ReactNode;
};

const guess = (v: unknown): string => (typeof v === "boolean" ? "boolean" : typeof v === "number" ? "integer" : typeof v === "string" ? "string" : "object");

export function Switch({ on, onChange, disabled, label }: { on: boolean; onChange: (v: boolean) => void; disabled?: boolean; label: string }) {
  return (
    <button type="button" role="switch" aria-checked={on} aria-label={label} className={`switch${on ? " on" : ""}`} disabled={disabled} onClick={() => onChange(!on)}>
      <span />
    </button>
  );
}

/**
 * Edits a flat settings document as rows: what the setting is on the left, its control on the right.
 * Booleans get a switch, enums a dropdown, numbers and strings an input, lists of options toggle chips,
 * other arrays and objects (slabs, patterns) a JSON box. Validation is the server's (invalid_setting per key).
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
  const keys = Array.from(new Set([...Object.keys(meta), ...Object.keys(values)])).filter((k) => !meta[k]?.when || meta[k].when!(values));
  const set = (k: string, v: unknown) => onChange({ ...values, [k]: v });
  // Keep the order of first appearance for sections; settings without one go under "General".
  const sections: [string, string[]][] = [];
  for (const k of keys) {
    const s = meta[k]?.section ?? "General";
    const at = sections.find(([n]) => n === s);
    if (at) at[1].push(k);
    else sections.push([s, [k]]);
  }

  const control = (k: string, m: SettingMeta, v: unknown, type: string, title: string) => {
    if (m.custom) return m.custom(v, (x) => set(k, x), readOnly);
    if (m.options) {
      const on = (v as string[]) ?? [];
      return (
        <div className="chips">
          {m.options.map((o) => (
            <button
              key={o}
              type="button"
              className={`chip${on.includes(o) ? " on" : ""}`}
              aria-pressed={on.includes(o)}
              disabled={readOnly}
              onClick={() => set(k, on.includes(o) ? on.filter((x) => x !== o) : [...on, o])}
            >
              {on.includes(o) && <span className="ms">check</span>}
              {optLabel(o.replace(/_id$/, ""))}
            </button>
          ))}
        </div>
      );
    }
    if (m.enum) return <Select disabled={readOnly} value={String(v ?? "")} onChange={(x) => set(k, x)} options={m.enum.map((o) => ({ value: o, label: o }))} aria-label={title} />;
    if (type === "boolean") return <Switch on={!!v} disabled={readOnly} onChange={(x) => set(k, x)} label={title} />;
    if (type === "integer" || type === "number")
      return <input type="number" aria-label={title} disabled={readOnly} value={v == null ? "" : String(v)} onChange={(e) => set(k, e.target.value === "" ? null : Number(e.target.value))} />;
    if (type === "string" || type === "null")
      return <input aria-label={title} disabled={readOnly} value={v == null ? "" : String(v)} onChange={(e) => set(k, e.target.value === "" && type === "null" ? null : e.target.value)} />;
    return <JsonBox value={v} readOnly={readOnly} label={title} onChange={(x) => set(k, x)} />;
  };

  return (
    <div className="settings">
      {sections.map(([name, ks]) => (
        <section key={name} className="set-section">
          {sections.length > 1 && <h3>{name}</h3>}
          <div className="set-list">
            {ks.map((k) => {
              const m = meta[k] ?? {};
              const v = k in values ? values[k] : m.default;
              const type = m.type ?? guess(v);
              const title = m.title ?? optLabel(k);
              const wide = !!m.options || !!m.custom || (!m.enum && !["boolean", "integer", "number", "string", "null"].includes(type));
              const msg = error?.field(`values.${k}`);
              return (
                <div key={k} className={`set-row${wide ? " wide" : ""}${msg ? " bad" : ""}`}>
                  <div className="set-txt">
                    <b>{title}</b>
                    {m.desc && <small>{m.desc}</small>}
                    {m["x-code"] && <span className="set-code">{m["x-code"]}</span>}
                  </div>
                  <div className="set-ctl">
                    {control(k, m, v, type, title)}
                    {msg && <small className="err">{msg}</small>}
                  </div>
                </div>
              );
            })}
          </div>
        </section>
      ))}
    </div>
  );
}

function JsonBox({ value, onChange, readOnly, label }: { value: unknown; onChange: (v: unknown) => void; readOnly?: boolean; label: string }) {
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
    <>
      <textarea
        className="mono"
        aria-label={label}
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
      <small className={bad ? "err" : "muted"}>{bad ? "Not valid JSON yet" : "JSON"}</small>
    </>
  );
}
