"use client";
import { useState } from "react";
import { Button, Card, ErrorBox, fmtDate, fmtTime, PageHead, Pill, ReasonDialog, Select, Table, TabTools } from "@/components/ui";
import { api } from "@/lib/api";
import { useAction, useApi } from "@/lib/hooks";

type Pick = { id: string; employee_id: string; state: string; version: number; created_at: string; decision_reason: string | null; holiday: { id: string; date: string; name: string } };

export default function OptionalHolidayPicks() {
  const [state, setState] = useState("pending");
  const list = useApi<{ items: Pick[] }>("/attendance/optional-holiday-picks", { state });
  const act = useAction();
  const [rejecting, setRejecting] = useState<Pick | null>(null);
  const decide = (p: Pick, to: "approve" | "reject", reason?: string) =>
    act.run(() => api(`/attendance/optional-holiday-picks/${p.id}/${to}`, { body: { reason: reason || null }, version: p.version })).then((r) => {
      if (r) {
        setRejecting(null);
        list.reload();
      }
    });
  return (
    <>
      <PageHead title="Optional holiday picks" />
      <TabTools>
        <Select value={state} onChange={setState} options={["pending", "approved", "rejected", "withdrawn", "cancelled"].map((s) => ({ value: s, label: s }))} placeholder="Any state" />
      </TabTools>
      <ErrorBox error={list.error ?? (rejecting ? null : act.error)} />
      <Card pad={false}>
        <Table
          rows={list.data?.items}
          empty="No picks."
          cols={[
            // shortcut: the pick shape carries only employee_id; show names when the API adds the employee object.
            { h: "Employee", c: (p) => <code>{p.employee_id.slice(0, 8)}</code> },
            { h: "Holiday", c: (p) => `${p.holiday.name} · ${fmtDate(p.holiday.date)}` },
            { h: "Raised", c: (p) => fmtTime(p.created_at) },
            { h: "State", c: (p) => <Pill value={p.state} /> },
            {
              h: "",
              right: true,
              c: (p) =>
                p.state === "pending" && (
                  <span className="row" style={{ justifyContent: "flex-end" }}>
                    <Button kind="link" disabled={act.busy} onClick={() => decide(p, "approve")}>Approve</Button>
                    <Button kind="link" onClick={() => setRejecting(p)}>Reject</Button>
                  </span>
                ),
            },
          ]}
        />
      </Card>
      <ReasonDialog open={!!rejecting} title="Reject pick" required={false} busy={act.busy} error={act.error} onClose={() => setRejecting(null)} onConfirm={(r) => decide(rejecting!, "reject", r)} />
    </>
  );
}
