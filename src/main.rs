mod account_manager;
mod admin;
mod api_keys;
mod autoheal;
mod config;
mod db;
mod error;
mod cache;
mod notifier;
mod proxy_manager;
mod settings;
mod request_log;
mod routes;
mod setup;
mod tidal_client;
mod token_manager;
mod upstash;

use std::net::SocketAddr;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use axum::extract::State;
use axum::http::Method;
use axum::middleware;
use axum::routing::{any, delete, get, patch, post, put};
use axum::{Json, Router};
use serde_json::Value;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crate::account_manager::{AccountManager, SwitchingWeights};
use crate::api_keys::ApiKeyManager;
use crate::config::Config;
use crate::token_manager::TokenManager;
use crate::upstash::UpstashStore;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub account_manager: Arc<AccountManager>,
    pub token_manager: Arc<TokenManager>,
    pub api_keys: Arc<ApiKeyManager>,
    pub tidal_client: Arc<tidal_client::TidalClient>,
    pub proxy_manager: Arc<proxy_manager::ProxyManager>,
    pub notifier: Arc<notifier::Notifier>,
    /// Live playback ops right now (no queue exists; every request runs
    /// directly). Feeds the panel's Playback card.
    pub playback_inflight: Arc<AtomicU64>,
    pub cache: Arc<cache::ResponseCache>,
    pub settings: Arc<settings::AppSettings>,
    pub request_log: Arc<request_log::RequestLog>,
    pub db: Option<sqlx::SqlitePool>,
    pub setup_sessions: admin::setup::Sessions,
    /// Shared cross-instance state (None = single-host mode, skip sync).
    pub upstash: Option<Arc<UpstashStore>>,
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let config = Arc::new(Config::from_env());

    let db = if config.database_url.is_empty() || config.database_url == "ephemeral" {
        tracing::info!("Running in ephemeral mode — no database");
        None
    } else {
        match db::init_pool(&config.database_url).await {
            Ok(pool) => {
                tracing::info!("Database initialized at {}", config.database_url);
                Some(pool)
            }
            Err(e) => {
                tracing::warn!("Failed to initialize database (ephemeral fallback): {}", e);
                None
            }
        }
    };

    // ProxyManager owns the shared HTTP client (direct by default; proxied
    // when USE_PROXIES=true). The initial proxy resolve runs in the background
    // so startup is never blocked on proxy tests.
    let proxy_manager = Arc::new(proxy_manager::ProxyManager::new(config.clone()));
    proxy_manager.spawn_initial_resolve();
    let http_client = Arc::new(proxy_manager.client());

    let switching_weights = SwitchingWeights::default();
    let settings = Arc::new(settings::AppSettings::from_env());
    if let Some(db) = &db {
        settings.load_from_db(db).await;
    }

    // Shared cross-instance state (Upstash Redis). Absent unless both env
    // vars are set — everything below degrades to single-host behavior.
    let upstash = UpstashStore::from_env();
    if let Some(store) = &upstash {
        if store.ping().await {
            tracing::info!("Redis sync enabled (backend: {})", store.backend_kind());
        } else {
            tracing::warn!("Redis unreachable at startup — running degraded (local-only) until it recovers");
        }
        // Fleet convergence: seed-if-empty, then adopt the shared values.
        // (Local DB/env already loaded above as the fallback/seed source.)
        settings.set_upstash(Some(store.clone()));
        settings.seed_and_load().await;
    }

    let account_manager = Arc::new(AccountManager::new(db.clone(), switching_weights));

    if let Err(e) = account_manager.load_from_db().await {
        tracing::warn!("Could not load accounts from DB: {}", e);
    }
    account_manager.set_upstash(upstash.clone());
    // Converge credentials with the fleet (SQLite only has
    // this host — a wiped disk restores its accounts from Redis here).
    account_manager.merge_accounts_from_redis().await;

    // Legacy credential file (upstream TOKEN_FILE, default token.json):
    // imported once into the DB when present. Supports the upstream
    // per-entry catalog marker (role="catalog" or catalog=true), which is
    // kept out of the playback pool.
    import_token_file(&account_manager, &config.token_file).await;

    // Dedicated metadata credential from the environment (upstream
    // CATALOG_REFRESH_TOKEN). Only when no catalog account exists yet.
    if account_manager.find_catalog_account().await.is_none()
        && !config.catalog_refresh_token.is_empty()
    {
        let secret = if config.catalog_client_secret.is_empty() {
            "Y8tIpqKJxs9BEIwYr0I9bSbMWDsogXJx9LaN3mCHwD4%3D".to_string()
        } else {
            config.catalog_client_secret.clone()
        };
        match account_manager
            .add_account(
                "Catalog (env)".into(),
                config.catalog_client_id.clone(),
                secret,
                config.catalog_refresh_token.clone(),
                config.catalog_user_id.clone(),
            )
            .await
        {
            Ok(acc) => {
                let _ = account_manager.set_account_catalog(&acc.id, true).await;
                tracing::info!("Loaded catalog account from env vars ({})", acc.id);
            }
            Err(e) => tracing::warn!("Failed to load catalog account from env vars: {}", e),
        }
    }

    if account_manager.account_count().await == 0 {
        let env_client_id = std::env::var("CLIENT_ID").unwrap_or_default();
        let env_client_secret = std::env::var("CLIENT_SECRET").unwrap_or_default();
        let env_refresh_token = std::env::var("REFRESH_TOKEN").unwrap_or_default();

        if !env_client_id.is_empty() && !env_refresh_token.is_empty() {
            let client_secret = if env_client_secret.is_empty() {
                "Y8tIpqKJxs9BEIwYr0I9bSbMWDsogXJx9LaN3mCHwD4%3D".to_string()
            } else {
                env_client_secret
            };
            let env_user_id = std::env::var("USER_ID").ok();
            match account_manager
                .add_account(
                    "Default Account (env)".into(),
                    env_client_id,
                    client_secret,
                    env_refresh_token,
                    env_user_id,
                )
                .await
            {
                Ok(acc) => tracing::info!("Loaded account from env vars ({})", acc.id),
                Err(e) => tracing::warn!("Failed to load account from env vars: {}", e),
            }
        }
    }

    if account_manager.account_count().await == 0 {
        if std::env::var("AUTO_SETUP").unwrap_or_default() == "true" {
            tracing::info!("AUTO_SETUP=true: Starting OAuth setup in background...");
            let am = account_manager.clone();
            let hc = http_client.clone();
            tokio::spawn(async move {
                if let Err(e) = setup::run_setup(&am, hc.as_ref()).await {
                    tracing::warn!("Auto-setup failed: {}. Add accounts via admin panel or env vars.", e);
                }
            });
        } else {
            tracing::warn!("No Tidal accounts configured. Add one via the admin panel at /admin or set CLIENT_ID/REFRESH_TOKEN in .env");
        }
    }

    let token_manager = Arc::new(TokenManager::new(db.clone()));
    token_manager.set_account_manager(account_manager.clone());
    token_manager.set_proxy_manager(proxy_manager.clone());
    token_manager.set_upstash(upstash.clone());

    let api_keys = Arc::new(ApiKeyManager::new(db.clone()));
    api_keys.set_upstash(upstash.clone());
    if let Err(e) = api_keys.load_from_db().await {
        tracing::warn!("Could not load API keys from DB: {}", e);
    }
    api_keys.sync_usage_from_redis().await;
    api_keys.merge_keys_from_redis().await;

    // Reconcile the fleet-wide quotas/rosters from Redis.
    {
        let am = account_manager.clone();
        let ak = api_keys.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                ak.sync_usage_from_redis().await;
                am.merge_accounts_from_redis().await;
                ak.merge_keys_from_redis().await;
            }
        });
    }

    // Periodically adopt shared settings from Redis.
    {
        let st = settings.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                interval.tick().await;
                st.refresh_from_redis().await;
            }
        });
    }

    let notifier = notifier::Notifier::new(config.discord_webhook_url.clone());

    let tidal_client = Arc::new(tidal_client::TidalClient::new(
        proxy_manager.clone(),
        token_manager.clone(),
        account_manager.clone(),
        notifier.clone(),
        config.clone(),
    ));

    let state = AppState {
        config: config.clone(),
        account_manager: account_manager.clone(),
        token_manager: token_manager.clone(),
        api_keys: api_keys.clone(),
        notifier: notifier.clone(),
        tidal_client: tidal_client.clone(),
        proxy_manager: proxy_manager.clone(),
        playback_inflight: Arc::new(AtomicU64::new(0)),
        cache: Arc::new(cache::ResponseCache::new()),
        settings: settings.clone(),
        request_log: Arc::new(request_log::RequestLog::new()),
        db,
        setup_sessions: admin::setup::new_session_store(),
        upstash: upstash.clone(),
    };

    // Start token pre-warming background task
    token_manager
        .clone()
        .start_prewarm_loop(account_manager.clone(), proxy_manager.clone())
        .await;

    // Start auto-heal background task (recovers system-disabled accounts)
    autoheal::start_autoheal_loop(
        account_manager.clone(),
        token_manager.clone(),
        proxy_manager.clone(),
        settings.clone(),
        notifier.clone(),
    )
    .await;

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers(Any);

    let app = Router::new()
        // Public API routes
        .route("/", get(index))
        .route("/info/", get(routes::info::get_info))
        .route("/track/", get(routes::track::get_track))
        .route("/trackManifests/{id}", get(routes::track::get_track_manifests))
        .route("/trackManifests", get(routes::track::get_track_manifests_query))
        .route("/trackManifests/", get(routes::track::get_track_manifests_query))
        .route("/dash/{id}", get(routes::track::get_dash_stream))
        .route("/widevine", any(routes::widevine::widevine_proxy))
        .route("/recommendations/", get(routes::recommendations::get_recommendations))
        .route("/search/", get(routes::search::search))
        .route("/album/", get(routes::album::get_album))
        .route("/album/similar/", get(routes::similar_albums::get_similar_albums))
        .route("/artist/", get(routes::artist::get_artist))
        .route("/artist/similar/", get(routes::similar_artists::get_similar_artists))
        .route("/mix/", get(routes::mix::get_mix))
        .route("/playlist/", get(routes::playlist::get_playlist))
        .route("/cover/", get(routes::cover::get_cover))
        .route("/lyrics/", get(routes::lyrics::get_lyrics))
        .route("/topvideos/", get(routes::topvideos::get_top_videos))
        .route("/video/", get(routes::video::get_video))
        .route("/health", get(routes::health::health))
        // Admin SPA (no auth — the SPA handles auth in-browser)
        .route("/admin", get(crate::admin::ui::admin_index))
        // Vendored terminal assets (public like the page itself)
        .route("/admin/assets/{file}", get(crate::admin::ui::admin_asset))
        // Admin API routes (auth-protected)
        .nest("/admin", admin_api(state.clone()))
        // Innermost: response cache.
        .layer(middleware::from_fn_with_state(
            state.clone(),
            cache::cache_responses,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            api_keys::ApiKeyManager::enforce_api_key,
        ))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            request_log::log_requests,
        ))
        .with_state(state);

    let addr = format!("{}:{}", config.host, config.port);
    tracing::info!("HiFi API v{} starting on {}", config.api_version, addr);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .unwrap();
}

/// Render (and Docker stop) send SIGTERM on redeploy: stop accepting new
/// connections but let in-flight playback/upstream calls finish instead of
/// dropping them mid-request.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("Shutdown signal received, draining in-flight requests");
}

/// Import a legacy upstream token.json file into the DB (once).
/// Tolerates upstream key variants (client_ID, userID) and the catalog
/// marker (role="catalog" / catalog=true). Duplicates by refresh token
/// are skipped, matching the admin import endpoint.
async fn import_token_file(manager: &Arc<AccountManager>, path: &str) {
    if path.is_empty() {
        return;
    }
    let raw = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => {
            tracing::debug!("Token file {} not present, skipping", path);
            return;
        }
    };
    let parsed: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Cannot parse token file {}: {}", path, e);
            return;
        }
    };
    let entries: Vec<Value> = match parsed {
        Value::Array(a) => a,
        Value::Object(_) => vec![parsed],
        _ => {
            tracing::warn!("Token file {} has unexpected shape, skipping", path);
            return;
        }
    };
    let env_client_id = std::env::var("CLIENT_ID").unwrap_or_default();
    let env_client_secret = std::env::var("CLIENT_SECRET").unwrap_or_default();
    let existing: std::collections::HashSet<String> = manager
        .list_accounts()
        .await
        .iter()
        .map(|a| a.refresh_token.clone())
        .collect();
    let mut seen = existing;
    let mut imported = 0usize;
    for entry in &entries {
        let s = |keys: &[&str]| {
            keys.iter()
                .filter_map(|k| entry.get(*k).and_then(|v| v.as_str()))
                .next()
                .unwrap_or("")
                .to_string()
        };
        let client_id = s(&["client_id", "client_ID", "clientID"]);
        let client_id = if client_id.is_empty() {
            env_client_id.clone()
        } else {
            client_id
        };
        let mut client_secret = s(&["client_secret", "clientSecret"]);
        if client_secret.is_empty() {
            client_secret = env_client_secret.clone();
        }
        let refresh_token = s(&["refresh_token", "refreshToken"]);
        if client_id.is_empty() || refresh_token.is_empty() || seen.contains(&refresh_token) {
            continue;
        }
        let user_id = [entry.get("user_id"), entry.get("userID"), entry.get("userId")]
            .into_iter()
            .filter_map(|v| v.and_then(|x| x.as_str()))
            .next()
            .map(|x| x.to_string());
        let label = entry
            .get("label")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let is_catalog = entry
            .get("role")
            .and_then(|v| v.as_str())
            .map(|r| r.eq_ignore_ascii_case("catalog"))
            .unwrap_or(false)
            || entry
                .get("catalog")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
        match manager
            .add_account(label, client_id, client_secret, refresh_token.clone(), user_id)
            .await
        {
            Ok(acc) => {
                if is_catalog {
                    let _ = manager.set_account_catalog(&acc.id, true).await;
                }
                seen.insert(refresh_token);
                imported += 1;
            }
            Err(e) => tracing::warn!("Token file entry skipped: {}", e),
        }
    }
    if imported > 0 {
        tracing::info!("Imported {} account(s) from {}", imported, path);
    }
}

fn admin_api(state: AppState) -> Router<AppState> {    Router::new()
        .route("/accounts/export", get(crate::admin::accounts::export_accounts))
        .route("/accounts/import", post(crate::admin::accounts::import_accounts))
        .route("/accounts", get(crate::admin::accounts::list_accounts).post(crate::admin::accounts::add_account))
        .route("/accounts/{id}", patch(crate::admin::accounts::update_account).delete(crate::admin::accounts::remove_account))
        .route("/accounts/{id}/toggle", put(crate::admin::accounts::toggle_account))
        .route("/accounts/{id}/catalog", put(crate::admin::accounts::set_account_catalog))
        .route("/accounts/test-all", post(crate::admin::accounts::test_all_accounts))
        .route("/accounts/{id}/test", post(crate::admin::accounts::test_account))
        .route("/accounts/{id}/refresh", post(crate::admin::accounts::refresh_account_token))
        .route("/accounts/{id}/check-premium", post(crate::admin::accounts::check_account_premium))
        .route("/stats", get(crate::admin::stats::get_stats))
        .route("/proxies", get(crate::admin::proxies::proxy_status))
        .route("/alerts", get(crate::admin::alerts::alert_status))
        .route("/alerts/test", post(crate::admin::alerts::alert_test))
        .route("/alerts/report", post(crate::admin::alerts::alert_report))
        .route("/cache", get(crate::admin::cache::cache_stats))
        .route("/cache/clear", post(crate::admin::cache::cache_clear))
        .route("/keys", get(crate::admin::api_keys::list_keys).post(crate::admin::api_keys::create_key))
        .route("/keys/{id}", delete(crate::admin::api_keys::remove_key))
        .route("/keys/{id}/toggle", put(crate::admin::api_keys::toggle_key))
        .route("/backup", get(crate::admin::backup::download_backup))
        .route("/backup/restore", post(crate::admin::backup::restore_backup))
        .route("/requests", get(crate::admin::requests::get_requests))
        .route(
            "/settings",
            get(crate::admin::settings::get_settings).put(crate::admin::settings::update_settings),
        )
        .route("/setup", post(crate::admin::setup::start_setup))
        .route(
            "/setup/{session}",
            get(crate::admin::setup::check_setup).patch(crate::admin::setup::update_setup),
        )
        .layer(middleware::from_fn_with_state(state, crate::admin::admin_auth))
}

async fn index(
    State(state): State<AppState>,
) -> Json<Value> {
    routes::index(&state.config)
}
