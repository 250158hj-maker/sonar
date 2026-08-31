// 逐次表示の検査。`node check/typing.mjs`
//
// 守りたい不変条件は1つ：**表示を消すのは beginTyping だけ。**
// モックの typeOut(text, done) は全文を受け取って questionEl.textContent = ""
// で始めていたので、SSE のデルタごとに呼ぶと表示が毎回リセットされる。
//
// さらに、スパイク4で「Anthropic は短い出力を2〜3デルタにしか刻まない」
// ことが実測されている。受け取ったまま描くと「1文字 → 残り全部」でちらつく。
//
// ここでは DOM を最小限スタブして initTalk() を実際に走らせ、
//   1. 表示が縮む瞬間が無いこと（＝リセットされていない）
//   2. 1フレームの増分が1〜2字に収まること（＝塊が透けていない）
// を見る。ブラウザを開かずに済むので、壊した瞬間に気づける。

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

let now = 0;
const rafQueue = [];
globalThis.performance = { now: () => now };
globalThis.requestAnimationFrame = (fn) => { rafQueue.push(fn); return rafQueue.length; };
globalThis.cancelAnimationFrame = () => {};

function mkEl(id) {
  const el = {
    id, value: "", disabled: false,
    // 実 DOM と同じく textContent への代入は中身を消す
    _tc: "", get textContent(){ return this._tc; }, set textContent(v){ this._tc = v; this._text = v; }, style: { setProperty(){}, height:"" },
    className: "", dataset: {}, children: [], _text: "",
    classList: {
      add(c){ if (id === "failed" && c === "hidden") failedShown = false; },
      remove(c){ if (id === "failed" && c === "hidden") failedShown = true; },
      contains(){ return false; },
    },
    appendChild(c){ this.children.push(c); c.parent = this; },
    removeChild(){}, remove(){ if(this.parent) this.parent.children = this.parent.children.filter(x=>x!==this); },
    querySelectorAll(){ return []; },
    addEventListener(t,f){ (this._ev ||= {})[t] = f; },
    insertAdjacentText(_pos, txt){ if (this.parent) this.parent._text += txt; },
    dispatchEvent(){}, focus(){}, get scrollHeight(){return 0;}, set scrollTop(_v){}, get scrollTop(){return 0;},
    _ih: "", get innerHTML(){return this._ih;}, set innerHTML(v){ this._ih = v; if (v === "") this.children = []; }, forEach(){},
  };
  return el;
}
const TALK_IDS = new Set(["talk","past","now","question","answer","send","stop","compose",
  "log","wrap","wrapList","wrapCount","mood","failed","retry","stopFromFailed","skipMood","theme"]);
const els = {};
globalThis.document = {
  // 対話画面に実在する id だけ返す。それ以外は null（initMap は stage を
  // 見つけられず黙って return する＝実際の挙動と同じ）
  getElementById(id){ return TALK_IDS.has(id) ? (els[id] ||= mkEl(id)) : null; },
  createElement(){ return mkEl("_new"); },
  documentElement: { dataset: {} },
  addEventListener(){}, querySelectorAll(){ return []; },
};
globalThis.window = { matchMedia: () => ({ matches:false, addEventListener(){} }),
                      requestAnimationFrame: globalThis.requestAnimationFrame,
                      cancelAnimationFrame: globalThis.cancelAnimationFrame,
                      setInterval(){return 0;}, clearInterval(){}, addEventListener(){}, SONAR: undefined };
globalThis.localStorage = { getItem(){return null;}, setItem(){} };
globalThis.ResizeObserver = class { observe(){} };
globalThis.EventSource = class { constructor(){ this.h={}; ES.push(this);} addEventListener(t,f){this.h[t]=f;} close(){this.closed=true;} };
const ES = [];
let POSTS = 0;
let failedShown = false;
globalThis.fetch = async () => { POSTS += 1; return { ok:true, json: async()=>({conversation_id:1,node_id:POSTS}) }; };

const src = fs.readFileSync(path.join(ROOT, "src/script.js"), "utf8");
new Function(src)();                       // initTheme/initTalk/initMap が末尾で走る

// 気分を選ぶ = startWith("fog") 相当。chip のリスナは querySelectorAll が空なので
// skipMood 経由で入る（startWith("none")）。
els.skipMood._ev.click();
const q = els.question;

function pump(ms) { now += ms; const fns = rafQueue.splice(0); fns.forEach(f => f(now)); }

// 1問目は固定文。全部いっぺんにキューへ入る。
const opener = "最近、印象に残っていることってありますか。どんな小さなことでも。";
let frames = [];
for (let i = 0; i < 200 && q._text.length < opener.length; i++) { pump(16.7); frames.push(q._text.length); }

console.log("=== 1問目（固定文・全文が一度にキューへ）===");
console.log("  文字数:", opener.length, "/ 描き切った文字数:", q._text.length);
console.log("  一致:", q._text === opener ? "✅" : "❌ " + JSON.stringify(q._text.slice(0,30)));
const deltas = frames.slice(1).map((v,i)=>v-frames[i]).filter(d=>d>0);
console.log("  1フレームあたりの増分:", [...new Set(deltas)].sort((a,b)=>a-b), "（1〜2字なら一定速度）");
const elapsed = frames.length * 16.7;
console.log("  実効速度:", (opener.length / (elapsed/1000)).toFixed(1), "字/秒（設定 24 字/秒）");

// ===== 本命：SSE のデルタが2〜3個の塊で届く場合 =====
// スパイク4の実測「Anthropic は短い出力を2〜3デルタにしか刻まない」を再現する。
// 素の typeOut() ならデルタごとに textContent="" が走り、表示がリセットされる。
els.answer.value = "断りたかったのに、その場で言えなくて引き受けてしまった";
await els.send._ev.click();          // submit() -> fetch -> streamQuestion()
await new Promise(r => setImmediate(r));

const es = ES[ES.length - 1];
const full = "引き受けたあと、いちばん最初に浮かんだのはどんなことでしたか。";
const lumps = [full.slice(0,1), full.slice(1,20), full.slice(20)];   // 1字 → 19字 → 残り

console.log("\n=== 2問目（SSE・デルタ3個: %d/%d/%d 字）===", ...lumps.map(l=>l.length));
const snapshots = [];
let fed = 0;
for (let i = 0; i < 400; i++) {
  // 最初の3フレームでデルタを一気に流し込む（＝到着は速い、描画は遅い）
  if (fed < lumps.length && i % 2 === 0) { es.h.delta({ data: JSON.stringify(lumps[fed]) }); fed++; }
  if (fed === lumps.length && !es.closed) es.h.done();
  pump(16.7);
  snapshots.push(els.question._text.length);
  if (els.question._text === full) break;
}
const okDrawn = els.question._text === full;
console.log("  描き切った:", okDrawn ? "✅" : "❌ " + JSON.stringify(els.question._text));
// 表示が縮んだ瞬間があればリセットが起きている
let shrank = snapshots.filter((v,i) => i>0 && v < snapshots[i-1]).length;
console.log("  表示が縮んだ回数:", shrank, shrank === 0 ? "（✅ リセットされていない）" : "（❌ デルタごとにリセットされている）");
const incs = snapshots.slice(1).map((v,i)=>v-snapshots[i]).filter(d=>d>0);
const max = Math.max(...incs);
console.log("  1フレームの最大増分:", max, max <= 2 ? "（✅ 一定速度。塊のまま出ていない）" : "（❌ 塊がそのまま出た）");


// ===== 問いが出せなかったとき（→設計書 画面遷移図 §5-1）=====
// 実測で Anthropic は時々失敗する（連続リクエストで 500 を観測）。
// 失敗しても会話を終わらせないこと、答えた分の点が消えないことを見る。
console.log("\n=== SSE が1デルタも返さずに切れた場合 ===");
els.answer.value = "去年の文化祭のときも同じだった";
await els.send._ev.click();
await new Promise(r => setImmediate(r));
const es2 = ES[ES.length - 1];
const placedBefore = els.wrapList.children.length;
es2.onerror();                              // 1つも届かないまま切れる
const okFailed = failedShown;
console.log("  #failed を出したか:", okFailed ? "✅" : "❌");
const okClosed = es2.closed;
console.log("  EventSource を閉じたか:", okClosed ? "✅（トークンを払い続けない）" : "❌");

// 「もう一度」は POST をやり直さない——ノードは保存済みで、作り直すのは問いだけ
const postsBefore = POSTS;
els.retry._ev.click();
await new Promise(r => setImmediate(r));
const okNoRepost = POSTS === postsBefore;
console.log("  「もう一度」で再POSTしていないか:", okNoRepost ? "✅" : "❌ POSTが増えた");
const okRestream = ES.length > 2;
console.log("  問いだけ作り直したか:", okRestream ? "✅" : "❌");

// 会話を終える → 答えた分がまとめに残る
els.stop._ev.click();
const placedCount = els.wrapList.children.length;
console.log("  まとめに残った点の数:", placedCount, "（答えた回数と一致すべき）");


// ===== 受信中に「ここまでにする」（→設計書 画面遷移図 §5-2）=====
// 中断しないと、誰も読まない出力にトークンを払い続ける。
// stopTyping() は EventSource.close() と対でなければならない。
console.log("\n=== 受信中に「ここまでにする」を押す ===");
els.answer.value = "まだ途中だけど、ここでやめる";
await els.send._ev.click();
await new Promise(r => setImmediate(r));
const es3 = ES[ES.length - 1];
es3.h.delta({ data: JSON.stringify("途中まで届いた") });   // 受信中
pump(16.7);
els.stop._ev.click();                                      // ここまでにする
const okAborted = es3.closed;
console.log("  受信中の EventSource を閉じたか:", okAborted ? "✅" : "❌ 払い続けてしまう");

// ===== 判定 =====
const fails = [];
if (!okDrawn)          fails.push("問いを描き切れていない");
if (shrank !== 0)      fails.push("デルタごとに表示がリセットされている");
if (max > 2)           fails.push("塊がそのまま出ている（一定速度になっていない）");
if (!okFailed)         fails.push("SSE 失敗時に #failed を出していない");
if (!okClosed)         fails.push("SSE 失敗時に EventSource を閉じていない");
if (!okNoRepost)       fails.push("「もう一度」で回答を再POSTしている");
if (!okRestream)       fails.push("「もう一度」で問いを作り直していない");
if (!okAborted)        fails.push("受信中の中断で EventSource を閉じていない");


if (fails.length) {
  console.error("\n❌ " + fails.join(" / "));
  process.exit(1);
}
console.log("\n✅ すべて通った");
