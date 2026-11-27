"use client";
import { useState } from "react";
import { Button, Card, ErrorBox, Field, Loading, Modal, PageHead, Select } from "@/components/ui";
import { api } from "@/lib/api";
import { today, useAction, useApi, useCan } from "@/lib/hooks";
import { useBranches, useList, useShifts } from "@/lib/lookups";
import { addDays, Outcome } from "../_shared";

type Entry = { shift_id?: string | null; code?: string | null; is_weekly_off?: boolean; source?: string; holiday?: { name: string; kind: string } };
type Row = { employee: { id: string; employee_code: string; full_name: string }; entries: Record<string, Entry> };
type View = { from: string; to: string; rows: Row[]; next_cursor: string | null };

const dates = (from: string, to: string) => {
  const out: string[] = [];
  for (let d = from; d <= to && out.length < 62; d = addDays(d, 1)) out.push(d);
  return out;
};

export default function Roster() {
  const can = useCan();
  const manage = can("attendance.roster.manage");
  const branches = useBranches();
  const departments = useList("departments");
  const shifts = useShifts();
  const [from, setFrom] = useState(today());
  const [to, setTo] = useState(addDays(today(), 13));
  const [branch, setBranch] = useState("");
  const [dept, setDept] = useState("");
  const [cursor, setCursor] = useState<string | undefined>();
  const view = useApi<View>("/attendance/roster", { from, to, branch_id: branch, department_id: dept, cursor });
  const [cell, setCell] = useState<{ row: Row; date: string } | null>(null);
  const [dialog, setDialog] = useState<null | "copy" | "rotation" | "clear">(null);
  const [outcome, setOutcome] = useState<any>(null);
  const done = (o: any) => {
    setOutcome(o);
    setCell(null);
    setDialog(null);
    view.reload();
  };
  const days = dates(from, to);

  return (
    <>
      <PageHead title="Roster">
        {manage && (
          <>
            <Button icon="content_copy" onClick={() => setDialog("copy")}>Copy</Button>
            <Button icon="autorenew" onClick={() => setDialog("rotation")}>Rotation</Button>
            <Button icon="backspace" onClick={() => setDialog("clear")}>Clear</Button>
          </>
        )}
      </PageHead>
      <div className="filters">
        <Field label="From"><input type="date" value={from} onChange={(e) => { setFrom(e.target.value); setCursor(undefined); }} /></Field>
        <Field label="To (≤ 62 days)"><input type="date" value={to} onChange={(e) => { setTo(e.target.value); setCursor(undefined); }} /></Field>
        <Field label="Branch"><Select value={branch} onChange={(v) => { setBranch(v); setCursor(undefined); }} options={branches.options} placeholder="Any" /></Field>
        <Field label="Department"><Select value={dept} onChange={(v) => { setDept(v); setCursor(undefined); }} options={departments.options} placeholder="Any" /></Field>
      </div>
      <Outcome value={outcome} />
      <ErrorBox error={view.error} />
      <Card pad={false}>
        {!view.data ? (
          <Loading />
        ) : (
          <div className="table-wrap">
            <table className="table">
              <thead>
                <tr>
                  <th>Employee</th>
                  {days.map((d) => (
                    <th key={d} style={{ textAlign: "center" }}>
                      {new Date(d + "T00:00:00").toLocaleDateString("en-IN", { weekday: "short" })}
                      <br />
                      {d.slice(8)}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {view.data.rows.map((r) => (
                  <tr key={r.employee.id}>
                    <td style={{ whiteSpace: "nowrap" }}>
                      <span className="code">{r.employee.employee_code}</span> {r.employee.full_name}
                    </td>
                    {days.map((d) => {
                      const e = r.entries[d];
                      const text = e?.is_weekly_off ? "WO" : e?.code ?? (e?.holiday ? "H" : "·");
                      return (
                        <td
                          key={d}
                          title={[e?.source, e?.holiday && `${e.holiday.name} (${e.holiday.kind})`].filter(Boolean).join(" · ")}
                          onClick={manage ? () => setCell({ row: r, date: d }) : undefined}
                          className={manage ? "click" : undefined}
                          style={{ textAlign: "center", cursor: manage ? "pointer" : undefined, color: e?.source === "default" ? "var(--muted)" : undefined, background: e?.holiday ? "var(--tint)" : undefined }}
                        >
                          {text}
                        </td>
                      );
                    })}
                  </tr>
                ))}
              </tbody>
            </table>
            {!view.data.rows.length && <div className="empty">No employees match.</div>}
          </div>
        )}
      </Card>
      <div className="row" style={{ marginTop: 12 }}>
        {cursor && <Button onClick={() => setCursor(undefined)}>First page</Button>}
        {view.data?.next_cursor && <Button onClick={() => setCursor(view.data!.next_cursor!)}>Next 100 employees</Button>}
        <span className="muted small">Grey = default shift · WO = weekly off · H = holiday</span>
      </div>
      {cell && <CellDialog cell={cell} shifts={shifts.options} onClose={() => setCell(null)} onDone={done} />}
      {dialog === "copy" && <CopyDialog branches={branches.options} departments={departments.options} onClose={() => setDialog(null)} onDone={done} />}
      {dialog === "rotation" && <RotationDialog rows={view.data?.rows ?? []} shifts={shifts.options} onClose={() => setDialog(null)} onDone={done} />}
      {dialog === "clear" && <ClearDialog rows={view.data?.rows ?? []} from={from} to={to} onClose={() => setDialog(null)} onDone={done} />}
    </>
  );
}

type Opt = { value: string; label: string };

function CellDialog({ cell, shifts, onClose, onDone }: { cell: { row: Row; date: string }; shifts: Opt[]; onClose: () => void; onDone: (o: any) => void }) {
  const cur = cell.row.entries[cell.date];
  const [choice, setChoice] = useState(cur?.is_weekly_off ? "weekly_off" : cur?.shift_id ?? "");
  const act = useAction();
  const save = () =>
    act.run(() =>
      api("/attendance/roster", {
        method: "PUT",
        body: { entries: [{ employee_id: cell.row.employee.id, work_date: cell.date, shift_id: choice && choice !== "weekly_off" ? choice : null, is_weekly_off: choice === "weekly_off" }] },
      }),
    ).then((r) => r && onDone(r));
  return (
    <Modal open title={`${cell.row.employee.full_name} · ${cell.date}`} onClose={onClose} footer={<><Button onClick={onClose}>Cancel</Button><Button kind="main" disabled={act.busy || !choice} onClick={save}>Save</Button></>}>
      <Field label="Shift">
        <Select value={choice} onChange={setChoice} options={[{ value: "weekly_off", label: "Weekly off" }, ...shifts]} placeholder="Choose…" />
      </Field>
      {cur?.holiday && <div className="note">Holiday: {cur.holiday.name}</div>}
      <ErrorBox error={act.error} />
    </Modal>
  );
}

function CopyDialog({ branches, departments, onClose, onDone }: { branches: Opt[]; departments: Opt[]; onClose: () => void; onDone: (o: any) => void }) {
  const [f, setF] = useState({ from: addDays(today(), -7), to: addDays(today(), -1), to_start: today(), branch_id: "", department_id: "" });
  const act = useAction();
  const save = () => act.run(() => api("/attendance/roster/copy", { body: { ...f, branch_id: f.branch_id || null, department_id: f.department_id || null } })).then((r) => r && onDone(r));
  return (
    <Modal open title="Copy roster forward" onClose={onClose} footer={<><Button onClick={onClose}>Cancel</Button><Button kind="main" disabled={act.busy} onClick={save}>Copy</Button></>}>
      <div className="grid2">
        <Field label="Copy from" name="from" error={act.error}><input type="date" value={f.from} onChange={(e) => setF({ ...f, from: e.target.value })} /></Field>
        <Field label="Copy to" name="to" error={act.error}><input type="date" value={f.to} onChange={(e) => setF({ ...f, to: e.target.value })} /></Field>
        <Field label="Paste starting" name="to_start" error={act.error}><input type="date" value={f.to_start} onChange={(e) => setF({ ...f, to_start: e.target.value })} /></Field>
        <Field label="Branch"><Select value={f.branch_id} onChange={(v) => setF({ ...f, branch_id: v })} options={branches} placeholder="All in scope" /></Field>
        <Field label="Department"><Select value={f.department_id} onChange={(v) => setF({ ...f, department_id: v })} options={departments} placeholder="All" /></Field>
      </div>
      <ErrorBox error={act.error} fields={["from", "to", "to_start"]} />
    </Modal>
  );
}

const WEEKDAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

function RotationDialog({ rows, shifts, onClose, onDone }: { rows: Row[]; shifts: Opt[]; onClose: () => void; onDone: (o: any) => void }) {
  const [ids, setIds] = useState<string[]>([]);
  const [start, setStart] = useState(today());
  const [weeks, setWeeks] = useState(4);
  const [pattern, setPattern] = useState<string[]>([""]);
  const [offs, setOffs] = useState<number[]>([7]);
  const act = useAction();
  const save = () => act.run(() => api("/attendance/roster/rotation", { body: { employee_ids: ids, start, weeks, pattern: pattern.filter(Boolean), weekly_off_days: offs } })).then((r) => r && onDone(r));
  const toggle = <T,>(xs: T[], x: T) => (xs.includes(x) ? xs.filter((y) => y !== x) : [...xs, x]);
  return (
    <Modal open wide title="Rotation" onClose={onClose} footer={<><Button onClick={onClose}>Cancel</Button><Button kind="main" disabled={act.busy} onClick={save}>Apply</Button></>}>
      <div className="grid2 form-section">
        <Field label="Start" name="start" error={act.error}><input type="date" value={start} onChange={(e) => setStart(e.target.value)} /></Field>
        <Field label="Weeks (1–26)" name="weeks" error={act.error}><input type="number" min={1} max={26} value={weeks} onChange={(e) => setWeeks(Number(e.target.value))} /></Field>
        <Field label="Weekly off days" name="weekly_off_days" error={act.error} wide>
          <span className="row">
            {WEEKDAYS.map((w, i) => (
              <label key={w} className="check"><input type="checkbox" checked={offs.includes(i + 1)} onChange={() => setOffs(toggle(offs, i + 1))} /> {w}</label>
            ))}
          </span>
        </Field>
        <Field label="Pattern (one slot per week, repeats)" name="pattern" error={act.error} wide>
          <div className="stack" style={{ gap: 8 }}>
            {pattern.map((p, i) => (
              <div className="row" key={i}>
                <span className="muted small">Week {i + 1}</span>
                <Select value={p} onChange={(v) => setPattern(pattern.map((x, j) => (j === i ? v : x)))} options={[{ value: "weekly_off", label: "Weekly off" }, ...shifts]} placeholder="Choose…" style={{ width: 260 }} />
                {pattern.length > 1 && <Button kind="link" onClick={() => setPattern(pattern.filter((_, j) => j !== i))}>Remove</Button>}
              </div>
            ))}
            <Button kind="link" onClick={() => setPattern([...pattern, ""])}>Add slot</Button>
          </div>
        </Field>
      </div>
      <Field label="Employees (from the grid)" name="employee_ids" error={act.error}>
        <div className="grid2">
          {rows.map((r) => (
            <label key={r.employee.id} className="check"><input type="checkbox" checked={ids.includes(r.employee.id)} onChange={() => setIds(toggle(ids, r.employee.id))} /> {r.employee.employee_code} · {r.employee.full_name}</label>
          ))}
        </div>
      </Field>
      <ErrorBox error={act.error} fields={["start", "weeks", "weekly_off_days", "pattern", "employee_ids"]} />
    </Modal>
  );
}

function ClearDialog({ rows, from, to, onClose, onDone }: { rows: Row[]; from: string; to: string; onClose: () => void; onDone: (o: any) => void }) {
  const [f, setF] = useState({ employee_id: "", from, to, reason: "" });
  const act = useAction();
  const save = () => act.run(() => api("/attendance/roster/entries", { method: "DELETE", body: f })).then((r) => r && onDone(r));
  return (
    <Modal open title="Clear roster entries" onClose={onClose} footer={<><Button onClick={onClose}>Cancel</Button><Button kind="danger" disabled={act.busy} onClick={save}>Clear</Button></>}>
      <p className="muted">Days go back to the employee&apos;s default shift.</p>
      <div className="grid2">
        <Field label="Employee" name="employee_id" error={act.error}>
          <Select value={f.employee_id} onChange={(v) => setF({ ...f, employee_id: v })} options={rows.map((r) => ({ value: r.employee.id, label: `${r.employee.employee_code} · ${r.employee.full_name}` }))} placeholder="Choose…" />
        </Field>
        <Field label="From" name="from" error={act.error}><input type="date" value={f.from} onChange={(e) => setF({ ...f, from: e.target.value })} /></Field>
        <Field label="To" name="to" error={act.error}><input type="date" value={f.to} onChange={(e) => setF({ ...f, to: e.target.value })} /></Field>
        <Field label="Reason" name="reason" error={act.error} wide><textarea rows={2} value={f.reason} onChange={(e) => setF({ ...f, reason: e.target.value })} /></Field>
      </div>
      <ErrorBox error={act.error} fields={["employee_id", "from", "to", "reason"]} />
    </Modal>
  );
}
