use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;

use crate::repo::Ticket;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct CreateTicketRequest {
    pub title: String,
}

pub async fn list_tickets(
    State(state): State<AppState>,
) -> Json<Vec<Ticket>> {
    let repo = state.repo.lock().unwrap();
    Json(repo.find_all())
}

pub async fn get_ticket(
    State(state): State<AppState>,
    Path(id): Path<u64>,
) -> Result<Json<Ticket>, StatusCode> {
    let repo = state.repo.lock().unwrap();
    repo.find_by_id(id)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

pub async fn create_ticket(
    State(state): State<AppState>,
    Json(payload): Json<CreateTicketRequest>,
) -> (StatusCode, Json<Ticket>) {
    let mut repo = state.repo.lock().unwrap();
    let ticket = repo.insert(payload.title);
    (StatusCode::CREATED, Json(ticket))
}
