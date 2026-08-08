mod handlers;
mod repo;
mod state;

use axum::{
    routing::{get, post},
    Router,
};
use state::AppState;
use std::net::SocketAddr;

fn router() -> Router {
    let state = AppState::new();
    Router::new()
        .route(
            "/api/tickets",
            get(handlers::list_tickets).post(handlers::create_ticket),
        )
        .route("/api/tickets/:id", get(handlers::get_ticket))
        .with_state(state)
}

#[tokio::main]
async fn main() {
    let app = router();
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("Listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
