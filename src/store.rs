//! DB に触る唯一のモジュール。
//!
//! ページやルートから直接 Toasty を呼ばない。理由は2つ：
//!   1. `parent_id = NULL`（＝会話の1手目）を書く場所を1箇所に絞るため。
//!      部分ユニーク索引は張られないので（→設計書 データベース §10-3）、
//!      「1会話に1手目は1つ」はアプリ側でしか担保できない。
//!   2. 気分の5値を必ず `Mood` enum に通すため（`CHECK` は張られない。→§10-4）。

use topcoat::context::{Cx, app_context};

/// リクエストごとのハンドル。
///
/// スパイクでは `&mut db` を取っていたが、これは**DBの排他ではなくハンドルへの
/// `&mut`**（`exec(self, executor: &mut dyn Executor)`）。`Db` は
/// `Arc<Shared>` で Clone が安いので、リクエストごとに clone すれば足りる。
/// Mutex も接続プールの自作も要らない。
#[allow(dead_code)]
pub fn db(cx: &Cx) -> toasty::Db {
    app_context::<toasty::Db>(cx).clone()
}
