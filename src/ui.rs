//! 3画面で共通の外枠。
//!
//! `#[layout]` ではなく `#[component]` にしてある。3つのモックは
//! `<body class="map">` / `<body class="talk" id="talk">` / `<body class="home">` と
//! **layout が持つはずの部分だけが違い**、さらに `script.js` は
//! `</body>` 直前でないと動かない（各 init が getElementById 空振りで黙って return する）。
//! layout に閉じ込めると、その2つがどちらも表現できなくなる。

use topcoat::{
    Result,
    asset::asset,
    view::{Unescaped, component, view},
};

/// 表示モードを**描画前に**当てるインラインスクリプト（→次の作業の入口 §4）。
///
/// `mock/*.html` の `<head>` にあるものと同じ。これが無いと初期表示が
/// ライト固定になり、ダークを選んでいる人には一瞬白い画面が出る。
///
/// **`Unescaped` が要る。** `view!` は `<script>` を特別扱いせず、中身を
/// テキストノードとして扱って `&` `<` `>` をエスケープする
/// （topcoat-view-0.6.2/src/html/escape.rs:18-27）。素で書くと `&&` が
/// `&amp;&amp;` になり、JS SyntaxError でこのスクリプトごと死ぬ。
const THEME_BOOT: &str = r#"(function(){var t=null;try{t=localStorage.getItem("sonar-theme")}catch(e){}
if(t!=="light"&&t!=="dark"){t=window.matchMedia("(prefers-color-scheme: dark)").matches?"dark":"light"}
document.documentElement.dataset.theme=t})();"#;

/// `<head>` の中身。`title` だけが画面ごとに変わる。
#[component]
pub async fn doc_head(title: &str) -> Result {
    view! {
        <meta charset="utf-8">
        <meta name="viewport" content="width=device-width, initial-scale=1">
        <title>(title)</title>
        <link rel="stylesheet" href=(asset!("./style.css"))>
        <script>(Unescaped::new_unchecked(THEME_BOOT))</script>
    }
}

/// ヘッダー。`current` は "home" / "talk" / "map" のいずれか。
///
/// `aria-current` は属性を出し分けるのではなく `"page"` / `"false"` を切り替える。
/// `false` は ARIA として正しく「現在地ではない」を意味するので、
/// 条件付きの属性省略を書かずに済む。
#[component]
pub async fn site_header(current: &str) -> Result {
    view! {
        <header class="head">
            <a class="head__brand" href="/">
                <span class="head__name">"Sonar"</span>
            </a>
            <nav class="head__nav">
                <a href="/" aria-current=(if current == "home" { "page" } else { "false" })>"ホーム"</a>
                <a href="/map" aria-current=(if current == "map" { "page" } else { "false" })>"地図"</a>
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
    }
}
