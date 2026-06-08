use actix_web::web;

use super::endpoints;

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route(
        "/config/frontend.toml",
        web::get().to(endpoints::config_frontend::handler),
    );
    cfg.service(
        web::scope("/api")
            .service(
                web::scope("/food")
                    .route("/ai-lookup", web::post().to(endpoints::ai_lookup::ai_lookup))
                    .route("/ai-vision", web::post().to(endpoints::ai_vision::ai_vision)),
            )
            .service(
                web::scope("/sync")
                    .route("/dump", web::post().to(endpoints::sync::sync_dump))
                    .route("/push", web::post().to(endpoints::sync::sync_push)),
            ),
    );
}
