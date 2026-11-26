"use client";
import { useState } from "react";
import { Button, Card, ErrorBox, Field, Modal, Opt, PageHead, Pill, Select, Table, Tabs } from "@/components/ui";
import { api } from "@/lib/api";
import { today, useAction, useApi, useCan } from "@/lib/hooks";
import { useBranches, useEmployeeOptions, useList } from "@/lib/lookups";

type Group = {
  id: string;
  name: string;
  pay_frequency: string;
  month_start_day: number | null;
  week_start_day: number | null;
  cycle_anchor: string | null;
  is_default: boolean;
  state: string;
  member_count_today: number;
  version: number;
};
const FREQ = ["monthly", "weekly", "bi_weekly", "daily"].map((v) => ({ value: v, label: v.replace("_", "-") }));
const DAYS = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"].map((d, i) => ({ value: String(i + 1), label: d }));

const cycle = (g: Group) =>
  g.pay_frequency === "monthly"
    ? g.month_start_day ? `From day ${g.month_start_day}` : "Calendar month"
    : g.pay_frequency === "weekly"
      ? `From ${DAYS[(g.week_start_day ?? 1) - 1]?.label}`
      : g.pay_frequency === "bi_weekly"
        ? `Anchor ${g.cycle_anchor}`
        : "Daily";

/** Values from a <select multiple>. */
const picked = (e: React.ChangeEvent<HTMLSelectElement>) => Array.from(e.target.selectedOptions, (o) => o.value);

function Multi({ label, options, value, onChange }: { label: string; options: Opt[]; value: string[]; onChange: (v: string[]) => void }) {
  return (
    <Field label={label} hint="Ctrl/Cmd-click to pick several">
      <select multiple size={Math.min(6, Math.max(3, options.length))} value={value} onChange={(e) => onChange(picked(e))}>
        {options.map((o) => (
          <option key={o.value} value={o.value}>{o.label}</option>
        ))}
      </select>
    </Field>
  );
}

function Assign({ g, onClose }: { g: Group; onClose: () => void }) {
  const [mode, setMode] = useState<"employees" | "rule">("employees");
  const [q, setQ] = useState("");
  const emps = useEmployeeOptions(q);
  const branches = useBranches();
  const deps = useList("departments");
  const cats = useList("categories");
  const types = useList("employee-types");
  const [f, setF] = useState({ effective_from: today(), reason: "", employee_ids: [] as string[], branch_ids: [] as string[], department_ids: [] as string[], category_ids: [] as string[], employee_type_ids: [] as string[] });
  const [res, setRes] = useState<{ changed: number; skipped: { employee_id: string; code: string }[] } | null>(null);
  const act = useAction();
  const set = (k: keyof typeof f) => (v: any) => setF({ ...f, [k]: v });
  const go = () =>
    act
      .run(() =>
        api(`/org/payroll-groups/${g.id}/assign`, {
          body: {
            effective_from: f.effective_from,
            reason: f.reason,
            ...(mode === "employees"
              ? { employee_ids: f.employee_ids }
              : { rule: { branch_ids: f.branch_ids, department_ids: f.department_ids, category_ids: f.category_ids, employee_type_ids: f.employee_type_ids } }),
          },
        }),
      )
      .then((r) => r && setRes(r));
  return (
    <Modal
      open
      wide
      title={`Assign employees to ${g.name}`}
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Close</Button>
          {!res && <Button kind="main" disabled={act.busy} onClick={go}>Assign</Button>}
        </>
      }
    >
      {res ? (
        <div className="stack">
          <div className="note">{res.changed} employee(s) moved to this group.</div>
          {res.skipped.length > 0 && (
            <Table rows={res.skipped} cols={[{ h: "Skipped employee", c: (s) => <code>{s.employee_id}</code> }, { h: "Why", c: (s) => s.code.replace(/_/g, " ") }]} />
          )}
        </div>
      ) : (
        <div className="stack">
          <Tabs value={mode} onChange={setMode} tabs={[{ value: "employees", label: "Pick employees" }, { value: "rule", label: "By rule" }]} />
          <div className="grid2">
            <Field label="Effective from" name="effective_from" error={act.error}>
              <input type="date" value={f.effective_from} onChange={(e) => set("effective_from")(e.target.value)} />
            </Field>
            <Field label="Reason" name="reason" error={act.error}>
              <input value={f.reason} onChange={(e) => set("reason")(e.target.value)} />
            </Field>
            {mode === "employees" ? (
              <>
                <Field label="Search employees">
                  <input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Name, code or mobile" />
                </Field>
                <Multi label={`Employees (${f.employee_ids.length} picked)`} options={emps.options} value={f.employee_ids} onChange={set("employee_ids")} />
              </>
            ) : (
              <>
                <Multi label="Branches" options={branches.options} value={f.branch_ids} onChange={set("branch_ids")} />
                <Multi label="Departments" options={deps.options} value={f.department_ids} onChange={set("department_ids")} />
                <Multi label="Categories" options={cats.options} value={f.category_ids} onChange={set("category_ids")} />
                <Multi label="Employee types" options={types.options} value={f.employee_type_ids} onChange={set("employee_type_ids")} />
              </>
            )}
          </div>
          <ErrorBox error={act.error} fields={["effective_from", "reason"]} />
        </div>
      )}
    </Modal>
  );
}

export default function PayrollGroups() {
  const can = useCan();
  const manage = can("org.payroll_group.manage");
  const list = useApi<{ items: Group[] }>("/org/payroll-groups");
  const [edit, setEdit] = useState<Group | "new" | null>(null);
  const [f, setF] = useState<any>({});
  const [del, setDel] = useState<Group | null>(null);
  const [assign, setAssign] = useState<Group | null>(null);
  const act = useAction();

  const open = (g: Group | "new") => {
    act.setError(null);
    setF(g === "new" ? { name: "", pay_frequency: "monthly", month_start_day: "", week_start_day: "1", cycle_anchor: "" } : { ...g, month_start_day: g.month_start_day ?? "", week_start_day: g.week_start_day ?? "1", cycle_anchor: g.cycle_anchor ?? "" });
    setEdit(g);
  };
  // Each frequency takes only its own cycle field, so the others go as null.
  const body = () => ({
    name: f.name,
    pay_frequency: f.pay_frequency,
    month_start_day: f.pay_frequency === "monthly" && f.month_start_day !== "" ? Number(f.month_start_day) : null,
    week_start_day: f.pay_frequency === "weekly" ? Number(f.week_start_day) : null,
    cycle_anchor: f.pay_frequency === "bi_weekly" ? f.cycle_anchor || null : null,
  });
  const save = () =>
    act
      .run(() => (edit === "new" ? api("/org/payroll-groups", { body: body() }) : api(`/org/payroll-groups/${(edit as Group).id}`, { method: "PATCH", body: body(), version: (edit as Group).version })))
      .then((r) => {
        if (r) {
          setEdit(null);
          list.reload();
        }
      });
  const remove = () =>
    act.run(async () => (await api(`/org/payroll-groups/${del!.id}`, { method: "DELETE", version: del!.version }), true)).then((ok) => {
      if (!ok) return;
      setDel(null);
      list.reload();
    });

  return (
    <>
      <PageHead title="Payroll groups" sub="Employees paid on the same cycle.">
        {manage && <Button kind="main" icon="add" onClick={() => open("new")}>New group</Button>}
      </PageHead>
      <ErrorBox error={list.error} />
      <Card pad={false}>
        <Table
          rows={list.data?.items}
          cols={[
            { h: "Name", c: (g) => <>{g.name} {g.is_default && <span className="code">default</span>}</> },
            { h: "Frequency", c: (g) => g.pay_frequency.replace("_", "-") },
            { h: "Cycle", c: cycle },
            { h: "Members today", c: (g) => g.member_count_today, right: true },
            { h: "State", c: (g) => <Pill value={g.state} /> },
            {
              h: "",
              right: true,
              c: (g) =>
                manage && (
                  <span className="row" style={{ justifyContent: "flex-end" }}>
                    {can("employee.edit") && <Button kind="link" onClick={() => setAssign(g)}>Assign</Button>}
                    <Button kind="link" onClick={() => open(g)}>Edit</Button>
                    {!g.is_default && <Button kind="link" onClick={() => { act.setError(null); setDel(g); }}>Delete</Button>}
                  </span>
                ),
            },
          ]}
        />
      </Card>
      <Modal
        open={!!edit}
        title={edit === "new" ? "New payroll group" : "Edit payroll group"}
        onClose={() => setEdit(null)}
        footer={
          <>
            <Button onClick={() => setEdit(null)}>Cancel</Button>
            <Button kind="main" disabled={act.busy} onClick={save}>Save</Button>
          </>
        }
      >
        <div className="grid2">
          <Field label="Name" name="name" error={act.error}>
            <input value={f.name ?? ""} onChange={(e) => setF({ ...f, name: e.target.value })} />
          </Field>
          <Field label="Pay frequency" name="pay_frequency" error={act.error}>
            <Select value={f.pay_frequency} onChange={(v) => setF({ ...f, pay_frequency: v })} options={FREQ} />
          </Field>
          {f.pay_frequency === "monthly" && (
            <Field label="Month start day" name="month_start_day" error={act.error} hint="1–28; blank = calendar month">
              <input type="number" min={1} max={28} value={f.month_start_day} onChange={(e) => setF({ ...f, month_start_day: e.target.value })} />
            </Field>
          )}
          {f.pay_frequency === "weekly" && (
            <Field label="Week starts on" name="week_start_day" error={act.error}>
              <Select value={String(f.week_start_day)} onChange={(v) => setF({ ...f, week_start_day: v })} options={DAYS} />
            </Field>
          )}
          {f.pay_frequency === "bi_weekly" && (
            <Field label="First day of any cycle" name="cycle_anchor" error={act.error}>
              <input type="date" value={f.cycle_anchor} onChange={(e) => setF({ ...f, cycle_anchor: e.target.value })} />
            </Field>
          )}
        </div>
        <ErrorBox error={act.error} fields={["name", "pay_frequency", "month_start_day", "week_start_day", "cycle_anchor"]} />
      </Modal>
      <Modal
        open={!!del}
        title={`Delete ${del?.name}?`}
        onClose={() => setDel(null)}
        footer={
          <>
            <Button onClick={() => setDel(null)}>Cancel</Button>
            <Button kind="danger" disabled={act.busy} onClick={remove}>Delete</Button>
          </>
        }
      >
        <p>Only a group that has never had members can be deleted. This can't be undone.</p>
        <ErrorBox error={act.error} />
      </Modal>
      {assign && <Assign g={assign} onClose={() => { setAssign(null); list.reload(); }} />}
    </>
  );
}
