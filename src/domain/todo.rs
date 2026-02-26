#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Todo {
    pub id: i64,
    pub project_id: i64,
    pub title: String,
    pub done: bool,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}
