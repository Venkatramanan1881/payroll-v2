"use client";
import Link from "next/link";
import { useParams } from "next/navigation";
import { useState } from "react";
import { Code, ErrorBox, Icon, Loading, PageHead, Pill } from "@/components/ui";
import { useApi, useCan } from "@/lib/hooks";
import { AttendanceTab, CustomFieldsTab, HistoryTab, JobTab, OverviewTab, PortalTab, ProbationTab, RejoinTab, StatusTab } from "./tabs";

const initials = (n: string) => n.split(/\s+/).filter(Boolean).map((w) => w[0]).join("").slice(0, 2).toUpperCase();

export default function EmployeePage() {
  const { id } = useParams<{ id: string }>();
  const emp = useApi<any>(`/employees/${id}`);
  const can = useCan();
  const [tab, setTab] = useState("overview");
  if (emp.error) return <ErrorBox error={emp.error} />;
  const e = emp.data;
  if (!e) return <Loading />;
  const c = e.can ?? {};
  const exited = e.status?.value === "exited";
  const tabs = [
    { value: "overview", label: "Overview" },
    c.edit && !exited && { value: "job", label: "Job change" },
    can("employee.history.view") && { value: "history", label: "History" },
    { value: "status", label: "Status" },
    (c.issue_pin || can("attendance.device_binding.reset")) && { value: "portal", label: "Portal & sign-in" },
    e.status?.value === "probation" && can("employee.probation.extend") && { value: "probation", label: "Probation" },
    { value: "custom", label: "Custom fields" },
    (can("attendance.roster.manage", "attendance.policy.manage", "attendance.day.view")) && { value: "attendance", label: "Attendance setup" },
    exited && can("employee.rejoin") && { value: "rejoin", label: "Rejoin" },
  ].filter(Boolean) as { value: string; label: string }[];
  const props = { e, reload: emp.reload };
  return (
    <>
      <PageHead title="Employee profile" sub="Personal, job, status and attendance setup for one person">
        <Link className="btn" href="/employees">
          <Icon name="arrow_back" /> Back to list
        </Link>
      </PageHead>
      <section className="card profile-head">
        <span className="tile avatar">{initials(e.personal.full_name)}</span>
        <div>
          <div className="row">
            <h2>{e.personal.full_name}</h2>
            <Code>{e.employee_code}</Code>
            <Pill value={e.status?.value} />
            {e.pending_status_request && <Pill value="pending" />}
          </div>
          <p className="muted small">{[e.job?.designation?.name, e.job?.department?.name, e.job?.master_branch?.name].filter(Boolean).join(" · ") || "—"}</p>
        </div>
      </section>
      <div className="tabs section-tabs profile-tabs" role="tablist">
        {tabs.map((t) => (
          <button key={t.value} role="tab" aria-selected={t.value === tab} className={t.value === tab ? "on" : ""} onClick={() => setTab(t.value)}>
            {t.label}
          </button>
        ))}
      </div>
      {tab === "overview" && <OverviewTab {...props} />}
      {tab === "job" && <JobTab {...props} />}
      {tab === "history" && <HistoryTab {...props} />}
      {tab === "status" && <StatusTab {...props} />}
      {tab === "portal" && <PortalTab {...props} />}
      {tab === "probation" && <ProbationTab {...props} />}
      {tab === "custom" && <CustomFieldsTab {...props} />}
      {tab === "attendance" && <AttendanceTab {...props} />}
      {tab === "rejoin" && <RejoinTab {...props} />}
    </>
  );
}
