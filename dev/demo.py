# -*- coding: utf-8 -*-
"""発表デモ用の `demo.db` を、本番と同じ経路で組み立てる。

    python3 dev/demo.py grow          # 未生成の「問い」を Claude に作らせて種へ書き戻す
    python3 dev/demo.py pending       # まだ回答の無いところを一覧する
    python3 dev/demo.py load --db demo.db   # 種を demo.db へ流し込む
    python3 dev/demo.py show --db demo.db   # 入ったものを読み返す

`dev/seed.py` との違い：あちらは**地図の描画**を確かめるためのもので LLM を呼ばない。
こちらは**発表で映すもの**なので、問いは `src/prompt/system.md` と `steer()` を通した
本物の Claude Haiku 4.5 の出力を使う。回答（本人の発話）だけが人の手で書かれている。

不変条件は `dev/seed.py` と同じものを守る（→設計書 データベース §10 ／ 詳細設計書 §5）：
  - 1会話につき parent_id IS NULL のノードはちょうど1つ
  - mood は5値のいずれか
  - created_at は ISO8601（+09:00）
  - 会話の中で枝分かれさせない。`src/script.js` は親を常に直前のノードに固定しており、
    地図から過去のノードへ戻る導線も無い＝**アプリが作れない形をデモに映さない**
"""
import argparse, datetime as dt, json, os, pathlib, sqlite3, sys, urllib.request, uuid

ROOT = pathlib.Path(__file__).resolve().parent.parent
SEED = ROOT / "dev" / "demo_seed.json"
SID_FILE = ROOT / "dev" / ".demo_sid"
SYSTEM_MD = ROOT / "src" / "prompt" / "system.md"
QUESTIONER_RS = ROOT / "src" / "questioner.rs"

MOODS = ["chat", "listen", "fog", "sort", "none"]

# `src/script.js` の MOODS[*].opener。1問目は LLM を呼ばず、この固定文が使われる
# （→詳細設計書 §4-2 ／ 設計書 プロンプト §2-1）。
OPENERS = {
    "chat":   "最近、ちょっと気になったことって何かありますか。",
    "listen": "何があったか、はじめから聞かせてもらえますか。",
    "fog":    "そのもやもやは、何をきっかけに出てきましたか。",
    "sort":   "整理したいのは、どのことについてですか。",
    "none":   "最近、印象に残っていることってありますか。どんな小さなことでも。",
}

# `src/questioner.rs` の steer() の写し。写しである以上ずれうるので、
# 下の assert_steer_matches_rust() が毎回 Rust 側と突き合わせる（→項番13 と同じ守り方）。
SHORT_ANSWER_CHARS = 20
STEER = {
    ("chat", True):   "深めない。直前の話題と同じ深さで、隣にあることを聞く。「なぜ」を聞かない。",
    ("chat", False):  "深めない。直前の話題と同じ深さで、隣にあることを聞く。「なぜ」を聞かない。",
    ("listen", True): "話題を変えない。同じ出来事の続きを促す。",
    ("listen", False):"話題を変えない。同じ出来事の続きを促す。",
    ("sort", True):   "深めてよい。話し手が挙げた要素どうしを突き合わせて、選んだ理由の側を聞く。",
    ("sort", False):  "深めてよい。話し手が挙げた要素どうしを突き合わせて、選んだ理由の側を聞く。",
    ("fog", False):   "一歩ずつ深める。",
    ("fog", True):    "深めない。同じ深さで、いま出ている言葉について聞き直す。",
    ("none", False):  "一歩深める。",
    ("none", True):   "深めない。同じ深さで別のことを聞く。",
}


def assert_steer_matches_rust():
    """写した指示文が `src/questioner.rs` に実在することを確かめる。

    ここがずれると、デモに映る問いだけが本番と違う掘り方で作られる——
    しかも出力は日本語として自然なので、見ても気づけない。"""
    src = QUESTIONER_RS.read_text(encoding="utf-8")
    missing = sorted({s for s in STEER.values() if s not in src})
    if missing:
        sys.exit(f"steer の写しが src/questioner.rs と合わない: {missing}")
    if f"SHORT_ANSWER_CHARS: usize = {SHORT_ANSWER_CHARS}" not in src:
        sys.exit("SHORT_ANSWER_CHARS が src/questioner.rs と合わない")


def steer(mood, last_answer):
    return STEER[(mood, len(last_answer) < SHORT_ANSWER_CHARS)]


def session_id(explicit=None):
    """デモの地図を出すための匿名セッションID。

    **種ファイル（`demo_seed.json`）には置かない。** 公開リポジトリへ入ると、
    デプロイした先で他人がその地図を開ける生きた鍵になり、git 履歴からは消せない
    （→docs/next.md §10-3。控え先は紙）。優先順は 引数 → 環境変数 → `dev/.demo_sid`。
    どれも無ければ発行して `dev/.demo_sid`（.gitignore 済み）へ書く。"""
    sid = explicit or os.environ.get("SONAR_DEMO_SID")
    if sid:
        return sid.strip()
    if SID_FILE.exists():
        return SID_FILE.read_text(encoding="utf-8").strip()
    sid = str(uuid.uuid4())
    SID_FILE.write_text(sid + "\n", encoding="utf-8")
    print(f"session_id を発行して {SID_FILE.relative_to(ROOT)} に書いた: {sid}")
    print("**紙に控えること。** ブラウザの Cookie `sonar_sid` にこの値を入れると地図が出る")
    return sid


def load_seed():
    return json.loads(SEED.read_text(encoding="utf-8"))


def save_seed(seed):
    SEED.write_text(json.dumps(seed, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


# ---------------------------------------------------------------------------
# grow：問いを Claude に作らせる。`src/questioner.rs` と同じ形で投げる
# ---------------------------------------------------------------------------

def ask(history, steer_text):
    """questioner.rs の `AnthropicQuestioner::ask` と同じ body・同じヘッダ。
    違いは stream を使わないことだけ（全文が要るので刻む意味が無い）。"""
    key = os.environ.get("ANTHROPIC_API_KEY")
    if not key:
        sys.exit("ANTHROPIC_API_KEY が無い（→ADR-0005 §3-5）")

    messages = []
    for t in history:
        messages.append({"role": "assistant", "content": t["question"]})
        messages.append({"role": "user", "content": t["answer"]})

    body = {
        "model": "claude-haiku-4-5",
        "max_tokens": 300,
        "system": SYSTEM_MD.read_text(encoding="utf-8").replace("{steer}", steer_text),
        "messages": messages,
    }
    req = urllib.request.Request(
        "https://api.anthropic.com/v1/messages",
        data=json.dumps(body).encode("utf-8"),
        headers={"x-api-key": key, "anthropic-version": "2023-06-01",
                 "content-type": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=60) as r:
        v = json.loads(r.read())
    return "".join(b.get("text", "") for b in v["content"]).strip()


def cmd_grow(args):
    assert_steer_matches_rust()
    seed = load_seed()
    made = 0
    for i, cv in enumerate(seed["conversations"], 1):
        turns = cv["turns"]
        # 回答待ちが残っている会話は触らない（履歴が穴だらけのまま投げないため）
        if any(t["answer"] is None for t in turns):
            continue
        if len(turns) >= cv["target_depth"]:
            continue
        q = ask(turns, steer(cv["mood"], turns[-1]["answer"]))
        turns.append({"question": q, "answer": None, "source": "api"})
        made += 1
        print(f"[{i:2d}] {cv['mood']:6s} 深さ{len(turns)}  {q}")
    save_seed(seed)
    print(f"\n生成: {made}件")


def cmd_pending(args):
    seed = load_seed()
    n = 0
    for i, cv in enumerate(seed["conversations"], 1):
        for d, t in enumerate(cv["turns"], 1):
            if t["answer"] is None:
                n += 1
                prev = cv["turns"][d - 2]["answer"] if d >= 2 else "—"
                print(f"[{i:2d}] 深さ{d} mood={cv['mood']}\n     直前の回答: {prev}\n     問い: {t['question']}")
    print(f"\n回答待ち: {n}件")


# ---------------------------------------------------------------------------
# load：demo.db へ流し込む
# ---------------------------------------------------------------------------

def cmd_load(args):
    seed = load_seed()
    holes = [(i, d) for i, cv in enumerate(seed["conversations"], 1)
             for d, t in enumerate(cv["turns"], 1) if not t["answer"]]
    if holes:
        sys.exit(f"回答が空のノードが {len(holes)}件ある: {holes[:5]}")

    db = sqlite3.connect(args.db)
    have = db.execute("select count(*) from conversations").fetchone()[0]
    if have and not args.append:
        sys.exit(f"{args.db} には既に会話が {have}本ある。消してから流すか --append を付ける")

    sid = session_id(args.sid)
    print(f"session_id: {sid}")
    for cv in seed["conversations"]:
        if cv["mood"] not in MOODS:
            sys.exit(f"mood が5値でない: {cv['mood']}")
        cur = db.execute("insert into conversations (session_id, mood, started_at) values (?,?,?)",
                         (sid, cv["mood"], cv["started_at"]))
        conv_id = cur.lastrowid
        parent = None                      # ← NULL はここだけ。ループ初回で必ず埋まる
        # 1手目の created_at は started_at と同じ（store::begin_conversation が
        # 同じ `now` を両方に書く）。2手目以降は1問1答ぶんの間隔を決め打ちで足す。
        t0 = dt.datetime.strptime(cv["started_at"], "%Y-%m-%dT%H:%M:%S%z")
        for k, t in enumerate(cv["turns"]):
            at = (t0 + dt.timedelta(seconds=k * 96 + (k * 37) % 71)).strftime("%Y-%m-%dT%H:%M:%S%z")
            at = at[:-2] + ":" + at[-2:]   # +0900 → +09:00
            cur = db.execute(
                "insert into nodes (conversation_id, parent_id, question, answer, created_at)"
                " values (?,?,?,?,?)", (conv_id, parent, t["question"], t["answer"], at))
            parent = cur.lastrowid
    db.commit()
    verify(db, sid)


def verify(db, sid=None):
    cv, nd = (db.execute("select count(*) from conversations").fetchone()[0],
              db.execute("select count(*) from nodes").fetchone()[0])
    bad_head = list(db.execute("select conversation_id, count(*) from nodes"
                               " where parent_id is null group by 1 having count(*) <> 1"))
    bad_mood = list(db.execute("select distinct mood from conversations"
                               " where mood not in ('chat','listen','fog','sort','none')"))
    bad_parent = list(db.execute(
        "select c.id from nodes c join nodes p on p.id = c.parent_id"
        " where p.conversation_id <> c.conversation_id"))
    bad_order = list(db.execute("select id from nodes where parent_id is not null and parent_id >= id"))
    branches = list(db.execute("select parent_id, count(*) from nodes"
                               " where parent_id is not null group by 1 having count(*) > 1"))
    sids = list(db.execute("select session_id, count(*) from conversations group by 1"))
    empty = list(db.execute("select id from nodes where trim(question)='' or trim(answer)=''"))

    print(f"会話 {cv}本 / ノード {nd}件")
    print("1手目がちょうど1つ :", "OK" if not bad_head else f"NG {bad_head}")
    print("mood が5値         :", "OK" if not bad_mood else f"NG {bad_mood}")
    print("親が同じ会話        :", "OK" if not bad_parent else f"NG {bad_parent}")
    print("親の id が子より小  :", "OK" if not bad_order else f"NG {bad_order}")
    print("問い・回答が非空    :", "OK" if not empty else f"NG {empty}")
    print("枝分かれ            :", "無し" if not branches else f"{len(branches)}箇所 {branches}")
    print("セッション          :", ", ".join(f"{s[:8]}…×{n}" for s, n in sids))
    # AUTOINCREMENT の続きを合わせる。ずれると次に本人が話したとき id が衝突する
    for tbl in ("conversations", "nodes"):
        mx = db.execute(f"select coalesce(max(id),0) from {tbl}").fetchone()[0]
        db.execute("insert into sqlite_sequence (name, seq) select ?, ? "
                   "where not exists (select 1 from sqlite_sequence where name = ?)", (tbl, mx, tbl))
        db.execute("update sqlite_sequence set seq = ? where name = ? and seq < ?", (mx, tbl, mx))
    db.commit()
    print("sqlite_sequence     :", dict(db.execute("select name, seq from sqlite_sequence")))


def cmd_show(args):
    db = sqlite3.connect(args.db)
    for cid, mood, started in db.execute(
            "select id, mood, started_at from conversations order by id"):
        rows = list(db.execute("select id, parent_id, question, answer from nodes"
                               " where conversation_id=? order by id", (cid,)))
        print(f"\n── 会話{cid}  {mood}  {started[:10]}  深さ{len(rows)}")
        for d, (nid, pid, q, a) in enumerate(rows, 1):
            print(f"  {d}. Q {q}")
            print(f"     A {a}")
    print()
    verify(db)


def cmd_verify(args):
    verify(sqlite3.connect(args.db))


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("grow").set_defaults(fn=cmd_grow)
    sub.add_parser("pending").set_defaults(fn=cmd_pending)
    p = sub.add_parser("load"); p.add_argument("--db", required=True)
    p.add_argument("--sid", default=None, help="匿名セッションID。既定は SONAR_DEMO_SID か dev/.demo_sid")
    p.add_argument("--append", action="store_true"); p.set_defaults(fn=cmd_load)
    p = sub.add_parser("show"); p.add_argument("--db", required=True); p.set_defaults(fn=cmd_show)
    p = sub.add_parser("verify"); p.add_argument("--db", required=True); p.set_defaults(fn=cmd_verify)
    a = ap.parse_args()
    a.fn(a)
