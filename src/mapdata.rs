//! `script.js` が消費する形（`window.SONAR`）を組み立てる。
//!
//! 地図（`/map`）とホームのプレビュー（`/`）が**同じデータを見る**ようにするための
//! モジュール。片方だけが別の作り方をすると、ホームの点の並びと
//! 地図の点の並びが食い違う。クライアント側も同じ `tidyX()` を通すので、
//! 「サーバのデータ1つ・座標の算術1つ」で両画面が揃う。

use std::collections::BTreeMap;

use serde::Serialize;
use topcoat::{Result, context::Cx};

use crate::models::Mood;
use crate::store;

/// 地図では畳んだり開いたりする単位（`script.js` の `CONVERSATIONS[]`）
#[derive(Serialize)]
struct Cv {
    id: String,
    head: String,
    date: String,
    mood: String,
}

/// `script.js` の `NODES[id]`。
/// `depth` / `parent` / `conv` は持たない——`script.js` が `children` から
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

pub struct MapData {
    /// `window.SONAR={…};`。`script.js` より**先に**置く
    pub script: String,
    /// 話した回数
    pub conversations: usize,
    /// 地図に残った言葉の数（合成した根は数えない）
    pub nodes: usize,
}

impl MapData {
    pub fn is_empty(&self) -> bool {
        self.conversations == 0
    }
}

/// DBの id を `script.js` のキー（文字列）にする。
fn key(id: u64) -> String {
    format!("n{id}")
}

/// `2026-08-31T11:21:00+09:00` → `2026年8月31日`
fn jp_date(iso: &str) -> String {
    if iso.len() < 10 {
        return iso.to_string();
    }
    fn num(s: &str) -> &str {
        s.trim_start_matches('0')
    }
    let (y, m, d) = (&iso[0..4], num(&iso[5..7]), num(&iso[8..10]));
    format!("{y}年{m}月{d}日")
}

pub async fn build(cx: &Cx) -> Result<MapData> {
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

    let conversation_count = conversations.len();
    let node_count = nodes.len();

    // 根「わたし」はテーブルに存在せず、画面側が描く（→設計書 データベース §4）。
    // 本人の言葉ではないので、上の node_count には数えない。
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

    Ok(MapData {
        script: format!("window.SONAR={json};"),
        conversations: conversation_count,
        nodes: node_count,
    })
}
