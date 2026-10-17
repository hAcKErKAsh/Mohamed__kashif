-- Lyceum core schema.
-- Conventions: uuid keys, timestamptz everywhere, text + CHECK instead of enums
-- (enums are painful to evolve), jsonb only for genuinely schemaless data
-- (custom fields, workflow graphs, settings).

create extension if not exists ltree;
create extension if not exists citext;
create extension if not exists pgcrypto;

-- ---------------------------------------------------------------------------
-- Organisation tree. `path` is an ltree of id-derived labels so renames and
-- code changes never rewrite the tree; moves rewrite the subtree's prefix.
-- ---------------------------------------------------------------------------
create table org_units (
    id          uuid primary key default gen_random_uuid(),
    parent_id   uuid references org_units(id) on delete restrict,
    kind        text not null,
    name        text not null check (length(name) between 1 and 120),
    code        text not null check (code ~ '^[A-Za-z0-9_-]{1,32}$'),
    path        ltree not null unique,
    sort_order  int not null default 0,
    created_at  timestamptz not null default now(),
    unique (parent_id, code)
);
create index org_units_path_gist on org_units using gist (path);
create index org_units_parent on org_units (parent_id);
-- Exactly one root.
create unique index org_units_single_root on org_units ((true)) where parent_id is null;

-- ---------------------------------------------------------------------------
-- Identity
-- ---------------------------------------------------------------------------
create table users (
    id                    uuid primary key default gen_random_uuid(),
    email                 citext not null unique check (email ~ '^[^@\s]+@[^@\s]+\.[^@\s]+$'),
    full_name             text not null check (length(full_name) between 1 and 120),
    password_hash         text not null,
    status                text not null default 'active' check (status in ('active', 'disabled')),
    must_change_password  boolean not null default false,
    failed_logins         int not null default 0,
    locked_until          timestamptz,
    totp_secret           text,
    totp_enabled          boolean not null default false,
    profile               jsonb not null default '{}'::jsonb,
    created_at            timestamptz not null default now(),
    updated_at            timestamptz not null default now(),
    last_login_at         timestamptz,
    password_changed_at   timestamptz not null default now()
);

create table sessions (
    id            uuid primary key default gen_random_uuid(),
    user_id       uuid not null references users(id) on delete cascade,
    token_hash    bytea not null unique,
    csrf_token    text not null,
    mfa_pending   boolean not null default false,
    ip            text,
    user_agent    text,
    created_at    timestamptz not null default now(),
    last_seen_at  timestamptz not null default now(),
    expires_at    timestamptz not null,
    revoked_at    timestamptz
);
create index sessions_user on sessions (user_id) where revoked_at is null;

-- ---------------------------------------------------------------------------
-- Authorisation: roles carry permissions; a membership grants a role at a node
-- of the org tree and applies to that node's whole subtree.
-- ---------------------------------------------------------------------------
create table roles (
    id           uuid primary key default gen_random_uuid(),
    key          text not null unique check (key ~ '^[a-z][a-z0-9_]{1,31}$'),
    name         text not null,
    description  text not null default '',
    color        text not null default '#5b6b5d',
    is_system    boolean not null default false,
    created_at   timestamptz not null default now()
);

create table role_permissions (
    role_id     uuid not null references roles(id) on delete cascade,
    permission  text not null,
    primary key (role_id, permission)
);

create table memberships (
    id           uuid primary key default gen_random_uuid(),
    user_id      uuid not null references users(id) on delete cascade,
    role_id      uuid not null references roles(id) on delete restrict,
    org_unit_id  uuid not null references org_units(id) on delete cascade,
    created_by   uuid references users(id) on delete set null,
    created_at   timestamptz not null default now(),
    unique (user_id, role_id, org_unit_id)
);
create index memberships_user on memberships (user_id);
create index memberships_unit_role on memberships (org_unit_id, role_id);

create table guardianships (
    guardian_id  uuid not null references users(id) on delete cascade,
    student_id   uuid not null references users(id) on delete cascade,
    relation     text not null default 'guardian',
    primary key (guardian_id, student_id),
    check (guardian_id <> student_id)
);
create index guardianships_student on guardianships (student_id);

-- ---------------------------------------------------------------------------
-- Tamper-evident audit log: every row is hash-chained to its predecessor in
-- `seq` order, and the table rejects UPDATE/DELETE.
-- ---------------------------------------------------------------------------
create table audit_log (
    id           bigserial primary key,
    seq          bigint not null unique,
    at           timestamptz not null default now(),
    actor_id     uuid,
    action       text not null,
    entity_type  text not null,
    entity_id    text,
    ip           text,
    detail       jsonb not null default '{}'::jsonb,
    prev_hash    bytea not null,
    hash         bytea not null
);
create index audit_log_at on audit_log (at desc);
create index audit_log_actor on audit_log (actor_id, at desc);
create index audit_log_entity on audit_log (entity_type, entity_id);

create function audit_row_digest(prev bytea, r audit_log) returns bytea
language sql immutable as $$
    select digest(
        prev || convert_to(
            r.seq::text || '|' || coalesce(r.actor_id::text, '') || '|' || r.action || '|' ||
            r.entity_type || '|' || coalesce(r.entity_id, '') || '|' || coalesce(r.ip, '') || '|' ||
            r.detail::text || '|' || to_char(r.at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US'),
            'UTF8'),
        'sha256')
$$;

create function audit_chain() returns trigger language plpgsql as $$
declare
    last_seq  bigint;
    last_hash bytea;
begin
    -- Serialise writers so the chain has a single, gap-free order.
    perform pg_advisory_xact_lock(724301101);
    select seq, hash into last_seq, last_hash from audit_log order by seq desc limit 1;
    new.seq := coalesce(last_seq, 0) + 1;
    new.prev_hash := coalesce(last_hash, '\x00'::bytea);
    new.hash := audit_row_digest(new.prev_hash, new);
    return new;
end $$;

create trigger audit_chain_bi before insert on audit_log
    for each row execute function audit_chain();

create function audit_immutable() returns trigger language plpgsql as $$
begin
    raise exception 'audit_log is append-only';
end $$;

create trigger audit_no_update before update or delete on audit_log
    for each row execute function audit_immutable();
create trigger audit_no_truncate before truncate on audit_log
    for each statement execute function audit_immutable();

-- ---------------------------------------------------------------------------
-- Workflow / approval engine
-- ---------------------------------------------------------------------------
create table workflow_definitions (
    id            uuid primary key default gen_random_uuid(),
    key           text not null check (key ~ '^[a-z][a-z0-9_]{1,47}$'),
    version       int not null,
    name          text not null,
    description   text not null default '',
    graph         jsonb not null,
    status        text not null default 'draft' check (status in ('draft', 'published', 'retired')),
    created_by    uuid references users(id) on delete set null,
    created_at    timestamptz not null default now(),
    published_at  timestamptz,
    unique (key, version)
);
-- At most one published version per key.
create unique index workflow_one_published on workflow_definitions (key) where status = 'published';

create table approval_requests (
    id            uuid primary key default gen_random_uuid(),
    workflow_id   uuid not null references workflow_definitions(id) on delete restrict,
    subject_type  text not null,
    subject_id    uuid not null,
    title         text not null,
    data          jsonb not null default '{}'::jsonb,
    requester_id  uuid not null references users(id) on delete restrict,
    org_unit_id   uuid not null references org_units(id) on delete restrict,
    status        text not null default 'in_progress'
                  check (status in ('in_progress', 'approved', 'rejected', 'returned', 'cancelled')),
    round         int not null default 1,
    created_at    timestamptz not null default now(),
    updated_at    timestamptz not null default now(),
    completed_at  timestamptz
);
create index approval_requests_requester on approval_requests (requester_id, created_at desc);
create index approval_requests_subject on approval_requests (subject_type, subject_id);
create index approval_requests_status on approval_requests (status, created_at desc);

create table approval_steps (
    id            uuid primary key default gen_random_uuid(),
    request_id    uuid not null references approval_requests(id) on delete cascade,
    round         int not null,
    node_id       text not null,
    node_name     text not null,
    node_type     text not null,
    status        text not null check (status in ('active', 'waiting', 'approved', 'rejected', 'returned', 'cancelled', 'passed')),
    mode          text not null default 'any' check (mode in ('any', 'all', 'quorum')),
    quorum        int,
    due_at        timestamptz,
    escalated     boolean not null default false,
    activated_at  timestamptz not null default now(),
    completed_at  timestamptz
);
create index approval_steps_request on approval_steps (request_id, activated_at);

create table approval_tasks (
    id              uuid primary key default gen_random_uuid(),
    step_id         uuid not null references approval_steps(id) on delete cascade,
    request_id      uuid not null references approval_requests(id) on delete cascade,
    assignee_id     uuid not null references users(id) on delete restrict,
    status          text not null default 'pending'
                    check (status in ('pending', 'approved', 'rejected', 'returned', 'withdrawn')),
    comment         text,
    delegated_from  uuid references users(id) on delete set null,
    created_at      timestamptz not null default now(),
    decided_at      timestamptz,
    unique (step_id, assignee_id)
);
create index approval_tasks_inbox on approval_tasks (assignee_id, status, created_at desc);

create table approval_events (
    id          bigserial primary key,
    request_id  uuid not null references approval_requests(id) on delete cascade,
    at          timestamptz not null default now(),
    actor_id    uuid references users(id) on delete set null,
    kind        text not null,
    node_id     text,
    message     text,
    detail      jsonb not null default '{}'::jsonb
);
create index approval_events_request on approval_events (request_id, id);

-- ---------------------------------------------------------------------------
-- Content management
-- ---------------------------------------------------------------------------
create table content_items (
    id                   uuid primary key default gen_random_uuid(),
    kind                 text not null check (kind in ('announcement', 'circular', 'page')),
    title                text not null check (length(title) between 1 and 200),
    summary              text not null default '',
    body                 text not null default '',
    status               text not null default 'draft'
                         check (status in ('draft', 'in_review', 'published', 'rejected', 'archived')),
    org_unit_id          uuid not null references org_units(id) on delete restrict,
    author_id            uuid not null references users(id) on delete restrict,
    pinned               boolean not null default false,
    approval_request_id  uuid references approval_requests(id) on delete set null,
    published_at         timestamptz,
    created_at           timestamptz not null default now(),
    updated_at           timestamptz not null default now()
);
create index content_items_feed on content_items (status, published_at desc);

-- ---------------------------------------------------------------------------
-- Social
-- ---------------------------------------------------------------------------
create table posts (
    id                   uuid primary key default gen_random_uuid(),
    author_id            uuid not null references users(id) on delete cascade,
    org_unit_id          uuid not null references org_units(id) on delete cascade,
    body                 text not null check (length(body) between 1 and 5000),
    status               text not null default 'published'
                         check (status in ('pending', 'published', 'rejected', 'removed')),
    approval_request_id  uuid references approval_requests(id) on delete set null,
    created_at           timestamptz not null default now()
);
create index posts_feed on posts (status, created_at desc);
create index posts_author on posts (author_id, created_at desc);

create table post_reactions (
    post_id     uuid not null references posts(id) on delete cascade,
    user_id     uuid not null references users(id) on delete cascade,
    kind        text not null default 'appreciate' check (kind in ('appreciate', 'celebrate', 'insightful')),
    created_at  timestamptz not null default now(),
    primary key (post_id, user_id)
);

create table post_comments (
    id          uuid primary key default gen_random_uuid(),
    post_id     uuid not null references posts(id) on delete cascade,
    author_id   uuid not null references users(id) on delete cascade,
    body        text not null check (length(body) between 1 and 2000),
    removed     boolean not null default false,
    created_at  timestamptz not null default now()
);
create index post_comments_post on post_comments (post_id, created_at);

-- ---------------------------------------------------------------------------
-- Low-code: form builder, settings, notifications
-- ---------------------------------------------------------------------------
create table form_definitions (
    id             uuid primary key default gen_random_uuid(),
    key            text not null unique check (key ~ '^[a-z][a-z0-9_]{1,47}$'),
    name           text not null,
    description    text not null default '',
    fields         jsonb not null default '[]'::jsonb,
    workflow_key   text,
    allowed_roles  text[] not null default '{}',
    active         boolean not null default true,
    created_at     timestamptz not null default now(),
    updated_at     timestamptz not null default now()
);

create table form_submissions (
    id                   uuid primary key default gen_random_uuid(),
    form_id              uuid not null references form_definitions(id) on delete restrict,
    submitter_id         uuid not null references users(id) on delete restrict,
    org_unit_id          uuid not null references org_units(id) on delete restrict,
    data                 jsonb not null default '{}'::jsonb,
    status               text not null default 'submitted'
                         check (status in ('submitted', 'in_review', 'approved', 'rejected', 'returned', 'cancelled')),
    approval_request_id  uuid references approval_requests(id) on delete set null,
    created_at           timestamptz not null default now(),
    updated_at           timestamptz not null default now()
);
create index form_submissions_submitter on form_submissions (submitter_id, created_at desc);

create table settings (
    key         text primary key,
    value       jsonb not null,
    updated_by  uuid references users(id) on delete set null,
    updated_at  timestamptz not null default now()
);

create table notifications (
    id          uuid primary key default gen_random_uuid(),
    user_id     uuid not null references users(id) on delete cascade,
    kind        text not null,
    title       text not null,
    link        text,
    read_at     timestamptz,
    created_at  timestamptz not null default now()
);
create index notifications_user on notifications (user_id, created_at desc);
