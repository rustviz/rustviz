use actix_cors::Cors;
use actix_files::Files;
use actix_governor::{Governor, GovernorConfigBuilder, KeyExtractor, SimpleKeyExtractionError};
use actix_web::{
    dev::ServiceRequest, error, http::header, middleware::Logger, web, App, HttpResponse,
    HttpServer,
};
use std::net::IpAddr;
use rustviz_lib::{error_kind, ErrorKind, Rustviz};
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

    // Rustviz::new blocks for the whole compile (up to the runner
    // timeout). Run it on actix's blocking pool so the async workers keep
    // serving other requests (and the platform health check) meanwhile.
    let code = code.clone();
    let result = web::block(move || {
        Rustviz::new(&code).map_err(|e| (error_kind(e.as_ref()), e.to_string()))
    })
    .await;
    match result {
        Ok(Ok(rv)) => HttpResponse::Ok().json(SubmitResponse {
            code_panel: rv.code_panel_string(),
            timeline_panel: rv.timeline_panel_string(),
        }),
        Ok(Err((kind, msg))) => error_response(kind, msg),
        Err(e) => HttpResponse::from_error(error::ErrorInternalServerError(e)),
    }
}

/// Response header naming the [`ErrorKind`] of a failed /submit-code
/// (`does-not-compile`, `unsupported`, `timeout`, ...). The body stays the
/// plain-text message, so frontends that predate the header still show it.
const ERROR_KIND_HEADER: &str = "X-RustViz-Error-Kind";

fn error_response(kind: ErrorKind, msg: String) -> HttpResponse {
    let mut resp = match kind {
        // The student's program, not the service: 422 Unprocessable Content.
        ErrorKind::DoesNotCompile => HttpResponse::UnprocessableEntity(),
        ErrorKind::Unsupported => {
            // Which constructs crash the plugin, for triage. The message
            // names the plugin source location, not the user's code.
            eprintln!("unsupported: {}", msg);
            HttpResponse::UnprocessableEntity()
        }
        ErrorKind::Timeout => HttpResponse::GatewayTimeout(),
        ErrorKind::SandboxUnavailable => {
            eprintln!("runner failure: {}", msg);
            return HttpResponse::ServiceUnavailable()
                .insert_header((ERROR_KIND_HEADER, kind.as_str()))
                .body("compile sandbox unavailable; try again shortly");
        }
        _ => HttpResponse::InternalServerError(),
    };
    resp.insert_header((ERROR_KIND_HEADER, kind.as_str()))
        .content_type("text/plain; charset=utf-8")
        .body(msg)
}

/// Rate-limit key: the real client IP.
///
/// Behind a platform proxy the TCP peer is the proxy, so keying on it
/// (actix-governor's default) puts every user in one shared bucket. When
/// `RV_CLIENT_IP_HEADER` names a header the platform sets, we key on that
/// instead:
///
/// * Fly: `Fly-Client-IP` (single value, overwritten by Fly's proxy).
/// * Cloud Run: `X-Forwarded-For`. Google's front end *appends* the client
///   address, so clients can prepend junk but not forge the last entry; we
///   take the rightmost value.
///
/// Unset (local dev) falls back to the peer address.
#[derive(Clone)]
struct ClientIp {
    header: Option<header::HeaderName>,
}

impl KeyExtractor for ClientIp {
    type Key = IpAddr;
    type KeyExtractionError = SimpleKeyExtractionError<&'static str>;

    fn extract(&self, req: &ServiceRequest) -> Result<IpAddr, Self::KeyExtractionError> {
        let from_header = self.header.as_ref().and_then(|h| {
            req.headers()
                .get(h)?
                .to_str()
                .ok()?
                .rsplit(',')
                .next()?
                .trim()
                .parse()
                .ok()
        });
        from_header
            .or_else(|| req.peer_addr().map(|a| a.ip()))
            .ok_or_else(|| SimpleKeyExtractionError::new("could not determine client IP"))
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    // The playground compiles arbitrary user-supplied Rust, so it
    // MUST run the plugin in the sandboxed Docker runner unless an
    // operator explicitly opts out (local dev). The rustviz-lib
    // default flipped to `local` in PR C of the reorg — that's the
    // right choice for library callers + the CLI, but for this
    // binary specifically we want Docker-by-default to preserve
    // the prior security posture. Production (Cloud Run) sets
    // RV_RUNNER=sandbox in its image; this guard catches the
    // local-dev case where the env var is unset.
    if std::env::var("RV_RUNNER").is_err() {
        std::env::set_var("RV_RUNNER", "docker");
    }

    // Cloud Run tells the container which port to listen on via $PORT.
    let bind_addr = std::env::var("RV_BIND").unwrap_or_else(|_| match std::env::var("PORT") {
        Ok(port) => format!("0.0.0.0:{}", port),
        Err(_) => "127.0.0.1:8080".to_string(),
    });

    // Per-IP token bucket. `seconds_per_replenish` controls the steady-state
    // rate (one token per N seconds); `burst_size` is the bucket depth.
    // Tunable via env so the deploy can adjust without a recompile.
    let seconds_per_replenish = std::env::var("RV_RATE_SECONDS_PER_REQUEST")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&n: &u64| n > 0)
        .unwrap_or(2u64);
    let rate_burst = std::env::var("RV_RATE_BURST")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&n: &u32| n > 0)
        .unwrap_or(5u32);
    let client_ip = ClientIp {
        header: std::env::var("RV_CLIENT_IP_HEADER")
            .ok()
            .and_then(|h| header::HeaderName::from_bytes(h.as_bytes()).ok()),
    };
    let governor_conf = GovernorConfigBuilder::default()
        .key_extractor(client_ip)
        .per_second(seconds_per_replenish)
        .burst_size(rate_burst)
        .finish()
        .expect("governor config");

    HttpServer::new(move || {
        // CORS allowlist. Local dev serves the SPA and the API from the
        // same origin (no preflight needed); production hosts the SPA on
        // GitHub Pages and calls /submit-code on the Cloud Run origin. That
        // cross-origin path needs explicit CORS. Vite dev server origins are listed too so
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
            // Cross-origin JS can only read response headers listed here.
            .expose_headers([ERROR_KIND_HEADER])
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

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::http::StatusCode;

    fn kind_header(resp: &HttpResponse) -> &str {
        resp.headers()
            .get(ERROR_KIND_HEADER)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
    }

    #[test]
    fn student_errors_are_422_with_kind_header() {
        for kind in [ErrorKind::DoesNotCompile, ErrorKind::Unsupported] {
            let resp = error_response(kind, "msg".to_string());
            assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
            assert_eq!(kind_header(&resp), kind.as_str());
        }
    }

    #[test]
    fn service_errors_keep_their_statuses() {
        let resp = error_response(ErrorKind::Timeout, "Visualization timed out".to_string());
        assert_eq!(resp.status(), StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(kind_header(&resp), "timeout");
        let resp = error_response(ErrorKind::SandboxUnavailable, "Sandbox unavailable: x".to_string());
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let resp = error_response(ErrorKind::Other, "boom".to_string());
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(kind_header(&resp), "other");
    }
}
