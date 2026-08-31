use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    ReviveFailed,
    InitModelFailed,
    InvalidMoc,
    InvalidInput(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReviveFailed => write!(f, "Moc revive failed"),
            Self::InitModelFailed => write!(f, "Model initialization failed"),
            Self::InvalidMoc => write!(f, "Moc consistency check failed"),
            Self::InvalidInput(msg) => write!(f, "Invalid input: {msg}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
