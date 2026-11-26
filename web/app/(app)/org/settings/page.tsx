"use client";
import { useEffect, useState } from "react";
import { SettingMeta, SettingsForm } from "@/components/SettingsForm";
import { Button, Card, Code, ErrorBox, Field, Loading, PageHead, Select, Tabs } from "@/components/ui";
import { api } from "@/lib/api";
import { today, useAction, useApi, useCan } from "@/lib/hooks";
import { useBranches } from "@/lib/lookups";

type Group = "OR" | "EM" | "AC";

// Labels and choices mirror backend/crates/core/src/settings.rs.
const META: Record<Group, Record<string, SettingMeta>> = {
  OR: {
    financial_year_start_month: { "x-code": "OR-01", title: "Financial year starts (month 1–12)" },
    leave_year_start_month: { "x-code": "OR-01", title: "Leave year starts (month 1–12)" },
    week_start_day: { "x-code": "OR-02", title: "Week starts (1 = Monday … 7 = Sunday)" },
    currency: { "x-code": "OR-04", title: "Currency (ISO 4217)" },
  },
  EM: {
    employee_code_mode: { "x-code": "EM-01", title: "Employee code", enum: ["automatic", "custom", "manual"] },
    employee_code_pattern: { "x-code": "EM-01", title: "Code pattern (used when custom)" },
    self_edit_contact: { "x-code": "EM-03", title: "Employees may edit their contact details" },
    probation_months: { "x-code": "EM-04", title: "Probation months" },
    probation_reminder_days: { "x-code": "EM-04", title: "Probation reminder, days before end" },
    allowed_branches: { "x-code": "EM-06", title: "Allowed branches", enum: ["master_only", "chosen", "all"] },
    hr_allowed_branch_access: { "x-code": "EM-10", title: "Branch HR on allowed-branch employees", enum: ["not_visible", "view", "view_edit"] },
    contract_requires_contractor: { "x-code": "EM-11", title: "Contract employees need a contractor" },
  },
  AC: {
    settings_change_approval: { "x-code": "AC-04", title: "Settings changes need Org Admin approval" },
  },
};
/** Array settings shown as checkboxes instead of JSON. */
const LISTS: Record<string, { code: string; title: string; options: string[] }> = {
  mandatory_fields: {
    code: "EM-02",
    title: "Also mandatory",
    options: ["date_of_birth", "gender", "email", "address", "emergency_contact_name", "emergency_contact_phone", "designation_id", "contractor_id", "reporting_manager_id", "enrolment_id", "probation_end_date"],
  },
  languages: { code: "OR-03", title: "Languages", options: ["english", "tamil", "hindi"] },
};

function Preview({ pattern }: { pattern: unknown }) {
  const branches = useBranches();
  const [branch, setBranch] = useState("");
  const act = useAction();
  const [code, setCode] = useState<string | null>(null);
  const run = () =>
    act.run(() => api("/org/settings/employee-code/preview", { query: { pattern: JSON.stringify(pattern), branch_id: branch } })).then((r) => setCode(r?.next_code ?? null));
  return (
    <div className="filters" style={{ marginTop: 12 }}>
      <Select value={branch} onChange={setBranch} options={branches.options} placeholder="Branch (if the pattern uses it)" />
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

  useEffect(() => {
    const { valid_from, ...v } = data.data?.groups[tab] ?? {};
    setValues(v);
    setSaved(false);
    act.setError(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [data.data, tab]);

  const current = data.data?.groups[tab];
  const plain = Object.fromEntries(Object.entries(values).filter(([k]) => !LISTS[k]));
  const save = () =>
    act.run(() => api(`/org/settings/${tab}`, { method: "PUT", body: { valid_from: validFrom, values, reason } })).then((r) => {
      if (!r) return;
      setSaved(true);
      setReason("");
      data.reload();
    });

  return (
    <>
      <PageHead title="Employee master settings" sub="Organisation-wide rules. Each save is a new dated version; history is kept.">
        <Field label="Showing settings as of">
          <input type="date" value={asOf} onChange={(e) => setAsOf(e.target.value)} />
        </Field>
      </PageHead>
      <div style={{ marginBottom: 16 }}>
        <Tabs value={tab} onChange={setTab} tabs={[{ value: "EM", label: "Employee master" }, { value: "OR", label: "Organisation" }, { value: "AC", label: "Access control" }]} />
      </div>
      <ErrorBox error={data.error} />
      {!current ? (
        <Loading />
      ) : (
        <Card title={`In force from ${current.valid_from ?? "the start (defaults)"}`}>
          <SettingsForm values={plain} onChange={(v) => setValues({ ...values, ...v })} meta={META[tab]} readOnly={!edit} error={act.error} />
          {Object.keys(values)
            .filter((k) => LISTS[k])
            .map((k) => {
              const l = LISTS[k];
              const on = (values[k] as string[]) ?? [];
              return (
                <Field key={k} label={`${l.code} · ${l.title}`} wide>
                  <span className="row">
                    {l.options.map((o) => (
                      <label key={o} className="check">
                        <input type="checkbox" disabled={!edit} checked={on.includes(o)} onChange={(e) => setValues({ ...values, [k]: e.target.checked ? [...on, o] : on.filter((x) => x !== o) })} />{" "}
                        {o.replace(/_id$/, "").replace(/_/g, " ")}
                      </label>
                    ))}
                  </span>
                </Field>
              );
            })}
          {tab === "EM" && values.employee_code_mode === "custom" && edit && <Preview pattern={values.employee_code_pattern} />}
          {edit && (
            <div className="filters" style={{ marginTop: 20, alignItems: "end" }}>
              <Field label="Applies from" name="valid_from" error={act.error}>
                <input type="date" value={validFrom} onChange={(e) => setValidFrom(e.target.value)} />
              </Field>
              <Field label="Reason" name="reason" error={act.error}>
                <input value={reason} onChange={(e) => setReason(e.target.value)} />
              </Field>
              <Button kind="main" disabled={act.busy} onClick={save}>Save new version</Button>
            </div>
          )}
          {saved && <div className="note">Saved. The new version applies from {validFrom}.</div>}
          <ErrorBox error={act.error} fields={["valid_from", "reason", ...Object.keys(values).map((k) => `values.${k}`)]} />
        </Card>
      )}
    </>
  );
}
