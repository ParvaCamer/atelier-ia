//! Identifiants typés. Un `TaskId` ne peut pas être passé là où un `AgentId`
//! est attendu — erreur de compilation, pas bug d'exécution.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

macro_rules! define_id {
    ($($name:ident),* $(,)?) => {$(
        // serde sérialise déjà un newtype comme sa valeur interne :
        // pas besoin de #[serde(transparent)], qui ne ferait qu'ajouter du bruit.
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
        #[ts(export)]
        pub struct $name(pub String);

        impl $name {
            /// UUID v7 : trié chronologiquement, ce qui rend les index SQLite
            /// séquentiels au lieu d'aléatoires.
            pub fn new() -> Self { Self(uuid::Uuid::now_v7().to_string()) }
            pub fn as_str(&self) -> &str { &self.0 }
        }
        impl Default for $name { fn default() -> Self { Self::new() } }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(&self.0) }
        }
        impl From<String> for $name { fn from(s: String) -> Self { Self(s) } }
        impl From<&str> for $name { fn from(s: &str) -> Self { Self(s.to_string()) } }
    )*};
}

define_id!(ProjectId, AgentId, WorkflowId, RunId, TaskId, ToolCallId, ApprovalId, MemoryId, LogId, PtyId, ScheduleId, HandoffId);
