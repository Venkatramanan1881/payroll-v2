"use client";
import { useState } from "react";
import { Button, ErrorBox, Modal, PageHead, Select, TabTools, Tabs } from "@/components/ui";
import { api } from "@/lib/api";
import { useAction, useCan } from "@/lib/hooks";
import type { ListName } from "@/lib/lookups";
import { ItemsPage } from "../ItemsPage";

const LISTS: { value: ListName; label: string; noun: string }[] = [
  { value: "departments", label: "Departments", noun: "department" },
  { value: "designations", label: "Designations", noun: "designation" },
  { value: "categories", label: "Categories", noun: "category" },
  { value: "employee-types", label: "Employee types", noun: "employee type" },
  { value: "exit-reasons", label: "Exit reasons", noun: "exit reason" },
];
const SECTORS = [
  { value: "factory", label: "Factory" },
  { value: "supermarket", label: "Supermarket" },
  { value: "restaurant_bakery", label: "Restaurant / bakery" },
  { value: "office_services", label: "Office / services" },
];

export default function Lists() {
  const can = useCan();
  const [tab, setTab] = useState<ListName>("departments");
  const [tpl, setTpl] = useState(false);
  const [sector, setSector] = useState("factory");
  const [done, setDone] = useState<Record<string, number> | null>(null);
  const [round, setRound] = useState(0); // bumps to reload the open list after a template
  const act = useAction();
  const l = LISTS.find((x) => x.value === tab)!;
  const apply = () =>
    act.run(() => api("/org/lists/apply-template", { body: { sector } })).then((r) => {
      if (!r) return;
      setDone(r.added);
      setRound((n) => n + 1);
    });
  return (
    <>
      <PageHead title="Departments & lists">
        {can("org.lists.manage") && (
          <Button icon="auto_awesome" onClick={() => { act.setError(null); setDone(null); setTpl(true); }}>
            Apply sector template
          </Button>
        )}
      </PageHead>
      <TabTools className="filters list-tabs">
        <Tabs value={tab} onChange={setTab} tabs={LISTS} />
      </TabTools>
      <ItemsPage key={tab + round} path={`/org/lists/${tab}`} noun={l.noun} manage="org.lists.manage" />
      <Modal
        open={tpl}
        title="Apply sector template"
        onClose={() => setTpl(false)}
        footer={
          <>
            <Button onClick={() => setTpl(false)}>Close</Button>
            {!done && <Button kind="main" disabled={act.busy} onClick={apply}>Apply</Button>}
          </>
        }
      >
        <p className="muted">Fills departments, designations, categories and employee types with a starter set. Works only while all four are empty.</p>
        <Select value={sector} onChange={setSector} options={SECTORS} />
        {done && <div className="note">Added: {Object.entries(done).map(([k, n]) => `${n} ${k.replace(/_/g, " ")}`).join(", ")}</div>}
        <ErrorBox error={act.error} />
      </Modal>
    </>
  );
}
