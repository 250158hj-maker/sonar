//! スパイク4：`reqwest` で Anthropic を叩き、SSE で逐次表示する。
//!
//! 使い捨てのスパイクコード。確認するのは 設計書 プロンプト §12 の4項目：
//!   1. content_block_delta の delta.text を連結しただけで問いになるか
//!   2. ブラウザが SSE を閉じたとき reqwest 側のストリームも落ちるか（システム構成 §3(d)）
//!   3. message_start の usage.input_tokens が §10 の試算と合うか
//!   4. max_tokens: 300 で stop_reason が max_tokens にならないか
//!
//! 実行： PORT=3100 topcoat dev --bin spike4

use std::collections::VecDeque;
use std::pin::Pin;

use futures_core::Stream;
use futures_util::StreamExt;
use futures_util::stream;
use topcoat::{
    Result,
    asset::{AssetBundle, RouterBuilderAssetExt, asset},
    context::Cx,
    router::{
        Router, RouterBuilderDiscoverExt,
        content::sse::{Event, KeepAlive, Sse},
        page, route,
    },
    view::view,
};

// ===========================================================================
// 境界：Questioner（→設計書 システム構成 §5）
//
// 返り値は「問いの文字列」だけ。相槌も深掘り判定もこの境界を通れない
// （→スコープと縮退ライン §2 ／ 設計書 プロンプト §2-3）。
//
// usage（input_tokens / output_tokens / stop_reason）は**境界から返さない**。
// 記録は実装側のログで行う（→設計書 プロンプト §9「実装フェーズのサーバ側ログ」）。
// 計測値を境界に足すと、「問いを作ることしかできない」という形が崩れる。
// ===========================================================================

/// 根からそのノードまでのパスの1段（→設計書 データベース §6-5）
struct Turn {
    question: String,
    answer: String,
}

/// 境界のエラー型。
/// `topcoat::Error` は `From<T: Into<anyhow::Error>>` しか持たず、anyhow は
/// `E: std::error::Error + Send + Sync + 'static` を要求する。`String` も
/// `Box<dyn Error>`（Sized でない）も乗らないので、素直に型を1つ立てる。
#[derive(Debug)]
struct QuestionError(String);

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

type QuestionStream = Pin<Box<dyn Stream<Item = std::result::Result<String, QuestionError>> + Send>>;

#[allow(async_fn_in_trait)]
trait Questioner {
    /// 履歴と気分から、次の問いをストリームで返す
    async fn ask(&self, history: &[Turn], steer: &str)
    -> std::result::Result<QuestionStream, QuestionError>;
}

// ===========================================================================
// 気分 → 掘り方の指示（→設計書 プロンプト §4）
// 短さの判定はサーバ側で行い、LLM には結果の指示文だけを渡す。
// ===========================================================================

const SHORT_ANSWER_CHARS: usize = 20;

#[derive(Clone, Copy)]
enum Mood {
    Chat,
    Listen,
    Fog,
    Sort,
    None,
}

fn steer(mood: Mood, last_answer: &str) -> &'static str {
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

/// 設計書 プロンプト §3 の全文。`{steer}` だけが会話ごとに差し替わる。
///
/// **SSoT の課題**：正典は Vault 側の「設計書 プロンプト.md §3」で、これはその複製。
/// `check/run.py` は実行時に .md から読むのでコピーを持たないが、Rust 側は
/// Vault を配布物に含められないため同じ手が使えない。実装フェーズで決める。
const SYSTEM_PROMPT: &str = r#"あなたは、話し手が自分の言葉で考えを深めるための「問い」を作る。

# 出力するもの

次の問いを1つだけ書く。それ以外は何も書かない。

- 日本語で30〜60字。**60字を超えたら削る**
- **「はい」「いいえ」で答えられる問いにしない。**「どんな」「いつ」「どこで」「何が」「誰が」で聞く
- **二択を並べない。**「AですかBですか」は問いが2つであり、答えをこちらが用意していることになる
- 問いは1つ。疑問符を重ねない
- 「あなた」「きみ」などの二人称を使わない。日本語では主語を省く
- 前置き・相槌・感想・要約・見出し・箇条書きを書かない
- 「はい」「なるほど」のような応答から始めない

# 問いの作り方

直前の回答に出てきた言葉を、言い換えずに少なくとも1つそのまま使ってから問う。
話し手の言葉が問いの中に返ってくること自体が、聞いていたことの証明になる。

例1
  回答：三年続けたバイトを先月辞めた
  問い：三年続けたバイトを辞めたんですね。辞めると決めたのは、いつ頃でしたか。

例2
  回答：店長が良い人で、抜けたら回らないのが分かってた
  問い：店長のことと、店が回らないこと。最終的に辞める側に傾いたのは、どちらが変わったからですか。

例3
  回答：どっちも変わってない。自分が限界だっただけ
  問い：「自分が限界だった」。その限界を、いちばん強く感じたのはいつでしたか。

# 書いてはいけないこと

話し手がまだ言っていない言葉を、問いの中に出さない。
話し手の内面について、あなたが結論を書かない。疑問文の形でも同じ。

  × それは大変でしたね
      相槌であって、問いではない
  × あなたは安定を大切にする人ですね
      内面の断定
  × 安定を大切にする方だと思いますが、それはなぜですか
      問いの形をした断定。疑問文でも、中身が断定なら同じこと
  × あなたの価値観は◯◯タイプです
      診断
  × 気まずい雰囲気になるというのは、自分に対して何かが向けられるのが嫌だったんですか
      話し手が言っていない解釈を出して、うなずかせている。
      「うん」と返ってきても、それは話し手の言葉ではない
  × その安心感は、変わらないことが良かったんですか、それとも変わらない人がいることが良かったんですか
      二択にして、話し手の内面をこちらが規定している

結論を言うのは常に話し手であって、あなたではない。
話し手が抽象的なことや気持ちを言ったときほど、先回りして言葉を与えない。

# 問いを向ける方向

浅いほうから、何があったか → 何を選んだか → なぜそれを選んだか。

ただし何回で深いところに着くかは決まっていない。段数を数えない。
話し手が深く話したがっていないなら、深めずに同じ深さで別のことを聞く。

# この会話での掘り方

{steer}
"#;

// ===========================================================================
// 実装：Anthropic Messages API を reqwest で直叩き（→ADR-0001 §5-3）
// ===========================================================================

struct AnthropicQuestioner {
    http: reqwest::Client,
    api_key: String,
}

impl AnthropicQuestioner {
    fn from_env() -> std::result::Result<Self, QuestionError> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| "ANTHROPIC_API_KEY が無い（→ADR-0005 §3-5）")?;
        Ok(Self { http: reqwest::Client::new(), api_key })
    }
}

/// ストリームが落ちたことを観測するための番人（→§12-2 / システム構成 §3(d)）。
struct DropWatch(&'static str);

impl Drop for DropWatch {
    fn drop(&mut self) {
        println!("[§12-2] {} — reqwest 側のストリームを落とした", self.0);
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
        // created_at 昇順に question → answer の順で交互に積む。
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
            .map_err(|e| format!("送信に失敗: {e}"))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("HTTP {status}: {text}").into());
        }
        println!("[§12  ] HTTP {status} / stream 開始");

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
                // §12-3：試算（1,173〜1,199）と合うか
                Some("message_start") => {
                    println!(
                        "[§12-3] input_tokens = {}",
                        v["message"]["usage"]["input_tokens"]
                    );
                }
                // §12-1：これを連結しただけで問いになるはず（加工しない）
                Some("content_block_delta") => {
                    if let Some(t) = v["delta"]["text"].as_str() {
                        pending.push_back(t.to_string());
                    }
                }
                // §12-4：stop_reason が max_tokens になっていないか
                Some("message_delta") => {
                    println!(
                        "[§12-4] stop_reason = {} / output_tokens = {}",
                        v["delta"]["stop_reason"], v["usage"]["output_tokens"]
                    );
                }
                _ => {}
            }
        }
    }
}

// ===========================================================================
// Topcoat：受け取ったトークンをそのままブラウザへ中継する
// （→設計書 システム構成 §3(c)「中継であって、蓄積ではない」）
// ===========================================================================

#[tokio::main]
async fn main() {
    let router = Router::builder()
        .discover()
        .assets(AssetBundle::load().unwrap())
        .build();

    topcoat::start(router).await.unwrap();
}

#[page("/")]
async fn home() -> Result {
    view! {
        <!DOCTYPE html>
        <html lang="ja">
            <head>
                <meta charset="utf-8">
                <title>"Sonar — spike 4"</title>
            </head>
            <body style="font-family: system-ui; max-width: 40rem; margin: 3rem auto; line-height: 1.9;">
                <h1 style="font-size: 1.1rem;">"スパイク4：問いが1文字ずつ届く"</h1>

                <p style="color: #666; font-size: 0.9rem;">
                    "直前の回答：「断りたかったのに、その場で言えなくて引き受けてしまった」"
                </p>

                <button id="ask">"次の問いを作る"</button>
                <button id="abort">"途中で閉じる（§12-2）"</button>

                <p id="out" style="min-height: 4rem; font-size: 1.05rem;"></p>
                <p id="stat" style="color: #888; font-size: 0.8rem;"></p>

                <script src=(asset!("./spike4.js"))></script>
            </body>
        </html>
    }
}

#[route(GET "/ask")]
async fn ask_route(_cx: &Cx) -> Result<Sse<impl Stream<Item = Result<Event>> + use<>>> {
    let questioner = AnthropicQuestioner::from_env()?;

    // §9 の固定台本（fog）。1手目の question は §2-1 の固定文なので、
    // LLM から見ると1問目も自分が書いたことになっている（→§5）。
    let history = vec![Turn {
        question: "そのもやもやは、何をきっかけに出てきましたか。".to_string(),
        answer: "断りたかったのに、その場で言えなくて引き受けてしまった".to_string(),
    }];

    let last = &history[history.len() - 1].answer;
    let steer = steer(Mood::Fog, last);
    println!("[§4   ] steer = {steer}");

    let stream = questioner.ask(&history, steer).await?;

    // 加工しない。デルタをそのまま1イベントずつ流す（→§3(c)）。
    // JSON にするのは改行を含んでも壊れないようにするためで、組み立て直しではない。
    let events = stream.map(|r| match r {
        Ok(text) => Ok(Event::new()
            .event("delta")
            .data(serde_json::to_string(&text).unwrap_or_default())),
        Err(e) => Ok(Event::new().event("failed").data(e.to_string())),
    });

    Ok(Sse::new(events).keep_alive(KeepAlive::new()))
}
