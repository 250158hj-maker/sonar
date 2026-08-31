//! 地図画面（→設計書 画面遷移図 §7）。
//!
//! `mock/map.html` を `view!` へ書き写したもの。スパイク2で
//! 「`script.js` を1バイトも編集せずに配れて、地図が実際に描画される」
//! ことを確認済み。座標計算（`layout()`）には触らない（→正典 §6）。

use topcoat::{Result, asset::asset, router::page, view::view};

use crate::ui::{doc_head, site_header};

#[page("/map")]
pub async fn map() -> Result {
    view! {
        <!DOCTYPE html>
        <html lang="ja">
            <head>
                doc_head(title: "Sonar — 地図")
            </head>
            <body class="map">
                site_header(current: "map")

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

                // script.js は末尾で initTheme(); initTalk(); initMap(); を素で呼ぶ。
                // 各関数は getElementById が空振りすると黙って return するので、
                // </body> 直前でないと全部が無言で何もしない。
                <script src=(asset!("../script.js"))></script>
            </body>
        </html>
    }
}
