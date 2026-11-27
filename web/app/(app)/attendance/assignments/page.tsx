"use client";
import { useState } from "react";
import { Button, Card, ErrorBox, Field, fmtDate, Modal, PageHead, Select, Table, TabTools } from "@/components/ui";
import { api } from "@/lib/api";
import { today, useAction, useApi } from "@/lib/hooks";
import { useBranches, useList } from "@/lib/lookups";

type Assignment = { id: string; policy_set_id: string; level: string; target_id: string; valid_from: string; valid_to: string | null; state: string };
const LEVELS = [
  { value: "branch", label: "Branch" },
  { value: "department", label: "Department" },
  { value: "category", label: "Category" },
  { value: "employee_type", label: "Employee type" },
];

export default function PolicyAssignments() {
  const sets = useApi<{ items: { id: string; name: string; state: string }[] }>("/attendance/policy-sets");
  const targets: Record<string, { value: string; label: string }[]> = {
    branch: useBranches().options,
    department: useList("departments").options,
    category: useList("categories").options,
    employee_type: useList("employee-types").options,
  };
  const [setId, setSetId] = useState("");
  const [level, setLevel] = useState("");
  const list = useApi<{ items: Assignment[] }>("/attendance/policy-assignments", { policy_set_id: setId, level });
  const [creating, setCreating] = useState(false);
  const [ending, setEnding] = useState<Assignment | null>(null);
  const setOpts = (sets.data?.items ?? []).map((s) => ({ value: s.id, label: s.name }));
  const name = (opts: { value: string; label: string }[], id: string) => opts.find((o) => o.value === id)?.label ?? id.slice(0, 8);
  return (
    <>
      <PageHead title="Policy assignments">
        <Button kind="main" icon="add" onClick={() => setCreating(true)}>Assign</Button>
      </PageHead>
      <TabTools>
        <Field label="Policy set"><Select value={setId} onChange={setSetId} options={setOpts} placeholder="Any" /></Field>
        <Field label="Level"><Select value={level} onChange={setLevel} options={LEVELS} placeholder="Any" /></Field>
      </TabTools>
      <ErrorBox error={list.error ?? sets.error} />
      <Card pad={false}>
        <Table
          rows={list.data?.items}
          empty="No active assignments; everyone gets the default policy set."
          cols={[
            { h: "Level", c: (a) => LEVELS.find((l) => l.value === a.level)?.label ?? a.level },
            { h: "Target", c: (a) => name(targets[a.level] ?? [], a.target_id) },
            { h: "Policy set", c: (a) => name(setOpts, a.policy_set_id) },
            { h: "From", c: (a) => fmtDate(a.valid_from) },
            { h: "To", c: (a) => (a.valid_to ? fmtDate(a.valid_to) : "open") },
            { h: "", right: true, c: (a) => <Button kind="link" onClick={() => setEnding(a)}>End</Button> },
          ]}
        />
      </Card>
      {creating && <Assign sets={setOpts} targets={targets} onClose={() => setCreating(false)} onDone={list.reload} />}
      {ending && <End a={ending} onClose={() => setEnding(null)} onDone={list.reload} />}
    </>
  );
}

function Assign({ sets, targets, onClose, onDone }: { sets: { value: string; label: string }[]; targets: Record<string, { value: string; label: string }[]>; onClose: () => void; onDone: () => void }) {
  const [f, setF] = useState({ policy_set_id: "", level: "branch", target_id: "", valid_from: today(), reason: "" });
  const act = useAction();
  const save = () =>
    act.run(() => api("/attendance/policy-assignments", { body: f })).then((r) => {
      if (r) {
        onDone();
        onClose();
      }
    });
  return (
    <Modal open title="Assign policy set" onClose={onClose} footer={<><Button onClick={onClose}>Cancel</Button><Button kind="main" disabled={act.busy} onClick={save}>Assign</Button></>}>
      <div className="grid2">
        <Field label="Policy set" name="policy_set_id" error={act.error}><Select value={f.policy_set_id} onChange={(v) => setF({ ...f, policy_set_id: v })} options={sets} placeholder="Choose…" /></Field>
        <Field label="Level" name="level" error={act.error}><Select value={f.level} onChange={(v) => setF({ ...f, level: v, target_id: "" })} options={LEVELS} /></Field>
        <Field label="Target" name="target_id" error={act.error}><Select value={f.target_id} onChange={(v) => setF({ ...f, target_id: v })} options={targets[f.level] ?? []} placeholder="Choose…" /></Field>
        <Field label="From" name="valid_from" error={act.error}><input type="date" value={f.valid_from} onChange={(e) => setF({ ...f, valid_from: e.target.value })} /></Field>
        <Field label="Reason" name="reason" error={act.error} wide><textarea rows={2} value={f.reason} onChange={(e) => setF({ ...f, reason: e.target.value })} /></Field>
      </div>
      <ErrorBox error={act.error} fields={["policy_set_id", "level", "target_id", "valid_from", "reason"]} />
    </Modal>
  );
}

function End({ a, onClose, onDone }: { a: Assignment; onClose: () => void; onDone: () => void }) {
  const [f, setF] = useState({ valid_to: today(), reason: "" });
  const act = useAction();
  const save = () =>
    act.run(() => api(`/attendance/policy-assignments/${a.id}/end`, { body: f })).then((r) => {
      if (r) {
        onDone();
        onClose();
      }
    });
  return (
    <Modal open title="End assignment" onClose={onClose} footer={<><Button onClick={onClose}>Cancel</Button><Button kind="danger" disabled={act.busy} onClick={save}>End</Button></>}>
      <div className="grid2">
        <Field label="Last day" name="valid_to" error={act.error}><input type="date" value={f.valid_to} onChange={(e) => setF({ ...f, valid_to: e.target.value })} /></Field>
        <Field label="Reason" name="reason" error={act.error} wide><textarea rows={2} value={f.reason} onChange={(e) => setF({ ...f, reason: e.target.value })} /></Field>
      </div>
      <ErrorBox error={act.error} fields={["valid_to", "reason"]} />
    </Modal>
  );
}
