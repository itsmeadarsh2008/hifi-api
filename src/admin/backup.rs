use std::collections::HashSet;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use sqlx::Row;

use crate::error::AppError;
use crate::AppState;

fn db_path(state: &AppState) -> Result<String, AppError> {
    let mut url = state.config.database_url.clone();
    if url.is_empty() || url == "ephemeral" {
        return Err(AppError::BadRequest(
            "No database configured (ephemeral mode) — nothing to back up".into(),
        ));
    }
    if let Some(rest) = url.strip_prefix("sqlite://") {
        url = rest.to_string();
    }
    if let Some((path, _)) = url.split_once('?') {
        url = path.to_string();
    }
    if url.is_empty() {
        return Err(AppError::BadRequest("Cannot determine database file path".into()));
    }
    Ok(url)
}

#[derive(Clone, Copy)]
enum ColType {
    Text,
    NullableText,
    Int,
    NullableInt,
}

/// Copy one table from the backup pool into the live transaction.
/// Only whitelisted columns are touched; columns missing from old backups
/// are skipped (live DEFAULTs apply). All identifiers are ours — the only
/// backup-derived input is row data.
async fn copy_table(
    src: &sqlx::SqlitePool,
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    table: &str,
    cols: &[(&str, ColType)],
) -> Result<usize, AppError> {
    let info: Vec<(i64, String, String, i64, Option<String>, i64)> =
        sqlx::query_as(&format!("PRAGMA table_info({})", table))
            .fetch_all(src)
            .await
            .map_err(|e| AppError::BadRequest(format!("Cannot read backup: {}", e)))?;
    let present: HashSet<&str> = info.iter().map(|c| c.1.as_str()).collect();
    let use_cols: Vec<(&str, ColType)> = cols
        .iter()
        .filter(|(name, _)| present.contains(name))
        .map(|(name, t)| (*name, *t))
        .collect();
    if use_cols.is_empty() {
        return Ok(0);
    }
    let names: Vec<&str> = use_cols.iter().map(|(n, _)| *n).collect();
    let placeholders = vec!["?"; names.len()].join(",");
    let select_sql = format!("SELECT {} FROM {}", names.join(","), table);
    let insert_sql = format!(
        "INSERT INTO {} ({}) VALUES ({})",
        table,
        names.join(","),
        placeholders
    );

    let rows = sqlx::query(&select_sql)
        .fetch_all(src)
        .await
        .map_err(|e| AppError::BadRequest(format!("Cannot read backup: {}", e)))?;

    sqlx::query(&format!("DELETE FROM {}", table))
        .execute(&mut **tx)
        .await
        .map_err(|e| AppError::Internal(format!("Restore failed: {}", e)))?;

        for row in &rows {
            let mut q = sqlx::query(&insert_sql);
        for (i, (_, ty)) in use_cols.iter().enumerate() {
            q = match ty {
                ColType::Text => {
                    let v: String = row.try_get(i).map_err(|e| {
                        AppError::BadRequest(format!("Cannot read backup: {}", e))
                    })?;
                    q.bind(v)
                }
                ColType::NullableText => {
                    let v: Option<String> = row.try_get(i).map_err(|e| {
                        AppError::BadRequest(format!("Cannot read backup: {}", e))
                    })?;
                    q.bind(v)
                }
                ColType::Int => {
                    let v: i64 = row.try_get(i).map_err(|e| {
                        AppError::BadRequest(format!("Cannot read backup: {}", e))
                    })?;
                    q.bind(v)
                }
                ColType::NullableInt => {
                    let v: Option<i64> = row.try_get(i).map_err(|e| {
                        AppError::BadRequest(format!("Cannot read backup: {}", e))
                    })?;
                    q.bind(v)
                }
            };
        }
        q.execute(&mut **tx)
            .await
            .map_err(|e| AppError::Internal(format!("Restore failed: {}", e)))?;
    }
    Ok(rows.len())
}

/// Download a consistent snapshot of the live database.
pub async fn download_backup(
    State(state): State<AppState>,
) -> Result<Response, AppError> {
    let db = state.db.as_ref().ok_or_else(|| {
        AppError::BadRequest("No database configured (ephemeral mode)".into())
    })?;
    let path = db_path(&state)?;

    let tmp = format!("{}.backup-{}", path, chrono::Utc::now().timestamp());
    sqlx::query(&format!("VACUUM INTO '{}'", tmp.replace('\'', "''")))
        .execute(db)
        .await
        .map_err(|e| AppError::Internal(format!("Backup failed: {}", e)))?;

    let bytes = tokio::fs::read(&tmp)
        .await
        .map_err(|e| AppError::Internal(format!("Backup read failed: {}", e)))?;
    let _ = tokio::fs::remove_file(&tmp).await;

    Ok((
        [
            (header::CONTENT_TYPE, "application/x-sqlite3"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"hifi-backup.db\"",
            ),
        ],
        bytes,
    )
        .into_response())
}

/// Restore from an uploaded SQLite file (raw bytes, not multipart).
/// Copies tables into the LIVE database, then reloads all in-memory state —
/// no restart needed. Rejects anything that isn't a hifi-api database.
pub async fn restore_backup(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<Json<Value>, AppError> {
    if body.len() < 100 || &body[..16] != b"SQLite format 3\0" {
        return Err(AppError::BadRequest("Not a SQLite database file".into()));
    }
    if body.len() > 50 * 1024 * 1024 {
        return Err(AppError::BadRequest("Backup file too large (max 50MB)".into()));
    }

    let db = state.db.as_ref().ok_or_else(|| {
        AppError::BadRequest("No database configured (ephemeral mode)".into())
    })?;

    // Open the upload separately and validate its schema first.
    let tmp = format!("/tmp/hifi-restore-{}.db", chrono::Utc::now().timestamp());
    tokio::fs::write(&tmp, &body)
        .await
        .map_err(|e| AppError::Internal(format!("Restore write failed: {}", e)))?;
    let src = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&format!("sqlite://{}?mode=ro", tmp))
        .await
        .map_err(|e| AppError::BadRequest(format!("Cannot open backup: {}", e)))?;

    let tables: Vec<(String,)> =
        sqlx::query_as("SELECT name FROM sqlite_master WHERE type='table'")
            .fetch_all(&src)
            .await
            .map_err(|e| AppError::BadRequest(format!("Cannot read backup: {}", e)))?;
    let names: Vec<&str> = tables.iter().map(|t| t.0.as_str()).collect();
    for required in ["accounts", "api_keys", "settings"] {
        if !names.contains(&required) {
            src.close().await;
            let _ = tokio::fs::remove_file(&tmp).await;
            return Err(AppError::BadRequest(format!(
                "Backup is missing the '{}' table — not a hifi-api database",
                required
            )));
        }
    }

    // Copy table contents into the live DB inside one transaction.
    use ColType::{Int, NullableInt, NullableText, Text};
    let mut tx = db
        .begin()
        .await
        .map_err(|e| AppError::Internal(format!("Restore failed: {}", e)))?;
    for (table, cols) in [
        (
            "accounts",
            &[
                ("id", Text),
                ("label", Text),
                ("client_id", Text),
                ("client_secret", Text),
                ("refresh_token", Text),
                ("user_id", NullableText),
                ("is_active", Int),
                ("auto_disabled", Int),
                ("notes", Text),
                ("created_at", Int),
                ("updated_at", Int),
            ][..],
        ),
        (
            "tokens",
            &[
                ("account_id", Text),
                ("access_token", Text),
                ("expires_at", Int),
                ("refreshed_at", Int),
            ][..],
        ),
        (
            "account_metrics",
            &[
                ("account_id", Text),
                ("request_count", Int),
                ("error_count", Int),
                ("rate_limit_hits", Int),
                ("last_used_at", NullableInt),
                ("last_error_at", NullableInt),
                ("last_error_message", NullableText),
            ][..],
        ),
        (
            "api_keys",
            &[
                ("id", Text),
                ("label", Text),
                ("key_hash", Text),
                ("key_prefix", Text),
                ("quota", Int),
                ("used", Int),
                ("is_active", Int),
                ("created_at", Int),
            ][..],
        ),
        ("settings", &[("key", Text), ("value", Text)][..]),
    ] {
        if !names.contains(&table) {
            continue;
        }
        copy_table(&src, &mut tx, table, cols).await?;
    }
    tx.commit()
        .await
        .map_err(|e| AppError::Internal(format!("Restore failed: {}", e)))?;
    src.close().await;
    let _ = tokio::fs::remove_file(&tmp).await;

    // Reload all in-memory state from the restored database.
    state.account_manager.reload_from_db().await?;
    state.api_keys.reload_from_db().await?;
    state.rate_limits.load_from_db(db).await;
    state.anti_ban.reload_limiter();

    let (healthy, total) = state.account_manager.healthy_count().await;
    Ok(Json(json!({
        "message": format!("Database restored ({} accounts, {}/{} healthy)", total, healthy, total),
        "accounts": total,
    })))
}
