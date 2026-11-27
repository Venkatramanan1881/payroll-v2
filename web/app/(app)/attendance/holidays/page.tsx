"use client";
import { useState } from "react";
import { Button, Card, ErrorBox, Field, fmtDate, Modal, PageHead, Pill, ReasonDialog, Select, Table } from "@/components/ui";
import { api } from "@/lib/api";
import { useAction, useApi, useCan } from "@/lib/hooks";
import { useBranches } from "@/lib/lookups";

type Cal = { id: string; name: string; year: number; state: string; version: number; holidays_count: number; branches: { id: string; short_code: string }[] };
type Holiday = { id: string; date: string; name: string; kind: string; removed_at: string | null };
const KINDS = ["national", "festival", "optional"].map((k) => ({ value: k, label: k }));

export default function Holidays() {
  const manage = useCan()("attendance.calendar.manage");
  const [year, setYear] = useState(String(new Date().getFullYear()));
  const list = useApi<{ items: Cal[] }>("/attendance/holiday-calendars", { year });
  const [creating, setCreating] = useState(false);
  const [open, setOpen] = useState<string | null>(null);
  return (
    <>
      <PageHead title="Holiday calendars">
        {manage && <Button kind="main" icon="add" onClick={() => setCreating(true)}>New calendar</Button>}
      </PageHead>
      <div className="filters">
        <Field label="Year"><input type="number" value={year} onChange={(e) => setYear(e.target.value)} /></Field>
      </div>
      <ErrorBox error={list.error} />
      <Card pad={false}>
        <Table
          rows={list.data?.items}
          onRow={(c) => setOpen(c.id)}
          empty="No calendars for this year."
          cols={[
            { h: "Name", c: (c) => c.name },
            { h: "Year", c: (c) => c.year },
            { h: "Holidays", c: (c) => c.holidays_count, right: true },
            { h: "Branches", c: (c) => c.branches.map((b) => b.short_code).join(", ") || "—" },
            { h: "State", c: (c) => <Pill value={c.state} /> },
          ]}
        />
      </Card>
      {creating && <CreateCal year={Number(year)} calendars={list.data?.items ?? []} onClose={() => setCreating(false)} onDone={list.reload} />}
      {open && <CalDetail id={open} manage={manage} onClose={() => setOpen(null)} onChanged={list.reload} />}
    </>
  );
}

function CreateCal({ year, calendars, onClose, onDone }: { year: number; calendars: Cal[]; onClose: () => void; onDone: () => void }) {
  const [f, setF] = useState({ name: "", year, copy_from_calendar_id: "" });
  const prev = useApi<{ items: Cal[] }>("/attendance/holiday-calendars", { year: f.year - 1 });
  const act = useAction();
  const save = () =>
    act.run(() => api("/attendance/holiday-calendars", { body: { ...f, copy_from_calendar_id: f.copy_from_calendar_id || null } })).then((r) => {
      if (r) {
        onDone();
        onClose();
      }
    });
  const sources = [...calendars, ...(prev.data?.items ?? [])].map((c) => ({ value: c.id, label: `${c.name} (${c.year})` }));
  return (
    <Modal open title="New holiday calendar" onClose={onClose} footer={<><Button onClick={onClose}>Cancel</Button><Button kind="main" disabled={act.busy} onClick={save}>Create</Button></>}>
      <div className="grid2">
        <Field label="Name" name="name" error={act.error}><input value={f.name} onChange={(e) => setF({ ...f, name: e.target.value })} /></Field>
        <Field label="Year" name="year" error={act.error}><input type="number" value={f.year} onChange={(e) => setF({ ...f, year: Number(e.target.value) })} /></Field>
        <Field label="Copy from" name="copy_from_calendar_id" error={act.error} hint="National dates kept; festival and optional copied to adjust" wide>
          <Select value={f.copy_from_calendar_id} onChange={(v) => setF({ ...f, copy_from_calendar_id: v })} options={sources} placeholder="Start empty" />
        </Field>
      </div>
      <ErrorBox error={act.error} fields={["name", "year", "copy_from_calendar_id"]} />
    </Modal>
  );
}

function CalDetail({ id, manage, onClose, onChanged }: { id: string; manage: boolean; onClose: () => void; onChanged: () => void }) {
  const cal = useApi<Cal & { holidays: Holiday[] }>(`/attendance/holiday-calendars/${id}`);
  const branches = useBranches();
  const act = useAction();
  const [edit, setEdit] = useState<Partial<Holiday> | null>(null);
  const [removing, setRemoving] = useState<Holiday | null>(null);
  const [branch, setBranch] = useState("");
  const c = cal.data;
  const reload = () => {
    cal.reload();
    onChanged();
  };
  const save = () => {
    if (!c || !edit) return;
    const body = { date: edit.date, name: edit.name, kind: edit.kind };
    act
      .run(() => (edit.id ? api(`/attendance/holiday-calendars/${id}/holidays/${edit.id}`, { method: "PATCH", body, version: c.version }) : api(`/attendance/holiday-calendars/${id}/holidays`, { body })))
      .then((r) => {
        if (r) {
          setEdit(null);
          reload();
        }
      });
  };
  const remove = (reason: string) =>
    act.run(() => api(`/attendance/holiday-calendars/${id}/holidays/${removing!.id}/remove`, { body: { reason }, version: c!.version })).then((r) => {
      if (r) {
        setRemoving(null);
        reload();
      }
    });
  const link = () => act.run(() => api(`/attendance/branches/${branch}/holiday-calendars/${c!.year}`, { method: "PUT", body: { calendar_id: id } })).then((r) => r && reload());

  return (
    <Modal open wide title={c ? `${c.name} · ${c.year}` : "Calendar"} onClose={onClose}>
      <ErrorBox error={cal.error} />
      {c && (
        <div className="stack">
          {manage && (
            <div className="filters">
              <Field label={`Use for a branch in ${c.year}`}><Select value={branch} onChange={setBranch} options={branches.options} placeholder="Choose branch…" /></Field>
              <Button disabled={!branch || act.busy} onClick={link}>Link branch</Button>
              <Button kind="main" icon="add" onClick={() => setEdit({ date: `${c.year}-01-01`, name: "", kind: "national" })}>Add holiday</Button>
            </div>
          )}
          <p className="muted small" style={{ margin: 0 }}>Linked: {c.branches.map((b) => b.short_code).join(", ") || "no branches"}</p>
          {edit && (
            <Card title={edit.id ? "Change holiday" : "New holiday"} actions={<><Button onClick={() => setEdit(null)}>Cancel</Button><Button kind="main" disabled={act.busy} onClick={save}>Save</Button></>}>
              <div className="grid2">
                <Field label="Date" name="date" error={act.error}><input type="date" value={edit.date} onChange={(e) => setEdit({ ...edit, date: e.target.value })} /></Field>
                <Field label="Name" name="name" error={act.error}><input value={edit.name} onChange={(e) => setEdit({ ...edit, name: e.target.value })} /></Field>
                <Field label="Kind" name="kind" error={act.error}><Select value={edit.kind} onChange={(v) => setEdit({ ...edit, kind: v })} options={KINDS} /></Field>
              </div>
            </Card>
          )}
          <ErrorBox error={removing ? null : act.error} fields={["date", "name", "kind"]} />
          <Table
            rows={c.holidays}
            empty="No holidays yet."
            cols={[
              { h: "Date", c: (h) => fmtDate(h.date) },
              { h: "Name", c: (h) => h.name },
              { h: "Kind", c: (h) => <Pill value={h.kind} /> },
              {
                h: "",
                right: true,
                c: (h) =>
                  manage && (
                    <span className="row" style={{ justifyContent: "flex-end" }}>
                      <Button kind="link" onClick={() => setEdit(h)}>Edit</Button>
                      <Button kind="link" onClick={() => setRemoving(h)}>Remove</Button>
                    </span>
                  ),
              },
            ]}
          />
        </div>
      )}
      <ReasonDialog open={!!removing} title={`Remove ${removing?.name ?? ""}`} busy={act.busy} error={act.error} onClose={() => setRemoving(null)} onConfirm={remove} />
    </Modal>
  );
}
