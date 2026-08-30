//! スパイク2：`mock/script.js` を編集せずに配れるか（正典 §6 の分岐点）。
//!
//! 合格条件は「配信できる」ではなく「地図が実際に描画され、パン・ズーム・
//! クリックが効く」こと。script.js / style.css は mock からのバイト単位の複製で、
//! 一切編集していない（→ ADR-0005 §3-4）。

use topcoat::{
    Result,
    asset::{AssetBundle, RouterBuilderAssetExt, asset},
    router::{Router, page},
    view::view,
};

#[tokio::main]
async fn main() {
    let router = Router::builder()
        .page(map)
        .assets(AssetBundle::load().unwrap())
        .build();

    topcoat::start(router).await.unwrap();
}

#[page("/")]
async fn map() -> Result {
    view! {
        <!DOCTYPE html>
        <html lang="ja">
            <head>
                <meta charset="utf-8">
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <title>"Sonar — 地図"</title>
                <link rel="stylesheet" href=(asset!("./style.css"))>
            </head>
            <body class="map">
                <header class="head">
                    <a class="head__brand" href="/">
                        <span class="head__name">"Sonar"</span>
                    </a>
                    <nav class="head__nav">
                        <a href="/">"ホーム"</a>
                        <a href="/" aria-current="page">"地図"</a>
                        <button class="themetoggle" id="theme" aria-label="表示モードを切り替える" title="表示モードを切り替える">
                            <svg class="themetoggle__sun" viewBox="0 0 24 24" aria-hidden="true">
                                <circle cx="12" cy="12" r="4.2"></circle>
                                <path d="M12 2.6v2.2M12 19.2v2.2M2.6 12h2.2M19.2 12h2.2M5.3 5.3l1.6 1.6M17.1 17.1l1.6 1.6M18.7 5.3l-1.6 1.6M6.9 17.1l-1.6 1.6"></path>
                            </svg>
                            <svg class="themetoggle__moon" viewBox="0 0 24 24" aria-hidden="true">
                                <path d="M20.2 14.6A8.4 8.4 0 0 1 9.4 3.8a8.4 8.4 0 1 0 10.8 10.8z"></path>
                            </svg>
                        </button>
                    </nav>
                </header>

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
                <script src=(asset!("./script.js"))></script>
            </body>
        </html>
    }
}
