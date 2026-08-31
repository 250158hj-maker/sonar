//! 対話画面（→設計書 画面遷移図 §4）。
//!
//! `mock/talk.html` を `view!` へ書き写したもの。モックに無かった
//! 「問いが出せなかった」状態（§5-1）はここで足している。
//!
//! 注意：`view!` は値なしのブール属性を通さないので `hidden` は書けない。
//! モックの `class="hidden"` はそのまま使えるのでそちらに寄せてある。

use topcoat::{Result, asset::asset, router::page, view::view};

use crate::ui::{doc_head, site_header};

#[page("/talk")]
pub async fn talk() -> Result {
    view! {
        <!DOCTYPE html>
        <html lang="ja">
            <head>
                doc_head(title: "Sonar — 話す")
            </head>
            <body class="talk" id="talk">
                site_header(current: "talk")

                // ===========================================================
                // 対話ログ
                // AI の発話は「問い」だけ。相槌・共感・感想は出さない。
                // 済んだやりとりは小さく薄くなり、上へ沈んでいく。
                // ===========================================================
                <div class="talk__log" id="log">
                    <div class="talk__inner">

                        // 済んだやりとり（JSがここに追加していく）
                        <div id="past"></div>

                        // 入口：今の気分を1回だけ聞く。
                        // 追加の一手間ではなく、最初の問いをタップ1回に置き換えるもの。
                        // ※「どれくらい深く話したいか」は聞かない（深さの仕組みを入口で見せないため）。
                        <section class="mood" id="mood">
                            <p class="mood__q">"いま、どんな感じですか。"</p>
                            <ul class="mood__list">
                                <li><button class="mood__chip" data-mood="chat">"なんとなく話したい"</button></li>
                                <li><button class="mood__chip" data-mood="listen">"聞いてほしいことがある"</button></li>
                                <li><button class="mood__chip" data-mood="fog">"もやもやしている"</button></li>
                                <li><button class="mood__chip" data-mood="sort">"考えを整理したい"</button></li>
                            </ul>
                            <button class="stop" id="skipMood">"選ばずに始める"</button>
                        </section>

                        // 現在の問い
                        <div class="now hidden" id="now">
                            <p class="now__q" id="question"></p>
                        </div>

                        // 問いが出せなかったとき（→設計書 画面遷移図 §5-1）。モックには無い状態。
                        // 「エラー」「失敗しました」とは書かない。ユーザーの発話は成功していて、
                        // 失敗したのはこちらの問いだけなので、主語をアプリ側に置く。
                        // 「もう一度」は POST をやり直さない——回答はすでに保存済みで、
                        // 作り直すのは問いだけ。
                        <section class="wrap hidden" id="failed">
                            <p>"うまく問いを作れませんでした。"</p>
                            <button class="btn" id="retry">"もう一度"</button>
                            <button class="stop" id="stopFromFailed">"ここまでにする"</button>
                        </section>

                        // 会話を終えたときのまとめ。
                        // 地図に置かれるのは本人が発した言葉だけで、AIの要約は入れない。
                        <section class="wrap hidden" id="wrap">
                            <h2>"地図に置かれた言葉"</h2>
                            <p id="wrapCount"></p>
                            <ul id="wrapList"></ul>
                            <a class="btn" href="/map">"地図を見る"</a>
                        </section>

                    </div>
                </div>

                // ===========================================================
                // 入力欄
                // 「ここまでにする」を常に見える位置に置く。
                // 深さを強制しない方針なので、やめることを後ろめたくさせない。
                // ===========================================================
                <div class="compose hidden" id="compose">
                    <div class="compose__inner">
                        <div class="compose__row">
                            <textarea id="answer" rows="1" placeholder="ひとことで大丈夫です" aria-label="回答"></textarea>
                            <button class="send" id="send" disabled=(true)>"送る"</button>
                        </div>
                        <div class="compose__foot">
                            <button class="stop" id="stop">"ここまでにする"</button>
                            <button class="stop" id="fillExample">"例を入れる"</button>
                        </div>
                    </div>
                </div>

                <script src=(asset!("../script.js"))></script>
            </body>
        </html>
    }
}
