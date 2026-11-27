"use client";
import Link from "next/link";
import { Card, ErrorBox, Icon, PageHead, Spinner } from "@/components/ui";
import { useApi, useCan } from "@/lib/hooks";

type Named = { id: string | null; name: string | null; count: number };
type Summary = { as_of: string; total: number; by_status: Record<string, number>; by_department: Named[]; by_branch: Named[] };

const bars = (xs: Named[], none: string) => xs.map((x) => ({ label: x.name ?? none, value: x.count }));

function Bars({ title, items }: { title: string; items: { label: string; value: number }[] }) {
  const max = Math.max(1, ...items.map((i) => i.value));
  return (
    <Card title={title}>
      {items.length ? (
        <div className="bars">
          {items.map((i) => (
            <div key={i.label}>
              <div className="spread small">
                <span className="muted">{i.label}</span>
                <b>{i.value}</b>
              </div>
              <div className="bar"><span style={{ width: `${Math.max(4, (i.value / max) * 100)}%` }} /></div>
            </div>
          ))}
        </div>
      ) : (
        <p className="muted small">No employees yet.</p>
      )}
    </Card>
  );
}

export default function EmployeeDashboard() {
  const can = useCan();
  const { data: d, error } = useApi<Summary>("/employees/summary");
  const n = (...s: string[]) => s.reduce((t, k) => t + (d?.by_status[k] ?? 0), 0);
  const kpis = [
    { l: "Total directory", v: d?.total ?? 0, i: "group" },
    { l: "Active", v: n("active"), i: "person_check" },
    { l: "Probation / notice", v: n("probation", "notice"), i: "schedule" },
    { l: "Exited / away", v: n("exited", "absconding", "long_leave"), i: "person_off" },
  ];
  return (
    <>
      <PageHead title="Employee dashboard">
        {can("employee.create") && (
          <Link className="btn main" href="/employees/new">
            <Icon name="person_add" /> Add employee
          </Link>
        )}
      </PageHead>
      <ErrorBox error={error} />
      <div className="stack">
        <div className="grid3 kpis4">
          {kpis.map((k) => (
            <div key={k.l} className="card kpi">
              <div className="kpi-top">
                <span>{k.l}</span>
                <span className="tile"><Icon name={k.i} fill /></span>
              </div>
              <div className="v">{d ? k.v : <Spinner small />}</div>
            </div>
          ))}
        </div>
        {d && (
          <div className="halves">
            <Bars title="Headcount by department" items={bars(d.by_department, "No department")} />
            <Bars title="Headcount by branch" items={bars(d.by_branch, "No branch")} />
          </div>
        )}
      </div>
    </>
  );
}
