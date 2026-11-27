"use client";
import { useState } from "react";
import { Button, Card, ErrorBox, FilePick, fmtTime, PageHead, Pill, Table } from "@/components/ui";
import { api, download } from "@/lib/api";
import { useAction, useApi } from "@/lib/hooks";

type Imp = { id: string; state: string; total_rows: number; applied_count: number; error_count: number; skipped_count: number; failure_reason: string | null; created_at: string; completed_at: string | null };

// B1–B5. Files are processed in the background; Refresh shows progress.
export default function Imports() {
  const list = useApi<{ items: Imp[] }>("/employee-imports");
  const act = useAction();
  const [file, setFile] = useState<File | null>(null);
  const upload = () => {
    const f = file;
    if (!f) return;
    const fd = new FormData();
    fd.append("file", f);
    act.run(() => api("/employee-imports", { body: fd })).then((r) => {
      if (!r) return;
      setFile(null);
      list.reload();
    });
  };
  const errors = (id: string) =>
    act.run(async () => {
      const { url } = await api<{ url: string }>(`/employee-imports/${id}/errors`);
      await download(url.replace(/^\/v1/, ""), `import-errors-${id.slice(0, 8)}.xlsx`);
    });
  return (
    <>
      <PageHead title="Bulk import">
        <Button icon="download" onClick={() => act.run(() => download("/employee-imports/template", "employee-import-template.xlsx"))}>Template</Button>
        <Button icon="refresh" onClick={list.reload}>Refresh</Button>
      </PageHead>
      <Card title="Upload">
        <div className="row">
          <FilePick file={file} onPick={setFile} accept=".xlsx,.csv" hint="Excel (.xlsx) or CSV file" />
          <Button kind="main" onClick={upload} disabled={act.busy || !file}>Upload</Button>
        </div>
        <ErrorBox error={act.error} />
      </Card>
      <div style={{ height: 16 }} />
      <ErrorBox error={list.error} />
      <Card title="Imports" pad={false}>
        <Table
          rows={list.data?.items}
          empty="No imports yet."
          cols={[
            { h: "Uploaded", c: (i) => fmtTime(i.created_at) },
            { h: "State", c: (i) => <Pill value={i.state} /> },
            { h: "Rows", c: (i) => i.total_rows, right: true },
            { h: "Applied", c: (i) => i.applied_count, right: true },
            { h: "Skipped", c: (i) => i.skipped_count, right: true },
            { h: "Errors", c: (i) => i.error_count, right: true },
            { h: "Note", c: (i) => i.failure_reason ?? "" },
            { h: "", c: (i) => i.error_count > 0 && <Button kind="link" onClick={() => errors(i.id)}>Error file</Button>, right: true },
          ]}
        />
      </Card>
    </>
  );
}
