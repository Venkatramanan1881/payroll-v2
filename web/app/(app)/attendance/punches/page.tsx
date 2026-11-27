"use client";
import { useState } from "react";
import { Button, Card, ErrorBox, Field, Modal, PageHead, Pill, ReasonDialog, Select, Table, TabTools } from "@/components/ui";
import { api } from "@/lib/api";
import { today, useAction, useApi, useCan } from "@/lib/hooks";
import { useBranches } from "@/lib/lookups";
import { addDays, EmployeePicker } from "../_shared";

type Punch = {
  id: string; employee: { id: string; employee_code: string; full_name: string } | null; source: string; direction: string;
  local_time: string; local_zone: string; branch: { short_code: string } | null; device: { serial: string } | null;
  location: { fence_distance_m: number | null } | null; flags: string[]; review: { state: string } | null;
  adjudications: { kind: string; reason: string }[]; counted: boolean; voided: boolean; reason?: string;
};
const opts = (xs: string[]) => xs.map((x) => ({ value: x, label: x.replace(/_/g, " ") }));

export default function Punches() {
  const can = useCan();
  const branches = useBranches();
  const [f, setF] = useState({ from: addDays(today(), -1), to: today(), employee_id: "", branch_id: "", source: "", flag: "", review: "" });
  const [cursor, setCursor] = useState<string | undefined>();
  const list = useApi<{ items: Punch[]; next_cursor: string | null }>("/attendance/punches", { ...f, cursor });
  const [adj, setAdj] = useState<Punch | null>(null);
  const [voiding, setVoiding] = useState<Punch | null>(null);
  const act = useAction();
  const set = (k: string) => (v: string) => {
    setF({ ...f, [k]: v });
    setCursor(undefined);
  };
  const doVoid = (reason: string) =>
    act.run(() => api(`/attendance/manual-punches/${voiding!.id}/void`, { body: { reason } })).then((r) => {
      if (r) {
        setVoiding(null);
        list.reload();
      }
    });
  return (
    <>
      <PageHead title="Punches" />
      <TabTools>
        <Field label="From"><input type="date" value={f.from} onChange={(e) => set("from")(e.target.value)} /></Field>
        <Field label="To"><input type="date" value={f.to} onChange={(e) => set("to")(e.target.value)} /></Field>
        <Field label="Branch"><Select value={f.branch_id} onChange={set("branch_id")} options={branches.options} placeholder="Any" /></Field>
        <Field label="Source"><Select value={f.source} onChange={set("source")} options={opts(["device", "app", "web", "import", "manual"])} placeholder="Any" /></Field>
        <Field label="Flag"><input value={f.flag} placeholder="e.g. outside_fence" onChange={(e) => set("flag")(e.target.value)} /></Field>
        <Field label="Review"><Select value={f.review} onChange={set("review")} options={opts(["pending", "approved", "rejected"])} placeholder="Any" /></Field>
        <EmployeePicker value={f.employee_id} onChange={set("employee_id")} />
      </TabTools>
      <ErrorBox error={list.error} />
      <Card pad={false}>
        <Table
          rows={list.data?.items}
          empty="No punches in this range."
          cols={[
            { h: "Local time", c: (p) => <span title={p.local_zone}>{p.local_time?.replace("T", " ").slice(0, 16)}</span> },
            { h: "Employee", c: (p) => (p.employee ? <><span className="code">{p.employee.employee_code}</span> {p.employee.full_name}</> : <Pill value="unmapped" />) },
            { h: "Dir", c: (p) => p.direction },
            { h: "Source", c: (p) => p.source + (p.device ? ` · ${p.device.serial}` : "") },
            { h: "Branch", c: (p) => p.branch?.short_code ?? "—" },
            { h: "Flags", c: (p) => p.flags.map((x) => <Pill key={x} value={x} />) },
            { h: "Review", c: (p) => (p.review ? <Pill value={p.review.state} /> : "—") },
            { h: "Counted", c: (p) => (p.voided ? <Pill value="voided" /> : p.counted ? "Yes" : <Pill value="excluded" />) },
            {
              h: "",
              right: true,
              c: (p) =>
                p.source === "manual"
                  ? !p.voided && can("attendance.punch.manual") && <Button kind="link" onClick={() => setVoiding(p)}>Void</Button>
                  : p.employee && can("attendance.punch.adjudicate") && <Button kind="link" onClick={() => setAdj(p)}>Correct punch</Button>,
            },
          ]}
        />
      </Card>
      <div className="row" style={{ marginTop: 12 }}>
        {cursor && <Button onClick={() => setCursor(undefined)}>First page</Button>}
        {list.data?.next_cursor && <Button onClick={() => setCursor(list.data!.next_cursor!)}>Older</Button>}
      </div>
      {adj && <Adjudicate punch={adj} onClose={() => setAdj(null)} onDone={list.reload} />}
      <ReasonDialog open={!!voiding} title="Void manual punch" busy={act.busy} error={act.error} onClose={() => setVoiding(null)} onConfirm={doVoid} />
    </>
  );
}

function Adjudicate({ punch, onClose, onDone }: { punch: Punch; onClose: () => void; onDone: () => void }) {
  const [f, setF] = useState({ kind: punch.counted ? "exclude" : "include", new_work_date: "", new_direction: "in", reason: "" });
  const act = useAction();
  const save = () =>
    act
      .run(() =>
        api(`/attendance/punches/${punch.id}/adjudications`, {
          body: { kind: f.kind, reason: f.reason, new_work_date: f.kind === "move_date" ? f.new_work_date : null, new_direction: f.kind === "change_direction" ? f.new_direction : null },
        }),
      )
      .then((r) => {
        if (r) {
          onDone();
          onClose();
        }
      });
  return (
    <Modal open title="Correct punch" onClose={onClose} footer={<><Button onClick={onClose}>Cancel</Button><Button kind="main" disabled={act.busy} onClick={save}>Save</Button></>}>
      <p className="muted">{punch.employee?.full_name} · {punch.local_time?.replace("T", " ").slice(0, 16)} · {punch.direction}</p>
      {punch.adjudications.length > 0 && <div className="note">Earlier: {punch.adjudications.map((a) => `${a.kind} (${a.reason})`).join("; ")}</div>}
      <div className="grid2">
        <Field label="Action" name="kind" error={act.error}><Select value={f.kind} onChange={(v) => setF({ ...f, kind: v })} options={opts(["exclude", "include", "move_date", "change_direction"])} /></Field>
        {f.kind === "move_date" && <Field label="New work date" name="new_work_date" error={act.error}><input type="date" value={f.new_work_date} onChange={(e) => setF({ ...f, new_work_date: e.target.value })} /></Field>}
        {f.kind === "change_direction" && <Field label="New direction" name="new_direction" error={act.error}><Select value={f.new_direction} onChange={(v) => setF({ ...f, new_direction: v })} options={opts(["in", "out"])} /></Field>}
        <Field label="Reason" name="reason" error={act.error} wide><textarea rows={2} value={f.reason} onChange={(e) => setF({ ...f, reason: e.target.value })} /></Field>
      </div>
      <ErrorBox error={act.error} fields={["kind", "new_work_date", "new_direction", "reason"]} />
    </Modal>
  );
}
