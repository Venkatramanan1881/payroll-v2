# PayLedger Huly Theme — Design Spec

Nov 22, 2026 · @Venkat

## Overview

PayLedger has two Huly-style themes, Dark and Light, each designed across three screens: Dashboard, Organizations and Add Employee. Both share one layout, one icon set and one navigation pattern, and differ only in colour and surface treatment.

| Theme | Character | Base | Accent use |
| --- | --- | --- | --- |
| Dark | Near-black with violet glow, flat charcoal panels, thin low-contrast borders | #0c0c0f page, #141418 cards | Violet for selected states; orange for the button edge glow, notification badge and payroll trend |
| Light | Off-white with violet glow and a faint dot grid, black-and-white UI | #f6f6f8 page, #ffffff cards | Black for main buttons, tabs and the active menu tile; violet and orange as accents |

All six screens are clickable prototypes: the menu flyout, date picker and payroll-step hover glow work in Play mode.

## Colour palette

Violet #8b7cf6 is the shared accent; orange #ff8a4c appears only in glows, the notification badge and the payroll trend line.

| Token | Dark | Light |
| --- | --- | --- |
| Page background | #0c0c0f | #f6f6f8 |
| Menu rail | #0a0a0c | White at 75%, border #e7e5e1 |
| Card / panel | #141418 | #ffffff |
| Input / chip fill | #1c1c21 | #f2f1ef |
| Border | #26262c | #e7e5e1 |
| Divider line | #222227 | #eeece8 |
| Primary text | #ededf0 | #171615 |
| Label text | #c4c6cc | #3d3b38 |
| Muted text | #9a9ca5 | #64625e |
| Accent (icons, strokes) | #8b7cf6 | #8a5cf6 |
| Accent text on tint | #c3b9ff | #6a3fd9 |
| Accent tint | Violet at 12–20% | Violet at 10–12% |
| Main button | #e9e9ec pill, #0c0c0f text | #141312 pill, white text |
| Orange | #ff8a4c | #f07a3a |
| Success (Active, done) | #5fd39a on green 13% | Violet tint (no green) |
| Warning (exceptions) | #f5b454 | #c2410c |

The Light theme uses warm greys throughout so no cool blue undertone shows.

## Typography and icons

Google Sans sets all UI text; Google Sans Code sets identifiers; Material Symbols Rounded supplies every icon.

| Use | Font | Size / weight |
| --- | --- | --- |
| Page title | Google Sans | 30 px, 600 |
| Card and section headings | Google Sans | 18–21 px, 600 |
| Body, inputs, buttons | Google Sans | 15–16 px, 400–600 |
| Labels, captions | Google Sans | 12–14 px, 500 |
| KPI values | Google Sans | 32 px, 600 |
| GSTIN, TAN, codes, kbd hints | Google Sans Code | 12–13 px, 400–500 |
| Main button label | Google Sans | 15 px, 700, uppercase, 0.4 px tracking |

Icons are outlined at weight 300 by default and switch to filled (weight 400) for on states: the active menu item, dashboard card icons and completed-step ticks.

| Area | Icons |
| --- | --- |
| Menu rail | space\_dashboard, group, apartment, work\_history, fingerprint, event\_available, account\_balance\_wallet, shield\_person |
| Top bar | search, download, dark\_mode, notifications, account\_circle |
| Tables and forms | edit, delete, view\_column, add, calendar\_today, chevrons |

## Navigation

Navigation is a 76 px icon-only rail; labels appear only as hover tooltips, and sub-pages open in a flyout on click.

1. **Rail:** logo tile at top, seven section icons (48 px buttons, 10 px radius), role switcher shield at the bottom.
2. **Current section:** violet tint behind the icon plus a 4 px violet bar on the rail edge (Light: black tile, soft violet icon).
3. **Hover:** a small tooltip shows the section name beside the icon; nothing expands.
4. **Click:** a 264 px panel slides out beside the rail with the section's sub-pages; the current page is highlighted with the accent tint.
5. **Close:** the panel's back button, a click outside, or clicking the same icon again.
6. **Role switcher:** the shield opens the same panel with a "Simulate the app as" dropdown (Super Admin, HR Manager, Branch Admin, Employee).

The top bar holds the breadcrumb, organisation and branch filters, search (⌘K), Install App, theme toggle, notifications (badge) and the account menu.

## Components

Corners are 8 px on controls and 12 px on cards; only main buttons and status pills are fully round.

| Component | Dark | Light |
| --- | --- | --- |
| Main button | Light grey pill, dark uppercase text, orange edge glow and warm streak on the right | Black pill, white uppercase text, same orange edge glow |
| Secondary button | Transparent, 1 px #34343b border, white text, 8 px radius | White, 1 px #dddad4 border, dark text |
| Card | #141418, 1 px #24252b border, no shadow, 12 px radius | White, fine border, soft shadow |
| Input / dropdown | #1c1c21 fill, no border, 8 px radius, violet focus ring | #f2f1ef fill, same treatment |
| Tabs | Selected: #26262c pill, violet text | Selected: black pill, white text |
| Status pill | "Active": mint text on green tint | "Active": violet text on violet tint |
| Code chip | Violet tint, Google Sans Code | Violet tint, Google Sans Code |
| KPI icon tile | 38 px rounded square, violet tint, inset violet ring, filled icon | Same |
| Date picker | Custom popover: month/year arrows, violet selected day, outlined today, Clear and Today links | Same, white panel, violet selection |
| Dropdown list | Native, styled dark via color-scheme | Native, light |

**Payroll steps** (Dashboard, seven stages):

- **Done (1–3):** label reads "Step N · Done" with a filled tick icon; Light shows them as white cards with a fine border.
- **Current (4):** violet-pink-orange-amber gradient border with a soft halo, always on.
- **Upcoming (5–7):** plain grey fill, muted text.
- **Hover (any step):** a coloured glow follows the pointer along the border.

## Effects

Four effects carry the Huly feel: background glows, the light-theme dot grid, the main-button edge glow and the payroll-step edge glow.

| Effect | Dark | Light |
| --- | --- | --- |
| Background glow | Violet radial glows: 900×520 px top-right at 24%, 760×560 px bottom-left at 16% | Same violet glows on #f6f6f8 |
| Dot grid | None (tried and removed) | 1 px dots every 20 px, near-black at 7% |
| Main button glow | 1 px orange ring at 55%, 18 px orange halo, warm inset streak on the right | Same on a black pill |
| Step edge glow | Conic gradient border on the current step; pointer-following radial glow on hover | Same |

The top bar is translucent so the glow shows through; cards, rail and top bar stay opaque enough that no effect sits behind text.

## Screens

Each screen exists in Dark and Light at 1440 px wide and reflows to phone width.

| Screen | What it shows | Interactions |
| --- | --- | --- |
| Dashboard | "Enterprise Payroll Overview": eight KPI cards (54 total, 53 active, 36 present, 0 on leave, 3 exceptions, 7 approvals, ₹38,39,247 net, status Open); November 2026 payroll steps for run RUN-202611-351; attendance compliance and gross payroll trend charts | Add Employee and Open Payroll Run buttons; step hover glow; menu flyout |
| Organizations | "Organization Structure" with Organizations / Branches / Departments / Designations tabs; table of KaryaTech Systems Pvt. Ltd. (KTS, 50 employees) and KaryaTech Logistics LLP (KTL, 4); GSTIN/TAN, location, status, actions | Search, Columns, Export CSV, New Organization; pagination |
| Add Employee | "New Employee Onboarding" form in three sections: Basic Information, Address, Organization & Employment | Custom date picker for Date of Birth and Joining Date; Reset, Cancel, Save as Draft, Create Employee |

Open question: the sub-pages listed under Shifts, Attendance, Leave & Permissions and Payroll are placeholders and need the real lists.
