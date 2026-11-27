"use client";
import { useState } from "react";
import { Button, Card, ErrorBox, Field, fmtDate, Modal, PageHead, Pill, ReasonDialog, Select, Table } from "@/components/ui";
import { api } from "@/lib/api";
import { today, useAction, useApi, useCan } from "@/lib/hooks";
import { useBranches, useShifts } from "@/lib/lookups";
import { addDays } from "../_shared";

type Waiver = { id: string; kind: string; work_date: string; branch_id: string | null; shift_id: string | null; reason: string; state: string; withdraw_reason: string | null };

export default function WaiversAndOutages() {
  const can = useCan();
  const branches = useBranches();
  const shifts = useShifts();
  const [from, setFrom] = useState(addDays(today(), -30));
  const [to, setTo] = useState(addDays(today(), 30));
  const waive = can("attendance.waiver.manage");
  const list = useApi<{ items: Waiver[] }>(waive ? "/attendance/waivers" : null, { from, to });
  const [creating, setCreating] = useState(false);
  const [withdrawing, setWithdrawing] = useState<Waiver | null>(null);
  const act = useAction();
  const label = (opts: { value: string; label: string }[], id: string | null) => (id ? opts.find((o) => o.value === id)?.label ?? id.slice(0, 8) : "—");
  const withdraw = (reason: string) =>
    act.run(() => api(`/attendance/waivers/${withdrawing!.id}/withdraw`, { body: { reason } })).then((r) => {
      if (r) {
        setWithdrawing(null);
        list.reload();
      }
    });
  return (
    <>
      <PageHead title="Waivers & outages">
        {waive && <Button kind="main" icon="add" onClick={() => setCreating(true)}>New waiver</Button>}
      </PageHead>
      {waive && (
        <>
          <div className="filters">
            <Field label="From"><input type="date" value={from} onChange={(e) => setFrom(e.target.value)} /></Field>
            <Field label="To"><input type="date" value={to} onChange={(e) => setTo(e.target.value)} /></Field>
          </div>
          <ErrorBox error={list.error ?? (withdrawing ? null : act.error)} />
          <Card title="Waivers" pad={false}>
            <Table
              rows={list.data?.items}
              empty="No waivers in this range."
              cols={[
                { h: "Date", c: (w) => fmtDate(w.work_date) },
                { h: "Kind", c: (w) => w.kind },
                { h: "Branch", c: (w) => label(branches.options, w.branch_id) },
                { h: "Shift", c: (w) => label(shifts.options, w.shift_id) },
                { h: "Reason", c: (w) => w.reason },
                { h: "State", c: (w) => <Pill value={w.state} /> },
                { h: "", right: true, c: (w) => w.state === "active" && <Button kind="link" onClick={() => setWithdrawing(w)}>Withdraw</Button> },
              ]}
            />
          </Card>
        </>
      )}
      {can("attendance.outage.regularise") && <Outage branches={branches.options} />}
      {creating && <NewWaiver branches={branches.options} shifts={shifts.options} onClose={() => setCreating(false)} onDone={list.reload} />}
      <ReasonDialog open={!!withdrawing} title="Withdraw waiver" busy={act.busy} error={act.error} onClose={() => setWithdrawing(null)} onConfirm={withdraw} />
    </>
  );
}

type Opt = { value: string; label: string };

function NewWaiver({ branches, shifts, onClose, onDone }: { branches: Opt[]; shifts: Opt[]; onClose: () => void; onDone: () => void }) {
  const [f, setF] = useState({ kind: "late", work_date: today(), branch_id: "", shift_id: "", reason: "" });
  const act = useAction();
  const save = () =>
    act.run(() => api("/attendance/waivers", { body: { ...f, branch_id: f.branch_id || null, shift_id: f.shift_id || null } })).then((r) => {
      if (r) {
        onDone();
        onClose();
      }
    });
  return (
    <Modal open title="New waiver" onClose={onClose} footer={<><Button onClick={onClose}>Cancel</Button><Button kind="main" disabled={act.busy} onClick={save}>Create</Button></>}>
      <div className="grid2">
        <Field label="Kind" name="kind" error={act.error}><Select value={f.kind} onChange={(v) => setF({ ...f, kind: v })} options={[{ value: "late", label: "Late arrival" }, { value: "early", label: "Early departure" }]} /></Field>
        <Field label="Date" name="work_date" error={act.error}><input type="date" value={f.work_date} onChange={(e) => setF({ ...f, work_date: e.target.value })} /></Field>
        <Field label="Branch" name="branch_id" error={act.error}><Select value={f.branch_id} onChange={(v) => setF({ ...f, branch_id: v })} options={branches} placeholder="Any branch" /></Field>
        <Field label="Shift" name="shift_id" error={act.error}><Select value={f.shift_id} onChange={(v) => setF({ ...f, shift_id: v })} options={shifts} placeholder="Any shift" /></Field>
        <Field label="Reason" name="reason" error={act.error} wide><textarea rows={2} value={f.reason} onChange={(e) => setF({ ...f, reason: e.target.value })} /></Field>
      </div>
      <p className="muted small">Give a branch, a shift, or both.</p>
      <ErrorBox error={act.error} fields={["kind", "work_date", "branch_id", "shift_id", "reason"]} />
    </Modal>
  );
}

function Outage({ branches }: { branches: Opt[] }) {
  const [f, setF] = useState({ branch_id: "", from: addDays(today(), -1), to: addDays(today(), -1), reason: "" });
  const act = useAction();
  const [result, setResult] = useState<{ affected_employees: number; queued: number } | null>(null);
  const save = () => act.run(() => api("/attendance/outage-regularisations", { body: f })).then((r) => r && setResult(r));
  return (
    <Card title="Device outage fill" actions={<Button kind="main" disabled={act.busy || !f.branch_id} onClick={save}>Fill missing punches</Button>}>
      <p className="muted" style={{ marginTop: 0 }}>Fills missing punches from shift times as manual punches for everyone at the branch. At most 31 days, not in the future.</p>
      <div className="grid2">
        <Field label="Branch" name="branch_id" error={act.error}><Select value={f.branch_id} onChange={(v) => setF({ ...f, branch_id: v })} options={branches} placeholder="Choose…" /></Field>
        <Field label="From" name="from" error={act.error}><input type="date" value={f.from} onChange={(e) => setF({ ...f, from: e.target.value })} /></Field>
        <Field label="To" name="to" error={act.error}><input type="date" value={f.to} onChange={(e) => setF({ ...f, to: e.target.value })} /></Field>
        <Field label="Reason" name="reason" error={act.error}><input value={f.reason} onChange={(e) => setF({ ...f, reason: e.target.value })} /></Field>
      </div>
      {result && <div className="note">{result.affected_employees} employees affected · {result.queued} employees queued for recalculation.</div>}
      <ErrorBox error={act.error} fields={["branch_id", "from", "to", "reason"]} />
    </Card>
  );
}
