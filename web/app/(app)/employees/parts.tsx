"use client";
import { useState } from "react";
import { Field, MultiSelect, Select } from "@/components/ui";
import { ApiError } from "@/lib/api";
import { useBranches, useContractors, useEmployeeOptions, useList, usePayrollGroups } from "@/lib/lookups";

// core EmployeeStatus (snake_case).
export const STATUSES = ["probation", "active", "notice", "long_leave", "absconding", "exited"];
export const statusOpts = STATUSES.map((s) => ({ value: s, label: s.replace(/_/g, " ") }));

export type Job = Record<string, string | string[] | null | undefined>;

/** Job fields every new joiner needs (backend employees.rs check_joiner); EM-02 can add more. */
export const JOB_REQUIRED = ["master_branch_id", "department_id", "category_id", "employee_type_id", "payroll_group_id"];

/** The ten job fields (Package 1 §2.2). `prefix` is how the server names errors: "job." on create/rejoin, "changes." on job changes. */
export function JobFields({ value, onChange, error, prefix, required = [] }: { value: Job; onChange: (k: string, v: string | string[] | null) => void; error?: ApiError | null; prefix: string; required?: string[] }) {
  const req = (k: string) => required.includes(k);
  const branches = useBranches();
  const depts = useList("departments");
  const desigs = useList("designations");
  const cats = useList("categories");
  const types = useList("employee-types");
  const contractors = useContractors();
  const groups = usePayrollGroups();
  const [q, setQ] = useState("");
  const managers = useEmployeeOptions(q);
  const sel = (k: string, label: string, options: { value: string; label: string }[], optional = false) => (
    <Field label={label} name={prefix + k} error={error} required={req(k)}>
      <Select value={(value[k] as string) ?? ""} onChange={(v) => onChange(k, v || null)} options={options} placeholder={optional && !req(k) ? "— None —" : "Choose…"} />
    </Field>
  );
  return (
    <div className="form-grid">
      {sel("master_branch_id", "Master branch", branches.options)}
      <Field label="Allowed branches" name={prefix + "allowed_branch_ids"} error={error}>
        <MultiSelect options={branches.options} value={(value.allowed_branch_ids as string[]) ?? []} onChange={(v) => onChange("allowed_branch_ids", v)} placeholder="Choose…" />
      </Field>
      {sel("department_id", "Department", depts.options)}
      {sel("designation_id", "Designation", desigs.options, true)}
      {sel("category_id", "Category", cats.options)}
      {sel("employee_type_id", "Employee type", types.options)}
      {sel("contractor_id", "Contractor", contractors.options, true)}
      {sel("payroll_group_id", "Payroll group", groups.options)}
      <Field label="Reporting manager" name={prefix + "reporting_manager_id"} error={error} required={req("reporting_manager_id")}>
        <Select
          value={(value.reporting_manager_id as string) ?? ""}
          onChange={(v) => onChange("reporting_manager_id", v || null)}
          options={managers.options}
          onSearch={setQ}
          placeholder={req("reporting_manager_id") ? "Choose…" : "— None —"}
        />
      </Field>
      <Field label="Enrolment ID" name={prefix + "enrolment_id"} error={error} required={req("enrolment_id")}>
        <input placeholder="Biometric device ID" value={(value.enrolment_id as string) ?? ""} onChange={(e) => onChange("enrolment_id", e.target.value || null)} />
      </Field>
    </div>
  );
}

/** Job doc (as returned by GET /employees/{id}) → job field ids. */
export function jobIds(job: any): Job {
  if (!job) return {};
  return {
    master_branch_id: job.master_branch?.id,
    allowed_branch_ids: (job.allowed_branches ?? []).map((b: any) => b.id),
    department_id: job.department?.id,
    designation_id: job.designation?.id ?? null,
    category_id: job.category?.id,
    employee_type_id: job.employee_type?.id,
    contractor_id: job.contractor?.id ?? null,
    reporting_manager_id: job.reporting_manager?.id ?? null,
    payroll_group_id: job.payroll_group?.id,
    enrolment_id: job.enrolment_id ?? null,
  };
}

export const named = (x: any) => (x ? [x.short_code ?? x.code ?? x.employee_code, x.name ?? x.full_name].filter(Boolean).join(" · ") : "—");
