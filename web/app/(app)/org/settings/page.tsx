"use client";
import { useEffect, useState } from "react";
import { SettingMeta, SettingsForm } from "@/components/SettingsForm";
import { Button, Code, ErrorBox, Field, Icon, Loading, PageHead, Select, TabTools, Tabs } from "@/components/ui";
import { api } from "@/lib/api";
import { today, useAction, useApi, useCan } from "@/lib/hooks";
import { useBranches } from "@/lib/lookups";

type Group = "OR" | "EM" | "AC";
type Part = { part: "org_short_code" | "branch_short_code" | "running_number" | "text"; value?: string };
type Pattern = { parts: Part[]; separator: string; digits: number; start_number: number };

const PART_LABEL: Record<Part["part"], string> = {
  org_short_code: "Organisation code",
  branch_short_code: "Branch code",
  running_number: "Running number",
  text: "Text",
};

/** EM-01 custom code: the parts in order, separator, digits and first number (backend core/employee_code.rs). */
function PatternEditor({ value, onChange, readOnly }: { value: unknown; onChange: (v: Pattern) => void; readOnly?: boolean }) {
  const p = (value as Pattern) ?? { parts: [], separator: "", digits: 4, start_number: 1 };
  const set = (x: Partial<Pattern>) => onChange({ ...p, ...x });
  const setPart = (i: number, x: Part | null) => set({ parts: x ? p.parts.map((y, j) => (j === i ? x : y)) : p.parts.filter((_, j) => j !== i) });
  const move = (i: number, d: number) => {
    const parts = [...p.parts];
    [parts[i], parts[i + d]] = [parts[i + d], parts[i]];
    set({ parts });
  };
  return (
    <div className="stack" style={{ gap: 12 }}>
      <div className="code-parts">
        {p.parts.map((x, i) => (
          <div key={i} className="code-part">
            <span className="muted small">{i + 1}</span>
            <b>{PART_LABEL[x.part]}</b>
            {x.part === "text" && (
              <input value={x.value ?? ""} disabled={readOnly} placeholder="e.g. EMP" aria-label="Text" onChange={(e) => setPart(i, { part: "text", value: e.target.value })} style={{ width: 90, minHeight: 30, padding: "4px 8px" }} />
            )}
            {!readOnly && (
              <span className="row" style={{ gap: 0 }}>
                <button type="button" className="icon-btn" disabled={i === 0} onClick={() => move(i, -1)} aria-label="Move left"><Icon name="chevron_left" /></button>
                <button type="button" className="icon-btn" disabled={i === p.parts.length - 1} onClick={() => move(i, 1)} aria-label="Move right"><Icon name="chevron_right" /></button>
                <button type="button" className="icon-btn" onClick={() => setPart(i, null)} aria-label="Remove"><Icon name="close" /></button>
              </span>
            )}
          </div>
        ))}
        {!readOnly && (
          <Select
            value=""
            placeholder="Add part…"
            onChange={(v) => v && set({ parts: [...p.parts, v === "text" ? { part: "text", value: "" } : { part: v as Part["part"] }] })}
            options={Object.entries(PART_LABEL).map(([value, label]) => ({ value, label }))}
            style={{ width: 180 }}
            aria-label="Add part"
          />
        )}
      </div>
      <div className="grid3">
        <Field label="Separator">
          <input value={p.separator} disabled={readOnly} maxLength={3} placeholder="None" onChange={(e) => set({ separator: e.target.value })} />
        </Field>
        <Field label="Digits in running number">
          <input type="number" min={1} max={10} value={p.digits} disabled={readOnly} onChange={(e) => set({ digits: Number(e.target.value) })} />
        </Field>
        <Field label="First number">
          <input type="number" min={0} value={p.start_number} disabled={readOnly} onChange={(e) => set({ start_number: Number(e.target.value) })} />
        </Field>
      </div>
    </div>
  );
}

const custom = (v: Record<string, unknown>) => v.employee_code_mode === "custom";

// Labels and choices mirror backend/crates/core/src/settings.rs.
const META: Record<Group, Record<string, SettingMeta>> = {
  EM: {
    employee_code_mode: { "x-code": "EM-01", section: "Employee codes", title: "How codes are given", enum: ["automatic", "custom", "manual"], desc: "Automatic uses org + branch + running number; Custom follows your pattern; Manual means HR types each code." },
    employee_code_pattern: { "x-code": "EM-01", section: "Employee codes", title: "Code pattern", desc: "Parts joined in this order.", when: custom, custom: (v, set, ro) => <PatternEditor value={v} onChange={set} readOnly={ro} /> },
    mandatory_fields: {
      "x-code": "EM-02",
      section: "Employee record",
      title: "Also mandatory",
      desc: "Fields HR must fill in, on top of the always-required ones.",
      options: ["date_of_birth", "gender", "email", "address", "emergency_contact_name", "emergency_contact_phone", "designation_id", "contractor_id", "reporting_manager_id", "enrolment_id", "probation_end_date"],
    },
    self_edit_contact: { "x-code": "EM-03", section: "Employee record", title: "Employees may edit their contact details", desc: "Phone, email and address from the employee portal." },
    contract_requires_contractor: { "x-code": "EM-11", section: "Employee record", title: "Contract employees need a contractor", desc: "Refuse saving a contract employee without one." },
    probation_months: { "x-code": "EM-04", section: "Probation", title: "Probation length", desc: "Months from the joining date." },
    probation_reminder_days: { "x-code": "EM-04", section: "Probation", title: "Reminder before probation ends", desc: "Days before the end date that HR is reminded." },
    allowed_branches: { "x-code": "EM-06", section: "Branches", title: "Branches an employee may work at", enum: ["master_only", "chosen", "all"], desc: "Master only, chosen branches per employee, or every branch." },
    hr_allowed_branch_access: { "x-code": "EM-10", section: "Branches", title: "Branch HR on visiting employees", enum: ["not_visible", "view", "view_edit"], desc: "What a branch's HR sees of employees whose master branch is elsewhere." },
  },
  OR: {
    financial_year_start_month: { "x-code": "OR-01", section: "Calendar", title: "Financial year starts", desc: "Month number, 1 = January." },
    leave_year_start_month: { "x-code": "OR-01", section: "Calendar", title: "Leave year starts", desc: "Month number, 1 = January." },
    week_start_day: { "x-code": "OR-02", section: "Calendar", title: "Week starts on", desc: "1 = Monday … 7 = Sunday." },
    currency: { "x-code": "OR-04", section: "Regional", title: "Currency", desc: "ISO 4217 code, e.g. INR." },
    languages: { "x-code": "OR-03", section: "Regional", title: "Languages", desc: "Offered in the employee portal.", options: ["english", "tamil", "hindi"] },
  },
  AC: {
    settings_change_approval: { "x-code": "AC-04", title: "Settings changes need Org Admin approval", desc: "A change by anyone else waits until an Org Admin approves it." },
  },
};

function Preview({ pattern }: { pattern: unknown }) {
  const branches = useBranches();
  const [branch, setBranch] = useState("");
  const act = useAction();
  const [code, setCode] = useState<string | null>(null);
  const run = () =>
    act.run(() => api("/org/settings/employee-code/preview", { query: { pattern: JSON.stringify(pattern), branch_id: branch } })).then((r) => setCode(r?.next_code ?? null));
  return (
    <div className="set-preview">
      <span className="muted small">Try it</span>
      <Select value={branch} onChange={setBranch} options={branches.options} placeholder="Branch (if the pattern uses it)" style={{ width: 260 }} />
      <Button icon="visibility" onClick={run} disabled={act.busy}>Preview next code</Button>
      {code && <Code>{code}</Code>}
      <ErrorBox error={act.error} />
    </div>
  );
}

export default function OrgSettings() {
  const can = useCan();
  const edit = can("org.settings.edit");
  const [asOf, setAsOf] = useState(today());
  const data = useApi<{ as_of: string; groups: Record<Group, Record<string, unknown>> }>("/org/settings", { as_of: asOf });
  const [tab, setTab] = useState<Group>("EM");
  const [values, setValues] = useState<Record<string, unknown>>({});
  const [validFrom, setValidFrom] = useState(today());
  const [reason, setReason] = useState("");
  const [saved, setSaved] = useState(false);
  const act = useAction();

  const current = data.data?.groups[tab];
  const loaded = (() => {
    const { valid_from, ...v } = current ?? {};
    return v;
  })();
  useEffect(() => {
    setValues(loaded);
    setSaved(false);
    act.setError(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [data.data, tab]);

  const dirty = JSON.stringify(values) !== JSON.stringify(loaded);
  const save = () =>
    act.run(() => api(`/org/settings/${tab}`, { method: "PUT", body: { valid_from: validFrom, values, reason } })).then((r) => {
      if (!r) return;
      setSaved(true);
      setReason("");
      data.reload();
    });

  return (
    <>
      <PageHead title="Employee master settings" />
      <TabTools className="filters list-tabs">
        <Tabs value={tab} onChange={setTab} tabs={[{ value: "EM", label: "Employee master" }, { value: "OR", label: "Organisation" }, { value: "AC", label: "Access control" }]} />
        <Field label="Settings as of">
          <input type="date" value={asOf} onChange={(e) => setAsOf(e.target.value)} />
        </Field>
      </TabTools>
      <ErrorBox error={data.error} />
      {!current ? (
        <Loading />
      ) : (
        <div className="card settings-card">
          <div className="settings-top">
            <Icon name="history" /> Version in force from {current.valid_from ? String(current.valid_from) : "the start (defaults)"}
          </div>
          <SettingsForm values={values} onChange={setValues} meta={META[tab]} readOnly={!edit} error={act.error} />
          {tab === "EM" && custom(values) && edit && <Preview pattern={values.employee_code_pattern} />}
          {edit && (
            <div className={`save-bar${dirty ? " dirty" : ""}`}>
              <span className="muted small">{dirty ? "Unsaved changes. Settings are versioned: pick when the new version applies." : saved ? `Saved. The new version applies from ${validFrom}.` : "No changes."}</span>
              <div className="row">
                <Field label="Applies from" name="valid_from" error={act.error}>
                  <input type="date" value={validFrom} onChange={(e) => setValidFrom(e.target.value)} />
                </Field>
                <Field label="Reason" name="reason" error={act.error}>
                  <input value={reason} placeholder="Why this change" onChange={(e) => setReason(e.target.value)} />
                </Field>
                <Button onClick={() => setValues(loaded)} disabled={!dirty || act.busy}>Discard</Button>
                <Button kind="main" disabled={!dirty || act.busy} onClick={save}>Save new version</Button>
              </div>
            </div>
          )}
          <ErrorBox error={act.error} fields={["valid_from", "reason", ...Object.keys(values).map((k) => `values.${k}`)]} />
        </div>
      )}
    </>
  );
}
