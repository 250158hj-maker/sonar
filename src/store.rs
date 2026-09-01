//! DB に触る唯一のモジュール。
//!
//! ページやルートから直接 Toasty を呼ばない。理由は3つ：
//!   1. `parent_id = NULL`（＝会話の1手目）を書く場所を1箇所に絞るため。
//!      部分ユニーク索引は張られないので（→設計書 データベース §10-3）、
//!      「1会話に1手目は1つ」はアプリ側でしか担保できない。
//!   2. 気分の5値を必ず `Mood` enum に通すため（`CHECK` は張られない。→§10-4）。
//!   3. 他人のセッションの会話に触れないようにするため。

use topcoat::{
    Result,
    context::{Cx, app_context},
    cookie::{
        Cookie, Cookies, cookies,
        time::{OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339},
    },
    router::error::{bad_request, forbidden, not_found},
};

use crate::models::{Conversation, Mood, Node};
use crate::questioner::Turn;

/// リクエストごとのハンドル。
///
/// スパイクでは `&mut db` を取っていたが、これは**DBの排他ではなくハンドルへの
/// `&mut`**（`exec(self, executor: &mut dyn Executor)`）。`Db` は
/// `Arc<Shared>` で Clone が安いので、リクエストごとに clone すれば足りる。
/// Mutex も接続プールの自作も要らない。
fn db(cx: &Cx) -> toasty::Db {
    app_context::<toasty::Db>(cx).clone()
}

// ---------------------------------------------------------------------------
// セッション
// ---------------------------------------------------------------------------

const SID: &str = "sonar_sid";

/// 匿名セッションID。無ければここで発行する（→設計書 システム構成 §4）。
///
/// ログインもトークンも無い。Cookie を消すと過去の地図に辿り着けないのは
/// 既知の制約で、学内デモでは無害（→設計書 データベース §8）。
///
/// `MaxAge` を必ず付ける。付けないとセッションCookieになり、
/// **タブを閉じた時点で地図を失う**——9/9 の第三者テストで即座に効く。
/// `Secure` は付けない。dev が `http://localhost` なので、付けると
/// ブラウザによって Cookie 自体が保存されない（逸脱として詳細設計書に記録）。
pub fn session_id(cx: &Cx) -> String {
    let jar = cookies(cx);
    if let Some(c) = jar.get(SID) {
        return c.value().to_owned();
    }
    let sid = uuid::Uuid::new_v4().to_string();
    jar.add(
        Cookie::build((SID, sid.clone()))
            .path("/")
            .http_only(true)
            .same_site(topcoat::cookie::SameSite::Lax)
            .max_age(topcoat::cookie::time::Duration::days(365))
            .build(),
    );
    sid
}

// ---------------------------------------------------------------------------
// 時刻
// ---------------------------------------------------------------------------

/// JST 固定のオフセット。
///
/// 端末のローカルタイムを引かないのは、`time` の `local-offset` が
/// マルチスレッド環境で失敗しうるため。用途が学内デモである以上、
/// JST を決め打ちするほうが正直で、日付が1日ずれない。
const JST: UtcOffset = match UtcOffset::from_hms(9, 0, 0) {
    Ok(o) => o,
    Err(_) => UtcOffset::UTC,
};

/// ISO8601 の TEXT（→設計書 データベース §4・§10-6）。
///
/// 秒で切る。ナノ秒まで持つと設計書 §4 の例と形が変わるうえ、
/// 桁数が可変なので文字列としての大小比較が信用できなくなる。
/// 並び順は `id` で取るので精度は要らない（→`path_to`）。
fn iso_now() -> String {
    OffsetDateTime::now_utc()
        .to_offset(JST)
        .replace_nanosecond(0)
        .map(|t| t.format(&Rfc3339))
        .unwrap_or_else(|_| Ok("1970-01-01T00:00:00+09:00".to_string()))
        .unwrap_or_else(|_| "1970-01-01T00:00:00+09:00".to_string())
}

// ---------------------------------------------------------------------------
// 書き込み
// ---------------------------------------------------------------------------

/// 会話を作り、その1手目を置く。
///
/// **`parent_id: None` を書くのはコードベースでここだけ。**
/// 部分ユニーク索引が張られない以上（→設計書 データベース §10-3）、
/// 「1会話に1手目は1つ」はこの関数が唯一の入口であることでしか担保できない。
/// `append` の親を `Option` にしない（＝ここを迂回できない）のもそのため。
/// **後から「単純化」してここに `Option` を通さないこと。**
///
/// 会話を作るのは気分を選んだときではなく**最初の回答が来たとき**
/// （→設計書 画面遷移図 §6）。選んだだけで離脱した人の分の空の会話が
/// 溜まると「話した回数」が実態とずれる。
pub async fn begin_conversation(
    cx: &Cx,
    mood: Mood,
    question: &str,
    answer: &str,
) -> Result<(u64, u64)> {
    let mut db = db(cx);
    let sid = session_id(cx);
    let now = iso_now();

    let cv = toasty::create!(Conversation {
        session_id: sid,
        mood: mood.as_str(),
        started_at: now.clone(),
    })
    .exec(&mut db)
    .await?;

    let node = toasty::create!(Node {
        conversation_id: cv.id,
        parent_id: None,
        question: question,
        answer: answer,
        created_at: now,
    })
    .exec(&mut db)
    .await?;

    Ok((cv.id, node.id))
}

/// 2手目以降を親にぶら下げる。親は `Option` ではない——1手目の入口は
/// `begin_conversation` だけだという不変条件を、型で守っている。
pub async fn append(
    cx: &Cx,
    conversation_id: u64,
    parent_id: u64,
    question: &str,
    answer: &str,
) -> Result<u64> {
    let mut db = db(cx);
    let sid = session_id(cx);

    let cv = Conversation::get_by_id(&mut db, conversation_id).await?;
    if cv.session_id != sid {
        return Err(forbidden().into());
    }

    // 親が同じ会話のものであることを確認する。別会話のノードにぶら下げると
    // 「別々の会話のノードは合流させない」（→ADR-0003）が壊れ、
    // 根からの距離が経路によって変わる＝縦軸の定義が崩れる。
    let parent = Node::get_by_id(&mut db, parent_id).await?;
    if parent.conversation_id != conversation_id {
        return Err(bad_request("親ノードが別の会話のもの").into());
    }

    let node = toasty::create!(Node {
        conversation_id: conversation_id,
        parent_id: Some(parent_id),
        question: question,
        answer: answer,
        created_at: iso_now(),
    })
    .exec(&mut db)
    .await?;

    Ok(node.id)
}

// ---------------------------------------------------------------------------
// 読み出し
// ---------------------------------------------------------------------------

/// そのノードに至るまでの「根からのパス」と、会話の気分を返す
/// （→設計書 データベース §6-5 ／ システム構成 §3(b)）。
///
/// 会話の全ノードではない。分岐した先では別の枝の発話は文脈に含まれない。
/// 木構造がそのまま「その問いに至る文脈」を定義している。
///
/// 気分はクエリ文字列ではなく**会話の行から読む**。気分をクライアントが
/// 持っているのは会話の行ができる前だけで（→画面遷移図 §6）、
/// できてしまえば DB が正典になる。
/// Toasty の「行が無い」だけを 404 に翻訳する。**それ以外はそのまま上げる。**
///
/// 一律に 404 へ倒すと本物の DB エラーまで「無い」に化けて、壊れていることが
/// 分からなくなる。Toasty は derive が `Error::record_not_found` を生むので、
/// `is_record_not_found()` で「行が無い」と「DB が壊れた」を区別できる。
fn missing_to_404(e: toasty::Error) -> topcoat::Error {
    if e.is_record_not_found() { not_found().into() } else { e.into() }
}

pub async fn path_to(cx: &Cx, node_id: u64) -> Result<(Mood, Vec<Turn>)> {
    let mut db = db(cx);
    let sid = session_id(cx);

    // 行が無いのは**クライアント側の誤り**なので 404 が筋。`?` でそのまま上げると
    // topcoat が 500 に丸め、「サーバが壊れた」と読めてしまう（→テスト項目書 項番40）。
    let node = Node::get_by_id(&mut db, node_id)
        .await
        .map_err(missing_to_404)?;
    let cv = Conversation::get_by_id(&mut db, node.conversation_id)
        .await
        .map_err(missing_to_404)?;
    if cv.session_id != sid {
        // 他人の履歴がプロンプトに入るのを止める
        return Err(forbidden().into());
    }

    // §6-2「会話を開く：その会話の枝を全部」。索引1本で引いて木はメモリ上で組む。
    let all = Node::filter_by_conversation_id(cv.id).exec(&mut db).await?;

    // 根まで辿って反転する。並び順を `created_at` ではなく `id` で持つのは、
    // §6-2 の狙いが「親が子より先に来る」ことだから。ISO8601 の秒解像度では
    // 同着しうるが、`id` は AUTOINCREMENT（→§10-5 実測）なので単調かつ一意。
    let mut chain: Vec<&Node> = Vec::new();
    let mut cur = Some(node_id);
    while let Some(id) = cur {
        let Some(n) = all.iter().find(|n| n.id == id) else { break };
        chain.push(n);
        cur = n.parent_id;
        // 木のはずだが、データ破損で無限ループしないための保険
        if chain.len() > all.len() {
            break;
        }
    }
    chain.reverse();

    let turns = chain
        .into_iter()
        .map(|n| Turn { question: n.question.clone(), answer: n.answer.clone() })
        .collect();

    let mood = Mood::parse(&cv.mood).unwrap_or(Mood::None);
    Ok((mood, turns))
}

/// このセッションの全会話と、それぞれの全ノード（→設計書 データベース §6-2）。
///
/// 会話ごとに1クエリ投げる N+1 になっているが、学内デモの規模
/// （会話は多くて数十件）では索引1本の引きが数回増えるだけで、
/// `Node::all()` から他人のノードごと持ってきて絞るより
/// **所有の境界が壊れない**。困ってから直す。
pub async fn map_of_session(cx: &Cx) -> Result<Vec<(Conversation, Vec<Node>)>> {
    let mut db = db(cx);
    let sid = session_id(cx);

    let mut convs = Conversation::filter_by_session_id(sid).exec(&mut db).await?;
    convs.sort_by_key(|c| c.id);

    let mut out = Vec::with_capacity(convs.len());
    for cv in convs {
        let mut nodes = Node::filter_by_conversation_id(cv.id).exec(&mut db).await?;
        nodes.sort_by_key(|n| n.id);
        out.push((cv, nodes));
    }
    Ok(out)
}
