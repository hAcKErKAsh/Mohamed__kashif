//! Demo school for UAT. Runs only against an empty database.

use std::collections::HashMap;

use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{auth, authz::CATALOG, engine};

pub const DEMO_PASSWORD: &str = "Northfield#2026";

pub async fn run(db: &PgPool) -> anyhow::Result<()> {
    let users: i64 = sqlx::query_scalar("select count(*) from users").fetch_one(db).await?;
    if users > 0 {
        tracing::info!("database already has users; skipping demo seed");
        return Ok(());
    }
    tracing::info!("seeding demo school");
    let mut tx = db.begin().await?;

    // --- settings -----------------------------------------------------------
    for (k, v) in [
        ("branding", json!({ "name": "Northfield Academy", "tagline": "Est. 1962 · Learning in good company", "accent": "#2f5d50", "motto": "Lux et veritas" })),
        ("profile_fields", json!([
            { "key": "phone", "label": "Phone", "type": "text" },
            { "key": "blood_group", "label": "Blood group", "type": "select", "options": ["A+", "A-", "B+", "B-", "AB+", "AB-", "O+", "O-"] },
            { "key": "emergency_contact", "label": "Emergency contact", "type": "text" }
        ])),
    ] {
        sqlx::query("insert into settings (key, value) values ($1, $2)").bind(k).bind(v).execute(&mut *tx).await?;
    }

    // --- org tree -------------------------------------------------------------
    let mut units: HashMap<&str, (Uuid, String)> = HashMap::new();
    let tree: &[(&str, Option<&str>, &str, &str)] = &[
        ("NFA", None, "school", "Northfield Academy"),
        ("PRI", Some("NFA"), "wing", "Primary Wing"),
        ("SEN", Some("NFA"), "wing", "Senior Wing"),
        ("G4", Some("PRI"), "grade", "Grade 4"),
        ("G5", Some("PRI"), "grade", "Grade 5"),
        ("G9", Some("SEN"), "grade", "Grade 9"),
        ("4A", Some("G4"), "section", "Section 4-A"),
        ("4B", Some("G4"), "section", "Section 4-B"),
        ("5A", Some("G5"), "section", "Section 5-A"),
        ("9A", Some("G9"), "section", "Section 9-A"),
        ("9B", Some("G9"), "section", "Section 9-B"),
        ("ROBO", Some("SEN"), "club", "Robotics Club"),
    ];
    for (i, (code, parent, kind, name)) in tree.iter().enumerate() {
        let id = Uuid::new_v4();
        let label = format!("u{}", id.simple());
        let (parent_id, path) = match parent {
            Some(p) => {
                let (pid, ppath) = &units[p];
                (Some(*pid), format!("{ppath}.{label}"))
            }
            None => (None, label),
        };
        sqlx::query("insert into org_units (id, parent_id, kind, name, code, path, sort_order) values ($1, $2, $3, $4, $5, $6::ltree, $7)")
            .bind(id)
            .bind(parent_id)
            .bind(kind)
            .bind(name)
            .bind(code)
            .bind(&path)
            .bind(i as i32)
            .execute(&mut *tx)
            .await?;
        units.insert(code, (id, path));
    }

    // --- roles ----------------------------------------------------------------
    let all: Vec<&str> = CATALOG.iter().map(|p| p.key).collect();
    let teaching = vec![
        "people.view", "content.view", "content.create", "social.view", "social.post", "social.comment",
        "social.post_direct", "social.moderate", "forms.submit",
    ];
    let mut coordinator = teaching.clone();
    coordinator.extend(["approvals.view_all", "content.publish"]);
    let mut headmaster = coordinator.clone();
    headmaster.extend(["admin.audit.view"]);
    let roles: Vec<(&str, &str, &str, &str, bool, Vec<&str>)> = vec![
        ("administrator", "Administrator", "Security consultant: owns identity, roles, policy and the audit trail.", "#7a2e2e", true, all),
        ("headmaster", "Headmaster", "Leads the school; final approver for school-wide matters.", "#2f5d50", true, headmaster),
        ("coordinator", "Wing coordinator", "Runs a wing; approves longer leave and wing notices.", "#8a5a1f", false, coordinator),
        ("teacher", "Teacher", "Teaches a section; first approver for student matters.", "#3b5a7a", true, teaching),
        ("parent", "Parent", "Guardian of one or more students.", "#6b4f7a", true, vec!["content.view", "social.view", "social.comment", "forms.submit"]),
        ("student", "Student", "Enrolled learner.", "#55704a", true, vec!["content.view", "social.view", "social.post", "social.comment", "forms.submit"]),
    ];
    let mut role_ids: HashMap<&str, Uuid> = HashMap::new();
    for (rank, (key, name, desc, color, system, perms)) in roles.into_iter().enumerate() {
        let id: Uuid = sqlx::query_scalar("insert into roles (key, name, description, color, is_system, rank) values ($1, $2, $3, $4, $5, $6) returning id")
            .bind(key)
            .bind(name)
            .bind(desc)
            .bind(color)
            .bind(system)
            .bind((rank as i32 + 1) * 10)
            .fetch_one(&mut *tx)
            .await?;
        for p in perms {
            sqlx::query("insert into role_permissions (role_id, permission) values ($1, $2)").bind(id).bind(p).execute(&mut *tx).await?;
        }
        role_ids.insert(key, id);
    }

    // --- people ---------------------------------------------------------------
    let hash = auth::hash_password(DEMO_PASSWORD.to_string()).await?;
    let people: &[(&str, &str, &str, &str)] = &[
        ("admin", "Amira Haddad", "administrator", "NFA"),
        ("headmaster", "Dr. Rafael Okafor", "headmaster", "NFA"),
        ("meera", "Meera Pillai", "coordinator", "PRI"),
        ("tomas", "Tomás Varga", "coordinator", "SEN"),
        ("grace", "Grace Lindqvist", "teacher", "4A"),
        ("daniel", "Daniel Mensah", "teacher", "4B"),
        ("hana", "Hana Sato", "teacher", "9A"),
        ("aarav", "Aarav Shah", "student", "4A"),
        ("lily", "Lily Chen", "student", "4A"),
        ("noah", "Noah Williams", "student", "9A"),
        ("priya", "Priya Shah", "parent", "4A"),
        ("wei", "Wei Chen", "parent", "4A"),
        ("sarah", "Sarah Williams", "parent", "9A"),
    ];
    let mut uid: HashMap<&str, Uuid> = HashMap::new();
    for (handle, name, role, unit) in people {
        let id: Uuid = sqlx::query_scalar(
            "insert into users (email, full_name, password_hash, last_login_at) values ($1, $2, $3, now() - (random() * interval '3 days')) returning id",
        )
        .bind(format!("{handle}@northfield.school"))
        .bind(name)
        .bind(&hash)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query("insert into memberships (user_id, role_id, org_unit_id) values ($1, $2, $3)")
            .bind(id)
            .bind(role_ids[role])
            .bind(units[unit].0)
            .execute(&mut *tx)
            .await?;
        uid.insert(handle, id);
    }
    // Hana also advises the robotics club.
    sqlx::query("insert into memberships (user_id, role_id, org_unit_id) values ($1, $2, $3)")
        .bind(uid["hana"])
        .bind(role_ids["teacher"])
        .bind(units["ROBO"].0)
        .execute(&mut *tx)
        .await?;
    for (g, s, rel) in [("priya", "aarav", "mother"), ("wei", "lily", "father"), ("sarah", "noah", "mother")] {
        sqlx::query("insert into guardianships (guardian_id, student_id, relation) values ($1, $2, $3)")
            .bind(uid[g])
            .bind(uid[s])
            .bind(rel)
            .execute(&mut *tx)
            .await?;
    }

    // --- workflows ------------------------------------------------------------
    for (key, name, desc, graph) in workflows() {
        sqlx::query(
            "insert into workflow_definitions (key, version, name, description, graph, status, created_by, published_at)
             values ($1, 1, $2, $3, $4, 'published', $5, now())",
        )
        .bind(key)
        .bind(name)
        .bind(desc)
        .bind(graph)
        .bind(uid["admin"])
        .execute(&mut *tx)
        .await?;
    }

    // --- forms ----------------------------------------------------------------
    let forms: Vec<(&str, &str, &str, Value, &str, Vec<&str>)> = vec![
        (
            "student_leave", "Student leave", "Ask for time away from school. Your guardian and class teacher both confirm; longer absences go to the wing coordinator.",
            json!([
                { "key": "start_date", "label": "First day away", "type": "date", "required": true },
                { "key": "days", "label": "School days", "type": "number", "required": true, "min": 1, "max": 30 },
                { "key": "reason", "label": "Reason", "type": "select", "required": true, "options": ["Medical", "Family", "Travel", "Competition", "Other"] },
                { "key": "details", "label": "Anything the school should know", "type": "textarea" }
            ]),
            "student_leave", vec!["student"],
        ),
        (
            "staff_leave", "Staff leave", "Request leave. Your coordinator approves; three days or more, or medical leave, also goes to the headmaster.",
            json!([
                { "key": "type", "label": "Leave type", "type": "select", "required": true, "options": ["Casual", "Medical", "Professional development"] },
                { "key": "start_date", "label": "Starting", "type": "date", "required": true },
                { "key": "days", "label": "Working days", "type": "number", "required": true, "min": 0.5, "max": 60 },
                { "key": "cover_arranged", "label": "I have arranged cover for my classes", "type": "checkbox" },
                { "key": "notes", "label": "Notes", "type": "textarea" }
            ]),
            "staff_leave", vec!["teacher", "coordinator"],
        ),
        (
            "event_request", "Event or trip proposal", "Propose an event, trip or purchase. Budgets above 1,000 need two senior sign-offs.",
            json!([
                { "key": "event", "label": "What is it?", "type": "text", "required": true },
                { "key": "date", "label": "Date", "type": "date", "required": true },
                { "key": "budget", "label": "Budget", "type": "number", "required": true, "min": 0 },
                { "key": "students", "label": "Students involved", "type": "number", "min": 0 },
                { "key": "plan", "label": "Plan and safety notes", "type": "textarea", "required": true }
            ]),
            "event_request", vec!["teacher", "coordinator"],
        ),
    ];
    let mut form_ids: HashMap<&str, Uuid> = HashMap::new();
    for (key, name, desc, fields, wf, roles) in forms {
        let id: Uuid = sqlx::query_scalar(
            "insert into form_definitions (key, name, description, fields, workflow_key, allowed_roles) values ($1, $2, $3, $4, $5, $6) returning id",
        )
        .bind(key)
        .bind(name)
        .bind(desc)
        .bind(fields)
        .bind(wf)
        .bind(roles.iter().map(|s| s.to_string()).collect::<Vec<_>>())
        .fetch_one(&mut *tx)
        .await?;
        form_ids.insert(key, id);
    }

    // --- content --------------------------------------------------------------
    let content: &[(&str, &str, &str, &str, &str, &str, bool, i32)] = &[
        ("announcement", "Term two begins Monday", "Timetables, the new late-arrival desk, and what to bring.",
         "Welcome back.\n\nTerm two starts on **Monday at 8:10**. Timetables are on the noticeboard and in each section's channel.\n\n- The late-arrival desk moves to the east entrance.\n- Lockers will be re-assigned in the first week.\n- Please label water bottles and lunch boxes.\n\nWe're glad to have everyone back.",
         "NFA", "headmaster", true, 2),
        ("circular", "Annual sports day: volunteers wanted", "We need twenty parent volunteers for timing and first aid.",
         "Sports day is on the last Friday of the month. If you can help with **timing, first aid or refreshments**, reply through the community feed or tell your section teacher.",
         "NFA", "headmaster", false, 4),
        ("announcement", "Grade 4 museum visit", "Permission slips are due Thursday.",
         "Grade 4 will visit the Natural History Museum next Wednesday. Buses leave at 9:00 and return by 14:30. Please send a packed lunch.",
         "G4", "meera", false, 1),
        ("announcement", "Robotics club qualifies for regionals", "Twelve weeks of work, one very determined robot.",
         "Congratulations to the Robotics Club, who placed second at the city round and qualify for regionals. Practice moves to Tuesdays and Thursdays.",
         "SEN", "tomas", false, 0),
    ];
    for (kind, title, summary, body, unit, author, pinned, days_ago) in content {
        sqlx::query(
            "insert into content_items (kind, title, summary, body, org_unit_id, author_id, pinned, status, published_at, created_at)
             values ($1, $2, $3, $4, $5, $6, $7, 'published', now() - make_interval(days => $8), now() - make_interval(days => $8))",
        )
        .bind(kind)
        .bind(title)
        .bind(summary)
        .bind(body)
        .bind(units[unit].0)
        .bind(uid[author])
        .bind(pinned)
        .bind(days_ago)
        .execute(&mut *tx)
        .await?;
    }

    // --- community ------------------------------------------------------------
    let posts: &[(&str, &str, &str, i32)] = &[
        ("headmaster", "NFA", "A quiet thank-you to the grounds team, who had the field ready two days early after last week's storm. Sports day is on.", 30),
        ("grace", "4A", "4-A built a working water filter from sand, gravel and charcoal today. The before-and-after jars are on the windowsill if you're picking up.", 20),
        ("hana", "ROBO", "Regionals prep: we need two more volunteers for the pit crew. No experience needed, just steady hands.", 12),
        ("noah", "9A", "Does anyone have last year's chemistry practical notes? Happy to swap for my physics ones.", 5),
    ];
    let mut post_ids = Vec::new();
    for (author, unit, body, hours_ago) in posts {
        let id: Uuid = sqlx::query_scalar(
            "insert into posts (author_id, org_unit_id, body, created_at) values ($1, $2, $3, now() - make_interval(hours => $4)) returning id",
        )
        .bind(uid[author])
        .bind(units[unit].0)
        .bind(body)
        .bind(hours_ago)
        .fetch_one(&mut *tx)
        .await?;
        post_ids.push(id);
    }
    for (post, user, kind) in [(0, "meera", "appreciate"), (0, "grace", "appreciate"), (0, "priya", "celebrate"), (1, "priya", "insightful"), (1, "wei", "appreciate"), (2, "noah", "celebrate")] {
        sqlx::query("insert into post_reactions (post_id, user_id, kind) values ($1, $2, $3)")
            .bind(post_ids[post])
            .bind(uid[user])
            .bind(kind)
            .execute(&mut *tx)
            .await?;
    }
    for (post, user, body) in [
        (1, "priya", "Aarav would not stop talking about this at dinner. Thank you!"),
        (2, "noah", "I can help on Thursdays."),
        (0, "tomas", "Seconded. The senior wing noticed too."),
    ] {
        sqlx::query("insert into post_comments (post_id, author_id, body) values ($1, $2, $3)")
            .bind(post_ids[post])
            .bind(uid[user])
            .bind(body)
            .execute(&mut *tx)
            .await?;
    }

    // --- live requests through the real engine ---------------------------------
    let submit = |form: &'static str, who: &'static str, unit: &'static str, data: Value, title: &'static str| (form, who, unit, data, title);
    let requests = [
        submit("student_leave", "aarav", "4A", json!({ "start_date": "2026-10-26", "days": 5, "reason": "Family", "details": "Grandmother's 80th birthday in Pune." }), "Student leave · Aarav Shah"),
        submit("student_leave", "lily", "4A", json!({ "start_date": "2026-10-20", "days": 1, "reason": "Medical", "details": "Dentist appointment." }), "Student leave · Lily Chen"),
        submit("staff_leave", "grace", "4A", json!({ "type": "Professional development", "start_date": "2026-11-03", "days": 3, "cover_arranged": true, "notes": "Science pedagogy workshop." }), "Staff leave · Grace Lindqvist"),
        submit("event_request", "hana", "ROBO", json!({ "event": "Regional robotics championship", "date": "2026-11-14", "budget": 2400, "students": 8, "plan": "Coach hire, two staff chaperones, team registration and spare parts." }), "Event or trip proposal · Hana Sato"),
    ];
    let mut request_ids = Vec::new();
    for (form, who, unit, data, title) in requests {
        let sub: Uuid = sqlx::query_scalar("insert into form_submissions (form_id, submitter_id, org_unit_id, data) values ($1, $2, $3, $4) returning id")
            .bind(form_ids[form])
            .bind(uid[who])
            .bind(units[unit].0)
            .bind(&data)
            .fetch_one(&mut *tx)
            .await?;
        let wf = engine::published_workflow(&mut tx, form).await?.expect("seeded workflow");
        let rid = engine::start(&mut tx, wf, "form", sub, title, data, uid[who], units[unit].0).await?;
        sqlx::query("update form_submissions set approval_request_id = $2 where id = $1").bind(sub).bind(rid).execute(&mut *tx).await?;
        request_ids.push(rid);
    }
    // Lily's dentist visit: Wei and Grace both approve, so it completes.
    for who in ["wei", "grace"] {
        let task: Uuid = sqlx::query_scalar("select id from approval_tasks where request_id = $1 and assignee_id = $2 and status = 'pending'")
            .bind(request_ids[1])
            .bind(uid[who])
            .fetch_one(&mut *tx)
            .await?;
        engine::decide(&mut tx, task, uid[who], engine::Decision::Approve, Some("Approved — get well soon.".into())).await?;
    }
    // Aarav's leave: his mother has consented; Grace still has to decide.
    let task: Uuid = sqlx::query_scalar("select id from approval_tasks where request_id = $1 and assignee_id = $2 and status = 'pending'")
        .bind(request_ids[0])
        .bind(uid["priya"])
        .fetch_one(&mut *tx)
        .await?;
    engine::decide(&mut tx, task, uid["priya"], engine::Decision::Approve, Some("We'll make sure homework is done.".into())).await?;

    sqlx::query("insert into audit_log (actor_id, action, entity_type, detail) values (null, 'system.seeded', 'system', $1)")
        .bind(json!({ "school": "Northfield Academy" }))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    tracing::info!("demo ready — sign in as admin@northfield.school / {DEMO_PASSWORD}");
    Ok(())
}

fn workflows() -> Vec<(&'static str, &'static str, &'static str, Value)> {
    vec![
        (
            "student_leave",
            "Student leave",
            "Guardian and class teacher confirm in parallel; absences over three days also go to the wing coordinator.",
            json!({
                "nodes": [
                    { "id": "start", "type": "start", "name": "Submitted" },
                    { "id": "split", "type": "parallel", "name": "Ask both" },
                    { "id": "guardian", "type": "approval", "name": "Guardian consent", "approvers": { "kind": "guardians" }, "mode": "any" },
                    { "id": "teacher", "type": "approval", "name": "Class teacher", "approvers": { "kind": "role", "role": "teacher", "search": "nearest" }, "mode": "any", "sla_hours": 24, "escalate_to": "coordinator" },
                    { "id": "join", "type": "join", "name": "Both agreed" },
                    { "id": "length", "type": "condition", "name": "Longer than 3 days?" },
                    { "id": "coordinator", "type": "approval", "name": "Wing coordinator", "approvers": { "kind": "role", "role": "coordinator", "search": "nearest" }, "mode": "any", "sla_hours": 48 },
                    { "id": "approved", "type": "end", "name": "Approved", "outcome": "approved" },
                    { "id": "declined", "type": "end", "name": "Declined", "outcome": "rejected" }
                ],
                "edges": [
                    { "from": "start", "to": "split" },
                    { "from": "split", "to": "guardian" },
                    { "from": "split", "to": "teacher" },
                    { "from": "guardian", "to": "join", "on": "approved" },
                    { "from": "guardian", "to": "declined", "on": "rejected" },
                    { "from": "teacher", "to": "join", "on": "approved" },
                    { "from": "teacher", "to": "declined", "on": "rejected" },
                    { "from": "join", "to": "length" },
                    { "from": "length", "to": "coordinator", "when": { "field": "days", "op": "gt", "value": 3 }, "label": "More than 3 days" },
                    { "from": "length", "to": "approved", "default": true, "label": "3 days or fewer" },
                    { "from": "coordinator", "to": "approved", "on": "approved" },
                    { "from": "coordinator", "to": "declined", "on": "rejected" }
                ]
            }),
        ),
        (
            "staff_leave",
            "Staff leave",
            "Coordinator approves; three or more days, or any medical leave, also needs the headmaster.",
            json!({
                "nodes": [
                    { "id": "start", "type": "start", "name": "Submitted" },
                    { "id": "coordinator", "type": "approval", "name": "Wing coordinator", "approvers": { "kind": "role", "role": "coordinator", "search": "nearest" }, "sla_hours": 48, "escalate_to": "headmaster" },
                    { "id": "check", "type": "condition", "name": "Needs the headmaster?" },
                    { "id": "head", "type": "approval", "name": "Headmaster", "approvers": { "kind": "role", "role": "headmaster", "search": "nearest" } },
                    { "id": "approved", "type": "end", "name": "Approved", "outcome": "approved" }
                ],
                "edges": [
                    { "from": "start", "to": "coordinator" },
                    { "from": "coordinator", "to": "check", "on": "approved" },
                    { "from": "check", "to": "head", "label": "3+ days or medical",
                      "when": { "any": [ { "field": "days", "op": "gte", "value": 3 }, { "field": "type", "op": "eq", "value": "Medical" } ] } },
                    { "from": "check", "to": "approved", "default": true, "label": "Short leave" },
                    { "from": "head", "to": "approved", "on": "approved" }
                ]
            }),
        ),
        (
            "event_request",
            "Event and spending",
            "Coordinator reviews; budgets over 1,000 need 2 of the senior leadership (coordinators + headmaster) to agree.",
            json!({
                "nodes": [
                    { "id": "start", "type": "start", "name": "Proposed" },
                    { "id": "coordinator", "type": "approval", "name": "Wing coordinator", "approvers": { "kind": "role", "role": "coordinator", "search": "nearest" } },
                    { "id": "budget", "type": "condition", "name": "Budget over 1,000?" },
                    { "id": "panel", "type": "approval", "name": "Leadership panel (2 of N)", "approvers": { "kind": "roles", "roles": ["headmaster", "coordinator"], "search": "all_ancestors" }, "mode": "quorum", "quorum": 2 },
                    { "id": "approved", "type": "end", "name": "Approved", "outcome": "approved" }
                ],
                "edges": [
                    { "from": "start", "to": "coordinator" },
                    { "from": "coordinator", "to": "budget", "on": "approved" },
                    { "from": "budget", "to": "panel", "when": { "field": "budget", "op": "gt", "value": 1000 }, "label": "Over 1,000" },
                    { "from": "budget", "to": "approved", "default": true, "label": "Within budget" },
                    { "from": "panel", "to": "approved", "on": "approved" }
                ]
            }),
        ),
        (
            "content_publish",
            "Publish content",
            "School-wide notices go to the headmaster; everything else to the nearest coordinator.",
            json!({
                "nodes": [
                    { "id": "start", "type": "start", "name": "Submitted" },
                    { "id": "audience", "type": "condition", "name": "Whole school?" },
                    { "id": "head", "type": "approval", "name": "Headmaster", "approvers": { "kind": "role", "role": "headmaster", "search": "nearest" } },
                    { "id": "coordinator", "type": "approval", "name": "Coordinator", "approvers": { "kind": "role", "role": "coordinator", "search": "nearest" } },
                    { "id": "published", "type": "end", "name": "Published", "outcome": "approved" }
                ],
                "edges": [
                    { "from": "start", "to": "audience" },
                    { "from": "audience", "to": "head", "when": { "field": "audience_kind", "op": "eq", "value": "school" }, "label": "School-wide" },
                    { "from": "audience", "to": "coordinator", "default": true, "label": "Wing or smaller" },
                    { "from": "head", "to": "published", "on": "approved" },
                    { "from": "coordinator", "to": "published", "on": "approved" }
                ]
            }),
        ),
        (
            "post_moderation",
            "Post moderation",
            "Student posts are checked by the nearest teacher before they appear.",
            json!({
                "nodes": [
                    { "id": "start", "type": "start", "name": "Posted" },
                    { "id": "teacher", "type": "approval", "name": "Teacher review", "approvers": { "kind": "role", "role": "teacher", "search": "nearest" }, "sla_hours": 12, "escalate_to": "coordinator" },
                    { "id": "live", "type": "end", "name": "Published", "outcome": "approved" }
                ],
                "edges": [
                    { "from": "start", "to": "teacher" },
                    { "from": "teacher", "to": "live", "on": "approved" }
                ]
            }),
        ),
    ]
}
