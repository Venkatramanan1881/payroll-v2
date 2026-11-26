"use client";
import { useState } from "react";
import { Button, Card, Code, ErrorBox, Field, fmtDate, Modal, PageHead, Pill, ReasonDialog, Select, Table } from "@/components/ui";
import { api } from "@/lib/api";
import { today, useAction, useApi, useCan } from "@/lib/hooks";
import { useBranches } from "@/lib/lookups";
import { hhmm, toTime } from "../_shared";

type Times = Record<string, any>;
type Shift = { id: string; code: string; name: string; branch_id: string | null; state: string; version: number; current: (Times & { valid_from: string }) | null };

// ShiftTimes defaults (core/src/shifts.rs).
const DEFAULT_TIMES: Times = {
  kind: "fixed", start_time: "09:00", end_time: "18:00", is_night: false, night_date_rule: "start_date", required_minutes: 480,
  window_before_min: 180, window_after_min: 360, flex_earliest: "", flex_latest: "", core_start: "", core_end: "",
  break_kind: "", break_start: "", break_end: "", half_day_point: "middle", half_day_time: "",
};
const TIME_KEYS = ["start_time", "end_time", "flex_earliest", "flex_latest", "core_start", "core_end", "break_start", "break_end", "half_day_time"];
const opts = (xs: string[]) => xs.map((x) => ({ value: x, label: x.replace(/_/g, " ") }));

/** Form values → API body: times get seconds, blanks become null. */
const timesBody = (t: Times) =>
  Object.fromEntries(Object.keys(DEFAULT_TIMES).map((k) => [k, TIME_KEYS.includes(k) ? toTime(t[k]) : k === "break_kind" ? t[k] || null : t[k]]));
const fromVersion = (v: Times | null) => (v ? Object.fromEntries(Object.keys(DEFAULT_TIMES).map((k) => [k, TIME_KEYS.includes(k) ? hhmm(v[k]) : v[k] ?? ""])) : { ...DEFAULT_TIMES });

function TimesForm({ t, set, error }: { t: Times; set: (t: Times) => void; error: any }) {
  const f = (k: string) => (v: any) => set({ ...t, [k]: v });
  const time = (k: string, label: string) => (
    <Field label={label} name={k} error={error}>
      <input type="time" value={t[k] ?? ""} onChange={(e) => f(k)(e.target.value)} />
    </Field>
  );
  const num = (k: string, label: string) => (
    <Field label={label} name={k} error={error}>
      <input type="number" value={t[k]} onChange={(e) => f(k)(Number(e.target.value))} />
    </Field>
  );
  return (
    <div className="grid2">
      <Field label="Kind" name="kind" error={error}>
        <Select value={t.kind} onChange={f("kind")} options={opts(["fixed", "flexible", "flexible_core", "split"])} />
      </Field>
      {time("start_time", "Start")}
      {time("end_time", "End")}
      {num("required_minutes", "Required minutes")}
      {num("window_before_min", "Window before start (min)")}
      {num("window_after_min", "Window after end (min)")}
      <Field label="Night shift" name="is_night" error={error}>
        <span className="check"><input type="checkbox" checked={!!t.is_night} onChange={(e) => f("is_night")(e.target.checked)} /> Crosses midnight</span>
      </Field>
      <Field label="Night date rule" name="night_date_rule" error={error}>
        <Select value={t.night_date_rule} onChange={f("night_date_rule")} options={opts(["start_date", "end_date"])} />
      </Field>
      {(t.kind === "flexible" || t.kind === "flexible_core") && (
        <>
          {time("flex_earliest", "Flex earliest")}
          {time("flex_latest", "Flex latest")}
        </>
      )}
      {t.kind === "flexible_core" && (
        <>
          {time("core_start", "Core start")}
          {time("core_end", "Core end")}
        </>
      )}
      <Field label="Break" name="break_kind" error={error}>
        <Select value={t.break_kind} onChange={f("break_kind")} options={opts(["fixed", "window"])} placeholder="No break" />
      </Field>
      {t.break_kind && (
        <>
          {time("break_start", "Break start")}
          {time("break_end", "Break end")}
        </>
      )}
      <Field label="Half-day point" name="half_day_point" error={error}>
        <Select value={t.half_day_point} onChange={f("half_day_point")} options={opts(["middle", "fixed_time", "break_start"])} />
      </Field>
      {t.half_day_point === "fixed_time" && time("half_day_time", "Half-day time")}
    </div>
  );
}

export default function Shifts() {
  const can = useCan();
  const manage = can("attendance.shift.manage");
  const branches = useBranches();
  const [state, setState] = useState("active");
  const [branch, setBranch] = useState("");
  const list = useApi<{ items: Shift[] }>("/attendance/shifts", { state, branch_id: branch });
  const [creating, setCreating] = useState(false);
  const [sel, setSel] = useState<Shift | null>(null);
  const bname = (id: string | null) => (id ? branches.options.find((o) => o.value === id)?.label ?? "—" : "All branches");

  return (
    <>
      <PageHead title="Shifts" sub="Shift times are dated versions; a saved version is never edited.">
        {manage && <Button kind="main" icon="add" onClick={() => setCreating(true)}>New shift</Button>}
      </PageHead>
      <div className="filters">
        <Select value={state} onChange={setState} options={opts(["active", "inactive"])} placeholder="Any state" />
        <Select value={branch} onChange={setBranch} options={branches.options} placeholder="Any branch" />
      </div>
      <ErrorBox error={list.error} />
      <Card pad={false}>
        <Table
          rows={list.data?.items}
          onRow={setSel}
          empty="No shifts yet."
          cols={[
            { h: "Code", c: (s) => <Code>{s.code}</Code> },
            { h: "Name", c: (s) => s.name },
            { h: "Branch", c: (s) => bname(s.branch_id) },
            { h: "Kind", c: (s) => s.current?.kind?.replace(/_/g, " ") ?? "—" },
            { h: "Times", c: (s) => (s.current ? `${hhmm(s.current.start_time)}–${hhmm(s.current.end_time)}${s.current.is_night ? " (night)" : ""}` : "—") },
            { h: "State", c: (s) => <Pill value={s.state} /> },
          ]}
        />
      </Card>
      {creating && <CreateShift branches={branches.options} onClose={() => setCreating(false)} onDone={list.reload} />}
      {sel && <ShiftDetail shift={sel} manage={manage} branches={branches.options} onClose={() => setSel(null)} onChanged={list.reload} />}
    </>
  );
}

function CreateShift({ branches, onClose, onDone }: { branches: { value: string; label: string }[]; onClose: () => void; onDone: () => void }) {
  const act = useAction();
  const [head, setHead] = useState({ code: "", name: "", branch_id: "", valid_from: today() });
  const [t, setT] = useState<Times>({ ...DEFAULT_TIMES });
  const save = () =>
    act.run(() => api("/attendance/shifts", { body: { ...head, branch_id: head.branch_id || null, ...timesBody(t) } })).then((r) => {
      if (r) {
        onDone();
        onClose();
      }
    });
  return (
    <Modal open wide title="New shift" onClose={onClose} footer={<><Button onClick={onClose}>Cancel</Button><Button kind="main" disabled={act.busy} onClick={save}>Create</Button></>}>
      <div className="grid2 form-section">
        <Field label="Code" name="code" error={act.error}><input value={head.code} onChange={(e) => setHead({ ...head, code: e.target.value })} /></Field>
        <Field label="Name" name="name" error={act.error}><input value={head.name} onChange={(e) => setHead({ ...head, name: e.target.value })} /></Field>
        <Field label="Branch" name="branch_id" error={act.error} hint="Blank = usable at every branch">
          <Select value={head.branch_id} onChange={(v) => setHead({ ...head, branch_id: v })} options={branches} placeholder="All branches" />
        </Field>
        <Field label="Times start from" name="valid_from" error={act.error}><input type="date" value={head.valid_from} onChange={(e) => setHead({ ...head, valid_from: e.target.value })} /></Field>
      </div>
      <TimesForm t={t} set={setT} error={act.error} />
      <ErrorBox error={act.error} fields={["code", "name", "branch_id", "valid_from", ...Object.keys(DEFAULT_TIMES)]} />
    </Modal>
  );
}

function ShiftDetail({ shift, manage, branches, onClose, onChanged }: { shift: Shift; manage: boolean; branches: { value: string; label: string }[]; onClose: () => void; onChanged: () => void }) {
  const versions = useApi<{ items: (Times & { id: string; valid_from: string; state: string })[] }>(`/attendance/shifts/${shift.id}/versions`);
  const act = useAction();
  const [doc, setDoc] = useState(shift);
  const [name, setName] = useState(shift.name);
  const [branch, setBranch] = useState(shift.branch_id ?? "");
  const [adding, setAdding] = useState(false);
  const [from, setFrom] = useState(today());
  const [t, setT] = useState<Times>(fromVersion(shift.current));
  const [ask, setAsk] = useState<null | "deactivate" | { discard: string }>(null);
  const refresh = (d?: Shift) => {
    if (d) setDoc(d);
    onChanged();
  };
  const rename = () => act.run(() => api<Shift>(`/attendance/shifts/${doc.id}`, { method: "PATCH", body: { name, branch_id: branch || null }, version: doc.version })).then((d) => d && refresh(d));
  const addVersion = () =>
    act.run(() => api(`/attendance/shifts/${doc.id}/versions`, { body: { valid_from: from, ...timesBody(t) } })).then((r) => {
      if (r) {
        setAdding(false);
        versions.reload();
        refresh();
      }
    });
  const setState = (to: "activate" | "deactivate", reason = "") =>
    act.run(() => api<Shift>(`/attendance/shifts/${doc.id}/${to}`, { body: { reason: reason || null }, version: doc.version })).then((d) => {
      if (d) {
        setAsk(null);
        refresh(d);
      }
    });
  const discard = (vid: string, reason: string) =>
    act.run(() => api(`/attendance/shifts/${doc.id}/versions/${vid}/discard`, { body: { reason: reason || null } })).then((r) => {
      if (r) {
        setAsk(null);
        versions.reload();
      }
    });

  return (
    <Modal open wide title={`${doc.code} · ${doc.name}`} onClose={onClose}>
      <div className="stack">
        {manage && (
          <div className="filters">
            <Field label="Name" name="name" error={act.error}><input value={name} onChange={(e) => setName(e.target.value)} /></Field>
            <Field label="Branch" name="branch_id" error={act.error}><Select value={branch} onChange={setBranch} options={branches} placeholder="All branches" /></Field>
            <Button onClick={rename} disabled={act.busy}>Save</Button>
            {doc.state === "active" ? (
              <Button kind="danger" onClick={() => setAsk("deactivate")}>Deactivate</Button>
            ) : (
              <Button onClick={() => setState("activate")}>Activate</Button>
            )}
          </div>
        )}
        <ErrorBox error={ask ? null : act.error} fields={["name", "branch_id", "valid_from", ...Object.keys(DEFAULT_TIMES)]} />
        <div className="spread">
          <h3>Versions</h3>
          {manage && !adding && <Button icon="add" onClick={() => setAdding(true)}>New version</Button>}
        </div>
        {adding && (
          <Card title="New version" actions={<><Button onClick={() => setAdding(false)}>Cancel</Button><Button kind="main" disabled={act.busy} onClick={addVersion}>Save version</Button></>}>
            <div className="form-section">
              <Field label="Starts from" name="valid_from" error={act.error}><input type="date" value={from} onChange={(e) => setFrom(e.target.value)} /></Field>
            </div>
            <TimesForm t={t} set={setT} error={act.error} />
          </Card>
        )}
        <Table
          rows={versions.data?.items}
          cols={[
            { h: "From", c: (v) => fmtDate(v.valid_from) },
            { h: "Kind", c: (v) => v.kind.replace(/_/g, " ") },
            { h: "Times", c: (v) => `${hhmm(v.start_time)}–${hhmm(v.end_time)}` },
            { h: "Required", c: (v) => `${v.required_minutes} min` },
            { h: "Break", c: (v) => (v.break_kind ? `${v.break_kind} ${hhmm(v.break_start)}–${hhmm(v.break_end)}` : "—") },
            { h: "State", c: (v) => <Pill value={v.state} /> },
            { h: "", c: (v) => manage && v.state === "scheduled" && <Button kind="link" onClick={() => setAsk({ discard: v.id })}>Discard</Button>, right: true },
          ]}
        />
      </div>
      <ReasonDialog
        open={!!ask}
        title={ask === "deactivate" ? "Deactivate shift" : "Discard scheduled version"}
        required={ask === "deactivate"}
        busy={act.busy}
        error={act.error}
        onClose={() => setAsk(null)}
        onConfirm={(r) => (ask === "deactivate" ? setState("deactivate", r) : ask && discard(ask.discard, r))}
      />
    </Modal>
  );
}
