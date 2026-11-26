"use client";
import { Button, Card, ErrorBox, fmtTime, PageHead, Table } from "@/components/ui";
import { api } from "@/lib/api";
import { useAction, useApi } from "@/lib/hooks";

type N = { id: string; event: string; payload: Record<string, unknown>; created_at: string; read_at: string | null };

export default function Notifications() {
  const list = useApi<{ items: N[]; unread_count: number }>("/attendance/me/notifications");
  const act = useAction();
  const read = (id?: string) =>
    act.run(() => api(id ? `/attendance/me/notifications/${id}/read` : "/attendance/me/notifications/read-all", { method: "POST" })).then(list.reload);
  return (
    <>
      <PageHead title="Notifications" sub={`${list.data?.unread_count ?? 0} unread`}>
        <Button icon="done_all" onClick={() => read()} disabled={act.busy}>
          Mark all read
        </Button>
      </PageHead>
      <ErrorBox error={list.error ?? act.error} />
      <Card pad={false}>
        <Table
          rows={list.data?.items}
          empty="No notifications."
          cols={[
            { h: "Event", c: (n) => <span style={{ fontWeight: n.read_at ? 400 : 600 }}>{n.event.replace(/_/g, " ")}</span> },
            {
              h: "Details",
              c: (n) => (
                <span className="muted small">
                  {Object.entries(n.payload ?? {})
                    .map(([k, v]) => `${k}: ${typeof v === "object" ? JSON.stringify(v) : v}`)
                    .join(" · ")}
                </span>
              ),
            },
            { h: "When", c: (n) => fmtTime(n.created_at) },
            { h: "", c: (n) => !n.read_at && <Button kind="link" onClick={() => read(n.id)}>Mark read</Button>, right: true },
          ]}
        />
      </Card>
    </>
  );
}
