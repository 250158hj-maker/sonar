# -*- coding: utf-8 -*-
"""地図が蓄積されたときの見え方を確かめるための種データ。

    python dev/seed.py --db /tmp/sonar-clean.db --sessions 10 --conversations 100
    python dev/seed.py --db /tmp/sonar-clean.db --into <session_id> --conversations 40

LLM は呼ばない。100会話ぶんの問いを実際に生成すると数百回の課金が発生するが、
**確かめたいのは地図の描画であって問いの品質ではない**（品質の検査は check/run.py）。

不変条件は守る（→設計書 データベース §10 ／ 詳細設計書 §5）：
  - 1会話につき parent_id IS NULL のノードはちょうど1つ
  - mood は5値のいずれか
  - created_at は ISO8601（+09:00）
"""
import argparse, random, sqlite3, uuid, datetime as dt

MOODS = ["chat", "listen", "fog", "sort", "none"]

# 話題ごとの (問い, 回答) の鎖。浅いところから深いところへ。
TOPICS = [
    [("そのもやもやは、何をきっかけに出てきましたか。", "断りたかったのに、その場で言えなくて引き受けてしまった"),
     ("引き受けたあと、いちばん最初に浮かんだのはどんなことでしたか。", "また同じことをやってる、と思った"),
     ("「また」というのは、どのあたりから続いていますか。", "去年の文化祭のときも同じだった"),
     ("そのときは、誰に何を言えずにいましたか。", "先輩に、もう無理ですと言えなかった"),
     ("言えなかったあと、その場をどう切り抜けましたか。", "笑ってごまかして、家で泣いた"),
     ("家で泣いたことは、誰かに話しましたか。", "話してない。心配かけたくなくて"),
     ("心配をかけたくない相手というのは、誰のことですか。", "母。もう十分やってもらってるから"),
     ("「十分やってもらってる」と感じるようになったのは、いつ頃からですか。", "父がいなくなったあたりから"),
     ("そのころ、自分の中で何が変わりましたか。", "わがままを言う枠がなくなった気がした")],

    [("整理したいのは、どのことについてですか。", "来年どうするかを決めないといけないんだけど、まだ決まってない"),
     ("いま挙がっている選択肢には、どんなものがありますか。", "進学と就職。どっちも中途半端に考えてる"),
     ("中途半端というのは、何が足りていない感じですか。", "どっちも自分で選んだ気がしない"),
     ("選んだ気がしないのは、どんなときに強く感じますか。", "人に説明しようとしたとき"),
     ("説明しようとして、詰まるのはどのあたりですか。", "なんでそれがしたいのか、が出てこない"),
     ("出てこないと気づいたのは、いつでしたか。", "面談で聞かれて黙ったとき")],

    [("何があったか、はじめから聞かせてもらえますか。", "先週、親と久しぶりに言い合いになった"),
     ("言い合いになったのは、どんなことがきっかけでしたか。", "進路のことで、勝手に決めるなと言われた"),
     ("「勝手に決めるな」。そう言われたとき、どう思いましたか。", "相談したつもりだったのに、と思った"),
     ("相談したつもりだった、というのはどんなやりとりでしたか。", "夏before に一度だけ話した"),
     ("そのときの相手の反応は、どんなものでしたか。", "ふうん、とだけ言われた")],

    [("最近、ちょっと気になったことって何かありますか。", "よく通る道にあった店が、いつのまにか閉まってた"),
     ("その店は、どのくらい前から通ってた場所だったんですか。", "中学のときから、たまに寄ってた"),
     ("たまに寄って、そこで何をしていましたか。", "何も買わずに雑誌を立ち読みしてた"),
     ("立ち読みしていた時間は、どんな時間でしたか。", "家に帰りたくない日の逃げ場だった")],

    [("そのもやもやは、何をきっかけに出てきましたか。", "朝起きるのがつらい"),
     ("朝起きるのがつらい。思い当たることはありますか。", "単純に寝るのが遅い"),
     ("寝るのが遅くなるのは、何をしている時間ですか。", "スマホを見て、気づくと2時"),
     ("その時間に見ているのは、どんなものですか。", "特に見たいものはない。ただ流してる")],

    [("整理したいのは、どのことについてですか。", "貯金の使い道を決めたい"),
     ("貯金の使い道。決めきれずにいるのは、何が引っかかっているからですか。", "使うより、残しておきたい気持ちのほうが強い"),
     ("残しておきたいのは、何に備えている感じですか。", "何かあったときに、人に頼らなくて済むように")],

    [("何があったか、はじめから聞かせてもらえますか。", "友達と久しぶりに会った"),
     ("久しぶりに会った友達。会ってみて、どうでしたか。", "変わってなかった。それが少し嬉しかった")],

    [("最近、印象に残っていることってありますか。どんな小さなことでも。", "今日は特に何もなかった")],

    [("整理したいのは、どのことについてですか。", "春に引っ越しを決めた"),
     ("引っ越し先を選ぶとき、いちばん譲れなかったのは何でしたか。", "一人になれる場所が欲しかった"),
     ("「一人になれる場所」。一人でないとき、何が起きていましたか。", "誰かといると、自分が薄まる感じがする"),
     ("薄まる感じ、というのはどんなときに気づきましたか。", "実家で family と食卓を囲んでいるとき")],

    [("そのもやもやは、何をきっかけに出てきましたか。", "三年続けたバイトを先月辞めた"),
     ("辞めると決めたのは、いつ頃でしたか。", "去年の冬から考えていた"),
     ("その間、辞めずにいたのは何が理由でしたか。", "店長が良い人で、抜けたら回らないのが分かってた"),
     ("最終的に辞める側に傾いたのは、どちらが変わったからですか。", "どっちも変わってない。自分が限界だっただけ"),
     ("その限界を、誰かに言えていましたか。", "言えてない。言ったら負けだと思ってた"),
     ("その負けは、誰に対しての負けでしたか。", "たぶん、誰にも借りを作りたくないんだと思う"),
     ("借りを作りたくない、と思うようになった出来事はありましたか。", "小学生のとき、借りたものを返せなかったことがある")],

    [("最近、ちょっと気になったことって何かありますか。", "駅前の桜が、もう散ってた"),
     ("その桜は、毎年見ている場所ですか。", "通学路だから、意識せずに見てた")],

    [("整理したいのは、どのことについてですか。", "人に頼るのが苦手なこと"),
     ("頼れなかった場面で、いちばん最近のものはどれですか。", "課題が終わらないとき、誰にも聞かなかった"),
     ("聞かずにいたのは、何が起きると思っていたからですか。", "できない人だと思われるのが怖かった"),
     ("「できない人だと思われる」。そう思われて困るのは、どんなところですか。", "次から声をかけてもらえなくなる気がする"),
     ("声をかけてもらえなくなる。それはどんな状態ですか。", "いてもいなくてもいい人になる")],
]

# 枝分かれ用の追加の一手（同じ親に2つ目の子をぶら下げる）
BRANCHES = [
    ("ほかにも思い当たることはありますか。", "起きても、やることが決まってない"),
    ("その場では、どんな言葉が出ましたか。", "いいですよ、とだけ言った"),
    ("そのとき、周りはどんな様子でしたか。", "誰も気づいてなかったと思う"),
    ("ほかに引っかかっていることはありますか。", "自分でも大げさだと思ってる"),
]


def iso(d):
    return d.strftime("%Y-%m-%dT%H:%M:%S+09:00")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--db", required=True)
    ap.add_argument("--sessions", type=int, default=10)
    ap.add_argument("--conversations", type=int, default=100)
    ap.add_argument("--into", default=None, help="既存のセッションIDを1つ目として使う（ブラウザで確認したいとき）")
    ap.add_argument("--seed", type=int, default=20260831)
    args = ap.parse_args()

    rnd = random.Random(args.seed)
    db = sqlite3.connect(args.db)

    sessions = []
    if args.into:
        sessions.append(args.into)
    while len(sessions) < args.sessions:
        sessions.append(str(uuid.uuid4()))

    # 直近4か月に散らす（地図の日付表示がばらけるように）
    end = dt.datetime(2026, 8, 31, 21, 0, 0)
    span = 120  # 日

    made_cv = made_nd = 0
    for i in range(args.conversations):
        sid = sessions[i % len(sessions)]
        topic = rnd.choice(TOPICS)
        # 浅い会話のほうが多い（実際の使われ方に寄せる）
        depth = min(len(topic), max(1, int(rnd.triangular(1, len(topic), 2))))
        started = end - dt.timedelta(days=rnd.uniform(0, span), hours=rnd.uniform(0, 12))
        mood = rnd.choice(MOODS)

        cur = db.execute(
            "insert into conversations (session_id, mood, started_at) values (?,?,?)",
            (sid, mood, iso(started)))
        cv = cur.lastrowid
        made_cv += 1

        parent = None
        t = started
        node_ids = []
        for k in range(depth):
            q, a = topic[k]
            t += dt.timedelta(seconds=rnd.randint(40, 260))
            cur = db.execute(
                "insert into nodes (conversation_id, parent_id, question, answer, created_at)"
                " values (?,?,?,?,?)", (cv, parent, q, a, iso(t)))
            parent = cur.lastrowid          # ← parent_id = NULL はループ初回だけ
            node_ids.append(parent)
            made_nd += 1

        # 3割の会話に枝を1本足す（木であって鎖ではないことを地図で見るため）
        if depth >= 2 and rnd.random() < 0.3:
            bq, ba = rnd.choice(BRANCHES)
            at = rnd.choice(node_ids[:-1])
            t += dt.timedelta(seconds=rnd.randint(40, 260))
            db.execute(
                "insert into nodes (conversation_id, parent_id, question, answer, created_at)"
                " values (?,?,?,?,?)", (cv, at, bq, ba, iso(t)))
            made_nd += 1

    db.commit()

    # 不変条件の確認（→詳細設計書 §5）
    bad = list(db.execute("select conversation_id, count(*) from nodes"
                          " where parent_id is null group by 1 having count(*) > 1"))
    bad_mood = list(db.execute("select distinct mood from conversations"
                               " where mood not in ('chat','listen','fog','sort','none')"))
    print(f"投入: 会話 {made_cv}本 / ノード {made_nd}件 / セッション {len(sessions)}個")
    print("1手目のユニーク性:", "✅ 違反なし" if not bad else f"❌ {bad}")
    print("mood の5値:", "✅ 逸脱なし" if not bad_mood else f"❌ {bad_mood}")
    for sid, n in db.execute("select session_id, count(*) from conversations"
                             " group by 1 order by count(*) desc"):
        print(f"  {sid[:8]}… : 会話 {n}本")


if __name__ == "__main__":
    main()
