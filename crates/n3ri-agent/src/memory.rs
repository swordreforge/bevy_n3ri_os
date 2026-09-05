//! 温冷记忆（M3）：文件存储 + BM25 召回 + 后台维护任务。
//!
//! 布局 `~/.config/n3ri_os/agent/`（与 `llm-config.json` 同父目录）：
//! `config.json / scheduler.json / profile.json / facts.json / facts_archive.json /`
//! `reflections.json / reflection_archive/ / persona.json / episodes-YYYY-MM-DD.jsonl / cursors.json`
//! 全部 JSON 原子写（tmp + rename）；读失败视为空，不阻塞聊天。

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;
use std::thread;

use n3ri_llm::{LlmClient, Message};

use crate::config::AgentConfig;

pub const HOT_CAP: usize = 20;
pub const HOT_TAIL: usize = 10;
pub const HOT_HARD_CAP_CHARS: usize = 60000;
pub const EXTRACT_EVERY_TURNS: u64 = 10;
pub const SYNTH_MIN_UNABSORBED: usize = 5;
pub const SYNTH_FACTS_MAX: usize = 20;
pub const PERSONA_BUDGET_CHARS: usize = 2000;
pub const REFLECTION_BUDGET_CHARS: usize = 2000;
pub const RECALL_ENTRY_MAX_CHARS: usize = 400;
pub const RECALL_TOTAL: usize = 8;
pub const RECALL_BM25_THRESHOLD: f32 = 0.1;
pub const ARCHIVE_FACT_DAYS: i64 = 7;
pub const ARCHIVE_REFLECTION_DAYS: i64 = 30;
pub const REFLECTION_ARCHIVE_SHARD_MAX: usize = 500;
pub const EXTRACT_MAX_TURNS: usize = 12;

pub fn agent_dir() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("n3ri_os")
        .join("agent")
}

fn read_json<T: Default + serde::de::DeserializeOwned>(name: &str) -> T {
    match std::fs::read_to_string(agent_dir().join(name)) {
        Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
        Err(_) => T::default(),
    }
}

fn write_json(name: &str, value: &impl Serialize) {
    let dir = agent_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(name);
    if let Ok(json) = serde_json::to_string_pretty(value) {
        let tmp = dir.join(format!("{name}.tmp"));
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

// ============================
//  Schema（字段名与 N.E.K.O 对齐，砍 embedding/scopes/trust）
// ============================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
    pub id: String,
    pub text: String,
    pub importance: u8,
    #[serde(default = "default_kind")]
    pub kind: String,
    pub created_at: String,
    #[serde(default)]
    pub absorbed: bool,
    pub hash: String,
}

fn default_kind() -> String {
    "other".to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReflStatus {
    #[default]
    Pending,
    Confirmed,
    Promoted,
    Denied,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reflection {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub status: ReflStatus,
    #[serde(default)]
    pub source_fact_ids: Vec<String>,
    #[serde(default)]
    pub reinforcement: f32,
    #[serde(default)]
    pub disputation: f32,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonaEntry {
    pub id: String,
    pub text: String,
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default)]
    pub protected: bool,
    pub created_at: String,
}

fn default_source() -> String {
    "manual".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Persona {
    #[serde(default)]
    pub user: Vec<PersonaEntry>,
    #[serde(default)]
    pub nori: Vec<PersonaEntry>,
    #[serde(default)]
    pub relationship: Vec<PersonaEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    pub seq: u64,
    pub ts: String,
    pub user: String,
    pub assistant: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Cursors {
    #[serde(default)]
    pub turn_seq: u64,
    #[serde(default)]
    pub last_synth_ts: i64,
}

pub fn normalize_hash(text: &str) -> String {
    let norm: String = text
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, '，' | '。' | '！' | '？' | ',' | '.' | '!' | '?' | '、' | '；' | ';' | '：' | ':'))
        .collect();
    let digest = sha256_hex(norm.as_bytes());
    digest[..16].to_string()
}

fn sha256_hex(data: &[u8]) -> String {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    let mut out = String::with_capacity(64);
    for v in h {
        out.push_str(&format!("{v:08x}"));
    }
    out
}

pub fn fact_id() -> String {
    let now = chrono::Local::now();
    let rand: u32 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0)
        % 0xffffff;
    format!(
        "fact_{}{:06x}",
        now.format("%Y%m%d%H%M%S"),
        rand & 0xffffff
    )
}

pub fn reflection_id(source_ids: &[String]) -> String {
    let mut ids = source_ids.to_vec();
    ids.sort();
    let digest = sha256_hex(ids.join(",").as_bytes());
    format!("ref_{}", &digest[..16])
}

pub fn now_iso() -> String {
    chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

// ============================
//  热记忆（内存，memo + tail）
// ============================

/// 热记忆：最近原文 + 头部摘要（抄 N.E.K.O `recent.json` memo 思路）。
#[derive(Resource, Debug, Clone, Default)]
pub struct HotMemory {
    pub memo: String,
    pub tail: VecDeque<HotTurn>,
    pub(crate) dirty: bool,
}

#[derive(Debug, Clone)]
pub struct HotTurn {
    pub user: String,
    pub assistant: String,
}

impl HotMemory {
    pub fn push(&mut self, user: String, assistant: String) {
        self.tail.push_back(HotTurn { user, assistant });
        while self.tail.len() > HOT_CAP {
            self.tail.pop_front();
        }
        self.dirty = true;
    }

    pub fn needs_summary(&self) -> bool {
        self.tail.len() > HOT_CAP - 2 && self.memo.is_empty()
            || self.tail.len() >= HOT_CAP
    }

    pub fn total_chars(&self) -> usize {
        self.memo.chars().count()
            + self.tail.iter().map(|t| t.user.chars().count() + t.assistant.chars().count()).sum::<usize>()
    }
}

// ============================
//  存储（文件读写，线程内调用）
// ============================

#[derive(Debug, Default)]
pub struct MemoryStore {
    pub facts: Vec<Fact>,
    pub archive: Vec<Fact>,
    pub reflections: Vec<Reflection>,
    pub persona: Persona,
}

impl MemoryStore {
    pub fn load() -> Self {
        Self {
            facts: read_json("facts.json"),
            archive: read_json("facts_archive.json"),
            reflections: read_json("reflections.json"),
            persona: read_json("persona.json"),
        }
    }

    pub fn save_facts(&self) {
        write_json("facts.json", &self.facts);
    }

    pub fn save_archive(&self) {
        write_json("facts_archive.json", &self.archive);
    }

    pub fn save_reflections(&self) {
        write_json("reflections.json", &self.reflections);
    }

    pub fn save_persona(&self) {
        write_json("persona.json", &self.persona);
    }

    pub fn unabsorbed(&self) -> Vec<&Fact> {
        let mut v: Vec<&Fact> = self.facts.iter().filter(|f| !f.absorbed && f.importance >= 5).collect();
        v.sort_by(|a, b| b.importance.cmp(&a.importance).then(a.created_at.cmp(&b.created_at)));
        v.truncate(SYNTH_FACTS_MAX);
        v
    }

    pub fn insert_facts(&mut self, mut new: Vec<Fact>) {
        for f in new.drain(..) {
            if self.facts.iter().any(|e| e.hash == f.hash)
                || self.archive.iter().any(|e| e.hash == f.hash)
            {
                continue;
            }
            if bm25_overlap(&self.facts, &f.text) {
                continue;
            }
            self.facts.push(f);
        }
    }
}

pub fn append_episode(ep: &Episode) {
    let dir = agent_dir();
    let _ = std::fs::create_dir_all(&dir);
    let name = format!("episodes-{}.jsonl", chrono::Local::now().format("%Y-%m-%d"));
    let path = dir.join(name);
    if let Ok(line) = serde_json::to_string(ep) {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            let _ = writeln!(f, "{line}");
        }
    }
}

pub fn load_cursors() -> Cursors {
    read_json("cursors.json")
}

pub fn save_cursors(c: &Cursors) {
    write_json("cursors.json", c);
}

/// 一键清除本地记忆文件（保留 config.json / scheduler.json）。
pub fn clear_memory_files() {
    let dir = agent_dir();
    for name in [
        "facts.json",
        "facts_archive.json",
        "reflections.json",
        "persona.json",
        "cursors.json",
    ] {
        let _ = std::fs::remove_file(dir.join(name));
    }
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("jsonl")
                && p
                    .file_name()
                    .and_then(|s| s.to_str())
                    .is_some_and(|n| n.starts_with("episodes-"))
            {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
    let _ = std::fs::remove_dir_all(dir.join("reflection_archive"));
}

// ============================
//  BM25（手写 Okapi，CJK 2-gram；阈值 0.1 保小池 exact match）
// ============================

pub fn tokenize(text: &str) -> Vec<String> {
    let mut toks: Vec<String> = Vec::new();
    let mut latin = String::new();
    let mut cjk: Vec<char> = Vec::new();
    let flush = |latin: &mut String, toks: &mut Vec<String>| {
        if !latin.is_empty() {
            toks.push(std::mem::take(latin).to_lowercase());
        }
    };
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            if !cjk.is_empty() {
                let grams = cjk_grams(&cjk);
                toks.extend(grams);
                cjk.clear();
            }
            latin.push(ch);
        } else if is_cjk(ch) {
            flush(&mut latin, &mut toks);
            cjk.push(ch);
        } else {
            flush(&mut latin, &mut toks);
            if !cjk.is_empty() {
                let grams = cjk_grams(&cjk);
                toks.extend(grams);
                cjk.clear();
            }
        }
    }
    flush(&mut latin, &mut toks);
    if !cjk.is_empty() {
        toks.extend(cjk_grams(&cjk));
    }
    toks
}

fn is_cjk(ch: char) -> bool {
    matches!(ch,
        '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{3040}'..='\u{30FF}' | '\u{AC00}'..='\u{D7AF}')
}

fn cjk_grams(chars: &[char]) -> Vec<String> {
    if chars.len() == 1 {
        return vec![chars.iter().collect()];
    }
    chars.windows(2).map(|w| w.iter().collect()).collect()
}

pub fn bm25_score(query: &[String], doc: &[String], avg_len: f32, doc_freq: &HashMap<String, usize>, n_docs: usize) -> f32 {
    const K1: f32 = 1.2;
    const B: f32 = 0.75;
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for t in doc {
        *counts.entry(t.as_str()).or_default() += 1;
    }
    let dl = doc.len() as f32;
    let mut score = 0.0;
    for q in query {
        let tf = *counts.get(q.as_str()).unwrap_or(&0) as f32;
        if tf == 0.0 {
            continue;
        }
        let df = *doc_freq.get(q).unwrap_or(&0) as f32;
        let idf = (((n_docs as f32 - df + 0.5) / (df + 0.5)) + 1.0).ln();
        score += idf * (tf * (K1 + 1.0)) / (tf + K1 * (1.0 - B + B * dl / avg_len.max(1.0)));
    }
    score
}

pub struct RecallHit {
    pub tag: &'static str,
    pub text: String,
    pub date: String,
    pub score: f32,
}

/// BM25 召回（persona 不入池，archive 可搜；阈值 0.1；top4/路，最多 8 条）。
pub fn recall(store: &MemoryStore, query: &str) -> Vec<RecallHit> {
    let qtoks = tokenize(query);
    if qtoks.is_empty() {
        return Vec::new();
    }
    let mut docs: Vec<(&'static str, &str, &str)> = Vec::new();
    for f in &store.facts {
        docs.push(("fact", f.text.as_str(), f.created_at.as_str()));
    }
    for r in &store.reflections {
        if matches!(r.status, ReflStatus::Denied | ReflStatus::Archived) {
            continue;
        }
        docs.push(("reflection", r.text.as_str(), r.created_at.as_str()));
    }
    for f in &store.archive {
        docs.push(("archive", f.text.as_str(), f.created_at.as_str()));
    }
    if docs.is_empty() {
        return Vec::new();
    }
    let tokenized: Vec<Vec<String>> = docs.iter().map(|(_, t, _)| tokenize(t)).collect();
    let avg_len = tokenized.iter().map(|d| d.len()).sum::<usize>() as f32 / tokenized.len() as f32;
    let mut df: HashMap<String, usize> = HashMap::new();
    for d in &tokenized {
        let mut seen: Vec<&str> = Vec::new();
        for t in d {
            if !seen.contains(&t.as_str()) {
                seen.push(t.as_str());
                *df.entry(t.clone()).or_default() += 1;
            }
        }
    }
    let n = docs.len();
    let mut scored: Vec<(usize, f32)> = tokenized
        .iter()
        .enumerate()
        .map(|(i, d)| (i, bm25_score(&qtoks, d, avg_len, &df, n)))
        .filter(|(_, s)| *s >= RECALL_BM25_THRESHOLD)
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(RECALL_TOTAL.min(4));
    scored
        .into_iter()
        .map(|(i, s)| {
            let (tag, text, date) = docs[i];
            let mut t: String = text.chars().take(RECALL_ENTRY_MAX_CHARS).collect();
            if text.chars().count() > RECALL_ENTRY_MAX_CHARS {
                t.push('…');
            }
            RecallHit {
                tag,
                text: t,
                date: date.get(..10).unwrap_or(date).to_string(),
                score: s,
            }
        })
        .collect()
}

pub fn render_hits(hits: &[RecallHit]) -> String {
    hits.iter()
        .enumerate()
        .map(|(i, h)| format!("{}. [{} {}] {}", i + 1, h.tag, h.date, h.text))
        .collect::<Vec<_>>()
        .join("\n")
}

fn bm25_overlap(existing: &[Fact], text: &str) -> bool {
    let q = tokenize(text);
    if q.is_empty() {
        return false;
    }
    let qs: std::collections::HashSet<String> = q.into_iter().collect();
    for f in existing {
        let dtoks = tokenize(&f.text);
        let d: std::collections::HashSet<&str> = dtoks.iter().map(|s| s.as_str()).collect();
        let qs_ref: std::collections::HashSet<&str> = qs.iter().map(|s| s.as_str()).collect();
        let inter = qs_ref.intersection(&d).count() as f32;
        let uni = qs_ref.union(&d).count() as f32;
        if uni > 0.0 && inter / uni >= 0.8 {
            return true;
        }
    }
    false
}

/// `recall_memory` handler（同步，永不抛错；失败返回空 hits）。
pub fn handle_recall_memory(store: &MemoryStore, args: &serde_json::Value) -> n3ri_llm::ToolEnvelope {
    let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("").trim();
    if query.is_empty() {
        return n3ri_llm::ToolEnvelope::ok(serde_json::json!({ "hits": [], "note": "query 为空" }));
    }
    let hits = recall(store, query);
    n3ri_llm::ToolEnvelope::ok(serde_json::json!({
        "hits": hits.iter().map(|h| serde_json::json!({
            "tag": h.tag, "text": h.text, "date": h.date,
        })).collect::<Vec<_>>(),
    }))
}

// ============================
//  记忆段渲染（自动段：persona + reflection + memo/tail；persona 不进 recall 池但进自动段）
// ============================

pub fn build_memory_block(store: &MemoryStore, hot: &HotMemory) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut used = 0usize;

    let mut persona_lines: Vec<String> = Vec::new();
    for e in store.persona.user.iter().chain(store.persona.nori.iter()).chain(store.persona.relationship.iter()) {
        if e.protected {
            persona_lines.push(format!("- {}", e.text));
        }
    }
    for e in store.persona.user.iter().chain(store.persona.nori.iter()).chain(store.persona.relationship.iter()).filter(|e| !e.protected) {
        let len = e.text.chars().count();
        if used + len > PERSONA_BUDGET_CHARS {
            break;
        }
        used += len;
        persona_lines.push(format!("- {}", e.text));
    }
    if !persona_lines.is_empty() {
        lines.push("【 persona 】".to_string());
        lines.extend(persona_lines);
    }

    let mut refls: Vec<&Reflection> = store
        .reflections
        .iter()
        .filter(|r| matches!(r.status, ReflStatus::Pending | ReflStatus::Confirmed))
        .collect();
    refls.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let mut used_r = 0usize;
    let mut rlines: Vec<String> = Vec::new();
    for r in refls.iter().take(3) {
        let len = r.text.chars().count();
        if used_r + len > REFLECTION_BUDGET_CHARS {
            break;
        }
        used_r += len;
        rlines.push(format!("- {}", r.text));
    }
    if !rlines.is_empty() {
        lines.push("【近况】".to_string());
        lines.extend(rlines);
    }

    if !hot.memo.is_empty() {
        lines.push("【 earlier 】".to_string());
        lines.push(hot.memo.clone());
    }

    if lines.is_empty() {
        return String::new();
    }
    format!("<memory>\n{}\n</memory>", lines.join("\n"))
}

// ============================
//  后台维护（线程内跑 LLM；失败降级，永不阻塞聊天）
// ============================

#[derive(Resource, Default)]
pub struct MemoryMaint {
    rx: Option<Mutex<Receiver<MaintOutput>>>,
    pub pending: bool,
}

#[derive(Debug)]
struct MaintOutput {
    job: &'static str,
    memo: Option<String>,
    dropped: usize,
    facts: Vec<Fact>,
    reflection: Option<(Reflection, Vec<String>)>,
}

fn summarize_prompt(turns: &[(String, String)]) -> Vec<Message> {
    let body = turns
        .iter()
        .map(|(u, a)| format!("用户：{u}\nNori：{a}"))
        .collect::<Vec<_>>()
        .join("\n");
    vec![
        Message::system("你是 Nori 的记忆压缩器。把下面的对话压缩成一段 200 字以内的中文备忘，只记事实（用户喜好/习惯/重要事件），不要记寒暄。直接输出备忘正文。".to_string()),
        Message::user(body),
    ]
}

fn extract_prompt(turns: &[(String, String)], known: &[String]) -> Vec<Message> {
    let body = turns
        .iter()
        .map(|(u, a)| format!("用户：{u}\nNori：{a}"))
        .collect::<Vec<_>>()
        .join("\n");
    let known_block = if known.is_empty() {
        String::new()
    } else {
        format!("\n已知（不要重复抽）：\n{}", known.join("\n"))
    };
    vec![
        Message::system(format!("你是 Nori 的事实抽取器。从对话中抽取原子事实（用户偏好/事件/特质），每条 importance 1..10（5+ 才重要），kind 只能是 preference/event/trait/other。只输出 JSON 数组，如 [{{\"text\":\"…\",\"importance\":7,\"kind\":\"preference\"}}]，无事实输出 []。{known_block}")),
        Message::user(body),
    ]
}

fn synth_prompt(facts: &[Fact]) -> Vec<Message> {
    let body = facts
        .iter()
        .map(|f| format!("- [{}] {}", f.id, f.text))
        .collect::<Vec<_>>()
        .join("\n");
    vec![
        Message::system("你是 Nori 的反思合成器。把下面的事实综合成 1 条 100 字以内的中文反思（更高层的理解，如“用户周末喜欢安静地玩策略游戏”）。只输出 JSON {\"text\":\"…\"}。".to_string()),
        Message::user(body),
    ]
}

fn spawn_maint<F>(f: F, maint: &mut MemoryMaint)
where
    F: FnOnce() -> MaintOutput + Send + 'static,
{
    let (tx, rx) = channel();
    thread::spawn(move || {
        let _ = tx.send(f());
    });
    maint.rx = Some(Mutex::new(rx));
    maint.pending = true;
}

fn maint_poll(
    maint: &mut MemoryMaint,
    hot: &mut HotMemory,
    store: &mut MemoryStore,
    cfg: &AgentConfig,
) {
    if !maint.pending {
        return;
    }
    let Some(rx) = maint.rx.as_ref() else {
        maint.pending = false;
        return;
    };
    let recv = rx.lock().unwrap().try_recv();
    match recv {
        Ok(out) => {
            maint.pending = false;
            maint.rx = None;
            match out.job {
                "summarize" => {
                    if let Some(memo) = out.memo {
                        hot.memo = memo;
                        for _ in 0..out.dropped {
                            hot.tail.pop_front();
                        }
                    }
                }
                "extract" => {
                    if !out.facts.is_empty() {
                        store.insert_facts(out.facts);
                        store.save_facts();
                    }
                }
                "synth" => {
                    if let Some((refl, ids)) = out.reflection {
                        if !store.reflections.iter().any(|r| r.id == refl.id) {
                            store.reflections.push(refl);
                            store.save_reflections();
                        }
                        let mut changed = false;
                        for f in store.facts.iter_mut() {
                            if ids.contains(&f.id) && !f.absorbed {
                                f.absorbed = true;
                                changed = true;
                            }
                        }
                        if changed {
                            store.save_facts();
                        }
                    }
                }
                _ => {}
            }
        }
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
            maint.pending = false;
            maint.rx = None;
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => {}
    }
    let _ = cfg;
}

/// 每日归档 sweep（facts absorbed>7d；终态 reflections>30d 进分片）。
pub fn archive_sweep(store: &mut MemoryStore) {
    let today = chrono::Local::now().date_naive();
    let mut keep: Vec<Fact> = Vec::new();
    for f in store.facts.drain(..) {
        let old = chrono::NaiveDate::parse_from_str(f.created_at.get(..10).unwrap_or(""), "%Y-%m-%d")
            .map(|d| (today - d).num_days() > ARCHIVE_FACT_DAYS)
            .unwrap_or(false);
        if f.absorbed && old {
            store.archive.push(f);
        } else {
            keep.push(f);
        }
    }
    store.facts = keep;
    let mut active: Vec<Reflection> = Vec::new();
    let mut archived: Vec<Reflection> = Vec::new();
    for r in store.reflections.drain(..) {
        let old = chrono::NaiveDate::parse_from_str(r.created_at.get(..10).unwrap_or(""), "%Y-%m-%d")
            .map(|d| (today - d).num_days() > ARCHIVE_REFLECTION_DAYS)
            .unwrap_or(false);
        let terminal = matches!(r.status, ReflStatus::Promoted | ReflStatus::Denied | ReflStatus::Archived);
        if terminal && old {
            archived.push(r);
        } else {
            active.push(r);
        }
    }
    store.reflections = active;
    if !archived.is_empty() {
        append_reflection_shard(&archived);
    }
    store.save_facts();
    store.save_archive();
    store.save_reflections();
}

fn append_reflection_shard(items: &[Reflection]) {
    let dir = agent_dir().join("reflection_archive");
    let _ = std::fs::create_dir_all(&dir);
    let name = format!("shard-{}.json", chrono::Local::now().format("%Y-%m"));
    let path = dir.join(&name);
    let mut cur: Vec<Reflection> = match std::fs::read_to_string(&path) {
        Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    cur.extend(items.iter().cloned());
    while cur.len() > REFLECTION_ARCHIVE_SHARD_MAX {
        cur.remove(0);
    }
    if let Ok(json) = serde_json::to_string_pretty(&cur) {
        let tmp = dir.join(format!("{name}.tmp"));
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

// ============================
//  systems
// ============================

pub fn record_turn(
    hot: &mut HotMemory,
    store: &MemoryStore,
    user: &str,
    assistant: &str,
    cfg: &AgentConfig,
) {
    if !cfg.memory_enabled {
        return;
    }
    hot.push(user.to_string(), assistant.to_string());
    if hot.total_chars() > HOT_HARD_CAP_CHARS {
        while hot.total_chars() > HOT_HARD_CAP_CHARS && !hot.tail.is_empty() {
            hot.tail.pop_front();
        }
    }
    let mut cursors = load_cursors();
    cursors.turn_seq += 1;
    let seq = cursors.turn_seq;
    save_cursors(&cursors);
    append_episode(&Episode {
        seq,
        ts: now_iso(),
        user: user.to_string(),
        assistant: assistant.to_string(),
    });
    let _ = store;
}

fn maybe_summarize(hot: &HotMemory, maint: &mut MemoryMaint, cfg: &AgentConfig) {
    if maint.pending || !cfg.memory_enabled || !hot.needs_summary() {
        return;
    }
    let snapshot: Vec<crate::memory::HotTurnSnapshot> = hot
        .tail
        .iter()
        .take(hot.tail.len().saturating_sub(HOT_TAIL))
        .map(|t| crate::memory::HotTurnSnapshot {
            user: t.user.clone(),
            assistant: t.assistant.clone(),
        })
        .collect();
    if snapshot.is_empty() {
        return;
    }
    let dropped = snapshot.len();
    spawn_maint(
        move || {
            let turns: Vec<(String, String)> =
                snapshot.iter().map(|t| (t.user.clone(), t.assistant.clone())).collect();
            let client = LlmClient::new();
            let mut lcfg = n3ri_llm::load_config();
            lcfg.max_tokens = 512;
            let memo = client.send(&summarize_prompt(&turns), &lcfg).ok();
            MaintOutput {
                job: "summarize",
                memo,
                dropped,
                facts: Vec::new(),
                reflection: None,
            }
        },
        maint,
    );
}

fn maybe_extract(hot: &HotMemory, store: &MemoryStore, maint: &mut MemoryMaint, cfg: &AgentConfig) {
    if maint.pending || !cfg.memory_enabled {
        return;
    }
    let cursors = load_cursors();
    if !cursors.turn_seq.is_multiple_of(EXTRACT_EVERY_TURNS) {
        return;
    }
    let turns: Vec<(String, String)> = hot
        .tail
        .iter()
        .rev()
        .take(EXTRACT_MAX_TURNS)
        .rev()
        .map(|t| (t.user.clone(), t.assistant.clone()))
        .collect();
    if turns.is_empty() {
        return;
    }
    let known: Vec<String> = store.facts.iter().rev().take(30).map(|f| f.text.clone()).collect();
    spawn_maint(
        move || {
            let client = LlmClient::new();
            let mut lcfg = n3ri_llm::load_config();
            lcfg.max_tokens = 1024;
            let facts = client
                .send(&extract_prompt(&turns, &known), &lcfg)
                .ok()
                .and_then(|s| parse_extract(&s))
                .unwrap_or_default();
            MaintOutput {
                job: "extract",
                memo: None,
                dropped: 0,
                facts,
                reflection: None,
            }
        },
        maint,
    );
}

fn parse_extract(s: &str) -> Option<Vec<Fact>> {
    let start = s.find('[')?;
    let end = s.rfind(']')?;
    let items: Vec<serde_json::Value> = serde_json::from_str(&s[start..=end]).ok()?;
    let mut out = Vec::new();
    for it in items.iter().take(20) {
        let text = it.get("text").and_then(|v| v.as_str()).unwrap_or("").trim();
        if text.is_empty() {
            continue;
        }
        let importance = it.get("importance").and_then(|v| v.as_u64()).unwrap_or(3).clamp(1, 10) as u8;
        let kind = it.get("kind").and_then(|v| v.as_str()).unwrap_or("other").to_string();
        let kind = match kind.as_str() {
            "preference" | "event" | "trait" => kind,
            _ => "other".to_string(),
        };
        out.push(Fact {
            id: fact_id(),
            hash: normalize_hash(text),
            text: text.to_string(),
            importance,
            kind,
            created_at: now_iso(),
            absorbed: false,
        });
    }
    Some(out)
}

fn maybe_synthesize(store: &MemoryStore, maint: &mut MemoryMaint, cfg: &AgentConfig) {
    if maint.pending || !cfg.memory_enabled {
        return;
    }
    let cands: Vec<Fact> = store.unabsorbed().into_iter().cloned().collect();
    if cands.len() < SYNTH_MIN_UNABSORBED {
        return;
    }
    let cursors = load_cursors();
    let today_ts = chrono::Local::now().timestamp();
    if today_ts - cursors.last_synth_ts < 180 {
        return;
    }
    let mut cursors = cursors;
    cursors.last_synth_ts = today_ts;
    save_cursors(&cursors);
    spawn_maint(
        move || {
            let client = LlmClient::new();
            let mut lcfg = n3ri_llm::load_config();
            lcfg.max_tokens = 512;
            let reflection = client
                .send(&synth_prompt(&cands), &lcfg)
                .ok()
                .and_then(|s| parse_synth(&s))
                .map(|text| {
                    let ids: Vec<String> = cands.iter().map(|f| f.id.clone()).collect();
                    (
                        Reflection {
                            id: reflection_id(&ids),
                            text,
                            status: ReflStatus::Pending,
                            source_fact_ids: ids.clone(),
                            reinforcement: 0.0,
                            disputation: 0.0,
                            created_at: now_iso(),
                        },
                        ids,
                    )
                });
            MaintOutput {
                job: "synth",
                memo: None,
                dropped: 0,
                facts: Vec::new(),
                reflection,
            }
        },
        maint,
    );
}

fn parse_synth(s: &str) -> Option<String> {
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    let v: serde_json::Value = serde_json::from_str(&s[start..=end]).ok()?;
    let text = v.get("text").and_then(|v| v.as_str()).unwrap_or("").trim();
    if text.is_empty() || text.chars().count() > 150 {
        return None;
    }
    Some(text.to_string())
}

pub struct MemoryPlugin;

impl Plugin for MemoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HotMemory>()
            .init_resource::<MemoryMaint>()
            .init_resource::<MemoryStoreRes>()
            .add_systems(Update, memory_maintenance_tick);
    }
}

#[derive(Resource, Default)]
pub struct MemoryStoreRes {
    pub store: MemoryStore,
    loaded: bool,
}

fn memory_maintenance_tick(
    cfg: Res<AgentConfig>,
    mut hot: ResMut<HotMemory>,
    mut maint: ResMut<MemoryMaint>,
    mut store_res: ResMut<MemoryStoreRes>,
    mut sweep_acc: Local<f32>,
    time: Res<Time>,
) {
    if !store_res.loaded {
        store_res.store = MemoryStore::load();
        store_res.loaded = true;
    }
    maint_poll(&mut maint, &mut hot, &mut store_res.store, &cfg);
    if !cfg.memory_enabled {
        return;
    }
    if !maint.pending {
        maybe_summarize(&hot, &mut maint, &cfg);
    }
    if !maint.pending {
        maybe_extract(&hot, &store_res.store, &mut maint, &cfg);
    }
    if !maint.pending {
        maybe_synthesize(&store_res.store, &mut maint, &cfg);
    }
    sweep_acc_tick(&mut sweep_acc, &time, &mut store_res.store);
}

fn sweep_acc_tick(acc: &mut f32, time: &Time, store: &mut MemoryStore) {
    *acc += time.delta_secs();
    if *acc < 3600.0 {
        return;
    }
    *acc = 0.0;
    archive_sweep(store);
}

// ============================
//  单测
// ============================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_cjk_bigrams() {
        let toks = tokenize("我喜欢玩国际象棋");
        assert!(toks.contains(&"喜欢".to_string()));
        assert!(toks.contains(&"国际".to_string()));
        let en = tokenize("play chess game");
        assert_eq!(en, vec!["play", "chess", "game"]);
    }

    #[test]
    fn bm25_small_pool_exact_match() {
        let store = MemoryStore {
            facts: vec![Fact {
                id: "fact_1".into(),
                text: "用户喜欢玩国际象棋".into(),
                importance: 8,
                kind: "preference".into(),
                created_at: "2026-09-05T10:00:00".into(),
                absorbed: false,
                hash: "a".into(),
            }],
            archive: vec![],
            reflections: vec![],
            persona: Persona::default(),
        };
        let hits = recall(&store, "国际象棋");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].tag, "fact");
    }

    #[test]
    fn recall_threshold_and_render() {
        let store = MemoryStore {
            facts: vec![Fact {
                id: "fact_1".into(),
                text: "今天天气不错".into(),
                importance: 3,
                kind: "other".into(),
                created_at: "2026-09-05T10:00:00".into(),
                absorbed: false,
                hash: "a".into(),
            }],
            archive: vec![],
            reflections: vec![],
            persona: Persona::default(),
        };
        let hits = recall(&store, "量子物理");
        assert!(hits.is_empty());
        let rendered = render_hits(&recall(&store, "天气"));
        assert!(rendered.contains("[fact"));
    }

    #[test]
    fn reflection_id_stable() {
        let a = reflection_id(&["b".to_string(), "a".to_string()]);
        let b = reflection_id(&["a".to_string(), "b".to_string()]);
        assert_eq!(a, b);
        assert!(a.starts_with("ref_"));
    }

    #[test]
    fn memory_block_budgets() {
        let store = MemoryStore {
            facts: vec![],
            archive: vec![],
            reflections: vec![Reflection {
                id: "ref_x".into(),
                text: "用户周末喜欢安静地玩策略游戏".into(),
                status: ReflStatus::Confirmed,
                source_fact_ids: vec![],
                reinforcement: 1.0,
                disputation: 0.0,
                created_at: "2026-09-05T10:00:00".into(),
            }],
            persona: Persona {
                user: vec![PersonaEntry {
                    id: "manual_1".into(),
                    text: "用户叫阿宅".into(),
                    source: "manual".into(),
                    protected: true,
                    created_at: "2026-09-01T00:00:00".into(),
                }],
                nori: vec![],
                relationship: vec![],
            },
        };
        let hot = HotMemory {
            memo: "用户喜欢下棋".into(),
            tail: VecDeque::new(),
            dirty: false,
        };
        let b = build_memory_block(&store, &hot);
        assert!(b.starts_with("<memory>"));
        assert!(b.contains("阿宅"));
        assert!(b.contains("策略游戏"));
        assert!(b.contains("喜欢下棋"));
    }

    #[test]
    fn extract_parse_tolerates_prose() {
        let s = "好的，这里是结果：[{\"text\":\"用户喜欢国际象棋\",\"importance\":8,\"kind\":\"preference\"}, {\"text\":\"x\",\"importance\":99,\"kind\":\"zzz\"}]";
        let facts = parse_extract(s).expect("解析成功");
        assert_eq!(facts.len(), 2);
        assert_eq!(facts[0].importance, 8);
        assert_eq!(facts[1].importance, 10);
        assert_eq!(facts[1].kind, "other");
    }

    #[test]
    fn hash_normalizes() {
        assert_eq!(normalize_hash("用户喜欢，下棋。"), normalize_hash("用户喜欢下棋"));
        assert_ne!(normalize_hash("喜欢下棋"), normalize_hash("喜欢游泳"));
    }
}

/// 热轮快照（给后台任务传参用的 owned 拷贝，避免借 ECS）。
#[derive(Debug, Clone)]
pub struct HotTurnSnapshot {
    pub user: String,
    pub assistant: String,
}
