//! rustviz-events server. Configuration is entirely environment variables;
//! see the crate README.

use actix_web::{web, App, HttpServer};
use rustviz_events::{routes, AppState, Config, DashboardSource, SinkKind};

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Logs go to stderr and only ever describe the service itself (startup
    // config, sink failures). No request logger: actix's `Logger` would
    // print the peer address.
    env_logger::Builder::from_env(env_logger::Env::new().default_filter_or("info"))
        .target(env_logger::Target::Stderr)
        .init();

    let config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("rustviz-events: {e}");
            std::process::exit(2);
        }
    };
    let bind = std::env::var("RV_BIND").unwrap_or_else(|_| match std::env::var("PORT") {
        Ok(port) => format!("0.0.0.0:{port}"),
        Err(_) => "127.0.0.1:8090".to_string(),
    });

    log::info!(
        "rustviz-events {} on {bind}: sink={}, programs={}, origins=[{}], dashboard={}",
        env!("CARGO_PKG_VERSION"),
        match config.sink {
            SinkKind::Stdout => "stdout".to_string(),
            SinkKind::File => format!("file:{}", config.events_file.display()),
        },
        if config.policy.accept_programs {
            "accepted"
        } else {
            "rejected"
        },
        config.allowed_origins.join(", "),
        match (&config.dashboard_source, &config.dashboard_token) {
            (_, None) => "disabled (no RV_EVENTS_DASHBOARD_TOKEN)".to_string(),
            (DashboardSource::None, _) => "no data source".to_string(),
            (DashboardSource::File(p), _) => format!("file:{}", p.display()),
            (
                DashboardSource::BigQuery {
                    project, dataset, ..
                },
                _,
            ) => format!("bigquery:{project}.{dataset}"),
        },
    );

    let state = web::Data::new(AppState::new(config)?);
    HttpServer::new(move || App::new().app_data(state.clone()).configure(routes))
        .bind(bind)?
        .run()
        .await
}
