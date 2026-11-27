"use client";
import { useState } from "react";
import { Button, Card, ErrorBox, Field, fmtDate, mins, PageHead, Pill, Table } from "@/components/ui";
import { today, useApi, useCan } from "@/lib/hooks";
import { Features, RequestForm } from "../RequestForm";

type Day = {
  work_date: string; status: string; state: string;
  shift: { code?: string } | null;
  first_in: string | null; last_out: string | null; net_minutes: number | null;
  late_minutes: number; early_minutes: number; flags: string[];
};
const at = (d: string | null) => (d ? new Date(d).toLocaleTimeString("en-IN", { hour: "2-digit", minute: "2-digit" }) : "—");
const monthStart = () => today().slice(0, 8) + "01";

// My calculated days (M3). The server works them out from punches, shift and policy.
export default function MyAttendance() {
  const can = useCan();
  const [f, setF] = useState({ from: monthStart(), to: today() });
  const days = useApi<{ items: Day[] }>("/attendance/me/days", f);
  const features = useApi<Features>("/attendance/me/attendance/features");
  const [ask, setAsk] = useState<string | null>(null);
  const canAsk = can("attendance.regularisation.request") && features.data?.regularisation.enabled;
  const count = (s: string) => days.data?.items.filter((d) => d.status === s).length ?? 0;

  return (
    <>
      <PageHead title="My attendance">
        {canAsk && <Button kind="main" icon="edit_calendar" onClick={() => setAsk("")}>Request a correction</Button>}
      </PageHead>
      <div className="filters">
        <Field label="From"><input type="date" value={f.from} onChange={(e) => setF({ ...f, from: e.target.value })} /></Field>
        <Field label="To"><input type="date" value={f.to} max={today()} onChange={(e) => setF({ ...f, to: e.target.value })} /></Field>
      </div>
      {days.data && (
        <div className="row muted small" style={{ marginBottom: 12, gap: 16 }}>
          <span>Present {count("present")}</span>
          <span>Half day {count("half_day")}</span>
          <span>Absent {count("absent") + count("absent_pending")}</span>
          <span>Late {days.data.items.filter((d) => d.late_minutes > 0).length}</span>
        </div>
      )}
      <ErrorBox error={days.error} />
      <Card pad={false}>
        <Table
          rows={days.data?.items}
          empty="No attendance calculated for these dates yet."
          cols={[
            { h: "Date", c: (d) => fmtDate(d.work_date) },
            { h: "Shift", c: (d) => d.shift?.code ?? "—" },
            { h: "In", c: (d) => at(d.first_in) },
            { h: "Out", c: (d) => at(d.last_out) },
            { h: "Worked", c: (d) => mins(d.net_minutes), right: true },
            { h: "Late", c: (d) => (d.late_minutes ? `${d.late_minutes}m` : "—"), right: true },
            { h: "Status", c: (d) => <Pill value={d.status} /> },
            { h: "", right: true, c: (d) => canAsk && d.state !== "locked" && <Button kind="link" onClick={() => setAsk(d.work_date)}>Correct</Button> },
          ]}
        />
      </Card>
      {ask !== null && features.data && <RequestForm date={ask || undefined} features={features.data} onClose={() => setAsk(null)} onDone={() => (days.reload(), features.reload())} />}
    </>
  );
}
