use actix_cors::Cors;
use actix_governor::{GlobalKeyExtractor, Governor, GovernorConfigBuilder};
use std::collections::HashMap;
use std::time::Duration;

use actix_web::{get, middleware, web, App, HttpResponse, HttpServer};
use actix_web_prometheus::PrometheusMetricsBuilder;

use structopt::StructOpt;

mod core;

const MAX_JSON_PAYLOAD: usize = 1024 * 1024; // 1 MB

#[derive(StructOpt, Debug)]
#[structopt(name = "server")]
pub struct FeedbackKeys {
    // Feedback
    /// GitHub personal access token
    #[structopt(short = "t", long)]
    github_token: Option<String>,
    /// Secret for the feedback token generation
    #[structopt(short = "jwt", long)]
    jwt_key: Option<String>,
}

#[get("/api/feedback/status")]
async fn health_status_handler() -> HttpResponse {
    let github_link = match std::env::var("GIT_COMMIT_SHA") {
        Ok(hash) => format!("https://github.com/TUM-Dev/navigatum/tree/{hash}"),
        Err(_) => "unknown commit hash, probably running in development".to_string(),
    };
    HttpResponse::Ok()
        .content_type("text/plain")
        .body(format!("healthy\nsource_code: {github_link}"))
}

const SECONDS_PER_DAY: u64 = 60 * 60 * 24;
#[tokio::main]
async fn main() -> std::io::Result<()> {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    let mut opt = FeedbackKeys::from_args();
    if opt.github_token.is_none() {
        opt.github_token = std::env::var("GITHUB_TOKEN").ok();
    }
    if opt.jwt_key.is_none() {
        opt.jwt_key = std::env::var("JWT_KEY").ok();
    }

    let feedback_ratelimit = GovernorConfigBuilder::default()
        .key_extractor(GlobalKeyExtractor)
        .period(Duration::from_secs(SECONDS_PER_DAY / 50))
        .burst_size(20)
        .finish()
        .unwrap();

    // metrics
    let labels = HashMap::from([(
        "revision".to_string(),
        std::env::var("GIT_COMMIT_SHA").unwrap_or("development".to_string()),
    )]);
    let prometheus = PrometheusMetricsBuilder::new("navigatum_feedback")
        .endpoint("/metrics")
        .const_labels(labels)
        .build()
        .unwrap();

    let state_feedback = web::Data::new(core::AppStateFeedback::from(opt));
    HttpServer::new(move || {
        let cors = Cors::default()
            .allow_any_origin()
            .allow_any_header()
            .allowed_methods(vec!["GET", "POST"])
            .max_age(3600);
        App::new()
            .wrap(cors)
            .wrap(middleware::Logger::default().exclude("/api/feedback/status"))
            .wrap(middleware::Compress::default())
            .wrap(prometheus.clone())
            .app_data(web::JsonConfig::default().limit(MAX_JSON_PAYLOAD))
            .service(health_status_handler)
            .app_data(state_feedback.clone())
            .service(core::send_feedback)
            .service(
                web::scope("")
                    .wrap(Governor::new(&feedback_ratelimit))
                    .service(core::get_token),
            )
    })
    .bind(std::env::var("BIND_ADDRESS").unwrap_or_else(|_| "0.0.0.0:8070".to_string()))?
    .run()
    .await
}
