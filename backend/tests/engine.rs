//! Engine behaviour against a real PostgreSQL database. Each test gets a fresh
//! database from `DATABASE_URL` (via `sqlx::test`), migrated and seeded.

use lyceum::{audit, engine, engine::Decision, seed};
use sqlx::PgPool;
use uuid::Uuid;

async fn user(db: &PgPool, handle: &str) -> Uuid {
    sqlx::query_scalar("select id from users where email = $1")
        .bind(format!("{handle}@northfield.school"))
        .fetch_one(db)
        .await
        .unwrap()
}

async fn request(db: &PgPool, title_like: &str) -> Uuid {
    sqlx::query_scalar("select id from approval_requests where title like $1")
        .bind(format!("%{title_like}%"))
        .fetch_one(db)
        .await
        .unwrap()
}

async fn task(db: &PgPool, req: Uuid, who: Uuid) -> Option<Uuid> {
    sqlx::query_scalar("select id from approval_tasks where request_id = $1 and assignee_id = $2 and status = 'pending'")
        .bind(req)
        .bind(who)
        .fetch_optional(db)
        .await
        .unwrap()
}

async fn status(db: &PgPool, req: Uuid) -> String {
    sqlx::query_scalar("select status from approval_requests where id = $1").bind(req).fetch_one(db).await.unwrap()
}

async fn active_stages(db: &PgPool, req: Uuid) -> Vec<String> {
    sqlx::query_scalar("select node_id from approval_steps where request_id = $1 and status = 'active' order by node_id")
        .bind(req)
        .fetch_all(db)
        .await
        .unwrap()
}

async fn decide(db: &PgPool, req: Uuid, who: &str, d: Decision) {
    let uid = user(db, who).await;
    let t = task(db, req, uid).await.unwrap_or_else(|| panic!("{who} has no pending task"));
    let mut tx = db.begin().await.unwrap();
    engine::decide(&mut tx, t, uid, d, Some("test".into())).await.unwrap();
    tx.commit().await.unwrap();
}

#[sqlx::test(migrations = "./migrations")]
async fn parallel_join_then_condition_routes_long_leave_to_coordinator(db: PgPool) {
    seed::run(&db).await.unwrap();
    let req = request(&db, "Aarav").await;
    // Guardian already consented in the seed; the teacher is still pending.
    assert_eq!(active_stages(&db, req).await, vec!["teacher"]);
    decide(&db, req, "grace", Decision::Approve).await;
    // 5 days > 3, so the decision routes to the wing coordinator.
    assert_eq!(active_stages(&db, req).await, vec!["coordinator"]);
    decide(&db, req, "meera", Decision::Approve).await;
    assert_eq!(status(&db, req).await, "approved");
    let sub: String = sqlx::query_scalar("select s.status from form_submissions s join approval_requests r on r.subject_id = s.id where r.id = $1")
        .bind(req)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(sub, "approved");
}

#[sqlx::test(migrations = "./migrations")]
async fn short_leave_skips_coordinator(db: PgPool) {
    seed::run(&db).await.unwrap();
    // Lily's one-day leave was fully approved in the seed without a coordinator stage.
    let req = request(&db, "Lily").await;
    assert_eq!(status(&db, req).await, "approved");
    let coordinator_steps: i64 = sqlx::query_scalar("select count(*) from approval_steps where request_id = $1 and node_id = 'coordinator'")
        .bind(req)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(coordinator_steps, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_rejection_in_one_branch_closes_the_request(db: PgPool) {
    seed::run(&db).await.unwrap();
    let req = request(&db, "Aarav").await;
    decide(&db, req, "grace", Decision::Reject).await;
    assert_eq!(status(&db, req).await, "rejected");
    assert!(active_stages(&db, req).await.is_empty());
    let pending: i64 = sqlx::query_scalar("select count(*) from approval_tasks where request_id = $1 and status = 'pending'")
        .bind(req)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(pending, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn quorum_panel_needs_two_votes(db: PgPool) {
    seed::run(&db).await.unwrap();
    let req = request(&db, "Hana").await;
    decide(&db, req, "tomas", Decision::Approve).await;
    // Budget 2400 > 1000: the leadership panel opens with a quorum of 2.
    assert_eq!(active_stages(&db, req).await, vec!["panel"]);
    decide(&db, req, "tomas", Decision::Approve).await;
    assert_eq!(status(&db, req).await, "in_progress", "one vote is not a quorum");
    decide(&db, req, "headmaster", Decision::Approve).await;
    assert_eq!(status(&db, req).await, "approved");
}

#[sqlx::test(migrations = "./migrations")]
async fn requester_is_never_their_own_approver(db: PgPool) {
    seed::run(&db).await.unwrap();
    // Meera is a coordinator; her own staff leave must not land with her.
    let meera = user(&db, "meera").await;
    let unit: Uuid = sqlx::query_scalar("select id from org_units where code = 'PRI'").fetch_one(&db).await.unwrap();
    let wf: Uuid = sqlx::query_scalar("select id from workflow_definitions where key = 'staff_leave'").fetch_one(&db).await.unwrap();
    let mut tx = db.begin().await.unwrap();
    let req = engine::start(&mut tx, wf, "test", Uuid::new_v4(), "Meera's leave", serde_json::json!({ "days": 1, "type": "Casual" }), meera, unit)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(task(&db, req, meera).await.is_none());
    // With no other coordinator above her, the stage falls back to administrators.
    let admin = user(&db, "admin").await;
    assert!(task(&db, req, admin).await.is_some());
}

#[sqlx::test(migrations = "./migrations")]
async fn send_back_and_resubmit_starts_a_new_round(db: PgPool) {
    seed::run(&db).await.unwrap();
    let req = request(&db, "Grace Lindqvist").await;
    decide(&db, req, "meera", Decision::Return).await;
    assert_eq!(status(&db, req).await, "returned");
    let grace = user(&db, "grace").await;
    let mut tx = db.begin().await.unwrap();
    engine::resubmit(&mut tx, req, grace, Some(serde_json::json!({ "type": "Casual", "start_date": "2026-11-03", "days": 1 })))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let round: i32 = sqlx::query_scalar("select round from approval_requests where id = $1").bind(req).fetch_one(&db).await.unwrap();
    assert_eq!(round, 2);
    decide(&db, req, "meera", Decision::Approve).await;
    // One casual day no longer needs the headmaster.
    assert_eq!(status(&db, req).await, "approved");
}

#[sqlx::test(migrations = "./migrations")]
async fn only_the_assignee_can_decide(db: PgPool) {
    seed::run(&db).await.unwrap();
    let req = request(&db, "Aarav").await;
    let grace = user(&db, "grace").await;
    let t = task(&db, req, grace).await.unwrap();
    let daniel = user(&db, "daniel").await;
    let mut tx = db.begin().await.unwrap();
    let err = engine::decide(&mut tx, t, daniel, Decision::Approve, None).await.unwrap_err();
    assert!(matches!(err, lyceum::error::AppError::Forbidden));
}

#[sqlx::test(migrations = "./migrations")]
async fn audit_log_is_chained_and_append_only(db: PgPool) {
    seed::run(&db).await.unwrap();
    for i in 0..5 {
        audit::record(&db, None, "test.event", "test", Some(i.to_string()), None, serde_json::json!({ "i": i })).await.unwrap();
    }
    let (rows, broken) = audit::verify_chain(&db).await.unwrap();
    assert!(rows >= 6);
    assert_eq!(broken, None);
    assert!(sqlx::query("update audit_log set action = 'forged' where seq = 2").execute(&db).await.is_err());
    assert!(sqlx::query("delete from audit_log").execute(&db).await.is_err());
}

#[test]
fn validation_rejects_bad_graphs() {
    let roles = ["teacher".to_string()].into_iter().collect();
    let g: engine::Graph = serde_json::from_value(serde_json::json!({
        "nodes": [
            { "id": "s", "type": "start" },
            { "id": "c", "type": "condition" },
            { "id": "e", "type": "end", "outcome": "approved" }
        ],
        "edges": [ { "from": "s", "to": "c" }, { "from": "c", "to": "e", "when": { "field": "x", "op": "eq", "value": 1 } } ]
    }))
    .unwrap();
    let errs = g.validate(&roles).unwrap_err();
    assert!(errs.iter().any(|e| e.contains("default path")));
}
