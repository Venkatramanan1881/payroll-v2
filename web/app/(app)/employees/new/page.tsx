"use client";
import { useRouter } from "next/navigation";
import { useState } from "react";
import { Button, ErrorBox, Field, PageHead, PhoneInput, Select } from "@/components/ui";
import { api } from "@/lib/api";
import { today, useAction, useApi } from "@/lib/hooks";
import { Job, JOB_REQUIRED, JobFields } from "../parts";

const blank = (o: Record<string, any>) => Object.fromEntries(Object.entries(o).filter(([, v]) => v !== "" && v != null));

// E5. Portal access is granted from the employee page after saving (the server refuses it here).
export default function NewEmployee() {
  const router = useRouter();
  const act = useAction();
  const settings = useApi<{ groups: { EM: { employee_code_mode: string; mandatory_fields?: string[] } } }>("/org/settings");
  const manual = settings.data?.groups.EM.employee_code_mode === "manual";
  // Always required, plus what EM-02 makes mandatory. Marked with * on the form.
  const extra = settings.data?.groups.EM.mandatory_fields ?? [];
  const must = (k: string) => ["full_name", "mobile", "joining_date", "initial_status", ...JOB_REQUIRED, ...extra].includes(k);
  const [p, setP] = useState<Record<string, string>>({});
  const [e, setE] = useState({ employee_code: "", joining_date: today(), probation_end_date: "", initial_status: "active" });
  const [job, setJob] = useState<Job>({});
  const pf = (k: string) => ({ value: p[k] ?? "", onChange: (ev: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => setP({ ...p, [k]: ev.target.value }) });

  const save = () =>
    act
      .run(() =>
        api("/employees", {
          body: {
            ...blank({ ...e, employee_code: manual ? e.employee_code : "" }),
            personal: blank(p),
            job: blank(job),
          },
        }),
      )
      .then((r) => r && router.push(`/employees/${r.id}`));

  const err = act.error;
  return (
    <>
      <PageHead title="New employee">
        <Button onClick={() => router.back()}>Cancel</Button>
        <Button kind="main" icon="person_add" onClick={save} disabled={act.busy}>
          Create employee
        </Button>
      </PageHead>
      <ErrorBox error={err} fields={err?.errors.map((x) => x.field).filter((f) => f.startsWith("personal.") || f.startsWith("job.") || f in e)} />
      <div className="card form-card">
        <Section title="Basic information" desc="Who the employee is. The employee code is given automatically unless your settings say otherwise.">
          {manual && (
            <Field label="Employee code" name="employee_code" error={err} required>
              <input value={e.employee_code} onChange={(x) => setE({ ...e, employee_code: x.target.value })} />
            </Field>
          )}
          <Field label="Full name" name="personal.full_name" error={err} required={must("full_name")}>
            <input {...pf("full_name")} placeholder="As on ID proof" />
          </Field>
          <Field label="Mobile" name="personal.mobile" error={err} required={must("mobile")}>
            <PhoneInput value={p.mobile} onChange={(v) => setP({ ...p, mobile: v })} />
          </Field>
          <Field label="Date of birth" name="personal.date_of_birth" error={err} required={must("date_of_birth")}>
            <input type="date" {...pf("date_of_birth")} />
          </Field>
          <Field label="Gender" name="personal.gender" error={err} required={must("gender")}>
            <Select value={p.gender} onChange={(v) => setP({ ...p, gender: v })} options={["female", "male", "other"].map((g) => ({ value: g, label: g }))} placeholder="Choose…" />
          </Field>
        </Section>
        <Section title="Contact & address" desc="How to reach the employee, and who to call in an emergency.">
          <Field label="Email" name="personal.email" error={err} required={must("email")}>
            <input type="email" {...pf("email")} placeholder="name@example.com" />
          </Field>
          <Field label="Emergency contact name" name="personal.emergency_contact_name" error={err} required={must("emergency_contact_name")}>
            <input {...pf("emergency_contact_name")} />
          </Field>
          <Field label="Emergency contact phone" name="personal.emergency_contact_phone" error={err} required={must("emergency_contact_phone")}>
            <PhoneInput value={p.emergency_contact_phone} onChange={(v) => setP({ ...p, emergency_contact_phone: v })} />
          </Field>
          <Field label="Address" name="personal.address" error={err} wide required={must("address")}>
            <textarea rows={3} {...pf("address")} placeholder="House, street, city, PIN" />
          </Field>
        </Section>
        <Section title="Employment" desc="When the employee starts. Probation uses the default length unless you set an end date.">
          <Field label="Joining date" name="joining_date" error={err} required>
            <input type="date" value={e.joining_date} onChange={(x) => setE({ ...e, joining_date: x.target.value })} />
          </Field>
          <Field label="Initial status" name="initial_status" error={err} required>
            <Select value={e.initial_status} onChange={(v) => setE({ ...e, initial_status: v })} options={[{ value: "active", label: "Active" }, { value: "probation", label: "Probation" }]} />
          </Field>
          {e.initial_status === "probation" && (
            <Field label="Probation end date" name="probation_end_date" error={err} required={must("probation_end_date")}>
              <input type="date" value={e.probation_end_date} onChange={(x) => setE({ ...e, probation_end_date: x.target.value })} />
            </Field>
          )}
        </Section>
        <Section title="Job & organisation" desc="Where the employee works, who they report to and how they are paid.">
          <JobFields value={job} onChange={(k, v) => setJob({ ...job, [k]: v })} error={err} prefix="job." required={[...JOB_REQUIRED, ...extra]} />
        </Section>
        <div className="form-foot">
          <span className="muted small">
            Fields marked <b className="req">*</b> are required.
          </span>
          <div className="row">
            <Button onClick={() => router.back()}>Cancel</Button>
            <Button kind="main" icon="person_add" onClick={save} disabled={act.busy}>
              Create employee
            </Button>
          </div>
        </div>
      </div>
    </>
  );
}

/** One form section: title and help on the left, a 3-column field grid on the right. */
function Section({ title, desc, children }: { title: string; desc: string; children: React.ReactNode }) {
  return (
    <section className="form-sec">
      <div className="form-sec-head">
        <h3>{title}</h3>
        <p className="muted">{desc}</p>
      </div>
      <div className="form-grid">{children}</div>
    </section>
  );
}
