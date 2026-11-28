"use client";
import { useState } from "react";
import { Button, Card, ErrorBox, Loading, PageHead } from "@/components/ui";
import { today, useAccess, useApi, useCan } from "@/lib/hooks";
import { MyShiftRequests } from "../ShiftRequests";

type Entry = { code: string | null; is_weekly_off: boolean; source: string };
type Roster = { entries: Record<string, Entry> };

const addDays = (d: string, n: number) => {
  const x = new Date(d + "T00:00:00");
  x.setDate(x.getDate() + n);
  return x.toLocaleDateString("en-CA");
};
// Monday of the week holding d.
const monday = (d: string) =>
  addDays(d, -((new Date(d + "T00:00:00").getDay() + 6) % 7));

// My shifts, four weeks at a time (M4).
export default function MyRoster() {
  const [from, setFrom] = useState(() => monday(today()));
  const to = addDays(from, 27);
  const r = useApi<Roster>("/attendance/me/roster", { from, to });
  const access = useAccess();
  const can = useCan();
  // SH-12: shift change and swap requests only when the policy allows them (AC-AT-05.3).
  const features = useApi<{ shift_requests: boolean }>(
    "/attendance/me/attendance/features",
  );
  const asks =
    can("attendance.shift_swap.request") && features.data?.shift_requests;
  const days = Array.from({ length: 28 }, (_, i) => addDays(from, i));
  const label = (d: string, o: Intl.DateTimeFormatOptions) =>
    new Date(d + "T00:00:00").toLocaleDateString("en-IN", o);

  return (
    <>
      <PageHead
        title="My roster"
        sub={`${label(from, { day: "numeric", month: "short" })} – ${label(to, { day: "numeric", month: "short", year: "numeric" })}`}
      >
        <Button
          icon="chevron_left"
          onClick={() => setFrom(addDays(from, -28))}
          aria-label="Earlier"
        />
        <Button onClick={() => setFrom(monday(today()))}>This week</Button>
        <Button
          icon="chevron_right"
          onClick={() => setFrom(addDays(from, 28))}
          aria-label="Later"
        />
      </PageHead>
      <ErrorBox error={r.error} />
      <div className="stack">
        {!r.data ? (
          !r.error && <Loading />
        ) : (
          <Card>
            <div className="roster-grid">
              {days.map((d) => {
                const e = r.data!.entries[d];
                return (
                  <div
                    key={d}
                    className={`roster-day${d === today() ? " now" : ""}${e?.is_weekly_off ? " off" : ""}`}
                  >
                    <div className="muted small">
                      {label(d, {
                        weekday: "short",
                        day: "numeric",
                        month: "short",
                      })}
                    </div>
                    <div className="shift">
                      {e ? (e.is_weekly_off ? "Off" : (e.code ?? "—")) : "—"}
                    </div>
                  </div>
                );
              })}
            </div>
          </Card>
        )}
        {asks && <MyShiftRequests me={access.employee_id ?? undefined} />}
      </div>
    </>
  );
}
