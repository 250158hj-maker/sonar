//! データモデル。設計書 データベース §4 の2表をそのまま写す。
//!
//! スパイク3（`spike3.rs`）で検証済みの定義をそのまま移したもの。
//! Toasty 0.7 で張れないもの（外部キー宣言・`CHECK`・部分ユニーク索引）は
//! ここではなくアプリ側で担保する（→ §10）。
//!   - `CHECK (mood IN (...))`        → 下の `Mood` enum
//!   - `UNIQUE ... WHERE parent_id IS NULL` → `store::begin_conversation` に入口を1つに絞る

#[derive(Debug, toasty::Model)]
pub struct Conversation {
    #[key]
    #[auto]
    pub id: u64,

    #[index]
    pub session_id: String,

    /// §10-4：`CHECK` が張れないので、値の担保は `Mood`（下）が持つ。
    pub mood: String,

    /// §10-6：ISO8601 の TEXT。設計書 §4 のとおり。
    pub started_at: String,

    #[has_many]
    pub nodes: toasty::Deferred<Vec<Node>>,
}

#[derive(Debug, toasty::Model)]
pub struct Node {
    #[key]
    #[auto]
    pub id: u64,

    #[index]
    pub conversation_id: u64,

    #[belongs_to(key = conversation_id, references = id)]
    pub conversation: toasty::Deferred<Conversation>,

    /// §10-1・§10-2：自己参照かつ NULL 許容。NULL がその会話の1手目を意味する。
    #[index]
    pub parent_id: Option<u64>,

    /// 子方向（`#[has_many] children`）は張らない。
    /// Toasty 0.7 は NULL 許容の `belongs_to` とペアになる `has_many` を認識できず、
    /// `verify_pair_belongs_to_exists_for_node` が見つからないというエラーになる。
    /// 子は `filter_by_conversation_id` で会話ごと引いてメモリ上で組むので損失は無い（→§6-2）。
    #[belongs_to(key = parent_id, references = id)]
    pub parent: toasty::Deferred<Option<Node>>,

    pub question: String,
    pub answer: String,
    pub created_at: String,
}

// ---------------------------------------------------------------------------
// 気分の5値（→設計書 画面遷移図 §4-1）
//
// `CHECK` が張れない代わりに、DB へ入る前に必ずこの enum を通す。
// 文字列 → enum の変換に失敗したら 400 にする（`store` 側）。
//
// なお `label()` は地図の会話見出しに出す表示名で、`script.js` の
// `MOODS[*].label` と同じ文言。`opener`（1問目の固定文）は**ここに持たない**
// ——1問目は LLM を呼ばず、クライアントが持っている文字列をそのまま
// 回答と一緒に送り返してくる（→設計書 システム構成 §3(a)）ので、
// サーバが同じ文字列をもう1つ持つ理由が無い。
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    Chat,
    Listen,
    Fog,
    Sort,
    None,
}

impl Mood {
    pub fn as_str(self) -> &'static str {
        match self {
            Mood::Chat => "chat",
            Mood::Listen => "listen",
            Mood::Fog => "fog",
            Mood::Sort => "sort",
            Mood::None => "none",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "chat" => Some(Mood::Chat),
            "listen" => Some(Mood::Listen),
            "fog" => Some(Mood::Fog),
            "sort" => Some(Mood::Sort),
            "none" => Some(Mood::None),
            _ => None,
        }
    }

    /// 地図の会話見出しに出す表示名（`script.js` の `MOODS[*].label` と同じ）。
    pub fn label(self) -> &'static str {
        match self {
            Mood::Chat => "なんとなく話したい",
            Mood::Listen => "聞いてほしいことがある",
            Mood::Fog => "もやもやしている",
            Mood::Sort => "考えを整理したい",
            Mood::None => "選ばずに始めた",
        }
    }
}
