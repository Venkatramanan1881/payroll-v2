"use client";
import { useState } from "react";
import { Modal, PageHead, Table } from "@/components/ui";
import { ItemsPage } from "../ItemsPage";

type Emp = { id: string; employee_code: string; full_name: string };

export default function Branches() {
  const [left, setLeft] = useState<Emp[] | null>(null);
  return (
    <>
      <PageHead title="Branches" sub="Locations employees belong to. Short codes appear in employee codes." />
      <ItemsPage
        path="/org/branches"
        codeKey="short_code"
        codeLabel="Short code"
        noun="branch"
        manage="org.branch.manage"
        extra={[
          { key: "address", label: "Address" },
          { key: "time_zone", label: "Time zone", hint: "IANA name, e.g. Asia/Kolkata" },
        ]}
        defaults={{ time_zone: "Asia/Kolkata" }}
        onStateResult={(doc) => doc.employees_with_this_branch?.length && setLeft(doc.employees_with_this_branch)}
      />
      <Modal open={!!left} title="Employees still on this branch" onClose={() => setLeft(null)}>
        <p className="muted">The branch is inactive, but these employees still have it as their master branch. Move them with a job change.</p>
        <Table rows={left} cols={[{ h: "Code", c: (e) => e.employee_code }, { h: "Name", c: (e) => e.full_name }]} />
      </Modal>
    </>
  );
}
