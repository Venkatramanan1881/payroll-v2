"use client";
import { useState } from "react";
import { Button, Card, ErrorBox, Field, fmtDate, fmtTime, Modal, PageHead, Pill, ReasonDialog, Select, Table } from "@/components/ui";
import { api } from "@/lib/api";
import { today, useAccess, useAction, useApi, useCan } from "@/lib/hooks";
import { EmployeePicker, toTime } from "../_shared";

type Req = {
  id: string; employee: { id: string; employee_code: string; full_name: string }; work_date: string; kind: string;
  requested: Record<string, string | null>; reason: string; state: string; level: number; levels: number; over_limit: boolean;
  on_behalf: boolean; raised_by: string; created_at: string; version: number; current_approver: string | null;
  decisions: { level: number; action: string; actor: string | null; reason: string | null; at: string }[]; cancel_reason: string | null;
};
const opts = (xs: string[]) => xs.map((x) => ({ value: x, label: x.replace(/_/g, " ") }));
const STATES = ["pending", "escalated", "approved", "rejected", "auto_rejected", "withdrawn", "cancelled"];
const KINDS = ["missed_punch", "late", "early", "wrong_punch", "whole_day"];
const showReq = (r: Record<string, string | null>) =>
  Object.entries(r ?? {})
    .filter(([, v]) => v)
    .map(([k, v]) => `${k.replace(/_/g, " ")} ${k.endsWith("time") ? String(v).slice(0, 5) : v}`)
    .join(" · ");

export default function Regularisations() {
  const can = useCan();
  const [f, setF] = useState({ state: "pending", from: "", to: "", awaiting_me: "" });
  const list = useApi<{ items: Req[] }>("/attendance/regularisations", f);
  const [open, setOpen] = useState<string | null>(null);
  const [raising, setRaising] = useState(false);
  const set = (k: string) => (v: string) => setF({ ...f, [k]: v });
  return (
    <>
      <PageHead title="Regularisations">
        {can("attendance.regularisation.request") && <Button kind="main" icon="add" onClick={() => setRaising(true)}>Raise on behalf</Button>}
      </PageHead>
      <div className="filters">
        <Field label="State"><Select value={f.state} onChange={set("state")} options={opts(STATES)} placeholder="Any" /></Field>
        <Field label="From"><input type="date" value={f.from} onChange={(e) => set("from")(e.target.value)} /></Field>
        <Field label="To"><input type="date" value={f.to} onChange={(e) => set("to")(e.target.value)} /></Field>
        <Field label="Show"><Select value={f.awaiting_me} onChange={set("awaiting_me")} options={[{ value: "true", label: "Waiting for me" }]} placeholder="All I can see" /></Field>
      </div>
      <ErrorBox error={list.error} />
      <Card pad={false}>
        <Table
          rows={list.data?.items}
          onRow={(r) => setOpen(r.id)}
          empty="No requests."
          cols={[
            { h: "Date", c: (r) => fmtDate(r.work_date) },
            { h: "Employee", c: (r) => <><span className="code">{r.employee.employee_code}</span> {r.employee.full_name}</> },
            { h: "Kind", c: (r) => r.kind.replace(/_/g, " ") },
            { h: "Asked for", c: (r) => showReq(r.requested) },
            { h: "Level", c: (r) => `${r.level}/${r.levels}${r.over_limit ? " · over limit" : ""}` },
            { h: "Waiting on", c: (r) => r.current_approver ?? "—" },
            { h: "State", c: (r) => <Pill value={r.state} /> },
          ]}
        />
      </Card>
      {open && <Detail id={open} onClose={() => setOpen(null)} onChanged={list.reload} />}
      {raising && <Raise onClose={() => setRaising(false)} onDone={list.reload} />}
    </>
  );
}

function Detail({ id, onClose, onChanged }: { id: string; onClose: () => void; onChanged: () => void }) {
  const me = useAccess();
  const can = useCan();
  const req = useApi<Req>(`/attendance/regularisations/${id}`);
  const act = useAction();
  const [ask, setAsk] = useState<null | "approve" | "reject" | "cancel">(null);
  const r = req.data;
  const run = (path: string, body?: object) =>
    act.run(() => api(`/attendance/regularisations/${id}/${path}`, { method: "POST", body, version: r!.version })).then((x) => {
      if (x) {
        setAsk(null);
        req.reload();
        onChanged();
      }
    });
  const open = r && (r.state === "pending" || r.state === "escalated");
  return (
    <Modal open wide title="Regularisation request" onClose={onClose}>
      <ErrorBox error={req.error ?? (ask ? null : act.error)} />
      {r && (
        <div className="stack">
          <dl className="kv">
            <dt>Employee</dt><dd>{r.employee.employee_code} · {r.employee.full_name}</dd>
            <dt>Date · kind</dt><dd>{fmtDate(r.work_date)} · {r.kind.replace(/_/g, " ")}</dd>
            <dt>Asked for</dt><dd>{showReq(r.requested) || "—"}</dd>
            <dt>Reason</dt><dd>{r.reason}</dd>
            <dt>State</dt><dd><Pill value={r.state} /> level {r.level} of {r.levels}{r.current_approver ? ` · waiting on ${r.current_approver}` : ""}</dd>
            <dt>Raised</dt><dd>{fmtTime(r.created_at)}{r.on_behalf ? " (on behalf)" : ""}</dd>
            {r.cancel_reason && <><dt>Cancel reason</dt><dd>{r.cancel_reason}</dd></>}
          </dl>
          <div className="row">
            {open && can("attendance.regularisation.approve") && (
              <>
                <Button kind="main" onClick={() => setAsk("approve")}>Approve</Button>
                <Button onClick={() => setAsk("reject")}>Reject</Button>
              </>
            )}
            {open && r.raised_by === me.user.id && <Button onClick={() => run("withdraw")}>Withdraw</Button>}
            {r.state === "approved" && can("attendance.regularisation.cancel") && <Button kind="danger" onClick={() => setAsk("cancel")}>Cancel approved request</Button>}
          </div>
          <Card title="Decisions" pad={false}>
            <Table
              rows={r.decisions}
              empty="No decisions yet."
              cols={[
                { h: "Level", c: (d) => d.level },
                { h: "Action", c: (d) => <Pill value={d.action} /> },
                { h: "By", c: (d) => (d.actor ? <code>{d.actor.slice(0, 8)}</code> : "system") },
                { h: "Reason", c: (d) => d.reason ?? "—" },
                { h: "At", c: (d) => fmtTime(d.at) },
              ]}
            />
          </Card>
        </div>
      )}
      <ReasonDialog
        open={!!ask}
        title={ask === "approve" ? "Approve request" : ask === "reject" ? "Reject request" : "Cancel approved request"}
        label={ask === "approve" ? "Note" : "Reason"}
        required={ask !== "approve"}
        confirm={ask === "approve" ? "Approve" : "Confirm"}
        busy={act.busy}
        error={act.error}
        onClose={() => setAsk(null)}
        onConfirm={(text) => run(ask!, ask === "approve" ? { note: text || null } : { reason: text })}
      />
    </Modal>
  );
}

function Raise({ onClose, onDone }: { onClose: () => void; onDone: () => void }) {
  const [f, setF] = useState({ employee_id: "", work_date: today(), kind: "missed_punch", in_time: "", out_time: "", punch_id: "", direction: "", reason: "" });
  const act = useAction();
  const requested =
    f.kind === "wrong_punch"
      ? { punch_id: f.punch_id || null, direction: f.direction || null }
      : { in_time: toTime(f.in_time), out_time: toTime(f.out_time) };
  const save = () =>
    act.run(() => api("/attendance/regularisations", { body: { employee_id: f.employee_id || null, work_date: f.work_date, kind: f.kind, requested, reason: f.reason } })).then((r) => {
      if (r) {
        onDone();
        onClose();
      }
    });
  const set = (k: string) => (v: string) => setF({ ...f, [k]: v });
  return (
    <Modal open wide title="Raise regularisation on behalf" onClose={onClose} footer={<><Button onClick={onClose}>Cancel</Button><Button kind="main" disabled={act.busy} onClick={save}>Raise</Button></>}>
      <div className="grid2">
        <EmployeePicker value={f.employee_id} onChange={set("employee_id")} name="employee_id" error={act.error} />
        <Field label="Work date" name="work_date" error={act.error}><input type="date" value={f.work_date} onChange={(e) => set("work_date")(e.target.value)} /></Field>
        <Field label="Kind" name="kind" error={act.error}><Select value={f.kind} onChange={set("kind")} options={opts(KINDS)} /></Field>
        {f.kind === "wrong_punch" ? (
          <>
            <Field label="Punch id" name="requested" error={act.error} hint="From the Punches page"><input value={f.punch_id} onChange={(e) => set("punch_id")(e.target.value)} /></Field>
            <Field label="Correct direction"><Select value={f.direction} onChange={set("direction")} options={opts(["in", "out"])} placeholder="Exclude the punch" /></Field>
          </>
        ) : (
          <>
            <Field label="In time" name="requested" error={act.error}><input type="time" value={f.in_time} onChange={(e) => set("in_time")(e.target.value)} /></Field>
            <Field label="Out time"><input type="time" value={f.out_time} onChange={(e) => set("out_time")(e.target.value)} /></Field>
          </>
        )}
        <Field label="Reason" name="reason" error={act.error} wide><textarea rows={2} value={f.reason} onChange={(e) => set("reason")(e.target.value)} /></Field>
      </div>
      <ErrorBox error={act.error} fields={["employee_id", "work_date", "kind", "requested", "reason"]} />
    </Modal>
  );
}
