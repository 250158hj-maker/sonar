// ホームのプレビューと地図の整合性検査。`node check/preview.mjs`
//
// モックのプレビューは手置き座標の固定SVG（7会話・24ノード）で、
// 実際の地図と食い違っていた。いまは両方が同じ window.SONAR を読み、
// 同じ tidyX() で並べる。ここで見るのは：
//   1. プレビューが実データの全ノードを描いていること
//   2. 会話の左右の並びが地図（全体表示）と一致すること
//   3. 枠（viewBox 640x220）から点がはみ出さないこと
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const NS = "http://www.w3.org/2000/svg";

// --- 実データに近い形の入力（会話3本・深さも枝分かれもばらばら）---
const SONAR = {
  conversations: [
    { id: "cv1", head: "n1", date: "2026年8月31日", mood: "もやもやしている" },
    { id: "cv2", head: "n5", date: "2026年8月31日", mood: "考えを整理したい" },
    { id: "cv3", head: "n8", date: "2026年8月31日", mood: "なんとなく話したい" },
    // 12段。ROW_MAX(31) のまま描くと y=18+12*31=390 で枠(220)を突き抜ける。
    // 正典 §2「回数を固定した層で切らない」以上、深い会話は実際に起こりうる。
    { id: "cv4", head: "d1", date: "2026年8月31日", mood: "考えを整理したい" },
  ],
  nodes: {
    root: { children: ["n1", "n5", "n8", "d1"], quote: "すべての会話がここから枝分かれします。", question: "" },
    n1: { children: ["n2", "n4"], quote: "断れなかった", question: "きっかけは。" },
    n2: { children: ["n3"], quote: "また同じことをやってる", question: "何が。" },
    n3: { children: [], quote: "去年もそうだった", question: "いつから。" },
    n4: { children: [], quote: "いいですよとだけ言った", question: "どんな言葉が。" },
    n5: { children: ["n6"], quote: "進路が決まらない", question: "何について。" },
    n6: { children: ["n7"], quote: "親に言えてない", question: "誰に。" },
    n7: { children: [], quote: "反対されると決めつけてる", question: "何が起きると。" },
    n8: { children: [], quote: "今日は特に何もなかった", question: "気になったことは。" },
    d1: { children: ["d2"], quote: "回答1", question: "問い1。" },
    d2: { children: ["d3"], quote: "回答2", question: "問い2。" },
    d3: { children: ["d4"], quote: "回答3", question: "問い3。" },
    d4: { children: ["d5"], quote: "回答4", question: "問い4。" },
    d5: { children: ["d6"], quote: "回答5", question: "問い5。" },
    d6: { children: ["d7"], quote: "回答6", question: "問い6。" },
    d7: { children: ["d8"], quote: "回答7", question: "問い7。" },
    d8: { children: ["d9"], quote: "回答8", question: "問い8。" },
    d9: { children: ["d10"], quote: "回答9", question: "問い9。" },
    d10: { children: ["d11"], quote: "回答10", question: "問い10。" },
    d11: { children: ["d12"], quote: "回答11", question: "問い11。" },
    d12: { children: [], quote: "回答12", question: "問い12。" },
  },
};

function mkEl(id, tag) {
  return {
    id, tag, children: [], attrs: {}, className: "", dataset: {}, textContent: "",
    style: { _p: {}, setProperty(k, v) { this._p[k] = v; }, getPropertyValue(k) { return this._p[k]; } },
    classList: { add() {}, remove() {}, contains() { return false; } },
    setAttribute(k, v) { this.attrs[k] = String(v); },
    getAttribute(k) { return this.attrs[k]; },
    appendChild(c) { this.children.push(c); return c; },
    removeChild() {}, remove() {}, querySelectorAll() { return []; },
    addEventListener() {}, focus() {}, insertAdjacentText() {},
    getBoundingClientRect() { return { width: 640, height: 220, left: 0, top: 0 }; },
    get scrollHeight() { return 0; }, set scrollTop(_v) {}, get scrollTop() { return 0; },
    innerHTML: "",
  };
}

// ホーム画面に実在する id だけ返す（initTalk / initMap は空振りして return する）
const HOME_IDS = new Set(["pvChart", "pvRules", "pvEdges", "pvNodes", "theme"]);
const els = {};
globalThis.window = { SONAR, matchMedia: () => ({ matches: false, addEventListener() {} }),
  requestAnimationFrame() { return 0; }, cancelAnimationFrame() {}, setInterval() { return 0; },
  clearInterval() {}, addEventListener() {} };
globalThis.document = {
  getElementById(id) { return HOME_IDS.has(id) ? (els[id] ||= mkEl(id)) : null; },
  createElement(t) { return mkEl("_" + t, t); },
  createElementNS(_ns, t) { return mkEl("_" + t, t); },
  documentElement: { dataset: {} }, addEventListener() {}, querySelectorAll() { return []; },
};
globalThis.localStorage = { getItem() { return null; }, setItem() {} };
globalThis.performance = { now: () => 0 };
globalThis.ResizeObserver = class { observe() {} };
globalThis.EventSource = class { addEventListener() {} close() {} };

const src = fs.readFileSync(path.join(ROOT, "src/script.js"), "utf8");
const scope = new Function(src + "\n;return { tidyX: tidyX, NODES: NODES, ROOT: ROOT };")();

const circles = els.pvNodes.children;
const edges = els.pvEdges.children;
const rules = els.pvRules.children;

console.log("=== プレビューが実データを描いているか ===");
const expectNodes = Object.keys(SONAR.nodes).length;          // root 含む
const expectEdges = Object.values(SONAR.nodes).reduce((n, v) => n + v.children.length, 0);
console.log(`  点  : ${circles.length} 個（データ ${expectNodes} 個）`);
console.log(`  枝  : ${edges.length} 本（データ ${expectEdges} 本）`);
console.log(`  目安線: ${rules.length} 本（最大深さと同数）`);

console.log("\n=== 枠（640x220）に収まっているか ===");
const xs = circles.map(c => parseFloat(c.attrs.cx));
const ys = circles.map(c => parseFloat(c.attrs.cy));
const inX = Math.min(...xs) >= 0 && Math.max(...xs) <= 640;
const inY = Math.min(...ys) >= 0 && Math.max(...ys) <= 220;
console.log(`  x: ${Math.min(...xs).toFixed(0)}〜${Math.max(...xs).toFixed(0)} ${inX ? "✅" : "❌"}`);
console.log(`  y: ${Math.min(...ys).toFixed(0)}〜${Math.max(...ys).toFixed(0)} ${inY ? "✅" : "❌"}`);

console.log("\n=== 会話の左右の並びが地図と一致するか ===");
// 地図の全体表示は「畳んだ」状態＝各会話の1手目だけ。同じ tidyX に
// その childrenOf を渡して、会話の並び順を出す。
const collapsed = scope.tidyX(id => (id === scope.ROOT ? scope.NODES[id].children : []));
const mapOrder = SONAR.conversations
  .map(c => c.head).sort((a, b) => collapsed.x[a] - collapsed.x[b]);
// プレビューは全部見せる。会話の1手目だけ取り出して左右に並べる
const expanded = scope.tidyX(id => scope.NODES[id].children);
const pvOrder = SONAR.conversations
  .map(c => c.head).sort((a, b) => expanded.x[a] - expanded.x[b]);
console.log("  地図（全体表示）:", mapOrder.join(" "));
console.log("  プレビュー      :", pvOrder.join(" "));
const sameOrder = mapOrder.join() === pvOrder.join();
console.log("  一致:", sameOrder ? "✅" : "❌");

console.log("\n=== 根と葉の色分け ===");
const rootC = circles.find(c => c.attrs.r === "6");
const leafC = circles.find(c => c.attrs.r === "5");
const okRoot = rootC && rootC.style.getPropertyValue("--c") === "var(--d-root)";
const okLeaf = leafC && leafC.style.getPropertyValue("--t") !== undefined;
console.log("  根は深さのランプに乗せない:", okRoot ? "✅" : "❌");
console.log("  葉は --t で深さを表す    :", okLeaf ? "✅" : "❌");

const fails = [];
if (circles.length !== expectNodes) fails.push(`点が ${circles.length} 個（${expectNodes} 個のはず）`);
if (edges.length !== expectEdges) fails.push(`枝が ${edges.length} 本（${expectEdges} 本のはず）`);
if (!inX || !inY) fails.push("点が枠からはみ出している");
if (!sameOrder) fails.push("会話の並びが地図と食い違う");
if (!okRoot) fails.push("根が深さのランプに乗ってしまっている");
if (!okLeaf) fails.push("葉に深さの色が付いていない");

if (fails.length) { console.error("\n❌ " + fails.join(" / ")); process.exit(1); }
console.log("\n✅ すべて通った");
