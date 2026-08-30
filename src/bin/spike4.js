/* スパイク4のブラウザ側。SSE のデルタを連結してそのまま出す。
   加工しないこと自体が §12-1 の確認になる。 */
(function () {
	const out = document.getElementById("out");
	const stat = document.getElementById("stat");
	let es = null;
	let started = 0;
	let first = null;

	document.getElementById("ask").addEventListener("click", function () {
		if (es) es.close();
		out.textContent = "";
		stat.textContent = "接続中…";
		started = performance.now();
		first = null;

		es = new EventSource("/ask");

		es.addEventListener("delta", function (e) {
			if (first === null) {
				first = performance.now() - started;
				stat.textContent = "初回トークンまで " + Math.round(first) + "ms";
			}
			/* 連結するだけ。ここに加工を書いたら §12-1 の検証にならない */
			out.textContent += JSON.parse(e.data);
		});

		es.addEventListener("failed", function (e) {
			stat.textContent = "失敗: " + e.data;
			es.close();
		});

		/* サーバがストリームを終えると EventSource は再接続しようとするので、
		   ここで明示的に閉じる。閉じないと問いが2回作られる。 */
		es.onerror = function () {
			es.close();
			stat.textContent += "（" + out.textContent.length + "字で完了）";
		};
	});

	/* ヘッドレス検証用。#auto を付けて開くと自動で走らせる。 */
	if (location.hash === "#auto") {
		document.getElementById("ask").click();
	}

	document.getElementById("abort").addEventListener("click", function () {
		if (!es) return;
		es.close();
		es = null;
		stat.textContent = "途中で閉じた（サーバ側のログを見る）";
	});
})();
