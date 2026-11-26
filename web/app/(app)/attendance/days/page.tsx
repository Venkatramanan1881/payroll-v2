"use client";
import { useState } from "react";
import { Button, Card, ErrorBox, Field, fmtDate, fmtTime, mins, PageHead, Pill, Select, Table } from "@/components/ui";
import { today, useApi } from "@/lib/hooks";
import { useBranches, useList } from "@/lib/lookups";
import { addDays, DAY_STATUSES } from "../_shared";
import { Day, DayDetail } from "./DayDetail";

const opts = (xs: string[]) => xs.map((x) => ({ value: x, label: x.replace(/_/g, " ") }));

export default function Days() {
  const branches = useBranches();
  const departments = useList("departments");
  const [f, setF] = useState({ from: addDays(today(), -6), to: today(), branch_id: "", department_id: "", status: "", flag: "", late: "", state: "" });
  const [cursor, setCursor] = useState<string | undefined>();
  const list = useApi<{ items: Day[]; next_cursor: string | null }>("/attendance/days", { ...f, cursor });
  const [open, setOpen] = useState<Day | null>(null);
  const set = (k: string) => (v: string) => {
    setF({ ...f, [k]: v });
    setCursor(undefined);
  };
  return (
    <>
      <PageHead title="Daily attendance" sub="Calculated by the server from punches, shifts and policy. At most 62 days per view." />
      <div className="filters">
        <Field label="From"><input type="date" value={f.from} onChange={(e) => set("from")(e.target.value)} /></Field>
        <Field label="To"><input type="date" value={f.to} onChange={(e) => set("to")(e.target.value)} /></Field>
        <Field label="Branch"><Select value={f.branch_id} onChange={set("branch_id")} options={branches.options} placeholder="Any" /></Field>
        <Field label="Department"><Select value={f.department_id} onChange={set("department_id")} options={departments.options} placeholder="Any" /></Field>
        <Field label="Status"><Select value={f.status} onChange={set("status")} options={opts(DAY_STATUSES)} placeholder="Any" /></Field>
        <Field label="Flag"><input value={f.flag} placeholder="e.g. missed_punch" onChange={(e) => set("flag")(e.target.value)} /></Field>
        <Field label="Late"><Select value={f.late} onChange={set("late")} options={[{ value: "true", label: "Late only" }, { value: "false", label: "Not late" }]} placeholder="Any" /></Field>
        <Field label="State"><Select value={f.state} onChange={set("state")} options={opts(["provisional", "finalised"])} placeholder="Any" /></Field>
      </div>
      <ErrorBox error={list.error} />
      <Card pad={false}>
        <Table
          rows={list.data?.items}
          onRow={setOpen}
          empty="No calculated days in this range."
          cols={[
            { h: "Date", c: (d) => fmtDate(d.work_date) },
            { h: "Employee", c: (d) => <><span className="code">{d.employee.employee_code}</span> {d.employee.full_name}</> },
            { h: "Branch", c: (d) => d.branch?.short_code },
            { h: "Shift", c: (d) => d.shift?.code ?? "—" },
            { h: "Status", c: (d) => <Pill value={d.status} /> },
            { h: "In", c: (d) => fmtTime(d.first_in) },
            { h: "Out", c: (d) => fmtTime(d.last_out) },
            { h: "Net", c: (d) => mins(d.net_minutes), right: true },
            { h: "Late", c: (d) => (d.late_minutes ? `${d.late_minutes}m` : "—"), right: true },
            { h: "Flags", c: (d) => d.flags.map((x) => <Pill key={x} value={x} />) },
            { h: "State", c: (d) => <Pill value={d.state} /> },
          ]}
        />
      </Card>
      <div className="row" style={{ marginTop: 12 }}>
        {cursor && <Button onClick={() => setCursor(undefined)}>First page</Button>}
        {list.data?.next_cursor && <Button onClick={() => setCursor(list.data!.next_cursor!)}>Next 200</Button>}
      </div>
      {open && <DayDetail employee={open.employee} date={open.work_date} onClose={() => setOpen(null)} onChanged={list.reload} />}
    </>
  );
}
