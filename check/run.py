# -*- coding: utf-8 -*-
"""設計書 プロンプト §9 の検査ハーネス。

    python3 check/run.py <mood>       # mood は chat / listen / fog / sort / none

1ターン目は §9 の固定台本、2ターン目以降は実際に答える。5ターンで1本。
結果は check/log/<mood>.json に残る（チェック表を埋める材料）。

system プロンプトは **`src/prompt/system.md`（正典）を実行時に読む**。
以前は Vault の「設計書 プロンプト.md」§3 の ````text ブロックを読んでいたが、
正典は 2026-08-31 に Sonar 側へ移設済み（→詳細設計書 §4-3）で、
**検査だけが旧本文を見続ける状態になっていた**ため、参照先をここで揃えた。
コピーを持たないので、正典を直せば次の実行から反映される。
"""
import os, sys, re, json, pathlib, urllib.request

sys.stdout.reconfigure(encoding="utf-8")
ROOT = pathlib.Path(__file__).resolve().parent.parent
PROMPT = ROOT / "src" / "prompt" / "system.md"
SHORT_ANSWER_CHARS = 20
TURNS = 5

OPENER = {
    "chat":   "最近、ちょっと気になったことって何かありますか。",
    "listen": "何があったか、はじめから聞かせてもらえますか。",
    "fog":    "そのもやもやは、何をきっかけに出てきましたか。",
    "sort":   "整理したいのは、どのことについてですか。",
    "none":   "最近、印象に残っていることってありますか。どんな小さなことでも。",
}
FIRST = {   # §9 の固定台本。few-shot と重ならないもの
    "chat":   "よく通る道にあった店が、いつのまにか閉まってた",
    "listen": "先週、親と久しぶりに言い合いになった",
    "fog":    "断りたかったのに、その場で言えなくて引き受けてしまった",
    "sort":   "来年どうするかを決めないといけないんだけど、まだ決まってない",
    "none":   "久しぶりに会った友達が、思ってたより元気そうだった",
}
STEER = {
    ("chat",   False): "深めない。直前の話題と同じ深さで、隣にあることを聞く。「なぜ」を聞かない。",
    ("chat",   True):  "深めない。直前の話題と同じ深さで、隣にあることを聞く。「なぜ」を聞かない。",
    ("listen", False): "話題を変えない。同じ出来事の続きを促す。",
    ("listen", True):  "話題を変えない。同じ出来事の続きを促す。",
    ("sort",   False): "深めてよい。話し手が挙げた要素どうしを突き合わせて、選んだ理由の側を聞く。",
    ("sort",   True):  "深めてよい。話し手が挙げた要素どうしを突き合わせて、選んだ理由の側を聞く。",
    ("fog",    False): "一歩ずつ深める。",
    ("fog",    True):  "深めない。同じ深さで、いま出ている言葉について聞き直す。",
    ("none",   False): "一歩深める。",
    ("none",   True):  "深めない。同じ深さで別のことを聞く。",
}
NG_WORDS = ["あなた", "きみ", "君は", "タイプ", "だと思いますが", "ですよね"]

# 疑問詞。「何か」「どこか」「いつか」は不定語であって疑問詞ではないので、
# 単純な部分一致だと Yes/No 型を見逃す（2026-08-27 の実測で判明）。
QUESTION_WORDS = ["どんな", "いつ", "どこ", "何", "誰", "どちら", "どう", "なぜ", "どの", "いくつ"]
INDEFINITE = ["何か", "どこか", "いつか", "誰か", "どうか"]


def shape_flags(q):
    """形で判定できる違反だけを返す。意味の検査は目視（→§9）。"""
    stripped = q
    for w in INDEFINITE:
        stripped = stripped.replace(w, "")
    open_q = any(w in stripped for w in QUESTION_WORDS)
    flags = []
    if not open_q and re.search(r"(ですか|でしたか|ますか|ましたか|んですか)[。？]?$", q):
        flags.append("Yes/No型")
    # 二択。`それとも` で並べる形と、「AかBか、どっち」で並べる形の2つがある。
    # 後者は 2026-08-31 の実測で出た（→詳細設計書 §6）。当時これが拾われたのは
    # **偶然** で、`どっち` が QUESTION_WORDS に無かったため Yes/No 型として
    # 別の理由で引っかかっていただけだった。
    # → QUESTION_WORDS に `どっち` を足すなら、必ずこの判定を入れた後で。
    if "それとも" in q or re.search(r"[^、。]+か[^、。]+か、?\s*(どっち|どちら)", q):
        flags.append("二択")
    if len(q) > 60:
        flags.append(f"{len(q)}字（上限60）")
    if len(q) < 30:
        flags.append(f"{len(q)}字（下限30）")
    if q.count("？") + q.count("?") > 1:
        flags.append("疑問符2つ以上")
    return flags


def system_prompt(steer):
    """正典をそのまま使う。`questioner.rs` の `SYSTEM_PROMPT` と同じファイル・同じ差し込み方。

    以前は設計書からブロックを正規表現で切り出していた。ファイルが丸ごと本文に
    なった以上、抽出は要らない——**加工が要るなら、それは正典が下書きだということ。**
    """
    return PROMPT.read_text(encoding="utf-8").replace("{steer}", steer)


def ask(system, messages):
    """stream:true で叩き、delta.text を連結して1文字ずつ表示する。"""
    payload = json.dumps({"model": "claude-haiku-4-5", "max_tokens": 300, "stream": True,
                          "system": system, "messages": messages}, ensure_ascii=False)
    req = urllib.request.Request(
        "https://api.anthropic.com/v1/messages", data=payload.encode("utf-8"), method="POST",
        headers={"x-api-key": os.environ["ANTHROPIC_API_KEY"],
                 "anthropic-version": "2023-06-01", "content-type": "application/json"})
    out, usage = [], {}
    with urllib.request.urlopen(req) as r:
        for raw in r:
            line = raw.decode("utf-8").strip()
            if not line.startswith("data: "):
                continue
            ev = json.loads(line[6:])
            if ev.get("type") == "message_start":
                usage.update(ev["message"]["usage"])
            elif ev.get("type") == "content_block_delta" and ev["delta"]["type"] == "text_delta":
                out.append(ev["delta"]["text"])
                print(ev["delta"]["text"], end="", flush=True)   # 逐次表示の再現
            elif ev.get("type") == "message_delta":
                usage.update(ev.get("usage", {}))
                usage["stop_reason"] = ev["delta"].get("stop_reason")
    print()
    return "".join(out), usage


def main(mood):
    if mood not in OPENER:
        sys.exit(f"mood は {' / '.join(OPENER)} のいずれか")
    if not os.environ.get("ANTHROPIC_API_KEY"):
        sys.exit("ANTHROPIC_API_KEY が未設定。新しいターミナルを開き直すと反映される")

    messages, rows = [], []
    question = OPENER[mood]
    print(f"=== {mood} ===\n[1] 問い : {question}")

    for turn in range(1, TURNS + 1):
        if turn == 1:
            answer = FIRST[mood]
            print(f"[1] 回答 : {answer}  （§9 の固定台本）")
        else:
            answer = input(f"[{turn}] 回答 > ").strip()
            if not answer:
                print("（空欄で終了）")
                break

        messages += [{"role": "assistant", "content": question},
                     {"role": "user", "content": answer}]
        short = len(answer) < SHORT_ANSWER_CHARS
        steer = STEER[(mood, short)]

        if turn == TURNS:
            rows.append({"turn": turn, "answer": answer, "question": None, "steer": None})
            break

        print(f"[{turn + 1}] 問い : ", end="", flush=True)
        question, usage = ask(system_prompt(steer), messages)
        ng = [w for w in NG_WORDS if w in question]
        flags = ([f"NG語 {ng}"] if ng else []) + shape_flags(question)
        rows.append({"turn": turn, "answer": answer, "answer_len": len(answer), "short": short,
                     "steer": steer, "question": question, "question_len": len(question),
                     "ng_words": ng, "flags": flags, "usage": usage})
        print(f"      → {'  '.join(flags) if flags else '形の検査は異常なし（意味の検査は目視）'}")

    scored = [r for r in rows if r.get("question")]
    tin = sum(r["usage"].get("input_tokens", 0) for r in scored)
    tout = sum(r["usage"].get("output_tokens", 0) for r in scored)
    print(f"\n--- {mood}: {len(scored)}問 / in={tin} out={tout} / ${tin/1e6 + tout*5/1e6:.5f}")

    out = ROOT / "check" / "log" / f"{mood}.json"
    out.parent.mkdir(parents=True, exist_ok=True)   # log/ は .gitignore 済み
    out.write_text(json.dumps({"mood": mood, "rows": rows}, ensure_ascii=False, indent=2),
                   encoding="utf-8")
    print(f"--- 記録: {out.relative_to(ROOT)}")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "")
