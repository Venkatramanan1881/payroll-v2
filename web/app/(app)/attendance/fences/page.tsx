"use client";
import { useState } from "react";
import { Button, Card, ErrorBox, Field, fmtDate, Modal, PageHead, Pill, Select, Table } from "@/components/ui";
import { api } from "@/lib/api";
import { useAction, useApi } from "@/lib/hooks";
import { useBranches } from "@/lib/lookups";

type Fence = { id: string; branch_id: string; name: string; state: string; version: number; latitude: number; longitude: number; radius_m: number; valid_from: string };

export default function Fences() {
  const branches = useBranches();
  const [branch, setBranch] = useState("");
  const list = useApi<{ items: Fence[] }>(branch ? `/attendance/branches/${branch}/fences` : null);
  const act = useAction();
  const [edit, setEdit] = useState<Partial<Fence> | null>(null);
  const [testing, setTesting] = useState<Fence | null>(null);
  const toggle = (f: Fence) => act.run(() => api(`/attendance/fences/${f.id}/${f.state === "active" ? "deactivate" : "activate"}`, { method: "POST", version: f.version })).then((r) => r && list.reload());
  return (
    <>
      <PageHead title="Geo-fences">
        {branch && <Button kind="main" icon="add" onClick={() => setEdit({ name: "", radius_m: 200 })}>New fence</Button>}
      </PageHead>
      <div className="filters">
        <Field label="Branch"><Select value={branch} onChange={setBranch} options={branches.options} placeholder="Choose…" /></Field>
      </div>
      <ErrorBox error={list.error ?? act.error} />
      {branch ? (
        <Card pad={false}>
          <Table
            rows={list.data?.items}
            empty="No fences for this branch."
            cols={[
              { h: "Name", c: (f) => f.name },
              { h: "Centre", c: (f) => <a href={`https://www.openstreetmap.org/?mlat=${f.latitude}&mlon=${f.longitude}#map=17/${f.latitude}/${f.longitude}`} target="_blank" rel="noreferrer">{f.latitude.toFixed(5)}, {f.longitude.toFixed(5)}</a> },
              { h: "Radius", c: (f) => `${f.radius_m} m`, right: true },
              { h: "Since", c: (f) => fmtDate(f.valid_from) },
              { h: "State", c: (f) => <Pill value={f.state} /> },
              {
                h: "",
                right: true,
                c: (f) => (
                  <span className="row" style={{ justifyContent: "flex-end" }}>
                    <Button kind="link" onClick={() => setEdit(f)}>Edit</Button>
                    <Button kind="link" onClick={() => setTesting(f)}>Test</Button>
                    <Button kind="link" disabled={act.busy} onClick={() => toggle(f)}>{f.state === "active" ? "Deactivate" : "Activate"}</Button>
                  </span>
                ),
              },
            ]}
          />
        </Card>
      ) : (
        <Card><p className="muted">Choose a branch.</p></Card>
      )}
      {edit && <EditFence branch={branch} fence={edit} onClose={() => setEdit(null)} onDone={list.reload} />}
      {testing && <TestFence fence={testing} onClose={() => setTesting(null)} />}
    </>
  );
}

function EditFence({ branch, fence, onClose, onDone }: { branch: string; fence: Partial<Fence>; onClose: () => void; onDone: () => void }) {
  const [f, setF] = useState({ name: fence.name ?? "", latitude: fence.latitude?.toString() ?? "", longitude: fence.longitude?.toString() ?? "", radius_m: fence.radius_m?.toString() ?? "200" });
  const act = useAction();
  const body = { name: f.name, latitude: Number(f.latitude), longitude: Number(f.longitude), radius_m: Number(f.radius_m) };
  const save = () =>
    act
      .run(() => (fence.id ? api(`/attendance/fences/${fence.id}`, { method: "PATCH", body, version: fence.version }) : api(`/attendance/branches/${branch}/fences`, { body })))
      .then((r) => {
        if (r) {
          onDone();
          onClose();
        }
      });
  const here = () => navigator.geolocation?.getCurrentPosition((p) => setF({ ...f, latitude: p.coords.latitude.toFixed(6), longitude: p.coords.longitude.toFixed(6) }));
  return (
    <Modal open title={fence.id ? "Edit fence" : "New fence"} onClose={onClose} footer={<><Button onClick={onClose}>Cancel</Button><Button kind="main" disabled={act.busy} onClick={save}>Save</Button></>}>
      {fence.id && fence.state === "active" && <div className="note">The fence is active: a change starts a new version from now.</div>}
      <div className="grid2">
        <Field label="Name" name="name" error={act.error}><input value={f.name} onChange={(e) => setF({ ...f, name: e.target.value })} /></Field>
        <Field label="Radius (20–5,000 m)" name="radius_m" error={act.error}><input type="number" value={f.radius_m} onChange={(e) => setF({ ...f, radius_m: e.target.value })} /></Field>
        <Field label="Latitude" name="latitude" error={act.error}><input type="number" step="any" value={f.latitude} onChange={(e) => setF({ ...f, latitude: e.target.value })} /></Field>
        <Field label="Longitude" name="longitude" error={act.error}><input type="number" step="any" value={f.longitude} onChange={(e) => setF({ ...f, longitude: e.target.value })} /></Field>
      </div>
      <Button kind="link" icon="my_location" onClick={here}>Use my location</Button>
      <ErrorBox error={act.error} fields={["name", "radius_m", "latitude", "longitude"]} />
    </Modal>
  );
}

function TestFence({ fence, onClose }: { fence: Fence; onClose: () => void }) {
  const [f, setF] = useState({ latitude: "", longitude: "", accuracy_m: "20" });
  const act = useAction();
  const [res, setRes] = useState<{ distance_m: number; inside: boolean } | null>(null);
  const test = () => act.run(() => api(`/attendance/fences/${fence.id}/test`, { body: { latitude: Number(f.latitude), longitude: Number(f.longitude), accuracy_m: Number(f.accuracy_m) } })).then((r) => r && setRes(r));
  return (
    <Modal open title={`Test ${fence.name}`} onClose={onClose} footer={<><Button onClick={onClose}>Close</Button><Button kind="main" disabled={act.busy} onClick={test}>Test point</Button></>}>
      <div className="grid2">
        <Field label="Latitude" name="latitude" error={act.error}><input type="number" step="any" value={f.latitude} onChange={(e) => setF({ ...f, latitude: e.target.value })} /></Field>
        <Field label="Longitude" name="longitude" error={act.error}><input type="number" step="any" value={f.longitude} onChange={(e) => setF({ ...f, longitude: e.target.value })} /></Field>
        <Field label="Accuracy (m)" name="accuracy_m" error={act.error}><input type="number" value={f.accuracy_m} onChange={(e) => setF({ ...f, accuracy_m: e.target.value })} /></Field>
      </div>
      {res && <div className="note">{res.distance_m} m from the centre — <strong>{res.inside ? "inside" : "outside"}</strong> the fence.</div>}
      <ErrorBox error={act.error} fields={["latitude", "longitude", "accuracy_m"]} />
    </Modal>
  );
}
