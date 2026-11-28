"use client";
import { useState } from "react";
import { Button, Card, ErrorBox, fmtDate, fmtTime, PageHead, Pill, ReasonDialog, Select, Table, TabTools } from "@/components/ui";
import { api } from "@/lib/api";
import { useAction, useApi } from "@/lib/hooks";
import { shiftAsk, ShiftRequest } from "@/app/portal/ShiftRequests";

// Shift change and swap requests from the team (SW2, SW3). One approval of a swap changes both rosters (ATQ-12).
export default function ShiftRequests() {
  const [state, setState] = useState("pending");
  const list = useApi<{ items: (ShiftRequest & { created_at: string })[] }>("/attendance/shift-requests", { state });
  const act = useAction();
  const [rejecting, setRejecting] = useState<ShiftRequest | null>(null);
  const decide = (r: ShiftRequest, to: "approve" | "reject", reason?: string) =>
    act.run(() => api(`/attendance/shift-requests/${r.id}/${to}`, { body: { reason: reason || null }, version: r.version })).then((x) => {
      if (x) {
        setRejecting(null);
        list.reload();
      }
    });
  return (
    <>
      <PageHead title="Shift requests" />
      <TabTools>
        <Select value={state} onChange={setState} options={["pending", "approved", "rejected", "withdrawn"].map((s) => ({ value: s, label: s }))} placeholder="Any state" />
      </TabTools>
      <ErrorBox error={list.error ?? (rejecting ? null : act.error)} />
      <Card pad={false}>
        <Table
          rows={list.data?.items}
          empty="No shift requests."
          cols={[
            { h: "Employee", c: (r) => <><span className="code">{r.requester.employee_code}</span> {r.requester.full_name}</> },
            { h: "Date", c: (r) => fmtDate(r.work_date) },
            { h: "Request", c: (r) => shiftAsk(r) },
            { h: "Reason", c: (r) => r.reason },
            { h: "Raised", c: (r) => fmtTime(r.created_at) },
            { h: "State", c: (r) => <><Pill value={r.state} />{r.decision_reason && <div className="muted small">{r.decision_reason}</div>}</> },
            {
              h: "",
              right: true,
              c: (r) =>
                r.state === "pending" && (
                  <span className="row" style={{ justifyContent: "flex-end" }}>
                    <Button kind="link" disabled={act.busy} onClick={() => decide(r, "approve")}>Approve</Button>
                    <Button kind="link" onClick={() => { act.setError(null); setRejecting(r); }}>Reject</Button>
                  </span>
                ),
            },
          ]}
        />
      </Card>
      <ReasonDialog open={!!rejecting} title="Reject shift request" busy={act.busy} error={act.error} onClose={() => setRejecting(null)} onConfirm={(r) => decide(rejecting!, "reject", r)} />
    </>
  );
}
