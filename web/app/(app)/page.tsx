"use client";
import Link from "next/link";
import { Card, ErrorBox, Icon, PageHead, Spinner, Table } from "@/components/ui";
import { useAccess, useApi, useCan } from "@/lib/hooks";

type Row = { branch: { id: string; short_code: string; name: string } | null; present: number; absent: number; late: number; on_leave: number; missed_punches: number };
type Dash = { date: string; branches: Row[]; pending_approvals: { punch_reviews: number; regularisations: number }; devices_offline: number };

export default function Dashboard() {
  const access = useAccess();
  const can = useCan();
  const dash = useApi<Dash>(can("dashboard.attendance") ? "/attendance/dashboard/attendance" : null);
  const d = dash.data;
  const sum = (k: keyof Row) => (d?.branches ?? []).reduce((n, r) => n + (Number(r[k]) || 0), 0);
  const kpis = [
    { l: "Present today", v: sum("present"), i: "how_to_reg" },
    { l: "Absent", v: sum("absent"), i: "person_off" },
    { l: "Late marks", v: sum("late"), i: "schedule" },
    { l: "Missed punches", v: sum("missed_punches"), i: "running_with_errors" },
    { l: "Punch reviews", v: d?.pending_approvals.punch_reviews ?? 0, i: "rule", href: "/attendance/reviews" },
    { l: "Regularisations", v: d?.pending_approvals.regularisations ?? 0, i: "assignment_turned_in", href: "/attendance/regularisations" },
    { l: "Devices offline", v: d?.devices_offline ?? 0, i: "wifi_off" },
  ];
  return (
    <>
      <PageHead title="Overview" sub={`${access.org.name} · ${d?.date ?? "today"}`}>
        {can("employee.create") && (
          <Link className="btn main" href="/employees/new">
            <Icon name="person_add" /> Add employee
          </Link>
        )}
      </PageHead>
      <ErrorBox error={dash.error} />
      {can("dashboard.attendance") ? (
        <div className="stack">
          <div className="grid3">
            {kpis.map((k) => {
              const body = (
                <div className="card kpi">
                  <div className="kpi-top">
                    <span>{k.l}</span>
                    <span className="tile"><Icon name={k.i} fill /></span>
                  </div>
                  <div className="v">{dash.loading ? <Spinner small /> : k.v}</div>
                </div>
              );
              return k.href ? (
                <Link key={k.l} href={k.href} style={{ color: "inherit", textDecoration: "none" }}>
                  {body}
                </Link>
              ) : (
                <div key={k.l}>{body}</div>
              );
            })}
          </div>
          <Card title="Today by branch" pad={false}>
            <Table
              rows={d?.branches}
              empty="No attendance calculated for today yet."
              cols={[
                { h: "Branch", c: (r) => (r.branch ? `${r.branch.short_code} · ${r.branch.name}` : "—") },
                { h: "Present", c: (r) => r.present, right: true },
                { h: "Absent", c: (r) => r.absent, right: true },
                { h: "Late", c: (r) => r.late, right: true },
                { h: "Missed punches", c: (r) => r.missed_punches, right: true },
              ]}
            />
          </Card>
        </div>
      ) : (
        <Card>
          <p className="muted">Pick a section from the left rail to get started.</p>
        </Card>
      )}
    </>
  );
}
