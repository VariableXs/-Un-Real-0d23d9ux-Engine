# -*- coding: utf-8 -*-
"""C4 B16–B30 两件套生成引擎。
用法（数据脚本内）:
    import sys; sys.path.insert(0, 'docs/unxreal/deepen')
    from _gen_engine_c4b import gen
    gen(batch='B16', theme='...', f_start=10701, rows_total=6500,
        graft='...', nomemo='...', fangchong='...', batch_note='...',
        prev_cum=85680, items=[...])
items 每条: dict(fid=10701, title=..., jd=..., rows=300, fz='...', dw='...', yb='...', yl='...', fx='...', zw='...')
断言（异常零静默）: 条数 20 / fid 连续 / 行数求和 / 判据 ID / 正文整行 >=300 字 / 判据复测成分
"""
import io

DEEP = "docs/unxreal/deepen/C4-%s.md"
BATCH = "docs/unxreal/batches/UNX-C4-%s.md"


def gen(batch, theme, f_start, rows_total, graft, nomemo, fangchong, batch_note, prev_cum, items):
    assert len(items) == 20, "%s 条数 != 20: %d" % (batch, len(items))
    assert sum(it["rows"] for it in items) == rows_total, "%s 行数求和 != %d" % (batch, rows_total)
    cum = prev_cum + rows_total

    for i, it in enumerate(items):
        fid_expect = f_start + i
        assert it["fid"] == fid_expect, "%s 第 %d 条 fid=%d 期望 %d（ID 连续性破坏）" % (batch, i, it["fid"], fid_expect)
        assert ("UNX-F%d-J1" % it["fid"]) in it["jd"], "%s F%d 判据缺 J1 编号" % (batch, it["fid"])
        zline = "- 正文：" + it["zw"]
        blen = len(zline)
        assert blen >= 300, "%s F%d 正文整行 %d 字 < 300（awk 口径）" % (batch, it["fid"], blen)
        # 判据复测成分抽查：统计面（N/10、千次、对平、对账、grep、万次 等）
        j = it["jd"]
        assert any(k in j for k in ("/10", "千次", "万次", "对平", "对账", "grep", "100/100", "核销", "零命中", "零偏离", "1 千", "断言")), \
            "%s F%d 判据缺复测性成分" % (batch, it["fid"])

    # ---------- 深化册 ----------
    head_d = (
        "# 域 UNX-C4 · 深化册 · UNX-C4-%s（F%d–F%d · 20 条全深化）\n\n"
        "> AI-14 承办｜%s｜嫁接源：%s，只跟随；禁凭记忆条款：%s｜防重声明：%s｜行数锁定：批内求和 %s，域累计 %s/240,000。\n\n"
        % (batch, f_start, f_start + 19, batch_note, graft, nomemo, fangchong,
           format(rows_total, ","), format(cum, ","))
    )
    body_d = []
    for it in items:
        body_d.append(
            "### UNX-F%d · %s\n"
            "- 域/批：C4/%s｜判据：%s｜纯功能行数：%d 行（%s；测试段不计）｜状态：[已深化]\n"
            "- **定位**：%s\n"
            "- **语义边界**：%s\n"
            "- **依赖与嫁接源**：%s\n"
            "- **风险与回退**：%s\n"
            "- 正文：%s\n\n"
            % (it["fid"], it["title"], batch, it["jd"], it["rows"], it["fz"],
               it["dw"], it["yb"], it["yl"], it["fx"], it["zw"])
        )
    deep_text = head_d + "".join(body_d).rstrip("\n") + "\n"
    io.open(DEEP % batch, "w", encoding="utf-8", newline="").write(deep_text)

    # ---------- 骨架册 ----------
    head_b = (
        "# UNX-C4-%s · %s（F%d–F%d · 20 条）\n\n"
        "> AI-14 承办｜域账累计：B01–B15 %s + 本批 %s = %s / 240,000｜嫁接源：%s（注出处，禁凭记忆）｜防重：%s｜批注：%s\n\n"
        % (batch, theme, f_start, f_start + 19,
           format(prev_cum, ","), format(rows_total, ","), format(cum, ","), graft, fangchong, batch_note)
    )
    body_b = []
    for it in items:
        body_b.append(
            "### UNX-F%d · %s\n"
            "- 域/批：C4/%s｜纯功能行数：%d｜状态：[骨架]｜判据：%s\n"
            % (it["fid"], it["title"], batch, it["rows"], it["jd"])
        )
    batch_text = head_b + "".join(body_b).rstrip("\n") + "\n"
    io.open(BATCH % batch, "w", encoding="utf-8", newline="").write(batch_text)

    print("%s 落盘：深化册 %d 字 / 骨架册 %d 字；正文最短整行 %d 字；行数求和 %s = 声明 ✓；域累计 %s"
          % (batch, len(deep_text), len(batch_text),
             min(len("- 正文：" + it["zw"]) for it in items),
             format(rows_total, ","), format(cum, ",")))
    return len(deep_text)
