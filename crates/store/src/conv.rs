//! Conversion enum ⇄ TEXT.
//!
//! Les enums du domaine sont déjà sérialisés en kebab-case pour l'IPC.
//! On réutilise cette représentation en base plutôt que d'en inventer une
//! seconde : une seule vérité, et la base reste lisible à la main.

use crate::error::{Result, StoreError};
use serde::{de::DeserializeOwned, Serialize};

pub fn enum_to_str<T: Serialize>(v: &T) -> String {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::String(s)) => s,
        other => format!("{other:?}"),
    }
}

pub fn str_to_enum<T: DeserializeOwned>(s: &str) -> Result<T> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).map_err(StoreError::Json)
}

pub fn json_to_vec<T: DeserializeOwned>(s: &str) -> Vec<T> {
    serde_json::from_str(s).unwrap_or_default()
}

pub fn vec_to_json<T: Serialize>(v: &[T]) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "[]".into())
}
