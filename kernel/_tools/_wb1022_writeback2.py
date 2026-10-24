#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""AI-WBB-1022 第二批 266 卡写回工具（流式，防 MemoryError）。
用法：python _tools/_wb1022_writeback2.py <taskboard.md>
写回规则：
  - 状态: 已领 -> 已完成
  - 结果: (空) -> 落点摘要（文件 · 行数）
  - 时间: 保留原领取时间不动；完成后追加完成时间到结果行
仅处理 _wb1022_claims2.txt 列出的 266 张卡且领取人= AI-WBB-1022。
"""
import io, os, re, sys, glob, datetime

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))  # kernel/
CLAIMS = os.path.join(ROOT, '_tools', '_wb1022_claims2.txt')

def load_claims():
    m = {}
    with io.open(CLAIMS, 'r', encoding='utf-8') as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            parts = line.split('|')
            if len(parts) >= 2:
                m[parts[0]] = parts[1]
    return m

def locate(fcid):
    """FCID -> 相对 kernel 的代码文件路径 or None"""
    dom, num = fcid.split('-F')
    num = int(num)
    if dom == 'UNX':
        pats = glob.glob(os.path.join(ROOT, 'varix/src/unxstar/unx%04d_*.rs' % num))
    elif dom == 'VE':
        pats = glob.glob(os.path.join(ROOT, 'varix/src/svstar2/veb%d_*.rs' % num))
    elif dom == 'CGPU':
        pats = glob.glob(os.path.join(ROOT, 'varix/src/cgpu/cgb%d_*.rs' % num))
    else:
        return None
    return pats[0] if pats else None

def file_info(path):
    if not path or not os.path.exists(path):
        return None
    n_lines = 0
    n_checks = 0
    with io.open(path, 'r', encoding='utf-8', errors='ignore') as f:
        for line in f:
            n_lines += 1
            if '.add(' in line and ('cs.add' in line or 's.add(' in line):
                n_checks += 1
    rel = os.path.relpath(path, os.path.dirname(ROOT))
    return rel.replace('\\', '/'), n_lines, n_checks

def writeback(board):
    claims = load_claims()
    ts = '10-22 20:50'
    tmp = board + '.tmp_wb1022'
    done = 0
    missing = []
    cur_id = None
    cur_owner = None
    state_wrote = False
    result_wrote = False
    n = 0
    with io.open(board, 'r', encoding='utf-8') as fin, \
         io.open(tmp, 'w', encoding='utf-8', newline='') as fout:
        for line in fin:
            n += 1
            m = re.match(r'^##\s+\[([A-Z]+-F\d+)\]', line)
            if m:
                # 卡边界：刷状态检查留给字段行处理
                cur_id = m.group(1)
                cur_owner = None
                state_wrote = False
                result_wrote = False
                fout.write(line)
                continue
            if cur_id and cur_id in claims:
                mo = re.match(r'^-\s*领取人[:：]\s*(.+)', line)
                if mo:
                    cur_owner = mo.group(1).strip()
                    fout.write(line)
                    continue
                if cur_owner == 'AI-WBB-1022':
                    ms = re.match(r'^(-\s*状态[:：]\s*)已领\s*$', line)
                    if ms and not state_wrote:
                        fout.write(ms.group(1) + '已完成\n')
                        state_wrote = True
                        continue
                    mr = re.match(r'^-\s*结果[:：]\s*$', line)
                    if mr and not result_wrote:
                        info = file_info(locate(cur_id))
                        if info:
                            rel, nl, nc = info
                            fout.write('- 结果: %s · %d 行 · 判据 %d 条（AI-WBB-1022 %s 完成）\n' % (rel, nl, nc, ts))
                        else:
                            missing.append(cur_id)
                            fout.write(line)
                        result_wrote = True
                        continue
            fout.write(line)
    os.replace(tmp, board)
    return done, missing

def main():
    boards = [
        os.path.join(os.path.dirname(ROOT), 'VarixTaskOps/VTaskBoard/taskboard.md'),
        os.path.join(os.path.dirname(ROOT), 'dist/taskboard.md'),
    ]
    for b in boards:
        if not os.path.exists(b):
            print('MISS', b)
            continue
        sz = os.path.getsize(b)
        if sz == 0:
            print('EMPTY-ABORT', b)
            continue
        # 备份
        bak = b + '.bak_wb1022_post'
        if not os.path.exists(bak):
            with io.open(b, 'r', encoding='utf-8') as fi, io.open(bak, 'w', encoding='utf-8', newline='') as fo:
                for ln in fi:
                    fo.write(ln)
        d, miss = writeback(b)
        sz2 = os.path.getsize(b)
        print('OK', b, 'size', sz, '->', sz2, 'missing', len(miss))
        if miss:
            print('MISSING-CODE', miss[:10])

if __name__ == '__main__':
    main()
