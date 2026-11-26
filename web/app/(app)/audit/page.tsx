"use client";
import { useEffect, useState } from "react";
import { Button, Card, Code, ErrorBox, Field, fmtTime, Modal, PageHead, Table } from "@/components/ui";
import { api, ApiError } from "@/lib/api";

type Entry = {
  id: number;
  occurred_at: string;
  actor_kind: string;
  actor_id: string | null;
  actor_name: string | null;
  action: string;
  target_type: string;
  target_id: string | null;
  old_values: unknown;
  new_values: unknown;
  reason: string | null;
  correlation_id: string | null;
};

// Accepted filters: target_type, target_id, actor_id, action, from, to (+ limit, before for paging).
export default function Audit() {
  const [f, setF] = useState({ target_type: "", target_id: "", actor_id: "", action: "", from: "", to: "" });
  const [applied, setApplied] = useState(f);
  const [items, setItems] = useState<Entry[] | null>(null);
  const [next, setNext] = useState<number | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  const [open, setOpen] = useState<Entry | null>(null);

  const load = async (before?: number) => {
    setError(null);
    try {
      const r = await api<{ items: Entry[]; next_before: number | null }>("/audit", { query: { ...applied, limit: 50, before } });
      setItems((cur) => (before ? [...(cur ?? []), ...r.items] : r.items));
      setNext(r.next_before);
    } catch (e) {
      setError(e as ApiError);
      if (!before) setItems([]);
    }
  };
  useEffect(() => {
    setItems(null);
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [applied]);

  const input = (k: keyof typeof f, label: string, type = "text") => (
    <Field label={label}>
      <input type={type} value={f[k]} onChange={(e) => setF({ ...f, [k]: e.target.value.trim() })} />
    </Field>
  );

  return (
    <>
      <PageHead title="Audit log" sub="Every change, who made it and why. Read-only." />
      <div className="filters">
        {input("action", "Action (permission code)")}
        {input("target_type", "Target type")}
        {input("target_id", "Target id")}
        {input("actor_id", "Actor id")}
        {input("from", "From", "date")}
        {input("to", "To", "date")}
        <Button kind="main" icon="filter_alt" onClick={() => setApplied(f)} style={{ alignSelf: "end" }}>Apply</Button>
      </div>
      <ErrorBox error={error} />
      <Card pad={false}>
        <Table
          rows={items}
          onRow={setOpen}
          empty="No audit entries match."
          cols={[
            { h: "When", c: (e) => fmtTime(e.occurred_at) },
            { h: "Actor", c: (e) => e.actor_name ?? e.actor_kind },
            { h: "Action", c: (e) => <Code>{e.action}</Code> },
            { h: "Target", c: (e) => <>{e.target_type} <span className="muted small">{e.target_id?.slice(0, 8)}</span></> },
            { h: "Reason", c: (e) => e.reason ?? <span className="muted">—</span> },
          ]}
        />
      </Card>
      {next && (
        <div style={{ marginTop: 12 }}>
          <Button onClick={() => load(next)}>Load more</Button>
        </div>
      )}
      <Modal open={!!open} wide title={open ? `${open.action} · ${open.target_type}` : ""} onClose={() => setOpen(null)}>
        {open && (
          <div className="stack">
            <dl className="kv">
              <dt>When</dt><dd>{fmtTime(open.occurred_at)}</dd>
              <dt>Actor</dt><dd>{open.actor_name ?? open.actor_kind} <span className="muted small">{open.actor_id}</span></dd>
              <dt>Target</dt><dd>{open.target_type} <span className="muted small">{open.target_id}</span></dd>
              <dt>Reason</dt><dd>{open.reason ?? "—"}</dd>
              <dt>Correlation id</dt><dd><code>{open.correlation_id ?? "—"}</code></dd>
            </dl>
            <div className="grid2">
              <div><h3 className="small muted">Before</h3><pre className="json">{JSON.stringify(open.old_values, null, 2)}</pre></div>
              <div><h3 className="small muted">After</h3><pre className="json">{JSON.stringify(open.new_values, null, 2)}</pre></div>
            </div>
          </div>
        )}
      </Modal>
    </>
  );
}
