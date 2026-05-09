use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

/// Simple local RAG store — SQLite + llama.cpp embeddings.

const CHUNK_SIZE: usize = 512; // chars per chunk
const CHUNK_OVERLAP: usize = 64;

pub struct RagStore {
    db: Mutex<Connection>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub text: String,
    pub source: String,
    pub score: f64,
}

#[derive(Deserialize)]
struct LlamaCppEmbedResponse {
    embedding: Vec<f64>,
}

impl RagStore {
    pub fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let db_path = Self::db_path();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&db_path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS chunks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source TEXT NOT NULL,
                text TEXT NOT NULL,
                embedding BLOB NOT NULL,
                created_at TEXT DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_chunks_source ON chunks(source);",
        )?;
        Ok(Self {
            db: Mutex::new(conn),
        })
    }

    fn db_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"))
            .join("voice-assistant")
            .join("knowledge.db")
    }

    /// Ingest a text document — chunk it, embed each chunk, store.
    pub async fn ingest(
        &self,
        source: &str,
        text: &str,
        llm_url: &str,
        embed_model: &str,
    ) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        let chunks = chunk_text(text, CHUNK_SIZE, CHUNK_OVERLAP);
        if chunks.is_empty() {
            return Ok(0);
        }

        // Embed all chunks in parallel
        let embeddings = embed_texts(llm_url, embed_model, &chunks).await?;

        let db = self.db.lock().unwrap();
        // Remove old chunks from this source
        db.execute("DELETE FROM chunks WHERE source = ?1", params![source])?;

        let mut stmt = db.prepare(
            "INSERT INTO chunks (source, text, embedding) VALUES (?1, ?2, ?3)",
        )?;

        for (chunk, embedding) in chunks.iter().zip(embeddings.iter()) {
            let blob = embedding_to_blob(embedding);
            stmt.execute(params![source, chunk, blob])?;
        }

        Ok(chunks.len())
    }

    /// Search the knowledge base.
    pub async fn search(
        &self,
        query: &str,
        llm_url: &str,
        embed_model: &str,
        top_k: usize,
    ) -> Result<Vec<SearchResult>, Box<dyn std::error::Error + Send + Sync>> {
        let query_embeddings = embed_texts(llm_url, embed_model, &[query.to_string()]).await?;
        let query_emb = &query_embeddings[0];

        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT source, text, embedding FROM chunks")?;
        let rows = stmt.query_map([], |row| {
            let source: String = row.get(0)?;
            let text: String = row.get(1)?;
            let blob: Vec<u8> = row.get(2)?;
            Ok((source, text, blob))
        })?;

        let mut results: Vec<SearchResult> = Vec::new();
        for row in rows {
            let (source, text, blob) = row?;
            let embedding = blob_to_embedding(&blob);
            let score = cosine_similarity(query_emb, &embedding);
            results.push(SearchResult {
                text,
                source,
                score,
            });
        }

        // Sort by score descending, take top_k
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(top_k);

        // Filter out low-relevance results
        results.retain(|r| r.score > 0.3);

        Ok(results)
    }

    /// List all ingested sources with chunk counts.
    pub fn list_sources(&self) -> Result<Vec<(String, usize)>, Box<dyn std::error::Error + Send + Sync>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT source, COUNT(*) FROM chunks GROUP BY source ORDER BY source")?;
        let rows = stmt.query_map([], |row| {
            let source: String = row.get(0)?;
            let count: usize = row.get(1)?;
            Ok((source, count))
        })?;
        let mut sources = Vec::new();
        for row in rows {
            sources.push(row?);
        }
        Ok(sources)
    }

    /// Delete a source from the knowledge base.
    pub fn delete_source(&self, source: &str) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        let db = self.db.lock().unwrap();
        let deleted = db.execute("DELETE FROM chunks WHERE source = ?1", params![source])?;
        Ok(deleted)
    }
}

// ── Helpers ──

fn chunk_text(text: &str, chunk_size: usize, overlap: usize) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    if text.len() <= chunk_size {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let end = (start + chunk_size).min(text.len());
        let chunk_end = if end < text.len() {
            let slice = &text[start..end];
            if let Some(pos) = slice.rfind(". ") {
                start + pos + 2
            } else if let Some(pos) = slice.rfind('\n') {
                start + pos + 1
            } else if let Some(pos) = slice.rfind(' ') {
                start + pos + 1
            } else {
                end
            }
        } else {
            end
        };

        let chunk = text[start..chunk_end].trim().to_string();
        if !chunk.is_empty() {
            chunks.push(chunk);
        }

        start = if chunk_end > start + overlap {
            chunk_end - overlap
        } else {
            chunk_end
        };
    }

    chunks
}

/// Embed texts using llama.cpp's /embedding endpoint.
/// Makes concurrent requests for batch embedding.
async fn embed_texts(
    llm_url: &str,
    _model: &str,
    texts: &[String],
) -> Result<Vec<Vec<f64>>, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    // Make concurrent requests for each text
    let futures: Vec<_> = texts.iter().map(|text| {
        let client = client.clone();
        let url = format!("{}/embedding", llm_url);
        let body = serde_json::json!({
            "content": text,
        });

        async move {
            let resp = client
                .post(&url)
                .json(&body)
                .send()
                .await
                .map_err(|e| format!("Embedding request failed: {}", e))?;

            let status = resp.status();
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Err(format!("Embedding API error {}: {}", status, body));
            }

            let result: LlamaCppEmbedResponse = resp.json().await
                .map_err(|e| format!("Failed to parse embedding response: {}", e))?;
            Ok::<Vec<f64>, String>(result.embedding)
        }
    }).collect();

    let results = futures::future::join_all(futures).await;
    let mut embeddings = Vec::new();
    for result in results {
        match result {
            Ok(emb) => embeddings.push(emb),
            Err(e) => return Err(e.into()),
        }
    }

    Ok(embeddings)
}

fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let norm_b: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

fn embedding_to_blob(embedding: &[f64]) -> Vec<u8> {
    embedding
        .iter()
        .flat_map(|f| f.to_le_bytes())
        .collect()
}

fn blob_to_embedding(blob: &[u8]) -> Vec<f64> {
    blob.chunks_exact(8)
        .map(|chunk| {
            let bytes: [u8; 8] = chunk.try_into().unwrap();
            f64::from_le_bytes(bytes)
        })
        .collect()
}
