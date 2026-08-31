use crate::ai::types::AiConfig;
use std::time::Duration;

/// A TTS voice entry as returned by the API.
#[derive(Debug, Clone)]
pub struct TtsVoice {
    /// Voice ID used in synthesis requests (e.g. "zh-CN-XiaoxiaoNeural").
    pub voice_id: String,
    /// Display name (e.g. "Xiaoxiao").
    pub voice_name: String,
    pub gender: String,
    pub language: String,
}

/// Fetch available voices from the TTS API.
/// Uses GET ?key=xxx&type=voices (Chinese voices) or type=allvoices (all).
pub fn list_voices(config: &AiConfig) -> Result<Vec<TtsVoice>, String> {
    let url = format!(
        "{}?key={}&type=voices",
        config.tts_api_url.trim_end_matches('/'),
        config.tts_key
    );
    let resp =
        reqwest::blocking::get(&url).map_err(|e| format!("TTS voices request failed: {e}"))?;
    let json: serde_json::Value = resp
        .json()
        .map_err(|e| format!("TTS voices JSON parse failed: {e}"))?;
    let voices = json["data"]
        .as_array()
        .ok_or_else(|| "unexpected voices response format".to_string())?;
    Ok(voices
        .iter()
        .map(|v| TtsVoice {
            voice_id: v["voice_id"].as_str().unwrap_or("").to_string(),
            voice_name: v["voice_name"].as_str().unwrap_or("").to_string(),
            gender: v["gender"].as_str().unwrap_or("").to_string(),
            language: v["language"].as_str().unwrap_or("").to_string(),
        })
        .collect())
}

/// Synthesize text to speech. Returns MP3 bytes on success.
///
/// Uses GET request with query parameters (per the API documentation):
///   ?key=xxx&type=speech&text=...&voice=...&speed=1.0&format=mp3&model=tts-1-hd
///
/// Some providers return raw audio directly; others return an HTML page
/// with an embedded `<source src="...">` — we handle both.
pub fn synthesize(config: &AiConfig, text: &str, voice: &str) -> Result<Vec<u8>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("TTS HTTP client build failed: {e}"))?;

    let url = format!(
        "{}?key={}&type=speech&text={}&voice={}&speed=1.0&format=mp3&model=tts-1-hd",
        config.tts_api_url.trim_end_matches('/'),
        config.tts_key,
        urlencoding(text),
        urlencoding(voice),
    );

    let resp = client
        .get(&url)
        .send()
        .map_err(|e| format!("TTS request failed: {e}"))?;

    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if content_type.contains("audio/") || content_type.contains("octet-stream") {
        // Direct audio response
        resp.bytes()
            .map(|b| b.to_vec())
            .map_err(|e| format!("TTS read response failed: {e}"))
    } else if content_type.contains("text/html") || content_type.contains("text/plain") {
        // HTML wrapper — extract the first <source src="..."> URL and fetch it
        let body = resp
            .text()
            .map_err(|e| format!("TTS read HTML response failed: {e}"))?;
        fetch_audio_from_html(&client, &body)
    } else {
        let body = resp.text().unwrap_or_default();
        Err(format!(
            "TTS unexpected response ({}): {}",
            content_type, body
        ))
    }
}

/// Parse an HTML response for `<source src="URL" type="audio/...">` and fetch the audio.
fn fetch_audio_from_html(
    client: &reqwest::blocking::Client,
    html: &str,
) -> Result<Vec<u8>, String> {
    // Look for: <source src="URL" type="audio/mpeg">
    if let Some(src_start) = html.find("<source src=\"") {
        let after = &html[src_start + "<source src=\"".len()..];
        if let Some(src_end) = after.find('"') {
            let audio_url = &after[..src_end];
            // HTML-entity decode &amp; → &
            let audio_url = audio_url.replace("&amp;", "&");
            let audio_resp = client
                .get(&audio_url)
                .send()
                .map_err(|e| format!("TTS audio fetch failed: {e}"))?;
            return audio_resp
                .bytes()
                .map(|b| b.to_vec())
                .map_err(|e| format!("TTS audio read failed: {e}"));
        }
    }
    // No audio source found — return the HTML as error context
    Err(format!(
        "TTS HTML response contained no audio source: {html}"
    ))
}

/// Sanitize AI-generated text before TTS synthesis.
///
/// Removes markdown formatting, emotion tags, URLs, and other artifacts
/// that would cause TTS engines to produce garbled audio or noise.
/// Preserves meaningful content (words, punctuation, line breaks).
pub fn sanitize_text_for_tts(text: &str) -> String {
    let mut s = text.to_string();

    // 1. Strip emotion tags: [happy], [sad], etc. — entire bracket + content
    //    Handles multi-word tags too: [surprised], [thinking], [embarrassed]
    strip_emotion_tags(&mut s);

    // 2. Strip markdown images ![alt](url) → keep alt text
    //    Must run before links because images contain a link.
    replace_all(&mut s, |s| {
        if let Some(start) = s.find("![") {
            if let Some(end) = s[start..].find(')') {
                // Extract alt text between ![ and ]
                if let Some(close_bracket) = s[start + 2..].find(']') {
                    let alt = s[start + 2..start + 2 + close_bracket].to_string();
                    let link_end = start + end + 1;
                    let before = s[..start].to_string();
                    let after_part = s[link_end..].to_string();
                    *s = before + &alt + &after_part;
                    return true;
                }
                let after = s[end + 1..].to_string();
                *s = s[..start].to_string() + &after;
                return true;
            }
        }
        false
    });

    // 3. Strip markdown links [text](url) → keep text
    replace_all(&mut s, |s| {
        if let Some(start) = s.find('[') {
            // Skip if preceded by ! (already handled above)
            if start > 0 && s.as_bytes()[start - 1] == b'!' {
                return false;
            }
            if let Some(close_bracket) = s[start..].find(']') {
                let text_end = start + close_bracket;
                if text_end + 1 < s.len() && s.as_bytes()[text_end + 1] == b'(' {
                    if let Some(link_end) = s[text_end + 1..].find(')') {
                        let link_close = text_end + 1 + link_end;
                        let display = s[start + 1..text_end].to_string();
                        let before = s[..start].to_string();
                        let after = s[link_close + 1..].to_string();
                        *s = before + &display + &after;
                        return true;
                    }
                }
            }
        }
        false
    });

    // 4. Remove horizontal rules (---, ***, ___ on their own line)
    replace_all(&mut s, |s| {
        if let Some(start) = s.find("\n---") {
            let end = start + 4;
            let after = s[end..].to_string();
            *s = s[..start].to_string() + "\n" + &after;
            return true;
        }
        if let Some(start) = s.find("\n***") {
            let end = start + 4;
            let after = s[end..].to_string();
            *s = s[..start].to_string() + "\n" + &after;
            return true;
        }
        if let Some(start) = s.find("\n___") {
            let end = start + 4;
            let after = s[end..].to_string();
            *s = s[..start].to_string() + "\n" + &after;
            return true;
        }
        false
    });

    // 5. Strip heading markers: remove "#" "##" etc. at line start
    replace_all(&mut s, |s| {
        for i in 0..s.len() {
            if s.as_bytes()[i] == b'\n' || i == 0 {
                let pos = if i == 0 { 0 } else { i + 1 };
                if pos < s.len() && s.as_bytes()[pos] == b'#' {
                    let after_hash = pos + 1;
                    let skip = after_hash
                        + s[after_hash..]
                            .chars()
                            .take_while(|c| *c == '#' || *c == ' ')
                            .count();
                    let before = s[..if i == 0 { 0 } else { i + 1 }].to_string();
                    let rest = s[skip..].to_string();
                    *s = before + &rest;
                    return true;
                }
            }
        }
        false
    });

    // 6. Strip blockquote markers: remove ">" at line start
    replace_all(&mut s, |s| {
        for i in 0..s.len() {
            if s.as_bytes()[i] == b'\n' || i == 0 {
                let pos = if i == 0 { 0 } else { i + 1 };
                if pos < s.len() && s.as_bytes()[pos] == b'>' {
                    let after = pos + 1;
                    let skip = after + s[after..].chars().take_while(|c| *c == ' ').count();
                    let before = s[..if i == 0 { 0 } else { i + 1 }].to_string();
                    let rest = s[skip..].to_string();
                    *s = before + &rest;
                    return true;
                }
            }
        }
        false
    });

    // 7. Strip inline code backticks: `` `code` `` → code
    replace_all(&mut s, |s| {
        if let Some(start) = s.find('`') {
            if let Some(end) = s[start + 1..].find('`') {
                let code = s[start + 1..start + 1 + end].to_string();
                let before = s[..start].to_string();
                let after = s[start + 2 + end..].to_string();
                *s = before + &code + &after;
                return true;
            }
            // Strip lone backtick
            let before = s[..start].to_string();
            let after = s[start + 1..].to_string();
            *s = before + &after;
            return true;
        }
        false
    });

    // 8. Strip bold/italic markers: **text**, __text__, *text*, _text_, ~~text~~
    //    Run multiple passes to handle nesting
    for _ in 0..3 {
        let changed = strip_matched_pair(&mut s, "**", "**")
            | strip_matched_pair(&mut s, "__", "__")
            | strip_matched_pair(&mut s, "~~", "~~")
            | strip_matched_pair(&mut s, "*", "*")
            | strip_matched_pair(&mut s, "_", "_");
        if !changed {
            break;
        }
    }

    // 9. Strip bare URLs (http://, https://)
    replace_all(&mut s, |s| {
        for proto in &["http://", "https://"] {
            if let Some(start) = s.find(proto) {
                let rest = &s[start..];
                let end = rest
                    .find(|c: char| c.is_whitespace() || c == ')' || c == '"' || c == '>')
                    .unwrap_or(rest.len());
                let before = s[..start].to_string();
                let after = s[start + end..].to_string();
                *s = before + &after;
                return true;
            }
        }
        false
    });

    // 10. Strip list markers: "- ", "+ ", "* ", "1. " at line start
    replace_all(&mut s, |s| {
        for i in 0..s.len() {
            if s.as_bytes()[i] == b'\n' || i == 0 {
                let pos = if i == 0 { 0 } else { i + 1 };
                if pos < s.len() {
                    let c = s.as_bytes()[pos];
                    if c == b'-' || c == b'+' || c == b'*' || c.is_ascii_digit() {
                        let rest = &s[pos..];
                        let marker_end = if c.is_ascii_digit() {
                            rest.find('.').map(|p| pos + p + 1)
                        } else {
                            Some(pos + 1)
                        };
                        if let Some(me) = marker_end {
                            if me < s.len() && s.as_bytes()[me] == b' ' {
                                let skip = me + 1;
                                let before = s[..if i == 0 { 0 } else { i + 1 }].to_string();
                                let rest_s = s[skip..].to_string();
                                *s = before + &rest_s;
                                return true;
                            }
                        }
                    }
                }
            }
        }
        false
    });

    // 11. Collapse multiple whitespace (including newlines) into single space
    {
        let mut collapsed = String::with_capacity(s.len());
        let mut prev_space = false;
        for ch in s.chars() {
            if ch.is_whitespace() {
                if !prev_space {
                    collapsed.push(' ');
                    prev_space = true;
                }
            } else {
                collapsed.push(ch);
                prev_space = false;
            }
        }
        s = collapsed;
    }

    // 12. Trim leading/trailing whitespace
    let s_trimmed = s.trim().to_string();
    s_trimmed
}

// ── helper functions ──

/// Repeatedly apply `f` until no more changes.
fn replace_all<F>(s: &mut String, f: F)
where
    F: Fn(&mut String) -> bool,
{
    while f(s) {}
}

/// Strip a paired delimiter from `s`, keeping the content between them.
/// e.g. `strip_matched_pair("hello **world**!", "**", "**")` → `"hello world!"`.
fn strip_matched_pair(s: &mut String, open: &str, close: &str) -> bool {
    if let Some(start) = s.find(open) {
        let after_open = start + open.len();
        if let Some(end) = s[after_open..].find(close) {
            let content = s[start + open.len()..after_open + end].to_string();
            let before = s[..start].to_string();
            let after = s[after_open + end + close.len()..].to_string();
            *s = before + &content + &after;
            return true;
        }
    }
    false
}

/// Strip emotion tags like `[happy]`, `[sad]`, `[surprised]`, etc.
fn strip_emotion_tags(s: &mut String) {
    loop {
        let start = {
            let bytes = s.as_bytes();
            let mut found = None;
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] == b'[' {
                    // Check if it's an emotion tag (ends with ] and contains only letters)
                    if let Some(end) = s[i + 1..].find(']') {
                        let tag = &s[i + 1..i + 1 + end];
                        if tag.chars().all(|c| c.is_alphabetic() || c == '_') && !tag.is_empty() {
                            found = Some(i);
                            break;
                        }
                    }
                }
                i += 1;
            }
            found
        };
        match start {
            Some(start) => {
                if let Some(end) = s[start + 1..].find(']') {
                    let close = start + 1 + end + 1;
                    // Also strip surrounding whitespace
                    let mut strip_start = start;
                    while strip_start > 0 && s.as_bytes()[strip_start - 1] == b' ' {
                        strip_start -= 1;
                    }
                    let mut strip_end = close;
                    while strip_end < s.len() && s.as_bytes()[strip_end] == b' ' {
                        strip_end += 1;
                    }
                    let before = s[..strip_start].to_string();
                    let after = s[strip_end..].to_string();
                    *s = before + &after;
                } else {
                    break;
                }
            }
            None => break,
        }
    }
}

/// Percent-encode a string for use in URL query parameters.
fn urlencoding(s: &str) -> String {
    // Manual percent-encoding (avoids adding a crate dependency for this)
    let mut out = String::with_capacity(s.len());
    for &byte in s.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push_str("%20"),
            _ => {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    out
}
