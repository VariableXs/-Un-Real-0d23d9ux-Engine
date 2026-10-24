//! VE-F3744 · 创作认知无障碍
//!
//! 锚点：创作认知无障碍（认知障碍创作者支持：工作流简化（复杂创作流程简化路径
//! （向导复用全域化（复述；术语简化（专业术语通俗解释（术语词典（术语红线：
//! 术语不解释=排除新手认知（解释断言（关键术语 hover 释义覆盖；认知负荷
//! （创作界面同时活动元素≤3（复用 P 域红线全域化执法——负荷红线复述全域；
//! 时间宽容（创作操作不限时+可保存进度（进度保存复述。
//!
//! 数据结构：简化路径；术语解释覆盖断言；负荷执法复用；宽容复述。
//! 错误路径与降级矩阵：术语无释义→断言拦截（红线实测——关键术语扫描）；
//! 负荷超限→整改（复述红线实测）；强制限时→修正（复述）；简化缺→补（复述）。
//! 跨批对接点：F3147/F3026 复用单源声明；R01 向导对端；F3622 编辑对端。
//!
//! 诊断码段：独占 0x87xx。

#![allow(dead_code)]

use alloc::string::String;
extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0x87xx）
// ─────────────────────────────────────────────────────────────

/// F3744 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C7Code(pub u16);

impl C7Code {
    /// 关键术语无释义——红线实测拦截。
    pub const TERM_NO_EXPLAIN: C7Code = C7Code(0x8701);
    /// 同时活动元素超 3——负荷红线复述全域执法。
    pub const LOAD_OVER: C7Code = C7Code(0x8702);
    /// 强制限时——时间宽容红线。
    pub const FORCE_TIMER: C7Code = C7Code(0x8703);
    /// 简化路径缺失——向导复用断言。
    pub const SIMPLIFY_MISS: C7Code = C7Code(0x8704);
    /// 进度不可保存——宽容复述。
    pub const PROGRESS_NO_SAVE: C7Code = C7Code(0x8705);
    /// 术语词典漂移——单源对账。
    pub const DICT_DRIFT: C7Code = C7Code(0x8706);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0x8701 => "关键术语无释义，排除新手认知，红线拦截。",
            0x8702 => "同时活动元素超 3，负荷红线复述全域执法不通过。",
            0x8703 => "存在强制限时，违反时间宽容红线。",
            0x8704 => "复杂流程缺简化路径，向导复用断言不通过。",
            0x8705 => "进度不可保存，宽容复述不完整。",
            0x8706 => "术语词典与单源声明漂移，需对账归一。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 术语词典与释义覆盖断言
// ─────────────────────────────────────────────────────────────

/// 单条术语。
#[derive(Clone, Debug)]
pub struct TermEntry {
    /// 术语标识。
    pub id: u32,
    /// 术语原文。
    pub name: alloc::string::String,
    /// 是否关键术语（关键术语必须有释义）。
    pub critical: bool,
    /// 通俗释义（空=无释义）。
    pub plain: alloc::string::String,
    /// hover 释义是否已挂载。
    pub hover_bound: bool,
}

impl TermEntry {
    /// 构造。
    pub fn new(id: u32, name: &str, critical: bool) -> TermEntry {
        TermEntry {
            id,
            name: alloc::string::String::from(name),
            critical,
            plain: alloc::string::String::new(),
            hover_bound: false,
        }
    }

    /// 释义有效：非空。
    pub fn has_plain(&self) -> bool {
        !self.plain.is_empty()
    }

    /// 关键术语合规：有释义 + hover 已挂。
    pub fn critical_ok(&self) -> bool {
        if !self.critical {
            return true;
        }
        self.has_plain() && self.hover_bound
    }
}

/// 术语词典。
#[derive(Clone, Debug, Default)]
pub struct TermDict {
    entries: Vec<TermEntry>,
    /// 单源标识（与 F3147/F3026 对账）。
    source_tag: u64,
}

impl TermDict {
    /// 空词典。
    pub fn new() -> TermDict {
        TermDict {
            entries: Vec::new(),
            source_tag: 0,
        }
    }

    /// 带单源标识。
    pub fn with_source(source_tag: u64) -> TermDict {
        TermDict {
            entries: Vec::new(),
            source_tag,
        }
    }

    /// 登记术语。
    pub fn register(&mut self, entry: TermEntry) -> bool {
        // 同 id 去重
        for e in self.entries.iter() {
            if e.id == entry.id {
                return false;
            }
        }
        self.entries.push(entry);
        true
    }

    /// 挂释义。
    pub fn attach_plain(&mut self, id: u32, plain: &str) -> bool {
        for e in self.entries.iter_mut() {
            if e.id == id {
                e.plain = alloc::string::String::from(plain);
                return true;
            }
        }
        false
    }

    /// 挂 hover。
    pub fn bind_hover(&mut self, id: u32) -> bool {
        for e in self.entries.iter_mut() {
            if e.id == id {
                e.hover_bound = true;
                return true;
            }
        }
        false
    }

    /// 扫描关键术语红线：返回违规 id 列表。
    pub fn scan_critical_gap(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for e in self.entries.iter() {
            if e.critical && !e.critical_ok() {
                bad.push(e.id);
            }
        }
        bad
    }

    /// 术语数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空判断。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 单源标识。
    pub fn source_tag(&self) -> u64 {
        self.source_tag
    }

    /// 取条目。
    pub fn get(&self, id: u32) -> Option<&TermEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// 与另一词典对账（单源一致性）：返回漂移 id 对。
    pub fn reconcile(&self, other: &TermDict) -> Vec<(u32, u32)> {
        let mut drift = Vec::new();
        for e in self.entries.iter() {
            match other.get(e.id) {
                Some(oe) => {
                    if oe.name != e.name || oe.critical != e.critical {
                        drift.push((e.id, e.id));
                    }
                }
                None => drift.push((e.id, 0)),
            }
        }
        for oe in other.entries.iter() {
            if self.get(oe.id).is_none() {
                drift.push((0, oe.id));
            }
        }
        drift
    }
}

// ─────────────────────────────────────────────────────────────
// 认知负荷执法（同时活动元素 ≤ 3，复用 P 域红线全域化）
// ─────────────────────────────────────────────────────────────

/// 活动元素计量器。
#[derive(Clone, Debug, Default)]
pub struct LoadMeter {
    /// 当前活动元素数。
    active: u32,
    /// 红线阈值（固定 3）。
    limit: u32,
}

impl LoadMeter {
    /// 构造（阈值恒为 3）。
    pub fn new() -> LoadMeter {
        LoadMeter { active: 0, limit: 3 }
    }

    /// 增加活动元素。
    pub fn activate(&mut self) -> bool {
        if self.active >= self.limit {
            return false;
        }
        self.active += 1;
        true
    }

    /// 减少。
    pub fn deactivate(&mut self) -> bool {
        if self.active == 0 {
            return false;
        }
        self.active -= 1;
        true
    }

    /// 当前负荷。
    pub fn current(&self) -> u32 {
        self.active
    }

    /// 阈值。
    pub fn threshold(&self) -> u32 {
        self.limit
    }

    /// 是否超限。
    pub fn over(&self) -> bool {
        self.active > self.limit
    }

    /// 是否恰在红线。
    pub fn at_limit(&self) -> bool {
        self.active == self.limit
    }

    /// 强制设值（用于外部状态恢复，超限返回 false 不设）。
    pub fn set_active(&mut self, n: u32) -> bool {
        if n > self.limit {
            return false;
        }
        self.active = n;
        true
    }

    /// 重置。
    pub fn reset(&mut self) {
        self.active = 0;
    }
}

// ─────────────────────────────────────────────────────────────
// 时间宽容（创作操作不限时 + 可保存进度）
// ─────────────────────────────────────────────────────────────

/// 宽容状态。
#[derive(Clone, Debug, Default)]
pub struct TimeGrace {
    /// 是否存在强制限时。
    force_timer: bool,
    /// 是否支持进度保存。
    save_supported: bool,
    /// 最近保存序号（0=未保存）。
    save_seq: u64,
}

impl TimeGrace {
    /// 构造。
    pub fn new() -> TimeGrace {
        TimeGrace {
            force_timer: false,
            save_supported: true,
            save_seq: 0,
        }
    }

    /// 检出强制限时（违规入口，用于演练注入）。
    pub fn inject_force_timer(&mut self) {
        self.force_timer = true;
    }

    /// 修正：清除强制限时。
    pub fn fix_force_timer(&mut self) {
        self.force_timer = false;
    }

    /// 保存进度。
    pub fn save(&mut self) -> u64 {
        self.save_seq += 1;
        self.save_seq
    }

    /// 最近保存。
    pub fn last_save(&self) -> u64 {
        self.save_seq
    }

    /// 合规：无强制限时 + 可保存。
    pub fn ok(&self) -> bool {
        !self.force_timer && self.save_supported
    }

    /// 是否强制限时。
    pub fn has_force_timer(&self) -> bool {
        self.force_timer
    }

    /// 设置可保存。
    pub fn set_save_supported(&mut self, v: bool) {
        self.save_supported = v;
    }
}

// ─────────────────────────────────────────────────────────────
// 简化路径（向导复用全域化）
// ─────────────────────────────────────────────────────────────

/// 简化路径条目。
#[derive(Clone, Debug)]
pub struct SimplifyPath {
    /// 流程标识。
    pub flow_id: u32,
    /// 是否复杂流程（必须有简化路径）。
    pub complex: bool,
    /// 简化路径步数（0=无简化）。
    pub steps: u32,
    /// 是否复用向导（R01 对端）。
    pub reuses_wizard: bool,
}

impl SimplifyPath {
    /// 构造。
    pub fn new(flow_id: u32, complex: bool) -> SimplifyPath {
        SimplifyPath {
            flow_id,
            complex,
            steps: 0,
            reuses_wizard: false,
        }
    }

    /// 简化有效：复杂流程必须有步数且复用向导。
    pub fn ok(&self) -> bool {
        if !self.complex {
            return true;
        }
        self.steps > 0 && self.reuses_wizard
    }
}

/// 简化路径登记表。
#[derive(Clone, Debug, Default)]
pub struct SimplifyRegistry {
    paths: Vec<SimplifyPath>,
}

impl SimplifyRegistry {
    /// 空表。
    pub fn new() -> SimplifyRegistry {
        SimplifyRegistry { paths: Vec::new() }
    }

    /// 登记。
    pub fn add(&mut self, p: SimplifyPath) {
        self.paths.push(p);
    }

    /// 设步数。
    pub fn set_steps(&mut self, flow_id: u32, steps: u32) -> bool {
        for p in self.paths.iter_mut() {
            if p.flow_id == flow_id {
                p.steps = steps;
                return true;
            }
        }
        false
    }

    /// 挂向导复用。
    pub fn bind_wizard(&mut self, flow_id: u32) -> bool {
        for p in self.paths.iter_mut() {
            if p.flow_id == flow_id {
                p.reuses_wizard = true;
                return true;
            }
        }
        false
    }

    /// 扫描缺失简化路径的复杂流程。
    pub fn scan_missing(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for p in self.paths.iter() {
            if p.complex && !p.ok() {
                bad.push(p.flow_id);
            }
        }
        bad
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.paths.len()
    }

    /// 空判断。
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────
// 主编排器
// ─────────────────────────────────────────────────────────────

/// 创作认知无障碍审查器。
#[derive(Clone, Debug, Default)]
pub struct CogA11yAuditor {
    dict: TermDict,
    load: LoadMeter,
    grace: TimeGrace,
    simplify: SimplifyRegistry,
}

impl CogA11yAuditor {
    /// 构造。
    pub fn new() -> CogA11yAuditor {
        CogA11yAuditor {
            dict: TermDict::new(),
            load: LoadMeter::new(),
            grace: TimeGrace::new(),
            simplify: SimplifyRegistry::new(),
        }
    }

    /// 带单源标识。
    pub fn with_source(tag: u64) -> CogA11yAuditor {
        CogA11yAuditor {
            dict: TermDict::with_source(tag),
            load: LoadMeter::new(),
            grace: TimeGrace::new(),
            simplify: SimplifyRegistry::new(),
        }
    }

    /// 取词典（可变）。
    pub fn dict_mut(&mut self) -> &mut TermDict {
        &mut self.dict
    }

    /// 取词典（只读）。
    pub fn dict(&self) -> &TermDict {
        &self.dict
    }

    /// 取负荷表。
    pub fn load(&self) -> &LoadMeter {
        &self.load
    }

    /// 取负荷表（可变）。
    pub fn load_mut(&mut self) -> &mut LoadMeter {
        &mut self.load
    }

    /// 取宽容。
    pub fn grace(&self) -> &TimeGrace {
        &self.grace
    }

    /// 取宽容（可变）。
    pub fn grace_mut(&mut self) -> &mut TimeGrace {
        &mut self.grace
    }

    /// 取简化表。
    pub fn simplify(&self) -> &SimplifyRegistry {
        &self.simplify
    }

    /// 取简化表（可变）。
    pub fn simplify_mut(&mut self) -> &mut SimplifyRegistry {
        &mut self.simplify
    }

    /// 全量审查：返回违规码列表（空=通过）。
    pub fn audit(&self) -> Vec<C7Code> {
        let mut bad = Vec::new();
        if !self.dict.scan_critical_gap().is_empty() {
            bad.push(C7Code::TERM_NO_EXPLAIN);
        }
        if self.load.over() {
            bad.push(C7Code::LOAD_OVER);
        }
        if self.grace.has_force_timer() {
            bad.push(C7Code::FORCE_TIMER);
        }
        if !self.simplify.scan_missing().is_empty() {
            bad.push(C7Code::SIMPLIFY_MISS);
        }
        if self.grace.last_save() == 0 && !self.grace.ok() {
            bad.push(C7Code::PROGRESS_NO_SAVE);
        }
        bad
    }

    /// 一键整改：返回整改动作数。
    pub fn remediate(&mut self) -> u32 {
        let mut n = 0;
        if self.grace.has_force_timer() {
            self.grace.fix_force_timer();
            n += 1;
        }
        while self.load.over() {
            self.load.deactivate();
            n += 1;
        }
        n
    }
}

// ─────────────────────────────────────────────────────────────
// 判据
// ─────────────────────────────────────────────────────────────

/// F3744 判据集。
pub fn run_vr08d_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3744");

    // 0x8701 术语红线
    {
        let mut d = TermDict::new();
        d.register(TermEntry::new(1, "令牌", true));
        let gaps = d.scan_critical_gap();
        out.add(
            "0x8701-术语红线拦",
            gaps.len() == 1 && gaps[0] == 1,
            "",
        );
        d.attach_plain(1, "界面视觉变量的统称");
        d.bind_hover(1);
        out.add("0x8701-释义挂后过", d.scan_critical_gap().is_empty(), "");
    }

    // 0x8702 负荷红线
    {
        let mut m = LoadMeter::new();
        let ok3 = m.activate() && m.activate() && m.activate();
        out.add("0x8702-三元素恰红线", ok3 && m.at_limit() && !m.over(), "");
        let rejected = !m.activate();
        out.add("0x8702-超限拒收", rejected && m.current() == 3, "");
        m.set_active(3);
        out.add("0x8702-设值超限拒", !m.set_active(4), "");
    }

    // 0x8703 时间宽容
    {
        let mut g = TimeGrace::new();
        out.add("0x8703-初始合规", g.ok(), "");
        g.inject_force_timer();
        out.add("0x8703-强制限时检出", !g.ok() && g.has_force_timer(), "");
        g.fix_force_timer();
        out.add("0x8703-修正后过", g.ok(), "");
    }

    // 0x8704 简化路径
    {
        let mut r = SimplifyRegistry::new();
        r.add(SimplifyPath::new(10, true));
        out.add("0x8704-缺简化拦", r.scan_missing().len() == 1, "");
        r.set_steps(10, 3);
        r.bind_wizard(10);
        out.add("0x8704-补全后过", r.scan_missing().is_empty(), "");
    }

    // 0x8705 进度保存
    {
        let mut g = TimeGrace::new();
        g.set_save_supported(true);
        let s1 = g.save();
        out.add("0x8705-保存递增", s1 == 1 && g.last_save() == 1, "");
    }

    // 0x8706 词典对账
    {
        let mut a = TermDict::with_source(0xAA);
        let mut b = TermDict::with_source(0xAA);
        a.register(TermEntry::new(1, "画布", true));
        b.register(TermEntry::new(1, "画布", true));
        out.add("0x8706-一致无漂移", a.reconcile(&b).is_empty(), "");
        b.register(TermEntry::new(2, "图层", true));
        out.add("0x8706-缺项检出", a.reconcile(&b).len() == 1, "");
    }

    // 判据
    {
        let mut a = CogA11yAuditor::new();
        out.add("0x8706-初始全过", a.audit().is_empty(), "");
        a.load_mut().activate();
        a.load_mut().activate();
        a.load_mut().activate();
        a.load_mut().activate();
        let bad = a.audit();
        out.add(
            "0x8702-审计检出负荷",
            bad.iter().any(|c| c.raw() == 0x8702),
            "",
        );
        let n = a.remediate();
        out.add("0x8703-整改有动作", n > 0 && a.audit().is_empty(), "");
    }

    out
}
