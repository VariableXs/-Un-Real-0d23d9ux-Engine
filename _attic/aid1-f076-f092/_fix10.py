# -*- coding: utf-8 -*-
import io

def patch(p, subs):
    s = io.open(p, encoding='utf-8').read()
    for a, b in subs:
        assert a in s, f"NOT FOUND in {p}: {a[:70]}"
        s = s.replace(a, b)
    io.open(p, 'w', encoding='utf-8').write(s)

# conflict: 改回——直接删除 only-written 的 now_ms 字段（面板无动画无时钟需求）
patch('src/deskstar/conflict.rs', [
    ('''    decided: Vec<(usize, Decision)>,
    applied: Vec<Applied>,
    now_ms: u64,
    /// 「应用到全部」武装态（需二次点确认——防误伤）。''', '''    decided: Vec<(usize, Decision)>,
    applied: Vec<Applied>,
    /// 「应用到全部」武装态（需二次点确认——防误伤）。'''),
    ('''            decided: Vec::new(),
            applied: Vec::new(),
            now_ms: 0,
            apply_all_armed: false,''', '''            decided: Vec::new(),
            applied: Vec::new(),
            apply_all_armed: false,'''),
    ('''    pub fn decide(&mut self, idx: usize, d: Decision, now_ms: u64) -> Option<String> {
        self.now_ms = now_ms;
        let pair = self.pairs.get(idx)?.clone();''', '''    pub fn decide(&mut self, idx: usize, d: Decision) -> Option<String> {
        let pair = self.pairs.get(idx)?.clone();'''),
    ('''    /// 决策记忆（同类冲突默认策略——首次「应用到全部」的决策沉淀）。
    pub fn memoized(&self) -> Option<Decision> {
        self.memo
    }

    /// 面板最近决策时刻（诊断面：决策时间线锚）。
    pub fn last_decision_ms(&self) -> u64 {
        self.now_ms
    }''', '''    /// 决策记忆（同类冲突默认策略——首次「应用到全部」的决策沉淀）。
    pub fn memoized(&self) -> Option<Decision> {
        self.memo
    }'''),
])
print('conflict reverted to field-removal')
