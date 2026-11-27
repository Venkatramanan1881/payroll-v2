"use client";
import Link from "next/link";
import { useState } from "react";
import { Button, Card, ErrorBox, Field, fmtDate, fmtTime, Icon, Loading, mins, PageHead, Pill, Select, TabTools } from "@/components/ui";
import { today, useApi, useCan } from "@/lib/hooks";
import { useBranches } from "@/lib/lookups";
import { addDays } from "../_shared";
import { Day, DayDetail } from "../days/DayDetail";

// Flags the day calculation raises (core/src/day.rs), with what HR usually does about each.
const FLAGS: Record<string, string> = {
  missed_punch: "In or out punch missing",
  flagged_punch: "A punch is waiting in Punch reviews",
  untagged_punch: "Punch without in/out direction",
  session_too_long: "Session longer than the shift allows",
  too_many_punches: "More punches than expected",
  auto_check_out: "Checked out automatically",
  excess_break: "Break longer than allowed",
  break_outside_window: "Break outside the break window",
  held_check_in: "Check-in held for review",
  unapproved_exit: "Left without approval",
  window_overlap: "Shift windows overlap",
};

/** Flagged days (PayLedger "Exceptions"): each one opens the day for a correction, or links to the queue that clears it. */
export default function Exceptions() {
  const can = useCan();
  const branches = useBranches();
  const [f, setF] = useState({ from: addDays(today(), -6), to: today(), branch_id: "", flag: "any" });
  const list = useApi<{ items: Day[]; next_cursor: string | null }>("/attendance/days", f);
  const [open, setOpen] = useState<Day | null>(null);
  const items = list.data?.items;
  return (
    <>
      <PageHead title="Exceptions">
        {can("attendance.punch.review") && (
          <Link className="btn" href="/attendance/reviews">
            <Icon name="rule" /> Punch reviews
          </Link>
        )}
      </PageHead>
      <TabTools>
        <Field label="From"><input type="date" value={f.from} onChange={(e) => setF({ ...f, from: e.target.value })} /></Field>
        <Field label="To"><input type="date" value={f.to} onChange={(e) => setF({ ...f, to: e.target.value })} /></Field>
        <Field label="Branch"><Select value={f.branch_id} onChange={(v) => setF({ ...f, branch_id: v })} options={branches.options} placeholder="Any" /></Field>
        <Field label="Flag">
          <Select value={f.flag} onChange={(v) => setF({ ...f, flag: v || "any" })} options={[{ value: "any", label: "Any flag" }, ...Object.entries(FLAGS).map(([value, label]) => ({ value, label }))]} />
        </Field>
      </TabTools>
      <ErrorBox error={list.error} />
      <Card pad={false}>
        {!items ? (
          <Loading table />
        ) : items.length === 0 ? (
          <div className="empty">
            <span className="tile"><Icon name="task_alt" /></span>
            <p className="muted">No flagged days in this range.</p>
          </div>
        ) : (
          <div className="exc-list">
            {items.map((d) => (
              <div key={d.employee_id + d.work_date} className="exc-row">
                <div>
                  <div className="row">
                    <span className="code">{d.employee.employee_code}</span>
                    <b>{d.employee.full_name}</b>
                    <span className="muted small">{fmtDate(d.work_date)}</span>
                    <Pill value={d.status} />
                    {d.flags.map((x) => <Pill key={x} value={x} />)}
                  </div>
                  <p className="muted small">
                    Shift {d.shift?.code ?? "—"} · {d.first_in ? fmtTime(d.first_in) : "No in"} → {d.last_out ? fmtTime(d.last_out) : "No out"} · {mins(d.net_minutes)} worked
                    {d.flags.map((x) => FLAGS[x]).filter(Boolean).length > 0 && ` · ${d.flags.map((x) => FLAGS[x]).filter(Boolean).join("; ")}`}
                  </p>
                </div>
                <div className="row">
                  {d.flags.includes("flagged_punch") && can("attendance.punch.review") && (
                    <Link className="btn" href="/attendance/reviews">Review punch</Link>
                  )}
                  <Button kind="main" icon="edit" onClick={() => setOpen(d)}>Review &amp; correct</Button>
                </div>
              </div>
            ))}
          </div>
        )}
      </Card>
      {list.data?.next_cursor && <p className="muted small">Showing the first 200. Narrow the dates or branch to see the rest.</p>}
      {open && <DayDetail employee={open.employee} date={open.work_date} onClose={() => setOpen(null)} onChanged={list.reload} />}
    </>
  );
}
