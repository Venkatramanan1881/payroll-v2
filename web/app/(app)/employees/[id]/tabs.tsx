"use client";
import { useState } from "react";
import { Button, Card, Code, ErrorBox, Field, fmtDate, fmtTime, Modal, PhoneInput, Pill, ReasonDialog, Select, Table } from "@/components/ui";
import { api } from "@/lib/api";
import { today, useAction, useApi, useCan } from "@/lib/hooks";
import { useList, useShifts } from "@/lib/lookups";
import { Job, jobIds, JobFields, named, statusOpts } from "../parts";

type P = { e: any; reload: () => void };
const ok = async (p: Promise<unknown>) => (await p, true);
const PERSONAL = ["full_name", "mobile", "date_of_birth", "gender", "email", "address", "emergency_contact_name", "emergency_contact_phone"];

function Kv({ rows }: { rows: [string, React.ReactNode][] }) {
  return (
    <dl className="kv">
      {rows.map(([k, v]) => (
        <div key={k} style={{ display: "contents" }}>
          <dt>{k}</dt>
          <dd>{v ?? "—"}</dd>
        </div>
      ))}
    </dl>
  );
}

// ---------- Overview: personal (E8), employment, job, enrolment status (E11) ----------
const cap = (k: string) => k.charAt(0).toUpperCase() + k.slice(1).replace(/_/g, " ");

export function OverviewTab({ e, reload }: P) {
  const act = useAction();
  const [edit, setEdit] = useState<Record<string, string> | null>(null);
  const savePersonal = () => {
    // Only changed fields; empty → null (clears an optional field).
    const body = Object.fromEntries(PERSONAL.filter((k) => (edit![k] ?? "") !== (e.personal[k] ?? "")).map((k) => [k, edit![k] || null]));
    act.run(() => ok(api(`/employees/${e.id}/personal`, { method: "PATCH", body, version: e.version }))).then((r) => r && (setEdit(null), reload()));
  };
  const setEnrol = (v: string) => act.run(() => ok(api(`/employees/${e.id}/enrolment-status`, { method: "PATCH", body: { enrolment_status: v }, version: e.version }))).then((r) => r && reload());
  const j = e.job ?? {};
  return (
    <div className="profile-grid">
      <ErrorBox error={edit ? null : act.error} />
      <Card title="Personal" actions={e.can?.edit && <Button icon="edit" onClick={() => setEdit(Object.fromEntries(PERSONAL.map((k) => [k, e.personal[k] ?? ""])))}>Edit</Button>}>
        <Kv rows={PERSONAL.map((k) => [cap(k), e.personal[k]])} />
      </Card>
      <Card title="Employment">
        <Kv
          rows={[
            ["Joining date", fmtDate(e.employment?.joining_date)],
            ["Probation ends", fmtDate(e.employment?.probation_end_date)],
            ["Last working day", fmtDate(e.employment?.last_working_day)],
            ["Exit reason", e.employment?.exit_reason?.name],
            ["Status", <><Pill value={e.status?.value} /> since {fmtDate(e.status?.since)}</>],
            ["Portal access", <Pill value={e.portal_access} />],
            [
              "Enrolment status",
              e.can?.edit ? (
                <Select
                  value={e.enrolment_status}
                  onChange={setEnrol}
                  options={["not_enrolled", "enrolled", "re_enrolment_needed"].map((v) => ({ value: v, label: v.replace(/_/g, " ") }))}
                  style={{ maxWidth: 240 }}
                />
              ) : (
                <Pill value={e.enrolment_status} />
              ),
            ],
            ["Scheduled job changes", e.scheduled_changes_count],
          ]}
        />
      </Card>
      <Card title={`Job (from ${fmtDate(j.valid_from)})`}>
        <Kv
          rows={[
            ["Master branch", named(j.master_branch)],
            ["Allowed branches", (j.allowed_branches ?? []).map(named).join(", ") || "—"],
            ["Department", named(j.department)],
            ["Designation", named(j.designation)],
            ["Category", named(j.category)],
            ["Employee type", named(j.employee_type)],
            ["Contractor", named(j.contractor)],
            ["Reporting manager", named(j.reporting_manager)],
            ["Payroll group", j.payroll_group?.name],
            ["Enrolment ID", j.enrolment_id && <Code>{j.enrolment_id}</Code>],
          ]}
        />
      </Card>
      <Modal
        open={!!edit}
        title="Edit personal details"
        onClose={() => setEdit(null)}
        footer={
          <>
            <Button onClick={() => setEdit(null)}>Cancel</Button>
            <Button kind="main" onClick={savePersonal} disabled={act.busy}>Save</Button>
          </>
        }
      >
        {edit && (
          <div className="grid2">
            {PERSONAL.map((k) => (
              <Field key={k} label={cap(k)} name={`personal.${k}`} error={act.error}>
                {k === "mobile" || k === "emergency_contact_phone" ? (
                  <PhoneInput value={edit[k]} onChange={(v) => setEdit({ ...edit, [k]: v })} />
                ) : (
                  <input type={k === "date_of_birth" ? "date" : "text"} value={edit[k]} onChange={(x) => setEdit({ ...edit, [k]: x.target.value })} />
                )}
              </Field>
            ))}
          </div>
        )}
        <ErrorBox error={act.error} fields={PERSONAL.map((k) => `personal.${k}`)} />
      </Modal>
    </div>
  );
}

// ---------- Job change (E6) and cancel scheduled (E7) ----------
export function JobTab({ e, reload }: P) {
  const act = useAction();
  const [from, setFrom] = useState(today());
  const [reason, setReason] = useState("");
  const [changes, setChanges] = useState<Job>({});
  const [done, setDone] = useState<any>(null);
  const hist = useApi<any>(`/employees/${e.id}/history`);
  const scheduled = (hist.data?.job_versions ?? []).filter((v: any) => v.state === "scheduled");
  const [cancel, setCancel] = useState<string | null>(null);
  const save = () =>
    act.run(() => api(`/employees/${e.id}/job-changes`, { body: { effective_from: from, changes, reason }, version: e.version })).then((r) => {
      if (!r) return;
      setDone(r);
      setChanges({});
      setReason("");
      reload();
      hist.reload();
    });
  return (
    <div className="stack">
      <Card title="Dated job change" actions={<Button kind="main" onClick={save} disabled={act.busy || !Object.keys(changes).length}>Save change</Button>}>
        <div className="grid2" style={{ marginBottom: 14 }}>
          <Field label="Effective from" name="effective_from" error={act.error} hint="A past date supersedes from that date">
            <input type="date" value={from} onChange={(x) => setFrom(x.target.value)} />
          </Field>
          <Field label="Reason" name="reason" error={act.error}>
            <input value={reason} onChange={(x) => setReason(x.target.value)} />
          </Field>
        </div>
        <p className="muted small">Change only the fields that move; the rest stay as they are.</p>
        <JobFields value={{ ...jobIds(e.job), ...changes }} onChange={(k, v) => setChanges({ ...changes, [k]: v })} error={act.error} prefix="changes." />
        <ErrorBox error={act.error} fields={["effective_from", "reason", ...act.error?.errors.map((x) => x.field).filter((f) => f.startsWith("changes.")) ?? []]} />
        {done && (
          <div className="note">
            Saved{done.scheduled ? " as a scheduled change" : ""}.
            {done.prompts?.length > 0 && <> Also review: {done.prompts.map((p: string) => p.replace(/_/g, " ")).join(", ")}.</>}
          </div>
        )}
      </Card>
      <Card title="Scheduled changes" pad={false}>
        <Table
          rows={hist.data ? scheduled : null}
          empty="No scheduled changes."
          cols={[
            { h: "From", c: (v: any) => fmtDate(v.valid_from) },
            { h: "Changes", c: (v: any) => v.changes.map((c: any) => c.field).join(", ") || "—" },
            { h: "Reason", c: (v: any) => v.reason },
            { h: "", c: (v: any) => <Button kind="link" onClick={() => setCancel(v.version_id)}>Cancel</Button>, right: true },
          ]}
        />
      </Card>
      <ReasonDialog
        open={!!cancel}
        title="Cancel scheduled change"
        onClose={() => setCancel(null)}
        error={act.error}
        busy={act.busy}
        onConfirm={(reason) =>
          act.run(() => ok(api(`/employees/${e.id}/job-changes/${cancel}/cancel`, { body: { reason } }))).then((r) => r && (setCancel(null), reload(), void hist.reload()))
        }
      />
    </div>
  );
}

// ---------- History (E3) ----------
export function HistoryTab({ e }: P) {
  const h = useApi<any>(`/employees/${e.id}/history`);
  const val = (v: unknown) => (v == null ? "—" : Array.isArray(v) ? v.length + " items" : String(v));
  return (
    <div className="stack">
      <ErrorBox error={h.error} />
      <Card title="Job versions" pad={false}>
        <Table
          rows={h.data?.job_versions}
          cols={[
            { h: "From", c: (v: any) => fmtDate(v.valid_from) },
            { h: "To", c: (v: any) => fmtDate(v.valid_to) },
            { h: "State", c: (v: any) => <Pill value={v.state} /> },
            { h: "Changes", c: (v: any) => v.changes.map((c: any) => `${c.field}: ${val(c.old)} → ${val(c.new)}`).join("; ") || "First version" },
            { h: "Reason", c: (v: any) => v.reason },
            { h: "Recorded", c: (v: any) => `${v.recorded_by ?? ""} ${fmtTime(v.recorded_at)}` },
          ]}
        />
      </Card>
      <Card title="Status" pad={false}>
        <Table rows={h.data?.status} cols={[{ h: "Status", c: (s: any) => <Pill value={s.status} /> }, { h: "From", c: (s: any) => fmtDate(s.effective_from) }, { h: "Superseded", c: (s: any) => fmtTime(s.superseded_at) }]} />
      </Card>
      <Card title="Employment periods" pad={false}>
        <Table
          rows={h.data?.periods}
          cols={[
            { h: "Joined", c: (p: any) => fmtDate(p.joining_date) },
            { h: "Last day", c: (p: any) => fmtDate(p.last_working_day) },
            { h: "Exit reason", c: (p: any) => p.exit_reason?.name ?? "—" },
            { h: "State", c: (p: any) => <Pill value={p.state} /> },
          ]}
        />
      </Card>
      <Card title="Personal detail changes" pad={false}>
        <Table rows={h.data?.personal_changes} empty="No changes." cols={[{ h: "Field", c: (c: any) => c.field }, { h: "Old", c: (c: any) => val(c.old) }, { h: "New", c: (c: any) => val(c.new) }, { h: "By", c: (c: any) => c.by }, { h: "When", c: (c: any) => fmtTime(c.at) }]} />
      </Card>
    </div>
  );
}

// ---------- Status change request (S1) ----------
export function StatusTab({ e, reload }: P) {
  const act = useAction();
  const exitReasons = useList("exit-reasons");
  const reqs = useApi<any>("/status-requests", { employee_id: e.id });
  const [f, setF] = useState({ to_status: "", effective_date: today(), reason: "", last_working_day: "", exit_reason_id: "", remarks: "" });
  const exit = f.to_status === "exited";
  const raise = () =>
    act
      .run(() =>
        api(`/employees/${e.id}/status-requests`, {
          body: {
            to_status: f.to_status || null,
            effective_date: f.effective_date || null,
            reason: f.reason,
            ...(exit ? { exit: { last_working_day: f.last_working_day || null, exit_reason_id: f.exit_reason_id || null, remarks: f.remarks || null } } : {}),
          },
        }),
      )
      .then((r) => r && (reload(), reqs.reload()));
  return (
    <div className="stack">
      {e.can?.raise_status && !e.pending_status_request && (
        <Card title="Raise a status change" actions={<Button kind="main" onClick={raise} disabled={act.busy}>Raise request</Button>}>
          <div className="grid2">
            <Field label="New status" name="to_status" error={act.error}>
              <Select value={f.to_status} onChange={(v) => setF({ ...f, to_status: v })} options={statusOpts.filter((o) => o.value !== e.status?.value)} placeholder="Choose…" />
            </Field>
            <Field label="Effective date" name="effective_date" error={act.error} hint={exit ? "For an exit: the day after the last working day" : undefined}>
              <input type="date" value={f.effective_date} onChange={(x) => setF({ ...f, effective_date: x.target.value })} />
            </Field>
            {exit && (
              <>
                <Field label="Last working day" name="exit.last_working_day" error={act.error}>
                  <input type="date" value={f.last_working_day} onChange={(x) => setF({ ...f, last_working_day: x.target.value })} />
                </Field>
                <Field label="Exit reason" name="exit.exit_reason_id" error={act.error}>
                  <Select value={f.exit_reason_id} onChange={(v) => setF({ ...f, exit_reason_id: v })} options={exitReasons.options} placeholder="Choose…" />
                </Field>
                <Field label="Remarks" name="exit.remarks" error={act.error} wide>
                  <input value={f.remarks} onChange={(x) => setF({ ...f, remarks: x.target.value })} />
                </Field>
              </>
            )}
            <Field label="Reason" name="reason" error={act.error} wide>
              <textarea rows={2} value={f.reason} onChange={(x) => setF({ ...f, reason: x.target.value })} />
            </Field>
          </div>
          <ErrorBox error={act.error} fields={["to_status", "effective_date", "reason", "exit.last_working_day", "exit.exit_reason_id", "exit.remarks"]} />
        </Card>
      )}
      <Card title="Requests" pad={false}>
        <ErrorBox error={reqs.error} />
        <Table
          rows={reqs.data?.items}
          empty="No status requests."
          cols={[
            { h: "Change", c: (r: any) => <>{r.from_status} → <b>{r.to_status}</b></> },
            { h: "Effective", c: (r: any) => fmtDate(r.effective_date) },
            { h: "State", c: (r: any) => <Pill value={r.state} /> },
            { h: "Reason", c: (r: any) => r.reason },
            { h: "Raised", c: (r: any) => `${r.raised_by?.name ?? ""} ${fmtTime(r.raised_at)}` },
          ]}
        />
      </Card>
      <p className="muted small">Approve, reject or cancel requests from Employees → Status requests.</p>
    </div>
  );
}

// ---------- Portal access and PINs (E13–E16, E4) and phone binding (P3) ----------
export function PortalTab({ e, reload }: P) {
  const can = useCan();
  const act = useAction();
  const devices = useApi<any[]>(e.can?.issue_pin ? `/employees/${e.id}/devices` : null);
  const [pin, setPin] = useState<any>(null);
  const [sendEmail, setSendEmail] = useState(true);
  const [ask, setAsk] = useState<"remove" | "binding" | null>(null);
  const has = e.portal_access === "employee_portal";
  const issue = (path: string) => act.run(() => api(path, { body: { send_email: sendEmail } })).then((r) => r && (setPin(r), reload()));
  return (
    <div className="stack">
      {e.can?.issue_pin && (
        <Card title="Employee portal">
          <div className="stack">
            <div className="row">
              Access: <Pill value={e.portal_access} />
            </div>
            <label className="check">
              <input type="checkbox" checked={sendEmail} onChange={(x) => setSendEmail(x.target.checked)} /> Email the temporary PIN to the employee
            </label>
            <div className="row">
              {!has && <Button kind="main" onClick={() => issue(`/employees/${e.id}/portal-access`)} disabled={act.busy}>Grant access</Button>}
              {has && <Button icon="password" onClick={() => issue(`/employees/${e.id}/pin-reset`)} disabled={act.busy}>Reset PIN</Button>}
              {has && <Button icon="lock_open" onClick={() => act.run(() => ok(api(`/employees/${e.id}/unlock`, { method: "POST" }))).then((r) => r && reload())} disabled={act.busy}>Unlock</Button>}
              {has && <Button kind="danger" onClick={() => setAsk("remove")}>Remove access</Button>}
            </div>
            <ErrorBox error={ask ? null : act.error} />
          </div>
        </Card>
      )}
      {e.can?.issue_pin && (
        <Card title="Signed-in devices" pad={false} actions={can("attendance.device_binding.reset") && <Button onClick={() => setAsk("binding")}>Reset phone binding</Button>}>
          <Table rows={devices.data} empty="No devices yet." cols={[{ h: "Platform", c: (d: any) => d.platform }, { h: "First seen", c: (d: any) => fmtTime(d.first_seen_at) }, { h: "Last seen", c: (d: any) => fmtTime(d.last_seen_at) }]} />
        </Card>
      )}
      {!e.can?.issue_pin && can("attendance.device_binding.reset") && (
        <Card title="Phone binding" actions={<Button onClick={() => setAsk("binding")}>Reset phone binding</Button>}>
          <p className="muted">The next app sign-in binds the new phone.</p>
        </Card>
      )}
      <Modal open={!!pin} title="Temporary PIN" onClose={() => setPin(null)} footer={<Button kind="main" onClick={() => setPin(null)}>Done</Button>}>
        {pin && (
          <div className="stack">
            <p>Give this PIN to the employee now. It is shown only once and is not stored.</p>
            <div style={{ fontSize: 32, letterSpacing: 6 }}>
              <Code>{pin.temporary_pin}</Code>
            </div>
            <p className="muted small">
              Expires {fmtTime(pin.expires_at)} · {pin.emailed ? "Emailed to the employee" : "Not emailed"}
            </p>
          </div>
        )}
      </Modal>
      <ReasonDialog
        open={!!ask}
        title={ask === "remove" ? "Remove portal access" : "Reset phone binding"}
        confirm={ask === "remove" ? "Remove" : "Reset"}
        onClose={() => setAsk(null)}
        error={act.error}
        busy={act.busy}
        onConfirm={(reason) =>
          act
            .run(() =>
              ok(
                ask === "remove"
                  ? api(`/employees/${e.id}/portal-access`, { method: "DELETE", body: { reason } })
                  : api(`/attendance/employees/${e.id}/device-binding/reset`, { body: { reason } }),
              ),
            )
            .then((r) => r && (setAsk(null), reload(), void devices.reload()))
        }
      />
    </div>
  );
}

// ---------- Probation (E9) ----------
export function ProbationTab({ e, reload }: P) {
  const act = useAction();
  const [f, setF] = useState({ probation_end_date: e.employment?.probation_end_date ?? "", reason: "" });
  const save = () => act.run(() => ok(api(`/employees/${e.id}/probation`, { method: "PATCH", body: f, version: e.version }))).then((r) => r && reload());
  return (
    <Card title="Probation" actions={<Button kind="main" onClick={save} disabled={act.busy}>Save</Button>}>
      <div className="grid2">
        <Field label="Probation end date" name="probation_end_date" error={act.error}>
          <input type="date" value={f.probation_end_date} onChange={(x) => setF({ ...f, probation_end_date: x.target.value })} />
        </Field>
        <Field label="Reason" name="reason" error={act.error}>
          <input value={f.reason} onChange={(x) => setF({ ...f, reason: x.target.value })} />
        </Field>
      </div>
      <ErrorBox error={act.error} fields={["probation_end_date", "reason"]} />
    </Card>
  );
}

// ---------- Custom fields (E10) ----------
export function CustomFieldsTab({ e, reload }: P) {
  const defs = useApi<{ items: any[] }>("/org/custom-fields", { state: "active" });
  const act = useAction();
  const [v, setV] = useState<Record<string, any>>(e.custom_fields ?? {});
  const changed = Object.fromEntries(Object.entries(v).filter(([k, x]) => JSON.stringify(x) !== JSON.stringify(e.custom_fields?.[k])));
  const save = () => act.run(() => ok(api(`/employees/${e.id}/custom-fields`, { method: "PUT", body: changed, version: e.version }))).then((r) => r && reload());
  const set = (k: string, x: any) => setV({ ...v, [k]: x === "" ? null : x });
  return (
    <Card title="Custom fields" actions={e.can?.edit && <Button kind="main" onClick={save} disabled={act.busy || !Object.keys(changed).length}>Save</Button>}>
      <ErrorBox error={defs.error} />
      {defs.data && !defs.data.items.length && <p className="muted">No custom fields are set up. Add them under Organisation → Custom fields.</p>}
      <div className="grid2">
        {defs.data?.items.map((d) => {
          const x = v[d.id];
          const ro = !e.can?.edit;
          const active = (d.options ?? []).filter((o: string) => !(d.inactive_options ?? []).includes(o) || o === x || (Array.isArray(x) && x.includes(o)));
          return (
            <Field key={d.id} label={d.label + (d.is_mandatory ? " *" : "")} name={d.id} error={act.error}>
              {d.field_type === "yes_no" ? (
                <Select disabled={ro} value={x == null ? "" : String(x)} onChange={(s) => set(d.id, s === "" ? null : s === "true")} options={[{ value: "true", label: "Yes" }, { value: "false", label: "No" }]} placeholder="—" />
              ) : d.field_type === "dropdown" ? (
                <Select disabled={ro} value={x ?? ""} onChange={(s) => set(d.id, s)} options={active.map((o: string) => ({ value: o, label: o }))} placeholder="—" />
              ) : d.field_type === "multiple_choice" ? (
                <select multiple disabled={ro} value={x ?? []} onChange={(ev) => set(d.id, Array.from(ev.target.selectedOptions, (o) => o.value))}>
                  {active.map((o: string) => (
                    <option key={o}>{o}</option>
                  ))}
                </select>
              ) : (
                <input
                  disabled={ro}
                  type={d.field_type === "number" ? "number" : d.field_type === "date" ? "date" : "text"}
                  value={x ?? ""}
                  onChange={(ev) => set(d.id, d.field_type === "number" && ev.target.value !== "" ? Number(ev.target.value) : ev.target.value)}
                />
              )}
            </Field>
          );
        })}
      </div>
      <ErrorBox error={act.error} fields={defs.data?.items.map((d) => d.id)} />
    </Card>
  );
}

// ---------- Attendance setup: shifts (R7) and policy set (PS8, PS7) ----------
export function AttendanceTab({ e, reload }: P) {
  const can = useCan();
  const shifts = useShifts();
  const sets = useApi<{ items: any[] }>(can("attendance.policy.manage", "attendance.day.view") ? "/attendance/policy-sets" : null);
  const resolved = useApi<any>(can("attendance.day.view") ? `/attendance/employees/${e.id}/policy-set` : null);
  const sh = useAction();
  const ps = useAction();
  const [s, setS] = useState({ effective_from: today(), default_shift_id: "", allowed_shift_ids: [] as string[], reason: "" });
  const [p, setP] = useState({ effective_from: today(), policy_set_id: "", reason: "" });
  const saveShifts = () =>
    sh.run(() => ok(api(`/attendance/employees/${e.id}/shifts`, { method: "PUT", body: { ...s, default_shift_id: s.default_shift_id || null }, version: e.version }))).then((r) => r && reload());
  const savePolicy = () =>
    ps
      .run(() => ok(api(`/attendance/employees/${e.id}/policy-set`, { method: "PUT", body: { ...p, policy_set_id: p.policy_set_id || null }, version: e.version })))
      .then((r) => r && (reload(), void resolved.reload()));
  return (
    <div className="stack">
      {can("attendance.roster.manage") && (
        <Card title="Shifts" actions={<Button kind="main" onClick={saveShifts} disabled={sh.busy}>Save shifts</Button>}>
          <div className="grid2">
            <Field label="Effective from" name="effective_from" error={sh.error}>
              <input type="date" value={s.effective_from} onChange={(x) => setS({ ...s, effective_from: x.target.value })} />
            </Field>
            <Field label="Default shift" name="default_shift_id" error={sh.error}>
              <Select value={s.default_shift_id} onChange={(v) => setS({ ...s, default_shift_id: v })} options={shifts.options} placeholder="— None —" />
            </Field>
            <Field label="Allowed shifts" name="allowed_shift_ids" error={sh.error} hint="Ctrl/Cmd-click for several">
              <select multiple value={s.allowed_shift_ids} onChange={(x) => setS({ ...s, allowed_shift_ids: Array.from(x.target.selectedOptions, (o) => o.value) })}>
                {shifts.options.map((o) => (
                  <option key={o.value} value={o.value}>
                    {o.label}
                  </option>
                ))}
              </select>
            </Field>
            <Field label="Reason" name="reason" error={sh.error}>
              <input value={s.reason} onChange={(x) => setS({ ...s, reason: x.target.value })} />
            </Field>
          </div>
          <ErrorBox error={sh.error} fields={["effective_from", "default_shift_id", "allowed_shift_ids", "reason"]} />
        </Card>
      )}
      <Card title="Attendance policy set" actions={can("attendance.policy.manage") && <Button kind="main" onClick={savePolicy} disabled={ps.busy}>Save policy</Button>}>
        <ErrorBox error={resolved.error} />
        {resolved.data !== undefined && (
          <p>
            In force today: <b>{resolved.data?.policy_set?.name ?? "none"}</b>
            {resolved.data?.source && <span className="muted"> · from {resolved.data.source.replace(/_/g, " ")}</span>}
          </p>
        )}
        {can("attendance.policy.manage") && (
          <div className="grid2">
            <Field label="Effective from" name="effective_from" error={ps.error}>
              <input type="date" value={p.effective_from} onChange={(x) => setP({ ...p, effective_from: x.target.value })} />
            </Field>
            <Field label="Employee's own policy set" name="policy_set_id" error={ps.error} hint="None = inherit from type, category, department or branch">
              <Select value={p.policy_set_id} onChange={(v) => setP({ ...p, policy_set_id: v })} options={(sets.data?.items ?? []).filter((x) => x.state !== "inactive").map((x) => ({ value: x.id, label: x.name }))} placeholder="— Inherit —" />
            </Field>
            <Field label="Reason" name="reason" error={ps.error} wide>
              <input value={p.reason} onChange={(x) => setP({ ...p, reason: x.target.value })} />
            </Field>
          </div>
        )}
        <ErrorBox error={ps.error} fields={["effective_from", "policy_set_id", "reason"]} />
      </Card>
    </div>
  );
}

// ---------- Rejoin (E12) ----------
export function RejoinTab({ e, reload }: P) {
  const act = useAction();
  const [f, setF] = useState({ joining_date: today(), probation_end_date: "", initial_status: "active", mobile: "", reason: "" });
  const [job, setJob] = useState<Job>(jobIds(e.job));
  const save = () =>
    act
      .run(() =>
        api(`/employees/${e.id}/rejoin`, {
          body: { ...f, probation_end_date: f.probation_end_date || null, mobile: f.mobile || null, job: Object.fromEntries(Object.entries(job).filter(([, v]) => v !== "" && v != null)) },
          version: e.version,
        }),
      )
      .then((r) => r && reload());
  return (
    <Card title="Rejoin" actions={<Button kind="main" onClick={save} disabled={act.busy}>Rejoin employee</Button>}>
      <p className="muted small">Keeps the employee code and starts a new employment period.</p>
      <div className="grid2" style={{ marginBottom: 14 }}>
        <Field label="Joining date" name="joining_date" error={act.error}>
          <input type="date" value={f.joining_date} onChange={(x) => setF({ ...f, joining_date: x.target.value })} />
        </Field>
        <Field label="Initial status" name="initial_status" error={act.error}>
          <Select value={f.initial_status} onChange={(v) => setF({ ...f, initial_status: v })} options={[{ value: "active", label: "Active" }, { value: "probation", label: "Probation" }]} />
        </Field>
        <Field label="Probation end date" name="probation_end_date" error={act.error}>
          <input type="date" value={f.probation_end_date} onChange={(x) => setF({ ...f, probation_end_date: x.target.value })} />
        </Field>
        <Field label="New mobile (if the old one is taken)" name="mobile" error={act.error}>
          <PhoneInput value={f.mobile} onChange={(v) => setF({ ...f, mobile: v })} />
        </Field>
        <Field label="Reason" name="reason" error={act.error} wide>
          <input value={f.reason} onChange={(x) => setF({ ...f, reason: x.target.value })} />
        </Field>
      </div>
      <JobFields value={job} onChange={(k, v) => setJob({ ...job, [k]: v })} error={act.error} prefix="job." />
      <ErrorBox error={act.error} fields={["joining_date", "initial_status", "probation_end_date", "mobile", "reason"]} />
    </Card>
  );
}
