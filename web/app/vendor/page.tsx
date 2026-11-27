"use client";
import { useRouter } from "next/navigation";
import { useEffect, useState } from "react";
import { AccountMenu, ThemeToggle } from "@/components/Shell";
import {
  Button,
  Card,
  ErrorBox,
  Field,
  Icon,
  Modal,
  PageHead,
  Pill,
  ReasonDialog,
  Select,
  Skeleton,
  Table,
} from "@/components/ui";
import { api, tokens } from "@/lib/api";
import { useAction, useApi } from "@/lib/hooks";

// Vendor console V1–V4: organisations, their plan and employee limit, suspend/reactivate.
type Plan = { id: string; code: string; name: string };
type Org = {
  id: string;
  short_code: string;
  name: string;
  state: string;
  plan: Plan;
  employee_limit: number;
  limit_behaviour: string;
  employee_count: number;
  created_at: string;
  version: number;
};

const BEHAVIOURS = [
  { value: "block", label: "Block new joiners" },
  { value: "allow_and_alert", label: "Allow and alert" },
];
const blank = {
  short_code: "",
  name: "",
  plan_id: "",
  employee_limit: "100",
  limit_behaviour: "block",
  admin_email: "",
  admin_name: "",
};

export default function Vendor() {
  const router = useRouter();
  useEffect(() => {
    if (!tokens.get()) router.replace("/login");
  }, [router]);
  const orgs = useApi<{ items: Org[] }>("/vendor/organisations");
  const act = useAction();
  const [form, setForm] = useState<typeof blank | null>(null);
  const [edit, setEdit] = useState<Org | null>(null);
  const [flip, setFlip] = useState<Org | null>(null);
  const [made, setMade] = useState<string | null>(null);

  // shortcut: no plans endpoint, so plans come from the organisations already listed. Add GET /vendor/plans if a new plan must be picked before any org uses it.
  const plans = [
    ...new Map(
      (orgs.data?.items ?? []).map((o) => [o.plan.id, o.plan]),
    ).values(),
  ].map((p) => ({ value: p.id, label: `${p.name} (${p.code})` }));
  const set = (k: keyof typeof blank, v: string) =>
    form && setForm({ ...form, [k]: v });

  const create = () =>
    act
      .run(() =>
        api("/vendor/organisations", {
          body: { ...form, employee_limit: Number(form!.employee_limit) },
        }),
      )
      .then((r) => {
        if (!r) return;
        setMade(form!.admin_email);
        setForm(null);
        orgs.reload();
      });
  const save = () =>
    act
      .run(() =>
        api(`/vendor/organisations/${edit!.id}`, {
          method: "PATCH",
          version: edit!.version,
          body: {
            plan_id: edit!.plan.id,
            employee_limit: Number(edit!.employee_limit),
            limit_behaviour: edit!.limit_behaviour,
          },
        }),
      )
      .then((r) => r && (setEdit(null), orgs.reload()));
  const toggle = (reason: string) =>
    act
      .run(() =>
        api(
          `/vendor/organisations/${flip!.id}/${flip!.state === "active" ? "suspend" : "reactivate"}`,
          { version: flip!.version, body: { reason } },
        ),
      )
      .then((r) => r && (setFlip(null), orgs.reload()));
  const list = orgs.data?.items;
  const kpis = [
    { l: "Organisations", v: list?.length, i: "domain" },
    {
      l: "Active",
      v: list?.filter((o) => o.state === "active").length,
      i: "check_circle",
    },
    {
      l: "Suspended",
      v: list?.filter((o) => o.state !== "active").length,
      i: "block",
    },
    {
      l: "Employees",
      v: list?.reduce((n, o) => n + o.employee_count, 0),
      i: "group",
    },
  ];

  return (
    <div className="portal">
      <header className="topbar">
        <span className="logo">PL</span>
        <div className="crumb">
          <span>Vendor console</span>
          <span aria-hidden>/</span>
          <b>Organisations</b>
        </div>
        <ThemeToggle />
        <AccountMenu name="Vendor" sub="Vendor console" />
      </header>
      <main className="content">
        <PageHead
          title="Organisations"
        >
          <Button
            icon="add"
            kind="main"
            onClick={() => {
              act.setError(null);
              setForm({ ...blank, plan_id: plans[0]?.value ?? "" });
            }}
          >
            New organisation
          </Button>
        </PageHead>
        <div className="grid3 kpis4" style={{ marginBottom: 16 }}>
          {kpis.map((k) => (
            <div key={k.l} className="card kpi">
              <div className="kpi-top">
                <span>{k.l}</span>
                <span className="tile">
                  <Icon name={k.i} fill />
                </span>
              </div>
              <div className="v">{list ? k.v : <Skeleton w={64} h={26} />}</div>
            </div>
          ))}
        </div>
        {made && (
          <div className="note" style={{ marginBottom: 16 }}>
            Organisation created. An invitation is queued for {made}.
          </div>
        )}
        <ErrorBox error={orgs.error} />
        <Card pad={false}>
          <Table
            name="organisations"
            rows={list}
            empty="No organisations yet."
            cols={[
              { h: "Code", c: (o) => o.short_code, w: "90px" },
              { h: "Name", c: (o) => o.name },
              { h: "Plan", c: (o) => o.plan.name },
              {
                h: "Employees",
                c: (o) => `${o.employee_count} / ${o.employee_limit}`,
                right: true,
              },
              {
                h: "Over limit",
                c: (o) =>
                  BEHAVIOURS.find((b) => b.value === o.limit_behaviour)?.label,
              },
              { h: "State", c: (o) => <Pill value={o.state} /> },
              {
                h: "",
                right: true,
                c: (o) => (
                  <div className="row" style={{ justifyContent: "flex-end" }}>
                    <Button
                      kind="link"
                      onClick={() => {
                        act.setError(null);
                        setEdit(o);
                      }}
                    >
                      Edit
                    </Button>
                    <Button
                      kind="link"
                      onClick={() => {
                        act.setError(null);
                        setFlip(o);
                      }}
                    >
                      {o.state === "active" ? "Suspend" : "Reactivate"}
                    </Button>
                  </div>
                ),
              },
            ]}
          />
        </Card>

        <Modal
          open={!!form}
          title="New organisation"
          onClose={() => setForm(null)}
          footer={
            <>
              <Button onClick={() => setForm(null)}>Cancel</Button>
              <Button kind="main" disabled={act.busy} onClick={create}>
                Create
              </Button>
            </>
          }
        >
          {form && (
            <div className="grid2">
              <Field
                label="Short code"
                name="short_code"
                error={act.error}
                hint="2–6 capital letters or digits"
              >
                <input
                  value={form.short_code}
                  onChange={(e) =>
                    set("short_code", e.target.value.toUpperCase())
                  }
                  maxLength={6}
                />
              </Field>
              <Field label="Name" name="name" error={act.error}>
                <input
                  value={form.name}
                  onChange={(e) => set("name", e.target.value)}
                />
              </Field>
              <Field label="Plan" name="plan_id" error={act.error}>
                <Select
                  value={form.plan_id}
                  onChange={(v) => set("plan_id", v)}
                  options={plans}
                  placeholder="Pick a plan"
                />
              </Field>
              <Field
                label="Employee limit"
                name="employee_limit"
                error={act.error}
              >
                <input
                  type="number"
                  min={1}
                  value={form.employee_limit}
                  onChange={(e) => set("employee_limit", e.target.value)}
                />
              </Field>
              <Field
                label="When over the limit"
                name="limit_behaviour"
                error={act.error}
              >
                <Select
                  value={form.limit_behaviour}
                  onChange={(v) => set("limit_behaviour", v)}
                  options={BEHAVIOURS}
                />
              </Field>
              <span />
              <Field
                label="First admin's name"
                name="admin_name"
                error={act.error}
              >
                <input
                  value={form.admin_name}
                  onChange={(e) => set("admin_name", e.target.value)}
                />
              </Field>
              <Field
                label="First admin's email"
                name="admin_email"
                error={act.error}
              >
                <input
                  type="email"
                  value={form.admin_email}
                  onChange={(e) => set("admin_email", e.target.value)}
                />
              </Field>
            </div>
          )}
          <ErrorBox error={act.error} fields={Object.keys(blank)} />
        </Modal>

        <Modal
          open={!!edit}
          title={`Edit ${edit?.name ?? ""}`}
          onClose={() => setEdit(null)}
          footer={
            <>
              <Button onClick={() => setEdit(null)}>Cancel</Button>
              <Button kind="main" disabled={act.busy} onClick={save}>
                Save
              </Button>
            </>
          }
        >
          {edit && (
            <div className="grid2">
              <Field label="Plan" name="plan_id" error={act.error}>
                <Select
                  value={edit.plan.id}
                  onChange={(v) =>
                    setEdit({ ...edit, plan: { ...edit.plan, id: v } })
                  }
                  options={plans}
                />
              </Field>
              <Field
                label="Employee limit"
                name="employee_limit"
                error={act.error}
                hint={`${edit.employee_count} employees now`}
              >
                <input
                  type="number"
                  min={1}
                  value={edit.employee_limit}
                  onChange={(e) =>
                    setEdit({
                      ...edit,
                      employee_limit: e.target.value as unknown as number,
                    })
                  }
                />
              </Field>
              <Field
                label="When over the limit"
                name="limit_behaviour"
                error={act.error}
              >
                <Select
                  value={edit.limit_behaviour}
                  onChange={(v) => setEdit({ ...edit, limit_behaviour: v })}
                  options={BEHAVIOURS}
                />
              </Field>
            </div>
          )}
          <ErrorBox
            error={act.error}
            fields={["plan_id", "employee_limit", "limit_behaviour"]}
          />
        </Modal>

        <ReasonDialog
          open={!!flip}
          title={`${flip?.state === "active" ? "Suspend" : "Reactivate"} ${flip?.name ?? ""}`}
          confirm={flip?.state === "active" ? "Suspend" : "Reactivate"}
          onClose={() => setFlip(null)}
          onConfirm={toggle}
          error={act.error}
          busy={act.busy}
        >
          {flip?.state === "active" && (
            <p className="muted">While suspended, its users can only view.</p>
          )}
        </ReasonDialog>
      </main>
    </div>
  );
}
