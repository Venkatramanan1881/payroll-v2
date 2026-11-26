"use client";
import { useRef, useState } from "react";
import { Button, Card, ErrorBox, fmtTime, PageHead, Pill, Table, Tabs } from "@/components/ui";
import { api, download } from "@/lib/api";
import { useAction, useApi, useCan } from "@/lib/hooks";

type Import = { id: string; state: string; total_rows: number | null; applied_count: number | null; error_count: number | null; skipped_count: number; failure_reason: string | null; created_at: string; completed_at: string | null };

export default function AttendanceImports() {
  const can = useCan();
  const tabs = [
    ...(can("attendance.punch.import") ? [{ value: "punch" as const, label: "Punch imports" }] : []),
    ...(can("attendance.roster.manage") ? [{ value: "roster" as const, label: "Roster imports" }] : []),
  ];
  const [tab, setTab] = useState<"punch" | "roster">(tabs[0]?.value ?? "punch");
  return (
    <>
      <PageHead title="Punch & roster imports" sub="Files are processed in the background; a row already applied is skipped if the same file comes again.">
        <Tabs value={tab} onChange={setTab} tabs={tabs} />
      </PageHead>
      <ImportPanel key={tab} kind={tab} />
    </>
  );
}

function ImportPanel({ kind }: { kind: "punch" | "roster" }) {
  const base = `/attendance/${kind}-imports`;
  const list = useApi<{ items: Import[] }>(base);
  const act = useAction();
  const file = useRef<HTMLInputElement>(null);
  const upload = () => {
    const f = file.current?.files?.[0];
    if (!f) return;
    const body = new FormData();
    body.append("file", f);
    act.run(() => api(base, { body })).then((r) => {
      if (r) {
        if (file.current) file.current.value = "";
        list.reload();
      }
    });
  };
  const errors = (id: string) => act.run(() => api<{ url: string }>(`${base}/${id}/errors`)).then((r) => r && window.open(r.url, "_blank"));
  return (
    <div className="stack">
      <Card
        title={kind === "punch" ? "Upload punches (CSV)" : "Upload roster (.xlsx or .csv)"}
        actions={kind === "roster" && <Button icon="download" onClick={() => act.run(() => download("/attendance/roster-imports/template", "roster-import-template.xlsx"))}>Template</Button>}
      >
        {kind === "punch" && (
          <p className="muted" style={{ marginTop: 0 }}>
            Columns: <code>external_id, punch_time, employee_code, enrolment_id, device_serial, direction, latitude, longitude</code>. Punch time with offset; employee code, or enrolment id with device serial.
          </p>
        )}
        <div className="row">
          <input ref={file} type="file" accept={kind === "punch" ? ".csv" : ".xlsx,.csv"} style={{ maxWidth: 360 }} />
          <Button kind="main" icon="upload" disabled={act.busy} onClick={upload}>Upload</Button>
          <Button icon="refresh" onClick={list.reload}>Refresh</Button>
        </div>
        <ErrorBox error={act.error} />
      </Card>
      <ErrorBox error={list.error} />
      <Card title="Recent imports" pad={false}>
        <Table
          rows={list.data?.items}
          empty="No imports yet."
          cols={[
            { h: "Uploaded", c: (i) => fmtTime(i.created_at) },
            { h: "State", c: (i) => <Pill value={i.state} /> },
            { h: "Rows", c: (i) => i.total_rows ?? "—", right: true },
            { h: "Applied", c: (i) => i.applied_count ?? "—", right: true },
            { h: "Skipped", c: (i) => i.skipped_count, right: true },
            { h: "Errors", c: (i) => i.error_count ?? "—", right: true },
            { h: "Note", c: (i) => i.failure_reason ?? "" },
            { h: "", right: true, c: (i) => !!i.error_count && <Button kind="link" onClick={() => errors(i.id)}>Error file</Button> },
          ]}
        />
      </Card>
    </div>
  );
}
