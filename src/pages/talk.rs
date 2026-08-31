//! 対話画面（→設計書 画面遷移図 §4）。
//!
//! `mock/talk.html` を `view!` へ書き写したもの。モックに無かった
//! 「問いが出せなかった」状態（§5-1）はここで足している。
//!
//! 注意：`view!` は値なしのブール属性を通さないので `hidden` は書けない。
//! モックの `class="hidden"` はそのまま使えるのでそちらに寄せてある。

use topcoat::{Result, asset::asset, router::page, view::view};

use crate::ui::{doc_head, site_header};

#[page("/talk")]
pub async fn talk() -> Result {
    view! {
        <!DOCTYPE html>
        <html lang="ja">
            <head>
                doc_head(title: "Sonar — 話す")
            </head>
            <body class="talk" id="talk">
                site_header(current: "talk")

                // ===========================================================
                // 対話ログ
                // AI の発話は「問い」だけ。相槌・共感・感想は出さない。
                // 済んだやりとりは小さく薄くなり、上へ沈んでいく。
                // ===========================================================
                <div class="talk__log" id="log">
                    <div class="talk__inner">

                        // 済んだやりとり（JSがここに追加していく）
                        <div id="past"></div>

                        // 入口：今の気分を1回だけ聞く。
                        // 追加の一手間ではなく、最初の問いをタップ1回に置き換えるもの。
                        // ※「どれくらい深く話したいか」は聞かない（深さの仕組みを入口で見せないため）。
                        <section class="mood" id="mood">
                            <p class="mood__q">"いま、どんな感じですか。"</p>
                            <ul class="mood__list">
                                <li><button class="mood__chip" data-mood="chat">"なんとなく話したい"</button></li>
                                <li><button class="mood__chip" data-mood="listen">"聞いてほしいことがある"</button></li>
                                <li><button class="mood__chip" data-mood="fog">"もやもやしている"</button></li>
                                <li><button class="mood__chip" data-mood="sort">"考えを整理したい"</button></li>
                            </ul>
                            <button class="stop" id="skipMood">"選ばずに始める"</button>
                        </section>

                        // 現在の問い
                        <div class="now hidden" id="now">
                            <p class="now__q" id="question"></p>
                        </div>

                        // 問いが出せなかったとき（→設計書 画面遷移図 §5-1）。モックには無い状態。
                        // 「エラー」「失敗しました」とは書かない。ユーザーの発話は成功していて、
                        // 失敗したのはこちらの問いだけなので、主語をアプリ側に置く。
                        // 「もう一度」は POST をやり直さない——回答はすでに保存済みで、
                        // 作り直すのは問いだけ。
                        <section class="wrap hidden" id="failed">
                            <p>"うまく問いを作れませんでした。"</p>
                            <button class="btn" id="retry">"もう一度"</button>
                            <button class="stop" id="stopFromFailed">"ここまでにする"</button>
                        </section>

                        // 会話を終えたときのまとめ。
                        // 地図に置かれるのは本人が発した言葉だけで、AIの要約は入れない。
                        <section class="wrap hidden" id="wrap">
                            <h2>"地図に置かれた言葉"</h2>
                            <p id="wrapCount"></p>
                            <ul id="wrapList"></ul>
                            <a class="btn" href="/map">"地図を見る"</a>
                        </section>

                    </div>
                </div>

                // ===========================================================
                // 入力欄
                // 「ここまでにする」を常に見える位置に置く。
                // 深さを強制しない方針なので、やめることを後ろめたくさせない。
                // ===========================================================
                <div class="compose hidden" id="compose">
                    <div class="compose__inner">
                        <div class="compose__row">
                            <textarea id="answer" rows="1" placeholder="ひとことで大丈夫です" aria-label="回答"></textarea>
                            <button class="send" id="send" disabled=(true)>"送る"</button>
                        </div>
                        <div class="compose__foot">
                            <button class="stop" id="stop">"ここまでにする"</button>
                            <button class="stop" id="fillExample">"例を入れる"</button>
                        </div>
                    </div>
                </div>

                <script src=(asset!("../script.js"))></script>
            </body>
        </html>
    }
}

// ===========================================================================
// 1ターンのデータの流れ（→設計書 システム構成 §3）
//
// 設計書 §4 は `POST /talk/answer` が SSE を返す形だが、**EventSource は
// GET しか発行できない**。POST で SSE を受けるには fetch + ReadableStream +
// SSEフレームの自前パースが要り、スパイク4が検証したのは EventSource の方。
// なので POST と SSE を2リクエストに割る。§3 のデータの流れ自体は変わらない。
//
// 割った結果、画面遷移図 §5-1 の保証が**構造になった**：POST と GET の
// あいだでブラウザが死んでも、回答は保存済みで、無いのは問いだけ。
//
// `POST /talk/start` は作らない。1問目は気分ごとの固定文（→プロンプト §2-1）で、
// その文字列は script.js の MOODS[*].opener がすでに持っている。
// クライアントが持っている定数を取りに行くだけの往復になる。
// ===========================================================================

use futures_core::Stream;
use futures_util::StreamExt;
use futures_util::stream;
use serde::{Deserialize, Serialize};
use topcoat::{
    context::Cx,
    router::{
        content::{
            Json,
            sse::{Event, KeepAlive, Sse},
        },
        error::bad_request,
        query_params, route,
    },
};

use crate::models::Mood;
use crate::questioner::{AnthropicQuestioner, Questioner, steer};
use crate::store;

#[derive(Deserialize)]
pub struct AnswerReq {
    /// `None` なら「これが最初の回答」＝会話をここで作る（→画面遷移図 §6）
    conversation_id: Option<u64>,
    /// `conversation_id` が `Some` のときだけ意味を持つ
    parent_id: Option<u64>,
    /// `conversation_id` が `None` のときだけ読む
    mood: String,
    /// ブラウザに出した問い。サーバは会話の途中状態を持たないので、
    /// 次の回答と一緒に送り返してもらう（→システム構成 §3(a)）
    question: String,
    answer: String,
}

#[derive(Serialize)]
pub struct AnswerRes {
    conversation_id: u64,
    node_id: u64,
}

#[route(POST "/talk/answer")]
pub async fn answer(cx: &Cx, Json(req): Json<AnswerReq>) -> topcoat::Result<Json<AnswerRes>> {
    // NOTE: `question` は #[route] が生成する単位構造体と衝突するので `asked`
    let asked = req.question.trim();
    let answer = req.answer.trim();
    if answer.is_empty() {
        return Err(bad_request("回答が空").into());
    }
    if asked.is_empty() {
        return Err(bad_request("直前の問いが無い").into());
    }

    let (conversation_id, node_id) = match (req.conversation_id, req.parent_id) {
        // 1手目。ここだけが会話を作る
        (None, _) => {
            let mood = Mood::parse(&req.mood)
                .ok_or_else(|| bad_request("気分が5値のいずれでもない"))?;
            store::begin_conversation(cx, mood, asked, answer).await?
        }
        // 2手目以降
        (Some(cv), Some(parent)) => {
            let node = store::append(cx, cv, parent, asked, answer).await?;
            (cv, node)
        }
        (Some(_), None) => {
            return Err(bad_request("2手目以降なのに親が無い").into());
        }
    };

    Ok(Json(AnswerRes { conversation_id, node_id }))
}

#[query_params(error = bad_request)]
pub struct QuestionQuery {
    /// 直前に保存されたノード。ここから根まで辿ったものが履歴になる
    node: u64,
}

#[route(GET "/talk/question")]
pub async fn question(
    cx: &Cx,
) -> topcoat::Result<Sse<impl Stream<Item = topcoat::Result<Event>> + use<>>> {
    let q = query_params::<QuestionQuery>(cx)?;

    // DB の読みはここで全部終わらせる。この下は cx を借りない
    // （SSE のストリームはハンドラより長く生きるため）。
    let (mood, history) = store::path_to(cx, q.node).await?;
    let last = history
        .last()
        .ok_or_else(|| bad_request("履歴が空"))?
        .answer
        .clone();

    // 掘り方は「気分 × 直前の回答の長さ」で決まる（→プロンプト §4）。
    // 短さの判定はサーバ側で行い、LLM には結果の指示文だけを渡す。
    let steer = steer(mood, &last);
    println!("[steer] mood={} / {steer}", mood.as_str());

    let questioner = AnthropicQuestioner::from_env()?;
    let stream = questioner.ask(&history, steer).await?;

    // 加工しない。デルタをそのまま1イベントずつ流す（→§3(c) 中継であって蓄積ではない）。
    // JSON にするのは改行を含んでも SSE のフレームが壊れないようにするためで、
    // 組み立て直しではない。
    let deltas = stream.map(|r| match r {
        Ok(text) => Ok(Event::new()
            .event("delta")
            .data(serde_json::to_string(&text).unwrap_or_default())),
        Err(e) => Ok(Event::new().event("failed").data(e.to_string())),
    });

    // 終端を明示する。正常終了した SSE は EventSource の onerror を撃って
    // 再接続を試みるので、`done` が無いと「完了」と「失敗」を区別できない
    // （→画面遷移図 §5-1 のエラー状態がヒューリスティックになってしまう）。
    let done = stream::once(async { Ok(Event::new().event("done").data("")) });

    Ok(Sse::new(deltas.chain(done)).keep_alive(KeepAlive::new()))
}
