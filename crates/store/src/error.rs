use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] sqlx::Error),
    #[error("migration: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0} introuvable: {1}")]
    NotFound(&'static str, String),
    #[error("transition d'état interdite: {from} -> {to}")]
    IllegalTransition { from: String, to: String },
}

pub type Result<T> = std::result::Result<T, StoreError>;
