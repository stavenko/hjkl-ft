//! # DEPRECATED
//!
//! This backend is no longer used. The app is offline-first (CRUD in
//! IndexedDB) with AI + sync on the Cloudflare workers. Kept for reference
//! only — do not extend or depend on it. See `backend/DEPRECATED.md`.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

mod api;
mod config;
mod providers;
mod use_cases;

use api::endpoints::config_frontend::FrontendConfigPath;

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Run {
        /// Path to config file
        #[arg(short, long)]
        config: PathBuf,
    },
}

#[actix_web::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("backend=debug".parse().unwrap()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Run { config } => run_server(&config).await,
    }
}

async fn run_server(config_path: &PathBuf) {
    let config = config::load(config_path);
    let db = providers::database::Database::open(&config.database_path);
    db.run_migrations();

    let db_data = actix_web::web::Data::new(db);
    let frontend_config_path =
        actix_web::web::Data::new(FrontendConfigPath(PathBuf::from(config.frontend_config_path)));
    let llm_executor = actix_web::web::Data::new(providers::llm::create_executor(&config.llm));
    let vision_config = actix_web::web::Data::new(config.vision.clone());

    tracing::info!("Starting server at {}:{}", config.addr, config.port);

    actix_web::HttpServer::new(move || {
        let cors = actix_cors::Cors::permissive();
        let payload_cfg = actix_web::web::PayloadConfig::new(50 * 1024 * 1024); // 50MB
        actix_web::App::new()
            .wrap(cors)
            .app_data(payload_cfg)
            .app_data(db_data.clone())
            .app_data(frontend_config_path.clone())
            .app_data(llm_executor.clone())
            .app_data(vision_config.clone())
            .configure(api::configurator::configure)
    })
    .bind((config.addr.as_str(), config.port))
    .expect("failed to bind server")
    .run()
    .await
    .expect("server error");
}
