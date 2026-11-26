// Dev only: shifts, a roster and biometric-style punches for the last 30 days, through the API as admin@gmail.com.
// Run dev-seed.js first. Safe to run again: shifts are reused, the roster is rewritten, punches dedupe on external_id.
//   node web/scripts/dev-seed-attendance.js
const fs = require("fs");
const path = require("path");
const crypto = require("crypto");

const BASE = process.env.API ?? "http://localhost:3000/v1";
const DAYS_BACK = 30;
const DAYS_AHEAD = 7;
const env = fs.readFileSync(path.join(__dirname, "../../backend/.env"), "utf8");
const get = (k) => env.match(new RegExp(`^${k}=(.+)$`, "m"))?.[1]?.trim();

let token;
// Sign in (again): the access token expires before a slow run against the remote dev database ends.
async function signIn() {
  const s = await fetch(BASE + "/auth/admin/sign-in", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ email: "admin@gmail.com", password: get("DEV_LOGIN_PASSWORD") }) }).then((r) => r.json());
  if (!s.access_token) throw new Error("Admin sign-in failed or asks for MFA: run dev-accounts.js and set MFA_DISABLED=1");
  token = s.access_token;
}
async function call(method, p, body, version, retried) {
  const h = { "Content-Type": "application/json", "Idempotency-Key": crypto.randomUUID(), Authorization: `Bearer ${token}` };
  if (version !== undefined) h["If-Match"] = `"${version}"`;
  const r = await fetch(BASE + p, { method, headers: h, body: body === undefined ? undefined : JSON.stringify(body) });
  if (r.status === 401 && !retried) return signIn().then(() => call(method, p, body, version, true));
  const t = await r.text();
  if (!r.ok) throw new Error(`${method} ${p} → ${r.status} ${t}`);
  return t ? JSON.parse(t) : null;
}

const SHIFTS = [
  { code: "GEN", name: "General", start_time: "09:00", end_time: "18:00" },
  { code: "MOR", name: "Morning", start_time: "06:00", end_time: "14:00" },
  { code: "EVE", name: "Evening", start_time: "14:00", end_time: "22:00" },
];

// Dates are local (IST) calendar days.
const IST = 330;
const day = (offset) => new Date(Date.now() + IST * 60000 + offset * 86400000).toISOString().slice(0, 10);
const at = (date, hhmm, plusMin) => {
  const [h, m] = hhmm.split(":").map(Number);
  const t = new Date(`${date}T00:00:00+05:30`).getTime() + (h * 60 + m + plusMin) * 60000;
  return new Date(t).toISOString().replace(".000Z", "Z");
};
// Deterministic "random" so a rerun produces the same punches (and the same external_ids).
const rand = (...k) => parseInt(crypto.createHash("md5").update(k.join("|")).digest("hex").slice(0, 8), 16) / 0xffffffff;

(async () => {
  await signIn();

  // 1. Shifts
  const have = (await call("GET", "/attendance/shifts")).items;
  const shiftId = {};
  for (const sh of SHIFTS) {
    let x = have.find((h) => h.code === sh.code);
    if (!x) {
      x = await call("POST", "/attendance/shifts", { ...sh, valid_from: day(-DAYS_BACK - 1), kind: "fixed", required_minutes: 480, half_day_point: "middle", window_before_min: 180, window_after_min: 360, is_night: false });
      console.log(`+ shift ${sh.code} ${sh.start_time}–${sh.end_time}`);
    }
    shiftId[sh.code] = x.id;
  }

  // 2. Employees with their joining dates
  const emps = [];
  for (let cursor = null; ; ) {
    const r = await call("GET", `/employees?limit=200${cursor ? `&cursor=${cursor}` : ""}`);
    emps.push(...r.items.filter((e) => ["active", "probation", "notice"].includes(e.status)));
    if (!(cursor = r.next_cursor)) break;
  }
  for (const e of emps) e.joined = (await call("GET", `/employees/${e.id}`)).employment.joining_date;
  console.log(`${emps.length} employees`);

  // 3. Roster: one shift per person (2 in 3 on General), Sundays off
  const shiftOf = (i) => (i % 3 === 1 ? "MOR" : i % 6 === 2 ? "EVE" : "GEN");
  // PUNCHES_ONLY=1 skips this slow pass when the roster is already written.
  for (const [i, e] of process.env.PUNCHES_ONLY ? [] : emps.entries()) {
    const entries = [];
    for (let o = -DAYS_BACK; o <= DAYS_AHEAD; o++) {
      const d = day(o);
      if (d < e.joined) continue;
      const sunday = new Date(d + "T00:00:00Z").getUTCDay() === 0;
      entries.push({ employee_id: e.id, work_date: d, shift_id: sunday ? null : shiftId[shiftOf(i)], is_weekly_off: sunday });
    }
    // Small batches with one retry: the remote dev database times out on large roster writes now and then.
    for (let k = 0; k < entries.length; k += 10) {
      const chunk = entries.slice(k, k + 10);
      await call("PUT", "/attendance/roster", { entries: chunk }).catch(() => call("PUT", "/attendance/roster", { entries: chunk }));
    }
  }
  console.log("roster written");

  // 4. Punches for past working days: ~6% absent, ~15% late, ~4% missing the out punch
  const rows = [];
  for (const [i, e] of emps.entries()) {
    const sh = SHIFTS.find((x) => x.code === shiftOf(i));
    for (let o = -DAYS_BACK; o <= -1; o++) {
      const d = day(o);
      if (d < e.joined || new Date(d + "T00:00:00Z").getUTCDay() === 0) continue;
      const r = rand(e.employee_code, d);
      if (r < 0.06) continue; // absent
      const late = r > 0.85 ? 15 + Math.round(rand("late", e.employee_code, d) * 40) : -Math.round(rand("in", e.employee_code, d) * 12);
      const extra = Math.round(rand("out", e.employee_code, d) * 45);
      const id = (dir) => `seed-${e.employee_code}-${d}-${dir}`;
      rows.push({ external_id: id("in"), punch_time: at(d, sh.start_time, late), employee_code: e.employee_code, device_serial: "SEED-DEV-01", direction: "in" });
      if (r < 0.1 && r >= 0.06) continue; // forgot to punch out
      const endShift = sh.end_time === "00:00" ? "24:00" : sh.end_time;
      rows.push({ external_id: id("out"), punch_time: at(d, endShift, extra), employee_code: e.employee_code, device_serial: "SEED-DEV-01", direction: "out" });
    }
  }
  const up = await call("POST", "/attendance/punch-imports/json", { rows });
  const imp = { id: up.import_id, state: up.state };
  console.log(`punch import ${imp.id}: ${rows.length} rows, ${imp.state}`);
  for (let n = 0; n < 60 && !["completed", "failed"].includes(imp.state); n++) {
    await new Promise((r) => setTimeout(r, 5000));
    Object.assign(imp, await call("GET", `/attendance/punch-imports/${imp.id}`));
    console.log(`  ${imp.state}: ${imp.applied_count ?? 0} applied, ${imp.error_count ?? 0} errors of ${imp.total_rows}`);
  }
})().catch((e) => {
  console.error(e.message);
  process.exit(1);
});
