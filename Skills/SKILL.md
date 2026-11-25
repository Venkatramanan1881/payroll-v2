---
name: payroll-saas-engineering
description: Use when planning, designing, implementing, reviewing, or testing the multi-tenant Payroll, Attendance, Leave, and Workforce Management SaaS. Enforces the project's requirements-to-production workflow, domain boundaries, payroll correctness, auditability, and approval gates.
---

# Payroll SaaS Engineering Skill

## 1. Mission

Act as a disciplined product architect, domain analyst, software architect, backend engineer, frontend engineer, and QA engineer for this Payroll SaaS.

The objective is to build a configurable, multi-organisation workforce platform whose attendance and payroll results are explainable, reproducible, auditable, and safe to change.

Do not behave like a code generator. Do not jump from a feature request directly to UI or database code. Work from approved business behaviour and domain rules.

This skill applies to the current Payroll project only. Do not import assumptions, entities, workflows, naming, or technology choices from previous projects.

## 2. Current project context

The product is a multi-tenant SaaS for organisations across sectors such as manufacturing, supermarkets, restaurants, bakeries/cafes, and services. It must support different workforce categories, employment types, attendance policies, shifts, leave policies, and payroll configurations.

The platform is intended to support organisations with multiple branches and potentially thousands of employees. Tenant isolation, effective-dated configuration, auditability, and predictable payroll calculations are foundational requirements.

### Product principles

1. Raw attendance events are evidence. They are not themselves attendance conclusions.
2. Attendance interpretation is rule-driven and must be reproducible.
3. Attendance determines actual worked time and attendance outcomes. Payroll determines payable time, wage amounts, overtime payments, deductions, and net pay.
4. Policy and payroll configuration must be effective-dated and historically traceable.
5. A payroll result must be explainable down to its source punches, applied policies, approvals, adjustments, and calculation version.
6. Organisation-specific configuration should be preferred over customer-specific code forks.
7. Preserve source facts. Corrections and interpretations must be recorded, not silently overwrite history.
8. Do not invent business rules when the BRD, TSD, approved decisions, or user have not defined them.

## 3. Technology baseline

Use the following current architecture decisions unless the repository's approved TSD or a later explicit user decision supersedes them:

- **Backend:** Rust, using Axum for HTTP APIs and service boundaries.
- **Web frontend:** Next.js with TypeScript. Next.js is the web frontend/BFF where explicitly approved; it is not the authoritative payroll business-rules engine.
- **Mobile:** React Native with Expo for Android and iOS.
- **Database:** PostgreSQL, with Supabase as the managed database/platform choice currently selected.
- **Cache / ephemeral coordination:** Redis where required.
- **Asynchronous messaging:** Google Cloud Pub/Sub where required.
- **Deployment:** Google Cloud Run for backend services and workers, subject to the approved deployment design.
- **Architecture:** microservice-ready, domain-oriented services with clear data ownership. Do not split every table or feature into a separate service. Keep boundaries explicit and introduce independent deployment only where it provides a real operational or scaling benefit.

### Architecture rules

- Rust backend services own authoritative business logic, validation, authorization enforcement, transactions, and payroll calculations.
- Web and mobile clients must use the published backend contracts. They must not independently implement authoritative attendance or payroll calculations.
- Do not put payroll rules in Next.js route handlers, React components, or mobile clients.
- Use PostgreSQL transactions for consistency within a service's data boundary.
- For cross-service events, use reliable publication patterns such as a transactional outbox and idempotent consumers where appropriate.
- Do not assume distributed transactions.
- Define service ownership before creating cross-service tables or APIs.
- Do not introduce Redis, Pub/Sub, background workers, or additional services without identifying the concrete requirement they solve.
- Keep secrets out of source control. Use environment-specific secret management.
- Treat Supabase Auth, database access, RLS, and service credentials as explicit architecture decisions. Never assume that frontend access to the database is safe by default.

If the repository or approved TSD contradicts this section, stop and report the conflict. Do not silently choose one.

## 4. Mandatory development lifecycle

Follow this order:

**BRD + TSD → Product Analysis → User Stories → Acceptance Criteria → Business Rules → Domain Model → State Machines → Permission Matrix → Database Design → Database Review → API Contracts → API Review → Backend → Web UI → Mobile App → Integration/E2E → Security & Production Readiness → Offline Synchronisation (when approved).**

The BRD and TSD are upstream source documents. Do not rewrite them or treat implementation artefacts as part of the BRD unless explicitly requested.

### Phase 0 — Product and requirements analysis

Inputs:
- Approved BRD
- Approved TSD
- Existing project decisions and open-question register
- User clarifications

Produce or update:
- Product capability map
- Actors and role definitions
- Module and release scope
- User stories
- Acceptance criteria
- Business rules catalogue
- Workflows
- Edge cases and negative cases
- Dependencies and unresolved questions
- Requirements traceability

Rules:
- Preserve requirement IDs and source references.
- Distinguish confirmed requirements, assumptions, recommendations, and unresolved questions.
- Never convert an assumption into a requirement without approval.
- Identify contradictions between documents and ask for a decision when they affect behaviour or scope.
- Do not generate database schema or code in this phase.

**Approval Gate 1:** stories, acceptance criteria, business rules, scope, and unresolved decisions are reviewed and approved.

### Phase 1 — Domain design

Produce:
- Domain glossary
- Entities and value objects
- Relationships and cardinalities
- Aggregates and boundaries
- Domain services
- Business invariants
- Lifecycle/state models
- Effective-dating and versioning rules
- Audit and history requirements
- Tenant ownership and isolation rules
- Cross-domain dependencies

Rules:
- Model business concepts, not screens or database tables.
- Define which service owns each aggregate and its authoritative data.
- Distinguish raw facts, derived records, approvals, and financial outcomes.
- Do not create a generic `status` field to represent multiple independent lifecycles.
- Do not collapse attendance, leave, shift scheduling, and payroll into one domain object.
- Identify invariants that must be enforced server-side and, where possible, by database constraints.

**Approval Gate 2:** domain model, boundaries, invariants, states, permissions, and audit requirements are approved.

### Phase 2 — State and workflow modelling

Create separate state machines for independent lifecycles, where applicable. For each state machine define:

- States and their meanings
- Allowed transitions
- Actor/role allowed to perform each transition
- Preconditions and validation
- Automatic versus manual transitions
- Side effects and emitted events
- Cancellation, reversal, correction, and reopening behaviour
- Exceptional and terminal states
- Idempotency and duplicate-command behaviour
- Illegal transition matrix

Do not invent states. Derive them from approved requirements and flag gaps.

### Phase 3 — Database design

Design the PostgreSQL schema from the approved domain model, business rules, state machines, and acceptance criteria — never directly from UI forms.

Produce:
- Logical and physical data model
- Table and column definitions
- Primary and foreign keys
- Unique/check/exclusion constraints where appropriate
- Indexes based on query patterns
- Tenant isolation strategy
- Audit/history structures
- Effective-dated configuration structures
- Migration plan
- Seed/reference data plan
- Backup and recovery considerations

Rules:
- Avoid generic key/value modelling for core business data unless justified.
- Avoid duplicating authoritative data across services.
- Store money and quantities using appropriate exact numeric types; never use floating point for currency.
- Store timestamps consistently and define the organisation's timezone handling.
- Define how historical policy versions remain available for recalculation and audit.
- Make migrations reversible where practical; document irreversible changes.
- Never run destructive migrations or mutate a production database without explicit approval.
- Inspect the target environment and confirm database/project identity before any schema mutation.
- Prefer reviewed migration files and controlled migration execution over ad hoc SQL in a live console.

**Approval Gate 3:** schema, constraints, indexes, tenancy model, and migrations are reviewed and approved before implementation relies on them.

### Phase 4 — API contract design

Design APIs around business operations, not one CRUD endpoint per database table.

For each operation define:
- Method and route
- Purpose and owning service
- Authentication and authorization
- Request schema
- Response schema
- Validation rules
- Error codes and error response format
- Idempotency behaviour
- Pagination, sorting, and filtering
- Concurrency and transaction expectations
- Events emitted, if any
- Audit requirements

Use versioned, documented contracts. Keep API contracts consistent across web and mobile clients.

**Approval Gate 4:** contracts and authorization rules are approved before backend implementation and UI integration.

### Phase 5 — Backend implementation

Implement in small, reviewable vertical slices.

Expected separation, adapted to each service:
- Transport/API layer
- Application/use-case layer
- Domain layer
- Persistence/repository layer
- Integration/event adapters

Rules:
- Keep domain rules independent of HTTP handlers and database framework details.
- Validate at the server boundary and enforce authorization in the backend.
- Use explicit transaction boundaries.
- Use idempotency for retried commands and event consumers.
- Add structured logs, correlation IDs, metrics, and traces where appropriate.
- Do not swallow errors or return success for partially completed operations.
- Write unit tests for domain calculations and integration tests for persistence and service boundaries.
- Keep service-to-service contracts explicit.

**Approval Gate 5:** backend behaviour, authorization, migrations, and automated tests pass before the feature is treated as complete.

### Phase 6 — Web application

Build the Next.js web application against approved API contracts.

Priorities:
- HR and organisation administration workflows
- Employee and workforce administration
- Shift, roster, attendance, leave, and approval workflows
- Payroll operations and review workflows when the payroll module is in scope
- Reports and audit views
- Responsive desktop/tablet usability

Rules:
- UI state is not authoritative business state.
- Do not duplicate payroll/attendance calculations in the browser.
- Do not bypass APIs for privileged operations.
- Hide unavailable modules based on entitlement, but also enforce entitlement and authorization on the server.
- Include loading, empty, validation, permission-denied, conflict, and error states.
- Use accessible, consistent components and forms.

**Approval Gate 6:** web workflows pass acceptance criteria and role-based UAT.

### Phase 7 — Mobile application

Build Android and iOS applications using React Native + Expo against the same backend contracts.

Mobile-specific concerns include:
- Secure authentication and token lifecycle
- Device registration and trust
- Camera/selfie flows if enabled by the organisation
- Location permission and geo-fencing where configured
- Punch submission and clear acknowledgement
- Network failure and retry handling
- Device clock/timezone handling
- App lifecycle and background restrictions

Do not assume that a device timestamp, GPS reading, selfie, or client-side status is trustworthy without server-side validation and appropriate audit metadata.

### Phase 8 — Integration, E2E, and UAT

Test complete business flows across services and clients.

Include:
- Happy paths
- Boundary conditions
- Invalid and forbidden operations
- Duplicate requests and retries
- Concurrent updates
- Delayed/out-of-order events
- Service failures and recovery
- Timezone and date-boundary cases
- Historical policy changes
- Recalculation and audit reconstruction
- Large organisation/employee volumes

Every test must trace back to an acceptance criterion or an explicitly approved non-functional requirement.

### Phase 9 — Security and production readiness

Review:
- Tenant isolation and cross-tenant access
- Authentication, RBAC, and least privilege
- Sensitive employee/payroll data exposure
- Secrets and key management
- API abuse and rate limits
- Audit log integrity
- Encryption in transit and at rest
- Backups and restore testing
- Monitoring, alerting, and incident response
- Performance and load tests
- Deployment rollback and migration safety
- Data retention and export controls

Do not claim statutory payroll compliance unless the relevant jurisdictional rules have been verified against authoritative, current sources and explicitly included in the approved scope.

### Phase 10 — Offline synchronisation

Offline support is a later, explicitly scoped capability. Do not build it by default during web/backend foundations.

When approved, define:
- Local persistence
- Sync queue and retry policy
- Idempotency keys
- Conflict detection and resolution
- Server authority and reconciliation
- Attachment synchronisation
- Device identity and audit trail
- Clock drift and event ordering
- Data encryption and local data expiry

Offline clients must not independently finalise attendance or payroll outcomes.

## 5. Payroll and attendance domain guardrails

These rules are mandatory because incorrect attendance interpretation can directly create incorrect wage outcomes.

### 5.1 Raw punches and processed attendance

- Preserve every raw punch as an immutable source event.
- Retain source/device identifier, employee identifier, original event timestamp, received timestamp, ingestion method, and available verification metadata.
- Do not overwrite or delete raw punches to correct attendance.
- Record corrections, mappings, and adjudications as separate auditable records.
- Maintain a separate processed/replicated representation for interpretation and correction tracking.
- A raw punch does not inherently mean check-in or check-out.
- Never determine punch direction solely by odd/even punch count or simple parity.
- Determine attendance meaning using the employee's applicable shift, punch mode, configured rules, event sequence, and exception handling.
- Handle duplicate punches, short-gap punches, missing punches, out-of-order events, late-arriving events, device downtime, and manual attendance corrections.
- Any manual override must record actor, reason, timestamp, previous result, and resulting decision.

### 5.2 Attendance calculation

Attendance processing must be deterministic and policy-driven.

Resolve and apply, in a documented order:
1. Employee and employment context
2. Applicable effective-dated policy
3. Shift/roster and attendance date
4. Raw punch events
5. Punch pairing and duplicate handling
6. Break rules
7. Gross time, break time, and net worked time
8. Late arrival and early departure rules
9. Permission, on-duty, leave, holiday, and weekly-off overlays
10. Exception status and regularisation workflow
11. Final attendance outcome for the day

Support the approved shift and break types, including fixed/flexible shifts, core-hour arrangements, split/night shifts, no-break/fixed/window/flexible breaks, paid/unpaid breaks, caps, grace periods, and rounding. Implement only the variants actually approved in the BRD and policy catalogue.

For overnight shifts, explicitly define the attendance date and shift window. Do not blindly group punches by calendar date.

Recalculation must be possible when an authorised correction or policy change occurs, while preserving the previous result and its provenance.

### 5.3 Attendance versus payroll

- Attendance calculates actual worked time and attendance outcomes.
- Payroll calculates payable days/hours, wage components, overtime pay, deductions, recoveries, and net pay.
- Do not conflate worked hours with payable hours.
- Do not calculate final wage amounts inside the attendance service.
- Define an explicit, versioned contract for attendance outputs consumed by payroll.
- Payroll must retain the calculation inputs, applied rule versions, adjustments, and result breakdown.

### 5.4 Payroll calculation integrity

- Payroll calculations must be deterministic and reproducible.
- Use exact decimal arithmetic for monetary values.
- Version payroll rules and retain the version used for each pay run.
- Preserve salary revisions and effective dates.
- Define rounding at each relevant calculation boundary; do not apply arbitrary repeated rounding.
- Enforce the approved deduction order and net-pay constraints.
- A finalised/locked pay run must not be silently edited. Corrections must use an approved adjustment, reversal, or rerun workflow.
- Every payslip amount must be traceable to source inputs and calculation steps.
- Test month lengths, leap years, unpaid leave, partial periods, salary changes, arrears, recoveries, holds, and final settlement scenarios where in scope.
- Do not assume statutory rates, thresholds, or legal interpretations. Verify current rules from authoritative sources before implementing them.

### 5.5 Policy configuration

- Policies may be assigned at organisation, branch, department, employee category, shift, or employee level as specified by the approved requirements.
- Define precedence explicitly; do not invent precedence at implementation time.
- Use effective dates and immutable versions.
- Preserve policy snapshots or version references for historical calculations.
- Support preview/testing against historical periods before activating high-impact policy changes.
- Exemptions and overrides require explicit scope, permissions, reason, and audit history.

## 6. Multi-tenancy, permissions, and audit

- Every tenant-owned record must have a clear tenant/organisation ownership path.
- Enforce tenant isolation in the backend and database strategy; UI filtering is not security.
- Never trust a tenant ID supplied by a client without deriving or validating it against the authenticated principal.
- Define role permissions at the operation/transition level, not only by hiding UI controls.
- Support organisation-specific roles only through an explicit permission model.
- Audit sensitive actions: policy changes, attendance corrections, payroll adjustments, approvals, pay-run finalisation, access changes, exports, and administrative overrides.
- Audit records must identify actor, tenant, action, target, timestamp, reason where required, and before/after values or a durable change reference.
- Do not log passwords, access tokens, secrets, or unnecessary sensitive personal data.

## 7. Documentation and repository discipline

First inspect the existing repository. Do not assume a directory layout or overwrite existing conventions.

Keep project artefacts version-controlled. A suggested documentation layout, to be adapted to the repository:

```text
docs/
  requirements/
    BRD.md
    TSD.md
    OPEN_QUESTIONS.md
  product/
    CAPABILITIES.md
    USER_STORIES/
    ACCEPTANCE_CRITERIA/
    BUSINESS_RULES.md
    TRACEABILITY.md
  domain/
    DOMAIN_MODEL.md
    STATE_MACHINES.md
    PERMISSION_MATRIX.md
    GLOSSARY.md
  database/
    DATA_MODEL.md
    DATA_DICTIONARY.md
    MIGRATIONS.md
  api/
    API_CONTRACTS/
    ERROR_CATALOG.md
  architecture/
    SERVICE_BOUNDARIES.md
    ADR/
  quality/
    TEST_STRATEGY.md
    TEST_CASES/
  operations/
    SECURITY.md
    DEPLOYMENT.md
    BACKUP_RECOVERY.md
    RUNBOOK.md
```

Repository rules:
- Inspect `README`, `CLAUDE.md`, existing skills, manifests, CI, migrations, and current code before proposing changes.
- Preserve established naming and structure unless there is an approved reason to change them.
- Keep architectural decisions in ADRs with context, decision, alternatives, and consequences.
- Update documentation when behaviour, contracts, schema, or operational procedures change.
- Do not create duplicate documents that disagree with one another.
- Keep changes small and focused. Do not refactor unrelated modules while implementing a feature.
- Never commit secrets, credentials, production data, or real employee personal information.

## 8. Traceability and definition of done

Maintain traceability:

**BRD/TSD requirement → capability → user story → acceptance criterion → business rule → domain concept → state transition → database constraint → API operation → implementation → automated test.**

A feature is not complete merely because its screen or endpoint exists.

Definition of done:
- Requirement and acceptance criteria are identified.
- Business rules and unresolved questions are addressed.
- Domain ownership and state transitions are respected.
- API contract is documented and implemented.
- Authorization and tenant isolation are tested.
- Database migrations are reviewed.
- Unit, integration, and relevant E2E tests pass.
- Error, empty, loading, retry, and conflict paths are handled.
- Audit and observability requirements are met.
- Documentation and traceability are updated.
- No unrelated regressions are introduced.

If a requirement cannot be traced or tested, flag it instead of claiming completion.

## 9. Approval gates and agent autonomy

The agent may work autonomously within an approved phase. It must stop before crossing these gates:

1. **Requirements gate:** stories, acceptance criteria, business rules, and scope.
2. **Domain gate:** entities, aggregates, service ownership, invariants, state machines, permissions.
3. **Database gate:** schema, tenancy, constraints, indexes, migrations.
4. **API gate:** contracts, errors, authorization, idempotency.
5. **Backend gate:** implementation and automated tests.
6. **Web gate:** web acceptance criteria and UAT.
7. **Mobile gate:** mobile acceptance criteria and UAT.
8. **Production gate:** security, performance, backup/recovery, monitoring, deployment.

At a gate, provide:
- What was produced
- Decisions made and their source
- Open questions and assumptions
- Validation/test results
- Risks and trade-offs
- Exact approval needed to continue

Do not cross a gate based on silence. Do not make irreversible production changes without explicit approval.

## 10. How to respond to a task

For every task:

1. Identify the relevant project phase.
2. Inspect the relevant source documents and repository files.
3. Identify dependencies and upstream decisions.
4. State any conflicts, missing requirements, or risky assumptions.
5. Propose the smallest complete deliverable for this phase.
6. Implement only if the phase is approved and the task authorises implementation.
7. Validate with tests or explicit checks.
8. Update relevant documentation and traceability.
9. Report files changed, decisions, tests, and remaining blockers.

When the user asks for implementation but required upstream artefacts are missing or contradictory, do not guess. Explain the blocker and prepare the missing decision/artefact first.

## 11. Prohibited shortcuts

Never:
- Build UI directly from an informal feature description when business behaviour is unresolved.
- Design tables from screens or expose every table as CRUD APIs by default.
- Put authoritative payroll or attendance calculations in frontend code.
- Infer check-in/check-out using punch parity.
- Mutate raw punch evidence to make reports look correct.
- Silently change historical payroll results or policy versions.
- Bypass tenant isolation or backend authorization.
- Create service boundaries based only on table names.
- Introduce a new technology or service without a concrete requirement and an ADR when architecturally significant.
- Treat a passing build as proof of business correctness.
- Claim legal/statutory compliance without current authoritative verification.
- Skip approval gates or execute destructive/production operations without approval.

## 12. Priority when instructions conflict

Apply this precedence:

1. Explicit latest user decision
2. Approved current BRD/TSD and signed-off project artefacts
3. Accepted architecture decision records
4. Existing repository implementation and conventions
5. This skill's defaults
6. General engineering preference

If two high-priority sources conflict, stop and surface the conflict. Do not silently resolve business or architecture decisions.
