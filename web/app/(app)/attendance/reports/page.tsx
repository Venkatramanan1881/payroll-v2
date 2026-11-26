"use client";
import { useState } from "react";
import { Card, ErrorBox, Field, PageHead, Select, Table } from "@/components/ui";
import { today, useApi } from "@/lib/hooks";
import { useBranches } from "@/lib/lookups";
import { addDays } from "../_shared";

const REPORTS = [
  { value: "r1", label: "R1 · Daily attendance" },
  { value: "r2", label: "R2 · Monthly register" },
  { value: "r3", label: "R3 · Late and early" },
  { value: "r4", label: "R4 · Missed punches & regularisations" },
  { value: "r5", label: "R5 · Exceptions" },
  { value: "r23", label: "R23 · Adjustments" },
  { value: "r26", label: "R26 · Settings history" },
];

const cell = (v: unknown) => (v == null ? "—" : typeof v === "object" ? JSON.stringify(v) : String(v));

export default function Reports() {
  const branches = useBranches();
  const [code, setCode] = useState("r1");
  const [from, setFrom] = useState(addDays(today(), -6));
  const [to, setTo] = useState(today());
  const [branch, setBranch] = useState("");
  const rep = useApi<{ columns: string[]; rows: Record<string, unknown>[] }>(`/attendance/reports/${code}`, { from, to, branch_id: branch });
  return (
    <>
      <PageHead title="Attendance reports" sub="At most 62 days per run." />
      <div className="filters">
        <Field label="Report"><Select value={code} onChange={setCode} options={REPORTS} /></Field>
        <Field label="From"><input type="date" value={from} onChange={(e) => setFrom(e.target.value)} /></Field>
        <Field label="To"><input type="date" value={to} onChange={(e) => setTo(e.target.value)} /></Field>
        <Field label="Branch"><Select value={branch} onChange={setBranch} options={branches.options} placeholder="All in scope" /></Field>
      </div>
      <ErrorBox error={rep.error} />
      <Card pad={false}>
        <Table
          rows={rep.error ? [] : rep.data?.rows}
          empty="No rows."
          cols={(rep.data?.columns ?? []).map((c) => ({ h: c.replace(/_/g, " "), c: (r: Record<string, unknown>) => cell(r[c]) }))}
        />
      </Card>
    </>
  );
}
