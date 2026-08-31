"use strict";

/* =============================================================
   Sonar スクリプト
   -------------------------------------------------------------
   もとは mock/script.js（サーバー・DB・LLM に繋がっていない固定データ版）。
   2026-08-31 に本実装へ接続した。モックからの差分は3箇所だけ：
	 - QUESTIONS      → 削除。問いは Claude が作って SSE で届く
	 - typeOut()      → キュー＋一定速度の描画ループに分解（SSE 対応）
	 - CONVERSATIONS / NODES → サーバが window.SONAR に先出しする DB のデータ
	                     （|| 以降にモックの固定データをフォールバックとして残置）
   layout() の座標計算には触っていない（→スコープと縮退ライン §6）。
   ============================================================= */

/* -------------------------------------------------------------
   対話のシナリオ

   設計上の約束（企画書・スコープと縮退ラインより）：
	 1. AI の発話は「問い」だけ。相槌・共感・感想は入れない
	 2. 問いには直前の発話を織り込む（＝聞いていた証明。相槌の代わり）
	 3. 本人がまだ言っていないことを問いに混ぜない（問いの形をした断定の禁止）
	 4. 深さは「何回掘り下げたか」であって、決まった段数ではない。
	    2回で深いところに着くこともあれば、9回かけても届かないこともある。
	    画面に段数やカテゴリ名は出さない
   ------------------------------------------------------------- */
/* -------------------------------------------------------------
   入口で聞く「今の気分」

   これは追加の一手間ではなく、最初の問いをタップ1回に置き換えるもの。
   「どれくらい深く話したいか」は聞かない（深さの仕組みを入口で見せないため）。
   気分から、掘る積極性を内部で導出する。

   steer は本実装でプロンプトに渡す指示。画面には出さない。
   ------------------------------------------------------------- */
const MOODS = {
	chat: {
		label: "なんとなく話したい",
		opener: "最近、ちょっと気になったことって何かありますか。",
		steer: "掘り下げない。同じ深さで横に広げる"
	},
	listen: {
		label: "聞いてほしいことがある",
		opener: "何があったか、はじめから聞かせてもらえますか。",
		steer: "遮らない。話題に留まって続きを促す"
	},
	fog: {
		label: "もやもやしている",
		// 本人が選んだ言葉「もやもや」をそのまま問いに織り込んでいる
		opener: "そのもやもやは、何をきっかけに出てきましたか。",
		steer: "慎重に掘る。本人が引いたら即座に止める"
	},
	sort: {
		label: "考えを整理したい",
		opener: "整理したいのは、どのことについてですか。",
		steer: "積極的に掘ってよい"
	},
	none: {
		label: "選ばずに始めた",
		opener: "最近、印象に残っていることってありますか。どんな小さなことでも。",
		steer: "既定。様子を見ながら判断する"
	}
};
/* QUESTIONS（モックの固定シナリオ）はここにあったが、本実装では
   問いは Claude が作って SSE で届くので削除した。Vault の mock/ には
   凍結された原本が残っている（提出物②）。 */

/* -------------------------------------------------------------
   深さ → 色の強さ（0〜1）

   深さに上限は無い。2回で着くこともあれば9回かけても届かないこともある、
   と決めているので、固定の段数で色を切ると必ず嘘になる。

   t = 1 - 0.68^depth は 1 に漸近するだけで到達しない。
   つまり何段でも「前の段より必ず深い色」になり、頭打ちしない。

   ※ 色そのものはここに持たない。CSS の --d-shallow / --d-mid / --d-deep
     から作る（JSに色を書くと、ライト／ダークの切り替えに追従しなくなる）。
   ------------------------------------------------------------- */
function depthT(depth) {
	return depth === 0 ? 0 : 1 - Math.pow(0.68, depth);
}

/* =============================================================
   地図のデータ
   -------------------------------------------------------------
   会話ごとに独立した枝。**別々の会話のノードは合流させない**
   （2026-08-26 決定。→ ADR-0003 / スコープと縮退ライン §8）。

   合流させると根からの距離が経路によって変わり、縦軸＝深さの定義が
   壊れる。加えて「この2つの発話は同じ話題だ」はAIの判断であり、
   §6 で拒否したラベル付けより強い断定になる。

   結果としてこのデータは常に**木**であり、
	 - 深さ＝根からの距離が一意に決まる
	 - tidy tree がそのまま使える
	 - DBではノードの親が1つで済む
   ============================================================= */
const ROOT = "root";

/* -------------------------------------------------------------
   tidy tree の中核。葉に等間隔のスロットを割り当て、親は子の中央に置く。

   どの子を見せるかを childrenOf で差し替えられるようにしてあるので、
   **地図（会話を畳む）とホームのプレビュー（全部見せる）が同じ算術を共有する。**
   ここを2つ持つと、プレビューと地図で枝の並びがずれる。
   ------------------------------------------------------------- */
function tidyX(childrenOf) {
	let slot = 0;
	const x = {};
	const list = [];

	(function walk(id) {
		list.push(id);
		const ch = childrenOf(id);
		if (!ch.length) {
			x[id] = slot;
			slot += 1;
			return;
		}
		ch.forEach(walk);
		x[id] = (x[ch[0]] + x[ch[ch.length - 1]]) / 2;
	})(ROOT);

	return { x: x, list: list, slots: slot };
}

/* 会話の見出し。地図では畳んだり開いたりする単位になる。
   本実装ではサーバが window.SONAR に DB のデータを先出しする。
   || 以降はモックの固定データで、単体で開いたときのフォールバック
   （モック画面としての自己説明性を残すため）。 */
const CONVERSATIONS = (window.SONAR && window.SONAR.conversations) || [
	{ id: "cv1", head: "cv1_1", date: "2026年8月24日", mood: "もやもやしている" },
	{ id: "cv2", head: "cv2_1", date: "2026年8月20日", mood: "考えを整理したい" },
	{ id: "cv3", head: "cv3_1", date: "2026年8月18日", mood: "聞いてほしいことがある" },
	{ id: "cv4", head: "cv4_1", date: "2026年8月15日", mood: "もやもやしている" },
	{ id: "cv5", head: "cv5_1", date: "2026年8月12日", mood: "考えを整理したい" },
	{ id: "cv6", head: "cv6_1", date: "2026年8月8日", mood: "考えを整理したい" },
	{ id: "cv7", head: "cv7_1", date: "2026年8月25日", mood: "なんとなく話したい" }
];

/* 地図に置かれるのは本人が発した言葉だけ。AIの要約は入れない。
   quote＝本人の発話、question＝そのとき聞かれたこと。
   CONVERSATIONS と同じく window.SONAR が正、|| 以降はフォールバック。 */
const NODES = (window.SONAR && window.SONAR.nodes) || {
	root: { children: ["cv1_1", "cv2_1", "cv3_1", "cv4_1", "cv5_1", "cv6_1", "cv7_1"],
		quote: "すべての会話がここから枝分かれします。", question: "" },

	/* --- 会話1：4回目まで掘り下げた（枝によって長さが違うことの例） --- */
	cv1_1: { children: ["cv1_2", "cv1_3"], quote: "三年続けたバイトを先月辞めた",
		question: "そのもやもやは、何をきっかけに出てきましたか。" },
	cv1_2: { children: [], quote: "店長が良い人で、抜けたら回らないのが分かってた",
		question: "去年の冬から考えていて、先月まで続けた。その間、辞めずにいたのは何が理由でしたか。" },
	cv1_3: { children: ["cv1_4"], quote: "どっちも変わってない。自分が限界だっただけ",
		question: "店長のことと、店が回らないこと。最終的に辞める側に傾いたのは、どちらが変わったからですか。" },
	cv1_4: { children: ["cv1_5"], quote: "言えてない。言ったら負けだと思ってた",
		question: "「自分が限界だった」。その限界を、誰かに言えていましたか。" },
	cv1_5: { children: [], quote: "たぶん、誰にも借りを作りたくないんだと思う",
		question: "「言ったら負け」。その負けは、誰に対しての負けでしたか。" },

	/* --- 会話2：6回目まで届いた。固定段数で切れないことの証明 --- */
	cv2_1: { children: ["cv2_2"], quote: "進路のことがずっと決まらない",
		question: "整理したいのは、どのことについてですか。" },
	cv2_2: { children: ["cv2_3"], quote: "親にはまだ言ってない",
		question: "進路が決まらない。そのことは、誰かに話していますか。" },
	cv2_3: { children: ["cv2_4"], quote: "反対されるって、決めつけてる気がする",
		question: "親には言っていない。言わないでいるのは、何が起きると思っているからですか。" },
	cv2_4: { children: ["cv2_5"], quote: "前に一度、違うことで反対されたことがある",
		question: "「反対されると決めつけている」。そう思うようになった出来事はありましたか。" },
	cv2_5: { children: ["cv2_6"], quote: "そのとき、何も言い返せなかった",
		question: "一度反対されたことがある。そのとき、あなたはどうしましたか。" },
	cv2_6: { children: [], quote: "言い返さなかったのは、自分でも自信がなかったからだと思う",
		question: "何も言い返せなかった。言い返さなかったのは、何が理由だったと思いますか。" },

	/* --- 会話3：2回で終わった --- */
	cv3_1: { children: ["cv3_2"], quote: "友達と久しぶりに会った",
		question: "何があったか、はじめから聞かせてもらえますか。" },
	cv3_2: { children: [], quote: "変わってなかった。それが少し嬉しかった",
		question: "久しぶりに会った友達。会ってみて、どうでしたか。" },

	/* --- 会話4：同じ問いから2つ答えが出て枝分かれした --- */
	cv4_1: { children: ["cv4_2", "cv4_3"], quote: "朝起きるのがつらい",
		question: "そのもやもやは、何をきっかけに出てきましたか。" },
	cv4_2: { children: [], quote: "単純に寝るのが遅い",
		question: "朝起きるのがつらい。思い当たることはありますか。" },
	cv4_3: { children: ["cv4_4"], quote: "起きても、やることが決まってない",
		question: "ほかにも思い当たることはありますか。" },
	cv4_4: { children: [], quote: "決めると、できなかったときが嫌だから",
		question: "「やることが決まっていない」。決めないでいるのは、何が起きると思っているからですか。" },

	/* --- 会話5：3回目まで --- */
	cv5_1: { children: ["cv5_2"], quote: "春に引っ越しを決めた",
		question: "整理したいのは、どのことについてですか。" },
	cv5_2: { children: ["cv5_3"], quote: "一人になれる場所が欲しかった",
		question: "引っ越し先を選ぶとき、いちばん譲れなかったのは何でしたか。" },
	cv5_3: { children: [], quote: "誰かといると、自分が薄まる感じがする",
		question: "「一人になれる場所」。一人でないとき、何が起きていましたか。" },

	/* --- 会話6：2回目まで --- */
	cv6_1: { children: ["cv6_2"], quote: "貯金の使い道を決めたい",
		question: "整理したいのは、どのことについてですか。" },
	cv6_2: { children: [], quote: "使うより、残しておきたい気持ちのほうが強い",
		question: "貯金の使い道。決めきれずにいるのは、何が引っかかっているからですか。" },

	/* --- 会話7：ひとことだけ答えて閉じた。これも失敗ではない --- */
	cv7_1: { children: [], quote: "今日は特に何もなかった",
		question: "最近、ちょっと気になったことって何かありますか。" }
};

/* 根からの距離を1回だけ計算して各ノードに持たせる。
   木なので経路は1本しかなく、深さは一意に決まる。
   （合流させる設計にすると、ここが一意でなくなる） */
(function assignDepth() {
	const seen = new Set();
	(function walk(id, depth) {
		if (seen.has(id)) return; /* 木のはずだが、データ破損で無限再帰しないための保険 */
		seen.add(id);
		NODES[id].depth = depth;
		NODES[id].children.forEach(function (child) {
			if (NODES[child]) {
				NODES[child].parent = id;
				walk(child, depth + 1);
			}
		});
	})(ROOT, 0);
})();

/* どの会話に属するノードかを引けるようにしておく（日付・気分の表示に使う） */
CONVERSATIONS.forEach(function (cv) {
	(function walk(id) {
		NODES[id].conv = cv.id;
		NODES[id].children.forEach(walk);
	})(cv.head);
});

/* =============================================================
   対話画面
   ============================================================= */
function initTalk() {
	const talk = document.getElementById("talk");
	if (!talk) return;

	const pastEl = document.getElementById("past");
	const nowEl = document.getElementById("now");
	const questionEl = document.getElementById("question");
	const answerEl = document.getElementById("answer");
	const sendEl = document.getElementById("send");
	const stopEl = document.getElementById("stop");
	const composeEl = document.getElementById("compose");
	const logEl = document.getElementById("log");
	const wrapEl = document.getElementById("wrap");
	const wrapListEl = document.getElementById("wrapList");
	const moodEl = document.getElementById("mood");
	const failedEl = document.getElementById("failed");

	let typing = false;
	let mood = MOODS.none;
	let moodKey = "none";

	/* サーバとのやりとりに要る状態。
	   問いはサーバに預けず、次の回答と一緒に送り返す
	   （サーバは会話の途中状態を持たない。→設計書 システム構成 §3(a)）。 */
	let conversationId = null;
	let lastNodeId = null;
	let currentQuestion = "";

	/* 実際に答えられた分だけを保持する。
	   地図に置かれるのは本人が言った言葉だけなので、
	   途中でやめたら、その先のノードは生まれない。 */
	const placed = [];

	/* =========================================================
	   逐次表示
	   ---------------------------------------------------------
	   モックの typeOut(text, done) は**全文を受け取って**
	   questionEl.textContent = "" で始めていた。SSE では文字が
	   継ぎ足されるので、そのままデルタごとに呼ぶと**表示が毎回リセットされる**。

	   届いた文字はキューに積むだけにして、描画ループは到着と無関係に
	   一定速度で回す。Anthropic は短い出力を2〜3デルタにしか刻まないので、
	   受け取ったまま描くと「1文字 → 残り全部」でちらつく。

	   不変条件：**表示を消すのは beginTyping だけ。**

	   setInterval ではなく requestAnimationFrame なのは、滑らかなことに加えて
	   背面タブで止まるため。タブを戻した人は溜まった文字を1フレームで受け取り、
	   遅い再生を眺めずに済む。
	   ========================================================= */
	const CPS = 24; /* 字/秒。モックの 42ms/字 と同じ体感 */
	let queue = "";
	let closed = false;
	let raf = 0;
	let lastAt = 0;
	let caret = null;
	let onDone = null;
	let es = null; /* いま開いている EventSource */

	function beginTyping(done) {
		stopTyping();
		typing = true;
		queue = "";
		closed = false;
		onDone = done;
		questionEl.textContent = "";
		caret = document.createElement("span");
		caret.className = "caret";
		questionEl.appendChild(caret);
		lastAt = performance.now();
		raf = window.requestAnimationFrame(tick);
	}

	/* デルタが届いた。表示には触らない */
	function pushText(t) {
		queue += t;
	}

	/* もう文字は来ない */
	function endOfStream() {
		closed = true;
	}

	function tick(now) {
		const due = Math.floor(((now - lastAt) * CPS) / 1000);
		if (due > 0) {
			const take = queue.slice(0, due);
			if (take) {
				caret.insertAdjacentText("beforebegin", take);
				queue = queue.slice(take.length);
			}
			lastAt = now;
		}
		if (closed && queue === "") {
			finishTyping();
			return;
		}
		raf = window.requestAnimationFrame(tick);
	}

	function finishTyping() {
		teardown();
		if (onDone) {
			const d = onDone;
			onDone = null;
			d();
		}
	}

	/* 中断。EventSource.close() と対にする。
	   閉じるとサーバ側で reqwest のストリームも落ちるので、
	   誰も読まない出力にトークンを払い続けずに済む
	   （→設計書 システム構成 §3(d)）。 */
	function stopTyping() {
		teardown();
		onDone = null;
		if (es) {
			es.close();
			es = null;
		}
	}

	function teardown() {
		if (raf) window.cancelAnimationFrame(raf);
		raf = 0;
		typing = false;
		if (caret) {
			caret.remove();
			caret = null;
		}
	}

	function updateSendState() {
		sendEl.disabled = typing || answerEl.value.trim() === "";
	}

	/* 背景の深さ＝根からの距離（＝これまでに答えた回数）。
	   地図のノードと同じ depthT() を通すので、段も上限も無い。
	   数値もラベルも画面には出さない。 */
	function applyDepth() {
		talk.style.setProperty("--t", depthT(placed.length).toFixed(3));
	}

	/* =========================================================
	   問いを出す
	   ========================================================= */

	/* 1問目は LLM を呼ばない（気分ごとの固定文。→設計書 プロンプト §2-1）。
	   固定文も同じキューを通すので、SSE の問いと速度が揃う。 */
	function showFixed(text) {
		currentQuestion = text;
		applyDepth();
		sendEl.disabled = true;
		beginTyping(function () {
			updateSendState();
			answerEl.focus();
		});
		pushText(text);
		endOfStream();
	}

	/* 2問目以降。保存されたノードから根までを履歴にして問いを作らせる */
	function streamQuestion(nodeId) {
		currentQuestion = "";
		let got = 0;
		applyDepth();
		sendEl.disabled = true;
		beginTyping(function () {
			updateSendState();
			answerEl.focus();
		});

		es = new EventSource("/talk/question?node=" + nodeId);

		es.addEventListener("delta", function (e) {
			const t = JSON.parse(e.data);
			got += 1;
			currentQuestion += t;
			pushText(t);
		});

		/* 終端。これが無いと、正常終了した SSE でも EventSource が
		   onerror を撃って再接続を試み、「完了」と「失敗」を区別できない */
		es.addEventListener("done", function () {
			es.close();
			es = null;
			endOfStream();
		});

		es.addEventListener("failed", function () {
			es.close();
			es = null;
			showFailed(function () {
				streamQuestion(nodeId);
			});
		});

		es.onerror = function () {
			if (!es) return;
			es.close();
			es = null;
			if (got > 0) {
				/* 途中まで届いている。切れた所までを問いとして残す */
				endOfStream();
			} else {
				showFailed(function () {
					streamQuestion(nodeId);
				});
			}
		};
	}

	/* 問いが出せなかった（→設計書 画面遷移図 §5-1）。
	   「エラー」「失敗しました」とは書かない。ユーザーの発話は成功していて、
	   失敗したのはこちらの問いだけ。**すでに答えた分の点は失われない。** */
	let retryFn = null;

	function showFailed(fn) {
		stopTyping();
		retryFn = fn;
		nowEl.classList.add("hidden");
		composeEl.classList.add("hidden");
		failedEl.classList.remove("hidden");
		logEl.scrollTop = logEl.scrollHeight;
	}

	function hideFailed() {
		failedEl.classList.add("hidden");
		nowEl.classList.remove("hidden");
		composeEl.classList.remove("hidden");
	}

	/* 済んだやりとりを上へ送る。小さく薄くなり「沈んでいく」 */
	function pushToPast(question, answer) {
		const block = document.createElement("div");
		block.className = "past";

		const q = document.createElement("p");
		q.className = "past__q";
		q.textContent = question;

		const a = document.createElement("p");
		a.className = "past__a";
		a.textContent = answer;

		block.appendChild(q);
		block.appendChild(a);
		pastEl.appendChild(block);
	}

	/* 地図のラベルは本人の発話そのもの。長ければ末尾を省く。
	   AI に要約させない（要約を置くと、地図に AI の言葉が混ざる） */
	function toLabel(text) {
		return text.length > 16 ? text.slice(0, 16) + "…" : text;
	}

	/* =========================================================
	   回答を送る
	   ---------------------------------------------------------
	   保存が成功してから画面を進める。先に進めてしまうと、
	   保存されていないのに「点が置かれた」ように見える。
	   ========================================================= */
	async function submit() {
		const value = answerEl.value.trim();
		if (value === "" || typing) return;

		const asked = currentQuestion;
		sendEl.disabled = true;

		let saved;
		try {
			const r = await fetch("/talk/answer", {
				method: "POST",
				headers: { "content-type": "application/json" },
				body: JSON.stringify({
					conversation_id: conversationId,
					parent_id: lastNodeId,
					mood: moodKey,
					question: asked,
					answer: value
				})
			});
			if (!r.ok) throw new Error("HTTP " + r.status);
			saved = await r.json();
		} catch (e) {
			/* まだ保存されていない。入力はそのまま残して送り直せるようにする */
			showFailed(function () {
				updateSendState();
			});
			return;
		}

		conversationId = saved.conversation_id;
		lastNodeId = saved.node_id;

		/* ここから先で失敗しても、点は既に置かれている（→§5-1） */
		pushToPast(asked, value);
		placed.push({ depth: placed.length + 1, text: toLabel(value) });
		answerEl.value = "";
		answerEl.style.height = "auto";
		logEl.scrollTop = logEl.scrollHeight;

		streamQuestion(saved.node_id);
	}

	/* 会話を終える。
	   途中でやめても「未完了」扱いにはしない。深さは本人が決めるため。
	   「ここまでにする」では DB に何も書かない（→画面遷移図 §6）。 */
	function finish() {
		stopTyping();
		moodEl.classList.add("hidden");
		nowEl.classList.add("hidden");
		composeEl.classList.add("hidden");
		failedEl.classList.add("hidden");

		wrapListEl.innerHTML = "";
		placed.forEach(function (item) {
			const li = document.createElement("li");
			/* 深さは字下げと色で示す。どちらも上限を持たせない
			   （何段でも「前の段より深い」が成り立つ） */
			li.style.setProperty("--depth", String(item.depth));
			li.style.setProperty("--t", depthT(item.depth).toFixed(3));
			li.textContent = item.text;
			wrapListEl.appendChild(li);
		});

		/* ひとことも話さずに閉じた場合も「失敗」にしない */
		document.getElementById("wrapCount").textContent =
			placed.length === 0
				? "「" + mood.label + "」から始まった会話。今日は点が置かれませんでした。また話しにきてください。"
				: "「" + mood.label + "」から始まった会話。" +
				  placed.length + "つの点が置かれました。";

		wrapEl.classList.remove("hidden");
		logEl.scrollTop = logEl.scrollHeight;
	}

	/* 入力欄の高さを内容に合わせる */
	answerEl.addEventListener("input", function () {
		answerEl.style.height = "auto";
		answerEl.style.height = Math.min(answerEl.scrollHeight, 140) + "px";
		updateSendState();
	});

	/* Enterで送信、Shift+Enterで改行。
	   日本語入力が前提なので、変換確定のEnterを送信と誤認しないよう
	   isComposing に加えて keyCode 229（IME処理中）も見る。 */
	answerEl.addEventListener("keydown", function (e) {
		if (e.key === "Enter" && !e.shiftKey && !e.isComposing && e.keyCode !== 229) {
			e.preventDefault();
			submit();
		}
	});

	sendEl.addEventListener("click", submit);
	stopEl.addEventListener("click", finish);

	document.getElementById("retry").addEventListener("click", function () {
		hideFailed();
		const fn = retryFn;
		retryFn = null;
		if (fn) fn();
	});

	document.getElementById("stopFromFailed").addEventListener("click", finish);

	/* 気分が選ばれたら、それに応じた1問目から会話を始める。
	   気分はここではまだ DB に書かない。会話の行を作るのは
	   **最初の回答が送られたとき**（→画面遷移図 §6）——選んだだけで
	   離脱した人の分の空の会話が溜まると「話した回数」が実態とずれる。 */
	function startWith(key) {
		moodKey = MOODS[key] ? key : "none";
		mood = MOODS[moodKey];

		moodEl.classList.add("hidden");
		nowEl.classList.remove("hidden");
		composeEl.classList.remove("hidden");

		showFixed(mood.opener);
	}

	moodEl.querySelectorAll(".mood__chip").forEach(function (chip) {
		chip.addEventListener("click", function () {
			startWith(chip.dataset.mood);
		});
	});

	document.getElementById("skipMood").addEventListener("click", function () {
		startWith("none");
	});
}

/* =============================================================
   地図画面
   -------------------------------------------------------------
   会話が増えても全体表示が壊れないように、**会話単位で畳む**。

	 全体表示 … 各会話の1手目だけを置く。1画面の点の数は
	            「ノード総数」ではなく「会話数」で決まる
	 会話を押す … その会話の枝だけが開く。他は畳まれたまま残るので、
	            会話が並んでいること自体は見え続ける
	 点を押す  … その点にズームして、そのとき言った言葉を出す

   それでも会話数そのものは増え続けるので、
	 - 縮尺には下限を設ける（点が潰れるまで引かない）
	 - 収まらない分はドラッグで動かす（パン）
   の2つで受ける。地図アプリと同じ考え方。
   ============================================================= */
function initMap() {
	const stage = document.getElementById("stage");
	if (!stage) return;

	const svg = document.getElementById("chart");
	const gScale = document.getElementById("scale");
	const gEdge = document.getElementById("edges");
	const gNode = document.getElementById("nodes");
	const detail = document.getElementById("detail");
	const detailbar = document.getElementById("detailbar");
	const prevBtn = document.getElementById("prev");
	const nextBtn = document.getElementById("next");
	const resetBtn = document.getElementById("reset");
	const quoteEl = document.getElementById("panelQuote");
	const depthEl = document.getElementById("panelDepth");
	const dateEl = document.getElementById("panelDate");
	const questionEl = document.getElementById("panelQuestion");
	const hintEl = document.getElementById("hint");

	const NS = "http://www.w3.org/2000/svg";

	/* --- 座標系の定数（すべてSVGのユーザー単位） --- */
	const LEAF_GAP = 96; /* 葉と葉の横の間隔 */
	const DEPTH_GAP = 130; /* 1段掘り下げるごとの縦の距離 */
	const TOP = 48; /* 根のY */
	const PAD_X = 90;
	const PAD_Y = 70;

	/* 縮尺の下限。これ以上は引かない。
	   引き切って全部を1画面に入れると、点の間隔が指の幅を下回って
	   隣を押してしまう。入り切らない分はパンで見る。 */
	const MIN_GAP_PX = 58;

	/* ノードを開いたときの倍率と、その点を画面のどこに置くか */
	const ZOOM = 0.62;
	const NODE_Y = 0.5;

	const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

	let view = { x: 0, y: 0, w: 1000, h: 700 };
	let base = null; /* 全体表示のビュー */
	let world = null; /* いま描かれている木の外接矩形 */
	let raf = 0;
	let moveRaf = 0;
	let selected = null; /* 開いている点 */
	let openConv = null; /* 開いている会話 */
	let pos = {}; /* いま画面に描かれている座標。畳む／開くのアニメに使う */
	let order = []; /* 開いている会話の中を辿る順序（深さ優先） */
	const el = {}; /* id → SVG要素 */

	/* -----------------------------------------------------------
	   tidy tree
	   葉を左から順に並べ、親は子の中点に置く。
	   これだけで「同じ深さのノードが重ならない」ことが保証される
	   （各ノードのXは自分の部分木の葉の範囲に必ず収まり、
	     兄弟の部分木どうしは葉の範囲が重ならないため）。

	   本実装で描画ライブラリを入れても、この関数はそのまま使える。
	   ----------------------------------------------------------- */
	function visibleChildren(id) {
		const node = NODES[id];
		if (id === ROOT) return node.children;
		/* 開いていない会話は、1手目から先を出さない（＝畳む） */
		return node.conv === openConv ? node.children : [];
	}

	function layout() {
		/* 座標の算術はトップレベルの tidyX が持つ（ホームのプレビューと共有）。
		   ここが渡す visibleChildren が「畳んだ会話は子を出さない」を表す。 */
		const placed = tidyX(visibleChildren);
		const x = placed.x;
		const list = placed.list;

		const span = Math.max(0, placed.slots - 1) * LEAF_GAP;
		const target = {};
		const byDepth = {};
		let maxDepth = 0;

		list.forEach(function (id) {
			const d = NODES[id].depth;
			target[id] = {
				x: x[id] * LEAF_GAP - span / 2,
				y: TOP + d * DEPTH_GAP
			};
			(byDepth[d] = byDepth[d] || []).push({
				x: target[id].x,
				labeled: !openConv || id === ROOT || NODES[id].conv === openConv
			});
			if (d > maxDepth) maxDepth = d;
		});

		/* ラベルを出す予定の点だけで、同じ深さの最小間隔を測る。
		   横幅が足りているかの判定に使う。

		   **測る対象を「出す予定のもの」に限るのが要点。**
		   会話を開いているときは畳んだ会話にラベルを出さないので、
		   それらの間隔で判定すると、出す気のない点のせいで
		   読める会話の言葉まで消えてしまう。 */
		let minGap = Infinity;
		Object.keys(byDepth).forEach(function (d) {
			const a = byDepth[d]
				.filter(function (p) { return p.labeled; })
				.map(function (p) { return p.x; })
				.sort(function (p, q) { return p - q; });
			for (let i = 1; i < a.length; i++) {
				minGap = Math.min(minGap, a[i] - a[i - 1]);
			}
		});

		return {
			target: target,
			visible: list,
			leaves: placed.slots,
			maxDepth: maxDepth,
			minGap: minGap,
			box: {
				x: -span / 2 - PAD_X,
				y: TOP - PAD_Y,
				w: span + PAD_X * 2,
				h: maxDepth * DEPTH_GAP + PAD_Y * 2
			}
		};
	}

	/* -----------------------------------------------------------
	   描画

	   ノードの要素は最初に全部作っておき、畳む／開くでは
	   座標と表示だけを動かす。畳んだ枝は親の位置へ吸い込まれて消える。
	   ----------------------------------------------------------- */
	function buildNodes() {
		Object.keys(NODES).forEach(function (id) {
			const node = NODES[id];

			const g = document.createElementNS(NS, "g");
			g.setAttribute("class", "node");
			g.setAttribute("data-id", id);
			g.setAttribute("data-depth", String(node.depth));
			g.setAttribute("role", "button");
			g.setAttribute("tabindex", "0");
			g.setAttribute("aria-label", id === ROOT ? "わたし" : node.quote);
			g.style.setProperty("--t", depthT(node.depth).toFixed(3));

			/* 見た目を変えずに当たり判定だけ広げる。
			   半径は CSS 側で画面px基準に計算する（--s を使う） */
			const hit = document.createElementNS(NS, "circle");
			hit.setAttribute("class", "node__hit");

			const halo = document.createElementNS(NS, "circle");
			halo.setAttribute("class", "node__halo");

			const dot = document.createElementNS(NS, "circle");
			dot.setAttribute("class", "node__dot");

			const label = document.createElementNS(NS, "text");
			label.setAttribute("class", "chart__label");
			label.textContent =
				id === ROOT ? "わたし" : shorten(node.quote);

			g.appendChild(hit);
			g.appendChild(halo);
			g.appendChild(dot);
			g.appendChild(label);
			gNode.appendChild(g);

			const edge = document.createElementNS(NS, "path");
			edge.setAttribute("class", "edge");
			gEdge.appendChild(edge);

			el[id] = { g: g, circles: [hit, halo, dot], label: label, edge: edge };

			g.addEventListener("click", function (e) {
				e.stopPropagation();
				activate(id);
			});
			g.addEventListener("keydown", function (e) {
				if (e.key === "Enter" || e.key === " ") {
					e.preventDefault();
					activate(id);
				}
			});
		});
	}

	function shorten(text) {
		return text.length > 14 ? text.slice(0, 14) + "…" : text;
	}

	/* 深さの目盛り。段数は固定しない。いま出ている最大の深さまで引く。
	   カテゴリ名は付けない（本人の発話を分類することになるため）。 */
	function drawScale(maxDepth, box) {
		gScale.textContent = "";
		for (let d = 1; d <= maxDepth; d += 1) {
			const line = document.createElementNS(NS, "line");
			line.setAttribute("class", "chart__rule");
			line.setAttribute("x1", box.x + 34);
			line.setAttribute("x2", box.x + box.w);
			line.setAttribute("y1", TOP + d * DEPTH_GAP);
			line.setAttribute("y2", TOP + d * DEPTH_GAP);
			gScale.appendChild(line);
		}
		const axis = document.createElementNS(NS, "line");
		axis.setAttribute("class", "chart__axis");
		axis.setAttribute("x1", box.x + 24);
		axis.setAttribute("x2", box.x + 24);
		axis.setAttribute("y1", TOP + 20);
		axis.setAttribute("y2", TOP + maxDepth * DEPTH_GAP + 20);
		gScale.appendChild(axis);

		[["浅い", TOP + 4], ["深い", TOP + maxDepth * DEPTH_GAP + 40]].forEach(function (p) {
			const t = document.createElementNS(NS, "text");
			t.setAttribute("class", "chart__band");
			t.setAttribute("x", box.x + 24);
			t.setAttribute("y", p[1]);
			t.setAttribute("text-anchor", "middle");
			t.textContent = p[0];
			gScale.appendChild(t);
		});
	}

	function place(id, p) {
		const e = el[id];
		e.circles.forEach(function (c) {
			c.setAttribute("cx", p.x.toFixed(1));
			c.setAttribute("cy", p.y.toFixed(1));
		});
		e.label.setAttribute("x", p.x.toFixed(1));
		e.label.setAttribute("y", (p.y - 22).toFixed(1));

		const parent = NODES[id].parent;
		if (!parent || !pos[parent]) {
			e.edge.setAttribute("d", "");
			return;
		}
		const q = pos[parent];
		const dy = (p.y - q.y) * 0.45;
		e.edge.setAttribute(
			"d",
			"M" + q.x.toFixed(1) + "," + q.y.toFixed(1) +
				" C" + q.x.toFixed(1) + "," + (q.y + dy).toFixed(1) +
				" " + p.x.toFixed(1) + "," + (p.y - dy).toFixed(1) +
				" " + p.x.toFixed(1) + "," + p.y.toFixed(1)
		);
	}

	/* 畳む／開くで座標が変わったぶんを補間して動かす */
	function applyLayout(L, ms) {
		cancelAnimationFrame(moveRaf);

		minGapWorld = L.minGap;
		const visible = new Set(L.visible);
		Object.keys(el).forEach(function (id) {
			el[id].g.classList.toggle("is-hidden", !visible.has(id));
			el[id].edge.classList.toggle("is-hidden", !visible.has(id));
			/* いま開いている会話に属する点。ラベルはここにだけ出す */
			el[id].g.classList.toggle(
				"is-open",
				!!openConv && (id === ROOT || NODES[id].conv === openConv)
			);
		});

		/* 新しく出てくる枝は、親のいた場所から生える */
		const from = {};
		L.visible.forEach(function (id) {
			from[id] = pos[id] || pos[NODES[id].parent] || L.target[id];
		});
		/* 消える枝は親へ吸い込む */
		Object.keys(pos).forEach(function (id) {
			if (!visible.has(id)) delete pos[id];
		});

		const start = performance.now();
		const run = function (now) {
			const t = ms > 0 ? Math.min(1, (now - start) / ms) : 1;
			const k = reduceMotion ? 1 : ease(t);
			L.visible.forEach(function (id) {
				pos[id] = {
					x: from[id].x + (L.target[id].x - from[id].x) * k,
					y: from[id].y + (L.target[id].y - from[id].y) * k
				};
			});
			/* 親から順に置かないと、枝が1フレーム前の親を見てしまう */
			L.visible.forEach(function (id) {
				place(id, pos[id]);
			});
			if (t < 1 && !reduceMotion) moveRaf = requestAnimationFrame(run);
		};
		run(performance.now());
	}

	/* -----------------------------------------------------------
	   ビュー（viewBox）
	   ----------------------------------------------------------- */
	/* ラベルを出すのに要る、点1つあたりの横幅（画面px）。
	   これを下回るとラベルどうしが重なって読めない塊になる。

	   ※ 押せるかどうかの下限（MIN_GAP_PX = 58）とは別の値。
	     押せる < 読める なので、その間の倍率では
	     「点は押せるがラベルは出ない（＝地形図）」状態になる。 */
	const LABEL_MIN_PX = 80;
	let minGapWorld = Infinity;

	function setView(v) {
		view = v;
		svg.setAttribute("viewBox", v.x + " " + v.y + " " + v.w + " " + v.h);

		/* 1画面pxが何ユーザー単位にあたるか。
		   点の大きさ・当たり判定・文字は、これを使って
		   **画面上の大きさが一定になる**ように CSS 側で計算する。
		   ここを世界座標のままにすると、引くほど文字が小さくなって読めなくなる。 */
		const r = svg.getBoundingClientRect();
		const s = v.w / (r.width || 1);
		stage.style.setProperty("--s", s.toFixed(4));

		/* ラベルを出すかどうか（意味的ズーム）。
		   判定は「1点あたり画面上に何pxあるか」。
		   点の数でも倍率でもなく、**実際に横幅が足りているか**で決める。
		   守りたい原則は「出す量を、読める量に収める」なので、
		   狭い画面では点が少なくてもラベルは出さない。

		   ※ 効くのは**会話を開いているとき**だけになった（2026-08-30）。
		     全体表示では幅によらずラベルを出さない（→ style.css）。 */
		stage.dataset.detail = minGapWorld / s >= LABEL_MIN_PX ? "high" : "low";
	}

	/* 木の全体が入るビュー。ただし引きすぎない。
	   MIN_GAP_PX を下回るところまで引くと点が潰れて押せなくなるので、
	   そこで止めて、残りはパンで見てもらう。 */
	function fitView(box, leaves) {
		const r = svg.getBoundingClientRect();
		const pw = r.width || 1;
		const ph = r.height || 1;
		const aspect = pw / ph;

		let w = box.w;
		let h = w / aspect;
		if (h < box.h) {
			h = box.h;
			w = h * aspect;
		}

		const limit = leaves > 1 ? (LEAF_GAP * pw) / MIN_GAP_PX : Infinity;
		if (w > limit) {
			w = limit;
			h = w / aspect;
		}

		/* 縦にも同じ下限をかける。
		   横（LEAF_GAP）だけを見ていると、**直線的に深い会話**——葉が1つなので
		   横の制限がそもそも効かない——で点が縦に潰れる。
		   19ターンの会話で縦の間隔が36pxまで詰まるのを実測した（下限は58px）。
		   ここで止めて、残りはパンで見てもらうのは横と同じ考え方。 */
		const hasDepth = box.h > PAD_Y * 2 + 1;
		const limitH = hasDepth ? (DEPTH_GAP * ph) / MIN_GAP_PX : Infinity;
		if (h > limitH) {
			h = limitH;
			w = h * aspect;
		}

		/* 縦が余ったら、上に寄せる。
		   中央に置くと木の上下に均等な余白ができるが、
		   この地図は「上が浅瀬・下が深海」なので、
		   余った水は**下**にあるべき（＝まだ潜っていない深さ）。 */
		/* 縦が余ったら上に寄せる。中央に置くと木の上下に均等な余白ができるが、
		   この地図は「上が浅瀬・下が深海」なので、余った水は**下**にあるべき
		   （＝まだ潜っていない深さ）。

		   入り切らないときも**上に合わせる。** 中央に置くと根も末端も画面から出て、
		   いま会話のどこを見ているのか分からなくなる。上から読み下ろせるようにする。 */
		const slack = h - box.h;
		const top = slack > 0 ? box.y - Math.min(slack * 0.18, 60) : box.y;

		return {
			x: box.x + box.w / 2 - w / 2,
			y: top,
			w: w,
			h: h
		};
	}

	function ease(t) {
		return t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2;
	}

	function animateTo(target, ms) {
		cancelAnimationFrame(raf);

		if (reduceMotion || ms <= 0) {
			setView(target);
			return;
		}

		const from = { x: view.x, y: view.y, w: view.w, h: view.h };
		const start = performance.now();

		function step(now) {
			const t = Math.min(1, (now - start) / ms);
			const e = ease(t);
			setView({
				x: from.x + (target.x - from.x) * e,
				y: from.y + (target.y - from.y) * e,
				w: from.w + (target.w - from.w) * e,
				h: from.h + (target.h - from.h) * e
			});
			if (t < 1) raf = requestAnimationFrame(step);
		}

		raf = requestAnimationFrame(step);
	}

	function zoomToNode(id, ms) {
		const w = base.w * ZOOM;
		const h = base.h * ZOOM;
		const p = pos[id] || { x: 0, y: TOP };
		animateTo({ x: p.x - w / 2, y: p.y - h * NODE_Y, w: w, h: h }, ms);
	}

	/* -----------------------------------------------------------
	   操作
	   ----------------------------------------------------------- */

	/* 押されたものが「畳まれている会話の1手目」なら、その会話を開く。
	   すでに開いている枝の中なら、その点の内容を出す。 */
	function activate(id) {
		const node = NODES[id];

		if (id !== ROOT && node.conv !== openConv) {
			expand(node.conv);
			/* ひとことだけで終わった会話は、開いても点が増えない。
			   その場合は開くだけで終わらせず、そのまま中身を出す。 */
			if (!node.children.length) select(id);
			return;
		}

		select(id);
	}

	function expand(convId) {
		openConv = convId;
		deselect();

		const L = layout();
		world = L.box;
		drawScale(L.maxDepth, L.box);
		applyLayout(L, reduceMotion ? 0 : 520);
		base = fitView(L.box, L.leaves);

		/* 開いた会話が画面に収まるところまで寄る。
		   根も範囲に入れる。枝がどこから生えているかが見えないと、
		   いま地図のどこを見ているのか分からなくなる。 */
		const cv = CONVERSATIONS.find(function (c) { return c.id === convId; });
		order = orderOf(cv.head);
		const xs = order.map(function (i) { return L.target[i].x; }).concat(L.target[ROOT].x);
		const ys = order.map(function (i) { return L.target[i].y; });
		const box = {
			x: Math.min.apply(null, xs) - PAD_X,
			y: TOP - PAD_Y,
			w: Math.max(1, Math.max.apply(null, xs) - Math.min.apply(null, xs)) + PAD_X * 2,
			h: Math.max.apply(null, ys) - TOP + PAD_Y * 2
		};
		/* fitView の leaves は「**横に並ぶ葉の数**」。横方向の下限（MIN_GAP_PX）を
		   守るための値なので、会話のノード総数（order.length）を渡してはいけない。
		   直線的に深い会話は葉が1つしかないのに、order.length を渡すと
		   「葉がN個ある」と誤って横幅が制限され、**その制限が縦を切って
		   根も末端も画面から出る**（14ターンで実測）。
		   モックの会話は最長6段で症状が出なかったため、初期コミットから残っていた。 */
		const leaves = order.filter(function (id) {
			return !NODES[id].children.length;
		}).length;
		animateTo(fitView(box, leaves), reduceMotion ? 0 : 620);

		stage.dataset.mode = "conv";
		resetBtn.hidden = false;
		hintEl.textContent = "点を押すと読めます";
	}

	function collapse() {
		openConv = null;
		order = [];
		deselect();

		const L = layout();
		world = L.box;
		drawScale(L.maxDepth, L.box);
		applyLayout(L, reduceMotion ? 0 : 520);
		base = fitView(L.box, L.leaves);
		animateTo(base, reduceMotion ? 0 : 520);

		stage.dataset.mode = "all";
		resetBtn.hidden = true;
		/* 全体俯瞰では案内文を出さない（2026-08-30）。
		   押せることは点の大きさで示す（→ style.css の data-mode="all"）。
		   会話を開いたあとは点が増えて1つずつが小さく見えるので、
		   そちらには案内を残している。 */
		hintEl.textContent = "";
	}

	/* 開いている会話の中を辿る順序。深さ優先。
	   連鎖している間は 親 → 子 を辿るので、
	   そのときの会話を順に読み直すのと同じ動きになる。 */
	function orderOf(head) {
		const out = [];
		(function walk(id) {
			out.push(id);
			NODES[id].children.forEach(walk);
		})(head);
		return out;
	}

	function select(id) {
		const node = NODES[id];
		selected = id;

		Object.keys(el).forEach(function (k) {
			el[k].g.setAttribute("aria-pressed", k === id ? "true" : "false");
		});

		detail.style.setProperty("--t", depthT(node.depth).toFixed(3));
		quoteEl.textContent = node.quote;

		/* カテゴリ名は出さない。事実として「何回掘り下げたところか」だけ書く */
		depthEl.textContent =
			node.depth === 0 ? "" : "この話題を " + node.depth + " 回掘り下げたところ";

		const cv = CONVERSATIONS.find(function (c) { return c.id === node.conv; });
		dateEl.textContent = cv ? cv.date + "の会話（" + cv.mood + "）" : "";

		questionEl.textContent = node.question;
		/* 根ノードには「聞かれたこと」が無いので枠ごと隠す */
		questionEl.parentElement.style.display = node.question ? "" : "none";

		/* 前後は「開いている会話の中」だけを辿る。
		   会話をまたいで進むと、いま誰の話を読んでいるのか分からなくなる。 */
		const i = order.indexOf(id);
		prevBtn.disabled = i <= 0;
		nextBtn.disabled = i < 0 || i >= order.length - 1;
		prevBtn.title = i > 0 ? NODES[order[i - 1]].quote : "";
		nextBtn.title = i >= 0 && i < order.length - 1 ? NODES[order[i + 1]].quote : "";

		stage.classList.add("is-zoomed");
		resetBtn.hidden = false;
		zoomToNode(id, reduceMotion ? 0 : 620);

		/* ズームが動き出してからカードを出す。同時だと視線が散る。
		   すでに開いているとき（前後移動）は出し直さない */
		if (detailbar.classList.contains("is-open")) return;
		window.setTimeout(function () {
			if (selected === id) detailbar.classList.add("is-open");
		}, reduceMotion ? 0 : 280);
	}

	function deselect() {
		selected = null;
		Object.keys(el).forEach(function (k) {
			el[k].g.setAttribute("aria-pressed", "false");
		});
		detailbar.classList.remove("is-open");
		stage.classList.remove("is-zoomed");
	}

	/* 戻るのは一段ずつ。点を見ている → 会話全体 → 全部畳む。
	   一気に戻すと、いま地図のどこにいたかが分からなくなる。 */
	function back() {
		if (selected) {
			deselect();
			animateTo(base, reduceMotion ? 0 : 520);
			return;
		}
		if (openConv) collapse();
	}

	function step(delta) {
		if (!selected) return;
		const i = order.indexOf(selected);
		const target = order[i + delta];
		if (target) select(target);
	}

	/* -----------------------------------------------------------
	   パン（ドラッグで動かす）
	   会話が増えると縮尺の下限で全部は入らなくなる。入り切らない分は動かして見る。
	   狭い画面で地図が小さくしか出ない問題も、これで実用になる。
	   ----------------------------------------------------------- */
	let drag = null;
	let suppressClick = false;

	svg.addEventListener("pointerdown", function (e) {
		if (e.button !== 0 && e.pointerType === "mouse") return;
		suppressClick = false;
		drag = { x: e.clientX, y: e.clientY, id: e.pointerId, moved: false };
		/* ここで setPointerCapture してはいけない。
		   捕捉した瞬間から pointerup も click も svg 自身に届くようになり、
		   ノードの click ハンドラが二度と呼ばれなくなる（＝点が押せなくなる）。
		   捕捉は「実際に動かし始めた」時点まで遅らせる。 */
	});

	svg.addEventListener("pointermove", function (e) {
		if (!drag || e.pointerId !== drag.id) return;
		const dx = e.clientX - drag.x;
		const dy = e.clientY - drag.y;

		/* 押しただけの微動でパンを始めない。
		   4px を超えて初めてドラッグとみなし、そこで捕捉する
		   （枠の外へ出ても動かし続けられるように）。 */
		if (!drag.moved) {
			if (Math.abs(dx) + Math.abs(dy) < 4) return;
			drag.moved = true;
			stage.classList.add("is-panning");
			try {
				svg.setPointerCapture(e.pointerId);
			} catch (err) {
				/* 捕捉できなくてもパン自体は動く */
			}
		}

		const r = svg.getBoundingClientRect();
		const k = view.w / (r.width || 1);
		drag.x = e.clientX;
		drag.y = e.clientY;
		cancelAnimationFrame(raf);
		setView({ x: view.x - dx * k, y: view.y - dy * k, w: view.w, h: view.h });
	});

	function endDrag(e) {
		if (!drag || e.pointerId !== drag.id) return;
		/* 動かしただけのときは、直後の click を「背景を押した」と解釈しない */
		suppressClick = drag.moved;
		drag = null;
		stage.classList.remove("is-panning");
		try {
			svg.releasePointerCapture(e.pointerId);
		} catch (err) {}
	}

	svg.addEventListener("pointerup", endDrag);
	svg.addEventListener("pointercancel", endDrag);

	/* ホイールで拡大縮小。カーソルの位置を動かさないように寄る */
	svg.addEventListener("wheel", function (e) {
		e.preventDefault();
		cancelAnimationFrame(raf);
		const r = svg.getBoundingClientRect();
		const k = Math.exp(e.deltaY * 0.0012);
		const nw = Math.min(base ? base.w * 3 : view.w * 3, Math.max(120, view.w * k));
		const scale = nw / view.w;
		const px = (e.clientX - r.left) / (r.width || 1);
		const py = (e.clientY - r.top) / (r.height || 1);
		setView({
			x: view.x + view.w * px * (1 - scale),
			y: view.y + view.h * py * (1 - scale),
			w: nw,
			h: view.h * scale
		});
	}, { passive: false });

	/* 背景を押したら一段戻る（ドラッグの直後は無視する）。
	   受けるのは背景の rect ではなく svg 自身にしている。
	   ドラッグでポインタを捕捉したあとは click が svg に届くので、
	   rect 側で受けていると取りこぼす。 */
	svg.addEventListener("click", function (e) {
		if (suppressClick) {
			suppressClick = false;
			return;
		}
		if (e.target.closest && e.target.closest(".node")) return;
		back();
	});

	resetBtn.addEventListener("click", collapse);
	prevBtn.addEventListener("click", function () { step(-1); });
	nextBtn.addEventListener("click", function () { step(1); });

	document.addEventListener("keydown", function (e) {
		if (e.key === "Escape") back();
		if (!selected) return;
		if (e.key === "ArrowLeft") { e.preventDefault(); step(-1); }
		if (e.key === "ArrowRight") { e.preventDefault(); step(1); }
	});

	/* 表示領域が変わったら測り直す。
	   読み込み直後にまだ大きさが確定していない場合もここで拾えるので、
	   resize イベントではなく ResizeObserver を使う。 */
	let first = true;
	new ResizeObserver(function () {
		const L = layout();
		world = L.box;
		base = fitView(L.box, L.leaves);
		if (first) {
			first = false;
			drawScale(L.maxDepth, L.box);
			applyLayout(L, 0);
			setView(base);
		} else if (selected) {
			zoomToNode(selected, 0);
		} else {
			setView(base);
		}
	}).observe(stage);

	buildNodes();
	stage.dataset.mode = "all";
	hintEl.textContent = "";
}

/* =============================================================
   表示モードの切り替え
   -------------------------------------------------------------
   画面ごとに明暗を強制しない。本人の選択を localStorage に残し、
   未選択なら端末設定（prefers-color-scheme）に従う。

   どちらであるかの解決は各HTMLの <head> のスクリプトが済ませていて、
   data-theme には必ず light か dark のどちらかが入っている。
   ここではその値を読み書きするだけ。
   ============================================================= */
function initTheme() {
	const root = document.documentElement;
	const media = window.matchMedia("(prefers-color-scheme: dark)");

	function chosen() {
		try {
			const v = localStorage.getItem("sonar-theme");
			return v === "light" || v === "dark" ? v : null;
		} catch (e) {
			return null;
		}
	}

	/* 本人がまだ選んでいない間だけ、端末設定の変更に追従する。
	   一度選んだあとに勝手に変わると、選んだ意味がなくなる。 */
	media.addEventListener("change", function (e) {
		if (chosen()) return;
		root.dataset.theme = e.matches ? "dark" : "light";
	});

	const btn = document.getElementById("theme");
	if (!btn) return;

	btn.addEventListener("click", function () {
		const next = root.dataset.theme === "dark" ? "light" : "dark";
		root.dataset.theme = next;
		try {
			localStorage.setItem("sonar-theme", next);
		} catch (e) {
			/* プライベートモード等で保存できなくても、その場の切り替えは効かせる */
		}
	});
}

/* =============================================================
   ホームの地図プレビュー
   -------------------------------------------------------------
   「起動時に、積み上がっているものが見える」ためのもの。
   モックでは手置き座標の固定SVGだったが、実データと食い違うので
   **地図と同じ NODES から、同じ tidyX で組み立てる**ようにした。

   地図本体との違いは2つだけ：
	 - 畳まない（全会話の全ノードを出す）。プレビューは俯瞰が役目なので
	 - 縦は段数に合わせて詰める（深い会話があっても枠から出ない）

   ラベルは出さない。全体表示でラベルを出さないのと同じ理由
   （→ mock/README 設計の意図11）。押すと地図が開く。
   ============================================================= */
function initPreview() {
	const svg = document.getElementById("pvChart");
	if (!svg) return;

	const NS = "http://www.w3.org/2000/svg";
	const gRules = document.getElementById("pvRules");
	const gEdges = document.getElementById("pvEdges");
	const gNodes = document.getElementById("pvNodes");

	const W = 640;
	const H = 220;
	const PAD = 26; /* 左右の余白。点が枠に触れないように */
	const TOP = 18; /* 根のY。モックと同じ */
	const ROW_MAX = 31; /* 1段あたりの縦の距離。モックと同じ */

	/* 畳まない。地図を「全部開いた」状態と同じ並びになる */
	const placed = tidyX(function (id) {
		return NODES[id].children;
	});

	let maxDepth = 0;
	placed.list.forEach(function (id) {
		if (NODES[id].depth > maxDepth) maxDepth = NODES[id].depth;
	});

	/* 段が増えても枠から出さない。浅いうちはモックと同じ間隔のまま */
	const row = maxDepth > 0 ? Math.min(ROW_MAX, (H - TOP - 16) / maxDepth) : ROW_MAX;
	const span = Math.max(1, placed.slots - 1);
	const step = (W - PAD * 2) / span;
	/* 葉が1つだけのときは中央に置く（step が効かないため） */
	const originX = placed.slots <= 1 ? W / 2 - 0 * step : PAD;

	function px(id) {
		return placed.slots <= 1 ? W / 2 : originX + placed.x[id] * step;
	}
	function py(id) {
		return TOP + NODES[id].depth * row;
	}

	/* 深さの目安線。カテゴリ名は付けない。段数も固定しない */
	for (let d = 1; d <= maxDepth; d += 1) {
		const line = document.createElementNS(NS, "line");
		line.setAttribute("x1", "0");
		line.setAttribute("x2", String(W));
		line.setAttribute("y1", String(TOP + d * row));
		line.setAttribute("y2", String(TOP + d * row));
		line.setAttribute("class", "pv-rule");
		line.setAttribute("stroke-dasharray", "3 6");
		gRules.appendChild(line);
	}

	/* 枝。地図本体と同じ縦向きのベジェ */
	const k = row * 0.45;
	placed.list.forEach(function (id) {
		const parent = NODES[id].parent;
		if (!parent) return;
		const x1 = px(parent);
		const y1 = py(parent);
		const x2 = px(id);
		const y2 = py(id);
		const path = document.createElementNS(NS, "path");
		path.setAttribute(
			"d",
			"M" + x1 + " " + y1 +
				" C " + x1 + " " + (y1 + k) + ", " + x2 + " " + (y2 - k) + ", " + x2 + " " + y2
		);
		gEdges.appendChild(path);
	});

	/* 点。色は地図と同じランプ（--t）。根だけは深さの外にある */
	placed.list.forEach(function (id) {
		const depth = NODES[id].depth;
		const c = document.createElementNS(NS, "circle");
		c.setAttribute("cx", String(px(id)));
		c.setAttribute("cy", String(py(id)));
		c.setAttribute("r", id === ROOT ? "6" : "5");
		c.setAttribute("class", "pv");
		if (id === ROOT) {
			c.style.setProperty("--c", "var(--d-root)");
		} else {
			c.style.setProperty("--t", depthT(depth).toFixed(3));
		}
		gNodes.appendChild(c);
	});
}

initTheme();
initTalk();
initMap();
initPreview();
