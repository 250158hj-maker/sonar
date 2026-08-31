//! Sonar — 雑談で、自分の深さを測る。
//!
//! 単一プロセス。バックグラウンドジョブもワーカーもキューも無い
//! （→設計書 システム構成 §1）。

mod models;
mod pages;
mod questioner;
mod store;
mod ui;

use topcoat::{
    asset::{AssetBundle, RouterBuilderAssetExt},
    cookie::RouterBuilderCookieExt,
    router::{Router, RouterBuilderDiscoverExt},
};

#[tokio::main]
async fn main() {
    let path = std::env::var("SONAR_DB").unwrap_or_else(|_| "sonar.db".to_string());

    // `push_schema()` はマイグレーションではない（→設計書 データベース §10）。
    // `IF NOT EXISTS` の無い `CREATE TABLE` を発行するので、毎回呼ぶと
    // 2回目の起動で `table already exists` で落ちる。
    // SQLite は接続時にファイルを作るので、判定は**接続より前**でなければならない。
    let fresh = !std::path::Path::new(&path).exists();

    let db = toasty::Db::builder()
        .models(toasty::models!(crate::models::Conversation, crate::models::Node))
        .connect(&format!("sqlite:{path}"))
        .await
        .expect("DB に接続できない");

    if fresh {
        db.push_schema().await.expect("スキーマを作れない");
        println!("[schema] 新規作成した ({path})");
    } else {
        println!("[schema] 既存のDBに接続した。push_schema は呼ばない ({path})");
    }

    // Toasty の `Db` は `Arc<Shared>` で Clone + Send + Sync。
    // ハンドルを app_context に1つ置き、各リクエストは clone して使う
    // （clone は Arc のカウント増加だけ。→ store::db()）。
    let router = Router::builder()
        .discover()
        .cookies()
        .app_context(db)
        .assets(AssetBundle::load().expect("アセットバンドルが無い（topcoat dev で起動する）"))
        .build();

    topcoat::start(router).await.unwrap();
}
