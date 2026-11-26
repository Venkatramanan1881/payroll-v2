// Dev only: gives a dev employee portal access, sets a PIN, saves it to backend/.env as DEV_EMPLOYEE_*,
// then prints each self-service response so the portal can be checked against real data.
//   node web/scripts/dev-employee.js [EMPLOYEE_CODE]      (default: first active employee)
const fs = require("fs");
const path = require("path");
const crypto = require("crypto");
const { execFileSync } = require("child_process");

const BASE = process.env.API ?? "http://localhost:3000/v1";
const envPath = path.join(__dirname, "../../backend/.env");
const env = () => fs.readFileSync(envPath, "utf8");
const get = (k) => env().match(new RegExp(`^${k}=(.+)$`, "m"))?.[1]?.trim();

async function call(method, p, body, token, version) {
  const h = { "Content-Type": "application/json", "Idempotency-Key": crypto.randomUUID() };
  if (token) h.Authorization = `Bearer ${token}`;
  if (version !== undefined) h["If-Match"] = `"${version}"`;
  const r = await fetch(BASE + p, { method, headers: h, body: body === undefined ? undefined : JSON.stringify(body) });
  const t = await r.text();
  const j = t ? JSON.parse(t) : null;
  if (!r.ok) throw Object.assign(new Error(`${method} ${p} → ${r.status} ${t}`), { status: r.status, body: j });
  return j;
}

const weak = (p) => /^(\d)\1+$/.test(p) || "0123456789".includes(p) || "9876543210".includes(p);

(async () => {
  const s1 = await call("POST", "/auth/admin/sign-in", { email: get("DEV_ADMIN_EMAIL"), password: get("DEV_ADMIN_PASSWORD") });
  const totp = execFileSync("node", [path.join(__dirname, "dev-totp.js")]).toString().split(" ")[0];
  const admin = (await call("POST", "/auth/admin/mfa", { mfa_token: s1.mfa_token, totp_code: totp })).access_token;
  const org = (await call("GET", "/me/access", undefined, admin)).org.short_code;

  const want = process.argv[2];
  const list = await call("GET", `/employees${want ? `?q=${want}` : ""}`, undefined, admin);
  const emp = list.items.find((e) => (!want || e.employee_code === want) && e.status !== "exited");
  if (!emp) throw new Error("No employee found");

  let issued;
  try {
    issued = await call("POST", `/employees/${emp.id}/portal-access`, { send_email: false }, admin);
  } catch (e) {
    if (e.body?.code !== "already_has_access") throw e;
    issued = await call("POST", `/employees/${emp.id}/pin-reset`, { send_email: false }, admin);
  }
  const device = { id: "dev-script", platform: "web" };
  const s = await call("POST", "/auth/employee/sign-in", { org_short_code: org, employee_code: emp.employee_code, pin: issued.temporary_pin, device });
  let pin;
  do pin = String(crypto.randomInt(0, 1e6)).padStart(6, "0");
  while (weak(pin));
  await call("POST", "/auth/employee/pin", { current_pin: issued.temporary_pin, new_pin: pin }, s.access_token);

  let text = env().replace(/^DEV_EMPLOYEE_.*\r?\n?/gm, "");
  if (!text.endsWith("\n")) text += "\n";
  fs.writeFileSync(envPath, text + `DEV_EMPLOYEE_ORG=${org}\nDEV_EMPLOYEE_CODE=${emp.employee_code}\nDEV_EMPLOYEE_PIN=${pin}\n`);
  console.log(`Portal access ready for ${emp.employee_code} (${emp.full_name}); org code, employee code and PIN saved to backend/.env as DEV_EMPLOYEE_*`);

  if (process.argv.includes("--dump")) {
    const t = s.access_token;
    for (const p of ["/me/access", "/me/profile", "/attendance/me/punches/today", "/attendance/me/days", "/attendance/me/roster", "/attendance/me/attendance/features", "/attendance/me/optional-holidays", "/attendance/me/notifications", "/attendance/regularisations", "/attendance/optional-holiday-picks"]) {
      try {
        console.log(`\n### ${p}\n` + JSON.stringify(await call("GET", p, undefined, t), null, 1).slice(0, 1500));
      } catch (e) {
        console.log(`\n### ${p}\n${e.message.slice(0, 400)}`);
      }
    }
  }
})().catch((e) => {
  console.error(e.message);
  process.exit(1);
});
