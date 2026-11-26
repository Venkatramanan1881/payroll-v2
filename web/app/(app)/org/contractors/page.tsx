"use client";
import { useState } from "react";
import { Button, ErrorBox, fmtDate, Modal, PageHead, Table } from "@/components/ui";
import { useApi } from "@/lib/hooks";
import { Item, ItemsPage } from "../ItemsPage";

type Worker = { id: string; employee_code: string; full_name: string; linked_from: string };

function Workers({ c, onClose }: { c: Item; onClose: () => void }) {
  const w = useApi<{ as_of: string; items: Worker[] }>(`/org/contractors/${c.id}/workers`);
  return (
    <Modal open title={`Workers of ${c.name}`} onClose={onClose}>
      <ErrorBox error={w.error} />
      <Table
        rows={w.data?.items}
        empty="No workers linked today."
        cols={[
          { h: "Code", c: (e) => <a href={`/employees/${e.id}`}>{e.employee_code}</a> },
          { h: "Name", c: (e) => e.full_name },
          { h: "Linked from", c: (e) => fmtDate(e.linked_from) },
        ]}
      />
    </Modal>
  );
}

export default function Contractors() {
  const [workers, setWorkers] = useState<Item | null>(null);
  return (
    <>
      <PageHead title="Contractors" sub="Agencies that supply contract workers." />
      <ItemsPage
        path="/org/contractors"
        noun="contractor"
        manage="org.contractor.manage"
        extra={[
          { key: "contact_name", label: "Contact" },
          { key: "contact_phone", label: "Phone" },
          { key: "contact_email", label: "Email" },
        ]}
        rowActions={(c) => <Button kind="link" onClick={() => setWorkers(c)}>Workers</Button>}
      />
      {workers && <Workers c={workers} onClose={() => setWorkers(null)} />}
    </>
  );
}
