#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, thiserror::Error)]
pub enum ValidationError {
    #[error("{0}")]
    Message(String),
}

impl From<String> for ValidationError {
    fn from(value: String) -> Self {
        Self::Message(value)
    }
}

impl From<&str> for ValidationError {
    fn from(value: &str) -> Self {
        Self::Message(value.to_owned())
    }
}
