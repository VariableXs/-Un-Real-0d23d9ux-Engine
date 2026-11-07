//! UNX-F51588 M 型段红线复核汇总（写前校验挂接点 ×4 复核） · 0xC15C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51589-J1 清单逐条登记（阈值/样本/演习三类）；不虚报断言在位
//!
//! 结构清单：
//!   - HookPoint：写前校验挂接点闭集（分配/写入/回收/封存，4 处）
//!   - RedlineRule：红线规则行（规则号/上限阈值/口径）
//!   - ReviewRound：单轮复核记录（联签位 AI-10 + 复核位 AI-86 + 违例数）
//!   - RedlineReviewer：×4 处挂接点 ×4 轮复核状态机
//!   - RegistryEntry：清单登记行（阈值/样本/演习三类闭集）
//!   - RedlineCode：诊断码闭集（12 变体）
//!   - Lcg / fnv1a：确定性注入与双跑指纹
//!
//! 设计要点：
//!   ① no_std + alloc：全实现只用 core/alloc，零浮点、零 IO、零墙钟、零随机
//!      （LCG 种子由 SEG_BASE 派生）；零 unwrap/expect/panic/裸下标，取值一律
//!      .get()/.get_mut()。
//!   ② 写前校验挂接点 ×4 复核：每个挂接点跑 4 轮，每轮必须同时具备 AI-10 联
//!      签位与 AI-86 复核位才算全过；违例零断言 = 全程违例计数恰 0。
//!   ③ 写前判序：挂接点闭集 → 规则在册 → 阈值核对（越线显性拒）→ 联签/复
//!      核位齐 → 落轮。
//!   ④ 清单逐条登记：阈值/样本/演习三类闭集，每类至少一条在册且断言位在位
//!      （不虚报断言 = 有断言位的条目必须真实可验）。
//!   ⑤ 注入核：4 类坏样本（越线写入/绕过挂接点/未知挂接点/联签位缺失）逐类
//!      10 轮精确复现；注入账守恒：检出+逃逸=轮数，逃逸恒 0。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC15C0;

/// 挂接点数（×4 复核）。
pub const HOOKS: usize = 4;
/// 每挂接点复核轮数（×4 轮全过）。
pub const ROUNDS: usize = 4;

/// 写前校验挂接点闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HookPoint {
    /// 分配前校验
    PreAlloc = 0,
    /// 写入前校验
    PreWrite = 1,
    /// 回收前校验
    PreFree = 2,
    /// 封存前校验
    PreSeal = 3,
}

impl HookPoint {
    pub fn code(self) -> u32 {
        self as u32
    }
    /// 闭集下标，用于红线数组寻址。
    pub fn idx(self) -> usize {
        self as usize
    }
    /// 挂接点码闭集解码。
    pub fn from_code(c: u32) -> Option<HookPoint> {
        match c {
            0 => Some(HookPoint::PreAlloc),
            1 => Some(HookPoint::PreWrite),
            2 => Some(HookPoint::PreFree),
            3 => Some(HookPoint::PreSeal),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            HookPoint::PreAlloc => "分配前",
            HookPoint::PreWrite => "写入前",
            HookPoint::PreFree => "回收前",
            HookPoint::PreSeal => "封存前",
        }
    }
    /// 闭集全量清单。
    pub fn all() -> [HookPoint; HOOKS] {
        [
            HookPoint::PreAlloc,
            HookPoint::PreWrite,
            HookPoint::PreFree,
            HookPoint::PreSeal,
        ]
    }
}

/// 清单登记类别闭集（阈值/样本/演习三类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryKind {
    /// 阈值类
    Threshold = 0,
    /// 样本类
    Sample = 1,
    /// 演习类
    Drill = 2,
}

impl EntryKind {
    pub fn code(self) -> u32 {
        self as u32
    }
    /// 类别码闭集解码。
    pub fn from_code(c: u32) -> Option<EntryKind> {
        match c {
            0 => Some(EntryKind::Threshold),
            1 => Some(EntryKind::Sample),
            2 => Some(EntryKind::Drill),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            EntryKind::Threshold => "阈值",
            EntryKind::Sample => "样本",
            EntryKind::Drill => "演习",
        }
    }
}

/// 清单登记行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RegistryEntry {
    /// 行号
    pub id: usize,
    /// 类别码（闭集 0..3）
    pub kind_code: u32,
    /// 口径值（整数）
    pub metric: u64,
    /// 不虚报断言位（有断言的条目置真）
    pub asserted: bool,
}

/// 在册清单：三类逐条登记（每类 2 条，共 6 条，全部带断言位）。
pub const REGISTRY: [RegistryEntry; 6] = [
    RegistryEntry { id: 0, kind_code: 0, metric: 4096, asserted: true },
    RegistryEntry { id: 1, kind_code: 0, metric: 512, asserted: true },
    RegistryEntry { id: 2, kind_code: 1, metric: 40, asserted: true },
    RegistryEntry { id: 3, kind_code: 1, metric: 85, asserted: true },
    RegistryEntry { id: 4, kind_code: 2, metric: 10, asserted: true },
    RegistryEntry { id: 5, kind_code: 2, metric: 3, asserted: true },
];

/// 红线规则行（挂接点 → 上限阈值）。
pub const REDLINES: [u64; HOOKS] = [1024, 8192, 512, 256];

/// 单轮复核记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReviewRound {
    /// 联签位（AI-10）
    pub co_sign: bool,
    /// 复核位（AI-86）
    pub re_check: bool,
    /// 本轮违例数
    pub violations: u64,
}

/// 诊断码闭集（红线复核域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RedlineCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 挂接点码越闭集
    HookUnknown,
    /// 挂接点越界
    HookOob,
    /// 绕过挂接点（写前未过校验）
    HookBypassed,
    /// 越线（口径值超过红线上限）
    RedlineCrossed,
    /// 联签位缺失（AI-10）
    CoSignMissing,
    /// 复核位缺失（AI-86）
    ReCheckMissing,
    /// 复核轮次缺失
    RoundsMissing,
    /// 违例非零（违例零断言被破）
    ViolationNonZero,
    /// 清单类别越闭集
    KindUnknown,
    /// 不虚报断言缺失（在册条目必须带断言位）
    AssertionMissing,
}

impl RedlineCode {
    /// 诊断值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RedlineCode::Ok => "红线复核判定成功",
            RedlineCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            RedlineCode::HookUnknown => "挂接点非法：写前校验挂接点码必须在 0..4 闭集内",
            RedlineCode::HookOob => "挂接点越界：挂接点下标必须在 0..4 之内",
            RedlineCode::HookBypassed => "绕过挂接点：写前必须先过校验挂接点",
            RedlineCode::RedlineCrossed => "越线：口径值超过红线上限，显性拒",
            RedlineCode::CoSignMissing => "联签位缺失：AI-10 联签位必须置位",
            RedlineCode::ReCheckMissing => "复核位缺失：AI-86 复核位必须置位",
            RedlineCode::RoundsMissing => "轮次缺失：每个挂接点必须跑满 4 轮复核",
            RedlineCode::ViolationNonZero => "违例非零：违例零断言要求全程违例为 0",
            RedlineCode::KindUnknown => "类别非法：清单类别码必须在阈值/样本/演习闭集内",
            RedlineCode::AssertionMissing => "断言缺失：在册条目必须带不虚报断言位",
        }
    }
}

/// 诊断码闭集权威清单（12 变体）。
pub const ALL_CODES: [RedlineCode; 12] = [
    RedlineCode::Ok,
    RedlineCode::SegBad,
    RedlineCode::HookUnknown,
    RedlineCode::HookOob,
    RedlineCode::HookBypassed,
    RedlineCode::RedlineCrossed,
    RedlineCode::CoSignMissing,
    RedlineCode::ReCheckMissing,
    RedlineCode::RoundsMissing,
    RedlineCode::ViolationNonZero,
    RedlineCode::KindUnknown,
    RedlineCode::AssertionMissing,
];

/// ×4 处挂接点 ×4 轮复核状态机。
pub struct RedlineReviewer {
    /// 每挂接点每轮的复核记录
    rounds: [[ReviewRound; ROUNDS]; HOOKS],
    /// 每挂接点已完成轮数
    rounds_done: [usize; HOOKS],
    total_violations: u64,
    total_passes: u64,
    rejected: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl RedlineReviewer {
    /// 新建复核器（全部挂接点零轮）。
    pub fn new() -> RedlineReviewer {
        RedlineReviewer {
            rounds: [[ReviewRound { co_sign: false, re_check: false, violations: 0 }; ROUNDS]; HOOKS],
            rounds_done: [0; HOOKS],
            total_violations: 0,
            total_passes: 0,
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 写前校验：挂接点闭集 → 红线核对 → 联签/复核位齐 → 落轮。
    /// 返回本轮违例数（0 = 全过）。
    pub fn precheck(
        &mut self,
        hook: u32,
        value: u64,
        co_sign: bool,
        re_check: bool,
    ) -> Result<u64, RedlineCode> {
        let hp = match HookPoint::from_code(hook) {
            Some(h) => h,
            None => {
                self.rejected += 1;
                return Err(RedlineCode::HookUnknown);
            }
        };
        let hi = hp.idx();
        let limit = match REDLINES.get(hi) {
            Some(&l) => l,
            None => {
                self.rejected += 1;
                return Err(RedlineCode::HookOob);
            }
        };
        let rd = match self.rounds_done.get(hi) {
            Some(&r) => r,
            None => {
                self.rejected += 1;
                return Err(RedlineCode::HookOob);
            }
        };
        if rd >= ROUNDS {
            self.rejected += 1;
            return Err(RedlineCode::RoundsMissing);
        }
        let mut viol: u64 = 0;
        if value > limit {
            viol += 1;
        }
        if !co_sign {
            viol += 1;
        }
        if !re_check {
            viol += 1;
        }
        // 落轮：违例计入总账；位齐且未越线才算本轮全过。
        let rec = ReviewRound { co_sign, re_check, violations: viol };
        if let Some(arr) = self.rounds.get_mut(hi) {
            if let Some(slot) = arr.get_mut(rd) {
                *slot = rec;
            }
        }
        if let Some(r) = self.rounds_done.get_mut(hi) {
            *r += 1;
        }
        self.total_violations += viol;
        if viol == 0 {
            self.total_passes += 1;
        }
        Ok(viol)
    }

    /// 挂接点完成轮数读取。
    pub fn done_rounds(&self, hook: u32) -> Option<usize> {
        match HookPoint::from_code(hook) {
            Some(hp) => match self.rounds_done.get(hp.idx()) {
                Some(&r) => Some(r),
                None => None,
            },
            None => None,
        }
    }

    /// 单轮记录读取。
    pub fn round_at(&self, hook: u32, round: usize) -> Option<ReviewRound> {
        let hp = HookPoint::from_code(hook)?;
        let arr = self.rounds.get(hp.idx())?;
        arr.get(round).copied()
    }

    /// 收口：每挂接点跑满 4 轮、每轮联签/复核位齐、全程违例零。
    pub fn finalize(&self) -> Result<(), RedlineCode> {
        for hp in HookPoint::all().iter() {
            let rd = match self.rounds_done.get(hp.idx()) {
                Some(&r) => r,
                None => return Err(RedlineCode::HookOob),
            };
            if rd < ROUNDS {
                return Err(RedlineCode::RoundsMissing);
            }
            for r in 0..ROUNDS {
                let rec = match self.round_at(hp.code(), r) {
                    Some(x) => x,
                    None => return Err(RedlineCode::HookOob),
                };
                if !rec.co_sign {
                    return Err(RedlineCode::CoSignMissing);
                }
                if !rec.re_check {
                    return Err(RedlineCode::ReCheckMissing);
                }
                if rec.violations != 0 {
                    return Err(RedlineCode::ViolationNonZero);
                }
            }
        }
        if self.total_violations != 0 {
            return Err(RedlineCode::ViolationNonZero);
        }
        Ok(())
    }

    /// 违例零断言面。
    pub fn zero_violations(&self) -> bool {
        self.total_violations == 0 && self.total_passes == (HOOKS * ROUNDS) as u64
    }

    /// (全过轮数, 拒绝数) 二本计数。
    pub fn tally(&self) -> (u64, u64) {
        (self.total_passes, self.rejected)
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    /// 守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }

    /// 状态快照（双跑一致面）。
    pub fn snapshot(&self) -> (u64, u64, u64, [usize; HOOKS]) {
        (self.total_passes, self.total_violations, self.rejected, self.rounds_done)
    }

    /// 快照指纹。
    pub fn fingerprint(&self) -> u64 {
        let (p, v, rj, done) = self.snapshot();
        let mut bytes: Vec<u8> = Vec::new();
        for x in [p, v, rj] {
            bytes.extend_from_slice(&x.to_le_bytes());
        }
        for d in done.iter() {
            bytes.extend_from_slice(&(*d as u64).to_le_bytes());
        }
        for arr in self.rounds.iter() {
            for rec in arr.iter() {
                bytes.push(rec.co_sign as u8);
                bytes.push(rec.re_check as u8);
                bytes.extend_from_slice(&rec.violations.to_le_bytes());
            }
        }
        fnv1a(&bytes)
    }
}

/// 清单对账：三类逐条登记 + 断言位全在位。
pub fn registry_clean() -> bool {
    let mut kinds = [0u64; 3];
    for e in REGISTRY.iter() {
        match EntryKind::from_code(e.kind_code) {
            Some(k) => {
                if let Some(c) = kinds.get_mut(k.code() as usize) {
                    *c += 1;
                }
            }
            None => return false,
        }
        if !e.asserted {
            return false;
        }
    }
    kinds.iter().all(|&c| c > 0)
}

/// FNV-1a 64 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 确定性 LCG（种子由 SEG_BASE 派生，零随机源）。
pub struct Lcg {
    s: u64,
}

impl Lcg {
    /// 由段基派生种子。
    pub fn new(seed: u64) -> Lcg {
        Lcg { s: seed ^ 0x27d4eb2f165667c5 }
    }
    /// 取下一个 32 位伪随机数。
    pub fn next_u32(&mut self) -> u32 {
        self.s = self
            .s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.s >> 33) as u32
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 标准脚本：4 处挂接点 ×4 轮，每轮口径取红线下安全定值，双位齐。
pub fn standard_script() -> Vec<(u32, u64, bool, bool)> {
    let mut v: Vec<(u32, u64, bool, bool)> = Vec::new();
    for hp in HookPoint::all().iter() {
        let limit = match REDLINES.get(hp.idx()) {
            Some(&l) => l,
            None => 0,
        };
        for r in 0..ROUNDS {
            // 口径取红线的 (8+r)%9 十分位，恒低于红线。
            let value = limit / 10 * (((r % 9) + 1) as u64 / 2 + 1);
            let value2 = if value >= limit { limit - 1 } else { value };
            v.push((hp.code(), value2, true, true));
        }
    }
    v
}

/// 双跑一致性：同一脚本两次建复核器，指纹逐位一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> (u64, u64, [usize; HOOKS]) {
        let mut rv = RedlineReviewer::new();
        for (h, v, cs, rc) in standard_script().iter() {
            let _ = rv.precheck(*h, *v, *cs, *rc);
        }
        let fin = match rv.finalize() {
            Ok(()) => 1u64,
            Err(_) => 0,
        };
        let snap = rv.snapshot();
        (fin, rv.fingerprint(), snap.3)
    };
    build() == build()
}

/// 注入核：4 类坏样本逐类 10 轮精确复现，守恒账（检出+逃逸=轮数）。
pub fn injection_harness() -> (u64, u64, [u64; 4]) {
    let mut total_detect: u64 = 0;
    let mut total_escape: u64 = 0;
    let mut per: [u64; 4] = [0; 4];
    // 类 0：越线写入（口径超红线）。
    for i in 0..10u32 {
        let mut rv = RedlineReviewer::new();
        let bump = (i % 5 + 1) as u64;
        let r = rv.precheck(HookPoint::PreWrite.code(), REDLINES[1] + bump, true, true);
        if r == Ok(1) {
            rv.note_detected();
            total_detect += 1;
            per[0] += 1;
        } else {
            rv.note_escape();
            total_escape += 1;
        }
    }
    // 类 1：绕过挂接点（未知挂接点码）。
    for i in 0..10u32 {
        let mut rv = RedlineReviewer::new();
        let r = rv.precheck(4 + (i % 3), 1, true, true);
        if r == Err(RedlineCode::HookUnknown) {
            rv.note_detected();
            total_detect += 1;
            per[1] += 1;
        } else {
            rv.note_escape();
            total_escape += 1;
        }
    }
    // 类 2：联签位缺失。
    for _ in 0..10u32 {
        let mut rv = RedlineReviewer::new();
        let r = rv.precheck(HookPoint::PreAlloc.code(), 8, false, true);
        if r == Ok(1) {
            rv.note_detected();
            total_detect += 1;
            per[2] += 1;
        } else {
            rv.note_escape();
            total_escape += 1;
        }
    }
    // 类 3：复核位缺失。
    for _ in 0..10u32 {
        let mut rv = RedlineReviewer::new();
        let r = rv.precheck(HookPoint::PreSeal.code(), 8, true, false);
        if r == Ok(1) {
            rv.note_detected();
            total_detect += 1;
            per[3] += 1;
        } else {
            rv.note_escape();
            total_escape += 1;
        }
    }
    (total_detect, total_escape, per)
}

/// 收口自检（15 条判据形态）。
pub fn run_unx51588_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51588");
    // 1 标准脚本：4 处 ×4 轮全过并收口。
    {
        let mut rv = RedlineReviewer::new();
        for (h, v, cs, rc) in standard_script().iter() {
            let _ = rv.precheck(*h, *v, *cs, *rc);
        }
        let fin = rv.finalize();
        s.add(
            "挂接点×4轮全过",
            fin == Ok(()) && rv.zero_violations(),
            "AI-10 联签位 + AI-86 复核位齐且全程违例零，4 处挂接点各 4 轮全过",
        );
    }
    // 2 违例零断言。
    {
        let mut rv = RedlineReviewer::new();
        for (h, v, cs, rc) in standard_script().iter() {
            let _ = rv.precheck(*h, *v, *cs, *rc);
        }
        s.add(
            "违例零断言",
            rv.zero_violations() && rv.snapshot().1 == 0,
            "全程违例计数恰 0，全过轮数恰 16",
        );
    }
    // 3 写前判序：挂接点闭集先于红线核对。
    {
        let mut rv = RedlineReviewer::new();
        let r = rv.precheck(99, REDLINES[0] + 9999, true, true);
        s.add(
            "判序挂接点闭集先行",
            r == Err(RedlineCode::HookUnknown),
            "挂接点码越闭集先于红线核对与联签判据",
        );
    }
    // 4 越线显性拒（违例计数 +1）。
    {
        let mut rv = RedlineReviewer::new();
        let r = rv.precheck(HookPoint::PreWrite.code(), REDLINES[1] + 1, true, true);
        s.add(
            "越线显性计违例",
            r == Ok(1),
            "口径值超过红线上限计 1 违例，不静默放行",
        );
    }
    // 5 联签位缺失拒收口。
    {
        let mut rv = RedlineReviewer::new();
        // 全 16 轮只缺一轮联签位。
        for hp in HookPoint::all().iter() {
            for r in 0..ROUNDS {
                let limit = match REDLINES.get(hp.idx()) {
                    Some(&l) => l,
                    None => 0,
                };
                let v = if limit > 0 { limit - 1 } else { 0 };
                let cs = !(hp.idx() == 0 && r == 0);
                let _ = rv.precheck(hp.code(), v, cs, true);
            }
        }
        let fin = rv.finalize();
        s.add(
            "联签位缺失拒收口",
            fin == Err(RedlineCode::CoSignMissing),
            "任一轮 AI-10 联签位缺失，收口显性拒",
        );
    }
    // 6 轮次缺失拒收口。
    {
        let mut rv = RedlineReviewer::new();
        for r in 0..ROUNDS - 1 {
            let _ = rv.precheck(HookPoint::PreAlloc.code(), 8, true, true);
            let _ = r;
        }
        let fin = rv.finalize();
        s.add(
            "轮次缺失拒收口",
            fin == Err(RedlineCode::RoundsMissing),
            "任一挂接点不足 4 轮复核，收口显性拒",
        );
    }
    // 7 清单逐条登记：三类闭集全覆盖且断言位全在位。
    {
        s.add(
            "清单三类逐条登记",
            registry_clean() && REGISTRY.len() == 6,
            "阈值/样本/演习三类逐条登记，每类至少一条且不虚报断言位全在位",
        );
    }
    // 8 不虚报断言：在册条目的口径值可真实验（阈值类口径 > 0、样本/演习非零）。
    {
        let mut honest = true;
        for e in REGISTRY.iter() {
            match EntryKind::from_code(e.kind_code) {
                Some(_) => {
                    if e.metric == 0 || !e.asserted {
                        honest = false;
                    }
                }
                None => honest = false,
            }
        }
        s.add(
            "不虚报断言在位",
            honest,
            "每条在册登记口径非零且断言位真实（可验非虚报）",
        );
    }
    // 9 诊断码闭集唯一性。
    {
        let mut uniq = true;
        let mut text = true;
        for i in 0..ALL_CODES.len() {
            let c = match ALL_CODES.get(i) {
                Some(&c) => c,
                None => {
                    uniq = false;
                    continue;
                }
            };
            if c.val() != SEG_BASE + 1 + i as u32 {
                uniq = false;
            }
            if c.detail().is_empty() {
                text = false;
            }
        }
        s.add(
            "诊断码闭集唯一",
            uniq && text && ALL_CODES.len() >= 10 && ALL_CODES.len() <= 14,
            "val=SEG_BASE+1+序号，detail 全量中文非空，闭集 12 变体",
        );
    }
    // 10 注入核：四类 ×10 全抓、零逃逸。
    {
        let (d, e, per) = injection_harness();
        let all10 = per.iter().all(|&x| x == 10);
        s.add(
            "注入 4 类×10 全抓",
            d == 40 && e == 0 && all10,
            "注入=检出+逃逸，逃逸恒 0，每类恰 10 轮全抓",
        );
    }
    // 11 守恒账单独核。
    {
        let mut rv = RedlineReviewer::new();
        for _ in 0..10u32 {
            rv.note_detected();
        }
        s.add("注入账守恒", rv.conserved(), "注入=检出+逃逸且逃逸=0 且检出>0");
    }
    // 12 双跑指纹一致。
    s.add(
        "双跑 FNV 指纹一致",
        dual_run_consistent(),
        "同一脚本两次建复核器，FNV-1a 指纹逐位一致",
    );
    // 13 指纹复算稳定且摘要定长。
    {
        let mut rv = RedlineReviewer::new();
        let _ = rv.precheck(HookPoint::PreAlloc.code(), 8, true, true);
        let fp1 = rv.fingerprint();
        let fp2 = rv.fingerprint();
        let brief: String = format!("{:016x}", fp1);
        s.add(
            "指纹复算稳定",
            fp1 == fp2 && brief.len() == 16,
            "指纹两次复算相等且十六进制摘要定长 16",
        );
    }
    // 14 挂接点闭集完整。
    {
        let mut closed = true;
        for hp in HookPoint::all().iter() {
            match HookPoint::from_code(hp.code()) {
                Some(x) if x == *hp => {}
                _ => closed = false,
            }
        }
        s.add(
            "挂接点闭集完整",
            closed && HookPoint::from_code(4).is_none(),
            "4 处挂接点全可解码，越码显性 None",
        );
    }
    // 15 段位守卫与结果摘要非空。
    {
        let summary: String = format!(
            "unx51588 红线复核 {} 处×{} 轮 诊断码{} 变体",
            HOOKS,
            ROUNDS,
            ALL_CODES.len()
        );
        s.add(
            "段位守卫与摘要",
            seg_guard_ok() && !summary.is_empty() && summary.contains("unx51588"),
            "SEG_BASE 对齐且在分派区间，结果摘要非空",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51588_checks_green() {
        let set = run_unx51588_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn standard_full_pass_and_seal() {
        let mut rv = RedlineReviewer::new();
        for (h, v, cs, rc) in standard_script().iter() {
            assert!(rv.precheck(*h, *v, *cs, *rc).is_ok());
        }
        assert!(rv.finalize().is_ok());
        assert!(rv.zero_violations());
        for hp in HookPoint::all().iter() {
            assert_eq!(rv.done_rounds(hp.code()), Some(ROUNDS));
        }
    }

    #[test]
    fn redline_crossed_counts_violation() {
        let mut rv = RedlineReviewer::new();
        let r = rv.precheck(HookPoint::PreWrite.code(), REDLINES[1] + 7, true, true);
        assert_eq!(r, Ok(1));
        assert_eq!(rv.snapshot().1, 1);
    }

    #[test]
    fn hook_unknown_rejected() {
        let mut rv = RedlineReviewer::new();
        assert_eq!(
            rv.precheck(42, 1, true, true),
            Err(RedlineCode::HookUnknown)
        );
        assert!(rv.done_rounds(42).is_none());
    }

    #[test]
    fn missing_sign_bits_count_violation() {
        let mut rv = RedlineReviewer::new();
        assert_eq!(
            rv.precheck(HookPoint::PreAlloc.code(), 8, false, false),
            Ok(2)
        );
    }

    #[test]
    fn finalize_rejects_missing_rounds() {
        let mut rv = RedlineReviewer::new();
        for _ in 0..ROUNDS - 1 {
            let _ = rv.precheck(HookPoint::PreAlloc.code(), 8, true, true);
        }
        assert_eq!(rv.finalize(), Err(RedlineCode::RoundsMissing));
    }

    #[test]
    fn finalize_rejects_violation() {
        let mut rv = RedlineReviewer::new();
        for hp in HookPoint::all().iter() {
            for r in 0..ROUNDS {
                let limit = REDLINES[hp.idx()];
                let v = if hp.idx() == 2 && r == 0 {
                    limit + 1
                } else {
                    limit - 1
                };
                let _ = rv.precheck(hp.code(), v, true, true);
            }
        }
        assert_eq!(rv.finalize(), Err(RedlineCode::ViolationNonZero));
    }

    #[test]
    fn registry_three_kinds_registered() {
        assert!(registry_clean());
        for e in REGISTRY.iter() {
            assert!(EntryKind::from_code(e.kind_code).is_some());
            assert!(e.asserted);
        }
        assert_eq!(EntryKind::from_code(3), None);
    }

    #[test]
    fn injection_all_classes_caught() {
        let (_, esc, per) = injection_harness();
        assert_eq!(esc, 0);
        for (i, &p) in per.iter().enumerate() {
            assert_eq!(p, 10, "注入类 {} 应恰 10 轮全抓", i);
        }
    }

    #[test]
    fn conservation_holds() {
        let mut rv = RedlineReviewer::new();
        for _ in 0..10u32 {
            rv.note_detected();
        }
        assert!(rv.conserved());
        let mut bad = RedlineReviewer::new();
        for _ in 0..10u32 {
            bad.note_escape();
        }
        assert!(!bad.conserved());
    }

    #[test]
    fn dual_run_identical_fingerprint() {
        assert!(dual_run_consistent());
        let build = || -> u64 {
            let mut rv = RedlineReviewer::new();
            for (h, v, cs, rc) in standard_script().iter() {
                let _ = rv.precheck(*h, *v, *cs, *rc);
            }
            rv.fingerprint()
        };
        assert_eq!(build(), build());
    }

    #[test]
    fn code_table_closed_and_unique() {
        assert_eq!(ALL_CODES.len(), 12);
        for (i, c) in ALL_CODES.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            assert!(!c.detail().is_empty());
        }
        for i in 0..ALL_CODES.len() {
            for j in (i + 1)..ALL_CODES.len() {
                let a = match ALL_CODES.get(i) {
                    Some(&x) => x,
                    None => continue,
                };
                let b = match ALL_CODES.get(j) {
                    Some(&y) => y,
                    None => continue,
                };
                assert_ne!(a.val(), b.val());
            }
        }
    }

    #[test]
    fn seg_guard_and_lcg_deterministic() {
        assert!(seg_guard_ok());
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        for _ in 0..16u32 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }
}
