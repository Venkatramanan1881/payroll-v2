"use client";
import { useEffect, useState } from "react";
import { Button, Card, ErrorBox, Loading, PageHead } from "@/components/ui";
import { api } from "@/lib/api";
import { useAction, useApi } from "@/lib/hooks";

type Row = { event: string; channel: string; recipient: string; enabled: boolean };

// shortcut: the events the server sends today (notify.rs); extend when more events go out.
const EVENTS = ["MissedPunch", "RegularisationSubmitted", "ApprovalsPending", "ShiftRequestSubmitted", "ShiftSwapProposed"];
const RECIPIENTS = ["employee", "manager", "hr_manager", "org_admin"];
const CHANNELS = ["in_app", "email"];
const key = (r: Omit<Row, "enabled">) => `${r.event}|${r.channel}|${r.recipient}`;

export default function NotificationSettings() {
  const data = useApi<{ rows: Row[] }>("/attendance/notification-settings");
  const act = useAction();
  const [on, setOn] = useState<Record<string, boolean>>({});
  const [saved, setSaved] = useState(false);
  useEffect(() => {
    if (data.data) setOn(Object.fromEntries(data.data.rows.map((r) => [key(r), r.enabled])));
  }, [data.data]);
  const save = () => {
    const rows = EVENTS.flatMap((event) => CHANNELS.flatMap((channel) => RECIPIENTS.map((recipient) => ({ event, channel, recipient, enabled: on[key({ event, channel, recipient })] ?? true }))));
    act.run(() => api<{ rows: Row[] }>("/attendance/notification-settings", { method: "PUT", body: { rows } })).then((r) => {
      if (r) {
        data.setData(r);
        setSaved(true);
      }
    });
  };
  return (
    <>
      <PageHead title="Notification settings">
        <Button kind="main" disabled={act.busy || !data.data} onClick={save}>Save</Button>
      </PageHead>
      <ErrorBox error={data.error ?? act.error} />
      {saved && <div className="note">Saved.</div>}
      {!data.data ? (
        <Loading />
      ) : (
        <Card pad={false}>
          <div className="table-wrap">
            <table className="table">
              <thead>
                <tr>
                  <th>Event</th>
                  <th>Recipient</th>
                  {CHANNELS.map((c) => <th key={c} style={{ textAlign: "center" }}>{c.replace("_", "-")}</th>)}
                </tr>
              </thead>
              <tbody>
                {EVENTS.flatMap((event) =>
                  RECIPIENTS.map((recipient, i) => (
                    <tr key={event + recipient}>
                      <td>{i === 0 ? <strong>{event.replace(/([a-z])([A-Z])/g, "$1 $2")}</strong> : ""}</td>
                      <td>{recipient.replace(/_/g, " ")}</td>
                      {CHANNELS.map((channel) => {
                        const k = key({ event, channel, recipient });
                        return (
                          <td key={channel} style={{ textAlign: "center" }}>
                            <input type="checkbox" aria-label={`${event} ${channel} ${recipient}`} checked={on[k] ?? true} onChange={(e) => { setOn({ ...on, [k]: e.target.checked }); setSaved(false); }} />
                          </td>
                        );
                      })}
                    </tr>
                  )),
                )}
              </tbody>
            </table>
          </div>
        </Card>
      )}
    </>
  );
}
