use tauri::ipc::InvokeError;

#[derive(Debug, thiserror::Error)]
pub enum StudioError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
    #[error(transparent)]
    Zip(#[from] zip::result::ZipError),
    #[error(transparent)]
    Csv(#[from] csv::Error),
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    #[error(transparent)]
    Keyring(#[from] keyring::Error),
    #[error(transparent)]
    Walkdir(#[from] walkdir::Error),
}

impl From<StudioError> for InvokeError {
    fn from(value: StudioError) -> Self {
        InvokeError::from(value.to_string())
    }
}

pub type StudioResult<T> = Result<T, StudioError>;

pub fn err(message: impl Into<String>) -> StudioError {
    StudioError::Message(message.into())
}
