use crate::repo::TicketRepo;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct AppState {
    pub repo: Arc<Mutex<TicketRepo>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            repo: Arc::new(Mutex::new(TicketRepo::new())),
        }
    }
}
