# -*- coding: utf-8 -*-
"""
unxreal_j2_skeleton_check.py — AI-47 · UNX-J2 首产段独立校验器（七查，复跑口径，不依赖生成器内存）
1) 卷标题在位 2) 15 批批头 3) 300 条 4) ID 连续零空洞 5) 批批求和 6,000 且总 90,000
6) 判据 300 枚唯一 7) 任务书锚五处归位 + 纯追加（相对 git HEAD 旧尾部零改动）
exit=0 即 ALL PASS。
"""
import os, re, subprocess, sys

ROOT = os.path.join(os.path.dirname(__file__), "..")
MD = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal",
                  "CoRun Varix STAR II · Unxreal.md")
VOL = "## 增补卷 · AI-47 · 波18 首产段 J2 域骨架立账"

def main():
    with open(MD, "r", encoding="utf-8") as f:
        text = f.read()
    # 1 卷标题
    assert VOL in text, "查1 失败：卷标题缺失"
    vol_idx = text.index(VOL)
    body = text[vol_idx:]
    # 2 批头
    batches = re.findall(r"^# UNX-J2-B(\d{2}) · ", body, re.M)
    assert batches == [f"{i:02d}" for i in range(1, 16)], f"查2 失败：批头 {batches}"
    # 3 条数
    entries = re.findall(r"^### UNX-F(\d+) · ", body, re.M)
    assert len(entries) == 300, f"查3 失败：条数 {len(entries)}"
    # 4 ID 连续
    ids = [int(x) for x in entries]
    assert ids == list(range(36801, 37101)), "查4 失败：ID 不连续"
    # 5 行数守恒（批内）
    per_batch = {}
    cur = None
    for line in body.splitlines():
        m = re.match(r"^# UNX-J2-B(\d{2}) · ", line)
        if m:
            cur = int(m.group(1)); per_batch[cur] = 0
        m2 = re.match(r"^- 域/批：J2/B(\d{2})｜纯功能行数：(\d+)｜", line)
        if m2:
            assert int(m2.group(1)) == cur, f"查5 失败：B{cur} 内出现 B{m2.group(1)} 条目"
            per_batch[cur] += int(m2.group(2))
    assert all(v == 6000 for v in per_batch.values()), f"查5 失败：批求和 {per_batch}"
    total = sum(per_batch.values())
    assert total == 90000, f"查5 失败：总行数 {total}"
    # 6 判据唯一
    crits = re.findall(r"UNX-F(\d+)-J1", body)
    assert len(crits) == 300 and len(set(crits)) == 300, "查6 失败：判据不唯一"
    # 7 锚归位
    for fid, b in {36805: 1, 36850: 3, 36901: 6, 36965: 9, 37050: 13}.items():
        m = re.search(rf"### UNX-F{fid} · .*（任务书示例锚·B{b:02d} 区间承载位）", body)
        assert m, f"查7 失败：锚 F{fid} 未归位 B{b:02d}"
    # 纯追加：git HEAD 版本须是当前文本的前缀
    old = subprocess.run(["git", "show", f"HEAD:{os.path.relpath(MD, ROOT).replace(os.sep, '/')}"],
                         cwd=ROOT, capture_output=True)
    if old.returncode == 0:
        old_text = old.stdout.decode("utf-8")
        assert text.startswith(old_text), "查7 失败：非纯追加（旧内容被改动）"
    print("ALL PASS exit=0：卷标题/15批/300条/ID连续/批批6000·总90,000/判据唯一/锚归位+纯追加 七查全绿")

if __name__ == "__main__":
    main()
