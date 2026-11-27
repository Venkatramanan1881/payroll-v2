"use client";
import { useState } from "react";
import { Button, Card, ErrorBox, Field, PageHead, Select, Table, TabTools } from "@/components/ui";
import { api } from "@/lib/api";
import { today, useAction, useApi } from "@/lib/hooks";
import { useBranches } from "@/lib/lookups";
import { Outcome, toTime } from "../_shared";

type Row = { employee: { id: string; employee_code: string; full_name: string }; entries: { id: string; direction: string; local_time: string }[] };

// P8: one batch per branch and date; an out earlier than the in is the next morning (server rule).
export default function ManualAttendance() {
  const branches = useBranches();
  const [branch, setBranch] = useState("");
  const [date, setDate] = useState(today());
  const sheet = useApi<{ items: Row[] }>(branch ? "/attendance/manual-attendance/sheet" : null, { branch_id: branch, work_date: date });
  const [times, setTimes] = useState<Record<string, { in: string; out: string }>>({});
  const [reason, setReason] = useState("");
  const [outcome, setOutcome] = useState<any>(null);
  const act = useAction();
  const t = (id: string) => times[id] ?? { in: "", out: "" };
  const setT = (id: string, k: "in" | "out", v: string) => setTimes({ ...times, [id]: { ...t(id), [k]: v } });
  const entries = Object.entries(times)
    .filter(([, v]) => v.in || v.out)
    .map(([employee_id, v]) => ({ employee_id, in_time: toTime(v.in), out_time: toTime(v.out) }));
  const submit = () =>
    act.run(() => api("/attendance/manual-attendance", { body: { branch_id: branch, work_date: date, reason, entries } })).then((r) => {
      if (r) {
        setOutcome(r);
        setTimes({});
        sheet.reload();
      }
    });
  return (
    <>
      <PageHead title="Manual attendance">
        <Button kind="main" disabled={!branch || !entries.length || act.busy} onClick={submit}>Save {entries.length || ""} entries</Button>
      </PageHead>
      <TabTools>
        <Field label="Branch"><Select value={branch} onChange={setBranch} options={branches.options} placeholder="Choose…" /></Field>
        <Field label="Date"><input type="date" value={date} onChange={(e) => setDate(e.target.value)} /></Field>
        <Field label="Reason for the batch" name="reason" error={act.error}><input value={reason} onChange={(e) => setReason(e.target.value)} /></Field>
      </TabTools>
      <Outcome value={outcome} />
      <ErrorBox error={sheet.error ?? act.error} fields={["reason"]} />
      {branch ? (
        <Card pad={false}>
          <Table
            rows={sheet.data?.items}
            empty="No employees placed at this branch on this date."
            cols={[
              { h: "Employee", c: (r) => <><span className="code">{r.employee.employee_code}</span> {r.employee.full_name}</> },
              { h: "Already entered", c: (r) => r.entries.map((e) => `${e.direction} ${e.local_time?.slice(11, 16)}`).join(", ") || "—" },
              { h: "In", c: (r) => <input type="time" value={t(r.employee.id).in} onChange={(e) => setT(r.employee.id, "in", e.target.value)} style={{ width: 130 }} /> },
              { h: "Out", c: (r) => <input type="time" value={t(r.employee.id).out} onChange={(e) => setT(r.employee.id, "out", e.target.value)} style={{ width: 130 }} /> },
            ]}
          />
        </Card>
      ) : (
        <Card><p className="muted">Choose a branch and date to load the sheet.</p></Card>
      )}
    </>
  );
}
