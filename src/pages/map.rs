//! 地図画面（→設計書 画面遷移図 §7）。
//!
//! `mock/map.html` を `view!` へ書き写したもの。スパイク2で
//! 「`script.js` を1バイトも編集せずに配れて、地図が実際に描画される」
//! ことを確認済み。
//!
//! **差し替わるのはデータだけ。** 座標計算（`layout()`）には触らない
//! （→正典 §6「マインドマップの実装方針」）。

use std::collections::BTreeMap;

use serde::Serialize;
use topcoat::{
    Result,
    asset::asset,
    context::Cx,
    router::page,
    view::{Unescaped, view},
};

use crate::models::Mood;
use crate::store;
use crate::ui::{doc_head, site_header};

// ---------------------------------------------------------------------------
// script.js が今まさに消費している形（`CONVERSATIONS` / `NODES`）に合わせる。
// 形を合わせておけば、置き換わるのは2つの const の右辺だけで済む。
// ---------------------------------------------------------------------------

/// 地図では畳んだり開いたりする単位（`script.js` の `CONVERSATIONS[]`）
#[derive(Serialize)]
struct Cv {
    id: String,
    head: String,
    date: String,
    mood: String,
}

/// `script.js` の `NODES[id]`。
/// `depth` と `parent` と `conv` は持たない——`script.js` が `children` から
/// 導出する（→設計書 データベース §5「導出できる値は保存しない」）。
#[derive(Serialize)]
struct Nd {
    children: Vec<String>,
    /// 本人が発した言葉。地図に置かれるのはこれだけで、AIの要約は入れない
    quote: String,
    /// そのとき聞かれたこと
    question: String,
}

#[derive(Serialize)]
struct Payload {
    conversations: Vec<Cv>,
    nodes: BTreeMap<String, Nd>,
}

/// DBの id を `script.js` のキー（文字列）にする。
fn key(id: u64) -> String {
    format!("n{id}")
}

/// `2026-08-31T11:21:00+09:00` → `2026年8月31日`
fn jp_date(iso: &str) -> String {
    let b = iso.as_bytes();
    if b.len() < 10 {
        return iso.to_string();
    }
    fn num(s: &str) -> &str {
        s.trim_start_matches('0')
    }
    let (y, m, d) = (&iso[0..4], num(&iso[5..7]), num(&iso[8..10]));
    format!("{y}年{m}月{d}日")
}

#[page("/map")]
pub async fn map(cx: &Cx) -> Result {
    let session = store::map_of_session(cx).await?;

    let mut conversations = Vec::new();
    let mut nodes: BTreeMap<String, Nd> = BTreeMap::new();
    let mut heads = Vec::new();

    for (cv, ns) in &session {
        let Some(head) = ns.iter().find(|n| n.parent_id.is_none()) else {
            // 1手目の無い会話は作られない（→store::begin_conversation）。
            // 万一あってもここで黙って飛ばす。地図が描けなくなるより良い。
            continue;
        };
        heads.push(key(head.id));
        conversations.push(Cv {
            id: format!("cv{}", cv.id),
            head: key(head.id),
            date: jp_date(&cv.started_at),
            mood: Mood::parse(&cv.mood).unwrap_or(Mood::None).label().to_string(),
        });

        for n in ns {
            let children = ns
                .iter()
                .filter(|c| c.parent_id == Some(n.id))
                .map(|c| key(c.id))
                .collect();
            nodes.insert(
                key(n.id),
                Nd { children, quote: n.answer.clone(), question: n.question.clone() },
            );
        }
    }

    // 根「わたし」はテーブルに存在せず、画面側が描く（→設計書 データベース §4）。
    let empty = conversations.is_empty();
    nodes.insert(
        "root".to_string(),
        Nd {
            children: heads,
            quote: "すべての会話がここから枝分かれします。".to_string(),
            question: String::new(),
        },
    );

    // `serde_json` は `"` と `\` を逃がすが **`<` は逃がさない**。
    // 回答に `</script>` と書かれるとスクリプト要素が途中で閉じ、地図が壊れる
    // （原理上は任意のマークアップが動く）。`<` は正しい JSON エスケープなので無損失。
    let json = serde_json::to_string(&Payload { conversations, nodes })?.replace('<', "\\u003c");
    let boot = format!("window.SONAR={json};");

    view! {
        <!DOCTYPE html>
        <html lang="ja">
            <head>
                doc_head(title: "Sonar — 地図")
            </head>
            <body class="map">
                site_header(current: "map")

                // データは script.js より**先に**置く。fetch にすると
                // initMap() を async にするか callback を足すことになり、
                // それは「データだけを差し替える」制約を越える。
                <script>(Unescaped::new_unchecked(boot))</script>

                if empty {
                    // §5-3：点が1つも無い座標系を描いても意味が無いので
                    // #stage ごと出さない。initMap() は stage を見つけられず
                    // 黙って return する（script.js:462）。
                    <div class="empty">
                        "まだ点がありません。"<br>
                        <a class="btn" href="/talk">"話す"</a>
                    </div>
                } else {
                    <div class="stage" id="stage" data-mode="all">
                        <svg class="chart" id="chart" viewBox="0 0 1000 700"
                             preserveAspectRatio="xMidYMid meet"
                             aria-label="これまでの会話から作られた地図。縦の位置が、その話題を何回掘り下げたかを表す">
                            <defs>
                                <linearGradient id="depth" x1="0" y1="0" x2="0" y2="1">
                                    <stop class="chart__grad--top" offset="0"></stop>
                                    <stop class="chart__grad--mid" offset="0.55"></stop>
                                    <stop class="chart__grad--bot" offset="1"></stop>
                                </linearGradient>
                            </defs>
                            <rect id="bg" x="-20000" y="-20000" width="40000" height="40000" fill="url(#depth)"></rect>
                            <g id="scale"></g>
                            <g id="edges" class="chart__edge"></g>
                            <g id="nodes"></g>
                        </svg>

                        <div class="detailbar" id="detailbar">
                            <button class="nav" id="prev" aria-label="前のノードへ">
                                <span aria-hidden="true">"‹"</span>
                            </button>
                            <div class="detail" id="detail" aria-live="polite">
                                <p class="detail__quote" id="panelQuote"></p>
                                <p class="detail__meta" id="panelDepth"></p>
                                <p class="detail__meta" id="panelDate"></p>
                                <div class="detail__q">
                                    <strong>"このとき聞かれたこと"</strong>
                                    <span id="panelQuestion"></span>
                                </div>
                            </div>
                            <button class="nav" id="next" aria-label="次のノードへ">
                                <span aria-hidden="true">"›"</span>
                            </button>
                        </div>

                        <p class="hint" id="hint"></p>
                        <button class="reset" id="reset" hidden=(true)>"全部を見る"</button>
                    </div>
                }

                // script.js は末尾で initTheme(); initTalk(); initMap(); を素で呼ぶ。
                // 各関数は getElementById が空振りすると黙って return するので、
                // </body> 直前でないと全部が無言で何もしない。
                <script src=(asset!("../script.js"))></script>
            </body>
        </html>
    }
}
