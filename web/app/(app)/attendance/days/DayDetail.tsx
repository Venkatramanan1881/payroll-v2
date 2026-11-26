"use client";
import { useState } from "react";
import { Button, Card, ErrorBox, Field, fmtDate, fmtTime, mins, Modal, Pill, ReasonDialog, Select, Table } from "@/components/ui";
import { api } from "@/lib/api";
import { useAction, useApi } from "@/lib/hooks";
import { DAY_STATUSES, Json } from "../_shared";

const opts = (xs: string[]) => xs.map((x) => ({ value: x, label: x.replace(/_/g, " ") }));

export type Day = {
  employee: { id: string; employee_code: string; full_name: string };
  employee_id: string; work_date: string; version: number; state: string; status: string;
  shift: { code?: string; start_time?: string; end_time?: string } | null;
  first_in: string | null; last_out: string | null; net_minutes: number | null; late_minutes: number; early_minutes: number;
  late_mark: boolean; early_mark: boolean; deduction_days: number; flags: string[]; branch: { short_code: string };
};

/** One employee-day: the calculation, its explanation and HR corrections. Shared by Daily attendance and Exceptions. */
export function DayDetail({ employee, date, onClose, onChanged }: { employee: Day["employee"]; date: string; onClose: () => void; onChanged: () => void }) {
  const day = useApi<any>(`/attendance/employees/${employee.id}/days/${date}`);
  const act = useAction();
  const [ask, setAsk] = useState<null | "recalc" | { void: string }>(null);
  const [ov, setOv] = useState<{ status: string; net_minutes: string; deduction_days: string; reason: string } | null>(null);
  const [note, setNote] = useState("");
  const d = day.data;
  const after = (msg: string) => (r: unknown) => {
    if (r === undefined) return;
    setAsk(null);
    setOv(null);
    setNote(msg);
    day.reload();
    onChanged();
  };
  const recalc = (reason: string) => act.run(() => api(`/attendance/employees/${employee.id}/days/recalculate`, { body: { from: date, to: date, reason } })).then(after("Recalculation queued; reload in a few seconds."));
  const voidOv = (id: string, reason: string) => act.run(() => api(`/attendance/day-overrides/${id}/void`, { body: { reason } })).then(after("Correction voided; recalculation queued."));
  const saveOv = () => {
    if (!ov) return;
    const fields: Record<string, unknown> = {};
    if (ov.status) fields.status = ov.status;
    if (ov.net_minutes !== "") fields.net_minutes = Number(ov.net_minutes);
    if (ov.deduction_days !== "") fields.deduction_days = Number(ov.deduction_days);
    act.run(() => api(`/attendance/employees/${employee.id}/days/${date}/overrides`, { body: { fields, reason: ov.reason } })).then(after("Correction saved; recalculation queued."));
  };
  const overrides: { id: string; status?: string; net_minutes?: number; fields?: object }[] = d?.explanation?.overrides ?? [];

  return (
    <Modal open wide title={`${employee.employee_code} · ${employee.full_name} · ${fmtDate(date)}`} onClose={onClose}>
      <ErrorBox error={day.error} />
      {note && <div className="note">{note}</div>}
      {d && (
        <div className="stack">
          <div className="row">
            <Pill value={d.status} /> <Pill value={d.state} />
            {d.pending_recalculation && <Pill value="pending" />}
            <span className="muted small">v{d.version} · {d.reason} · {fmtTime(d.calculated_at)}</span>
          </div>
          <dl className="kv">
            <dt>Shift</dt><dd>{d.shift ? `${d.shift.code ?? ""} ${d.shift.start_time ?? ""}–${d.shift.end_time ?? ""}` : "—"}</dd>
            <dt>First in / last out</dt><dd>{fmtTime(d.first_in)} / {fmtTime(d.last_out)}</dd>
            <dt>Gross / break / net</dt><dd>{mins(d.gross_minutes)} / {mins(d.break_minutes)} / {mins(d.net_minutes)} (rounded {mins(d.net_minutes_rounded)})</dd>
            <dt>Late / early</dt><dd>{d.late_minutes}m {d.late_mark ? "(mark)" : ""} / {d.early_minutes}m {d.early_mark ? "(mark)" : ""}</dd>
            <dt>Deduction days</dt><dd>{d.deduction_days}</dd>
            <dt>Flags</dt><dd>{d.flags?.join(", ") || "—"}</dd>
            <dt>Policy set</dt><dd>{d.explanation?.policy_set ? `${d.explanation.policy_set.name} (${d.explanation.policy_set.source})` : "—"}</dd>
          </dl>
          <div className="row">
            {d.can?.correct && <Button icon="refresh" onClick={() => setAsk("recalc")}>Recalculate</Button>}
            {d.can?.correct && <Button icon="edit" onClick={() => setOv({ status: "", net_minutes: "", deduction_days: "", reason: "" })}>Correct day</Button>}
          </div>
          {ov && (
            <Card title="HR correction" actions={<><Button onClick={() => setOv(null)}>Cancel</Button><Button kind="main" disabled={act.busy} onClick={saveOv}>Save</Button></>}>
              <div className="grid2">
                <Field label="Status" name="fields.status" error={act.error}><Select value={ov.status} onChange={(v) => setOv({ ...ov, status: v })} options={opts(DAY_STATUSES)} placeholder="Unchanged" /></Field>
                <Field label="Net minutes (0–1440)" name="fields.net_minutes" error={act.error}><input type="number" value={ov.net_minutes} onChange={(e) => setOv({ ...ov, net_minutes: e.target.value })} /></Field>
                <Field label="Deduction days" name="fields.deduction_days" error={act.error}><Select value={ov.deduction_days} onChange={(v) => setOv({ ...ov, deduction_days: v })} options={["0", "0.5", "1"].map((x) => ({ value: x, label: x }))} placeholder="Unchanged" /></Field>
                <Field label="Reason" name="reason" error={act.error} wide><textarea rows={2} value={ov.reason} onChange={(e) => setOv({ ...ov, reason: e.target.value })} /></Field>
              </div>
            </Card>
          )}
          <ErrorBox error={ask ? null : act.error} fields={["fields.status", "fields.net_minutes", "fields.deduction_days", "reason"]} />
          {overrides.length > 0 && (
            <Card title="Corrections in force" pad={false}>
              <Table
                rows={overrides}
                cols={[
                  { h: "Status", c: (o) => o.status ?? "—" },
                  { h: "Net", c: (o) => mins(o.net_minutes) },
                  { h: "Other", c: (o) => JSON.stringify(o.fields ?? {}) },
                  { h: "", right: true, c: (o) => d.can?.correct && <Button kind="link" onClick={() => setAsk({ void: o.id })}>Void</Button> },
                ]}
              />
            </Card>
          )}
          <Card title="Requests" pad={false}>
            <Table rows={d.requests} empty="No regularisation requests." cols={[{ h: "Kind", c: (r: any) => r.kind }, { h: "State", c: (r: any) => <Pill value={r.state} /> }, { h: "Raised", c: (r: any) => fmtTime(r.created_at) }]} />
          </Card>
          <Card title="Versions" pad={false}>
            <Table rows={d.versions} cols={[{ h: "Version", c: (v: any) => v.version }, { h: "Reason", c: (v: any) => v.reason }, { h: "Calculated", c: (v: any) => fmtTime(v.calculated_at) }]} />
          </Card>
          <details>
            <summary className="muted">Explanation (rules, sessions, punches used)</summary>
            <Json value={{ sessions: d.sessions, ...d.explanation }} />
          </details>
        </div>
      )}
      <ReasonDialog
        open={!!ask}
        title={ask === "recalc" ? "Recalculate this day" : "Void correction"}
        busy={act.busy}
        error={act.error}
        onClose={() => setAsk(null)}
        onConfirm={(r) => (ask === "recalc" ? recalc(r) : ask && voidOv(ask.void, r))}
      />
    </Modal>
  );
}
