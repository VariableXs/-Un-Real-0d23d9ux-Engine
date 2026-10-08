import re, sys, io
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8')
BASE = r"d:/2/14/-Un-Real-0d23d9ux-Engine-main/docs/unxreal"

def hanzi(s): return len(re.findall(r'[\u4e00-\u9fff]', s))

def parse(path):
    entries = {}
    order = []
    cur = None
    with open(path, encoding='utf-8') as f:
        for line in f:
            m = re.match(r'^### (UNX-F\d+) · (.+?)\s*$', line)
            if m:
                cur = m.group(1); entries[cur] = {'name': m.group(2).strip()}; order.append(cur); continue
            if cur:
                if '判据：UNX-F' not in entries[cur] and re.search(r'判据：(UNX-F\d+-J\d+)\s', line):
                    entries[cur]['crit'] = re.search(r'判据：(UNX-F\d+-J\d+.*)$', line.rstrip('\n')).group(1).strip()
                lm = re.search(r'纯功能行数：(\d+)', line)
                if lm: entries[cur].setdefault('lines', int(lm.group(1)))
                if line.startswith('- 正文：'): entries[cur]['body'] = line
    return order, entries

ok = True
for batch, fs, fe, total in [('B39', 8761, 8780, 8700), ('B40', 8781, 8800, 8690)]:
    sk_order, sk = parse(f"{BASE}/batches/UNX-C1-{batch}.md")
    dp_order, dp = parse(f"{BASE}/deepen/C1-{batch}.md")
    # ① ID 连续唯一
    ids = [int(x[1:]) for x in sk_order]
    assert len(set(ids)) == len(ids), f"{batch} ID 重复"
    assert ids == list(range(fs, fe+1)), f"{batch} ID 不连续: {ids[:3]}...{ids[-3:]}"
    print(f"[{batch}] ① ID 连续唯一 {fs}–{fe}（{len(ids)} 条）: PASS")
    # ② 行数求和
    s = sum(sk[i]['lines'] for i in sk_order)
    st = "PASS" if s == total else "FAIL"
    print(f"[{batch}] ② 行数求和 {s} == {total}: {st}")
    if s != total: ok = False
    # 行数范围 380-480
    bad = [i for i in sk_order if not (380 <= sk[i]['lines'] <= 480)]
    print(f"[{batch}]    行数范围 380–480: {'PASS' if not bad else 'FAIL '+str(bad)}")
    if bad: ok = False
    # ③ 骨架/深化 ID+判据+条目名+行数一致
    assert sk_order == dp_order, f"{batch} 两册 ID 序不一致"
    diffs = []
    for i in sk_order:
        if sk[i]['name'] != dp[i]['name']: diffs.append((i,'name'))
        c1 = sk[i].get('crit',''); c2 = dp[i].get('crit','')
        # 深化册判据行含行数后缀，取判据字段比对（判据：...｜纯功能行数之前）
        c2 = c2.split('｜纯功能行数')[0].strip()
        if c1 != c2: diffs.append((i,'crit', c1[:40], c2[:40]))
        if sk[i]['lines'] != dp[i]['lines']: diffs.append((i,'lines'))
    print(f"[{batch}] ③ 骨架/深化同名同判据同行数: {'PASS' if not diffs else 'FAIL '+str(diffs)}")
    if diffs: ok = False
    # 行数分解之和 == 纯功能行数
    badsum = []
    for i in sk_order:
        with open(f"{BASE}/deepen/C1-{batch}.md", encoding='utf-8') as f: pass
    # parse decomposition from deepen file
    text = open(f"{BASE}/deepen/C1-{batch}.md", encoding='utf-8').read()
    for m in re.finditer(r'### (UNX-F\d+).*?纯功能行数：(\d+) 行（(.+?)；测试段不计）', text, re.S):
        fid, n, dec = m.group(1), int(m.group(2)), m.group(3)
        parts = [int(x) for x in re.findall(r'(\d+)\s*(?=[+；]|$)', dec)]
        if sum(parts) != n: badsum.append((fid, sum(parts), n))
    print(f"[{batch}]    三段分解之和==纯功能行数: {'PASS' if not badsum else 'FAIL '+str(badsum)}")
    if badsum: ok = False
    # ④ 正文汉字数 >= 300
    mins = []
    badh = []
    for i in sk_order:
        h = hanzi(dp[i].get('body',''))
        mins.append(h)
        if h < 300: badh.append((i, h))
    print(f"[{batch}] ④ 正文汉字数≥300: 最小 {min(mins)} — {'PASS' if not badh else 'FAIL '+str(badh)}")
    if badh: ok = False
print("ALL GREEN" if ok else "HAS FAILURES")
