// Rail sections and their sub-pages. `perm`: shown when the user holds any of these (the server still checks).
export type NavPage = { href: string; label: string; icon: string; perm?: string[] };
export type NavSection = { key: string; label: string; icon: string; pages: NavPage[] };

export const NAV: NavSection[] = [
  { key: "home", label: "Dashboard", icon: "space_dashboard", pages: [{ href: "/", label: "Overview", icon: "space_dashboard" }] },
  {
    key: "employees",
    label: "Employees",
    icon: "group",
    pages: [
      { href: "/employees/dashboard", label: "Dashboard", icon: "monitoring", perm: ["employee.view"] },
      { href: "/employees", label: "Employee master", icon: "badge", perm: ["employee.view"] },
      { href: "/employees/new", label: "Add employee", icon: "person_add", perm: ["employee.create"] },
      { href: "/employees/status-requests", label: "Status requests", icon: "swap_horiz", perm: ["employee.status.raise", "employee.status.approve"] },
      { href: "/employees/imports", label: "Bulk import", icon: "upload_file", perm: ["employee.bulk_import"] },
    ],
  },
  {
    key: "org",
    label: "Organisation",
    icon: "apartment",
    pages: [
      { href: "/org/branches", label: "Branches", icon: "store", perm: ["employee.view", "org.branch.manage"] },
      { href: "/org/lists", label: "Departments & lists", icon: "category", perm: ["employee.view", "org.lists.manage"] },
      { href: "/org/contractors", label: "Contractors", icon: "engineering", perm: ["employee.view", "org.contractor.manage"] },
      { href: "/org/payroll-groups", label: "Payroll groups", icon: "groups", perm: ["employee.view", "org.payroll_group.manage"] },
      { href: "/org/custom-fields", label: "Custom fields", icon: "dynamic_form", perm: ["employee.view", "employee.custom_fields.manage"] },
      { href: "/org/settings", label: "Employee master settings", icon: "tune", perm: ["employee.view", "org.settings.edit"] },
    ],
  },
  {
    key: "shifts",
    label: "Shifts & roster",
    icon: "work_history",
    pages: [
      { href: "/attendance/shifts", label: "Shifts", icon: "schedule", perm: ["attendance.roster.view", "attendance.shift.manage"] },
      { href: "/attendance/roster", label: "Roster", icon: "calendar_view_week", perm: ["attendance.roster.view"] },
      { href: "/attendance/holidays", label: "Holiday calendars", icon: "event", perm: ["attendance.roster.view", "attendance.calendar.manage"] },
      { href: "/attendance/optional-holidays", label: "Optional holiday picks", icon: "event_available", perm: ["attendance.optional_holiday.approve"] },
    ],
  },
  {
    key: "attendance",
    label: "Attendance",
    icon: "fingerprint",
    pages: [
      { href: "/attendance/days", label: "Daily attendance", icon: "today", perm: ["attendance.day.view"] },
      { href: "/attendance/exceptions", label: "Exceptions", icon: "report", perm: ["attendance.day.view"] },
      { href: "/attendance/punches", label: "Punches", icon: "fingerprint", perm: ["attendance.punch.view"] },
      { href: "/attendance/reviews", label: "Punch reviews", icon: "rule", perm: ["attendance.punch.review"] },
      { href: "/attendance/manual", label: "Manual attendance", icon: "edit_calendar", perm: ["attendance.punch.manual"] },
      { href: "/attendance/regularisations", label: "Regularisations", icon: "assignment_turned_in", perm: ["attendance.regularisation.approve", "attendance.regularisation.request"] },
      { href: "/attendance/waivers", label: "Waivers & outages", icon: "healing", perm: ["attendance.waiver.manage", "attendance.outage.regularise"] },
      { href: "/attendance/imports", label: "Punch & roster imports", icon: "upload_file", perm: ["attendance.punch.import", "attendance.roster.manage"] },
      { href: "/attendance/reports", label: "Reports", icon: "summarize", perm: ["attendance.day.view"] },
    ],
  },
  {
    key: "at-settings",
    label: "Attendance settings",
    icon: "settings_applications",
    pages: [
      { href: "/attendance/policies", label: "Policy sets", icon: "policy", perm: ["attendance.policy.manage", "attendance.day.view"] },
      { href: "/attendance/assignments", label: "Policy assignments", icon: "account_tree", perm: ["attendance.policy.manage"] },
      { href: "/attendance/fences", label: "Geo-fences", icon: "location_on", perm: ["attendance.fence.manage"] },
      { href: "/attendance/notifications", label: "Notifications", icon: "notifications_active", perm: ["notification.settings.manage"] },
    ],
  },
  {
    key: "access",
    label: "Users & roles",
    icon: "shield_person",
    pages: [
      { href: "/access/users", label: "Users", icon: "manage_accounts", perm: ["access.role.assign"] },
      { href: "/access/roles", label: "Roles & permissions", icon: "admin_panel_settings", perm: ["access.role.edit", "access.role.assign"] },
      { href: "/audit", label: "Audit log", icon: "history", perm: ["audit.view"] },
    ],
  },
];

/** The section and page a path belongs to (longest matching href wins). */
export function locate(path: string) {
  let best: { section: NavSection; page: NavPage } | null = null;
  for (const section of NAV)
    for (const page of section.pages) {
      const hit = page.href === "/" ? path === "/" : path === page.href || path.startsWith(page.href + "/");
      if (hit && (!best || page.href.length > best.page.href.length)) best = { section, page };
    }
  return best;
}
