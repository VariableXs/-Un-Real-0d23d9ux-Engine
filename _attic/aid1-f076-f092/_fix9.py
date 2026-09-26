# -*- coding: utf-8 -*-
import io

def patch(p, subs):
    s = io.open(p, encoding='utf-8').read()
    for a, b in subs:
        assert a in s, f"NOT FOUND in {p}: {a[:70]}"
        s = s.replace(a, b)
    io.open(p, 'w', encoding='utf-8').write(s)

# calflyout: 删除未用绑定
patch('src/deskstar/calflyout.rs', [('    // 尾补位：次月开头。\n    let nm = next_month(year, month).1;\n', '    // 尾补位：次月开头（日号按位置重排——次月年月号无需显式取）。\n')])
# livesearch: built_s 增加读取口 + 自检消费
patch('src/deskstar/livesearch.rs', [
    ('''    pub fn is_stale(&self) -> bool {
        self.stale
    }''', '''    pub fn is_stale(&self) -> bool {
        self.stale
    }

    /// 建索引时刻（秒）——索引新鲜度对账面。
    pub fn built_s(&self) -> u64 {
        self.built_s
    }'''),
    ('''    let in_budget = ls.first_hit_in_budget();
    set.add(
        "first-hit-300ms",
        in_budget && !ls.hits().is_empty(),
        "<300ms @5000 files",
    );''', '''    let in_budget = ls.first_hit_in_budget();
    let fresh = ls.index.as_ref().map(|i| i.built_s() == 1 && !i.is_stale()) == Some(true);
    set.add(
        "first-hit-300ms",
        in_budget && fresh && !ls.hits().is_empty(),
        "<300ms @5000 files",
    );'''),
])
# trashui: now_ms 参数未读——full_choice 驱动 toast 时刻入账
patch('src/deskstar/trashui.rs', [
    ('''    pub fn full_choice(&mut self, volume: &str, choice: FullChoice, now_ms: u64) -> bool {
        match choice {''', '''    pub fn full_choice(&mut self, volume: &str, choice: FullChoice, now_ms: u64) -> bool {
        self.now_ms = now_ms; // 三选执行时刻入账（toast 时间线锚）
        match choice {'''),
])
# conflict: 删除未用 ok_if
patch('src/deskstar/conflict.rs', [('''fn ok_if(set: &mut CheckSet, group: &'static str, ok: bool, detail: &'static str) {
    set.add(group, ok, detail);
}
''', '')])
# sndfx: 字段蛇形命名
patch('src/deskstar/sndfx.rs', [
    ('    /// 归一增益（毫分贝——按条目音量与 -18LUFS 目标折算）。\n    gain_mdB: i32,', '    /// 归一增益（毫分贝——按条目音量与 -18LUFS 目标折算）。\n    gain_md_b: i32,'),
    ('            gain_mdB: Self::gain_md_b(self.entries[event.index()].volume),', '            gain_md_b: Self::gain_md_b(self.entries[event.index()].volume),'),
])
# conflict now_ms 字段未读：加只读口并自检消费
patch('src/deskstar/conflict.rs', [
    ('''    /// 决策记忆（同类冲突默认策略——首次「应用到全部」的决策沉淀）。
    pub fn memoized(&self) -> Option<Decision> {
        self.memo
    }''', '''    /// 决策记忆（同类冲突默认策略——首次「应用到全部」的决策沉淀）。
    pub fn memoized(&self) -> Option<Decision> {
        self.memo
    }

    /// 面板最近决策时刻（诊断面：决策时间线锚）。
    pub fn last_decision_ms(&self) -> u64 {
        self.now_ms
    }'''),
    ('''    pub fn decide(&mut self, idx: usize, d: Decision) -> Option<String> {
        let pair = self.pairs.get(idx)?.clone();''', '''    pub fn decide(&mut self, idx: usize, d: Decision, now_ms: u64) -> Option<String> {
        self.now_ms = now_ms;
        let pair = self.pairs.get(idx)?.clone();'''),
    ('''            self.decided.push((idx, d));
        }
        placed
    }

    /// 只读目标二选：强制（清除只读后覆盖）或跳过。''', '''            self.decided.push((idx, d));
        }
        placed
    }

    /// 只读目标二选：强制（清除只读后覆盖）或跳过。'''),
])
# copydlg: cur_permille/current_name 进自检消费
patch('src/deskstar/copydlg.rs', [
    ('''    m2.progress(id2, 400, 0, 0);
    let p = m2.pause(id2);''', '''    m2.progress(id2, 400, 0, 0);
    let cur_ok = {
        let t = m2.task(id2).unwrap();
        t.cur_permille() == 400 && t.current_name() == Some("a")
    };
    let p = m2.pause(id2);'''),
    ('''    set.add("pause-resume", p && frozen && r && done, "pause/resume keep");''',
     '''    set.add("pause-resume", p && frozen && r && done && cur_ok, "pause/resume keep");'''),
])
print('pass8 ok')
