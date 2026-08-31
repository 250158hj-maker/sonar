// 地図の検査。`node check/map.mjs`
//
// **このファイルが無かったせいで、地図を丸ごと壊したまま出してしまった。**
// tidyX を切り出したとき layout() の末尾に残った `leaves: slot` を見落とし、
// initMap が ReferenceError で落ちる状態で commit した。
// typing.mjs も preview.mjs も initMap を走らせていなかったので誰も気づかなかった。
//
// ここで見るのは：
//   1. initMap が例外を投げずに最後まで走ること（← 上のクラスのバグ）
//   2. 全体表示で点が viewBox に収まること
//   3. 会話を開いたとき、枝全体が入るか、入らないなら
//      **根が見えていて縦の間隔が下限を割らない**こと（引きすぎない設計）
//   4. 深さの目安線が段数と一致すること
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const VIEW = { width: 1280, height: 720 };
const MIN_GAP_PX = 58;   // script.js と同じ値
const DEPTH_GAP = 130;

function chain(turns, branch) {
  const nodes = { root: { children: ["d1"], quote: "根", question: "" } };
  for (let i = 1; i <= turns; i++) {
    const ch = i < turns ? ["d" + (i + 1)] : [];
    if (branch && i === 2) ch.push("b1");
    nodes["d" + i] = { children: ch, quote: "回答" + i, question: "問い" + i + "。" };
  }
  if (branch) nodes.b1 = { children: [], quote: "枝の回答", question: "枝の問い。" };
  return {
    conversations: [{ id: "cv1", head: "d1", date: "2026年8月31日", mood: "もやもやしている" }],
    nodes,
  };
}

/* 蓄積のケース。会話を n 本並べる（全体表示は会話単位で畳まれる）。 */
function many(n) {
  const nodes = { root: { children: [], quote: "根", question: "" } };
  const conversations = [];
  let id = 0;
  for (let c = 1; c <= n; c++) {
    const depth = 1 + (c % 5);
    const head = "n" + ++id;
    nodes.root.children.push(head);
    nodes[head] = { children: [], quote: "回答" + c, question: "問い" + c + "。" };
    conversations.push({ id: "cv" + c, head, date: "2026年8月31日", mood: "もやもやしている" });
    let prev = head;
    for (let d = 2; d <= depth; d++) {
      const k = "n" + ++id;
      nodes[prev].children.push(k);
      nodes[k] = { children: [], quote: `回答${c}-${d}`, question: `問い${c}-${d}。` };
      prev = k;
    }
  }
  return { conversations, nodes };
}

function run(SONAR, openHead) {

  let CLOCK = 0;
  /* ハンドル付きで持つ。**ここを配列にして cancel でまとめて捨てると、
     animateTo(ビューの補間) が applyLayout(点の移動) のフレームまで
     消してしまい、点が動かないまま「動いた」ことになる。**
     全体表示と会話を開いた状態で根の位置が同じだった頃は症状が出ず、
     放射状にして初めて露見した。 */
  const RAF = new Map();
  let nextRaf = 1;
  const PENDING = [];
  const els = {};
  const IDS = new Set(["stage","chart","scale","edges","nodes","detail","detailbar","prev","next",
    "reset","panelQuote","panelDepth","panelDate","panelQuestion","hint","bg","theme"]);

  function mkEl(id, tag) {
    const e = {
      id, tag, children: [], attrs: {}, dataset: {}, _cls: new Set(), _ev: {},
      // 実 DOM と同じく textContent への代入は子要素ごと消す
      // （drawScale は gScale.textContent = "" で引き直す）
      _tc: "", get textContent() { return this._tc; },
      set textContent(v) { this._tc = v; if (v === "") this.children = []; },
      style: { _p: {}, setProperty(k, v) { this._p[k] = v; }, getPropertyValue(k) { return this._p[k]; } },
      setAttribute(k, v) { this.attrs[k] = String(v); }, getAttribute(k) { return this.attrs[k]; },
      appendChild(c) { this.children.push(c); return c; }, removeChild() {}, remove() {},
      querySelectorAll() { return []; }, focus() {}, insertAdjacentText() {},
      addEventListener(t, f) { (this._ev[t] ||= []).push(f); },
      getBoundingClientRect() { return { width: VIEW.width, height: VIEW.height, left: 0, top: 0 }; },
      innerHTML: "", get scrollHeight() { return 0; }, set scrollTop(_v) {}, get scrollTop() { return 0; },
    };
    e.classList = { add: c => e._cls.add(c), remove: c => e._cls.delete(c),
      toggle: (c, on) => (on ? e._cls.add(c) : e._cls.delete(c)), contains: c => e._cls.has(c) };
    return e;
  }
  globalThis.performance = { now: () => CLOCK };
  globalThis.requestAnimationFrame = f => { const h = nextRaf++; RAF.set(h, f); return h; };
  globalThis.cancelAnimationFrame = h => { RAF.delete(h); };
  const drain = () => {
    for (let i = 0; i < 400 && RAF.size; i++) {
      CLOCK += 16.7;
      const fns = [...RAF.values()];
      RAF.clear();
      fns.forEach(f => f(CLOCK));
    }
  };
  globalThis.window = { SONAR, matchMedia: () => ({ matches: false, addEventListener() {} }),
    requestAnimationFrame: f => globalThis.requestAnimationFrame(f),
    cancelAnimationFrame: h => globalThis.cancelAnimationFrame(h),
    setInterval: () => 0, clearInterval() {}, addEventListener() {},
    innerWidth: VIEW.width, innerHeight: VIEW.height };
  globalThis.document = {
    getElementById(id) { return IDS.has(id) ? (els[id] ||= mkEl(id)) : null; },
    createElement: t => mkEl("_" + t, t), createElementNS: (_n, t) => mkEl("_" + t, t),
    documentElement: { dataset: {} }, addEventListener() {}, querySelectorAll() { return []; },
  };
  globalThis.localStorage = { getItem: () => null, setItem() {} };
  // 本物の ResizeObserver は observe() で同期発火しない。script.js は
  // observe() の後に buildNodes() を呼ぶので、その順序を守る。
  globalThis.ResizeObserver = class { constructor(cb) { this.cb = cb; } observe() { PENDING.push(this.cb); } };
  globalThis.EventSource = class { addEventListener() {} close() {} };

  new Function(fs.readFileSync(path.join(ROOT_DIR, "src/script.js"), "utf8"))();
  PENDING.forEach(cb => cb([]));
  drain();

  const geom = () => {
    const vb = els.chart.attrs.viewBox.split(" ").map(Number);
    const cs = [];
    (function w(e) { if (e.tag === "circle" && e.attrs.cx !== undefined) cs.push(e); e.children.forEach(w); })(els.nodes);
    const ys = cs.map(c => parseFloat(c.attrs.cy));
    const xs = cs.map(c => parseFloat(c.attrs.cx));
    // 点（3円で1点）ごとの座標にまとめてから最近接距離を測る
    const pts = [];
    for (let i = 0; i < cs.length; i += 3) pts.push({ x: xs[i], y: ys[i] });
    let near = Infinity;
    for (let i = 0; i < pts.length; i++)
      for (let j = i + 1; j < pts.length; j++)
        near = Math.min(near, Math.hypot(pts[i].x - pts[j].x, pts[i].y - pts[j].y));
    // 枝の形。放射状では直線（M…L…）、木では縦のS字（M…C…）であるべき
    const ds = els.edges.children
      .map(e => e.attrs.d).filter(d => d && d.length);
    const curved = ds.filter(d => d.includes("C")).length;
    const straight = ds.filter(d => d.includes("L")).length;
    return { vb, xs, ys, pts, near, edges: ds.length, curved, straight, circles: cs.length,
      rules: els.scale.children.filter(e => e.tag === "line" && e.attrs.class === "chart__rule").length };
  };

  const all = geom();
  // openHead を渡さないときは全体表示のまま返す（→項番56 は「開く」を含まない）
  if (!openHead) return { all };
  const head = els.nodes.children.find(g => g.attrs["data-id"] === openHead);
  head._ev.click[0]({ stopPropagation() {}, preventDefault() {} });
  drain();
  const open = geom();
  return { all, open, mode: els.stage.dataset.mode };
}

const fails = [];
function check(label, cond, detail) {
  console.log(`  ${cond ? "✅" : "❌"} ${label}${detail ? " — " + detail : ""}`);
  if (!cond) fails.push(label);
}

for (const [turns, branch] of [[3, false], [8, true], [19, false], [25, false]]) {
  console.log(`\n=== ${turns}ターン${branch ? "（枝分かれあり）" : ""} ===`);
  let r;
  try {
    r = run(chain(turns, branch), "d1");
    r.maxDepth = turns;
  } catch (e) {
    console.log(`  ❌ initMap が例外を投げた — ${e.message}`);
    fails.push(`${turns}ターンで例外: ${e.message}`);
    continue;
  }

  // 全体表示（放射）：会話は畳まれるので必ず収まる。縦横の両方を見る
  const a = r.all;
  const inX = Math.min(...a.xs) >= a.vb[0] - 0.5 && Math.max(...a.xs) <= a.vb[0] + a.vb[2] + 0.5;
  const inY = Math.min(...a.ys) >= a.vb[1] - 0.5 && Math.max(...a.ys) <= a.vb[1] + a.vb[3] + 0.5;
  check("全体表示で点が viewBox に収まる", inX && inY,
    `x ${Math.min(...a.xs).toFixed(0)}〜${Math.max(...a.xs).toFixed(0)} / y ${Math.min(...a.ys).toFixed(0)}〜${Math.max(...a.ys).toFixed(0)}`);
  // わたしは中心にいる（放射の意味そのもの）
  const cx = a.vb[0] + a.vb[2] / 2, cy = a.vb[1] + a.vb[3] / 2;
  check("わたしが viewBox の中心にいる", Math.hypot(0 - cx, 0 - cy) < 1,
    `中心 (${cx.toFixed(0)}, ${cy.toFixed(0)}) / 根 (0, 0)`);

  // 会話を開いた
  const o = r.open;
  check("会話が開いた（mode=conv）", r.mode === "conv", r.mode);
  check("目安線が段数と一致", o.rules === r.maxDepth, `${o.rules}本 / 段数 ${r.maxDepth}`);

  const gapPx = (DEPTH_GAP * VIEW.height) / o.vb[3];
  check("縦の間隔が下限を割らない", gapPx >= MIN_GAP_PX - 0.5, `${gapPx.toFixed(1)}px（下限 ${MIN_GAP_PX}px）`);

  const fits = Math.max(...o.ys) <= o.vb[1] + o.vb[3] + 0.5;
  const rootVisible = Math.min(...o.ys) >= o.vb[1] - 0.5;
  if (fits) {
    check("枝全体が収まっている", true, `y ${Math.min(...o.ys).toFixed(0)}〜${Math.max(...o.ys).toFixed(0)}`);
  } else {
    // 引きすぎない設計。入り切らないときは根が見えていてパンで辿れること
    check("入り切らないが根は見えている（上から読み下ろせる）", rootVisible,
      `根 y=${Math.min(...o.ys).toFixed(0)} / view 上端 ${o.vb[1].toFixed(0)}`);
    // 「引きすぎない」は「限界までは引く」でもある。手前で止まっていると、
    // 出せるはずの段数を出さずにパンを強いることになる。
    // （fitView の leaves に葉の数ではなくノード総数を渡すと、ここが手前で止まる）
    check("限界まで引けている", gapPx <= MIN_GAP_PX + 2,
      `${gapPx.toFixed(1)}px（下限 ${MIN_GAP_PX}px。手前で止まると出せる段数が減る）`);
  }
}

// ---------------------------------------------------------------------------
// 蓄積したとき（→mock/README「積み残し」）
//
// README は「会話が数百件になったときは畳んでも全体表示に入り切らなくなる」と
// 見積もっていたが、**実測では24本で入り切らなくなる**（1桁早い）。
// 入り切らないこと自体は破綻ではない——下限（58px）で止めてパンに渡すのが設計。
// ここで見るのは「潰れないこと」と「例外を出さずに描けること」。
// ---------------------------------------------------------------------------
for (const n of [11, 24, 50, 100, 400]) {
  console.log(`\n=== 会話 ${n}本（全体表示）===`);
  let r;
  try {
    r = run(many(n), "n1");
  } catch (e) {
    console.log(`  ❌ 例外 — ${e.message}`);
    fails.push(`会話${n}本で例外: ${e.message}`);
    continue;
  }
  const a = r.all;
  // 放射状では「横の間隔」に意味が無いので、**いちばん近い2点の距離**で測る
  const gap = (a.near * VIEW.width) / a.vb[2];
  check("点が潰れない（最も近い2点が下限以上）", gap >= MIN_GAP_PX - 0.5, `${gap.toFixed(1)}px`);
  check("会話の数だけ点がある", a.circles / 3 === n + 1, `${a.circles / 3} 個（会話${n}＋根）`);
  const okX = Math.min(...a.xs) >= a.vb[0] - 0.5 && Math.max(...a.xs) <= a.vb[0] + a.vb[2] + 0.5;
  const okY = Math.min(...a.ys) >= a.vb[1] - 0.5 && Math.max(...a.ys) <= a.vb[1] + a.vb[3] + 0.5;
  check("目安線を引いていない（深さが1段しかない）", a.rules === 0, `${a.rules}本`);
  // 放射状では子は根からの光線上にあるので、枝は直線が正しい。
  // 縦向きのS字を使うと、親より上にある子（約半数）で制御点が反転する。
  check("枝が直線で描かれている", a.curved === 0 && a.straight === n,
    `直線 ${a.straight} / 曲線 ${a.curved}（会話${n}本ぶん）`);
  // 入り切るかどうかは会話数しだい。入らないこと自体は破綻ではない
  // （下限で止めてパンに渡すのが設計）。ここでは事実だけ出す。
  console.log(`  ${okX && okY ? "全体が1画面に入る" : "入り切らない（パンで辿る）"}`
    + ` — x ${Math.min(...a.xs).toFixed(0)}〜${Math.max(...a.xs).toFixed(0)}`
    + ` / y ${Math.min(...a.ys).toFixed(0)}〜${Math.max(...a.ys).toFixed(0)}`);
  // ただし**わたしは必ず見えている**こと。中心を見失うと現在地が分からない
  check("入り切らなくても、わたしは画面内にいる",
    0 >= a.vb[0] - 0.5 && 0 <= a.vb[0] + a.vb[2] + 0.5 &&
    0 >= a.vb[1] - 0.5 && 0 <= a.vb[1] + a.vb[3] + 0.5, "根 (0, 0)");
}

// ---------------------------------------------------------------------------
// 項番56：window.SONAR を与えずに script.js を読み込む
//
// script.js は `(window.SONAR && window.SONAR.conversations) || [...]` の形で
// モックの固定データに落ちる。**モック単体で開いたときの自己説明性**が
// 生きているかの確認（→詳細設計書 §4-7）。ここが死ぬと、mock/map.html を
// そのまま開いた人に空の座標系だけが出る。
// ---------------------------------------------------------------------------
console.log("\n=== 項番56：window.SONAR を与えない（モック単体で開いたとき）===");
{
  let r = null, err = null;
  try {
    r = run(undefined, null);          // SONAR を渡さない＝window.SONAR は undefined
  } catch (e) {
    err = e;
  }
  const points = r ? r.all.circles / 3 : 0;
  check("組み込みの CONVERSATIONS / NODES だけで描ける", !err && points === 8,
    err ? `例外: ${err.message}` : `${points}個（組み込みの会話7＋根）`);
}

if (fails.length) { console.error("\n❌ " + fails.join(" / ")); process.exit(1); }
console.log("\n✅ すべて通った");
