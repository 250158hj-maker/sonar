//! ホーム画面（→設計書 画面遷移図 §3）。
//!
//! `mock/index.html` を `view!` へ書き写したもの。モックとの違いは、
//! **統計もプレビューも DB から作る**こと。
//!
//! モックのプレビューは手置き座標の固定SVG（7会話・24ノード）だったので、
//! 実際の地図と食い違っていた。いまは `mapdata` が地図と同じデータを渡し、
//! `script.js` の `initPreview()` が地図と同じ `tidyX()` で並べる。
//!
//! 初回（会話0件）は統計もプレビューも出さない。「0回」「0個」と並べると、
//! まだ始めていない人を減点する表示になる（→§3）。
//!
//! なお §3 の表は「統計3つ」と書いているが、「いちばん深く掘り下げた回数」は
//! 2026-08-30 に削除済み（→`mock/README.md` 設計の意図4）。2つが正しい。

use topcoat::{
    Result,
    asset::asset,
    context::Cx,
    router::page,
    view::{Unescaped, view},
};

use crate::mapdata;
use crate::ui::{doc_head, site_header};

#[page("/")]
pub async fn home(cx: &Cx) -> Result {
    let data = mapdata::build(cx).await?;
    let empty = data.is_empty();
    let conversations = data.conversations;
    let words = data.nodes;
    let boot = data.script;

    view! {
        <!DOCTYPE html>
        <html lang="ja">
            <head>
                doc_head(title: "Sonar — 雑談で、自分の深さを測る。")
            </head>
            <body class="home">
                site_header(current: "home")

                // プレビューも地図と同じ window.SONAR を読む。
                // script.js より先に置く（→pages/map.rs と同じ理由）。
                <script>(Unescaped::new_unchecked(boot))</script>

                <main class="home__main">

                    // 入口は1アクションだけ。
                    // 「入力が億劫」を解く企画なので、始めるまでに選ばせない。
                    <a class="start" href="/talk">
                        <span class="start__ping"></span>
                        <span class="start__ping"></span>
                        <span class="start__ping"></span>
                        <span class="start__label">"話す"</span>
                        <span class="start__note">"ひとことでも、途中でやめても大丈夫です"</span>
                    </a>

                    if empty {
                        // 初回。数字のゼロは「何もしていない」を突きつける表示になるので出さない。
                        <div class="empty">
                            "まだ地図はありません。"<br>
                            "ひとこと話すと、最初の点が置かれます。"
                        </div>
                    } else {
                        // 出すのは「自分の言葉がどれだけ残ったか」という量だけ。
                        // 「どこまで深く降りたか」という到達度は出さない
                        // （→ mock/README 設計の意図4）
                        <h2 class="sec__title">"これまで"</h2>
                        <div class="stats">
                            <div class="stat">
                                <span class="stat__num">(conversations)</span>
                                <span class="stat__label">"話した回数"</span>
                            </div>
                            <div class="stat">
                                <span class="stat__num">(words)</span>
                                <span class="stat__label">"地図に残った言葉"</span>
                            </div>
                        </div>

                        // 地図のミニプレビュー。中身は initPreview() が入れる。
                        // ラベルは出さない（全体表示と同じ理由。→設計の意図11）
                        <h2 class="sec__title">"あなたの地図"</h2>
                        <a class="preview" href="/map">
                            <svg id="pvChart" viewBox="0 0 640 220" role="img"
                                 aria-label="これまでの会話から作られた地図のプレビュー。押すと地図が開く">
                                <g id="pvRules"></g>
                                <g id="pvEdges" class="pv-edge"></g>
                                <g id="pvNodes"></g>
                            </svg>
                            <span class="preview__cap">"地図を開く →"</span>
                        </a>
                    }

                </main>

                <script src=(asset!("../script.js"))></script>
            </body>
        </html>
    }
}
