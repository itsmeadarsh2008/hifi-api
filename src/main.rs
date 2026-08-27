mod account_manager;
mod admin;
mod anti_ban;
mod config;
mod db;
mod error;
mod ip_limiter;
mod proxy_manager;
mod rate_limit;
mod routes;
mod setup;
mod tidal_client;
mod token_manager;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::State;
use axum::http::Method;
use axum::middleware;
use axum::routing::{any, get, patch, post, put};
use axum::{Json, Router};
use reqwest::Client;
use serde_json::Value;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crate::account_manager::{AccountManager, SwitchingWeights};
use crate::config::Config;
use crate::token_manager::TokenManager;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub account_manager: Arc<AccountManager>,
    pub token_manager: Arc<TokenManager>,
    pub tidal_client: Arc<tidal_client::TidalClient>,
    pub proxy_manager: Arc<proxy_manager::ProxyManager>,
    pub anti_ban: Arc<anti_ban::AntiBan>,
    pub rate_limits: Arc<rate_limit::RateLimitSettings>,
    pub db: Option<sqlx::SqlitePool>,
    pub setup_sessions: admin::setup::Sessions,
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

    let http_client = Client::builder()
        .gzip(true)
        .http2_prior_knowledge()
        .http2_adaptive_window(true)
        .pool_max_idle_per_host(500)
        .pool_idle_timeout(std::time::Duration::from_secs(30))
        .user_agent("okhttp/5.3.2")
        .build()
        .expect("Failed to build HTTP client");
    let http_client = Arc::new(http_client);

    let switching_weights = SwitchingWeights::default();
    let account_manager = Arc::new(AccountManager::new(db.clone(), switching_weights));

    if let Err(e) = account_manager.load_from_db().await {
        tracing::warn!("Could not load accounts from DB: {}", e);
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

    let rate_limits = Arc::new(rate_limit::RateLimitSettings::from_env());
    if let Some(db) = &db {
        rate_limits.load_from_db(db).await;
    }

    let anti_ban = Arc::new(anti_ban::AntiBan::new(rate_limits.clone()));

    // Periodically rebuild the per-IP limiter so stale IP buckets are dropped
    // (bounds memory) and no IP is throttled forever by past activity.
    {
        let ab = anti_ban.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(600));
            loop {
                interval.tick().await;
                ab.reload_limiter();
            }
        });
    }

    let tidal_client = Arc::new(tidal_client::TidalClient::new(
        (*http_client).clone(),
        token_manager.clone(),
        account_manager.clone(),
        anti_ban.clone(),
        rate_limits.clone(),
        config.clone(),
    ));

    let proxy_manager = Arc::new(proxy_manager::ProxyManager::new(config.clone()));

    let state = AppState {
        config: config.clone(),
        account_manager: account_manager.clone(),
        token_manager: token_manager.clone(),
        tidal_client: tidal_client.clone(),
        proxy_manager,
        anti_ban,
        rate_limits,
        db,
        setup_sessions: admin::setup::new_session_store(),
    };

    // Start token pre-warming background task
    token_manager.start_prewarm_loop(account_manager, http_client).await;

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
        // Admin API routes (auth-protected)
        .nest("/admin", admin_api(state.clone()))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            ip_limiter::enforce_ip_rate_limit,
        ))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = format!("{}:{}", config.host, config.port);
    tracing::info!("HiFi API v{} starting on {}", config.api_version, addr);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}

fn admin_api(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/accounts/export", get(crate::admin::accounts::export_accounts))
        .route("/accounts/import", post(crate::admin::accounts::import_accounts))
        .route("/accounts", get(crate::admin::accounts::list_accounts).post(crate::admin::accounts::add_account))
        .route("/accounts/{id}", patch(crate::admin::accounts::update_account).delete(crate::admin::accounts::remove_account))
        .route("/accounts/{id}/toggle", put(crate::admin::accounts::toggle_account))
        .route("/accounts/test-all", post(crate::admin::accounts::test_all_accounts))
        .route("/accounts/{id}/test", post(crate::admin::accounts::test_account))
        .route("/accounts/{id}/refresh", post(crate::admin::accounts::refresh_account_token))
        .route("/stats", get(crate::admin::stats::get_stats))
        .route(
            "/settings",
            get(crate::admin::settings::get_settings).put(crate::admin::settings::update_settings),
        )
        .route("/setup", post(crate::admin::setup::start_setup))
        .route("/setup/{session}", get(crate::admin::setup::check_setup))
        .layer(middleware::from_fn_with_state(state, crate::admin::admin_auth))
}

async fn index(
    State(state): State<AppState>,
) -> Json<Value> {
    routes::index(&state.config)
}
