//! 問いを作る境界（→設計書 システム構成 §5 ／ ADR-0002）。
//!
//! スパイク4（`spike4.rs`）で実測済みの実装をそのまま移したもの。
//! 初回トークン 864ms（要件3秒）、`input_tokens` 1,174、`stop_reason` は `end_turn`。
//!
//! **返り値は「問いの文字列」だけ。** 相槌も深掘り判定もこの境界を通れない
//! （→スコープと縮退ライン §2 ／ 設計書 プロンプト §2-3）。
//! usage（input_tokens / output_tokens / stop_reason）も境界から返さず、
//! 実装側のログに落とす。計測値を境界に足すと「問いを作ることしかできない」
//! という形が崩れる。

use std::collections::VecDeque;
use std::pin::Pin;

use futures_core::Stream;
use futures_util::StreamExt;
use futures_util::stream;

use crate::models::Mood;

/// 根からそのノードまでのパスの1段（→設計書 データベース §6-5）
pub struct Turn {
    pub question: String,
    pub answer: String,
}

/// 境界のエラー型。
/// `topcoat::Error` は `From<T: Into<anyhow::Error>>` しか持たず、anyhow は
/// `E: std::error::Error + Send + Sync + 'static` を要求する。`String` も
/// `Box<dyn Error>`（Sized でない）も乗らないので、素直に型を1つ立てる。
#[derive(Debug)]
pub struct QuestionError(String);

impl std::fmt::Display for QuestionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for QuestionError {}

impl From<String> for QuestionError {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for QuestionError {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

pub type QuestionStream =
    Pin<Box<dyn Stream<Item = std::result::Result<String, QuestionError>> + Send>>;

#[allow(async_fn_in_trait)]
pub trait Questioner {
    /// 履歴と掘り方の指示から、次の問いをストリームで返す
    async fn ask(
        &self,
        history: &[Turn],
        steer: &str,
    ) -> std::result::Result<QuestionStream, QuestionError>;
}

// ===========================================================================
// 気分 → 掘り方の指示（→設計書 プロンプト §4）
// 短さの判定はサーバ側で行い、LLM には結果の指示文だけを渡す。
// ===========================================================================

pub const SHORT_ANSWER_CHARS: usize = 20;

pub fn steer(mood: Mood, last_answer: &str) -> &'static str {
    let short = last_answer.chars().count() < SHORT_ANSWER_CHARS;
    match (mood, short) {
        (Mood::Chat, _) => "深めない。直前の話題と同じ深さで、隣にあることを聞く。「なぜ」を聞かない。",
        (Mood::Listen, _) => "話題を変えない。同じ出来事の続きを促す。",
        (Mood::Sort, _) => "深めてよい。話し手が挙げた要素どうしを突き合わせて、選んだ理由の側を聞く。",
        (Mood::Fog, false) => "一歩ずつ深める。",
        (Mood::Fog, true) => "深めない。同じ深さで、いま出ている言葉について聞き直す。",
        (Mood::None, false) => "一歩深める。",
        (Mood::None, true) => "深めない。同じ深さで別のことを聞く。",
    }
}

/// system プロンプトの**正典**（2026-08-31 に Vault から移設）。
///
/// 以前は設計書 プロンプト.md §3 が正典で、Rust はその複製を持っていた。
/// ビルド時に .md から生成する案は ADR-0005 §3-3「Vault は配布物に含められない」
/// と衝突し、GitHub のチェックアウト単体でビルドが壊れるため採らなかった。
/// 設計書側は意図と根拠だけを持ち、本文はこのファイルが持つ。
const SYSTEM_PROMPT: &str = include_str!("prompt/system.md");

// ===========================================================================
// 実装：Anthropic Messages API を reqwest で直叩き（→ADR-0001 §5-3）
// ===========================================================================

pub struct AnthropicQuestioner {
    http: reqwest::Client,
    api_key: String,
}

impl AnthropicQuestioner {
    pub fn from_env() -> std::result::Result<Self, QuestionError> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| "ANTHROPIC_API_KEY が無い（→ADR-0005 §3-5）")?;
        Ok(Self { http: reqwest::Client::new(), api_key })
    }
}

/// ストリームが落ちたことを観測するための番人（→設計書 システム構成 §3(d)）。
/// ブラウザが SSE を閉じるとこれが drop され、reqwest 側も一緒に落ちる。
/// 誰も読まない出力にトークンを払い続けないため。
struct DropWatch(&'static str);

impl Drop for DropWatch {
    fn drop(&mut self) {
        println!("[sse ] {} — reqwest 側のストリームを落とした", self.0);
    }
}

struct SseState {
    inner: Pin<Box<dyn Stream<Item = reqwest::Result<bytes::Bytes>> + Send>>,
    buf: String,
    pending: VecDeque<String>,
    /// これが drop された時点で inner も一緒に落ちる
    _watch: DropWatch,
}

impl Questioner for AnthropicQuestioner {
    async fn ask(
        &self,
        history: &[Turn],
        steer: &str,
    ) -> std::result::Result<QuestionStream, QuestionError> {
        // --- messages の組み立て（→設計書 プロンプト §5）------------------
        // 根からのパスを順に question → answer で交互に積む。
        // 最後は必ず user（本人の最新の回答）で終わる。
        let mut messages = Vec::new();
        for t in history {
            messages.push(serde_json::json!({"role": "assistant", "content": t.question}));
            messages.push(serde_json::json!({"role": "user", "content": t.answer}));
        }

        // --- パラメータ（→設計書 プロンプト §6）---------------------------
        // temperature / thinking / output_config / cache_control は付けない（§11）。
        let body = serde_json::json!({
            "model": "claude-haiku-4-5",
            "max_tokens": 300,
            "stream": true,
            "system": SYSTEM_PROMPT.replace("{steer}", steer),
            "messages": messages,
        });

        // ヘッダは3つだけ。anthropic-beta は付かない（→§6）。
        let resp = self
            .http
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                // 失敗の中身はここでしか見られない。境界から返す QuestionError は
                // 画面側で「うまく問いを作れませんでした」に丸められるので、
                // サーバログに残さないと 9/9 の第三者テスト中に原因が追えない。
                eprintln!("[llm  ] 送信に失敗: {e}");
                format!("送信に失敗: {e}")
            })?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            eprintln!("[llm  ] HTTP {status}: {text}");
            return Err(format!("HTTP {status}: {text}").into());
        }

        let state = SseState {
            inner: Box::pin(resp.bytes_stream()),
            buf: String::new(),
            pending: VecDeque::new(),
            _watch: DropWatch("ストリームが drop された"),
        };

        let s = stream::unfold(state, |mut st| async move {
            loop {
                if let Some(text) = st.pending.pop_front() {
                    return Some((Ok(text), st));
                }

                match st.inner.next().await {
                    Some(Ok(chunk)) => {
                        st.buf.push_str(&String::from_utf8_lossy(&chunk));
                        drain_frames(&mut st.buf, &mut st.pending);
                    }
                    Some(Err(e)) => return Some((Err(format!("受信に失敗: {e}").into()), st)),
                    None => return None,
                }
            }
        });

        Ok(Box::pin(s))
    }
}

/// SSE のフレーム（`\n\n` 区切り）を取り出し、`delta.text` を pending に積む。
/// usage / stop_reason はここでログに出す（境界からは返さない）。
fn drain_frames(buf: &mut String, pending: &mut VecDeque<String>) {
    while let Some(pos) = buf.find("\n\n") {
        let frame: String = buf.drain(..pos + 2).collect();

        for line in frame.lines() {
            let Some(payload) = line.strip_prefix("data: ") else {
                continue;
            };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(payload) else {
                continue;
            };

            match v["type"].as_str() {
                Some("message_start") => {
                    println!("[usage] input_tokens = {}", v["message"]["usage"]["input_tokens"]);
                }
                // 連結しただけで問いになる（加工しない。→§12-1 で実測済み）
                Some("content_block_delta") => {
                    if let Some(t) = v["delta"]["text"].as_str() {
                        pending.push_back(t.to_string());
                    }
                }
                Some("message_delta") => {
                    println!(
                        "[usage] stop_reason = {} / output_tokens = {}",
                        v["delta"]["stop_reason"], v["usage"]["output_tokens"]
                    );
                }
                _ => {}
            }
        }
    }
}

// ===========================================================================
// L1 テスト（→テスト項目書 §4-2 項番7〜13 ／ §4-3 項番14〜19）
// `drain_frames` は private。`tests/` からは見えないので同じモジュールに置く（→§3-1）。
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- §4-2 steer ---------------------------------------------------------

    /// 項番7（正常系）：Fog ＋ 30文字 → 深める側
    #[test]
    fn t07_steer_fog_with_long_answer() {
        assert_eq!(steer(Mood::Fog, &"あ".repeat(30)), "一歩ずつ深める。");
    }

    /// 項番8（境界値）：19文字＝`SHORT_ANSWER_CHARS`(20) 未満 → 短い側
    #[test]
    fn t08_steer_fog_with_19_chars_is_short() {
        assert_eq!(
            steer(Mood::Fog, &"あ".repeat(19)),
            "深めない。同じ深さで、いま出ている言葉について聞き直す。"
        );
    }

    /// 項番9（境界値）：境界ちょうど。判定は `<` であって `<=` ではない
    #[test]
    fn t09_steer_fog_with_20_chars_is_not_short() {
        assert_eq!(steer(Mood::Fog, &"あ".repeat(20)), "一歩ずつ深める。");
    }

    /// 項番10（正常系）：Chat は長さで分岐しない（Listen / Sort も同じ性質）
    #[test]
    fn t10_steer_chat_does_not_branch_on_length() {
        let got: Vec<&str> =
            [0, 19, 20, 40].into_iter().map(|n| steer(Mood::Chat, &"あ".repeat(n))).collect();
        assert!(got.iter().all(|s| *s == got[0]), "0 / 19 / 20 / 40 文字で割れた: {got:?}");
    }

    /// 項番11（境界値）：0文字は短い側
    #[test]
    fn t11_steer_none_with_empty_answer_is_short() {
        assert_eq!(steer(Mood::None, ""), "深めない。同じ深さで別のことを聞く。");
    }

    /// 項番12（境界値）：`chars().count()` で数える。バイト長でも UTF-16 長でもない。
    /// 絵文字10 ＋ かな9 ＝ 19文字。バイト長なら67、UTF-16 長なら29——
    /// どちらで数えても20以上になり「短くない」側へ落ちる。
    #[test]
    fn t12_steer_counts_chars_not_bytes_or_utf16() {
        let answer = format!("{}{}", "😀".repeat(10), "あ".repeat(9));
        assert_eq!(
            steer(Mood::Fog, &answer),
            "深めない。同じ深さで、いま出ている言葉について聞き直す。"
        );
    }

    /// 項番13（正常系）：検査ハーネスと実装の指示文がずれていないこと（→§1-1 の失敗クラス3）。
    /// 7種は `steer` から引き出す。ここに書き写すと、写しどうしの照合になって意味が無い。
    #[test]
    fn t13_all_steer_strings_appear_in_run_py() {
        let run_py = include_str!("../check/run.py");
        let mut all: Vec<&'static str> = Vec::new();
        for mood in [Mood::Chat, Mood::Listen, Mood::Fog, Mood::Sort, Mood::None] {
            for answer in ["", "あ".repeat(SHORT_ANSWER_CHARS).as_str()] {
                let s = steer(mood, answer);
                if !all.contains(&s) {
                    all.push(s);
                }
            }
        }
        let missing: Vec<&str> = all.iter().copied().filter(|s| !run_py.contains(s)).collect();
        assert!(
            all.len() == 7 && missing.is_empty(),
            "steer の戻り値は {}種（期待7種）／ check/run.py に無いもの: {missing:?}",
            all.len()
        );
    }

    // -- §4-3 drain_frames --------------------------------------------------

    /// `content_block_delta` の1フレーム（末尾の `\n\n` 込み）
    fn delta_frame(text: &str) -> String {
        format!("data: {{\"type\":\"content_block_delta\",\"delta\":{{\"text\":\"{text}\"}}}}\n\n")
    }

    fn drain(input: &str) -> (Vec<String>, String) {
        let mut buf = input.to_string();
        let mut pending = VecDeque::new();
        drain_frames(&mut buf, &mut pending);
        (pending.into_iter().collect(), buf)
    }

    /// 項番14（正常系）：pending に1件積まれ、buf が空文字になる
    #[test]
    fn t14_drain_frames_takes_one_complete_frame() {
        assert_eq!(drain(&delta_frame("問い")), (vec!["問い".to_string()], String::new()));
    }

    /// 項番15（正常系）：2フレーム連結 → 受信順に2件
    #[test]
    fn t15_drain_frames_keeps_receive_order() {
        let input = format!("{}{}", delta_frame("問"), delta_frame("い"));
        assert_eq!(drain(&input).0, vec!["問".to_string(), "い".to_string()]);
    }

    /// 項番16（境界値）：`\n\n` を含まない途中までのチャンクは、次のチャンクまで buf に残る
    #[test]
    fn t16_drain_frames_holds_incomplete_chunk() {
        let partial = "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"問";
        assert_eq!(drain(partial), (vec![], partial.to_string()));
    }

    /// 項番17（境界値）：完成した1フレーム ＋ 次フレームの先頭 → pending 1件、buf に未完成分だけ
    #[test]
    fn t17_drain_frames_leaves_only_the_incomplete_tail() {
        let head = "data: {\"type\":\"content_block_de";
        let input = format!("{}{head}", delta_frame("問い"));
        assert_eq!(drain(&input), (vec!["問い".to_string()], head.to_string()));
    }

    /// 項番18（異常系）：壊れた JSON はパニックせず、その行を捨てる
    #[test]
    fn t18_drain_frames_discards_broken_json() {
        assert_eq!(drain("data: {壊れたJSON}\n\n").0, Vec::<String>::new());
    }

    /// 項番19（正常系）：usage / stop_reason は境界から返さない（→詳細設計書 §2）
    #[test]
    fn t19_drain_frames_does_not_emit_message_delta() {
        let frame = "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\
                     \"usage\":{\"output_tokens\":42}}\n\n";
        assert_eq!(drain(frame).0, Vec::<String>::new());
    }
}
