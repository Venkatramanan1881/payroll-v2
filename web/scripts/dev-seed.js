// Dev only: fills the dev organisation with demo data through the API, signed in as admin@gmail.com
// (DEV_LOGIN_PASSWORD in backend/.env, MFA_DISABLED=1). Safe to run again: anything already there is skipped.
//   node web/scripts/dev-seed.js
const fs = require("fs");
const path = require("path");
const crypto = require("crypto");

const BASE = process.env.API ?? "http://localhost:3000/v1";
const env = fs.readFileSync(path.join(__dirname, "../../backend/.env"), "utf8");
const get = (k) => env.match(new RegExp(`^${k}=(.+)$`, "m"))?.[1]?.trim();

let token;
async function call(method, p, body) {
  const h = { "Content-Type": "application/json", "Idempotency-Key": crypto.randomUUID(), Authorization: `Bearer ${token}` };
  const r = await fetch(BASE + p, { method, headers: h, body: body === undefined ? undefined : JSON.stringify(body) });
  const t = await r.text();
  if (!r.ok) throw new Error(`${method} ${p} → ${r.status} ${t}`);
  return t ? JSON.parse(t) : null;
}

const BRANCHES = [
  ["CHN", "Chennai"],
  ["MDU", "Madurai"],
  ["CBE", "Coimbatore"],
];
const LISTS = {
  departments: [["PROD", "Production"], ["ADMIN", "Administration"], ["QA", "Quality"], ["MAINT", "Maintenance"], ["STORES", "Stores"], ["HR", "Human Resources"]],
  designations: [["OPR", "Operator"], ["TECH", "Technician"], ["SUP", "Supervisor"], ["EXEC", "Executive"], ["MGR", "Manager"], ["HLP", "Helper"]],
};
const FIRST = ["Arun", "Priya", "Karthik", "Divya", "Suresh", "Lakshmi", "Vignesh", "Kavya", "Ramesh", "Anitha", "Manoj", "Deepa", "Senthil", "Revathi", "Prakash", "Saranya", "Gopal", "Nithya", "Bala", "Sangeetha", "Mohan", "Keerthana", "Rajesh", "Pavithra", "Dinesh", "Swetha", "Ganesh", "Janani"];
const LAST = ["Kumar", "S", "R", "Natarajan", "Murugan", "Iyer", "Pillai", "Raman", "K", "Subramanian"];
const FEMALE = new Set(["Priya", "Divya", "Lakshmi", "Kavya", "Anitha", "Deepa", "Revathi", "Saranya", "Nithya", "Sangeetha", "Keerthana", "Pavithra", "Swetha", "Janani"]);

/** Makes each [code, name] in `want` exist at `p`; returns code → id for all of them. */
async function ensure(p, codeKey, want, extra = {}) {
  const have = (await call("GET", p)).items;
  const ids = {};
  for (const [code, name] of want) {
    let item = have.find((i) => i[codeKey] === code || i.name === name);
    if (!item) {
      item = await call("POST", p, { [codeKey]: code, name, ...extra });
      console.log(`+ ${p} ${code} ${name}`);
    }
    ids[code] = item.id;
  }
  return ids;
}

const pick = (a, i) => a[i % a.length];
const iso = (d) => d.toISOString().slice(0, 10);

(async () => {
  const s = await fetch(BASE + "/auth/admin/sign-in", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ email: "admin@gmail.com", password: get("DEV_LOGIN_PASSWORD") }) }).then((r) => r.json());
  if (!s.access_token) throw new Error("Admin sign-in failed or asks for MFA: run dev-accounts.js and set MFA_DISABLED=1");
  token = s.access_token;

  const branches = await ensure("/org/branches", "short_code", BRANCHES, { time_zone: "Asia/Kolkata" });
  const depts = await ensure("/org/lists/departments", "code", LISTS.departments);
  const desigs = await ensure("/org/lists/designations", "code", LISTS.designations);

  // Use what the org already has for these; the first active one of each if only one exists.
  const active = async (p) => (await call("GET", p)).items.filter((x) => (x.state ?? "active") === "active").map((x) => x.id);
  const cats = await active("/org/lists/categories");
  const types = await active("/org/lists/employee-types");
  const groups = await active("/org/payroll-groups");
  if (!cats.length || !types.length || !groups.length) throw new Error("Add at least one category, employee type and payroll group first (Organisation pages)");

  const existing = [];
  for (let cursor = null; ; ) {
    const r = await call("GET", `/employees?limit=200${cursor ? `&cursor=${cursor}` : ""}`);
    existing.push(...r.items);
    if (!(cursor = r.next_cursor)) break;
  }
  const mobiles = new Set(existing.map((e) => e.mobile));

  const deptCodes = Object.keys(depts);
  const desigCodes = Object.keys(desigs);
  const branchCodes = Object.keys(branches);
  let made = 0;
  for (let i = 0; i < FIRST.length; i++) {
    const mobile = `+9197000${String(10000 + i).slice(1)}`;
    if (mobiles.has(mobile)) continue;
    const first = FIRST[i];
    const joined = new Date(Date.now() - (30 + i * 47) * 86400000); // spread over the last ~4 years
    const probation = i % 6 === 0; // a few recent-style joiners still on probation
    const body = {
      joining_date: iso(probation ? new Date(Date.now() - (10 + i) * 86400000) : joined),
      initial_status: probation ? "probation" : "active",
      ...(probation && { probation_end_date: iso(new Date(Date.now() + 150 * 86400000)) }),
      personal: {
        full_name: `${first} ${pick(LAST, i * 3)}`,
        mobile,
        gender: FEMALE.has(first) ? "female" : "male",
        date_of_birth: iso(new Date(1975 + (i % 25), i % 12, 1 + (i % 27))),
      },
      job: {
        master_branch_id: branches[pick(branchCodes, i)],
        department_id: depts[pick(deptCodes, i)],
        designation_id: desigs[pick(desigCodes, i * 5)],
        category_id: pick(cats, i),
        employee_type_id: pick(types, i * 7),
        payroll_group_id: pick(groups, i),
      },
    };
    try {
      const e = await call("POST", "/employees", body);
      made++;
      console.log(`+ employee ${e.employee_code ?? ""} ${body.personal.full_name}`);
    } catch (err) {
      console.error(`! ${body.personal.full_name}: ${err.message}`);
      if (made === 0) process.exit(1); // the first failure is usually a missing mandatory field: stop and show it
    }
  }
  console.log(`Done: ${made} employees added, ${existing.length} were already there.`);
})().catch((e) => {
  console.error(e.message);
  process.exit(1);
});
