"use client";
import { useState } from "react";
import { Button, Card, ErrorBox, Field, fmtTime, PageHead, Pill, ReasonDialog, Select, Table } from "@/components/ui";
import { api } from "@/lib/api";
import { useAction, useApi } from "@/lib/hooks";
import { useBranches } from "@/lib/lookups";

type Review = {
  id: string; state: string; flags: string[]; opened_at: string; version: number; decision_reason: string | null;
  punch: { employee: { employee_code: string; full_name: string } | null; local_time: string; direction: string; source: string; location: { fence_distance_m: number | null; accuracy_m: number } | null };
};

export default function PunchReviews() {
  const branches = useBranches();
  const [state, setState] = useState("pending");
  const [branch, setBranch] = useState("");
  const list = useApi<{ items: Review[] }>("/attendance/punch-reviews", { state, branch_id: branch });
  const act = useAction();
  const [rejecting, setRejecting] = useState<Review | null>(null);
  const decide = (r: Review, to: "approve" | "reject", reason?: string) =>
    act.run(() => api(`/attendance/punch-reviews/${r.id}/${to}`, { body: { reason: reason || null }, version: r.version })).then((x) => {
      if (x) {
        setRejecting(null);
        list.reload();
      }
    });
  return (
    <>
      <PageHead title="Punch reviews" />
      <div className="filters">
        <Field label="State"><Select value={state} onChange={setState} options={["pending", "approved", "rejected"].map((s) => ({ value: s, label: s }))} placeholder="Any" /></Field>
        <Field label="Branch"><Select value={branch} onChange={setBranch} options={branches.options} placeholder="Any" /></Field>
      </div>
      <ErrorBox error={list.error ?? (rejecting ? null : act.error)} />
      <Card pad={false}>
        <Table
          rows={list.data?.items}
          empty="Nothing to review."
          cols={[
            { h: "Employee", c: (r) => (r.punch.employee ? `${r.punch.employee.employee_code} · ${r.punch.employee.full_name}` : "—") },
            { h: "Punch", c: (r) => `${r.punch.local_time?.replace("T", " ").slice(0, 16)} · ${r.punch.direction} · ${r.punch.source}` },
            { h: "Flags", c: (r) => r.flags.map((f) => <Pill key={f} value={f} />) },
            { h: "Distance", c: (r) => (r.punch.location?.fence_distance_m != null ? `${Math.round(r.punch.location.fence_distance_m)} m` : "—") },
            { h: "Opened", c: (r) => fmtTime(r.opened_at) },
            { h: "State", c: (r) => <Pill value={r.state} /> },
            {
              h: "",
              right: true,
              c: (r) =>
                r.state === "pending" && (
                  <span className="row" style={{ justifyContent: "flex-end" }}>
                    <Button kind="link" disabled={act.busy} onClick={() => decide(r, "approve")}>Approve</Button>
                    <Button kind="link" onClick={() => setRejecting(r)}>Reject</Button>
                  </span>
                ),
            },
          ]}
        />
      </Card>
      <ReasonDialog open={!!rejecting} title="Reject punch (it will be excluded)" busy={act.busy} error={act.error} onClose={() => setRejecting(null)} onConfirm={(r) => decide(rejecting!, "reject", r)} />
    </>
  );
}
