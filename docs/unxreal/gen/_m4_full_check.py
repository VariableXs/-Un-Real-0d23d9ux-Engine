# -*- coding: utf-8 -*-
"""AI-64 · UNX-M4 全域收口校验器（八查）：ALL PASS 才算收口。
1 40 册在位  2 800 条 ID 逐一  3 每条 ≥300 字符  4 E 型 J1R 保留
5 主册状态 800/800 已深化零残留  6 判据主轴 J-1/J-2 承载在册
7 I 型联签域锚定  8 域关门印/终了声明 + ID 段连续
"""
import io, os, re, sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
DEEP = os.path.join(ROOT, "deepen")
MAIN = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal.md")
ETYPE = set(range(21, 29))

def main():
    errs = []
    # 1 在位
    missing = [f"M4-B{i:02d}.md" for i in range(1, 41)
               if not os.path.isfile(os.path.join(DEEP, f"M4-B{i:02d}.md"))]
    if missing: errs.append(f"查1 缺册: {missing}")
    else: print("查1 40 册在位 PASS")
    # 2/3/4 逐册
    total = 0; j1r = 0
    for i in range(1, 41):
        path = os.path.join(DEEP, f"M4-B{i:02d}.md")
        s = io.open(path, encoding="utf-8").read()
        ids = re.findall(r"^### UNX-F(\d{5}) · ", s, re.M)
        exp = [50400 + (i-1)*20 + k + 1 for k in range(20)]
        if [int(x) for x in ids] != exp:
            errs.append(f"查2 B{i:02d} ID 不符: {ids[:3]}...")
        total += len(ids)
        blocks = re.split(r"^### UNX-F\d{5} · ", s, flags=re.M)[1:]
        for blk in blocks:
            body = blk.split("【判据】")[0]
            if len(body) < 300:
                errs.append(f"查3 B{i:02d} 正文 <300 字符: {blk[:24]}")
        if i in ETYPE:
            c = len(re.findall(r"(?m)^【反判据】", s))
            if c != 20: errs.append(f"查4 B{i:02d} J1R={c} != 20")
            j1r += c
    if not any(e.startswith("查2") for e in errs): print("查2 800 条 ID 逐一 PASS")
    if not any(e.startswith("查3") for e in errs): print("查3 每条 ≥300 字符 PASS")
    if not any(e.startswith("查4") for e in errs): print(f"查4 E 型 J1R 保留 PASS（{j1r}/160）")
    # 5 主册状态
    s = io.open(MAIN, encoding="utf-8").read()
    m4_ids = set(range(50401, 51201))
    done = sum(1 for fid in m4_ids if f"| 已深化 | UNX-F{fid}-J1" in s)
    residual = sum(1 for fid in m4_ids if f"| 增补 | UNX-F{fid}-J1" in s)
    if done != 800: errs.append(f"查5 已深化 {done}/800")
    if residual != 0: errs.append(f"查5 残留增补 {residual}")
    if done == 800 and residual == 0: print(f"查5 主册状态 800/800 已深化零残留 PASS")
    # 6 判据主轴
    if "断连重连 ×10 零手工重配" not in s: errs.append("查6 J-1 承载条目缺失")
    elif "六段延迟账总纲" not in s: errs.append("查6 J-2 承载条目缺失")
    else: print("查6 判据主轴 J-1/J-2 承载在册 PASS")
    # 7 I 型联签锚定
    i_ok = all(f"{d}向联签" in s for d in ["H1 音频桥","I3 共存","K4 电源","M5 解析器/管道","M2 键鼠","H5 手柄","J2/J3 凭据审计","L2/L3 出厂链"])
    if not i_ok: errs.append("查7 I 型联签域锚定缺失")
    else: print("查7 I 型八向联签锚定 PASS")
    # 8 关门印/终了/连续
    if "UNX-F51199" not in s or "UNX-F51200" not in s: errs.append("查8 域关门印/终了声明缺失")
    ids_in_main = set(int(x) for x in re.findall(r"\| UNX-F(5(?:0[4-9]|1[0-2])\d{2}) \|", s))
    full = set(range(50401, 51201))
    if not full <= ids_in_main: errs.append(f"查8 主册缺号: {sorted(full - ids_in_main)[:5]}")
    if "UNX-F51199" in s and "UNX-F51200" in s and full <= ids_in_main:
        print(f"查8 域关门印/终了声明在册 + ID 段 F50401–F51200 连续在主册 PASS")
    if errs:
        print("\n".join(errs)); sys.exit(1)
    print("ALL PASS exit=0")

if __name__ == "__main__":
    main()
