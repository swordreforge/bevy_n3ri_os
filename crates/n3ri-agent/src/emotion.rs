//! 情绪标签解析与分句（M2 从 `chat_capsule` 提取，被动/主动轮共用）。
//!
//! 语义与 `chat_capsule` 原实现一字不差：标签只出现一次、放末尾，解析后剥离。

pub const EMOTION_TAGS: &[(&str, &str)] = &[
    ("[开心]", "happy"),
    ("[难过]", "sad"),
    ("[生气]", "angry"),
    ("[惊讶]", "surprised"),
    ("[困惑]", "confused"),
    ("[得意]", "proud"),
    ("[害羞]", "shy"),
    ("[疲惫]", "tired"),
    ("[平静]", "neutral"),
];

pub fn extract_emotion(text: &str) -> (String, Option<String>) {
    let mut result = text.to_string();
    let mut emotion: Option<String> = None;
    for (tag, key) in EMOTION_TAGS {
        if result.contains(tag) {
            if emotion.is_none() {
                emotion = Some((*key).to_string());
            }
            result = result.replace(tag, "");
        }
    }
    (result, emotion)
}

pub fn split_sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        cur.push(ch);
        if matches!(ch, '。' | '！' | '？' | '!' | '?' | '…' | '~' | '～') {
            let t = cur.trim().to_string();
            if !t.is_empty() {
                out.push(t);
            }
            cur.clear();
        }
    }
    let t = cur.trim().to_string();
    if !t.is_empty() {
        out.push(t);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emotion_parity() {
        let (cleaned, e) = extract_emotion("你好呀。[开心]");
        assert_eq!(e.as_deref(), Some("happy"));
        assert!(!cleaned.contains("[开心]"));
        let (_, e) = extract_emotion("普通一句话。");
        assert_eq!(e, None);
    }

    #[test]
    fn sentences_parity() {
        let s = split_sentences("第一句。第二句！");
        assert_eq!(s.len(), 2);
        assert!(split_sentences("").is_empty());
    }
}
