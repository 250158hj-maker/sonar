//! スパイク3：Toasty で read/write が通るか（→[[HUB]]／設計書 データベース §10）。
//!
//! 使い捨てのスパイクコード。確認するのは §10 の6項目：
//!   1. 自己参照の外部キー（node.parent_id → node.id）を表現できるか
//!   2. NULL 許容の外部キーを扱えるか（1手目の parent_id）
//!   3. 部分ユニーク索引（WHERE parent_id IS NULL）を張れるか
//!   4. CHECK 制約を張れるか
//!   5. 主キーの型（INTEGER で通るか、UUID を要求されるか）
//!   6. 日時の型（TEXT ISO8601 で持てるか）

use toasty::Db;

// ---------------------------------------------------------------------------
// モデル定義。設計書 データベース §4 の2表をそのまま写す。
// ---------------------------------------------------------------------------

#[derive(Debug, toasty::Model)]
struct Conversation {
    #[key]
    #[auto]
    id: u64,

    #[index]
    session_id: String,

    /// §10-4：CHECK が張れない場合の担保は Rust 側の enum（下の `Mood`）。
    mood: String,

    /// §10-6：ISO8601 の TEXT。設計書 §4 のとおり。
    started_at: String,

    #[has_many]
    nodes: toasty::Deferred<Vec<Node>>,
}

#[derive(Debug, toasty::Model)]
struct Node {
    #[key]
    #[auto]
    id: u64,

    #[index]
    conversation_id: u64,

    #[belongs_to(key = conversation_id, references = id)]
    conversation: toasty::Deferred<Conversation>,

    /// §10-1・§10-2：自己参照かつ NULL 許容。NULL がその会話の1手目を意味する。
    #[index]
    parent_id: Option<u64>,

    /// 子方向（#[has_many] children）は張らない。
    /// Toasty 0.7 は NULL 許容の belongs_to とペアになる has_many を認識できず、
    /// `verify_pair_belongs_to_exists_for_node` が見つからないというエラーになる。
    /// 子は filter_by_parent_id で引けるので、設計上の損失は無い（→§6-2）。
    #[belongs_to(key = parent_id, references = id)]
    parent: toasty::Deferred<Option<Node>>,

    question: String,
    answer: String,
    created_at: String,
}

// ---------------------------------------------------------------------------
// §10-4 のフォールバック：mood の5値を Rust 側で担保する。
// 設計書は「張れなければ Rust の enum で担保する（そちらのほうが本来は正しい）」。
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mood {
    Chat,
    Listen,
    Fog,
    Sort,
    None,
}

impl Mood {
    fn as_str(self) -> &'static str {
        match self {
            Mood::Chat => "chat",
            Mood::Listen => "listen",
            Mood::Fog => "fog",
            Mood::Sort => "sort",
            Mood::None => "none",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "chat" => Some(Mood::Chat),
            "listen" => Some(Mood::Listen),
            "fog" => Some(Mood::Fog),
            "sort" => Some(Mood::Sort),
            "none" => Some(Mood::None),
            _ => None,
        }
    }
}

#[tokio::main]
async fn main() -> toasty::Result<()> {
    let path = std::env::var("SONAR_DB").unwrap_or_else(|_| "/tmp/sonar-spike3.db".to_string());

    // Toasty 0.7 の push_schema は `CREATE TABLE`（IF NOT EXISTS なし）を発行し、
    // マイグレーション機構は無い（公開APIは push_schema と reset_db=全削除のみ）。
    // そのまま毎回呼ぶと2回目の起動で "table already exists" で落ちる。
    // SQLite は接続時にファイルを作るので、**接続前に**存在を見て分岐する。
    let fresh = !std::path::Path::new(&path).exists();

    let mut db = Db::builder()
        .models(toasty::models!(crate::*))
        .connect(&format!("sqlite:{path}"))
        .await?;

    if fresh {
        db.push_schema().await?;
        println!("[schema] 新規作成した ({path})");
    } else {
        println!("[schema] 既存のDBに接続した。push_schema は呼ばない ({path})");
    }

    // --- write ------------------------------------------------------------
    let cv = toasty::create!(Conversation {
        session_id: "sess-spike3",
        mood: Mood::Fog.as_str(),
        started_at: "2026-08-31T00:10:00Z",
    })
    .exec(&mut db)
    .await?;
    println!("[write] conversation id={} mood={}", cv.id, cv.mood);

    // 1手目：parent_id = NULL（§10-2）
    let n1 = toasty::create!(Node {
        conversation_id: cv.id,
        parent_id: None,
        question: "そのもやもやは、何をきっかけに出てきましたか。",
        answer: "断りたかったのに、その場で言えなくて引き受けてしまった",
        created_at: "2026-08-31T00:10:05Z",
    })
    .exec(&mut db)
    .await?;
    println!("[write] node id={} parent_id={:?} (1手目)", n1.id, n1.parent_id);

    // 2手目・3手目：自己参照でぶら下げる（§10-1）
    let n2 = toasty::create!(Node {
        conversation_id: cv.id,
        parent_id: Some(n1.id),
        question: "引き受けたあと、いちばん最初に浮かんだのはどんなことでしたか。",
        answer: "また同じことをやってる、と思った",
        created_at: "2026-08-31T00:11:00Z",
    })
    .exec(&mut db)
    .await?;

    let n3 = toasty::create!(Node {
        conversation_id: cv.id,
        parent_id: Some(n2.id),
        question: "「また」というのは、どのあたりから続いていますか。",
        answer: "去年の文化祭のときも同じだった",
        created_at: "2026-08-31T00:12:00Z",
    })
    .exec(&mut db)
    .await?;

    // 枝分かれ（同じ親に2つ子がぶら下がる）
    let n2b = toasty::create!(Node {
        conversation_id: cv.id,
        parent_id: Some(n1.id),
        question: "その場では、どんな言葉が出ましたか。",
        answer: "いいですよ、とだけ言った",
        created_at: "2026-08-31T00:11:30Z",
    })
    .exec(&mut db)
    .await?;
    println!(
        "[write] node id={} parent={:?} / id={} parent={:?} / id={} parent={:?}",
        n2.id, n2.parent_id, n3.id, n3.parent_id, n2b.id, n2b.parent_id
    );

    // --- read -------------------------------------------------------------
    let got = Node::get_by_id(&mut db, n3.id).await?;
    println!("[read ] get_by_id({}) -> answer={:?}", n3.id, got.answer);

    let all = Node::all().exec(&mut db).await?;
    println!("[read ] Node::all() -> {} 件", all.len());

    // §6-2「会話を開く：その会話の枝を全部」を索引で引く
    let of_cv = Node::filter_by_conversation_id(cv.id).exec(&mut db).await?;
    println!("[read ] filter_by_conversation_id({}) -> {} 件", cv.id, of_cv.len());

    // §7「深さは parent_id を辿って求める」（depth は保存しない。→§5）
    let depth_of = |id: u64| -> usize {
        let mut d = 0;
        let mut cur = id;
        while let Some(n) = all.iter().find(|n| n.id == cur) {
            match n.parent_id {
                Some(p) => {
                    d += 1;
                    cur = p;
                }
                None => break,
            }
        }
        d
    };
    for n in &of_cv {
        println!(
            "[tree ] id={} depth={} parent={:?} answer={}",
            n.id,
            depth_of(n.id),
            n.parent_id,
            n.answer
        );
    }

    // --- §10-3：部分ユニーク索引が効いているか ---------------------------
    // 同じ会話に2つ目の「1手目」を入れてみる。
    // 効いていれば失敗し、効いていなければ通ってしまう（＝アプリ側で担保が要る）。
    let second_head = toasty::create!(Node {
        conversation_id: cv.id,
        parent_id: None,
        question: "2つ目の1手目（入ってはいけない）",
        answer: "これが入るなら部分ユニーク索引は効いていない",
        created_at: "2026-08-31T00:13:00Z",
    })
    .exec(&mut db)
    .await;
    match second_head {
        Ok(n) => println!(
            "[§10-3] ⚠️ 2つ目の1手目が入った（id={}）→ 部分ユニーク索引は効いていない。アプリ側で担保が要る",
            n.id
        ),
        Err(e) => println!("[§10-3] ✅ 2つ目の1手目は拒否された: {e}"),
    }

    // --- §10-4：CHECK 相当を Rust 側で担保できるか ------------------------
    println!(
        "[§10-4] Mood::parse(\"fog\")={:?} / Mood::parse(\"casual\")={:?}",
        Mood::parse("fog"),
        Mood::parse("casual")
    );

    Ok(())
}
