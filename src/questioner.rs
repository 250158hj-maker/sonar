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
