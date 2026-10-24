//! VE-F3787 · R 域体验日志与观测终审（R10 收口组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3787`
//!
//! **判据（锚点原文）**：四信号单源、二十成员、中心实测、隐私复验、
//! 复用八用、判据。
//!
//! **本条是什么**：八组观测统一性终审——埋点抽查/信号口径（R 四信号：
//! 创作卡顿/心流中断/市场困惑/辅具断供单源）/协议一致（家族二十成员
//! 断言）/总中心实测。
//!
//! 诊断码段：独占 0xB2xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xB2xx）
// ─────────────────────────────────────────────────────────────

/// F3787 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct B2Code(pub u16);

impl B2Code {
    /// 埋点缺——补。
    pub const TRACK_MISS: B2Code = B2Code(0xB201);
    /// 口径分叉——单源。
    pub const FORK: B2Code = B2Code(0xB202);
    /// 协议不一——修正。
    pub const PROTO_MISMATCH: B2Code = B2Code(0xB203);
    /// 中心失灵——实测。
    pub const CENTER_DOWN: B2Code = B2Code(0xB204);
    /// 四信号缺信号。
    pub const SIGNAL_MISS: B2Code = B2Code(0xB205);
    /// 成员缺协议。
    pub const MEMBER_MISS: B2Code = B2Code(0xB206);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xB201 => "埋点缺失，需补齐。",
            0xB202 => "信号口径分叉，须归一单源。",
            0xB203 => "家族成员协议不一致。",
            0xB204 => "观测总中心失灵，需实测。",
            0xB205 => "四信号缺信号。",
            0xB206 => "二十成员缺协议登记。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// R 四信号单源
// ─────────────────────────────────────────────────────────────

/// R 信号（四信号闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RSignal {
    /// 创作卡顿。
    CreateLag,
    /// 心流中断。
    FlowBreak,
    /// 市场困惑。
    MarketConfuse,
    /// 辅具断供。
    AssistCut,
}

impl RSignal {
    /// 全集。
    pub const ALL: [RSignal; 4] = [
        RSignal::CreateLag,
        RSignal::FlowBreak,
        RSignal::MarketConfuse,
        RSignal::AssistCut,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            RSignal::CreateLag => 0,
            RSignal::FlowBreak => 1,
            RSignal::MarketConfuse => 2,
            RSignal::AssistCut => 3,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            RSignal::CreateLag => "创作卡顿",
            RSignal::FlowBreak => "心流中断",
            RSignal::MarketConfuse => "市场困惑",
            RSignal::AssistCut => "辅具断供",
        }
    }
}

/// 信号口径登记（单源：每信号只允许一个定义）。
#[derive(Clone, Debug)]
pub struct SignalRegistry {
    /// 每信号定义源 id（0=未定义）。
    defs: [u16; 4],
}

impl SignalRegistry {
    /// 新建。
    pub const fn new() -> Self {
        SignalRegistry { defs: [0; 4] }
    }

    /// 定义信号（单源：已定义且源不同 → 分叉）。
    pub fn define(&mut self, s: RSignal, src: u16) -> bool {
        let i = s.ordinal();
        if src == 0 {
            return false;
        }
        if self.defs[i] == 0 {
            self.defs[i] = src;
            true
        } else {
            self.defs[i] == src
        }
    }

    /// 分叉数（多源定义被拒次数代理：未定义数之外的不一致由 define 返回 false）。
    pub fn undefined(&self) -> usize {
        self.defs.iter().filter(|d| **d == 0).count()
    }

    /// 四信号齐。
    pub fn complete(&self) -> bool {
        self.undefined() == 0
    }

    /// 单源性（全部已定义）即成立。
    pub fn single_source(&self) -> bool {
        self.complete()
    }
}

// ─────────────────────────────────────────────────────────────
// 家族二十成员协议断言
// ─────────────────────────────────────────────────────────────

/// 协议版本登记。
#[derive(Clone, Debug)]
pub struct ProtoTable {
    /// 20 成员协议版本（0=未登记）。
    vers: [u16; 20],
}

impl ProtoTable {
    /// 新建。
    pub const fn new() -> Self {
        ProtoTable { vers: [0; 20] }
    }

    /// 登记成员协议。
    pub fn enroll(&mut self, member: usize, ver: u16) -> bool {
        if member < 20 && ver != 0 {
            self.vers[member] = ver;
            true
        } else {
            false
        }
    }

    /// 未登记数。
    pub fn missing(&self) -> usize {
        self.vers.iter().filter(|v| **v == 0).count()
    }

    /// 二十成员齐。
    pub fn complete(&self) -> bool {
        self.missing() == 0
    }

    /// 协议一致（所有已登记版本相同）。
    pub fn uniform(&self) -> bool {
        let mut base: Option<u16> = None;
        for v in self.vers.iter() {
            if *v == 0 {
                continue;
            }
            match base {
                None => base = Some(*v),
                Some(b) => {
                    if *v != b {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// 版本分歧对数。
    pub fn diverge_pairs(&self) -> usize {
        let mut n = 0;
        for i in 0..20 {
            if self.vers[i] == 0 {
                continue;
            }
            for j in (i + 1)..20 {
                if self.vers[j] != 0 && self.vers[i] != self.vers[j] {
                    n += 1;
                }
            }
        }
        n
    }
}

// ─────────────────────────────────────────────────────────────
// 总中心实测 + 埋点抽查
// ─────────────────────────────────────────────────────────────

/// 观测中心。
#[derive(Clone, Debug)]
pub struct ObsCenter {
    /// 实测通过。
    pub tested: bool,
    /// 埋点数。
    pub tracks: u32,
    /// 抽查命中数。
    pub spot_hits: u32,
}

impl ObsCenter {
    /// 新建。
    pub const fn new() -> Self {
        ObsCenter {
            tested: false,
            tracks: 0,
            spot_hits: 0,
        }
    }

    /// 实测。
    pub fn self_test(&mut self, ok: bool) -> bool {
        self.tested = ok;
        ok
    }

    /// 加埋点。
    pub fn add_track(&mut self) {
        self.tracks += 1;
    }

    /// 抽查命中。
    pub fn spot(&mut self, hit: bool) {
        if hit {
            self.spot_hits += 1;
        }
    }

    /// 命中率万分比。
    pub fn hit_bp(&self) -> u32 {
        if self.tracks == 0 {
            0
        } else {
            (self.spot_hits * 10_000) / self.tracks
        }
    }

    /// 中心健康。
    pub fn healthy(&self) -> bool {
        self.tested && self.tracks > 0
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3787 判据集。
pub fn run_vr10g_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3787");

    // 0xB205/0xB202 四信号单源
    {
        let mut r = SignalRegistry::new();
        set.add("0xB205-初始全缺", r.undefined() == 4, "");
        for s in RSignal::ALL.iter() {
            r.define(*s, 1);
        }
        set.add("0xB205-四信号齐", r.complete(), "");
        set.add("0xB202-单源成立", r.single_source(), "");
        set.add("0xB202-同源幂等", r.define(RSignal::CreateLag, 1), "");
        set.add("0xB202-异源拒", !r.define(RSignal::CreateLag, 2), "");
        set.add("0xB202-零源拒", !r.define(RSignal::FlowBreak, 0), "");
    }

    // 0xB206 二十成员协议
    {
        let mut p = ProtoTable::new();
        set.add("0xB206-初始缺20", p.missing() == 20, "");
        for i in 0..20 {
            p.enroll(i, 3);
        }
        set.add("0xB206-二十齐", p.complete(), "");
        set.add("0xB203-版本一致", p.uniform() && p.diverge_pairs() == 0, "");
        p.enroll(5, 4);
        set.add("0xB203-分歧检出", !p.uniform() && p.diverge_pairs() > 0, "");
        set.add("0xB206-越界拒", !p.enroll(20, 1), "");
    }

    // 0xB204 中心实测
    {
        let mut c = ObsCenter::new();
        set.add("0xB204-初始不健康", !c.healthy(), "");
        c.self_test(true);
        set.add("0xB204-无埋点不健康", !c.healthy(), "");
        c.add_track();
        c.add_track();
        set.add("0xB204-有埋点健康", c.healthy(), "");
        c.spot(true);
        set.add("0xB204-命中率", c.hit_bp() == 5000, "");
        c.self_test(false);
        set.add("0xB204-实测失败不健康", !c.healthy(), "");
    }

    // 0xB201 埋点抽查
    {
        let mut c = ObsCenter::new();
        set.add("0xB201-零埋点零率", c.hit_bp() == 0, "");
        c.add_track();
        c.spot(false);
        set.add("0xB201-未中零率", c.hit_bp() == 0, "");
    }

    // 信号序自洽
    {
        let mut ok = true;
        for (i, s) in RSignal::ALL.iter().enumerate() {
            if s.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xB205-信号序自洽", ok, "");
    }

    set
}
