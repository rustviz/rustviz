use actix_cors::Cors;
use actix_files::Files;
use actix_governor::{Governor, GovernorConfigBuilder};
use actix_web::{error, http::header, middleware::Logger, web, App, HttpResponse, HttpServer};
use rustviz2::Rustviz;
use serde::{Deserialize, Serialize};

/// Hard cap on the JSON body Actix will accept on /submit-code.
/// 16 KiB is comfortably above the largest hand-written teaching example
/// while leaving no headroom for an attacker to ship a hostile macro corpus.
const MAX_BODY_BYTES: usize = 16 * 1024;

/// Same cap, applied to the inner `code` string after JSON parsing. Belt and
/// suspenders — the body limit is the actual gate, but checking again here
/// gives us a clean error message instead of a silent truncation if the
/// frontend ever stops constraining input.
const MAX_CODE_BYTES: usize = 16 * 1024;

#[derive(Deserialize)]
struct SubmitCodePayload {
    code: String,
}

#[derive(Deserialize, Serialize)]
struct SubmitResponse {
    code_panel: String,
    timeline_panel: String,
}

async fn submit_code(payload: web::Json<SubmitCodePayload>) -> HttpResponse {
    let code = &payload.code;
    if code.len() > MAX_CODE_BYTES {
        return HttpResponse::PayloadTooLarge()
            .body(format!("code must be at most {} bytes", MAX_CODE_BYTES));
    }

    match Rustviz::new(code) {
        Ok(rv) => {
            let body = serde_json::to_string(&SubmitResponse {
                code_panel: rv.code_panel_string(),
                timeline_panel: rv.timeline_panel_string(),
            })
            .unwrap();
            HttpResponse::Ok().body(body.into_bytes())
        }
        Err(e) => HttpResponse::from_error(
            <Box<dyn std::error::Error> as Into<error::Error>>::into(e),
        ),
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    // The playground compiles arbitrary user-supplied Rust, so it
    // MUST run the plugin in the sandboxed Docker runner unless an
    // operator explicitly opts out (local dev). The rustviz2 lib's
    // default flipped to `local` in PR C of the reorg — that's the
    // right choice for library callers + the CLI, but for this
    // binary specifically we want Docker-by-default to preserve
    // the prior security posture. Production (Fly) sets RV_RUNNER
    // explicitly via fly.toml; this guard catches the local-dev
    // case where the env var is unset.
    if std::env::var("RV_RUNNER").is_err() {
        std::env::set_var("RV_RUNNER", "docker");
    }

    let bind_addr = std::env::var("RV_BIND").unwrap_or_else(|_| "127.0.0.1:8080".to_string());

    // Per-IP token bucket. `seconds_per_replenish` controls the steady-state
    // rate (one token per N seconds); `burst_size` is the bucket depth.
    // Tunable via env so the deploy can adjust without a recompile.
    let seconds_per_replenish = std::env::var("RV_RATE_SECONDS_PER_REQUEST")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2u64);
    let rate_burst = std::env::var("RV_RATE_BURST")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5u32);
    let governor_conf = GovernorConfigBuilder::default()
        .per_second(seconds_per_replenish)
        .burst_size(rate_burst)
        .finish()
        .expect("governor config");

    HttpServer::new(move || {
        // CORS allowlist. The all-in-one Fly deploy serves the SPA and the
        // API from the same origin (no preflight needed) but we also support
        // hosting the static frontend on GitHub Pages and only proxying
        // /submit-code back to the Fly origin. That cross-origin path needs
        // explicit CORS. Vite dev server origins are listed too so
        // `npm run dev` can talk to a locally-running playground.
        //
        // The allowlist is *the* control over who can drive the API from a
        // browser; widen it sparingly. Adding a new public origin here also
        // means agreeing to absorb the compute cost of its traffic.
        let cors = Cors::default()
            .allowed_origin("https://rustviz.github.io")
            .allowed_origin("http://localhost:3000")
            .allowed_origin("http://127.0.0.1:3000")
            .allowed_methods(["GET", "POST", "OPTIONS"])
            .allowed_headers([header::CONTENT_TYPE])
            .max_age(3600);

        App::new()
            .wrap(Logger::default())
            .wrap(cors)
            .app_data(web::JsonConfig::default().limit(MAX_BODY_BYTES))
            .service(
                web::resource("/submit-code")
                    .wrap(Governor::new(&governor_conf))
                    .route(web::post().to(submit_code)),
            )
            // Vite emits the SPA + hashed assets/ + the ex-assets/ helper
            // scripts under frontend/dist/. Single mount handles all static
            // routes (assets/*, ex-assets/*, manifest.json, /).
            .service(Files::new("/", "./frontend/dist/").index_file("index.html"))
    })
    .bind(bind_addr)?
    .run()
    .await
}
