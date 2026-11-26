"use client";
import Link from "next/link";
import { useParams } from "next/navigation";
import { useEffect, useState } from "react";
import { SettingMeta, SettingsForm } from "@/components/SettingsForm";
import { Button, Card, ErrorBox, Field, fmtDate, fmtTime, Loading, Modal, PageHead, Pill, ReasonDialog, Table } from "@/components/ui";
import { api } from "@/lib/api";
import { today, useAction, useApi, useCan } from "@/lib/hooks";

type Set = { id: string; name: string; is_default: boolean; state: string; version: number };
type Version = {
  id: string; valid_from: string; state: string; values: Record<string, unknown>; change_reason: string | null; version: number;
  author: { id: string; name: string } | null; submitted_at: string | null; decided_at: string | null; decision_reason: string | null;
};
type Schema = { properties: Record<string, SettingMeta>; "x-removed"?: { code: string; reason: string }[] };

export default function PolicySetDetail() {
  const { id } = useParams<{ id: string }>();
  const can = useCan();
  const manage = can("attendance.policy.manage");
  const sets = useApi<{ items: Set[] }>("/attendance/policy-sets");
  const versions = useApi<{ items: Version[] }>(`/attendance/policy-sets/${id}/versions`);
  const schema = useApi<Schema>(manage ? "/attendance/policy-settings/schema" : null);
  const [sel, setSel] = useState<string | null>(null);
  const [ask, setAsk] = useState<null | "rename" | "deactivate" | "new">(null);
  const act = useAction();
  const set = sets.data?.items.find((s) => s.id === id);
  const items = versions.data?.items;
  useEffect(() => {
    if (!sel && items?.length) setSel((items.find((v) => v.state === "active") ?? items[0]).id);
  }, [items, sel]);
  const reload = () => {
    sets.reload();
    versions.reload();
  };
  const deactivate = (reason: string) =>
    act.run(() => api(`/attendance/policy-sets/${id}/deactivate`, { body: { reason: reason || null }, version: set!.version })).then((r) => {
      if (r) {
        setAsk(null);
        reload();
      }
    });

  if (!set) return sets.error ? <ErrorBox error={sets.error} /> : <Loading />;
  return (
    <>
      <PageHead title={set.name} sub={set.is_default ? "Organisation default policy set" : "Applied through assignments or per employee"}>
        <Link className="btn" href="/attendance/policies">All sets</Link>
        {manage && <Button onClick={() => setAsk("rename")}>Rename</Button>}
        {manage && !set.is_default && set.state === "active" && <Button kind="danger" onClick={() => setAsk("deactivate")}>Deactivate</Button>}
        {manage && set.state === "active" && <Button kind="main" icon="add" onClick={() => setAsk("new")}>New version</Button>}
      </PageHead>
      <ErrorBox error={versions.error ?? (ask ? null : act.error)} />
      <div className="stack">
        <Card title="Versions" pad={false}>
          <Table
            rows={items}
            onRow={(v) => setSel(v.id)}
            cols={[
              { h: "From", c: (v) => <span style={{ fontWeight: v.id === sel ? 600 : 400 }}>{fmtDate(v.valid_from)}</span> },
              { h: "State", c: (v) => <Pill value={v.state} /> },
              { h: "Reason", c: (v) => v.change_reason ?? "—" },
              { h: "Author", c: (v) => v.author?.name ?? "—" },
              { h: "Decided", c: (v) => (v.decided_at ? `${fmtTime(v.decided_at)}${v.decision_reason ? ` · ${v.decision_reason}` : ""}` : "—") },
            ]}
          />
        </Card>
        {sel && <VersionEditor key={sel} setId={id} vid={sel} meta={schema.data?.properties} manage={manage} approve={can("attendance.policy.approve")} onChanged={versions.reload} />}
      </div>
      {ask === "rename" && <Rename set={set} onClose={() => setAsk(null)} onDone={reload} />}
      {ask === "new" && (
        <NewVersion
          setId={id}
          values={items?.find((v) => v.id === sel)?.values ?? {}}
          onClose={() => setAsk(null)}
          onDone={(v) => {
            versions.reload();
            setSel(v);
          }}
        />
      )}
      <ReasonDialog open={ask === "deactivate"} title="Deactivate policy set" required={false} busy={act.busy} error={act.error} onClose={() => setAsk(null)} onConfirm={deactivate} />
    </>
  );
}

function Rename({ set, onClose, onDone }: { set: Set; onClose: () => void; onDone: () => void }) {
  const [name, setName] = useState(set.name);
  const act = useAction();
  const save = () =>
    act.run(() => api(`/attendance/policy-sets/${set.id}`, { method: "PATCH", body: { name }, version: set.version })).then((r) => {
      if (r) {
        onDone();
        onClose();
      }
    });
  return (
    <Modal open title="Rename policy set" onClose={onClose} footer={<><Button onClick={onClose}>Cancel</Button><Button kind="main" disabled={act.busy} onClick={save}>Save</Button></>}>
      <Field label="Name" name="name" error={act.error}><input value={name} onChange={(e) => setName(e.target.value)} /></Field>
      <ErrorBox error={act.error} fields={["name"]} />
    </Modal>
  );
}

function NewVersion({ setId, values, onClose, onDone }: { setId: string; values: Record<string, unknown>; onClose: () => void; onDone: (vid: string) => void }) {
  const [f, setF] = useState({ valid_from: today(), change_reason: "" });
  const act = useAction();
  const save = () =>
    act.run(() => api<Version>(`/attendance/policy-sets/${setId}/versions`, { body: { ...f, values, change_reason: f.change_reason || null } })).then((r) => {
      if (r) {
        onDone(r.id);
        onClose();
      }
    });
  return (
    <Modal open title="New draft version" onClose={onClose} footer={<><Button onClick={onClose}>Cancel</Button><Button kind="main" disabled={act.busy} onClick={save}>Create draft</Button></>}>
      <p className="muted">Starts as a copy of the selected version; edit the draft, then submit.</p>
      <div className="grid2">
        <Field label="Starts from" name="valid_from" error={act.error}><input type="date" value={f.valid_from} onChange={(e) => setF({ ...f, valid_from: e.target.value })} /></Field>
        <Field label="Change reason" name="change_reason" error={act.error}><input value={f.change_reason} onChange={(e) => setF({ ...f, change_reason: e.target.value })} /></Field>
      </div>
      <ErrorBox error={act.error} fields={["valid_from", "change_reason"]} />
    </Modal>
  );
}

function VersionEditor({ setId, vid, meta, manage, approve, onChanged }: { setId: string; vid: string; meta?: Record<string, SettingMeta>; manage: boolean; approve: boolean; onChanged: () => void }) {
  const base = `/attendance/policy-sets/${setId}/versions/${vid}`;
  const ver = useApi<Version>(base, { with_defaults: true });
  const act = useAction();
  const [values, setValues] = useState<Record<string, unknown> | null>(null);
  const [head, setHead] = useState({ valid_from: "", change_reason: "" });
  const [ask, setAsk] = useState<null | "reject" | "discard">(null);
  const v = ver.data;
  useEffect(() => {
    if (v) {
      setValues(v.values);
      setHead({ valid_from: v.valid_from, change_reason: v.change_reason ?? "" });
    }
  }, [v]);
  const done = (r: unknown) => {
    if (!r) return;
    setAsk(null);
    ver.reload();
    onChanged();
  };
  const save = () => act.run(() => api(base, { method: "PATCH", body: { ...head, change_reason: head.change_reason || null, values }, version: v!.version })).then(done);
  const step = (s: string, reason?: string) => act.run(() => api(`${base}/${s}`, { body: { reason: reason || null }, version: v!.version })).then(done);
  const draft = v?.state === "draft";
  return (
    <Card
      title={v ? `Version from ${fmtDate(v.valid_from)}` : "Version"}
      actions={
        v && (
          <>
            <Pill value={v.state} />
            {manage && draft && <Button onClick={save} disabled={act.busy}>Save draft</Button>}
            {manage && draft && <Button kind="main" onClick={() => step("submit")} disabled={act.busy}>Submit</Button>}
            {approve && v.state === "pending_approval" && <Button kind="main" onClick={() => step("approve")} disabled={act.busy}>Approve</Button>}
            {approve && v.state === "pending_approval" && <Button onClick={() => setAsk("reject")}>Reject</Button>}
            {manage && ["draft", "pending_approval", "scheduled"].includes(v.state) && <Button kind="danger" onClick={() => setAsk("discard")}>Discard</Button>}
          </>
        )
      }
    >
      <ErrorBox error={ver.error ?? (ask ? null : act.error)} fields={Object.keys(values ?? {}).map((k) => `values.${k}`)} />
      {!v || !values ? (
        <Loading />
      ) : (
        <div className="stack">
          {draft && manage && (
            <div className="grid2">
              <Field label="Starts from" name="valid_from" error={act.error}><input type="date" value={head.valid_from} onChange={(e) => setHead({ ...head, valid_from: e.target.value })} /></Field>
              <Field label="Change reason" name="change_reason" error={act.error}><input value={head.change_reason} onChange={(e) => setHead({ ...head, change_reason: e.target.value })} /></Field>
            </div>
          )}
          <SettingsForm values={values} onChange={setValues} meta={meta} error={act.error} readOnly={!(draft && manage)} />
        </div>
      )}
      <ReasonDialog
        open={!!ask}
        title={ask === "reject" ? "Reject version" : "Discard version"}
        required={ask === "reject"}
        busy={act.busy}
        error={act.error}
        onClose={() => setAsk(null)}
        onConfirm={(r) => step(ask!, r)}
      />
    </Card>
  );
}
