"use client";
import { useState } from "react";
import { Button, Card, ErrorBox, fmtDate, fmtTime, Modal, PageHead, Pill, Table } from "@/components/ui";
import { api } from "@/lib/api";
import { useAction, useApi, useCan } from "@/lib/hooks";
import { Features, kindLabel, RequestForm } from "../RequestForm";

type Req = { id: string; work_date: string; kind: string; requested: Record<string, string | null>; reason: string; state: string; created_at: string; version: number; current_approver: string | null };
const asked = (r: Record<string, string | null>) =>
  [r.in_time && `in ${r.in_time.slice(0, 5)}`, r.out_time && `out ${r.out_time.slice(0, 5)}`, r.punch_id && (r.direction ? `make it ${r.direction}` : "don't count it")].filter(Boolean).join(" · ") || "—";

// My regularisation requests: raise, track, withdraw while pending.
export default function MyRequests() {
  const can = useCan();
  const list = useApi<{ items: Req[] }>("/attendance/regularisations");
  const features = useApi<Features>("/attendance/me/attendance/features");
  const act = useAction();
  const [raising, setRaising] = useState(false);
  const [withdraw, setWithdraw] = useState<Req | null>(null);
  const r = features.data?.regularisation;

  const doWithdraw = () =>
    act.run(async () => (await api(`/attendance/regularisations/${withdraw!.id}/withdraw`, { method: "POST", version: withdraw!.version }), true)).then((ok) => {
      if (!ok) return;
      setWithdraw(null);
      list.reload();
      features.reload();
    });

  return (
    <>
      <PageHead title="My requests" sub={r?.limit ? `${r.used} of ${r.limit} corrections used this month` : "Corrections to your attendance"}>
        {can("attendance.regularisation.request") && r?.enabled && <Button kind="main" icon="add" onClick={() => setRaising(true)}>New request</Button>}
      </PageHead>
      {r && !r.enabled && <div className="note">Your organisation doesn't take correction requests.</div>}
      <ErrorBox error={list.error} />
      <Card pad={false}>
        <Table
          rows={list.data?.items}
          empty="You haven't asked for any corrections."
          cols={[
            { h: "Date", c: (x) => fmtDate(x.work_date) },
            { h: "Kind", c: (x) => kindLabel(x.kind) },
            { h: "Asked for", c: (x) => asked(x.requested) },
            { h: "Raised", c: (x) => fmtTime(x.created_at) },
            { h: "State", c: (x) => <><Pill value={x.state} />{x.current_approver && <div className="muted small">with {x.current_approver}</div>}</> },
            { h: "", right: true, c: (x) => (x.state === "pending" || x.state === "escalated") && <Button kind="link" onClick={() => { act.setError(null); setWithdraw(x); }}>Withdraw</Button> },
          ]}
        />
      </Card>
      {raising && features.data && <RequestForm features={features.data} onClose={() => setRaising(false)} onDone={() => (list.reload(), features.reload())} />}
      <Modal
        open={!!withdraw}
        center
        title="Withdraw this request?"
        onClose={() => setWithdraw(null)}
        footer={<><Button onClick={() => setWithdraw(null)}>Keep it</Button><Button kind="danger" disabled={act.busy} onClick={doWithdraw}>Withdraw</Button></>}
      >
        <p className="muted" style={{ marginTop: 0 }}>{withdraw && `${kindLabel(withdraw.kind)} on ${fmtDate(withdraw.work_date)}`}</p>
        <ErrorBox error={act.error} />
      </Modal>
    </>
  );
}
