# -*- coding: utf-8 -*-
"""UNX-E3 域批册生成器（AI-23 · B01–B40 · 800 条骨架满账）
- 数据源：e3_data_p1/p2/p3（每批 20 条：FID、条目名、行数、判据）
- 配平：每批纯功能行数求和机械配平至恰 6,000（单条钳位 200–400，AI-19/D4 判例口径）
- 输出：docs/unxreal/batches/UNX-E3-B01..B15.md（骨架态批册）
"""
import os, sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
OUT = os.path.join(REPO, "docs", "unxreal", "batches")

sys.path.insert(0, HERE)
import e3_data_p1 as p1
import e3_data_p2 as p2
import e3_data_p3 as p3
import e3_data_p4 as p4
import e3_data_p5 as p5
import e3_data_p6 as p6
import e3_data_p7 as p7

LO, HI, TARGET = 200, 400, 6000


def rebalance(rows, bname):
    rows = [list(r) for r in rows]
    assert len(rows) == 20, bname + " count != 20"
    delta = TARGET - sum(r[2] for r in rows)
    guard = 0
    while delta != 0:
        sign = 1 if delta > 0 else -1
        amount = abs(delta)
        movable = [r for r in rows if (delta > 0 and r[2] < HI) or (delta < 0 and r[2] > LO)]
        if not movable:
            raise SystemExit(bname + " cannot rebalance within bounds")
        base = amount // len(movable)
        rem = amount % len(movable)
        if base == 0 and rem > 0:
            for i in range(rem):
                movable[i][2] += sign
        else:
            for i, r in enumerate(movable):
                add = base + (1 if i < rem else 0)
                if sign > 0:
                    add = min(add, HI - r[2])
                else:
                    add = min(add, r[2] - LO)
                r[2] += sign * add
        delta = TARGET - sum(r[2] for r in rows)
        guard += 1
        if guard > 200:
            raise SystemExit(bname + " rebalance not converging")
    for r in rows:
        assert LO <= r[2] <= HI, bname + " row out of bounds: %d" % r[2]
    return rows


HEADER_TMPL = (
    "> AI-23 承办（波 09 首轮立账 · 一次对话 300 项明令）｜%s"
    "｜域账累计：%s / 240,000｜判据主轴：HTTPS 全链真站复测集（真站面随 B35+，本批为本地测试端锚）"
    "｜嫁接源：任务书六专题（HINTERNET 句柄树/代理解析/cookie 罐与缓存/Schannel 适配/异步回调与真站集）"
    "＋Wine 网络 DLL 语义参考（只跟随参考）＋MSDN 口径锚（预期值注来源）＋Windows 真机回调采样第一真值"
    "｜防重：全域 ID F17601–F18400 与已收口域零撞号（grep 五范围：kernel/varix/src、docs/START、_attic、已 finalize deepen 册、总纲既有段）"
    "｜上游：AI-21（上栈环境）、AI-42（Winsock 冻结契约先行冻结+fake 对接，波 16 全量回归联调）、AI-44（TLS 通道冻结接口，B21+ 消费）、AI-18（cookie/凭据持久化载体）、AI-47（凭据管理器）"
    "｜红线声明：本批无引导设施红线与硬件数据安全红线触发条目；真站复测集只读 GET 纪律、凭据禁明文落盘（走 AI-47）、TLS 降级选项缺省禁令——三条间接纪律全程生效"
)


def gen():
    all_rows = []
    for bkey, title, theme, rows in (p1.BATCHES_PART1 + p2.BATCHES_PART2 + p3.BATCHES_PART3 + p4.BATCHES_PART4 + p5.BATCHES_PART5 + p6.BATCHES_PART6 + p7.BATCHES_PART7):
        rows = rebalance(sorted(rows, key=lambda r: r[0]), bkey)
        all_rows.append((bkey, title, theme, rows))

    # 全域一致性
    ids = [r[0] for _, _, _, rows in all_rows for r in rows]
    assert ids == list(range(17601, 18401)), "ID not contiguous"
    assert len(set(ids)) == 800
    jids = []
    for _, _, _, rows in all_rows:
        for r in rows:
            jid = "UNX-F%d-J1" % r[0]
            assert r[3].startswith(jid), "judge id mismatch: %s" % r[0]
            jids.append(jid)
    assert len(set(jids)) == 800, "judge ids not unique"

    os.makedirs(OUT, exist_ok=True)
    total = 0
    for i, (bkey, title, theme, rows) in enumerate(all_rows):
        cum = (i + 1) * TARGET
        total += sum(r[2] for r in rows)
        L = []
        L.append("# %s · %s" % (bkey, title))
        L.append("")
        L.append(HEADER_TMPL % (theme, "{:,}".format(cum)))
        L.append("")
        bno = bkey[-3:]
        for fid, name, n, judge in rows:
            L.append("### UNX-F%d · %s" % (fid, name))
            L.append("- 域/批：E3/%s｜纯功能行数：%d｜状态：[骨架]｜判据：%s" % (bno, n, judge))
        path = os.path.join(OUT, bkey + ".md")
        with open(path, "w", encoding="utf-8", newline="\n") as fh:
            fh.write("\n".join(L) + "\n")
        print("%s: 20 items, rows=%d, cum=%d" % (bkey, sum(r[2] for r in rows), cum))
    assert total == 240000
    print("ALL OK: 40 batches, 800 items, total rows = %d (domain 240,000/240,000 FULL)" % total)


if __name__ == "__main__":
    gen()
