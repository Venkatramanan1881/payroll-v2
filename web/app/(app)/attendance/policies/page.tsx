"use client";
import { useRouter } from "next/navigation";
import { useState } from "react";
import { Button, Card, ErrorBox, Field, fmtDate, Modal, PageHead, Pill, Select, Table } from "@/components/ui";
import { api } from "@/lib/api";
import { today, useAction, useApi, useCan } from "@/lib/hooks";

type PolicySet = { id: string; name: string; is_default: boolean; state: string; version: number; active_version: { id: string; valid_from: string } | null; assignments_count: number };

export default function PolicySets() {
  const router = useRouter();
  const manage = useCan()("attendance.policy.manage");
  const list = useApi<{ items: PolicySet[] }>("/attendance/policy-sets");
  const [creating, setCreating] = useState(false);
  return (
    <>
      <PageHead title="Policy sets">
        {manage && <Button kind="main" icon="add" onClick={() => setCreating(true)}>New policy set</Button>}
      </PageHead>
      <ErrorBox error={list.error} />
      <Card pad={false}>
        <Table
          rows={list.data?.items}
          onRow={(s) => router.push(`/attendance/policies/${s.id}`)}
          empty="No policy sets."
          cols={[
            { h: "Name", c: (s) => <>{s.name} {s.is_default && <Pill value="default" />}</> },
            { h: "In force from", c: (s) => (s.active_version ? fmtDate(s.active_version.valid_from) : "—") },
            { h: "Assignments", c: (s) => s.assignments_count, right: true },
            { h: "State", c: (s) => <Pill value={s.state} /> },
          ]}
        />
      </Card>
      {creating && (
        <CreateSet
          sets={list.data?.items ?? []}
          onClose={() => setCreating(false)}
          onDone={(id) => router.push(`/attendance/policies/${id}`)}
        />
      )}
    </>
  );
}

function CreateSet({ sets, onClose, onDone }: { sets: PolicySet[]; onClose: () => void; onDone: (id: string) => void }) {
  const [f, setF] = useState({ name: "", copy_from_policy_set_id: "", valid_from: today(), change_reason: "" });
  const act = useAction();
  const save = () =>
    act
      .run(() => api<PolicySet>("/attendance/policy-sets", { body: { ...f, copy_from_policy_set_id: f.copy_from_policy_set_id || null, change_reason: f.change_reason || null } }))
      .then((r) => r && onDone(r.id));
  return (
    <Modal open title="New policy set" onClose={onClose} footer={<><Button onClick={onClose}>Cancel</Button><Button kind="main" disabled={act.busy} onClick={save}>Create draft</Button></>}>
      <div className="grid2">
        <Field label="Name" name="name" error={act.error}><input value={f.name} onChange={(e) => setF({ ...f, name: e.target.value })} /></Field>
        <Field label="First version starts" name="valid_from" error={act.error}><input type="date" value={f.valid_from} onChange={(e) => setF({ ...f, valid_from: e.target.value })} /></Field>
        <Field label="Copy settings from" name="copy_from_policy_set_id" error={act.error} wide>
          <Select value={f.copy_from_policy_set_id} onChange={(v) => setF({ ...f, copy_from_policy_set_id: v })} options={sets.map((s) => ({ value: s.id, label: s.name }))} placeholder="Defaults" />
        </Field>
        <Field label="Change reason" name="change_reason" error={act.error} wide><input value={f.change_reason} onChange={(e) => setF({ ...f, change_reason: e.target.value })} /></Field>
      </div>
      <ErrorBox error={act.error} fields={["name", "valid_from", "copy_from_policy_set_id", "change_reason"]} />
    </Modal>
  );
}
