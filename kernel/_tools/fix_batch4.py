#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""第四批收敛（check4 → 目标 0 错）。跳过并行会话热文件（vr09r/vr10r/veb259/vet08/ves41）。"""
import os

SRC = 'varix/src'


def patch(rel, pairs):
    p = os.path.join(SRC, rel)
    raw = open(p, encoding='utf-8', newline='').read()
    crlf = '\r\n' in raw
    s = raw.replace('\r\n', '\n')
    for old, new, cnt in pairs:
        c = s.count(old)
        if c == 0:
            if s.count(new) >= cnt:
                continue  # 已应用（幂等重跑）
            print('SKIP(外部已修或形态已变):', rel, repr(old[:50]))
            continue
        assert c == cnt, '%s: %r 计数 %d != %d' % (rel, old[:70], c, cnt)
        s = s.replace(old, new)
    if crlf:
        s = s.replace('\n', '\r\n')
    open(p, 'w', encoding='utf-8', newline='').write(s)
    print('OK', rel, '(%d 项)' % len(pairs))


def insert_leak(rel, linenos):
    p = os.path.join(SRC, rel)
    raw = open(p, encoding='utf-8', newline='').read()
    crlf = '\r\n' in raw
    s = raw.replace('\r\n', '\n')
    ls = s.split('\n')

    def ins(lines_1b, lineno):
        idx = lineno - 1
        text = '\n'.join(lines_1b)
        off = sum(len(x) + 1 for x in lines_1b[:idx])
        line = lines_1b[idx]
        pos = line.find('alloc::format!(')
        assert pos >= 0, 'L%d: %r' % (lineno, line[:120])
        d = off + pos + len('alloc::format!(')
        depth, k, n, in_str = 1, d, len(text), False
        while k < n and depth > 0:
            c = text[k]
            if in_str:
                if c == '\\':
                    k += 2
                    continue
                if c == '"':
                    in_str = False
            elif c == '"':
                in_str = True
            elif c == '(':
                depth += 1
            elif c == ')':
                depth -= 1
                if depth == 0:
                    break
            k += 1
        assert depth == 0, 'L%d 配对失败' % lineno
        text = text[:k + 1] + '.leak()' + text[k + 1:]
        return text.split('\n')

    for ln in sorted(linenos, reverse=True):
        ls = ins(ls, ln)
    s2 = '\n'.join(ls)
    assert s2.count('.leak()') >= len(linenos), rel
    if crlf:
        s2 = s2.replace('\n', '\r\n')
    open(p, 'w', encoding='utf-8', newline='').write(s2)
    print('OK', rel, '(leak x%d)' % len(linenos))


# --- svstar2（冷文件） ---
patch('svstar2/ven09_fuzz.rs', [
    ('#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]\npub enum AttackVerdict',
     '#[derive(Clone, Debug, Default, PartialEq, Eq)]\npub enum AttackVerdict', 1),
])
patch('svstar2/ves98_r05integ.rs', [
    ('.filter(|(_, &&b)| !b)', '.filter(|(_, &b)| !b)', 1),
])
patch('svstar2/ves102_scoreengine.rs', [
    ('Self { scores: [None; 5] }', 'Self { scores: [const { None }; 5] }', 1),
])
patch('svstar2/veb258_amddebug.rs', [
    ('''        let Some(c) = self.ch(id) else {
            return Err(XC_BAD_CMD);
        };
        // 降级通道：仅宿主侧只读，虚拟机侧命令全拒
        if c.degraded && !c.host_only {
            let r = Reply {
                cmd,
                seq: c.seq,
                ok: false,
                code: XC_DEGRADED,
                payload: "通道降级仅宿主侧".into(),
            };
            c.replies.push(r.clone());
            return Err(XC_DEGRADED);
        }
        // 配额（不跨通道挪用：各通道独立账）
        if c.quota.used >= c.quota.cap {
            let r = Reply { cmd, seq: c.seq, ok: false, code: XC_QUOTA, payload: "超配额".into() };
            c.replies.push(r.clone());
            return Err(XC_QUOTA);
        }
        c.quota.used += 1;
        c.seq += 1;
        let seq = c.seq;
        // 原始负载必须过 F0097 隐私过滤才出通道
        let filtered = privacy_filter(payload);
        let Some(fp) = filtered else {
            self.filter_fail += 1;
            let r = Reply { cmd, seq, ok: false, code: XC_PRIV, payload: "过滤失败不出通道".into() };
            c.replies.push(r.clone());
            return Err(XC_PRIV);
        };
        self.filtered_out.push(fp.clone());
        let r = Reply {
            cmd,
            seq,
            ok: true,
            code: 0,
            payload: format!("{}:{}", cmd_name(cmd), fp),
        };
        c.replies.push(r.clone());
        Ok(r)''',
     '''        // 降级/配额前置检查 + 配额记账（独立借段：与 self 自账字段互斥解耦）
        let seq = {
            let Some(c) = self.ch(id) else {
                return Err(XC_BAD_CMD);
            };
            // 降级通道：仅宿主侧只读，虚拟机侧命令全拒
            if c.degraded && !c.host_only {
                let r = Reply {
                    cmd,
                    seq: c.seq,
                    ok: false,
                    code: XC_DEGRADED,
                    payload: "通道降级仅宿主侧".into(),
                };
                c.replies.push(r.clone());
                return Err(XC_DEGRADED);
            }
            // 配额（不跨通道挪用：各通道独立账）
            if c.quota.used >= c.quota.cap {
                let r = Reply { cmd, seq: c.seq, ok: false, code: XC_QUOTA, payload: "超配额".into() };
                c.replies.push(r.clone());
                return Err(XC_QUOTA);
            }
            c.quota.used += 1;
            c.seq += 1;
            c.seq
        };
        // 原始负载必须过 F0097 隐私过滤才出通道
        let filtered = privacy_filter(payload);
        let Some(fp) = filtered else {
            self.filter_fail += 1;
            let c = self.ch(id).expect("前置检查已确认通道存在");
            let r = Reply { cmd, seq, ok: false, code: XC_PRIV, payload: "过滤失败不出通道".into() };
            c.replies.push(r.clone());
            return Err(XC_PRIV);
        };
        self.filtered_out.push(fp.clone());
        let c = self.ch(id).expect("前置检查已确认通道存在");
        let r = Reply {
            cmd,
            seq,
            ok: true,
            code: 0,
            payload: format!("{}:{}", cmd_name(cmd), fp),
        };
        c.replies.push(r.clone());
        Ok(r)''', 1),
])

# --- cgpu ---
patch('svstar2/veb250_amdcache.rs', [
    ('use crate::alloc::string::{String, ToString};\nuse crate::alloc::vec;\n',
     'use crate::alloc::string::{String, ToString};\n', 1),
])
patch('cgpu/cgb193_parsplit.rs', [
    ('contains(&ti as u32)', 'contains(&(ti as u32))', 1),
    ('#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub enum SplitStrategy',
     '#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]\npub enum SplitStrategy', 1),
])
patch('cgpu/cgb194_worksteal.rs', [
    ('''/// 窃取统计台账（判据「遥测」容器）。
#[derive(Default)]
pub struct StealTelemetry {''',
     '''/// 窃取统计台账（判据「遥测」容器）。
pub struct StealTelemetry {''', 1),
    ('''    /// 各核窃得件数。
    thief_counts: [u32; 64],
}''',
     '''    /// 各核窃得件数。
    thief_counts: [u32; 64],
}

impl core::default::Default for StealTelemetry {
    fn default() -> Self {
        StealTelemetry { events: Vec::new(), victim_counts: [0; 64], thief_counts: [0; 64] }
    }
}''', 1),
    ('overhead_us as u64 * (100 + hops * 30) / 100',
     'overhead_us as u64 * (100 + hops * 30) as u64 / 100', 1),
])
patch('cgpu/cgb277_frametele.rs', [
    ('mean < 13_000 &&', 'mean < 13_000.0 &&', 1),
])
patch('cgpu/cgb260_recover.rs', [
    ('''    let r_flip = RecoveryEngine::new().recover({
        let mut d = file.data.clone();
        let last = d.len() - 1;
        d[last] ^= 0x01;
        &d
    });''',
     '''    let mut d = file.data.clone();
    let last = d.len() - 1;
    d[last] ^= 0x01;
    let r_flip = RecoveryEngine::new().recover(&d);''', 1),
])
insert_leak('cgpu/cgb268_fuzz.rs', [290])

# --- unxstar ---
patch('unxstar/unx0133_badslot.rs', [
    ('use crate::alloc::{format, string::String, vec::Vec};',
     'use crate::alloc::{format, string::String, vec, vec::Vec};', 1),
])
patch('unxstar/unx0137_argedit.rs', [
    ('self.shadow.set(key, val.to_string());', 'self.shadow.set(key, val);', 1),
])
patch('unxstar/unx267_sym.rs', [
    ('''        self.symbols.iter()
            .find(|s| addr >= s.addr && addr < s.addr + s.size)
            .min_by_key(|s| addr - s.addr) // 最近匹配''',
     '''        self.symbols.iter()
            .filter(|s| addr >= s.addr && addr < s.addr + s.size)
            .min_by_key(|s| addr - s.addr) // 最近匹配''', 1),
])
patch('unxstar/unx272_session.rs', [
    ('#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub enum SessionEvent',
     '#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]\npub enum SessionEvent', 1),
])

# --- vestar ---
patch('vestar/ve188_perfbaseline.rs', [
    ('ok_or(0x8101)?', 'ok_or(0x8101u16)?', 1),
])
patch('vestar/ve189_memledger.rs', [
    ('ok_or(0x8201)?', 'ok_or(0x8201u16)?', 2),
])
patch('vestar/ve191_docgen.rs', [
    ('push_str(alloc::format!', 'push_str(&alloc::format!', 5),
])
patch('vestar/ve193_acceptance.rs', [
    ('push_str(alloc::format!', 'push_str(&alloc::format!', 4),
])
print('第四批完成')
