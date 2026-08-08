use serde::Serialize;

#[derive(Clone, Serialize)]
pub struct Ticket {
    pub id: u64,
    pub title: String,
}

pub struct TicketRepo {
    tickets: Vec<Ticket>,
    next_id: u64,
}

impl TicketRepo {
    pub fn new() -> Self {
        Self {
            tickets: Vec::new(),
            next_id: 1,
        }
    }

    pub fn find_all(&self) -> Vec<Ticket> {
        self.tickets.clone()
    }

    pub fn find_by_id(&self, id: u64) -> Option<Ticket> {
        self.tickets.iter().find(|t| t.id == id).cloned()
    }

    pub fn insert(&mut self, title: String) -> Ticket {
        let id = self.next_id();
        let ticket = Ticket { id, title };
        self.tickets.push(ticket.clone());
        ticket
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}
