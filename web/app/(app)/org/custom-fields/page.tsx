"use client";
import { useState } from "react";
import { Button, Card, ErrorBox, Field, Modal, PageHead, Pill, ReasonDialog, Select, Table } from "@/components/ui";
import { api } from "@/lib/api";
import { useAction, useApi, useCan } from "@/lib/hooks";

type Def = { id: string; label: string; field_type: string; options: string[] | null; inactive_options: string[] | null; is_mandatory: boolean; state: string; version: number };
const TYPES = ["text", "number", "date", "yes_no", "dropdown", "multiple_choice"].map((v) => ({ value: v, label: v.replace(/_/g, " ") }));
const choice = (t: string) => t === "dropdown" || t === "multiple_choice";
const lines = (s: string) => s.split("\n").map((x) => x.trim()).filter(Boolean);

export default function CustomFields() {
  const can = useCan();
  const manage = can("employee.custom_fields.manage");
  const [state, setState] = useState("active");
  const list = useApi<{ items: Def[] }>("/org/custom-fields", { state });
  const [edit, setEdit] = useState<Def | "new" | null>(null);
  const [f, setF] = useState({ label: "", field_type: "text", options: "", is_mandatory: false, inactive: [] as string[] });
  const [off, setOff] = useState<Def | null>(null);
  const act = useAction();

  const open = (d: Def | "new") => {
    act.setError(null);
    setF(d === "new" ? { label: "", field_type: "text", options: "", is_mandatory: false, inactive: [] } : { label: d.label, field_type: d.field_type, options: (d.options ?? []).join("\n"), is_mandatory: d.is_mandatory, inactive: d.inactive_options ?? [] });
    setEdit(d);
  };
  const save = () =>
    act
      .run(() => {
        if (edit === "new") return api("/org/custom-fields", { body: { label: f.label, field_type: f.field_type, is_mandatory: f.is_mandatory, ...(choice(f.field_type) ? { options: lines(f.options) } : {}) } });
        const d = edit as Def;
        const b: Record<string, unknown> = {};
        if (f.label !== d.label) b.label = f.label;
        if (f.field_type !== d.field_type) b.field_type = f.field_type;
        if (f.is_mandatory !== d.is_mandatory) b.is_mandatory = f.is_mandatory;
        if (choice(f.field_type) && lines(f.options).join("\n") !== (d.options ?? []).join("\n")) b.options = lines(f.options);
        if (choice(f.field_type) && f.inactive.join() !== (d.inactive_options ?? []).join()) b.inactive_options = f.inactive;
        return api(`/org/custom-fields/${d.id}`, { method: "PATCH", body: b, version: d.version });
      })
      .then((r) => {
        if (r) {
          setEdit(null);
          list.reload();
        }
      });
  const deactivate = (reason: string) =>
    act.run(() => api(`/org/custom-fields/${off!.id}/deactivate`, { body: { reason }, version: off!.version })).then((r) => {
      if (r) {
        setOff(null);
        list.reload();
      }
    });

  return (
    <>
      <PageHead title="Custom fields" sub="Extra fields on every employee record.">
        {manage && <Button kind="main" icon="add" onClick={() => open("new")}>New field</Button>}
      </PageHead>
      <div className="filters">
        <Select value={state} onChange={setState} options={[{ value: "active", label: "Active" }, { value: "inactive", label: "Inactive" }]} />
      </div>
      <ErrorBox error={list.error} />
      <Card pad={false}>
        <Table
          rows={list.data?.items}
          empty="No custom fields."
          cols={[
            { h: "Label", c: (d) => d.label },
            { h: "Type", c: (d) => d.field_type.replace(/_/g, " ") },
            { h: "Options", c: (d) => (d.options ?? []).map((o) => ((d.inactive_options ?? []).includes(o) ? `${o} (off)` : o)).join(", ") || <span className="muted">—</span> },
            { h: "Mandatory", c: (d) => (d.is_mandatory ? "Yes" : "No") },
            { h: "State", c: (d) => <Pill value={d.state} /> },
            {
              h: "",
              right: true,
              c: (d) =>
                manage &&
                d.state === "active" && (
                  <span className="row" style={{ justifyContent: "flex-end" }}>
                    <Button kind="link" onClick={() => open(d)}>Edit</Button>
                    <Button kind="link" onClick={() => { act.setError(null); setOff(d); }}>Deactivate</Button>
                  </span>
                ),
            },
          ]}
        />
      </Card>
      <Modal
        open={!!edit}
        title={edit === "new" ? "New custom field" : "Edit custom field"}
        onClose={() => setEdit(null)}
        footer={
          <>
            <Button onClick={() => setEdit(null)}>Cancel</Button>
            <Button kind="main" disabled={act.busy} onClick={save}>Save</Button>
          </>
        }
      >
        <div className="grid2">
          <Field label="Label" name="label" error={act.error}>
            <input value={f.label} onChange={(e) => setF({ ...f, label: e.target.value })} />
          </Field>
          <Field label="Type" name="field_type" error={act.error}>
            <Select value={f.field_type} onChange={(v) => setF({ ...f, field_type: v })} options={TYPES} />
          </Field>
          <Field label="Mandatory">
            <span className="check">
              <input type="checkbox" checked={f.is_mandatory} onChange={(e) => setF({ ...f, is_mandatory: e.target.checked })} /> Required on every employee
            </span>
          </Field>
          {choice(f.field_type) && (
            <Field label="Options" name="options" error={act.error} hint="One per line. Options in use can't be removed; switch them off below." wide>
              <textarea rows={5} value={f.options} onChange={(e) => setF({ ...f, options: e.target.value })} />
            </Field>
          )}
          {choice(f.field_type) && edit !== "new" && lines(f.options).length > 0 && (
            <Field label="Inactive options" name="inactive_options" error={act.error} wide>
              <span className="row">
                {lines(f.options).map((o) => (
                  <label key={o} className="check">
                    <input type="checkbox" checked={f.inactive.includes(o)} onChange={(e) => setF({ ...f, inactive: e.target.checked ? [...f.inactive, o] : f.inactive.filter((x) => x !== o) })} /> {o}
                  </label>
                ))}
              </span>
            </Field>
          )}
        </div>
        <ErrorBox error={act.error} fields={["label", "field_type", "options", "inactive_options"]} />
      </Modal>
      <ReasonDialog open={!!off} title={`Deactivate ${off?.label}`} confirm="Deactivate" onClose={() => setOff(null)} onConfirm={deactivate} error={act.error} busy={act.busy}>
        <p className="muted">The field is hidden from records. Values already saved are kept.</p>
      </ReasonDialog>
    </>
  );
}
