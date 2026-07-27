use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;

pub async fn init_pool(database_url: &str) -> Result<SqlitePool, sqlx::Error> {
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS accounts (
            id TEXT PRIMARY KEY,
            label TEXT NOT NULL DEFAULT '',
            client_id TEXT NOT NULL,
            client_secret TEXT NOT NULL,
            refresh_token TEXT NOT NULL,
            user_id TEXT,
            is_active INTEGER NOT NULL DEFAULT 1,
            notes TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        )",
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS tokens (
            account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
            access_token TEXT NOT NULL,
            expires_at INTEGER NOT NULL,
            refreshed_at INTEGER NOT NULL
        )",
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS account_metrics (
            account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
            request_count INTEGER NOT NULL DEFAULT 0,
            error_count INTEGER NOT NULL DEFAULT 0,
            rate_limit_hits INTEGER NOT NULL DEFAULT 0,
            last_used_at INTEGER,
            last_error_at INTEGER,
            last_error_message TEXT
        )",
    )
    .execute(&pool)
    .await?;

    Ok(pool)
}
