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
    classList: { add(){}, remove(){}, contains(){return false;} },
    appendChild(c){ this.children.push(c); c.parent = this; },
    removeChild(){}, remove(){ if(this.parent) this.parent.children = this.parent.children.filter(x=>x!==this); },
    querySelectorAll(){ return []; },
    addEventListener(t,f){ (this._ev ||= {})[t] = f; },
    insertAdjacentText(_pos, txt){ if (this.parent) this.parent._text += txt; },
    dispatchEvent(){}, focus(){}, get scrollHeight(){return 0;}, set scrollTop(_v){}, get scrollTop(){return 0;},
    innerHTML: "", forEach(){},
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
globalThis.fetch = async () => ({ ok:true, json: async()=>({conversation_id:1,node_id:1}) });

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
console.log("  描き切った:", els.question._text === full ? "✅" : "❌ " + JSON.stringify(els.question._text));
// 表示が縮んだ瞬間があればリセットが起きている
let shrank = snapshots.filter((v,i) => i>0 && v < snapshots[i-1]).length;
console.log("  表示が縮んだ回数:", shrank, shrank === 0 ? "（✅ リセットされていない）" : "（❌ デルタごとにリセットされている）");
const incs = snapshots.slice(1).map((v,i)=>v-snapshots[i]).filter(d=>d>0);
const max = Math.max(...incs);
console.log("  1フレームの最大増分:", max, max <= 2 ? "（✅ 一定速度。塊のまま出ていない）" : "（❌ 塊がそのまま出た）");

if (els.question._text !== full || shrank !== 0 || max > 2) {
  console.error("\n❌ 逐次表示が壊れている");
  process.exit(1);
}
console.log("\n✅ すべて通った");
