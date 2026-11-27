"use client";
import { useState } from "react";
import { Button, Card, ErrorBox, Field, Modal, MultiSelect, Opt, PageHead, Pill, ReasonDialog, Select, Table, Tabs } from "@/components/ui";
import { api } from "@/lib/api";
import { useAction, useApi } from "@/lib/hooks";
import { useBranches, useList } from "@/lib/lookups";

type Grant = { role_id: string; scope_kind: string; branch_ids: string[]; department_ids: string[] };
type User = { id: string; kind: string; email: string | null; display_name: string; state: string; employee_id: string | null; version: number; roles: (Grant & { name: string; builtin_key: string | null })[] };
type Role = { id: string; name: string; builtin_key: string | null; state: string };

const SCOPES = [
  { value: "organisation", label: "Whole organisation" },
  { value: "branches", label: "Chosen branches" },
  { value: "departments", label: "Chosen departments" },
];

function Grants({ value, onChange, roles, employeeUser }: { value: Grant[]; onChange: (g: Grant[]) => void; roles: Role[]; employeeUser: boolean }) {
  const branches = useBranches();
  const deps = useList("departments");
  // The Employee role comes with portal access; Manager is only for employee users.
  const options: Opt[] = roles
    .filter((r) => r.state === "active" && r.builtin_key !== "employee" && (employeeUser || r.builtin_key !== "manager"))
    .map((r) => ({ value: r.id, label: r.name }));
  const set = (i: number, g: Partial<Grant>) => onChange(value.map((x, j) => (j === i ? { ...x, ...g } : x)));
  const multi = (opts: Opt[], v: string[], f: (v: string[]) => void) => (
    <MultiSelect options={opts} value={v} onChange={f} placeholder="Choose…" />
  );
  return (
    <div className="stack">
      {value.map((g, i) => (
        <div key={i} className="grid2" style={{ alignItems: "start", paddingBottom: 12, borderBottom: "1px solid var(--divider)" }}>
          <Field label="Role">
            <Select value={g.role_id} onChange={(v) => set(i, { role_id: v })} options={options} placeholder="Choose a role" />
          </Field>
          <Field label="Scope">
            <Select value={g.scope_kind} onChange={(v) => set(i, { scope_kind: v, branch_ids: [], department_ids: [] })} options={SCOPES} />
          </Field>
          {g.scope_kind === "branches" && <Field label="Branches">{multi(branches.options, g.branch_ids, (v) => set(i, { branch_ids: v }))}</Field>}
          {g.scope_kind === "departments" && <Field label="Departments">{multi(deps.options, g.department_ids, (v) => set(i, { department_ids: v }))}</Field>}
          <div className="row" style={{ alignSelf: "end" }}>
            <Button kind="link" onClick={() => onChange(value.filter((_, j) => j !== i))}>Remove</Button>
          </div>
        </div>
      ))}
      <div>
        <Button icon="add" onClick={() => onChange([...value, { role_id: "", scope_kind: "organisation", branch_ids: [], department_ids: [] }])}>Add role</Button>
      </div>
    </div>
  );
}

const clean = (gs: Grant[]) => gs.map(({ role_id, scope_kind, branch_ids, department_ids }) => ({ role_id, scope_kind, branch_ids, department_ids }));

export default function Users() {
  const [kind, setKind] = useState<"admin" | "employee">("admin");
  const list = useApi<{ items: User[] }>("/access/users", { kind });
  const roles = useApi<{ items: Role[] }>("/access/roles");
  const branches = useBranches();
  const deps = useList("departments");
  const [invite, setInvite] = useState(false);
  const [form, setForm] = useState({ email: "", display_name: "" });
  const [grants, setGrants] = useState<Grant[]>([]);
  const [edit, setEdit] = useState<User | null>(null);
  const [toggle, setToggle] = useState<User | null>(null);
  const [notice, setNotice] = useState("");
  const act = useAction();

  const name = (opts: Opt[], id: string) => opts.find((o) => o.value === id)?.label ?? id.slice(0, 8);
  const scope = (g: Grant) =>
    g.scope_kind === "branches" ? g.branch_ids.map((b) => name(branches.options, b)).join(", ") : g.scope_kind === "departments" ? g.department_ids.map((d) => name(deps.options, d)).join(", ") : "organisation";
  const done = (msg: string) => (r: unknown) => {
    if (!r) return;
    setInvite(false);
    setEdit(null);
    setToggle(null);
    setNotice(msg);
    list.reload();
  };
  const send = () => act.run(() => api("/access/users", { body: { ...form, roles: clean(grants) } })).then(done(`Invitation sent to ${form.email}.`));
  const saveRoles = () => act.run(() => api(`/access/users/${edit!.id}/roles`, { method: "PUT", body: { roles: clean(grants) }, version: edit!.version })).then(done("Roles saved."));
  const flip = (reason: string) => {
    const u = toggle!;
    const to = u.state === "suspended" ? "reactivate" : "suspend";
    act.run(() => api(`/access/users/${u.id}/${to}`, { body: { reason: reason || undefined } })).then(done(`${u.display_name} ${to === "suspend" ? "suspended" : "reactivated"}.`));
  };

  return (
    <>
      <PageHead title="Users">
        <Button kind="main" icon="person_add" onClick={() => { act.setError(null); setForm({ email: "", display_name: "" }); setGrants([]); setInvite(true); }}>
          Invite admin
        </Button>
      </PageHead>
      <div className="filters">
        <Tabs value={kind} onChange={setKind} tabs={[{ value: "admin", label: "Admins" }, { value: "employee", label: "Employee users" }]} />
      </div>
      {notice && <div className="note">{notice}</div>}
      <ErrorBox error={list.error ?? roles.error} />
      <Card pad={false}>
        <Table
          rows={list.data?.items}
          empty="No users."
          cols={[
            { h: "Name", c: (u) => <><div>{u.display_name}</div><div className="muted small">{u.email}</div></> },
            { h: "Roles", c: (u) => u.roles.map((g) => <div key={g.role_id}>{g.name} <span className="muted small">· {scope(g)}</span></div>) },
            { h: "State", c: (u) => <Pill value={u.state} /> },
            {
              h: "",
              right: true,
              c: (u) => (
                <span className="row" style={{ justifyContent: "flex-end" }}>
                  <Button kind="link" onClick={() => { act.setError(null); setGrants(clean(u.roles.filter((g) => g.builtin_key !== "employee"))); setEdit(u); }}>Roles</Button>
                  {(u.state === "active" || u.state === "suspended") && (
                    <Button kind="link" onClick={() => { act.setError(null); setToggle(u); }}>{u.state === "suspended" ? "Reactivate" : "Suspend"}</Button>
                  )}
                </span>
              ),
            },
          ]}
        />
      </Card>
      <Modal
        open={invite}
        wide
        title="Invite an admin"
        onClose={() => setInvite(false)}
        footer={
          <>
            <Button onClick={() => setInvite(false)}>Cancel</Button>
            <Button kind="main" disabled={act.busy} onClick={send}>Send invitation</Button>
          </>
        }
      >
        <div className="grid2" style={{ marginBottom: 16 }}>
          <Field label="Email" name="email" error={act.error}>
            <input type="email" value={form.email} onChange={(e) => setForm({ ...form, email: e.target.value })} />
          </Field>
          <Field label="Display name" name="display_name" error={act.error}>
            <input value={form.display_name} onChange={(e) => setForm({ ...form, display_name: e.target.value })} />
          </Field>
        </div>
        <Grants value={grants} onChange={setGrants} roles={roles.data?.items ?? []} employeeUser={false} />
        <ErrorBox error={act.error} fields={["email", "display_name"]} />
      </Modal>
      <Modal
        open={!!edit}
        wide
        title={`Roles of ${edit?.display_name}`}
        onClose={() => setEdit(null)}
        footer={
          <>
            <Button onClick={() => setEdit(null)}>Cancel</Button>
            <Button kind="main" disabled={act.busy} onClick={saveRoles}>Save roles</Button>
          </>
        }
      >
        {edit && <Grants value={grants} onChange={setGrants} roles={roles.data?.items ?? []} employeeUser={edit.kind === "employee"} />}
        <ErrorBox error={act.error} />
      </Modal>
      <ReasonDialog
        open={!!toggle}
        title={`${toggle?.state === "suspended" ? "Reactivate" : "Suspend"} ${toggle?.display_name}`}
        required={toggle?.state !== "suspended"}
        confirm={toggle?.state === "suspended" ? "Reactivate" : "Suspend"}
        onClose={() => setToggle(null)}
        onConfirm={flip}
        error={act.error}
        busy={act.busy}
      >
        {toggle?.state !== "suspended" && <p className="muted">Suspending signs this user out everywhere at once.</p>}
      </ReasonDialog>
    </>
  );
}
