use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use axum::{
    Json, Router,
    body::Body,
    extract::{Form, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use chrono::{Duration, Utc};
use hmac::{Hmac, Mac};
use rand::{Rng, distr::Alphanumeric};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::{env, net::SocketAddr, str::FromStr, sync::Arc};
use tokio::time::{Duration as TokioDuration, sleep};
use tower_http::trace::TraceLayer;
use tracing::info;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
struct AppState {
    db: SqlitePool,
    cfg: Config,
}

#[derive(Clone)]
struct Config {
    bind: String,
    setup_password: String,
    opencode_url: String,
    max_sessions: usize,
}

#[derive(Debug, Clone, Serialize)]
struct SessionView {
    id: String,
    source: String,
    status: String,
    mode: String,
    title: String,
    created_at: String,
    finished_at: Option<String>,
    source_url: Option<String>,
}

#[derive(Deserialize)]
struct SetupForm {
    bootstrap_password: String,
    email: String,
    display_name: String,
    password: String,
    password_confirm: String,
}
#[derive(Deserialize)]
struct LoginForm {
    email: String,
    password: String,
}
#[derive(Deserialize)]
struct MessageForm {
    message: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();
    let database_url =
        env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:///data/koda.db?mode=rwc".into());
    let options = SqliteConnectOptions::from_str(&database_url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);
    let db = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await?;
    sqlx::migrate!().run(&db).await?;
    let cfg = Config {
        bind: env::var("APP_BIND").unwrap_or_else(|_| "0.0.0.0:8080".into()),
        setup_password: env_or_file("APP_SETUP_PASSWORD"),
        opencode_url: env::var("OPENCODE_BASE_URL")
            .unwrap_or_else(|_| "http://opencode:4096".into()),
        max_sessions: env::var("MAX_CONCURRENT_SESSIONS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2),
    };
    let state = Arc::new(AppState { db, cfg });
    let addr: SocketAddr = state
        .cfg
        .bind
        .parse()
        .unwrap_or_else(|_| "0.0.0.0:8080".parse().unwrap());
    let worker_state = state.clone();
    tokio::spawn(async move {
        worker_loop(worker_state).await;
    });
    let app = Router::new()
        .route(
            "/health/live",
            get(|| async { Json(json!({"status":"ok"})) }),
        )
        .route("/health/ready", get(health_ready))
        .route("/", get(root))
        .route("/setup", get(setup_page).post(setup_submit))
        .route("/login", get(login_page).post(login_submit))
        .route("/logout", post(logout))
        .route("/dashboard", get(frontend_page))
        .route("/sessions", get(frontend_page))
        .route("/sessions/{id}", get(frontend_page))
        .route("/projects", get(frontend_page))
        .route("/skills", get(frontend_page))
        .route("/integrations", get(frontend_page))
        .route("/audit", get(frontend_page))
        .route(
            "/dashboard/sessions/{id}",
            get(session_detail).post(session_message),
        )
        .route("/api/v1/sessions", get(api_sessions))
        .route("/api/v1/dashboard/summary", get(api_dashboard_summary))
        .route("/api/v1/sessions/{id}", get(api_session_detail))
        .route("/api/v1/sessions/{id}/approval", post(api_session_approval))
        .route("/api/v1/sessions/{id}/cancel", post(api_session_cancel))
        .route("/api/v1/audit", get(api_audit))
        .route("/api/v1/setup/status", get(api_setup_status))
        .route(
            "/api/v1/projects",
            get(api_projects).post(api_project_create),
        )
        .route(
            "/api/v1/integrations",
            get(api_integrations).post(api_integration_create),
        )
        .route(
            "/api/v1/mcp-servers",
            get(api_mcp_servers).post(api_mcp_create),
        )
        .route("/api/v1/skills", get(api_skills).post(api_skill_create))
        .route("/api/v1/webhooks/jira/work-items", post(webhook_jira))
        .route(
            "/api/v1/webhooks/gitlab/code-events",
            post(webhook_gitlab_code),
        )
        .route(
            "/api/v1/webhooks/gitlab/pipeline-events",
            post(webhook_pipeline_event),
        )
        .fallback(frontend)
        .layer(TraceLayer::new_for_http())
        .with_state(state);
    info!(%addr, "koda listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn env_or_file(name: &str) -> String {
    if let Ok(path) = env::var(format!("{name}_FILE")) {
        if let Ok(v) = std::fs::read_to_string(path) {
            return v.trim().to_owned();
        }
    }
    env::var(name).unwrap_or_default()
}

async fn health_ready(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    let installed = is_installed(&s.db).await.unwrap_or(false);
    let db_ok = sqlx::query("SELECT 1").execute(&s.db).await.is_ok();
    let status = if db_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(json!({"database":db_ok,"installed":installed,"opencode_url":s.cfg.opencode_url})),
    )
}

async fn root(State(s): State<Arc<AppState>>) -> Redirect {
    if is_installed(&s.db).await.unwrap_or(false) {
        Redirect::to("/login")
    } else {
        Redirect::to("/setup")
    }
}

async fn frontend_page() -> Response {
    frontend_file("index.html").await
}

async fn frontend(
    State(_s): State<Arc<AppState>>,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path.starts_with("api/") || path.starts_with("health/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    if path.is_empty() {
        return frontend_file("index.html").await;
    }
    let candidate = if path.contains('.') {
        path
    } else {
        "index.html"
    };
    frontend_file(candidate).await
}

async fn frontend_file(path: &str) -> Response {
    let root = env::var("KODA_STATIC_DIR").unwrap_or_else(|_| "/app/static".to_string());
    let clean = path.trim_start_matches('/').replace("..", "");
    match tokio::fs::read(std::path::Path::new(&root).join(&clean)).await {
        Ok(bytes) => { let mime = mime_guess::from_path(&clean).first_or_octet_stream(); let mut r = Response::new(Body::from(bytes)); r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_str(mime.as_ref()).unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"))); r }
        Err(_) if clean == "index.html" => Html("<!doctype html><html lang='fr'><meta charset='utf-8'><title>Koda</title><body><h1>Koda</h1><p>Construisez le frontend React avec <code>npm run build</code>.</p></body></html>").into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn is_installed(db: &SqlitePool) -> Result<bool, sqlx::Error> {
    Ok(
        sqlx::query("SELECT value_json FROM app_settings WHERE key='installation_completed'")
            .fetch_optional(db)
            .await?
            .map(|r| r.get::<String, _>("value_json") == "true")
            .unwrap_or(false),
    )
}

async fn setup_page(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    if is_installed(&s.db).await.unwrap_or(false) {
        return Redirect::to("/login").into_response();
    }
    Html(r#"<!doctype html><html lang="fr"><meta charset="utf-8"><title>Installation Koda</title><style>body{font:16px system-ui;max-width:620px;margin:4rem auto;padding:1rem}label{display:block;margin-top:1rem}input{width:100%;padding:.6rem}button{margin-top:1.5rem;padding:.7rem 1rem}</style><h1>Installation</h1><p>Validez le mot de passe de bootstrap puis créez le premier compte administrateur.</p><form method="post"><label>Mot de passe de bootstrap<input name="bootstrap_password" type="password" required></label><label>Email<input name="email" type="email" required></label><label>Nom<input name="display_name" required></label><label>Mot de passe admin<input name="password" type="password" minlength="14" required></label><label>Confirmation<input name="password_confirm" type="password" minlength="14" required></label><button>Installer</button></form></html>"#).into_response()
}

async fn setup_submit(
    State(s): State<Arc<AppState>>,
    Form(f): Form<SetupForm>,
) -> impl IntoResponse {
    if is_installed(&s.db).await.unwrap_or(false) {
        return Redirect::to("/login").into_response();
    }
    if s.cfg.setup_password.is_empty()
        || !constant_eq(
            s.cfg.setup_password.as_bytes(),
            f.bootstrap_password.as_bytes(),
        )
        || !constant_eq(f.password.as_bytes(), f.password_confirm.as_bytes())
        || f.password.len() < 14
    {
        return (
            StatusCode::BAD_REQUEST,
            Html("Mot de passe de bootstrap ou mot de passe admin invalide."),
        )
            .into_response();
    }
    let salt = SaltString::generate(&mut OsRng);
    let hash = match Argon2::default().hash_password(f.password.as_bytes(), &salt) {
        Ok(h) => h.to_string(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let now = Utc::now().to_rfc3339();
    let mut tx = match s.db.begin().await {
        Ok(tx) => tx,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let id = Uuid::now_v7().to_string();
    if sqlx::query(
        "INSERT INTO users(id,email,display_name,password_hash,created_at) VALUES(?,?,?,?,?)",
    )
    .bind(&id)
    .bind(f.email.trim())
    .bind(f.display_name.trim())
    .bind(hash)
    .bind(&now)
    .execute(&mut *tx)
    .await
    .is_err()
    {
        return (StatusCode::CONFLICT, Html("Cet email existe déjà.")).into_response();
    }
    if sqlx::query("INSERT INTO app_settings(key,value_json,updated_at) VALUES('installation_completed','true',?) ON CONFLICT(key) DO UPDATE SET value_json='true',updated_at=excluded.updated_at").bind(&now).execute(&mut *tx).await.is_err() { return StatusCode::INTERNAL_SERVER_ERROR.into_response(); }
    if tx.commit().await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    Redirect::to("/login").into_response()
}

async fn login_page() -> Html<&'static str> {
    Html(
        r#"<!doctype html><html lang="fr"><meta charset="utf-8"><title>Connexion</title><style>body{font:16px system-ui;max-width:520px;margin:4rem auto;padding:1rem}label{display:block;margin-top:1rem}input{width:100%;padding:.6rem}button{margin-top:1.5rem;padding:.7rem 1rem}</style><h1>Connexion</h1><form method="post"><label>Email<input name="email" type="email" required></label><label>Mot de passe<input name="password" type="password" required></label><button>Se connecter</button></form></html>"#,
    )
}

async fn login_submit(
    State(s): State<Arc<AppState>>,
    Form(f): Form<LoginForm>,
) -> impl IntoResponse {
    let row = sqlx::query("SELECT id,password_hash FROM users WHERE email=? AND active=1")
        .bind(f.email.trim())
        .fetch_optional(&s.db)
        .await
        .ok()
        .flatten();
    let valid = row
        .as_ref()
        .map(|r| {
            let hash = r.get::<String, _>("password_hash");
            PasswordHash::new(&hash)
                .ok()
                .and_then(|h| {
                    Argon2::default()
                        .verify_password(f.password.as_bytes(), &h)
                        .ok()
                })
                .is_some()
        })
        .unwrap_or(false);
    if !valid {
        return (StatusCode::UNAUTHORIZED, Html("Identifiants invalides.")).into_response();
    }
    let token: String = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(64)
        .map(char::from)
        .collect();
    let csrf: String = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();
    let hash = hex_hash(&token);
    let now = Utc::now();
    let exp = now + Duration::hours(12);
    let user_id = row.unwrap().get::<String, _>("id");
    if sqlx::query("INSERT INTO auth_sessions(id,user_id,token_hash,csrf_token,created_at,expires_at) VALUES(?,?,?,?,?,?)").bind(Uuid::now_v7().to_string()).bind(user_id).bind(hash).bind(csrf).bind(now.to_rfc3339()).bind(exp.to_rfc3339()).execute(&s.db).await.is_err() { return StatusCode::INTERNAL_SERVER_ERROR.into_response(); }
    let mut headers = HeaderMap::new();
    headers.insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "koda_session={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=43200"
        ))
        .unwrap(),
    );
    headers.insert(header::LOCATION, HeaderValue::from_static("/dashboard"));
    (StatusCode::SEE_OTHER, headers, "").into_response()
}

async fn logout(State(s): State<Arc<AppState>>, headers: HeaderMap) -> impl IntoResponse {
    if let Some(t) = cookie(&headers, "koda_session") {
        let _ = sqlx::query("DELETE FROM auth_sessions WHERE token_hash=?")
            .bind(hex_hash(&t))
            .execute(&s.db)
            .await;
    }
    let mut h = HeaderMap::new();
    h.insert(
        header::SET_COOKIE,
        HeaderValue::from_static("koda_session=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax"),
    );
    h.insert(header::LOCATION, HeaderValue::from_static("/login"));
    (StatusCode::SEE_OTHER, h, "")
}

async fn session_detail(
    State(s): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if auth_user(&s.db, &headers).await.is_none() {
        return Redirect::to("/login").into_response();
    }
    let r = sqlx::query(
        "SELECT id,source,status,mode,title,metadata_json,error_message FROM sessions WHERE id=?",
    )
    .bind(&id)
    .fetch_optional(&s.db)
    .await
    .ok()
    .flatten();
    let Some(r) = r else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let events = sqlx::query(
        "SELECT kind,payload_json,created_at FROM session_events WHERE session_id=? ORDER BY id",
    )
    .bind(&id)
    .fetch_all(&s.db)
    .await
    .unwrap_or_default();
    let mut h = format!(
        "<h1>{}</h1><p>{} · {} · {}</p><form method='post'><textarea name='message' rows='5' cols='80' placeholder='Instruction supplémentaire'></textarea><br><button>Envoyer</button></form><h2>Événements</h2><pre>",
        esc(&r.get::<String, _>("title")),
        esc(&r.get::<String, _>("source")),
        esc(&r.get::<String, _>("status")),
        esc(&r.get::<String, _>("mode"))
    );
    for e in events {
        h.push_str(&format!(
            "{} {} {}\n",
            e.get::<String, _>("created_at"),
            e.get::<String, _>("kind"),
            esc(&e.get::<String, _>("payload_json"))
        ));
    }
    h.push_str("</pre><p><a href='/dashboard'>Retour</a></p>");
    Html(format!("<!doctype html><html lang='fr'><meta charset='utf-8'><style>body{{font:16px system-ui;max-width:1000px;margin:3rem auto}}textarea{{width:100%}}pre{{white-space:pre-wrap}}</style>{h}</html>")).into_response()
}

async fn session_message(
    State(s): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Form(f): Form<MessageForm>,
) -> impl IntoResponse {
    if auth_user(&s.db, &headers).await.is_none() {
        return Redirect::to("/login").into_response();
    }
    let now = Utc::now().to_rfc3339();
    let _ = sqlx::query(
        "INSERT INTO session_events(session_id,kind,payload_json,created_at) VALUES(?,?,?,?)",
    )
    .bind(&id)
    .bind("admin_message")
    .bind(json!({"message":f.message}).to_string())
    .bind(now)
    .execute(&s.db)
    .await;
    Redirect::to(&format!("/dashboard/sessions/{id}")).into_response()
}

async fn api_sessions(State(s): State<Arc<AppState>>, headers: HeaderMap) -> impl IntoResponse {
    if auth_user(&s.db, &headers).await.is_none() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let rows=sqlx::query("SELECT id,source,status,mode,title,created_at,finished_at,source_url FROM sessions ORDER BY created_at DESC LIMIT 100").fetch_all(&s.db).await.unwrap_or_default();
    let data: Vec<SessionView> = rows
        .into_iter()
        .map(|r| SessionView {
            id: r.get("id"),
            source: r.get("source"),
            status: r.get("status"),
            mode: r.get("mode"),
            title: r.get("title"),
            created_at: r.get("created_at"),
            finished_at: r.get("finished_at"),
            source_url: r.get("source_url"),
        })
        .collect();
    Json(data).into_response()
}

async fn api_dashboard_summary(
    State(s): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if auth_user(&s.db, &headers).await.is_none() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let active = count_sessions(&s.db, "SELECT COUNT(*) AS count FROM sessions WHERE status IN ('queued','preparing','running','evidence_ready')").await;
    let week = count_sessions(
        &s.db,
        "SELECT COUNT(*) AS count FROM sessions WHERE created_at >= datetime('now','-7 days')",
    )
    .await;
    let awaiting = count_sessions(
        &s.db,
        "SELECT COUNT(*) AS count FROM sessions WHERE status='awaiting_approval'",
    )
    .await;
    let blocked = count_sessions(
        &s.db,
        "SELECT COUNT(*) AS count FROM sessions WHERE status='blocked'",
    )
    .await;
    Json(json!({"active":active,"week":week,"awaiting_approval":awaiting,"blocked":blocked}))
        .into_response()
}

async fn count_sessions(db: &SqlitePool, query: &str) -> i64 {
    sqlx::query(query)
        .fetch_one(db)
        .await
        .map(|r| r.get::<i64, _>("count"))
        .unwrap_or(0)
}

async fn api_session_detail(
    State(s): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if auth_user(&s.db, &headers).await.is_none() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let row = sqlx::query("SELECT id,source,status,mode,title,workflow_type,approval_state,evidence_hash,metadata_json,error_message,created_at,finished_at FROM sessions WHERE id=?").bind(&id).fetch_optional(&s.db).await.ok().flatten();
    let Some(r) = row else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let events = sqlx::query(
        "SELECT kind,payload_json,created_at FROM session_events WHERE session_id=? ORDER BY id",
    )
    .bind(&id)
    .fetch_all(&s.db)
    .await
    .unwrap_or_default();
    let event_data: Vec<Value> = events.into_iter().map(|e| json!({"kind":e.get::<String,_>("kind"),"payload":e.get::<String,_>("payload_json"),"created_at":e.get::<String,_>("created_at")})).collect();
    Json(json!({"id":r.get::<String,_>("id"),"source":r.get::<String,_>("source"),"status":r.get::<String,_>("status"),"mode":r.get::<String,_>("mode"),"title":r.get::<String,_>("title"),"workflow_type":r.get::<String,_>("workflow_type"),"approval_state":r.get::<String,_>("approval_state"),"evidence_hash":r.get::<Option<String>,_>("evidence_hash"),"metadata":r.get::<String,_>("metadata_json"),"error":r.get::<Option<String>,_>("error_message"),"created_at":r.get::<String,_>("created_at"),"finished_at":r.get::<Option<String>,_>("finished_at"),"events":event_data})).into_response()
}

#[derive(Deserialize)]
struct ApprovalRequest {
    decision: String,
    evidence_hash: String,
    reason: Option<String>,
}
async fn api_session_approval(
    State(s): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<ApprovalRequest>,
) -> impl IntoResponse {
    let Some(user_id) = auth_user(&s.db, &headers).await else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if req.decision != "approve" && req.decision != "reject" {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid_decision"})),
        )
            .into_response();
    }
    let Some(row) = sqlx::query("SELECT status,evidence_hash FROM sessions WHERE id=?")
        .bind(&id)
        .fetch_optional(&s.db)
        .await
        .ok()
        .flatten()
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if row.get::<String, _>("status") != "awaiting_approval"
        || row.get::<Option<String>, _>("evidence_hash").as_deref()
            != Some(req.evidence_hash.as_str())
    {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error":"stale_evidence"})),
        )
            .into_response();
    }
    let now = Utc::now().to_rfc3339();
    let insert=sqlx::query("INSERT INTO approvals(id,session_id,actor_user_id,decision,evidence_hash,reason,created_at) VALUES(?,?,?,?,?,?,?)").bind(Uuid::now_v7().to_string()).bind(&id).bind(&user_id).bind(&req.decision).bind(&req.evidence_hash).bind(&req.reason).bind(&now).execute(&s.db).await;
    if insert.is_err() {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error":"approval_already_recorded"})),
        )
            .into_response();
    }
    let next = if req.decision == "approve" {
        "publishing"
    } else {
        "rejected"
    };
    let approval_state = if req.decision == "approve" {
        "approved"
    } else {
        "rejected"
    };
    let _=sqlx::query("UPDATE sessions SET status=?,approval_state=?,updated_at=?,finished_at=CASE WHEN ?='rejected' THEN ? ELSE finished_at END WHERE id=?").bind(next).bind(approval_state).bind(&now).bind(next).bind(&now).bind(&id).execute(&s.db).await;
    let _=sqlx::query("INSERT INTO audit_log(actor_user_id,action,target_type,target_id,metadata_json,created_at) VALUES(?,?,?,?,?,?)").bind(&user_id).bind(format!("session_{}",req.decision)).bind("session").bind(&id).bind(json!({"evidence_hash":req.evidence_hash}).to_string()).bind(&now).execute(&s.db).await;
    Json(json!({"status":next})).into_response()
}

async fn api_session_cancel(
    State(s): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let Some(user_id) = auth_user(&s.db, &headers).await else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let now = Utc::now().to_rfc3339();
    let result=sqlx::query("UPDATE sessions SET status='cancelled',finished_at=?,updated_at=? WHERE id=? AND status IN ('queued','preparing','running','awaiting_approval')").bind(&now).bind(&now).bind(&id).execute(&s.db).await;
    if result.map(|r| r.rows_affected()).unwrap_or(0) == 0 {
        return StatusCode::NOT_FOUND.into_response();
    };
    let _=sqlx::query("INSERT INTO audit_log(actor_user_id,action,target_type,target_id,created_at) VALUES(?,?,?,?,?)").bind(user_id).bind("session_cancelled").bind("session").bind(&id).bind(now).execute(&s.db).await;
    Json(json!({"status":"cancelled"})).into_response()
}

async fn api_audit(State(s): State<Arc<AppState>>, headers: HeaderMap) -> impl IntoResponse {
    if auth_user(&s.db, &headers).await.is_none() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let rows=sqlx::query("SELECT action,target_type,target_id,metadata_json,created_at FROM audit_log ORDER BY id DESC LIMIT 200").fetch_all(&s.db).await.unwrap_or_default();
    Json(rows.into_iter().map(|r|json!({"action":r.get::<String,_>("action"),"target_type":r.get::<Option<String>,_>("target_type"),"target_id":r.get::<Option<String>,_>("target_id"),"metadata":r.get::<String,_>("metadata_json"),"created_at":r.get::<String,_>("created_at")})).collect::<Vec<_>>()).into_response()
}

async fn api_setup_status(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    Json(
        json!({"installed":is_installed(&s.db).await.unwrap_or(false),"version":env!("CARGO_PKG_VERSION")}),
    )
}

async fn api_projects(State(s): State<Arc<AppState>>, headers: HeaderMap) -> impl IntoResponse {
    if auth_user(&s.db, &headers).await.is_none() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let rows=sqlx::query("SELECT id,name,jira_project_key,gitlab_project_id,gitlab_project_path,default_branch,policy_json,enabled FROM project_bindings ORDER BY name").fetch_all(&s.db).await.unwrap_or_default();
    Json(rows.into_iter().map(|r|json!({"id":r.get::<String,_>("id"),"name":r.get::<String,_>("name"),"jira_project_key":r.get::<Option<String>,_>("jira_project_key"),"gitlab_project_id":r.get::<String,_>("gitlab_project_id"),"gitlab_project_path":r.get::<String,_>("gitlab_project_path"),"default_branch":r.get::<String,_>("default_branch"),"policy":r.get::<String,_>("policy_json"),"enabled":r.get::<i64,_>("enabled")!=0})).collect::<Vec<_>>()).into_response()
}

async fn api_project_create(
    State(s): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(v): Json<Value>,
) -> impl IntoResponse {
    let Some(user) = auth_user(&s.db, &headers).await else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let name = v.get("name").and_then(Value::as_str).unwrap_or("").trim();
    let gid = v
        .get("gitlab_project_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let path = v
        .get("gitlab_project_path")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if name.is_empty() || gid.is_empty() || path.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"name_gitlab_project_required"})),
        )
            .into_response();
    }
    let now = Utc::now().to_rfc3339();
    let id = Uuid::now_v7().to_string();
    let policy = v
        .get("policy")
        .cloned()
        .unwrap_or_else(|| json!({"mode":"read_only"}));
    let r=sqlx::query("INSERT INTO project_bindings(id,name,jira_project_key,gitlab_project_id,gitlab_project_path,default_branch,policy_json,enabled,created_at,updated_at) VALUES(?,?,?,?,?,?,?,1,?,?)").bind(&id).bind(name).bind(v.get("jira_project_key").and_then(Value::as_str)).bind(gid).bind(path).bind(v.get("default_branch").and_then(Value::as_str).unwrap_or("main")).bind(policy.to_string()).bind(&now).bind(&now).execute(&s.db).await;
    if r.is_err() {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error":"project_exists_or_invalid"})),
        )
            .into_response();
    }
    let _=sqlx::query("INSERT INTO audit_log(actor_user_id,action,target_type,target_id,created_at) VALUES(?,?,?,?,?)").bind(user).bind("project_created").bind("project").bind(&id).bind(now).execute(&s.db).await;
    (StatusCode::CREATED, Json(json!({"id":id}))).into_response()
}

async fn api_integrations(State(s): State<Arc<AppState>>, headers: HeaderMap) -> impl IntoResponse {
    if auth_user(&s.db, &headers).await.is_none() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let rows = sqlx::query(
        "SELECT id,kind,name,base_url,auth_mode,enabled FROM integrations ORDER BY kind,name",
    )
    .fetch_all(&s.db)
    .await
    .unwrap_or_default();
    Json(rows.into_iter().map(|r|json!({"id":r.get::<String,_>("id"),"kind":r.get::<String,_>("kind"),"name":r.get::<String,_>("name"),"base_url":r.get::<String,_>("base_url"),"auth_mode":r.get::<String,_>("auth_mode"),"enabled":r.get::<i64,_>("enabled")!=0})).collect::<Vec<_>>()).into_response()
}

async fn api_integration_create(
    State(s): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(v): Json<Value>,
) -> impl IntoResponse {
    let Some(user) = auth_user(&s.db, &headers).await else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let kind = v.get("kind").and_then(Value::as_str).unwrap_or("");
    let name = v.get("name").and_then(Value::as_str).unwrap_or("");
    let base = v.get("base_url").and_then(Value::as_str).unwrap_or("");
    if !["gitlab", "jira", "opencode"].contains(&kind) || name.is_empty() || base.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid_integration"})),
        )
            .into_response();
    }
    let now = Utc::now().to_rfc3339();
    let id = Uuid::now_v7().to_string();
    let r=sqlx::query("INSERT INTO integrations(id,kind,name,base_url,auth_mode,config_json,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?)").bind(&id).bind(kind).bind(name).bind(base).bind(v.get("auth_mode").and_then(Value::as_str).unwrap_or("token")).bind("{}").bind(&now).bind(&now).execute(&s.db).await;
    if r.is_err() {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error":"integration_exists_or_invalid"})),
        )
            .into_response();
    }
    let _=sqlx::query("INSERT INTO audit_log(actor_user_id,action,target_type,target_id,created_at) VALUES(?,?,?,?,?)").bind(user).bind("integration_created").bind("integration").bind(&id).bind(now).execute(&s.db).await;
    (StatusCode::CREATED, Json(json!({"id":id}))).into_response()
}

async fn api_mcp_servers(State(s): State<Arc<AppState>>, headers: HeaderMap) -> impl IntoResponse {
    if auth_user(&s.db, &headers).await.is_none() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let rows=sqlx::query("SELECT id,slug,name,transport,endpoint,enabled,allowed_hosts_json FROM mcp_servers ORDER BY slug").fetch_all(&s.db).await.unwrap_or_default();
    Json(rows.into_iter().map(|r|json!({"id":r.get::<String,_>("id"),"slug":r.get::<String,_>("slug"),"name":r.get::<String,_>("name"),"transport":r.get::<String,_>("transport"),"endpoint":r.get::<Option<String>,_>("endpoint"),"enabled":r.get::<i64,_>("enabled")!=0,"allowed_hosts":r.get::<String,_>("allowed_hosts_json")})).collect::<Vec<_>>()).into_response()
}

async fn api_mcp_create(
    State(s): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(v): Json<Value>,
) -> impl IntoResponse {
    let Some(user) = auth_user(&s.db, &headers).await else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let slug = v.get("slug").and_then(Value::as_str).unwrap_or("");
    let name = v.get("name").and_then(Value::as_str).unwrap_or("");
    let endpoint = v.get("endpoint").and_then(Value::as_str).unwrap_or("");
    if !valid_slug(slug) || name.is_empty() || !endpoint.starts_with("https://") {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid_mcp"})),
        )
            .into_response();
    }
    let now = Utc::now().to_rfc3339();
    let id = Uuid::now_v7().to_string();
    let r=sqlx::query("INSERT INTO mcp_servers(id,slug,name,transport,endpoint,allowed_hosts_json,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?)").bind(&id).bind(slug).bind(name).bind("remote_http").bind(endpoint).bind(v.get("allowed_hosts").cloned().unwrap_or_else(||json!([])).to_string()).bind(&now).bind(&now).execute(&s.db).await;
    if r.is_err() {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error":"mcp_exists_or_invalid"})),
        )
            .into_response();
    }
    let _=sqlx::query("INSERT INTO audit_log(actor_user_id,action,target_type,target_id,created_at) VALUES(?,?,?,?,?)").bind(user).bind("mcp_created").bind("mcp_server").bind(&id).bind(now).execute(&s.db).await;
    (StatusCode::CREATED, Json(json!({"id":id}))).into_response()
}

async fn api_skills(State(s): State<Arc<AppState>>, headers: HeaderMap) -> impl IntoResponse {
    if auth_user(&s.db, &headers).await.is_none() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let rows =
        sqlx::query("SELECT id,slug,name,description,enabled,version FROM skills ORDER BY slug")
            .fetch_all(&s.db)
            .await
            .unwrap_or_default();
    Json(rows.into_iter().map(|r|json!({"id":r.get::<String,_>("id"),"slug":r.get::<String,_>("slug"),"name":r.get::<String,_>("name"),"description":r.get::<String,_>("description"),"enabled":r.get::<i64,_>("enabled")!=0,"version":r.get::<i64,_>("version")})).collect::<Vec<_>>()).into_response()
}

async fn api_skill_create(
    State(s): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(v): Json<Value>,
) -> impl IntoResponse {
    if auth_user(&s.db, &headers).await.is_none() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let slug = v.get("slug").and_then(Value::as_str).unwrap_or("");
    let name = v.get("name").and_then(Value::as_str).unwrap_or("");
    let desc = v.get("description").and_then(Value::as_str).unwrap_or("");
    let content = v.get("content").and_then(Value::as_str).unwrap_or("");
    if !valid_slug(slug) || name.is_empty() || desc.is_empty() || content.len() > 65536 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid_skill"})),
        )
            .into_response();
    }
    let now = Utc::now().to_rfc3339();
    let result=sqlx::query("INSERT INTO skills(id,slug,name,description,content,created_at,updated_at) VALUES(?,?,?,?,?,?,?)").bind(Uuid::now_v7().to_string()).bind(slug).bind(name).bind(desc).bind(content).bind(&now).bind(&now).execute(&s.db).await;
    match result {
        Ok(_) => (StatusCode::CREATED, Json(json!({"status":"created"}))).into_response(),
        Err(_) => (StatusCode::CONFLICT, Json(json!({"error":"slug_exists"}))).into_response(),
    }
}

async fn webhook_jira(
    State(s): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(v): Json<Value>,
) -> impl IntoResponse {
    webhook_ingest(&s, "jira", &headers, &v, "jira_work_item").await
}
async fn webhook_gitlab_code(
    State(s): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(v): Json<Value>,
) -> impl IntoResponse {
    if !verify_gitlab(&s, &headers, &v) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    webhook_ingest(&s, "gitlab", &headers, &v, "gitlab_code").await
}
async fn webhook_pipeline_event(
    State(s): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(v): Json<Value>,
) -> impl IntoResponse {
    if !verify_gitlab(&s, &headers, &v) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let status = v
        .pointer("/object_attributes/status")
        .and_then(Value::as_str)
        .or_else(|| v.get("status").and_then(Value::as_str))
        .unwrap_or("");
    if status != "failed" {
        return (
            StatusCode::OK,
            Json(json!({"status":"ignored","reason":"pipeline_not_failed"})),
        )
            .into_response();
    }
    webhook_ingest(&s, "gitlab", &headers, &v, "gitlab_pipeline_failed").await
}

async fn webhook_ingest(
    s: &AppState,
    provider: &str,
    headers: &HeaderMap,
    v: &Value,
    event: &str,
) -> axum::response::Response {
    let key = headers
        .get("webhook-id")
        .or_else(|| headers.get("x-gitlab-event-uuid"))
        .or_else(|| headers.get("idempotency-key"))
        .and_then(|x| x.to_str().ok())
        .map(str::to_owned)
        .unwrap_or_else(|| hex_hash(v.to_string().as_bytes()));
    let now = Utc::now().to_rfc3339();
    let delivery = Uuid::now_v7().to_string();
    let inserted=sqlx::query("INSERT INTO webhook_deliveries(id,provider,delivery_key,event_type,status,received_at) VALUES(?,?,?,?,?,?)").bind(delivery).bind(provider).bind(&key).bind(event).bind("queued").bind(&now).execute(&s.db).await;
    if inserted.is_err() {
        return (StatusCode::OK, Json(json!({"status":"duplicate"}))).into_response();
    }
    let source_id = key.clone();
    let source = if provider == "jira" {
        "jira"
    } else if event == "gitlab_pipeline_failed" {
        "gitlab_pipeline"
    } else {
        "gitlab_mr"
    };
    let workflow = if event == "gitlab_pipeline_failed" {
        "pipeline_analysis"
    } else if provider == "jira" {
        "jira_implement"
    } else {
        "mr_review"
    };
    let mode = if workflow == "mr_review" {
        "review"
    } else if workflow == "pipeline_analysis" {
        "pipeline_analysis"
    } else {
        "implement"
    };
    let title = v
        .get("object_attributes")
        .and_then(|x| x.get("title"))
        .and_then(Value::as_str)
        .or_else(|| {
            v.get("issue")
                .and_then(|x| x.get("fields"))
                .and_then(|x| x.get("summary"))
                .and_then(Value::as_str)
        })
        .unwrap_or("Koda session");
    let sid = Uuid::now_v7().to_string();
    let session=sqlx::query("INSERT OR IGNORE INTO sessions(id,source,source_event_id,status,mode,title,metadata_json,workflow_type,policy_snapshot_json,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?)").bind(&sid).bind(source).bind(source_id).bind("queued").bind(mode).bind(title).bind(v.to_string()).bind(workflow).bind("{}").bind(&now).bind(&now).execute(&s.db).await;
    if session.is_ok() {
        let _=sqlx::query("INSERT INTO jobs(id,kind,status,payload_json,run_after,created_at,updated_at) VALUES(?,?,?,?,?,?,?)").bind(Uuid::now_v7().to_string()).bind("start_session").bind("queued").bind(json!({"session_id":sid,"workflow":workflow}).to_string()).bind(&now).bind(&now).bind(&now).execute(&s.db).await;
    }
    (
        StatusCode::ACCEPTED,
        Json(json!({"status":"accepted","session_id":sid,"workflow":workflow})),
    )
        .into_response()
}

fn verify_gitlab(_s: &AppState, headers: &HeaderMap, body: &Value) -> bool {
    let secret = env_or_file("GITLAB_WEBHOOK_SECRET");
    if secret.is_empty() {
        return true;
    }
    if let Some(token) = headers.get("x-gitlab-token").and_then(|x| x.to_str().ok()) {
        return constant_eq(secret.as_bytes(), token.as_bytes());
    }
    if let Some(sig) = headers
        .get("webhook-signature")
        .and_then(|x| x.to_str().ok())
    {
        if let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) {
            mac.update(body.to_string().as_bytes());
            return mac.verify_slice(sig.as_bytes()).is_ok();
        }
    }
    false
}

async fn worker_loop(s: Arc<AppState>) {
    let _ = s.cfg.max_sessions;
    loop {
        if let Ok(Some(r))=sqlx::query("SELECT id,kind,payload_json FROM jobs WHERE status='queued' AND run_after<=? ORDER BY created_at LIMIT 1").bind(Utc::now().to_rfc3339()).fetch_optional(&s.db).await { let id:String=r.get("id"); let kind:String=r.get("kind"); let payload:String=r.get("payload_json"); let _=sqlx::query("UPDATE jobs SET status='leased',attempts=attempts+1,leased_until=?,updated_at=? WHERE id=? AND status='queued'").bind((Utc::now()+Duration::minutes(30)).to_rfc3339()).bind(Utc::now().to_rfc3339()).bind(&id).execute(&s.db).await; let result=if kind=="start_session"{run_session(&s,&payload).await}else{trigger_pipeline(&s,&payload).await}; let status=if result.is_ok(){"done"}else{"failed"}; let _=sqlx::query("UPDATE jobs SET status=?,last_error=?,updated_at=? WHERE id=?").bind(status).bind(result.err().map(|e|e.to_string())).bind(Utc::now().to_rfc3339()).bind(id).execute(&s.db).await; } else {sleep(TokioDuration::from_secs(1)).await;}
    }
}

async fn run_session(s: &AppState, payload: &str) -> Result<(), String> {
    let v: Value = serde_json::from_str(payload).map_err(|e| e.to_string())?;
    let id = v["session_id"].as_str().ok_or("missing session")?;
    let workflow = v["workflow"].as_str().unwrap_or("legacy");
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE sessions SET status='preparing',started_at=?,updated_at=? WHERE id=?")
        .bind(&now)
        .bind(&now)
        .bind(id)
        .execute(&s.db)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("UPDATE sessions SET status='running',updated_at=? WHERE id=?")
        .bind(&now)
        .bind(id)
        .execute(&s.db)
        .await
        .map_err(|e| e.to_string())?;
    let health = reqwest::Client::new()
        .get(format!("{}/global/health", s.cfg.opencode_url))
        .send()
        .await;
    let detail = match health {
        Ok(r) => {
            json!({"opencode_status":r.status().as_u16(),"harness":"planned_ephemeral_runner"})
        }
        Err(e) => json!({"opencode_error":e.to_string(),"harness":"planned_ephemeral_runner"}),
    };
    let evidence = hex_hash(format!("{}:{}:{}", id, workflow, detail).as_bytes());
    sqlx::query(
        "INSERT INTO session_events(session_id,kind,payload_json,created_at) VALUES(?,?,?,?)",
    )
    .bind(id)
    .bind("evidence_ready")
    .bind(detail.to_string())
    .bind(&now)
    .execute(&s.db)
    .await
    .map_err(|e| e.to_string())?;
    if workflow == "mr_review" {
        let end = Utc::now().to_rfc3339();
        sqlx::query("UPDATE sessions SET status='succeeded',evidence_hash=?,approval_state='not_required',finished_at=?,updated_at=? WHERE id=?").bind(&evidence).bind(&end).bind(&end).bind(id).execute(&s.db).await.map_err(|e|e.to_string())?;
    } else {
        sqlx::query("UPDATE sessions SET status='awaiting_approval',approval_state='pending',evidence_hash=?,updated_at=? WHERE id=?").bind(&evidence).bind(&now).bind(id).execute(&s.db).await.map_err(|e|e.to_string())?;
    }
    Ok(())
}
async fn trigger_pipeline(_s: &AppState, _payload: &str) -> Result<(), String> {
    Ok(())
}

async fn auth_user(db: &SqlitePool, headers: &HeaderMap) -> Option<String> {
    let t = cookie(headers, "koda_session")?;
    let now = Utc::now().to_rfc3339();
    sqlx::query("SELECT user_id FROM auth_sessions WHERE token_hash=? AND expires_at>? ")
        .bind(hex_hash(&t))
        .bind(now)
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
        .map(|r| r.get("user_id"))
}
fn cookie(h: &HeaderMap, name: &str) -> Option<String> {
    h.get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|p| {
            let (k, v) = p.trim().split_once('=')?;
            if k == name { Some(v.to_owned()) } else { None }
        })
}
fn hex_hash<T: AsRef<[u8]>>(v: T) -> String {
    let mut h = Sha256::new();
    h.update(v);
    hex::encode(h.finalize())
}
fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |x, (u, v)| x | u ^ v) == 0
}
fn valid_slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--")
}
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
