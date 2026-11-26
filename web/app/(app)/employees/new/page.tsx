"use client";
import { useRouter } from "next/navigation";
import { useState } from "react";
import { Button, Card, ErrorBox, Field, PageHead, PhoneInput, Select } from "@/components/ui";
import { api } from "@/lib/api";
import { today, useAction, useApi } from "@/lib/hooks";
import { Job, JobFields } from "../parts";

const blank = (o: Record<string, any>) => Object.fromEntries(Object.entries(o).filter(([, v]) => v !== "" && v != null));

// E5. Portal access is granted from the employee page after saving (the server refuses it here).
export default function NewEmployee() {
  const router = useRouter();
  const act = useAction();
  const settings = useApi<{ groups: { EM: { employee_code_mode: string } } }>("/org/settings");
  const manual = settings.data?.groups.EM.employee_code_mode === "manual";
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
      <PageHead title="New employee" sub="Mandatory fields depend on your organisation's settings">
        <Button onClick={() => router.back()}>Cancel</Button>
        <Button kind="main" onClick={save} disabled={act.busy}>
          Create employee
        </Button>
      </PageHead>
      <ErrorBox error={err} fields={err?.errors.map((x) => x.field).filter((f) => f.startsWith("personal.") || f.startsWith("job.") || f in e)} />
      <div className="stack">
        <Card title="Basic information">
          <div className="grid2">
            {manual && (
              <Field label="Employee code" name="employee_code" error={err}>
                <input value={e.employee_code} onChange={(x) => setE({ ...e, employee_code: x.target.value })} />
              </Field>
            )}
            <Field label="Full name" name="personal.full_name" error={err}>
              <input {...pf("full_name")} />
            </Field>
            <Field label="Mobile" name="personal.mobile" error={err}>
              <PhoneInput value={p.mobile} onChange={(v) => setP({ ...p, mobile: v })} />
            </Field>
            <Field label="Date of birth" name="personal.date_of_birth" error={err}>
              <input type="date" {...pf("date_of_birth")} />
            </Field>
            <Field label="Gender" name="personal.gender" error={err}>
              <Select value={p.gender} onChange={(v) => setP({ ...p, gender: v })} options={["female", "male", "other"].map((g) => ({ value: g, label: g }))} placeholder="—" />
            </Field>
          </div>
        </Card>
        <Card title="Address & contact">
          <div className="grid2">
            <Field label="Email" name="personal.email" error={err}>
              <input type="email" {...pf("email")} />
            </Field>
            <Field label="Emergency contact name" name="personal.emergency_contact_name" error={err}>
              <input {...pf("emergency_contact_name")} />
            </Field>
            <Field label="Emergency contact phone" name="personal.emergency_contact_phone" error={err}>
              <PhoneInput value={p.emergency_contact_phone} onChange={(v) => setP({ ...p, emergency_contact_phone: v })} />
            </Field>
            <Field label="Address" name="personal.address" error={err} wide>
              <textarea rows={2} {...pf("address")} />
            </Field>
          </div>
        </Card>
        <Card title="Organisation & employment">
          <div className="grid2" style={{ marginBottom: 14 }}>
            <Field label="Joining date" name="joining_date" error={err}>
              <input type="date" value={e.joining_date} onChange={(x) => setE({ ...e, joining_date: x.target.value })} />
            </Field>
            <Field label="Initial status" name="initial_status" error={err}>
              <Select value={e.initial_status} onChange={(v) => setE({ ...e, initial_status: v })} options={[{ value: "active", label: "Active" }, { value: "probation", label: "Probation" }]} />
            </Field>
            {e.initial_status === "probation" && (
              <Field label="Probation end date" name="probation_end_date" error={err} hint="Leave empty for the default length">
                <input type="date" value={e.probation_end_date} onChange={(x) => setE({ ...e, probation_end_date: x.target.value })} />
              </Field>
            )}
          </div>
          <JobFields value={job} onChange={(k, v) => setJob({ ...job, [k]: v })} error={err} prefix="job." />
        </Card>
      </div>
    </>
  );
}
