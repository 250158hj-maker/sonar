# -*- coding: utf-8 -*-
"""テスト項目書 §4-5〜§4-8 の HTTP／DB 検査（項番25〜50）と、項番70。

    rm -f /tmp/sonar-test.db
    env -u ANTHROPIC_API_KEY PORT=3199 SONAR_DB=/tmp/sonar-test.db topcoat dev
    python3 check/http.py

**依存を1つも足さない**（→テスト項目書 §3-2）。`urllib.request`（HTTP）・
`http.cookiejar`（セッションの作り分け）・`sqlite3`（DB の直読み／直書き）は
すべて標準ライブラリ。

**項番ごとに新しい Cookie から始める**（→同 §3-3 規約4）。DB は使い捨てで、
サーバを起動する前に消す。サーバが掴んでいる最中に消すと接続が宙に浮くので、
項番ごとの独立性は「新しいセッション」で取り、DB のリセットでは取らない。

**`ANTHROPIC_API_KEY` を外してサーバを起動する。** 項番37〜40・44 は
`/talk/question` を叩くが、いずれも LLM に到達する前（クエリ検証・所有確認・
`path_to`）で決着する。キーが無ければ `AnthropicQuestioner::from_env()` が
即座に失敗するので、**課金を発生させずに経路を確かめられる**。
項番36（実際にデルタが届くか）だけはこの方法では確かめられないので実装しない。
"""
# 【罠】このファイル名（§3-1 が指定した `check/http.py`）は**標準ライブラリの
# `http` パッケージを隠す**。スクリプトとして起動すると `sys.path[0]` が
# `check/` になり、`import http.cookiejar` が自分自身を掴んで
# `'http' is not a package` で落ちる。標準ライブラリを読む前に自分を外す。
import os
import sys

_HERE = os.path.dirname(os.path.abspath(__file__))
sys.path[:] = [p for p in sys.path if os.path.abspath(p or ".") != _HERE]

import http.cookiejar
import importlib.util
import json
import pathlib
import re
import sqlite3
import urllib.error
import urllib.request

sys.stdout.reconfigure(encoding="utf-8")

ROOT = pathlib.Path(__file__).resolve().parent.parent
BASE = os.environ.get("SONAR_TEST_BASE", "http://127.0.0.1:3199")
DB = os.environ.get("SONAR_DB", "/tmp/sonar-test.db")

CHECKS = []


def check(no, desc):
    """§4 の項番と1対1で登録する。関数は (合否, 実際の値) を返す。"""
    def deco(fn):
        CHECKS.append((no, desc, fn))
        return fn
    return deco


def eq(actual, expected):
    return actual == expected, repr(actual)


# ---------------------------------------------------------------------------
# HTTP
# ---------------------------------------------------------------------------

class Session:
    """1項番に1つ。最初のリクエストで新しい `sonar_sid` が発行される。"""

    def __init__(self):
        self.jar = http.cookiejar.CookieJar()
        self.opener = urllib.request.build_opener(
            urllib.request.HTTPCookieProcessor(self.jar))

    def request(self, method, path, body=None, timeout=15):
        data = None
        if body is not None:
            data = json.dumps(body, ensure_ascii=False).encode("utf-8")
        req = urllib.request.Request(BASE + path, data=data, method=method)
        if data is not None:
            req.add_header("content-type", "application/json")
        try:
            with self.opener.open(req, timeout=timeout) as r:
                return r.status, r.read().decode("utf-8", "replace")
        except urllib.error.HTTPError as e:
            return e.code, e.read().decode("utf-8", "replace")

    def post(self, mood="fog", question="問い", answer="回答",
             conversation_id=None, parent_id=None):
        body = {"mood": mood, "question": question, "answer": answer}
        if conversation_id is not None:
            body["conversation_id"] = conversation_id
        if parent_id is not None:
            body["parent_id"] = parent_id
        return self.request("POST", "/talk/answer", body)

    def get(self, path, timeout=15):
        return self.request("GET", path, timeout=timeout)

    # -- 組み立て用（検査そのものではない） --------------------------------

    def start(self, mood="fog", question="問い", answer="回答"):
        """会話を1本作り、(conversation_id, node_id) を返す。"""
        st, body = self.post(mood=mood, question=question, answer=answer)
        if st != 200:
            raise RuntimeError(f"前提の POST が {st}: {body[:200]}")
        d = json.loads(body)
        return d["conversation_id"], d["node_id"]

    def append(self, cv, parent, question="問い", answer="回答"):
        st, body = self.post(conversation_id=cv, parent_id=parent,
                             question=question, answer=answer)
        if st != 200:
            raise RuntimeError(f"前提の POST が {st}: {body[:200]}")
        return json.loads(body)["node_id"]


# ---------------------------------------------------------------------------
# DB（HTTP の応答だけでは確かめられないものを直接見る → §3-2 の理由1）
# ---------------------------------------------------------------------------

def rows(sql, *args):
    con = sqlite3.connect(DB)
    try:
        return con.execute(sql, args).fetchall()
    finally:
        con.close()


def write(sql, *args):
    con = sqlite3.connect(DB)
    try:
        cur = con.execute(sql, args)
        con.commit()
        return cur.lastrowid
    finally:
        con.close()


def roots_of(cv):
    return rows("select count(*) from nodes where conversation_id=? and parent_id is null", cv)[0][0]


# ---------------------------------------------------------------------------
# HTML から `window.SONAR` を取り出す（本番と同じ出力を見る → §3-3 規約5）
# ---------------------------------------------------------------------------

SONAR = re.compile(r"window\.SONAR=(.*?);</script>", re.S)


def sonar_src(html):
    """`window.SONAR=` の右辺を**文字列のまま**返す。無ければ None。"""
    m = SONAR.search(html)
    return m.group(1) if m else None


def sonar_json(html):
    src = sonar_src(html)
    return json.loads(src) if src else None


# ===========================================================================
# §4-5 POST /talk/answer（項番25〜35）
# ===========================================================================

@check(25, "空のセッションで POST → 200・id が返り・1手目が1件できる")
def c25():
    s = Session()
    st, body = s.post(mood="fog", question="問い", answer="回答")
    d = json.loads(body) if st == 200 else {}
    got = (st, "conversation_id" in d and "node_id" in d,
           roots_of(d["conversation_id"]) if st == 200 else None)
    return eq(got, (200, True, 1))


@check(26, "2手目を POST → 200・その行の parent_id が1手目になる")
def c26():
    s = Session()
    cv, first = s.start()
    st, body = s.post(conversation_id=cv, parent_id=first, question="問い2", answer="回答2")
    second = json.loads(body)["node_id"] if st == 200 else None
    parent = rows("select parent_id from nodes where id=?", second)[0][0] if second else None
    return eq((st, parent), (200, first))


@check(27, "answer:'' で POST → 400")
def c27():
    return eq(Session().post(answer="")[0], 400)


@check(28, "answer:'   '（空白のみ）で POST → 400")
def c28():
    return eq(Session().post(answer="   ")[0], 400)


@check(29, "question:'' で POST → 400")
def c29():
    return eq(Session().post(question="")[0], 400)


@check(30, "mood:'happy' で POST → 400")
def c30():
    return eq(Session().post(mood="happy")[0], 400)


@check(31, "conversation_id あり・parent_id なしで POST → 400")
def c31():
    s = Session()
    cv, _ = s.start()
    return eq(s.post(conversation_id=cv)[0], 400)


@check(32, "Cookie B で A の会話に追記 → 403")
def c32():
    a, b = Session(), Session()
    cv, node = a.start()
    b.start()                       # B にも自分のセッションを持たせる
    return eq(b.post(conversation_id=cv, parent_id=node)[0], 403)


@check(33, "会話1に会話2のノードを親として POST → 400")
def c33():
    s = Session()
    cv1, _ = s.start()
    _, node2 = s.start()            # 同じセッションで2本目
    return eq(s.post(conversation_id=cv1, parent_id=node2)[0], 400)


@check(34, "conversation_id なし・parent_id ありで POST → 200・新しい会話の1手目")
def c34():
    s = Session()
    _, other = s.start()
    st, body = s.post(parent_id=other, question="問い", answer="回答")
    node = json.loads(body)["node_id"] if st == 200 else None
    parent = rows("select parent_id from nodes where id=?", node)[0][0] if node else "行が無い"
    return eq((st, parent), (200, None))


@check(35, "1手目＋19手つないで20手 → その会話の parent_id IS NULL がちょうど1件")
def c35():
    s = Session()
    cv, node = s.start()
    for i in range(19):
        node = s.append(cv, node, question=f"問い{i}", answer=f"回答{i}")
    total = rows("select count(*) from nodes where conversation_id=?", cv)[0][0]
    return eq((total, roots_of(cv)), (20, 1))


# ===========================================================================
# §4-6 GET /talk/question（項番37〜40。項番36 は課金あり＝未実装）
# ===========================================================================

@check(37, "node を付けずに GET → 400")
def c37():
    return eq(Session().get("/talk/question")[0], 400)


@check(38, "?node=abc（数値でない）で GET → 400")
def c38():
    return eq(Session().get("/talk/question?node=abc")[0], 400)


@check(39, "Cookie B で A の node を GET → 403")
def c39():
    a, b = Session(), Session()
    _, node = a.start()
    b.start()
    return eq(b.get(f"/talk/question?node={node}")[0], 403)


@check(40, "?node=999999（存在しない）で GET → 4xx（500 にならない）")
def c40():
    st = Session().get("/talk/question?node=999999")[0]
    return 400 <= st < 500, repr(st)


# ===========================================================================
# §4-7 木構造と履歴（項番41〜44）
# ===========================================================================

@check(41, "2手目を親にして枝を作る → 200・parent_id が2手目の行が2件")
def c41():
    s = Session()
    cv, n1 = s.start()
    n2 = s.append(cv, n1)
    s.append(cv, n2)                            # 直列3手目
    st, _ = s.post(conversation_id=cv, parent_id=n2, question="枝", answer="枝の回答")
    kids = rows("select count(*) from nodes where parent_id=?", n2)[0][0]
    return eq((st, kids), (200, 2))


@check(42, "同じ会話の全ノードで、親の id が必ず子の id より小さい")
def c42():
    s = Session()
    cv, n1 = s.start()
    n2 = s.append(cv, n1)
    s.append(cv, n2)
    s.append(cv, n2)                            # 枝も混ぜる
    bad = rows("""select c.id, c.parent_id from nodes c join nodes p on p.id = c.parent_id
                  where c.conversation_id=? and p.id >= c.id""", cv)
    return eq(bad, [])


@check(43, "created_at が同一秒の親子を直接投入 → created_at では順序が決まらない")
def c43():
    s = Session()
    cv, _ = s.start()
    same = "2026-08-31T12:00:00+09:00"
    parent = write("""insert into nodes (conversation_id, parent_id, question, answer, created_at)
                      values (?, null, '同秒の親', '回答', ?)""", cv, same)
    child = write("""insert into nodes (conversation_id, parent_id, question, answer, created_at)
                     values (?, ?, '同秒の子', '回答', ?)""", cv, parent, same)
    got = rows("select created_at from nodes where id in (?,?)", parent, child)
    return eq(len({t for (t,) in got}), 1)      # 同値＝created_at では並べられない


@check(44, "parent_id が循環するデータで GET /talk/question → 無限ループせず返る")
def c44():
    s = Session()
    cv, root = s.start()
    a = write("""insert into nodes (conversation_id, parent_id, question, answer, created_at)
                 values (?, ?, '循環A', '回答', '2026-08-31T12:00:00+09:00')""", cv, root)
    b = write("""insert into nodes (conversation_id, parent_id, question, answer, created_at)
                 values (?, ?, '循環B', '回答', '2026-08-31T12:00:01+09:00')""", cv, a)
    write("update nodes set parent_id=? where id=?", b, a)      # A→B→A の輪を閉じる
    try:
        st, _ = s.get(f"/talk/question?node={a}", timeout=20)
    except Exception as e:                                       # タイムアウト＝返らなかった
        return False, f"レスポンスが返らなかった: {type(e).__name__}"
    return True, f"HTTP {st} が返った（無限ループしていない）"


# ===========================================================================
# §4-8 描画データ（項番45〜50）
# ===========================================================================

@check(45, "会話1本・3ノードで /map → nodes が root 込みで4件・conversations が1件")
def c45():
    s = Session()
    cv, n1 = s.start()
    n2 = s.append(cv, n1)
    s.append(cv, n2)
    d = sonar_json(s.get("/map")[1])
    got = (len(d["nodes"]), len(d["conversations"])) if d else None
    return eq(got, (4, 1))


@check(46, "answer に </script> を含めて /map → 生の </script> が出ず \\u003c に置換")
def c46():
    s = Session()
    s.start(answer="</script><script>alert(1)</script>")
    src = sonar_src(s.get("/map")[1])
    got = (src is not None and "</script>" not in src, src is not None and "\\u003c" in src)
    return eq(got, (True, True))


@check(47, "空のセッションで /map → window.SONAR も座標系も出力されない")
def c47():
    html = Session().get("/map")[1]
    return eq((sonar_src(html) is None, 'id="stage"' in html), (True, False))


@check(48, "空のセッションで / → 統計もプレビューも出力されない")
def c48():
    html = Session().get("/")[1]
    return eq(('class="stats"' in html, 'id="pvChart"' in html), (False, False))


@check(49, "会話1本以上で / → 統計の項目数が2つ")
def c49():
    s = Session()
    s.start()
    return eq(s.get("/")[1].count('class="stat__num"'), 2)


@check(50, "Cookie B で /map → B の会話だけ・A のノードが混ざらない")
def c50():
    a, b = Session(), Session()
    a.start(answer="Aだけの言葉")
    b.start(answer="Bだけの言葉")
    html = b.get("/map")[1]
    d = sonar_json(html)
    quotes = [n["quote"] for n in d["nodes"].values()] if d else []
    got = (len(d["conversations"]) if d else None, "Aだけの言葉" in "".join(quotes))
    return eq(got, (1, False))


# ===========================================================================
# §4-12 のうち課金なしで確かめられるもの（項番70）
#
# 【置き場所の逸脱】§3-1 は L4 を `check/run.py`（既存。触らない）としているが、
# 項番70 は「run.py が読むパスの確認」であって API を叩かない。run.py 自身に
# 検査を足すと「触らない」に反するので、Python の検査ハーネスであるここに置く。
# ===========================================================================

@check(70, "check/run.py が読む system プロンプトが src/prompt/system.md（正典）")
def c70():
    spec = importlib.util.spec_from_file_location("sonar_run", ROOT / "check" / "run.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)                 # 定数をソースから読む。写さない
    return eq(pathlib.Path(mod.PROMPT).resolve(), (ROOT / "src" / "prompt" / "system.md").resolve())


# ---------------------------------------------------------------------------

def main():
    try:
        Session().get("/", timeout=5)
    except Exception as e:
        sys.exit(f"❌ {BASE} に繋がらない（{type(e).__name__}）。\n"
                 f"   rm -f {DB}\n"
                 f"   env -u ANTHROPIC_API_KEY PORT=3199 SONAR_DB={DB} topcoat dev")
    if not pathlib.Path(DB).exists():
        sys.exit(f"❌ DB が無い: {DB}（SONAR_DB がサーバと揃っているか確認する）")

    only = {int(a) for a in sys.argv[1:] if a.isdigit()}
    fails = []
    for no, desc, fn in CHECKS:
        if only and no not in only:
            continue
        try:
            ok, actual = fn()
        except Exception as e:
            ok, actual = False, f"例外 {type(e).__name__}: {e}"
        print(f"{'✅' if ok else '❌'} 項番{no:<3} {desc}")
        if not ok:
            print(f"          実際: {actual}")
            fails.append((no, desc, actual))

    ran = len(only) if only else len(CHECKS)
    print(f"\n{ran - len(fails)}/{ran} 合格")
    if fails:
        print("不合格:")
        for no, desc, actual in fails:
            print(f"  項番{no}  {desc}\n          実際: {actual}")
        sys.exit(1)


if __name__ == "__main__":
    main()
