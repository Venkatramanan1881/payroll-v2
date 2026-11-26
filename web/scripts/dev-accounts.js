// Dev only: makes the shared demo logins in the dev organisation, using DEV_LOGIN_PASSWORD from backend/.env.
//   admin@gmail.com    Org Admin (password; needs MFA_DISABLED=1 to skip the authenticator)
//   vendor@gmail.com   vendor console (password)
//   employee@gmail.com email on the dev employee DEV_EMPLOYEE_CODE, who signs in with org code + employee code + PIN
// Safe to run again: existing accounts are left as they are.
const fs = require("fs");
const path = require("path");
const crypto = require("crypto");
const { execFileSync } = require("child_process");

const BASE = process.env.API ?? "http://localhost:3000/v1";
const backend = path.join(__dirname, "../../backend");
const env = fs.readFileSync(path.join(backend, ".env"), "utf8");
const get = (k) => env.match(new RegExp(`^${k}=(.+)$`, "m"))?.[1]?.trim();
const PASSWORD = get("DEV_LOGIN_PASSWORD");
if (!PASSWORD) throw new Error("DEV_LOGIN_PASSWORD is not in backend/.env");

async function call(method, p, body, token, version) {
  const h = { "Content-Type": "application/json", "Idempotency-Key": crypto.randomUUID() };
  if (token) h.Authorization = `Bearer ${token}`;
  if (version !== undefined) h["If-Match"] = `"${version}"`;
  const r = await fetch(BASE + p, { method, headers: h, body: body === undefined ? undefined : JSON.stringify(body) });
  const t = await r.text();
  const j = t ? JSON.parse(t) : null;
  if (!r.ok) throw Object.assign(new Error(`${method} ${p} → ${r.status} ${t}`), { body: j });
  return j;
}
const cargo = (bin, args, input) =>
  execFileSync("cargo", ["run", "-q", "-p", "api", "--bin", bin, "--", ...args], { cwd: backend, input, encoding: "utf8" });

(async () => {
  const s = await call("POST", "/auth/admin/sign-in", { email: get("DEV_ADMIN_EMAIL"), password: get("DEV_ADMIN_PASSWORD") });
  if (!s.access_token) throw new Error("Admin sign-in still asks for MFA: set MFA_DISABLED=1 in backend/.env and restart the API");
  const admin = s.access_token;

  // Admin
  const users = await call("GET", "/access/users?kind=admin", undefined, admin);
  if (users.items.some((u) => u.email?.toLowerCase() === "admin@gmail.com")) console.log("admin@gmail.com: already there");
  else {
    const roles = await call("GET", "/access/roles", undefined, admin);
    const orgAdmin = roles.items.find((r) => r.builtin_key === "org_admin");
    await call("POST", "/access/users", { email: "admin@gmail.com", display_name: "Admin", roles: [{ role_id: orgAdmin.id, scope_kind: "organisation" }] }, admin);
    const link = cargo("invite_link", ["admin@gmail.com"]).trim();
    const token = new URL(link.split(/\s+/).find((w) => w.startsWith("http"))).searchParams.get("token");
    await call("POST", "/auth/admin/accept-invite", { invite_token: token, password: PASSWORD });
    console.log("admin@gmail.com: created (Org Admin)");
  }

  // Vendor
  try {
    cargo("vendor_user", ["vendor@gmail.com", "Vendor"], PASSWORD + "\n");
    console.log("vendor@gmail.com: created");
  } catch (e) {
    if (/in_use|duplicate|unique/i.test(String(e.stderr ?? e.message))) console.log("vendor@gmail.com: already there");
    else throw e;
  }

  // Employee: email on the dev employee
  const code = get("DEV_EMPLOYEE_CODE");
  const hit = (await call("GET", `/employees?q=${code}`, undefined, admin)).items.find((e) => e.employee_code === code);
  const emp = await call("GET", `/employees/${hit.id}`, undefined, admin);
  if (emp.personal.email === "employee@gmail.com") console.log(`employee@gmail.com: already on ${code}`);
  else {
    await call("PATCH", `/employees/${hit.id}/personal`, { email: "employee@gmail.com" }, admin, emp.version);
    console.log(`employee@gmail.com: set on ${code} (${emp.personal.full_name}); sign in with DEV_EMPLOYEE_ORG + DEV_EMPLOYEE_CODE + DEV_EMPLOYEE_PIN`);
  }
})().catch((e) => {
  console.error(e.message);
  process.exit(1);
});
