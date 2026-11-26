"use client";
import Link from "next/link";
import { useEffect, useState } from "react";
import { Button, Card, ErrorBox, fmtDate, Loading, PageHead, Pill, Table } from "@/components/ui";
import { api, ApiError } from "@/lib/api";
import { useAccess, useAction, useApi, useCan } from "@/lib/hooks";

type Today = {
  date: string;
  next_direction: "in" | "out";
  shift: { code: string; name: string; start: string; end: string; is_weekly_off: boolean } | null;
  punches: { id?: string; punched_at: string; direction: string; source?: string; status?: string; flags?: string[] }[];
  features: { punch: { web: boolean; web_location_required?: boolean; location_required?: boolean; selfie_required?: boolean } };
};

const hm = (t?: string | null) => (t ? t.slice(0, 5) : "—");
const at = (d: string) => new Date(d).toLocaleTimeString("en-IN", { hour: "2-digit", minute: "2-digit" });

/** Browser location, when the organisation asks for it. */
function locate(): Promise<{ latitude: number; longitude: number; accuracy_m: number }> {
  return new Promise((ok, fail) => {
    const no = (detail: string) => fail(new ApiError(0, "location_unavailable", "Location needed", detail));
    if (!navigator.geolocation) return no("This browser can't share your location.");
    navigator.geolocation.getCurrentPosition(
      (p) => ok({ latitude: p.coords.latitude, longitude: p.coords.longitude, accuracy_m: p.coords.accuracy }),
      (e) => no(e.code === e.PERMISSION_DENIED ? "Allow location for this site to punch." : "Couldn't read your location. Try again."),
      { enableHighAccuracy: true, timeout: 15000, maximumAge: 0 },
    );
  });
}

export default function Today() {
  const access = useAccess();
  const can = useCan();
  const t = useApi<Today>("/attendance/me/punches/today");
  const act = useAction();
  const [now, setNow] = useState(() => new Date());
  const [msg, setMsg] = useState<string | null>(null);
  useEffect(() => {
    const id = setInterval(() => setNow(new Date()), 1000);
    return () => clearInterval(id);
  }, []);

  const d = t.data;
  const p = d?.features.punch;
  const needsLocation = !!(p?.web_location_required ?? p?.location_required);
  const punch = () => {
    setMsg(null);
    act
      .run(async () => {
        const location = needsLocation ? await locate() : undefined;
        return api<{ flags?: string[]; review_opened?: boolean }>("/attendance/me/punches", {
          body: { direction: d!.next_direction, client_key: crypto.randomUUID(), ...(location && { location }) },
        });
      })
      .then((r) => {
        if (!r) return;
        setMsg(r.review_opened ? "Punch saved. It needs a review by HR before it counts." : `Punched ${d!.next_direction}.`);
        t.reload();
      });
  };

  return (
    <>
      <PageHead title={`Hello, ${access.user.name.split(" ")[0]}`} sub={`${access.org.name} · ${fmtDate(d?.date ?? null)}`} />
      <ErrorBox error={t.error} />
      {!d ? (
        !t.error && <Loading />
      ) : (
        <div className="grid2" style={{ gridTemplateColumns: "repeat(auto-fit, minmax(300px, 1fr))" }}>
          <Card title="Punch">
            <div className="punch">
              <div className="clock">{now.toLocaleTimeString("en-IN", { hour: "2-digit", minute: "2-digit", second: "2-digit" })}</div>
              <div className="muted">
                {d.shift ? (d.shift.is_weekly_off ? "Weekly off today" : `${d.shift.name} · ${hm(d.shift.start)}–${hm(d.shift.end)}`) : "No shift today"}
              </div>
              {can("attendance.punch.self") && p?.web ? (
                <>
                  <Button kind="main" icon={d.next_direction === "in" ? "login" : "logout"} onClick={punch} disabled={act.busy}>
                    {act.busy ? "Punching…" : `Punch ${d.next_direction}`}
                  </Button>
                  {needsLocation && <small className="muted">Your location is checked against your branch.</small>}
                </>
              ) : (
                <div className="note">Punching from the web is switched off. Use the mobile app or the biometric device.</div>
              )}
              {msg && <div className="note">{msg}</div>}
              <ErrorBox error={act.error} />
            </div>
          </Card>
          <Card title="Today's punches" pad={false}>
            <Table
              rows={d.punches}
              empty="No punches yet today."
              cols={[
                { h: "Time", c: (r) => at(r.punched_at) },
                { h: "Direction", c: (r) => r.direction },
                { h: "From", c: (r) => r.source ?? "—" },
                { h: "", c: (r) => (r.status ? <Pill value={r.status} /> : null), right: true },
              ]}
            />
          </Card>
        </div>
      )}
      <div className="row" style={{ marginTop: 16 }}>
        <Link className="btn ghost" href="/portal/attendance">My attendance</Link>
        <Link className="btn ghost" href="/portal/requests">Request a correction</Link>
      </div>
    </>
  );
}
