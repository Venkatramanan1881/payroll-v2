"use client";
import { useState } from "react";
import { Button, Card, Code, ErrorBox, Field, Modal, PageHead, Pill, Select, Table } from "@/components/ui";
import { api } from "@/lib/api";
import { useAction, useApi, useCan } from "@/lib/hooks";

type Role = { id: string; name: string; builtin_key: string | null; state: string; permissions: string[]; user_count: number; version: number };
type Perm = { code: string; description: string; is_fixed: boolean };

function Permissions({ role, catalogue, onClose, onSaved }: { role: Role; catalogue: Perm[]; onClose: () => void; onSaved: () => void }) {
  const can = useCan();
  const [on, setOn] = useState(new Set(role.permissions));
  const act = useAction();
  // Org Admin always holds everything; fixed permissions can't be granted or removed.
  const locked = role.builtin_key === "org_admin" || !can("access.role.edit");
  const groups = catalogue.reduce<Record<string, Perm[]>>((g, p) => ((g[p.code.split(".")[0]] ??= []).push(p), g), {});
  const flip = (c: string) => {
    const n = new Set(on);
    n.has(c) ? n.delete(c) : n.add(c);
    setOn(n);
  };
  const save = () =>
    act.run(() => api(`/access/roles/${role.id}/permissions`, { method: "PUT", body: { permissions: [...on] }, version: role.version })).then((r) => r && onSaved());
  return (
    <Modal
      open
      wide
      title={`${role.name} · permissions`}
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Close</Button>
          {!locked && <Button kind="main" disabled={act.busy} onClick={save}>Save</Button>}
        </>
      }
    >
      {role.builtin_key === "org_admin" && <div className="note">The Organisation Admin role always has every permission.</div>}
      <div className="stack">
        {Object.entries(groups).map(([g, perms]) => (
          <div key={g} className="form-section">
            <h3 style={{ textTransform: "capitalize" }}>{g}</h3>
            <div className="grid2">
              {perms.map((p) => (
                <label key={p.code} className="check" style={{ alignItems: "flex-start" }} title={p.is_fixed ? "Fixed permission" : undefined}>
                  <input type="checkbox" checked={on.has(p.code)} disabled={locked || p.is_fixed} onChange={() => flip(p.code)} />
                  <span>
                    <Code>{p.code}</Code>
                    <div className="muted small">{p.description}</div>
                  </span>
                </label>
              ))}
            </div>
          </div>
        ))}
      </div>
      <ErrorBox error={act.error} />
    </Modal>
  );
}

export default function Roles() {
  const can = useCan();
  const list = useApi<{ items: Role[] }>("/access/roles");
  const catalogue = useApi<{ items: Perm[] }>(can("access.role.edit") ? "/access/permissions" : null);
  const [open, setOpen] = useState<Role | null>(null);
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");
  const [copy, setCopy] = useState("");
  const act = useAction();
  const create = () =>
    act.run(() => api("/access/roles", { body: { name, copy_from_role_id: copy || undefined } })).then((r) => {
      if (!r) return;
      setCreating(false);
      setName("");
      setCopy("");
      list.reload();
    });

  return (
    <>
      <PageHead title="Roles & permissions">
        {can("access.role.edit") && <Button kind="main" icon="add" onClick={() => { act.setError(null); setCreating(true); }}>New role</Button>}
      </PageHead>
      <ErrorBox error={list.error ?? catalogue.error} />
      <Card pad={false}>
        <Table
          rows={list.data?.items}
          onRow={catalogue.data ? setOpen : undefined}
          cols={[
            { h: "Role", c: (r) => <>{r.name} {r.builtin_key && <span className="code">built-in</span>}</> },
            { h: "Permissions", c: (r) => r.permissions.length, right: true },
            { h: "Users", c: (r) => r.user_count, right: true },
            { h: "State", c: (r) => <Pill value={r.state} /> },
          ]}
        />
      </Card>
      {open && catalogue.data && (
        <Permissions role={open} catalogue={catalogue.data.items} onClose={() => setOpen(null)} onSaved={() => { setOpen(null); list.reload(); }} />
      )}
      <Modal
        open={creating}
        title="New role"
        onClose={() => setCreating(false)}
        footer={
          <>
            <Button onClick={() => setCreating(false)}>Cancel</Button>
            <Button kind="main" disabled={act.busy} onClick={create}>Create</Button>
          </>
        }
      >
        <div className="stack">
          <Field label="Name" name="name" error={act.error}>
            <input value={name} onChange={(e) => setName(e.target.value)} />
          </Field>
          <Field label="Copy permissions from" name="copy_from_role_id" error={act.error} hint="Optional. Fixed permissions are never copied.">
            <Select value={copy} onChange={setCopy} placeholder="Start empty" options={(list.data?.items ?? []).map((r) => ({ value: r.id, label: r.name }))} />
          </Field>
        </div>
        <ErrorBox error={act.error} fields={["name", "copy_from_role_id"]} />
      </Modal>
    </>
  );
}
