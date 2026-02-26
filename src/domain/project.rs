#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub archived: bool,
    pub created_at: String,
    pub updated_at: String,
}
