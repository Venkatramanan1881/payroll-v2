"use client";
import { useState } from "react";
import { Button, Card, ErrorBox, fmtDate, PageHead, Pill, ReasonDialog, Table } from "@/components/ui";
import { api } from "@/lib/api";
import { today, useAction, useApi, useCan } from "@/lib/hooks";

type Holiday = { id: string; date: string; name: string; pick: { id: string; state: string } | null };
type Mine = { year: number; allowed: number; used: number; holidays: Holiday[] };
type Pick = { id: string; state: string; version: number; decision_reason: string | null };

// Optional holidays: pick up to the allowance; withdraw while pending, cancel once approved (H6–H8).
export default function MyHolidays() {
  const can = useCan();
  const [year, setYear] = useState(() => Number(today().slice(0, 4)));
  const mine = useApi<Mine>("/attendance/me/optional-holidays", { year });
  const picks = useApi<{ items: Pick[] }>("/attendance/optional-holiday-picks");
  const act = useAction();
  const [back, setBack] = useState<Holiday | null>(null);
  const d = mine.data;
  const pickOf = (h: Holiday) => picks.data?.items.find((p) => p.id === h.pick?.id);
  const reload = () => (mine.reload(), picks.reload());

  const pick = (h: Holiday) => act.run(() => api("/attendance/optional-holiday-picks", { body: { holiday_id: h.id } })).then((r) => r && reload());
  const takeBack = (reason: string) => {
    const p = pickOf(back!);
    const to = p?.state === "approved" ? "cancel" : "withdraw";
    act.run(() => api(`/attendance/optional-holiday-picks/${back!.pick!.id}/${to}`, { version: p?.version, body: { reason: reason || null } })).then((r) => {
      if (!r) return;
      setBack(null);
      reload();
    });
  };
  const live = (h: Holiday) => h.pick && ["pending", "approved"].includes(h.pick.state);

  return (
    <>
      <PageHead title="Optional holidays" sub={d ? `${d.used} of ${d.allowed} used in ${d.year}` : undefined}>
        <Button icon="chevron_left" onClick={() => setYear(year - 1)} aria-label="Previous year" />
        <span>{year}</span>
        <Button icon="chevron_right" onClick={() => setYear(year + 1)} aria-label="Next year" />
      </PageHead>
      {d && d.allowed === 0 && <div className="note">Your organisation gives no optional holidays this year.</div>}
      <ErrorBox error={mine.error ?? act.error} />
      <Card pad={false}>
        <Table
          rows={d?.holidays}
          empty="No optional holidays on your calendar."
          cols={[
            { h: "Date", c: (h) => fmtDate(h.date) },
            { h: "Holiday", c: (h) => h.name },
            { h: "My pick", c: (h) => (h.pick ? <><Pill value={h.pick.state} />{pickOf(h)?.decision_reason && <div className="muted small">{pickOf(h)!.decision_reason}</div>}</> : <span className="muted">—</span>) },
            {
              h: "",
              right: true,
              c: (h) =>
                live(h) ? (
                  <Button kind="link" onClick={() => { act.setError(null); setBack(h); }}>{h.pick!.state === "approved" ? "Cancel" : "Withdraw"}</Button>
                ) : (
                  can("attendance.optional_holiday.request") && h.date >= today() && d!.used < d!.allowed && <Button kind="link" disabled={act.busy} onClick={() => pick(h)}>Pick</Button>
                ),
            },
          ]}
        />
      </Card>
      <ReasonDialog
        open={!!back}
        title={`${back?.pick?.state === "approved" ? "Cancel" : "Withdraw"} ${back?.name ?? ""}?`}
        required={false}
        confirm={back?.pick?.state === "approved" ? "Cancel pick" : "Withdraw"}
        onClose={() => setBack(null)}
        onConfirm={takeBack}
        error={act.error}
        busy={act.busy}
      />
    </>
  );
}
