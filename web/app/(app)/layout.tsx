"use client";
import { useRouter } from "next/navigation";
import { useEffect, useState } from "react";
import { Shell } from "@/components/Shell";
import { ErrorBox, Loading } from "@/components/ui";
import { api, ApiError, tokens } from "@/lib/api";
import { Access, AccessContext } from "@/lib/hooks";

// Every page below needs a signed-in admin; /me/access says what they may see.
export default function AppLayout({ children }: { children: React.ReactNode }) {
  const router = useRouter();
  const [access, setAccess] = useState<Access | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  useEffect(() => {
    if (!tokens.get()) return router.replace("/login");
    api<Access>("/me/access").then((a) => (a.user.kind === "employee" ? router.replace("/portal") : setAccess(a)), setError);
  }, [router]);
  if (error?.code === "pin_change_required") {
    router.replace("/portal");
    return <Loading full />;
  }
  if (error) return <div className="content"><ErrorBox error={error} /></div>;
  if (!access) return <Loading full />;
  return (
    <AccessContext.Provider value={access}>
      <Shell>{children}</Shell>
    </AccessContext.Provider>
  );
}
