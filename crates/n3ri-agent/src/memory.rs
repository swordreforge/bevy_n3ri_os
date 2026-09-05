//! 温冷记忆存储（M0 占位）。

#[derive(Debug, Clone)]
pub struct Fact {
    pub id: String,
    pub text: String,
    pub importance: u8,
    pub kind: String,
    pub created_at: String,
    pub absorbed: bool,
    pub hash: String,
}
