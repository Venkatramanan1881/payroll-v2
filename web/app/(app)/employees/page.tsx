"use client";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useEffect, useState } from "react";
import { Button, Card, Code, ErrorBox, Icon, PageHead, Pill, Select, Table, TabTools } from "@/components/ui";
import { api, ApiError, cache } from "@/lib/api";
import { useCan } from "@/lib/hooks";
import { useBranches, useList } from "@/lib/lookups";
import { named, statusOpts } from "./parts";

type Row = { id: string; employee_code: string; full_name: string; mobile: string; status: string; master_branch: any; department: any; designation: any; payroll_group: any; portal_access: string };

export default function Employees() {
  const router = useRouter();
  const can = useCan();
  const branches = useBranches();
  const depts = useList("departments");
  const [f, setF] = useState({ q: "", branch_id: "", department_id: "", status: "" });
  // First page per filter is cached, so coming back shows the list at once while it refreshes.
  const first = (q: typeof f) => cache.get("employees:" + JSON.stringify(q)) as { items: Row[]; next_cursor: string | null } | undefined;
  const [rows, setRows] = useState<Row[] | null>(() => first(f)?.items ?? null);
  const [next, setNext] = useState<string | null>(() => first(f)?.next_cursor ?? null);
  const [error, setError] = useState<ApiError | null>(null);

  const load = async (cursor?: string) => {
    setError(null);
    try {
      const r = await api<{ items: Row[]; next_cursor: string | null }>("/employees", { query: { ...f, cursor } });
      if (!cursor) cache.set("employees:" + JSON.stringify(f), r);
      setRows((old) => (cursor ? [...(old ?? []), ...r.items] : r.items));
      setNext(r.next_cursor);
    } catch (e) {
      setError(e as ApiError);
      setRows([]);
    }
  };
  useEffect(() => {
    const t = setTimeout(() => load(), 250); // debounce typing in search
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [f]);

  return (
    <>
      <PageHead title="Employee master">
        {can("employee.create") && (
          <Link className="btn main" href="/employees/new">
            <Icon name="person_add" /> Add employee
          </Link>
        )}
      </PageHead>
      <TabTools>
        <div className="search with-icon">
          <Icon name="search" />
          <input aria-label="Search" placeholder="Search name, code or mobile" value={f.q} onChange={(e) => setF({ ...f, q: e.target.value })} />
        </div>
        <Select value={f.branch_id} onChange={(v) => setF({ ...f, branch_id: v })} options={branches.options} placeholder="All branches" />
        <Select value={f.department_id} onChange={(v) => setF({ ...f, department_id: v })} options={depts.options} placeholder="All departments" />
        <Select value={f.status} onChange={(v) => setF({ ...f, status: v })} options={statusOpts} placeholder="Any status" />
      </TabTools>
      <ErrorBox error={error} />
      <Card pad={false}>
        <Table
          search={false}
          rows={rows}
          onRow={(r) => router.push(`/employees/${r.id}`)}
          empty="No employees match."
          cols={[
            { h: "Code", c: (r) => <Code>{r.employee_code}</Code> },
            { h: "Name", c: (r) => r.full_name },
            { h: "Mobile", c: (r) => r.mobile },
            { h: "Branch", c: (r) => named(r.master_branch) },
            { h: "Department", c: (r) => r.department?.name ?? "—" },
            { h: "Designation", c: (r) => r.designation?.name ?? "—" },
            { h: "Status", c: (r) => <Pill value={r.status} /> },
            { h: "Portal", c: (r) => <Pill value={r.portal_access} /> },
          ]}
        />
      </Card>
      {next && (
        <div className="row" style={{ justifyContent: "center", marginTop: 14 }}>
          <Button onClick={() => load(next)}>Load more</Button>
        </div>
      )}
    </>
  );
}
