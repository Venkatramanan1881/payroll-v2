"use client";
import { useState } from "react";
import { Button, Card, ErrorBox, Field, fmtDate, Modal, Pill, ReasonDialog, Select, Table, Tabs } from "@/components/ui";
import { api } from "@/lib/api";
import { today, useAction, useApi } from "@/lib/hooks";

type Ref = { id: string; code: string; name: string };
type Person = { id: string; employee_code: string; full_name: string };
export type ShiftRequest = {
  id: string; kind: "change" | "swap"; state: string; work_date: string; reason: string; version: number;
  requester: Person; colleague: Person | null; wanted_shift: Ref; colleague_shift: Ref | null; decision_reason: string | null;
};
type Options = { current: Ref | null; shifts: Ref[]; colleagues: (Person & { shift: Ref })[] };

const tomorrow = () => {
  const d = new Date(today() + "T00:00:00");
  d.setDate(d.getDate() + 1);
  return d.toLocaleDateString("en-CA");
};

/** What the request does, in one line: "MOR → EVE" or "Swap with Ravi: MOR ↔ EVE". */
export const shiftAsk = (r: ShiftRequest) =>
  r.kind === "swap" ? `Swap with ${r.colleague?.full_name}: ${r.colleague_shift?.code} ↔ ${r.wanted_shift.code}` : `Change to ${r.wanted_shift.code}`;

/** My shift change and swap requests (SW1, SW2, SW4), shown under My roster when SH-12 is on. */
export function MyShiftRequests({ me }: { me?: string }) {
  const list = useApi<{ items: ShiftRequest[] }>("/attendance/shift-requests");
  const [asking, setAsking] = useState(false);
  const [back, setBack] = useState<ShiftRequest | null>(null);
  const act = useAction();
  const withdraw = (reason: string) =>
    act.run(() => api(`/attendance/shift-requests/${back!.id}/withdraw`, { version: back!.version, body: { reason: reason || null } })).then((r) => {
      if (!r) return;
      setBack(null);
      list.reload();
    });

  return (
    <Card title="Shift requests" actions={<Button icon="swap_horiz" onClick={() => setAsking(true)}>Ask for a change</Button>} pad={false}>
      <ErrorBox error={list.error} />
      <Table
        search={false}
        rows={list.data?.items}
        empty="No shift requests yet."
        cols={[
          { h: "Date", c: (r) => fmtDate(r.work_date) },
          { h: "Request", c: (r) => (r.requester.id === me ? shiftAsk(r) : `${r.requester.full_name} asks to swap: ${r.wanted_shift.code} ↔ ${r.colleague_shift?.code}`) },
          { h: "Reason", c: (r) => r.reason },
          { h: "State", c: (r) => <><Pill value={r.state} />{r.decision_reason && <div className="muted small">{r.decision_reason}</div>}</> },
          { h: "", right: true, c: (r) => r.state === "pending" && r.requester.id === me && <Button kind="link" onClick={() => { act.setError(null); setBack(r); }}>Withdraw</Button> },
        ]}
      />
      {asking && <AskShift onClose={() => setAsking(false)} onDone={() => (setAsking(false), list.reload())} />}
      <ReasonDialog open={!!back} title="Withdraw this request?" required={false} confirm="Withdraw" onClose={() => setBack(null)} onConfirm={withdraw} error={act.error} busy={act.busy} />
    </Card>
  );
}

function AskShift({ onClose, onDone }: { onClose: () => void; onDone: () => void }) {
  const [date, setDate] = useState(tomorrow());
  const [kind, setKind] = useState<"change" | "swap">("change");
  const [shift, setShift] = useState("");
  const [colleague, setColleague] = useState("");
  const [reason, setReason] = useState("");
  const opts = useApi<Options>(date ? "/attendance/me/shift-options" : null, { date });
  const act = useAction();
  const o = opts.data;
  const pick = o?.colleagues.find((c) => c.id === colleague);
  const save = () =>
    act
      .run(() =>
        api("/attendance/shift-requests", {
          body: { work_date: date, kind, reason, ...(kind === "change" ? { shift_id: shift || null } : { colleague_employee_id: colleague || null }) },
        }),
      )
      .then((r) => r && onDone());

  return (
    <Modal open title="Ask for a shift change" onClose={onClose} footer={<><Button onClick={onClose}>Cancel</Button><Button kind="main" disabled={act.busy} onClick={save}>Send to my manager</Button></>}>
      <div className="stack">
        <Tabs value={kind} onChange={setKind} tabs={[{ value: "change", label: "Different shift" }, { value: "swap", label: "Swap with a colleague" }]} />
        <div className="grid2">
          <Field label="Date" name="work_date" error={act.error} required>
            <input type="date" min={tomorrow()} value={date} onChange={(e) => (setDate(e.target.value), setShift(""), setColleague(""))} />
          </Field>
          <Field label="My shift that day">
            <input readOnly value={o ? (o.current ? `${o.current.code} · ${o.current.name}` : "No shift (weekly off or none)") : ""} />
          </Field>
          {kind === "change" ? (
            <Field label="Shift I want" name="shift_id" error={act.error} required>
              <Select value={shift} onChange={setShift} options={(o?.shifts ?? []).map((s) => ({ value: s.id, label: `${s.code} · ${s.name}` }))} placeholder={o && !o.shifts.length ? "No other shift allowed" : "Choose…"} />
            </Field>
          ) : (
            <Field label="Colleague" name="colleague_employee_id" error={act.error} required>
              <Select
                value={colleague}
                onChange={setColleague}
                options={(o?.colleagues ?? []).map((c) => ({ value: c.id, label: `${c.full_name} (${c.shift.code})` }))}
                placeholder={o && !o.colleagues.length ? "No one to swap with that day" : "Choose…"}
              />
            </Field>
          )}
          {kind === "swap" && pick && o?.current && (
            <Field label="After the swap">
              <input readOnly value={`You ${pick.shift.code}, ${pick.full_name} ${o.current.code}`} />
            </Field>
          )}
          <Field label="Reason" name="reason" error={act.error} required wide>
            <textarea rows={2} value={reason} onChange={(e) => setReason(e.target.value)} placeholder="Why you need the change" />
          </Field>
        </div>
        <p className="muted small">Your manager decides. {kind === "swap" && "Your colleague is told; one approval changes both rosters."}</p>
        <ErrorBox error={opts.error ?? act.error} fields={["work_date", "shift_id", "colleague_employee_id", "reason"]} />
      </div>
    </Modal>
  );
}
