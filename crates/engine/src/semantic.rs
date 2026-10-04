//! Recherche par proximité de sens dans la mémoire, **en local uniquement**.
//!
//! Les vecteurs viennent d'Ollama (`/api/embeddings`), jamais d'un
//! fournisseur payant ni du registre (pas de repli). Ollama éteint, modèle
//! absent ou réglage vide : la recherche reste celle d'avant, FTS5 seul, à
//! l'identique et sans bruit.
//!
//! Les deux classements sont **fusionnés** (rang réciproque) plutôt que
//! l'un remplacé par l'autre : FTS5 reste imbattable sur un identifiant
//! exact (`testDebugUnitTest`), les vecteurs retrouvent une reformulation.

use crate::Engine;
use async_trait::async_trait;
use atelier_domain::*;
use atelier_store::repo;
use std::collections::HashMap;

/// Souvenirs comparés au plus par recherche : la similarité est calculée en
/// Rust, sur une base personnelle de quelques centaines d'entrées.
const MAX_CANDIDATES: i64 = 2_000;
/// En deçà, la proximité n'est que du bruit.
const MIN_SIMILARITY: f32 = 0.35;
/// Constante de la fusion par rang réciproque (valeur usuelle).
const RRF_K: f32 = 60.0;
const BACKFILL_BATCH: i64 = 32;

/// Source de vecteurs. Injectable : les tests fournissent des vecteurs à la
/// main, sans aucun appel réseau.
#[async_trait]
pub trait Embedder: Send + Sync {
    /// `(modèle, vecteur)`, ou `None` si indisponible.
    async fn embed(&self, text: &str) -> Option<(String, Vec<f32>)>;
}

impl Engine {
    /// Vecteur d'un texte : embedder injecté, sinon Ollama selon les réglages.
    async fn embed_text(&self, text: &str) -> Option<(String, Vec<f32>)> {
        if let Some(embedder) = &self.config.embedder {
            return embedder.embed(text).await;
        }
        let model = self.settings().await.ok()?.embedding_model.trim().to_string();
        if model.is_empty() {
            return None;
        }
        let ollama = repo::providers::list_providers(self.db()).await.ok()?.into_iter().find(|p| p.kind == "ollama" && p.enabled)?;
        let url = ollama.base_url.unwrap_or_else(|| crate::config::DEFAULT_OLLAMA_URL.into());
        match atelier_providers::ollama::embed(&url, &model, text).await {
            Ok(v) => Some((model, v)),
            Err(e) => {
                tracing::debug!("embeddings indisponibles, recherche par mots seulement : {e}");
                None
            }
        }
    }

    /// Calcule et range le vecteur d'un souvenir. Sans Ollama : rien, en silence.
    pub(crate) async fn index_memory(&self, entry: &MemoryEntry) {
        if let Some((model, vector)) = self.embed_text(&entry.content).await {
            if let Err(e) = repo::memory::set_embedding(self.db(), &entry.id, &model, &vector).await {
                tracing::warn!("vecteur du souvenir {} non enregistré : {e}", entry.id);
            }
        }
    }

    /// Indexe les souvenirs qui n'ont pas encore de vecteur (anciens, saisis
    /// pendant qu'Ollama était éteint, ou d'un autre modèle).
    pub async fn backfill_embeddings(&self) -> anyhow::Result<usize> {
        let Some((model, _)) = self.embed_text("atelier").await else { return Ok(0) };
        let mut done = 0;
        for entry in repo::memory::missing_embeddings(self.db(), &model, BACKFILL_BATCH).await? {
            match self.embed_text(&entry.content).await {
                Some((m, v)) if m == model => {
                    repo::memory::set_embedding(self.db(), &entry.id, &m, &v).await?;
                    done += 1;
                }
                _ => break,
            }
        }
        Ok(done)
    }

    /// Souvenirs rapprochés d'une requête : FTS5 et proximité de sens, fusionnés.
    /// Sans vecteurs disponibles, renvoie exactement le résultat de FTS5.
    pub async fn recall_memories(&self, project: &ProjectId, query: &str, limit: usize) -> anyhow::Result<Vec<MemoryEntry>> {
        let lexical = repo::memory::search(self.db(), project, query, limit as i64).await?;
        let Some((model, wanted)) = self.embed_text(query).await else { return Ok(lexical) };

        let mut scored: Vec<(f32, MemoryEntry)> = repo::memory::with_embeddings(self.db(), project, &model, MAX_CANDIDATES)
            .await?
            .into_iter()
            .filter_map(|(entry, v)| cosine(&wanted, &v).filter(|s| *s >= MIN_SIMILARITY).map(|s| (s, entry)))
            .collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        let semantic: Vec<MemoryEntry> = scored.into_iter().take(limit).map(|(_, e)| e).collect();

        Ok(fuse(&[lexical, semantic], limit))
    }
}

/// Similarité cosinus ; `None` si les dimensions diffèrent ou un vecteur est nul.
pub fn cosine(a: &[f32], b: &[f32]) -> Option<f32> {
    if a.len() != b.len() || a.is_empty() {
        return None;
    }
    let (mut dot, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    (na > 0.0 && nb > 0.0).then(|| dot / (na.sqrt() * nb.sqrt()))
}

/// Fusion par rang réciproque : une entrée bien classée dans l'une OU
/// l'autre liste remonte, une entrée présente dans les deux encore plus.
/// Avec une seule liste, l'ordre est conservé tel quel.
pub fn fuse(rankings: &[Vec<MemoryEntry>], limit: usize) -> Vec<MemoryEntry> {
    let mut scores: HashMap<MemoryId, (f32, usize, MemoryEntry)> = HashMap::new();
    let mut order = 0usize;
    for list in rankings {
        for (rank, entry) in list.iter().enumerate() {
            let slot = scores.entry(entry.id.clone()).or_insert_with(|| {
                order += 1;
                (0.0, order, entry.clone())
            });
            slot.0 += 1.0 / (RRF_K + rank as f32 + 1.0);
        }
    }
    let mut out: Vec<(f32, usize, MemoryEntry)> = scores.into_values().collect();
    // À score égal, ordre d'apparition : déterministe.
    out.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    out.into_iter().take(limit).map(|(_, _, e)| e).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn entry(id: &str) -> MemoryEntry {
        MemoryEntry {
            id: MemoryId(id.into()), scope: MemoryScope::Project, kind: MemoryKind::Fact, project_id: None, agent_id: None,
            run_id: None, task_id: None, content: id.into(), importance: 0.5, created_at: Utc::now(),
        }
    }
    fn ids(v: &[MemoryEntry]) -> Vec<&str> {
        v.iter().map(|e| e.id.as_str()).collect()
    }

    #[test]
    fn similarite() {
        assert!((cosine(&[1.0, 0.0], &[2.0, 0.0]).unwrap() - 1.0).abs() < 1e-6);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).unwrap().abs() < 1e-6);
        assert_eq!(cosine(&[1.0], &[1.0, 2.0]), None, "dimensions différentes : incomparables");
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 2.0]), None);
    }

    #[test]
    fn fusion_des_classements() {
        let lexical = vec![entry("a"), entry("b"), entry("c")];
        assert_eq!(ids(&fuse(&[lexical.clone()], 10)), ["a", "b", "c"], "une seule liste : ordre inchangé");
        assert_eq!(ids(&fuse(&[lexical.clone(), vec![]], 2)), ["a", "b"]);
        // « c » est troisième en mots mais premier en sens : il passe devant « b ».
        let fused = fuse(&[lexical, vec![entry("c"), entry("d")]], 10);
        assert_eq!(ids(&fused)[0], "c");
        assert!(ids(&fused).contains(&"d"), "l'entrée que FTS5 rate est retrouvée");
    }
}
