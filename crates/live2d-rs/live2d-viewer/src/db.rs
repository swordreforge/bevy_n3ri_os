use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use tokio::runtime::Runtime;

use crate::ai::types::{CharacterCard, MemoryEntry};

// ---------------------------------------------------------------------------
// Word-vector embedding via character n-gram hashing trick (FNV-1a)
// ---------------------------------------------------------------------------

/// Dimensionality of embedding vectors.
const EMBED_DIM: usize = 128;

/// Embed `text` into an L2-normalised f32 vector using character n-gram
/// feature hashing (FNV-1a).  Zero external dependencies — pure std.
///
/// Unigrams, bigrams, and trigrams are each hashed into `[0, EMBED_DIM)`,
/// weighted by log-frequency `ln(1 + count)`, then L2-normalised.
fn embed_text(text: &str) -> Vec<f32> {
    let mut vec = vec![0.0f32; EMBED_DIM];
    let chars: Vec<char> = text.chars().collect();
    // 1-3 grams
    for span_len in 1..=3 {
        for window in chars.windows(span_len) {
            let mut hash: u64 = span_len as u64;
            for &c in window {
                hash = hash.wrapping_mul(0x100000001b3) ^ (c as u64);
            }
            let idx = (hash as usize) % EMBED_DIM;
            vec[idx] += 1.0;
        }
    }
    // Log-frequency scaling
    for v in &mut vec {
        *v = (*v).ln_1p();
    }
    // L2 normalise
    let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for v in &mut vec {
            *v /= norm;
        }
    }
    vec
}

/// Cosine similarity between two vectors (dot product for L2-normalised).
fn cosine_sim(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Single-threaded runtime used to bridge libsql's async API into the
/// viewer's synchronous event loop.  Initialised once at first DB access.
fn rt() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("failed to create tokio runtime for libSQL")
    })
}

/// A single search result — model identity plus similarity score.
#[derive(Clone)]
pub struct SearchResult {
    pub file_path: String,
    pub name: String,
    pub similarity: f32,
}

/// A model record stored in the local history database.
pub struct ModelRecord {
    pub file_path: String,
    pub name: String,
    #[allow(dead_code)]
    pub model_version: String, // "V2" or "V3"
    pub zoom_scale: Option<f32>,
    pub layout_pan_x: Option<f32>,
    pub layout_pan_y: Option<f32>,
    #[allow(dead_code)]
    pub last_opened: String, // ISO datetime string from SQLite
}

/// Local libSQL database for the viewer.
///
/// Stores global key–value settings and a model history table.
/// All operations are synchronous on the caller's thread; async libsql API
/// is bridged via a global tokio runtime.
pub struct AppDb {
    conn: libsql::Connection,
}

impl AppDb {
    /// Open (or create) the database at `path`, enable WAL mode, and
    /// ensure the schema tables exist.
    pub fn open(path: &Path) -> Result<Self> {
        let path_str = path.to_str().context("non-UTF-8 db path")?;
        let db = rt().block_on(libsql::Builder::new_local(path_str).build())?;
        let conn = db.connect()?;

        rt().block_on(conn.execute_batch("PRAGMA journal_mode=WAL"))?;
        // Migration: add layout columns to existing model_history (no-op if already present)
        let _ = rt()
            .block_on(conn.execute("ALTER TABLE model_history ADD COLUMN layout_pan_x REAL", ()));
        let _ = rt()
            .block_on(conn.execute("ALTER TABLE model_history ADD COLUMN layout_pan_y REAL", ()));
        rt().block_on(conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS global_settings (
                key    TEXT PRIMARY KEY,
                value  TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS model_history (
                file_path    TEXT PRIMARY KEY,
                name         TEXT NOT NULL,
                model_version TEXT NOT NULL,
                zoom_scale   REAL,
                layout_pan_x REAL,
                layout_pan_y REAL,
                last_opened  TEXT NOT NULL DEFAULT (datetime('now')),
                created_at   TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS model_embeddings (
                file_path    TEXT PRIMARY KEY REFERENCES model_history(file_path) ON DELETE CASCADE,
                embedding    BLOB NOT NULL,
                updated_at   TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS parameter_presets (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path    TEXT NOT NULL REFERENCES model_history(file_path) ON DELETE CASCADE,
                name         TEXT NOT NULL,
                parameters   BLOB NOT NULL,
                created_at   TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(file_path, name)
            );

            CREATE TABLE IF NOT EXISTS character_cards (
                file_path       TEXT PRIMARY KEY REFERENCES model_history(file_path) ON DELETE CASCADE,
                name            TEXT NOT NULL DEFAULT '',
                description     TEXT NOT NULL DEFAULT '',
                personality     TEXT NOT NULL DEFAULT '',
                scenario        TEXT NOT NULL DEFAULT '',
                example_dialogs TEXT NOT NULL DEFAULT '',
                system_prompt   TEXT NOT NULL DEFAULT '',
                tts_voice       TEXT NOT NULL DEFAULT '',
                updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS conversation_memory (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path   TEXT NOT NULL REFERENCES model_history(file_path) ON DELETE CASCADE,
                content     TEXT NOT NULL,
                embedding   BLOB,
                entry_type  TEXT NOT NULL DEFAULT 'message',
                created_at  TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS tool_execution_log (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path   TEXT NOT NULL REFERENCES model_history(file_path) ON DELETE CASCADE,
                tool_name   TEXT NOT NULL,
                arguments   TEXT NOT NULL DEFAULT '',
                result      TEXT NOT NULL DEFAULT '',
                approved    INTEGER NOT NULL DEFAULT 1,
                duration_ms INTEGER NOT NULL DEFAULT 0,
                created_at  TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        ))?;

        Ok(Self { conn })
    }

    pub fn get_setting(&self, key: &str) -> Option<String> {
        let mut rows = rt()
            .block_on(self.conn.query(
                "SELECT value FROM global_settings WHERE key = ?1",
                libsql::params![key],
            ))
            .ok()?;
        let row = rt().block_on(rows.next()).ok()??;
        row.get::<String>(0).ok()
    }

    /// Insert or replace a global setting.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        rt().block_on(self.conn.execute(
            "INSERT INTO global_settings (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = ?2",
            libsql::params![key, value],
        ))?;
        Ok(())
    }

    /// Insert or update a model history record.
    ///
    /// When a record with the same `file_path` already exists its
    /// `name`, `model_version`, `zoom_scale` and `last_opened` fields
    /// are updated and the row is *not* duplicated.
    pub fn add_or_update_model(
        &self,
        file_path: &str,
        name: &str,
        model_version: &str,
        zoom_scale: Option<f32>,
    ) -> Result<()> {
        // libSQL ToValue supports Option<T> → None maps to SQL NULL
        let zoom: Option<f64> = zoom_scale.map(|z| z as f64);
        rt().block_on(self.conn.execute(
            "INSERT INTO model_history (file_path, name, model_version, zoom_scale) \
             VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT(file_path) DO UPDATE SET \
                name=?2, model_version=?3, zoom_scale=?4, last_opened=datetime('now')",
            libsql::params![file_path, name, model_version, zoom],
        ))?;
        Ok(())
    }

    /// Return every model history record ordered by `last_opened`
    /// descending (most recent first).
    pub fn model_history(&self) -> Result<Vec<ModelRecord>> {
        rt().block_on(self.model_history_impl())
    }

    async fn model_history_impl(&self) -> Result<Vec<ModelRecord>> {
        let mut rows = self
            .conn
            .query(
                "SELECT file_path, name, model_version, zoom_scale, layout_pan_x, layout_pan_y, last_opened \
                 FROM model_history ORDER BY last_opened DESC",
                (),
            )
            .await?;

        let mut records = Vec::new();
        while let Some(row) = rows.next().await? {
            records.push(ModelRecord {
                file_path: row.get::<String>(0)?,
                name: row.get::<String>(1)?,
                model_version: row.get::<String>(2)?,
                zoom_scale: row.get::<Option<f64>>(3)?.map(|z| z as f32),
                layout_pan_x: row.get::<Option<f64>>(4)?.map(|x| x as f32),
                layout_pan_y: row.get::<Option<f64>>(5)?.map(|y| y as f32),
                last_opened: row.get::<String>(6)?,
            });
        }
        Ok(records)
    }

    /// Retrieve a single model record by its file path.
    pub fn get_model(&self, file_path: &str) -> Result<Option<ModelRecord>> {
        let mut rows = rt().block_on(self.conn.query(
            "SELECT file_path, name, model_version, zoom_scale, layout_pan_x, layout_pan_y, last_opened \
             FROM model_history WHERE file_path = ?1",
            libsql::params![file_path],
        ))?;

        match rt().block_on(rows.next())? {
            Some(row) => Ok(Some(ModelRecord {
                file_path: row.get::<String>(0)?,
                name: row.get::<String>(1)?,
                model_version: row.get::<String>(2)?,
                zoom_scale: row.get::<Option<f64>>(3)?.map(|z| z as f32),
                layout_pan_x: row.get::<Option<f64>>(4)?.map(|x| x as f32),
                layout_pan_y: row.get::<Option<f64>>(5)?.map(|y| y as f32),
                last_opened: row.get::<String>(6)?,
            })),
            None => Ok(None),
        }
    }

    /// Save full layout (pan + zoom) for a model.
    pub fn set_model_layout(
        &self,
        file_path: &str,
        pan_x: Option<f32>,
        pan_y: Option<f32>,
        zoom: Option<f32>,
    ) -> Result<()> {
        let px: Option<f64> = pan_x.map(|x| x as f64);
        let py: Option<f64> = pan_y.map(|y| y as f64);
        let z: Option<f64> = zoom.map(|z| z as f64);
        rt().block_on(self.conn.execute(
            "UPDATE model_history SET layout_pan_x=?1, layout_pan_y=?2, zoom_scale=?3 \
             WHERE file_path=?4",
            libsql::params![px, py, z, file_path],
        ))?;
        Ok(())
    }

    // ------------------------------------------------------------------
    //  Parameter presets
    // ------------------------------------------------------------------

    /// Save a named parameter preset for a model. Overwrites if name exists.
    pub fn save_preset(&self, file_path: &str, name: &str, params: &[u8]) -> Result<()> {
        rt().block_on(self.conn.execute(
            "INSERT INTO parameter_presets (file_path, name, parameters) VALUES (?1, ?2, ?3) \
             ON CONFLICT(file_path, name) DO UPDATE SET parameters=?3, created_at=datetime('now')",
            libsql::params![file_path, name, params.to_vec()],
        ))?;
        Ok(())
    }

    /// Load a named preset's parameter data for a model.
    pub fn load_preset(&self, file_path: &str, name: &str) -> Result<Option<Vec<u8>>> {
        let mut rows = rt().block_on(self.conn.query(
            "SELECT parameters FROM parameter_presets WHERE file_path = ?1 AND name = ?2",
            libsql::params![file_path, name],
        ))?;
        match rt().block_on(rows.next())? {
            Some(row) => Ok(Some(row.get::<Vec<u8>>(0)?)),
            None => Ok(None),
        }
    }

    /// List all preset names for a model, ordered by creation time.
    pub fn list_presets(&self, file_path: &str) -> Result<Vec<String>> {
        let mut rows = rt().block_on(self.conn.query(
            "SELECT name FROM parameter_presets WHERE file_path = ?1 ORDER BY created_at ASC",
            libsql::params![file_path],
        ))?;
        let mut names = Vec::new();
        while let Some(row) = rt().block_on(rows.next())? {
            names.push(row.get::<String>(0)?);
        }
        Ok(names)
    }

    /// Delete a named preset for a model.
    pub fn delete_preset(&self, file_path: &str, name: &str) -> Result<()> {
        rt().block_on(self.conn.execute(
            "DELETE FROM parameter_presets WHERE file_path = ?1 AND name = ?2",
            libsql::params![file_path, name],
        ))?;
        Ok(())
    }

    /// Remove a model from the history (e.g. when its directory no longer
    /// exists on disk).
    pub fn remove_model(&self, file_path: &str) -> Result<()> {
        rt().block_on(self.conn.execute(
            "DELETE FROM model_history WHERE file_path = ?1",
            libsql::params![file_path],
        ))?;
        Ok(())
    }

    /// Rename a model in the history (user-friendly display name).
    pub fn rename_model(&self, file_path: &str, new_name: &str) -> Result<()> {
        rt().block_on(self.conn.execute(
            "UPDATE model_history SET name = ?1 WHERE file_path = ?2",
            libsql::params![new_name, file_path],
        ))?;
        Ok(())
    }

    /// Given the current model's `file_path`, return the previous and next
    /// model paths in stable `file_path ASC` order (wrapping around).
    /// Returns `(prev, next)` or `None` if only one record exists.
    pub fn prev_next_paths(&self, current: &str) -> Result<Option<(String, String)>> {
        let mut rows = rt().block_on(self.conn.query(
            "SELECT file_path FROM model_history ORDER BY file_path ASC",
            (),
        ))?;
        let mut paths = Vec::new();
        while let Some(row) = rt().block_on(rows.next())? {
            paths.push(row.get::<String>(0)?);
        }
        if paths.len() < 2 {
            return Ok(None);
        }
        let idx = paths.iter().position(|p| p == current);
        let idx = match idx {
            Some(i) => i,
            None => return Ok(None),
        };
        let prev = if idx == 0 { paths.len() - 1 } else { idx - 1 };
        let next = (idx + 1) % paths.len();
        Ok(Some((paths[prev].clone(), paths[next].clone())))
    }

    // ------------------------------------------------------------------
    //  Model embedding helpers (word-vector search)
    // ------------------------------------------------------------------

    async fn set_model_embedding_impl(&self, file_path: &str, bytes: Vec<u8>) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO model_embeddings (file_path, embedding) VALUES (?1, ?2) \
                 ON CONFLICT(file_path) DO UPDATE SET \
                    embedding=?2, updated_at=datetime('now')",
                libsql::params![file_path, bytes],
            )
            .await?;
        Ok(())
    }

    /// Batch-load all cached embeddings in one query — avoids N+1 SELECTs
    /// when scoring the whole history during search.
    async fn load_all_embeddings_impl(&self) -> Result<HashMap<String, Vec<f32>>> {
        let mut rows = self
            .conn
            .query("SELECT file_path, embedding FROM model_embeddings", ())
            .await?;
        let mut map = HashMap::new();
        while let Some(row) = rows.next().await? {
            let path: String = row.get(0)?;
            let bytes: Vec<u8> = row.get(1)?;
            if bytes.len() == EMBED_DIM * 4 {
                let embedding = bytes
                    .chunks_exact(4)
                    .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                    .collect();
                map.insert(path, embedding);
            }
        }
        Ok(map)
    }

    /// Search model history by semantic similarity against `query`
    /// (character n-gram embeddings), returning top-`limit` results sorted
    /// descending by cosine similarity.
    ///
    /// Driven on a caller-supplied runtime: background threads must pass
    /// their own runtime because the global `rt()` is current-thread and
    /// panics if two threads `block_on` it concurrently.
    pub fn search_models_on(
        &self,
        runtime: &Runtime,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        runtime.block_on(self.search_models_impl(query, limit))
    }

    async fn search_models_impl(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let records = self.model_history_impl().await?;
        if records.is_empty() {
            return Ok(Vec::new());
        }

        let cached = self.load_all_embeddings_impl().await?;
        let q_embed = embed_text(query);
        let mut scored: Vec<SearchResult> = Vec::with_capacity(records.len());
        let mut uncached: Vec<(String, Vec<f32>)> = Vec::new();

        for rec in &records {
            let embed = match cached.get(&rec.file_path) {
                Some(e) => e.clone(),
                None => {
                    let e = embed_text(&rec.name);
                    uncached.push((rec.file_path.clone(), e.clone()));
                    e
                }
            };
            let sim = cosine_sim(&q_embed, &embed);
            scored.push(SearchResult {
                file_path: rec.file_path.clone(),
                name: rec.name.clone(),
                similarity: sim,
            });
        }

        // Sort descending by similarity, take top-k
        scored.sort_by(|a, b| {
            b.similarity
                .partial_cmp(&a.similarity)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(limit);

        // Persist newly computed embeddings best-effort so later searches hit
        // the cache; failures never affect the result.
        for (path, embed) in &uncached {
            let bytes: Vec<u8> = embed.iter().flat_map(|f| f.to_le_bytes()).collect();
            if let Err(e) = self.set_model_embedding_impl(path, bytes).await {
                log::warn!("embedding cache store failed for {path}: {e}");
            }
        }

        Ok(scored)
    }

    // ------------------------------------------------------------------
    //  Character cards (AI companion profile per model)
    // ------------------------------------------------------------------

    /// Retrieve the character card for a model, if one exists.
    pub fn get_character_card(&self, file_path: &str) -> Result<Option<CharacterCard>> {
        let mut rows = rt().block_on(self.conn.query(
            "SELECT file_path, name, description, personality, scenario, \
             example_dialogs, system_prompt, tts_voice \
             FROM character_cards WHERE file_path = ?1",
            libsql::params![file_path],
        ))?;
        match rt().block_on(rows.next())? {
            Some(row) => Ok(Some(CharacterCard {
                file_path: row.get::<String>(0)?,
                name: row.get::<String>(1)?,
                description: row.get::<String>(2)?,
                personality: row.get::<String>(3)?,
                scenario: row.get::<String>(4)?,
                example_dialogs: row.get::<String>(5)?,
                system_prompt: row.get::<String>(6)?,
                tts_voice: row.get::<String>(7)?,
            })),
            None => Ok(None),
        }
    }

    /// Insert or update a character card for a model.
    pub fn save_character_card(&self, card: &CharacterCard) -> Result<()> {
        rt().block_on(self.conn.execute(
            "INSERT INTO character_cards \
             (file_path, name, description, personality, scenario, \
              example_dialogs, system_prompt, tts_voice) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
             ON CONFLICT(file_path) DO UPDATE SET \
             name=?2, description=?3, personality=?4, scenario=?5, \
             example_dialogs=?6, system_prompt=?7, tts_voice=?8, \
             updated_at=datetime('now')",
            libsql::params![
                card.file_path.clone(),
                card.name.clone(),
                card.description.clone(),
                card.personality.clone(),
                card.scenario.clone(),
                card.example_dialogs.clone(),
                card.system_prompt.clone(),
                card.tts_voice.clone(),
            ],
        ))?;
        Ok(())
    }

    /// Delete a character card for a model.
    pub fn delete_character_card(&self, file_path: &str) -> Result<()> {
        rt().block_on(self.conn.execute(
            "DELETE FROM character_cards WHERE file_path = ?1",
            libsql::params![file_path],
        ))?;
        Ok(())
    }

    // ------------------------------------------------------------------
    //  Conversation memory (vector-searchable history)
    // ------------------------------------------------------------------

    /// Store a message as a vector-searchable memory entry.
    ///
    /// The n-gram embedding is computed automatically from `content`.
    /// Returns the row ID of the inserted entry.
    pub fn save_memory(&self, file_path: &str, content: &str, entry_type: &str) -> Result<i64> {
        let embedding = embed_text(content);
        let bytes: Vec<u8> = embedding.iter().flat_map(|f| f.to_le_bytes()).collect();
        rt().block_on(self.conn.execute(
            "INSERT INTO conversation_memory (file_path, content, embedding, entry_type) \
             VALUES (?1, ?2, ?3, ?4)",
            libsql::params![file_path, content, bytes, entry_type],
        ))?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Search conversation memory by semantic similarity to `query`.
    ///
    /// Returns up to `limit` entries sorted by descending cosine similarity.
    pub fn search_memories(
        &self,
        file_path: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        let q_embed = embed_text(query);
        let mut rows = rt().block_on(self.conn.query(
            "SELECT id, file_path, content, entry_type, created_at, embedding \
             FROM conversation_memory WHERE file_path = ?1",
            libsql::params![file_path],
        ))?;

        let mut scored: Vec<(f32, MemoryEntry)> = Vec::new();
        while let Some(row) = rt().block_on(rows.next())? {
            let id = row.get::<i64>(0)?;
            let fp = row.get::<String>(1)?;
            let content = row.get::<String>(2)?;
            let entry_type = row.get::<String>(3)?;
            let created_at = row.get::<String>(4)?;
            let bytes: Vec<u8> = row.get::<Vec<u8>>(5)?;
            let embed: Vec<f32> = bytes
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            let sim = cosine_sim(&q_embed, &embed);
            scored.push((
                sim,
                MemoryEntry {
                    id,
                    file_path: fp,
                    content,
                    entry_type,
                    created_at,
                },
            ));
        }

        // Sort descending by similarity, take top-k
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        Ok(scored.into_iter().map(|(_, entry)| entry).collect())
    }

    /// Log a tool execution to the audit trail.
    pub fn save_tool_execution(
        &self,
        file_path: &str,
        tool_name: &str,
        arguments: &str,
        result: &str,
        approved: bool,
        duration_ms: u64,
    ) -> Result<i64> {
        rt().block_on(self.conn.execute(
            "INSERT INTO tool_execution_log (file_path, tool_name, arguments, result, approved, duration_ms) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            libsql::params![file_path, tool_name, arguments, result, approved as i32, duration_ms as i64],
        ))?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Get configured model scan directories (JSON array of paths).
    pub fn get_scan_dirs(&self) -> Vec<String> {
        self.get_setting("scan_dirs")
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default()
    }

    /// Save model scan directories.
    pub fn set_scan_dirs(&self, dirs: &[String]) -> Result<()> {
        let json = serde_json::to_string(dirs)?;
        self.set_setting("scan_dirs", &json)
    }
}

/// A background model-search request (UI thread → search worker).
pub struct SearchJob {
    pub generation: u64,
    pub query: String,
    pub limit: usize,
}

/// A finished background search (search worker → UI thread).
pub struct SearchOutcome {
    pub generation: u64,
    pub result: std::result::Result<Vec<SearchResult>, String>,
}

/// Spawn the dedicated model-search worker thread.
///
/// The worker drives its OWN current-thread tokio runtime: the global `rt()`
/// is current-thread too, and two threads calling `block_on` on it
/// concurrently is a panic. The thread exits when both channels close.
pub fn spawn_search_worker(
    db: std::sync::Arc<AppDb>,
    jobs: std::sync::mpsc::Receiver<SearchJob>,
    results: std::sync::mpsc::Sender<SearchOutcome>,
) {
    let spawned = std::thread::Builder::new()
        .name("model-search".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    log::error!("model-search worker runtime init failed: {e}");
                    return;
                }
            };
            while let Ok(job) = jobs.recv() {
                let result = db
                    .search_models_on(&runtime, &job.query, job.limit)
                    .map_err(|e| e.to_string());
                if results
                    .send(SearchOutcome {
                        generation: job.generation,
                        result,
                    })
                    .is_err()
                {
                    break; // receiver dropped — UI thread gone
                }
            }
        });
    match spawned {
        Ok(handle) => drop(handle), // detach; exits when channels close
        Err(e) => log::error!("failed to spawn model-search worker: {e}"),
    }
}
