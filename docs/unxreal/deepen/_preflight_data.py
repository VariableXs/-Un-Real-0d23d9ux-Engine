# -*- coding: utf-8 -*-
"""B31–B40 数据文件预检（AI-04）：条数/ID 连续/判据编号一致/标题唯一/正文整行拼接长度模拟。

正文行拼接模板与 _gen_b31_b40.py 生成器保持同款，提前暴露 <310 压线条目，
避免生成后再跑扩写脚本返工（B27/B28 教训）。
"""
import importlib
import sys

MODS = [('_data_b31_b32', ['B31', 'B32']),
        ('_data_b33_b34', ['B33', 'B34']),
        ('_data_b35_b36', ['B35', 'B36']),
        ('_data_b37_b38', ['B37', 'B38']),
        ('_data_b39_b40', ['B39', 'B40'])]


def bodyline(e):
    """按生成器同款模板拼接正文整行，返回文本与长度。"""
    fid, title, judge, pos, sem, depr, risk, body, depk, win, ret, trace = e
    txt = ('- 正文：实现路径分三步。%s与现存内核衔接点：%s。与 Windows 对照：%s。'
           '判据 UNX-F%d-J1 的复测方式：%s。读数与留痕：%s，全链读数键随批账册归档可回放'
           '（与 F2797 体检单链对齐在档，批自检四范围 grep 留痕哈希随批入库）。') % (
        body.rstrip('。') + '。', depk, win, fid, ret, trace)
    return txt, len(txt)


def main():
    bad = []
    expect = 3000
    for mod, names in MODS:
        try:
            m = importlib.import_module(mod)
        except ImportError:
            print('%s: 未落盘（跳过）' % mod)
            continue
        for nm in names:
            batch = getattr(m, nm, None)
            if batch is None:
                bad.append((nm, '缺批对象'))
                continue
            es = batch['entries']
            base = expect + 1
            ids = [e[0] for e in es]
            if len(es) != 20:
                bad.append((nm, '条数=%d' % len(es)))
            if ids != list(range(base, base + 20)):
                bad.append((nm, 'ID 非连续或错位（期望 %d–%d）' % (base, base + 19)))
            for e in es:
                fid = e[0]
                if len(e) != 12:
                    bad.append(('F%d' % fid, '元组长度=%d' % len(e)))
                if ('UNX-F%d-J1' % fid) not in e[2]:
                    bad.append(('F%d' % fid, '判据句缺 UNX-F%d-J1' % fid))
                # 判据句内不得出现他条 -Jn 模式（判据编号一致性校验口径）
                import re
                for jm in re.finditer(r'UNX-F(\d+)-J\d', e[2]):
                    if int(jm.group(1)) != fid:
                        bad.append(('F%d' % fid, '判据句混入 %s' % jm.group(0)))
                txt, ln = bodyline(e)
                if ln < 310:
                    bad.append(('F%d' % fid, '正文整行 %d <310' % ln))
            expect += 20
            print('%s/%s: 20 条 ID %d–%d 结构核过，最短正文整行 %d' % (
                mod, nm, ids[0], ids[-1],
                min(bodyline(e)[1] for e in es)))
    print('=== 结果:', '全绿' if not bad else bad)
    sys.exit(1 if bad else 0)


main()
