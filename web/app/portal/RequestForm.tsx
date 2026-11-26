"use client";
import { useState } from "react";
import { Button, ErrorBox, Field, Modal, Select } from "@/components/ui";
import { api } from "@/lib/api";
import { today, useAction, useApi } from "@/lib/hooks";

export type Features = {
  regularisation: { enabled: boolean; kinds: string[]; limit: number | null; used: number; window_days: number | null };
  optional_holidays: { allowed: number; used: number };
};

// Settings name the kinds one way, the request route another (KINDS in regularise.rs).
const KIND: Record<string, { value: string; label: string; in?: boolean; out?: boolean }> = {
  missed_punch: { value: "missed_punch", label: "Missed punch", in: true, out: true },
  late_arrival: { value: "late", label: "Late arrival", in: true },
  early_check_out: { value: "early", label: "Left early", out: true },
  wrong_punch: { value: "wrong_punch", label: "Wrong punch" },
  whole_day: { value: "whole_day", label: "Whole day", in: true, out: true },
};
export const kindLabel = (k: string) => Object.values(KIND).find((x) => x.value === k)?.label ?? k.replace(/_/g, " ");

type DayPunches = { explanation?: { punches?: [string, string, string, boolean, string][] } };
const toTime = (v: string) => (v ? `${v}:00` : null);

/** Raise a regularisation for one of my days (US-AT-32). The server checks the window, the monthly limit and the kind. */
export function RequestForm({ date, features, onClose, onDone }: { date?: string; features: Features; onClose: () => void; onDone: () => void }) {
  const kinds = features.regularisation.kinds.filter((k) => KIND[k]).map((k) => KIND[k]);
  const [f, setF] = useState({ work_date: date ?? today(), kind: kinds[0]?.value ?? "missed_punch", in_time: "", out_time: "", punch_id: "", direction: "", reason: "" });
  const kind = Object.values(KIND).find((k) => k.value === f.kind)!;
  const day = useApi<DayPunches>(f.kind === "wrong_punch" && f.work_date ? `/attendance/me/days/${f.work_date}` : null);
  const punches = day.data?.explanation?.punches ?? [];
  const act = useAction();
  const set = (k: keyof typeof f) => (v: string) => setF({ ...f, [k]: v });
  const r = features.regularisation;

  const requested =
    f.kind === "wrong_punch"
      ? { punch_id: f.punch_id || null, direction: f.direction || null }
      : { in_time: kind.in ? toTime(f.in_time) : null, out_time: kind.out ? toTime(f.out_time) : null };
  const save = () =>
    act.run(() => api("/attendance/regularisations", { body: { work_date: f.work_date, kind: f.kind, requested, reason: f.reason } })).then((x) => {
      if (!x) return;
      onDone();
      onClose();
    });

  return (
    <Modal
      open
      title="Request a correction"
      onClose={onClose}
      footer={<><Button onClick={onClose}>Cancel</Button><Button kind="main" disabled={act.busy} onClick={save}>Send request</Button></>}
    >
      <p className="muted small" style={{ marginTop: 0 }}>
        {r.window_days ? `Up to ${r.window_days} days back. ` : ""}
        {r.limit ? `${r.used} of ${r.limit} used this month.` : ""}
      </p>
      <div className="grid2">
        <Field label="Date" name="work_date" error={act.error}>
          <input type="date" max={today()} value={f.work_date} onChange={(e) => set("work_date")(e.target.value)} />
        </Field>
        <Field label="What happened" name="kind" error={act.error}>
          <Select value={f.kind} onChange={set("kind")} options={kinds} />
        </Field>
        {f.kind === "wrong_punch" ? (
          <>
            <Field label="Which punch" name="requested" error={act.error} hint={day.error ? "No attendance calculated for this date yet." : undefined}>
              <Select
                value={f.punch_id}
                onChange={set("punch_id")}
                placeholder={punches.length ? "Pick a punch" : "No punches on this date"}
                options={punches.map(([id, at, dir]) => ({ value: id, label: `${new Date(at).toLocaleTimeString("en-IN", { hour: "2-digit", minute: "2-digit" })} · ${dir.toLowerCase()}` }))}
              />
            </Field>
            <Field label="It should be">
              <Select value={f.direction} onChange={set("direction")} options={[{ value: "in", label: "An in punch" }, { value: "out", label: "An out punch" }]} placeholder="Not counted at all" />
            </Field>
          </>
        ) : (
          <>
            {kind.in && (
              <Field label={f.kind === "late" ? "Actual in time" : "In time"} name="requested" error={act.error}>
                <input type="time" value={f.in_time} onChange={(e) => set("in_time")(e.target.value)} />
              </Field>
            )}
            {kind.out && (
              <Field label={f.kind === "early" ? "Actual out time" : "Out time"} name={kind.in ? undefined : "requested"} error={act.error}>
                <input type="time" value={f.out_time} onChange={(e) => set("out_time")(e.target.value)} />
              </Field>
            )}
          </>
        )}
        <Field label="Reason" name="reason" error={act.error} wide>
          <textarea rows={3} value={f.reason} onChange={(e) => set("reason")(e.target.value)} />
        </Field>
      </div>
      <ErrorBox error={act.error} fields={["work_date", "kind", "requested", "reason"]} />
    </Modal>
  );
}
