# Lyceum — school operating system

A secure, configurable school management platform: identity and access control
for a security administrator, an organisation tree, a multi-stage, multi-path
approval engine, notices, a moderated community feed, and no-code forms.

**Live demo (runs in your browser, read-only):** https://hackerkash.github.io/mohamed__kashif/
Pick any of the six demo people on the sign-in screen.

| Layer    | Choice                                                    |
| -------- | --------------------------------------------------------- |
| Server   | Rust · Axum · SQLx · Tokio                                |
| Database | PostgreSQL 16 (`ltree` for the tree, append-only audit)   |
| Web      | React 18 · TypeScript · TanStack Query · hand-written CSS |
| Ops      | Single binary serves API + SPA · Docker · GitHub Actions  |

---

## What's inside

### Security (the "control room")
- **Argon2id** password hashing, length and common-password policy, forced change of one-time passwords.
- **Account lockout** after repeated failures; per-IP and per-account sign-in throttling.
- **Opaque session tokens** (256-bit, stored only as SHA-256) in `HttpOnly; SameSite=Strict` cookies, idle and absolute timeouts, sign-out-everywhere, live session list with revoke.
- **CSRF** synchroniser token on every state change, plus Origin checks.
- **TOTP two-step sign-in** — optional, or mandatory per role by policy. The session rotates after the second factor.
- **Scoped RBAC**: a role is granted *at a node of the organisation tree* and applies to its whole subtree. Every endpoint checks permission against the target's position in the tree.
- **No privilege escalation**: you can only grant a role whose permissions you already hold at that node. The last top-level administrator can't be removed, and the administrator role can't lose its core powers.
- **Tamper-evident audit log**: every sign-in, grant, setting change and decision is hash-chained (SHA-256) in PostgreSQL. A trigger blocks UPDATE, DELETE and TRUNCATE, and the integrity can be verified from the UI.
- Strict **Content-Security-Policy**, `X-Frame-Options: DENY`, `nosniff`, HSTS (when TLS), no-store on the API, body size limits, and generic error messages (internals are logged, never returned).

### Organisation tree
School → wings → grades → sections, plus clubs, houses or anything else (the unit kinds are configurable).
Moving a branch rewrites the subtree in one statement, and approval rules search *up* the tree to find the right person.

### Approval engine — multi-stage, multi-path
Workflows are versioned DAGs designed visually in the UI:

| Step            | Behaviour                                                                                           |
| --------------- | --------------------------------------------------------------------------------------------------- |
| Approval stage  | Approvers resolved at activation: a role (nearest / anywhere above / at a unit kind), a panel of several roles, named people, or the requester's guardians. Votes: **any one**, **everyone**, or a **quorum of N**. |
| Decision        | Routes by request data with `all` / `any` / `not` rules (`gt`, `in`, `contains`…).                   |
| Parallel / Wait | Fork several stages at once, then join when they all finish.                                        |
| End             | Approved or declined.                                                                               |

Also included: time limits with **escalation** to another role, **send back for changes** and resubmit (a new round), cancel, fallback when nobody holds a role, and **no self-approval** — the requester is never asked to approve their own request.
Published versions are frozen: requests already under way finish on the version they started with.
A **test run** shows exactly which path a request would take and who would be asked, before you publish.

### Content, community, low-code
- **Notices** (announcements, circulars, pages) addressed to any branch of the tree. Authors without publish rights go through the publishing workflow.
- **Community feed**: posts, comments and reactions with audience scoping. Posts from roles without "post without moderation" go through a moderation workflow before they appear.
- **Form builder**: fields, validation, who may submit, and which workflow approves it — with a live preview. Workflow rules can use any field.
- **Modules & policy**: switch modules on or off, set branding (name, motto, accent colour), security policy, publishing workflows, custom profile fields and unit kinds — all from the UI, all audited.

---

## Run it

### Option 1 — Docker (one command)
```bash
docker compose up --build
```
Open http://localhost:8080 and sign in as **admin@northfield.school** / **Northfield#2026**.
Every demo account uses the same password: `headmaster`, `meera`, `tomas`, `grace`, `daniel`, `hana`, `aarav`, `lily`, `noah`, `priya`, `wei`, `sarah` `@northfield.school`.

### Option 2 — GitHub Codespaces
On GitHub, click **Code → Codespaces → Create codespace**. The container installs everything, builds the app and starts it on port 8080.

### Option 3 — Local development
Requirements: Rust (stable), Node 22, PostgreSQL 16.
```bash
createdb lyceum                                   # any empty database
cp .env.example backend/.env                      # then edit DATABASE_URL
(cd web && npm ci && npm run build)               # or `npm run dev` for hot reload on :5173
(cd backend && cargo run -- --seed)               # migrates, seeds the demo school, serves :8080
```
Use `npm run dev` in `web/` while working on the UI; it proxies `/api` to the Rust server.

### Tests
```bash
cd backend && DATABASE_URL=postgres://user:pass@localhost/postgres cargo test
```
The integration tests create a throwaway database for each test. They cover parallel joins, conditional routing, quorum votes, rejection short-circuiting, send-back and resubmit, no self-approval, assignee-only decisions, and the audit chain's immutability.

---

## Configuration

| Variable          | Default                         | Purpose                                                     |
| ----------------- | ------------------------------- | ----------------------------------------------------------- |
| `DATABASE_URL`    | `postgres://campus:campus_dev@localhost/campus` | PostgreSQL connection                       |
| `BIND`            | `0.0.0.0:8080`                  | Listen address                                              |
| `STATIC_DIR`      | `../web/dist`                   | Built web app                                               |
| `SECURE_COOKIES`  | `false`                         | Set `true` behind HTTPS (Secure cookies + HSTS)             |
| `TRUST_PROXY`     | `false`                         | Read client IP from `X-Forwarded-For` (only behind your proxy) |
| `ALLOWED_ORIGINS` | —                               | Extra origins allowed to make changes                       |
| `SEED_DEMO`       | —                               | `1` loads the demo school into an empty database            |

## Project layout
```
backend/
  migrations/      PostgreSQL schema (tree, RBAC, audit chain, workflows, content, forms)
  src/auth.rs      passwords, sessions, CSRF, rate limiting, TOTP
  src/authz.rs     permission catalog and tree-scoped checks
  src/engine.rs    the approval engine (graph model, validation, runtime)
  src/routes/      HTTP API
  src/seed.rs      Northfield Academy demo school
  tests/           database integration tests
web/
  src/pages/       everyday screens + admin control room
  src/components/  design system, workflow diagram, form renderer
  src/demo/        in-browser replay used by the GitHub Pages demo
```

## Toward production
This build targets UAT with a handful of users. Before a wider rollout:
- Run behind a TLS-terminating proxy with `SECURE_COOKIES=true` and `TRUST_PROXY=true`.
- Move the rate limiter to Redis (or the proxy) if you run more than one instance.
- Add email or SMS delivery for notifications and password resets.
- Take managed PostgreSQL backups and send logs to central storage.
- Add file attachments (object storage) and a mobile-friendly PWA manifest.
