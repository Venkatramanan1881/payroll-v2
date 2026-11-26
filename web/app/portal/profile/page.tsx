"use client";
import { useState } from "react";
import { Button, Card, ErrorBox, Field, fmtDate, Loading, Modal, PageHead, PhoneInput, Pill } from "@/components/ui";
import { api, etagOf } from "@/lib/api";
import { useAccess, useAction, useApi, useCan } from "@/lib/hooks";
import { ChangePin } from "../ChangePin";

type Ref = { name: string; code?: string; short_code?: string } | null;
type Profile = {
  personal: { full_name: string; mobile: string | null; email: string | null; address: string | null; date_of_birth: string | null; gender: string | null; emergency_contact_name: string | null; emergency_contact_phone: string | null };
  employment: { joining_date: string; probation_end_date: string | null };
  status: { value: string; since: string };
  job: { master_branch: Ref; department: Ref; designation: Ref; category: Ref; employee_type: Ref; contractor: Ref; reporting_manager: { full_name?: string; name?: string } | null };
};
const CONTACT = ["mobile", "email", "address", "emergency_contact_name", "emergency_contact_phone"] as const;
type Contact = Record<(typeof CONTACT)[number], string>;

const Row = ({ k, v }: { k: string; v: React.ReactNode }) => (
  <div>
    <div className="muted small">{k}</div>
    <div>{v || "—"}</div>
  </div>
);

// M1 view, M2 contact edit (only when the organisation allows it, EM-03), A4 change PIN.
export default function MyProfile() {
  const access = useAccess();
  const can = useCan();
  const p = useApi<Profile>("/me/profile");
  const act = useAction();
  const [edit, setEdit] = useState<Contact | null>(null);
  const [pin, setPin] = useState(false);
  const d = p.data;

  const openEdit = () => {
    act.setError(null);
    setEdit(Object.fromEntries(CONTACT.map((k) => [k, d!.personal[k] ?? ""])) as Contact);
  };
  const save = () => {
    // Only what changed; blank clears it.
    const changed = Object.fromEntries(CONTACT.filter((k) => (d!.personal[k] ?? "") !== edit![k]).map((k) => [k, edit![k].trim() || null]));
    if (!Object.keys(changed).length) return setEdit(null);
    act.run(() => api("/me/contact", { method: "PATCH", body: changed, version: etagOf("/me/profile") })).then((r) => {
      if (!r) return;
      setEdit(null);
      p.reload();
    });
  };

  if (!d) return <><PageHead title="My profile" /><ErrorBox error={p.error} />{!p.error && <Loading />}</>;
  const j = d.job;
  const name = (r: Ref) => r?.name;
  return (
    <>
      <PageHead title={d.personal.full_name} sub={`${access.org.name} · ${name(j.designation) ?? ""}`}>
        <Button icon="password" onClick={() => setPin(true)}>Change PIN</Button>
      </PageHead>
      <div className="stack">
        <Card title="Contact" actions={can("employee.profile.edit_own") && <Button icon="edit" onClick={openEdit}>Edit</Button>}>
          <div className="grid2">
            <Row k="Mobile" v={d.personal.mobile} />
            <Row k="Email" v={d.personal.email} />
            <Row k="Address" v={d.personal.address} />
            <Row k="Emergency contact" v={[d.personal.emergency_contact_name, d.personal.emergency_contact_phone].filter(Boolean).join(" · ")} />
          </div>
        </Card>
        <Card title="Job">
          <div className="grid2">
            <Row k="Status" v={<><Pill value={d.status.value} /> <span className="muted small">since {fmtDate(d.status.since)}</span></>} />
            <Row k="Joined" v={fmtDate(d.employment.joining_date)} />
            {d.employment.probation_end_date && <Row k="Probation ends" v={fmtDate(d.employment.probation_end_date)} />}
            <Row k="Branch" v={name(j.master_branch)} />
            <Row k="Department" v={name(j.department)} />
            <Row k="Designation" v={name(j.designation)} />
            <Row k="Category" v={name(j.category)} />
            <Row k="Employee type" v={name(j.employee_type)} />
            {j.contractor && <Row k="Contractor" v={name(j.contractor)} />}
            <Row k="Reports to" v={j.reporting_manager?.full_name ?? j.reporting_manager?.name} />
          </div>
        </Card>
        <Card title="Personal">
          <div className="grid2">
            <Row k="Date of birth" v={d.personal.date_of_birth && fmtDate(d.personal.date_of_birth)} />
            <Row k="Gender" v={d.personal.gender?.replace(/_/g, " ")} />
          </div>
          <p className="muted small" style={{ marginBottom: 0 }}>To change your name, date of birth or job details, ask HR.</p>
        </Card>
      </div>

      <Modal open={!!edit} title="Edit contact details" onClose={() => setEdit(null)} footer={<><Button onClick={() => setEdit(null)}>Cancel</Button><Button kind="main" disabled={act.busy} onClick={save}>Save</Button></>}>
        {edit && (
          <div className="grid2">
            <Field label="Mobile" name="personal.mobile" error={act.error}>
              <PhoneInput value={edit.mobile} onChange={(v) => setEdit({ ...edit, mobile: v })} />
            </Field>
            <Field label="Email" name="personal.email" error={act.error}>
              <input type="email" value={edit.email} onChange={(e) => setEdit({ ...edit, email: e.target.value })} />
            </Field>
            <Field label="Address" name="personal.address" error={act.error} wide>
              <textarea rows={2} value={edit.address} onChange={(e) => setEdit({ ...edit, address: e.target.value })} />
            </Field>
            <Field label="Emergency contact name" name="personal.emergency_contact_name" error={act.error}>
              <input value={edit.emergency_contact_name} onChange={(e) => setEdit({ ...edit, emergency_contact_name: e.target.value })} />
            </Field>
            <Field label="Emergency contact phone" name="personal.emergency_contact_phone" error={act.error}>
              <PhoneInput value={edit.emergency_contact_phone} onChange={(v) => setEdit({ ...edit, emergency_contact_phone: v })} />
            </Field>
          </div>
        )}
        <ErrorBox error={act.error} fields={CONTACT.map((k) => `personal.${k}`)} />
      </Modal>

      <Modal open={pin} title="Change PIN" onClose={() => setPin(false)}>
        <ChangePin onDone={() => {}} />
      </Modal>
    </>
  );
}
