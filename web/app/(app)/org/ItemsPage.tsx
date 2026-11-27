"use client";
import { ReactNode, useState } from "react";
import { Button, Card, Code, ErrorBox, Field, Icon, Modal, PhoneInput, Pill, ReasonDialog, Select, Table, TabTools } from "@/components/ui";
import { api } from "@/lib/api";
import { useAction, useApi, useCan } from "@/lib/hooks";

export type Item = { id: string; state: string; version: number; name?: string; [k: string]: any };

/**
 * List + create/edit + deactivate/activate for the org.rs "items" routes (branches, lists, contractors).
 * Request keys are exactly `code` + `name` + `extra`; the server refuses anything else.
 */
export function ItemsPage({
  path,
  codeKey = "code",
  codeLabel = "Code",
  extra = [],
  defaults = {},
  manage,
  noun,
  rowActions,
  onStateResult,
}: {
  path: string;
  codeKey?: string;
  codeLabel?: string;
  extra?: { key: string; label: string; hint?: string }[];
  defaults?: Record<string, string>;
  manage: string;
  noun: string;
  rowActions?: (i: Item) => ReactNode;
  onStateResult?: (doc: any) => void;
}) {
  const can = useCan();
  const [state, setState] = useState("active");
  const [q, setQ] = useState("");
  const list = useApi<{ items: Item[] }>(path, { state, q });
  const [edit, setEdit] = useState<Item | "new" | null>(null);
  const [form, setForm] = useState<Record<string, string>>({});
  const [toggle, setToggle] = useState<Item | null>(null);
  const act = useAction();
  const keys = [codeKey, "name", ...extra.map((e) => e.key)];
  const labels: Record<string, string> = { [codeKey]: codeLabel, name: "Name", ...Object.fromEntries(extra.map((e) => [e.key, e.label])) };

  const open = (i: Item | "new") => {
    act.setError(null);
    setForm(Object.fromEntries(keys.map((k) => [k, i === "new" ? defaults[k] ?? "" : i[k] ?? ""])));
    setEdit(i);
  };
  const save = () =>
    act
      .run(() => {
        if (edit === "new") return api(path, { body: Object.fromEntries(keys.filter((k) => form[k]).map((k) => [k, form[k]])) });
        const i = edit as Item;
        const changed = Object.fromEntries(keys.filter((k) => (i[k] ?? "") !== form[k]).map((k) => [k, form[k] || null]));
        return api(`${path}/${i.id}`, { method: "PATCH", body: changed, version: i.version });
      })
      .then((r) => {
        if (r) {
          setEdit(null);
          list.reload();
        }
      });
  const flip = (reason: string) => {
    const i = toggle!;
    const to = i.state === "active" ? "deactivate" : "activate";
    act.run(() => api(`${path}/${i.id}/${to}`, { body: { reason: reason || undefined }, version: i.version })).then((doc) => {
      if (!doc) return;
      setToggle(null);
      onStateResult?.(doc);
      list.reload();
    });
  };

  return (
    <>
      <TabTools>
        <div className="search with-icon">
          <Icon name="search" />
          <input aria-label="Search" placeholder="Search code or name" value={q} onChange={(e) => setQ(e.target.value)} />
        </div>
        <Select value={state} onChange={setState} options={[{ value: "active", label: "Active" }, { value: "inactive", label: "Inactive" }]} />
      </TabTools>
      {can(manage) && (
        <TabTools to="head-actions" className="row">
          <Button kind="main" icon="add" onClick={() => open("new")}>
            New {noun}
          </Button>
        </TabTools>
      )}
      <ErrorBox error={list.error} />
      <Card pad={false}>
        <Table
          search={false}
          rows={list.data?.items}
          empty={`No ${state} ${noun}s.`}
          cols={[
            { h: codeLabel, c: (i) => <Code>{i[codeKey]}</Code> },
            { h: "Name", c: (i) => i.name },
            ...extra.map((e) => ({ h: e.label, c: (i: Item) => i[e.key] ?? <span className="muted">—</span> })),
            { h: "State", c: (i) => <Pill value={i.state} /> },
            {
              h: "",
              right: true,
              c: (i) => (
                <span className="row" style={{ justifyContent: "flex-end" }}>
                  {rowActions?.(i)}
                  {can(manage) && (
                    <>
                      <Button kind="link" onClick={() => open(i)}>Edit</Button>
                      <Button kind="link" onClick={() => { act.setError(null); setToggle(i); }}>
                        {i.state === "active" ? "Deactivate" : "Activate"}
                      </Button>
                    </>
                  )}
                </span>
              ),
            },
          ]}
        />
      </Card>
      <Modal
        open={!!edit}
        title={edit === "new" ? `New ${noun}` : `Edit ${noun}`}
        onClose={() => setEdit(null)}
        footer={
          <>
            <Button onClick={() => setEdit(null)}>Cancel</Button>
            <Button kind="main" disabled={act.busy} onClick={save}>Save</Button>
          </>
        }
      >
        <div className="grid2">
          {keys.map((k) => (
            <Field key={k} label={labels[k]} name={k} error={act.error} hint={extra.find((e) => e.key === k)?.hint}>
              {k.endsWith("phone") ? (
                <PhoneInput value={form[k]} onChange={(v) => setForm({ ...form, [k]: v })} />
              ) : (
                <input value={form[k] ?? ""} onChange={(e) => setForm({ ...form, [k]: e.target.value })} />
              )}
            </Field>
          ))}
        </div>
        <ErrorBox error={act.error} fields={keys} />
      </Modal>
      <ReasonDialog
        open={!!toggle}
        title={`${toggle?.state === "active" ? "Deactivate" : "Activate"} ${toggle?.name ?? noun}`}
        required={toggle?.state === "active"}
        confirm={toggle?.state === "active" ? "Deactivate" : "Activate"}
        onClose={() => setToggle(null)}
        onConfirm={flip}
        error={act.error}
        busy={act.busy}
      />
    </>
  );
}
