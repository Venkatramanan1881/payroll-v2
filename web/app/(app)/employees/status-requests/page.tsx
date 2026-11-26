"use client";
import Link from "next/link";
import { useState } from "react";
import { Button, Card, ErrorBox, fmtDate, fmtTime, PageHead, Pill, ReasonDialog, Select, Table } from "@/components/ui";
import { api } from "@/lib/api";
import { useAccess, useAction, useApi, useCan } from "@/lib/hooks";

type Req = any;
type Act = "approve" | "reject" | "withdraw" | "cancel";
const STATES = ["pending", "scheduled", "effective", "rejected", "withdrawn", "cancelled"].map((s) => ({ value: s, label: s }));

// S2–S7.
export default function StatusRequests() {
  const me = useAccess();
  const can = useCan();
  const [state, setState] = useState("pending");
  const [mine, setMine] = useState(false);
  const list = useApi<{ items: Req[]; next_cursor: string | null }>("/status-requests", { state, awaiting_me: mine || undefined });
  const act = useAction();
  const [pick, setPick] = useState<{ r: Req; a: Act } | null>(null);
  const run = (reason: string) => {
    const { r, a } = pick!;
    const body = a === "approve" ? { note: reason || undefined } : a === "withdraw" ? undefined : { reason };
    act.run(() => api(`/status-requests/${r.id}/${a}`, { method: "POST", body, version: r.version })).then((x) => x && (setPick(null), list.reload()));
  };
  const actions = (r: Req) => (
    <div className="row" style={{ justifyContent: "flex-end" }}>
      {r.state === "pending" && can("employee.status.approve") && r.raised_by?.id !== me.user.id && (
        <>
          <Button kind="link" onClick={() => setPick({ r, a: "approve" })}>Approve</Button>
          <Button kind="link" onClick={() => setPick({ r, a: "reject" })}>Reject</Button>
        </>
      )}
      {r.state === "pending" && r.raised_by?.id === me.user.id && <Button kind="link" onClick={() => setPick({ r, a: "withdraw" })}>Withdraw</Button>}
      {r.state === "scheduled" && can("employee.status.cancel") && <Button kind="link" onClick={() => setPick({ r, a: "cancel" })}>Cancel</Button>}
    </div>
  );
  return (
    <>
      <PageHead title="Status requests" sub="Joining, notice, long leave and exit changes awaiting a decision" />
      <div className="filters">
        <Select value={state} onChange={setState} options={STATES} placeholder="Any state" />
        <label className="check">
          <input type="checkbox" checked={mine} onChange={(x) => setMine(x.target.checked)} /> Awaiting me
        </label>
      </div>
      <ErrorBox error={list.error} />
      <Card pad={false}>
        <Table
          rows={list.data?.items}
          empty="No requests."
          cols={[
            { h: "Employee", c: (r) => <Link href={`/employees/${r.employee.id}`}>{r.employee.employee_code} · {r.employee.full_name}</Link> },
            { h: "Change", c: (r) => <>{r.from_status} → <b>{r.to_status}</b></> },
            { h: "Effective", c: (r) => fmtDate(r.effective_date) },
            { h: "Reason", c: (r) => r.reason },
            { h: "Approver", c: (r) => r.approver_role?.replace(/_/g, " ") },
            { h: "Raised", c: (r) => `${r.raised_by?.name ?? ""} · ${fmtTime(r.raised_at)}` },
            { h: "State", c: (r) => <Pill value={r.state} /> },
            { h: "", c: actions, right: true },
          ]}
        />
      </Card>
      <ReasonDialog
        open={!!pick}
        title={pick ? `${pick.a[0].toUpperCase()}${pick.a.slice(1)} request` : ""}
        label={pick?.a === "approve" ? "Note" : "Reason"}
        required={pick?.a === "reject" || pick?.a === "cancel"}
        confirm={pick?.a === "approve" ? "Approve" : "Confirm"}
        onClose={() => setPick(null)}
        onConfirm={run}
        error={act.error}
        busy={act.busy}
      >
        {pick && (
          <p>
            {pick.r.employee.full_name}: {pick.r.from_status} → {pick.r.to_status} from {fmtDate(pick.r.effective_date)}
          </p>
        )}
      </ReasonDialog>
    </>
  );
}
