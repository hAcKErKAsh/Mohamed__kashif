use serde_json::Value;
use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use crate::error::AppResult;

/// Append to the hash-chained audit log. Takes any executor so callers can
/// record inside the same transaction as the change being audited.
pub async fn record<'e>(
    db: impl PgExecutor<'e>,
    actor: Option<Uuid>,
    action: &str,
    entity_type: &str,
    entity_id: Option<String>,
    ip: Option<&str>,
    detail: Value,
) -> AppResult<()> {
    sqlx::query(
        "insert into audit_log (actor_id, action, entity_type, entity_id, ip, detail) values ($1, $2, $3, $4, $5, $6)",
    )
    .bind(actor)
    .bind(action)
    .bind(entity_type)
    .bind(entity_id)
    .bind(ip)
    .bind(detail)
    .execute(db)
    .await?;
    Ok(())
}

/// Recompute the whole chain. Returns the first broken `seq`, if any.
pub async fn verify_chain(db: &PgPool) -> AppResult<(i64, Option<i64>)> {
    let row: (i64, Option<i64>) = sqlx::query_as(
        r#"with ordered as (
               select a.seq, a.hash, a.prev_hash,
                      lag(a.hash) over (order by a.seq) as expected_prev,
                      audit_row_digest(a.prev_hash, a) as recomputed
               from audit_log a
           )
           select count(*)::bigint,
                  min(seq) filter (
                      where recomputed <> hash
                         or (expected_prev is not null and expected_prev <> prev_hash)
                         or (expected_prev is null and prev_hash <> '\x00'::bytea))
           from ordered"#,
    )
    .fetch_one(db)
    .await?;
    Ok(row)
}
