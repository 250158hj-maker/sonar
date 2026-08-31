//! ホーム画面（→設計書 画面遷移図 §3）。
//!
//! `mock/index.html` を `view!` へ書き写したもの。
//! **統計とプレビューはまだモックの固定値**。DB からの出し分け
//! （会話0件なら空状態、1件以上なら統計＋プレビュー）は §2-4 の作業で、
//! このプラン（縦1本 §2-1〜2-3）のスコープ外。

use topcoat::{Result, asset::asset, router::page, view::view};

use crate::ui::{doc_head, site_header};

#[page("/")]
pub async fn home() -> Result {
    view! {
        <!DOCTYPE html>
        <html lang="ja">
            <head>
                doc_head(title: "Sonar — 雑談で、自分の深さを測る。")
            </head>
            <body class="home">
                site_header(current: "home")

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

                    // 出すのは「自分の言葉がどれだけ残ったか」という量だけ。
                    // 「どこまで深く降りたか」という到達度は出さない（→ mock/README 設計の意図4）
                    <h2 class="sec__title">"これまで"</h2>
                    <div class="stats">
                        <div class="stat">
                            <span class="stat__num">"7"</span>
                            <span class="stat__label">"話した回数"</span>
                        </div>
                        <div class="stat">
                            <span class="stat__num">"23"</span>
                            <span class="stat__label">"地図に残った言葉"</span>
                        </div>
                    </div>

                    <h2 class="sec__title">"あなたの地図"</h2>
                    <a class="preview" href="/map">
                <svg viewBox="0 0 640 220" role="img" aria-label="これまでの会話から作られた地図のプレビュー">
                <line x1="0" y1="49" x2="640" y2="49" class="pv-rule" stroke-dasharray="3 6"></line>
                <line x1="0" y1="80" x2="640" y2="80" class="pv-rule" stroke-dasharray="3 6"></line>
                <line x1="0" y1="111" x2="640" y2="111" class="pv-rule" stroke-dasharray="3 6"></line>
                <line x1="0" y1="142" x2="640" y2="142" class="pv-rule" stroke-dasharray="3 6"></line>
                <line x1="0" y1="173" x2="640" y2="173" class="pv-rule" stroke-dasharray="3 6"></line>
                <line x1="0" y1="204" x2="640" y2="204" class="pv-rule" stroke-dasharray="3 6"></line>
                <g class="pv-edge">
                <path d="M328 18 C 328 32, 65 35, 65 49"></path>
                <path d="M65 49 C 65 63, 30 66, 30 80"></path>
                <path d="M65 49 C 65 63, 100 66, 100 80"></path>
                <path d="M100 80 C 100 94, 100 97, 100 111"></path>
                <path d="M100 111 C 100 125, 100 128, 100 142"></path>
                <path d="M328 18 C 328 32, 170 35, 170 49"></path>
                <path d="M170 49 C 170 63, 170 66, 170 80"></path>
                <path d="M170 80 C 170 94, 170 97, 170 111"></path>
                <path d="M170 111 C 170 125, 170 128, 170 142"></path>
                <path d="M170 142 C 170 156, 170 159, 170 173"></path>
                <path d="M170 173 C 170 187, 170 190, 170 204"></path>
                <path d="M328 18 C 328 32, 240 35, 240 49"></path>
                <path d="M240 49 C 240 63, 240 66, 240 80"></path>
                <path d="M328 18 C 328 32, 345 35, 345 49"></path>
                <path d="M345 49 C 345 63, 310 66, 310 80"></path>
                <path d="M345 49 C 345 63, 380 66, 380 80"></path>
                <path d="M380 80 C 380 94, 380 97, 380 111"></path>
                <path d="M328 18 C 328 32, 450 35, 450 49"></path>
                <path d="M450 49 C 450 63, 450 66, 450 80"></path>
                <path d="M450 80 C 450 94, 450 97, 450 111"></path>
                <path d="M328 18 C 328 32, 520 35, 520 49"></path>
                <path d="M520 49 C 520 63, 520 66, 520 80"></path>
                <path d="M328 18 C 328 32, 590 35, 590 49"></path>
                </g>
                <circle cx="328" cy="18" r="6" class="pv" style="--c: var(--d-root)"></circle>
                <circle cx="65" cy="49" r="5" class="pv" style="--t: 0.32"></circle>
                <circle cx="30" cy="80" r="5" class="pv" style="--t: 0.538"></circle>
                <circle cx="100" cy="80" r="5" class="pv" style="--t: 0.538"></circle>
                <circle cx="100" cy="111" r="5" class="pv" style="--t: 0.686"></circle>
                <circle cx="100" cy="142" r="5" class="pv" style="--t: 0.786"></circle>
                <circle cx="170" cy="49" r="5" class="pv" style="--t: 0.32"></circle>
                <circle cx="170" cy="80" r="5" class="pv" style="--t: 0.538"></circle>
                <circle cx="170" cy="111" r="5" class="pv" style="--t: 0.686"></circle>
                <circle cx="170" cy="142" r="5" class="pv" style="--t: 0.786"></circle>
                <circle cx="170" cy="173" r="5" class="pv" style="--t: 0.855"></circle>
                <circle cx="170" cy="204" r="5" class="pv" style="--t: 0.901"></circle>
                <circle cx="240" cy="49" r="5" class="pv" style="--t: 0.32"></circle>
                <circle cx="240" cy="80" r="5" class="pv" style="--t: 0.538"></circle>
                <circle cx="345" cy="49" r="5" class="pv" style="--t: 0.32"></circle>
                <circle cx="310" cy="80" r="5" class="pv" style="--t: 0.538"></circle>
                <circle cx="380" cy="80" r="5" class="pv" style="--t: 0.538"></circle>
                <circle cx="380" cy="111" r="5" class="pv" style="--t: 0.686"></circle>
                <circle cx="450" cy="49" r="5" class="pv" style="--t: 0.32"></circle>
                <circle cx="450" cy="80" r="5" class="pv" style="--t: 0.538"></circle>
                <circle cx="450" cy="111" r="5" class="pv" style="--t: 0.686"></circle>
                <circle cx="520" cy="49" r="5" class="pv" style="--t: 0.32"></circle>
                <circle cx="520" cy="80" r="5" class="pv" style="--t: 0.538"></circle>
                <circle cx="590" cy="49" r="5" class="pv" style="--t: 0.32"></circle>
                </svg>
                        <span class="preview__cap">"地図を開く →"</span>
                    </a>

                    // 初回起動時（記録ゼロ）の見え方。§2-4 でここと上を出し分ける。
                    <div class="empty hidden" id="emptyState">
                        "まだ地図はありません。"<br>
                        "ひとこと話すと、最初の点が置かれます。"
                    </div>

                </main>

                <script src=(asset!("../script.js"))></script>
            </body>
        </html>
    }
}
