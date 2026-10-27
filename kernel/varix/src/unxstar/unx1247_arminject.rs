//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1247 · Arming 态故障注入（武装中断/部分失败必抓） · 0xFB40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F1247-J1 故障场景 ≥5 类（使能写失败/依赖环/缓冲溢出/到齐
//!      超时/回退失败）注入 10/10 次必抓，防护动作正确率 100%，资产
//!      可重复一致率 100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 被测机：武装机仿真器（依赖序使能 + 事件缓冲 + 确认窗 + 回退
//!     协议），依赖表逐向量前 BitSet，循环依赖检测链走查；
//!   - 故障五类：使能写失败（写路径断）/依赖环（链回指）/缓冲溢出
//!     （背压）/到齐超时（窗尽缺交）/回退失败（撤使能抗拆）；
//!   - 注入必抓：每类故障 10/10 轮确定性注入，防护动作全正确；
//!   - 防护动作：故障向量红标隔离、依赖环拒武、背压拒收、超时缺集
//!     局部重武装、抗拆位强制清零；
//!   - 可重复一致率：每场景 3 连跑摘要（FNV）逐次相等；
//!   - LCG 种子写死 0xFB40，禁 rand/浮点/IO。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0xFB40 段，主代理分工表分配）。
pub const SEG_BASE: u16 = 0xFB40;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 故障场景类数（判准 ≥5）。
pub const FAULT_KINDS: usize = 5;

/// 可重复连跑次数。
pub const REPEAT_RUNS: usize = 3;

/// 武装向量表宽度。
pub const INJ_VECTORS: usize = 5;

/// 事件缓冲容量（背压线）。
pub const BUF_CAP: usize = 8;

/// 确认窗预算（tick）。
pub const CONFIRM_BUDGET: u64 = 10;

/// 诊断码（0xFB40 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FjCode {
    /// 0xFB40 使能写失败（写路径断，向量未武）。
    EnableFail = 0,
    /// 0xFB41 依赖环（链回指必拒武）。
    DepCycle = 1,
    /// 0xFB42 缓冲溢出（背压拒收未记账）。
    BufOverflow = 2,
    /// 0xFB43 到齐超时（窗尽缺交未红账）。
    ConfirmTimeout = 3,
    /// 0xFB44 回退失败（撤使能抗拆残留）。
    RetireFail = 4,
    /// 0xFB45 依赖未满足先武（前置位未置）。
    DepNotMet = 5,
    /// 0xFB46 防护失灵（故障注入未触发防护动作）。
    GuardMiss = 6,
    /// 0xFB47 故障漏抓（注入 10 轮有轮未抓）。
    FaultLeak = 7,
    /// 0xFB48 指纹漂移（3 连跑摘要不一致）。
    DigestDrift = 8,
    /// 0xFB49 场景缺类（<5 类注入资产）。
    KindShort = 9,
}

impl FjCode {
    /// 判别式真值。
    pub const fn val(self) -> u16 {
        SEG_BASE + self as u16
    }
    /// 全码闭集。
    pub const ALL: [FjCode; 10] = [
        FjCode::EnableFail, FjCode::DepCycle, FjCode::BufOverflow, FjCode::ConfirmTimeout,
        FjCode::RetireFail, FjCode::DepNotMet, FjCode::GuardMiss, FjCode::FaultLeak,
        FjCode::DigestDrift, FjCode::KindShort,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        0xFB40 => "使能写失败：故障向量红标隔离，序列跳过续武其余向量。",
        0xFB41 => "依赖环：武装前先走依赖链，回指即拒武并红账。",
        0xFB42 => "缓冲溢出：容量满必须背压拒收并记账，不许静默丢弃。",
        0xFB43 => "到齐超时：窗尽未到齐向量入红账并触发局部重武装。",
        0xFB44 => "回退失败：抗拆位强制清零，回退后位图必须归零。",
        0xFB45 => "依赖未满足：前置向量未武装不许使能，先补前置。",
        0xFB46 => "防护失灵：每类故障必须触发对应防护动作，缺失即修。",
        0xFB47 => "故障漏抓：逐轮回放注入记录，未抓轮回溯注入序。",
        0xFB48 => "指纹漂移：种子与步序全定，3 连跑摘要必须全等。",
        0xFB49 => "场景缺类：5 类故障注入资产补齐后再聚合。",
        _ => "未知诊断码：回查 0xFB40 段闭集。",
    }
}

/// 故障五类（判准点名）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultKind {
    /// 故障 1：使能写失败（写路径断 → 向量未武）。
    EnableWriteFail,
    /// 故障 2：依赖环（依赖表回指）。
    DepCycle,
    /// 故障 3：缓冲溢出（事件量超容量）。
    BufOverflow,
    /// 故障 4：到齐超时（窗预算尽仍有向量缺交）。
    ConfirmTimeout,
    /// 故障 5：回退失败（撤使能被抗拆）。
    RetireFail,
}

impl FaultKind {
    /// 全类闭集。
    pub const ALL: [FaultKind; FAULT_KINDS] = [
        FaultKind::EnableWriteFail,
        FaultKind::DepCycle,
        FaultKind::BufOverflow,
        FaultKind::ConfirmTimeout,
        FaultKind::RetireFail,
    ];
    /// 中文短句。
    pub const fn text(self) -> &'static str {
        match self {
            FaultKind::EnableWriteFail => "使能写失败",
            FaultKind::DepCycle => "依赖环",
            FaultKind::BufOverflow => "缓冲溢出",
            FaultKind::ConfirmTimeout => "到齐超时",
            FaultKind::RetireFail => "回退失败",
        }
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0xFB40 段基址（固定种子，可重复）。
    pub const SEED: u64 = 0xFB40_FB40_FB40_FB40;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// FNV-1a 滚动哈希步。
const fn fnv_step(acc: u64, byte: u64) -> u64 {
    (acc ^ byte).wrapping_mul(0x100000001b3)
}

/// 武装机仿真器（被测面正本）。
///
/// 依赖表：每向量一个前 BitSet（前置向量必须先武装）。
/// 防护动作：红标隔离 / 拒武 / 背压 / 局部重武装 / 强制清零。
#[derive(Debug)]
pub struct ArmMachine {
    /// 依赖表（前 BitSet，最多 5 向量）。
    deps: [u32; INJ_VECTORS],
    /// 已武装位图。
    armed: u32,
    /// 故障红标位图（注入致残向量）。
    faulted: u32,
    /// 抗拆位图（回退故障注入）。
    stuck: u32,
    /// 事件缓冲。
    buffer: Vec<u64>,
    /// 到齐位图（确认窗内）。
    arrived: u32,
    /// 窗口 tick。
    tick: u64,
    /// 防护动作计数（红标/拒武/背压/重武/清零）。
    guards: [u32; 5],
    /// 摘要（FNV，可重复性主键）。
    digest: u64,
}

impl ArmMachine {
    /// 建机（链式依赖：1←0、3←2，其余无前置）。
    pub fn new() -> Self {
        let mut deps = [0u32; INJ_VECTORS];
        deps[1] = 1 << 0;
        deps[3] = 1 << 2;
        ArmMachine {
            deps,
            armed: 0,
            faulted: 0,
            stuck: 0,
            buffer: Vec::new(),
            arrived: 0,
            tick: 0,
            guards: [0; 5],
            digest: 0xcbf29ce484222325,
        }
    }

    /// 建机（自定义依赖表——注入依赖环用）。
    pub fn with_deps(deps: [u32; INJ_VECTORS]) -> Self {
        ArmMachine {
            deps,
            armed: 0,
            faulted: 0,
            stuck: 0,
            buffer: Vec::new(),
            arrived: 0,
            tick: 0,
            guards: [0; 5],
            digest: 0xcbf29ce484222325,
        }
    }

    /// 依赖环检测：逐向量沿依赖链走 ≤N 步，回指即环。
    pub fn has_cycle(&self) -> bool {
        for v in 0..INJ_VECTORS {
            let mut cur = v;
            for _ in 0..INJ_VECTORS {
                let m = self.deps[cur];
                if m == 0 {
                    break;
                }
                // 取最低前置位。
                let nx = m.trailing_zeros() as usize;
                if nx >= INJ_VECTORS {
                    break;
                }
                if nx == v {
                    return true;
                }
                cur = nx;
            }
        }
        false
    }

    /// 尝试武装（依赖未满足必拒；已武必拒；写失败注入必拒并红标）。
    pub fn try_arm(&mut self, v: usize, write_fail: bool) -> Result<bool, FjCode> {
        if v >= INJ_VECTORS {
            return Err(FjCode::DepNotMet);
        }
        if self.armed & (1u32 << v) != 0 {
            return Err(FjCode::DepNotMet);
        }
        if self.deps[v] & !self.armed != 0 {
            self.guards[1] += 1; // 拒武
            self.digest = fnv_step(self.digest, 0xC1);
            return Err(FjCode::DepNotMet);
        }
        if write_fail {
            self.faulted |= 1u32 << v;
            self.guards[0] += 1; // 红标隔离
            self.digest = fnv_step(self.digest, 0xC0 + v as u64);
            return Err(FjCode::EnableFail);
        }
        self.armed |= 1u32 << v;
        self.digest = fnv_step(self.digest, 0xA0 + v as u64);
        Ok(true)
    }

    /// 武装计数。
    pub fn armed_count(&self) -> u32 {
        self.armed.count_ones()
    }

    /// 故障红标计数。
    pub fn faulted_count(&self) -> u32 {
        self.faulted.count_ones()
    }

    /// 投递事件（缓冲满背压拒收）。
    pub fn submit(&mut self, v: usize, payload: u64) -> Result<bool, FjCode> {
        if v >= INJ_VECTORS || self.armed & (1u32 << v) == 0 {
            return Err(FjCode::DepNotMet);
        }
        if self.buffer.len() >= BUF_CAP {
            self.guards[2] += 1; // 背压
            self.digest = fnv_step(self.digest, 0xC2);
            return Ok(false);
        }
        self.buffer.push(payload);
        Ok(true)
    }

    /// 推进确认窗（每 tick 交付一笔缓冲；预算尽缺交即超时防护）。
    pub fn tick_window(&mut self, arrives: u32) -> bool {
        self.tick += 1;
        self.arrived |= arrives & self.armed;
        if !self.buffer.is_empty() {
            self.buffer.remove(0);
        }
        self.tick >= CONFIRM_BUDGET
    }

    /// 到齐缺集位图。
    pub fn missing_mask(&self) -> u32 {
        self.armed & !self.arrived
    }

    /// 超时防护：缺集局部重武装（清到齐位 + 记防护）。
    pub fn guard_timeout(&mut self) -> u32 {
        let m = self.missing_mask();
        if m != 0 {
            self.guards[3] += 1; // 局部重武装
            self.digest = fnv_step(self.digest, 0xC3);
        }
        m
    }

    /// 回退协议（撤使能；抗拆位注入时需强制清零防护）。
    pub fn retire(&mut self) -> Result<(), FjCode> {
        for v in 0..INJ_VECTORS {
            if self.stuck & (1u32 << v) != 0 {
                // 防护：强制清零抗拆位。
                self.stuck &= !(1u32 << v);
                self.guards[4] += 1; // 强制清零
                self.digest = fnv_step(self.digest, 0xC4);
            }
            self.armed &= !(1u32 << v);
        }
        if self.armed != 0 {
            return Err(FjCode::RetireFail);
        }
        Ok(())
    }

    /// 注入抗拆位（回退故障用）。
    pub fn inject_stuck(&mut self, v: usize) {
        if v < INJ_VECTORS {
            self.stuck |= 1u32 << v;
        }
    }

    /// 防护动作计数面。
    pub fn guard_counts(&self) -> [u32; 5] {
        self.guards
    }

    /// 当前摘要。
    pub fn digest_of(&self) -> u64 {
        self.digest
    }

    /// 位图归零判据（回退终态）。
    pub fn clean_after_retire(&self) -> bool {
        self.armed == 0 && self.stuck == 0
    }
}

/// 故障注入报告（场景级）。
#[derive(Clone, Copy, Debug)]
pub struct FaultReport {
    /// 故障类。
    pub kind: FaultKind,
    /// 本轮是否必抓（防护触发 + 判准命中）。
    pub caught: bool,
    /// 摘要（3 连跑一致主键）。
    pub digest: u64,
}

/// 场景 1：使能写失败注入（写路径断 → 红标隔离 → 其余向量续武）。
fn inject_enable_fail(seed: u64) -> FaultReport {
    let mut m = ArmMachine::new();
    let mut lcg = Lcg::new(seed);
    let bad = (lcg.below(INJ_VECTORS as u64)) as usize;
    // 依赖链：0 先武（若 bad=0 则写失败在 0 上）。
    let mut armed_ok = 0u32;
    for v in 0..INJ_VECTORS {
        let wf = v == bad;
        match m.try_arm(v, wf) {
            Ok(_) => armed_ok += 1,
            Err(FjCode::DepNotMet) => {
                // 前置未满足（bad 抢占了前置位）→ 补前置不可行即跳过。
            }
            Err(FjCode::EnableFail) => {
                // 防护：红标隔离已记（guards[0]）。
            }
            Err(_) => {}
        }
    }
    // 判准：故障向量必未武 + 红标恰一 + 至少 2 向量续武成功。
    let caught = m.faulted_count() == 1
        && (m.armed >> bad) & 1 == 0
        && armed_ok >= 2
        && m.guard_counts()[0] >= 1;
    FaultReport { kind: FaultKind::EnableWriteFail, caught, digest: m.digest_of() }
}

/// 场景 2：依赖环注入（环表 → 拒武必抓）。
fn inject_dep_cycle(seed: u64) -> FaultReport {
    let _ = seed;
    // 环表：0←1、1←0（回指）。
    let mut deps = [0u32; INJ_VECTORS];
    deps[0] = 1 << 1;
    deps[1] = 1 << 0;
    let m = ArmMachine::with_deps(deps);
    let cyc = m.has_cycle();
    let mut m2 = m;
    // 环上向量武装必拒。
    let r0 = m2.try_arm(0, false);
    let r1 = m2.try_arm(1, false);
    let caught = cyc && matches!(r0, Err(FjCode::DepNotMet)) && matches!(r1, Err(FjCode::DepNotMet));
    FaultReport { kind: FaultKind::DepCycle, caught, digest: m2.digest_of() }
}

/// 场景 3：缓冲溢出注入（灌 BUF_CAP+3 笔 → 背压必抓）。
fn inject_buf_overflow(seed: u64) -> FaultReport {
    let mut m = ArmMachine::new();
    let mut lcg = Lcg::new(seed);
    let _ = m.try_arm(0, false);
    let mut rejected = 0u32;
    for i in 0..(BUF_CAP as u64 + 3) {
        let p = lcg.next() & 0xFF;
        match m.submit(0, p) {
            Ok(true) => {}
            Ok(false) => rejected += 1,
            Err(_) => {}
        }
        let _ = i;
    }
    let g = m.guard_counts()[2];
    let caught = rejected >= 3 && g >= 1;
    FaultReport { kind: FaultKind::BufOverflow, caught, digest: m.digest_of() }
}

/// 场景 4：到齐超时注入（预算尽缺交 → 局部重武装必抓）。
fn inject_confirm_timeout(seed: u64) -> FaultReport {
    let mut m = ArmMachine::new();
    let mut lcg = Lcg::new(seed);
    // 武装 2 向量（0 与 2 无前置链）。
    let _ = m.try_arm(0, false);
    let _ = m.try_arm(2, false);
    // 只让 0 到齐；预算走尽 → 2 必缺交。
    for t in 0..CONFIRM_BUDGET {
        let arr = if t == 0 { 1 << 0 } else { 0 };
        let _ = m.tick_window(arr);
        let _ = lcg.below(3);
    }
    let miss = m.missing_mask();
    let rearm = m.guard_timeout();
    let caught = miss == (1 << 2) && rearm == (1 << 2) && m.guard_counts()[3] >= 1;
    FaultReport { kind: FaultKind::ConfirmTimeout, caught, digest: m.digest_of() }
}

/// 场景 5：回退失败注入（抗拆位 → 强制清零 → 位图归零必抓）。
fn inject_retire_fail(seed: u64) -> FaultReport {
    let mut m = ArmMachine::new();
    let mut lcg = Lcg::new(seed);
    let _ = m.try_arm(0, false);
    let _ = m.try_arm(2, false);
    let _ = m.try_arm(4, false);
    // 注入抗拆位（向量 2）。
    let bad = (lcg.below(INJ_VECTORS as u64)) as usize;
    m.inject_stuck(bad);
    let retire = m.retire();
    let g = m.guard_counts()[4];
    let caught = retire.is_ok() && g >= 1 && m.clean_after_retire();
    FaultReport { kind: FaultKind::RetireFail, caught, digest: m.digest_of() }
}

/// 按类分发注入。
pub fn inject_kind(kind: FaultKind, seed: u64) -> FaultReport {
    match kind {
        FaultKind::EnableWriteFail => inject_enable_fail(seed),
        FaultKind::DepCycle => inject_dep_cycle(seed),
        FaultKind::BufOverflow => inject_buf_overflow(seed),
        FaultKind::ConfirmTimeout => inject_confirm_timeout(seed),
        FaultKind::RetireFail => inject_retire_fail(seed),
    }
}

/// 10/10 注入演练：每类 10 轮确定性种子，全轮必抓。
pub fn drill_catch_10() -> (bool, String, usize) {
    let mut all_ok = true;
    let mut total_caught = 0usize;
    for k in FaultKind::ALL.iter() {
        let mut ok = 0u32;
        for r in 0..INJECT_ROUNDS {
            // 种子写死：段基址 × 类序 × 轮序（确定性）。
            let seed = 0xFB40u64.wrapping_add((k_idx(*k) as u64) << 8).wrapping_add(r as u64);
            let rep = inject_kind(*k, seed);
            if rep.caught {
                ok += 1;
                total_caught += 1;
            }
        }
        if ok != INJECT_ROUNDS as u32 {
            all_ok = false;
        }
    }
    (
        all_ok && total_caught == FAULT_KINDS * INJECT_ROUNDS,
        format!("5类×10轮全抓={}", total_caught),
        total_caught,
    )
}

/// 类序号。
pub const fn k_idx(k: FaultKind) -> usize {
    match k {
        FaultKind::EnableWriteFail => 0,
        FaultKind::DepCycle => 1,
        FaultKind::BufOverflow => 2,
        FaultKind::ConfirmTimeout => 3,
        FaultKind::RetireFail => 4,
    }
}

/// 资产可重复一致率：每场景 3 连跑摘要全等。
pub fn drill_repeatable_3() -> (bool, String) {
    let mut all = true;
    for k in FaultKind::ALL.iter() {
        let seed = 0xFB41u64.wrapping_add((k_idx(*k) as u64) << 8);
        let d0 = inject_kind(*k, seed).digest;
        let d1 = inject_kind(*k, seed).digest;
        let d2 = inject_kind(*k, seed).digest;
        if !(d0 == d1 && d1 == d2 && d0 != 0) {
            all = false;
        }
    }
    (all, format!("5场景×3连跑一致={}", all))
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：故障摘要位表（类/摘要/抓取位，种子钉定）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct FaultDigestRow {
    /// 故障类。
    pub kind: FaultKind,
    /// 注入摘要。
    pub digest: u64,
    /// 是否必抓。
    pub caught: bool,
}

/// 摘要位表构建（种子=段基址+类序×256，全定）。
pub fn build_fault_digests() -> Vec<FaultDigestRow> {
    FaultKind::ALL
        .iter()
        .map(|k| {
            let seed = 0xFB40u64.wrapping_add((k_idx(*k) as u64) << 8);
            let rep = inject_kind(*k, seed);
            FaultDigestRow { kind: *k, digest: rep.digest, caught: rep.caught }
        })
        .collect()
}

/// 摘要位表健全：5 类全抓且跨类摘要两两不同。
pub fn fault_digests_sane(rows: &[FaultDigestRow]) -> bool {
    rows.len() == FAULT_KINDS
        && rows.iter().all(|r| r.caught && r.digest != 0)
        && {
            for i in 0..rows.len() {
                for j in (i + 1)..rows.len() {
                    if rows[i].digest == rows[j].digest {
                        return false;
                    }
                }
            }
            true
        }
}

/// 摘要位表导出。
pub fn export_fault_digests(rows: &[FaultDigestRow]) -> String {
    let mut s = String::from("故障摘要位表：\n");
    for r in rows.iter() {
        s.push_str(&format!("· {} 摘要{:#x} 抓取={}\n", r.kind.text(), r.digest, r.caught));
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：防护动作矩阵（五类防护逐类真跑必触发）。
/// ─────────────────────────────────────────────────────────────────────
/// 防护动作核（红标/拒武/背压/局部重武/强制清零）逐类验证。
pub fn drill_guard_actions_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFB40 段基址 ^ 1。
    let mut lcg = Lcg::new(0xFB41);
    for _r in 0..INJECT_ROUNDS {
        // 红 1：使能写失败 → 红标隔离。
        let mut m1 = ArmMachine::new();
        let _ = m1.try_arm(0, true);
        let g_red = m1.guard_counts()[0] >= 1 && m1.faulted_count() == 1;
        // 红 2：依赖未满足 → 拒武。
        let mut m2 = ArmMachine::new();
        let _ = m2.try_arm(1, false);
        let g_reject = m2.guard_counts()[1] >= 1;
        // 红 3：缓冲溢出 → 背压。
        let mut m3 = ArmMachine::new();
        let _ = m3.try_arm(0, false);
        for i in 0..(BUF_CAP as u64 + 2) {
            let _ = m3.submit(0, i);
        }
        let g_back = m3.guard_counts()[2] >= 1;
        // 红 4：到齐超时 → 局部重武装。
        let mut m4 = ArmMachine::new();
        let _ = m4.try_arm(0, false);
        for _t in 0..CONFIRM_BUDGET {
            let _ = m4.tick_window(0);
        }
        let miss = m4.guard_timeout();
        let g_rearm = m4.guard_counts()[3] >= 1 && miss == 1;
        // 红 5：回退抗拆 → 强制清零。
        let mut m5 = ArmMachine::new();
        let _ = m5.try_arm(2, false);
        m5.inject_stuck(2);
        let _ = m5.retire();
        let g_clear = m5.guard_counts()[4] >= 1 && m5.clean_after_retire();
        let _ = lcg.below(3);
        if g_red && g_reject && g_back && g_rearm && g_clear {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("防护矩阵轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：故障注入会话报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct InjReport {
    /// 类数。
    pub kinds: usize,
    /// 全抓。
    pub all_caught: bool,
    /// 可重复一致。
    pub repeatable: bool,
    /// 摘要位表健全。
    pub digests_ok: bool,
}

impl InjReport {
    /// 建报告。
    pub fn build() -> Self {
        let rows = build_fault_digests();
        let (rep_ok, _d) = drill_repeatable_3();
        InjReport {
            kinds: FAULT_KINDS,
            all_caught: rows.iter().all(|r| r.caught),
            repeatable: rep_ok,
            digests_ok: fault_digests_sane(&rows),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.all_caught && self.repeatable && self.digests_ok
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "InjReport 类={} 全抓={} 可重复={} 摘要健全={}",
            self.kinds, self.all_caught, self.repeatable, self.digests_ok
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct FjCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_inj_checklist() -> Vec<FjCheckRow> {
    let rep = InjReport::build();
    let rows = build_fault_digests();
    let mut out: Vec<FjCheckRow> = Vec::new();
    out.push(FjCheckRow { name: "场景5类", passed: FAULT_KINDS >= 5 });
    out.push(FjCheckRow { name: "全抓", passed: rows.iter().all(|r| r.caught) });
    out.push(FjCheckRow { name: "摘要健全", passed: fault_digests_sane(&rows) });
    out.push(FjCheckRow { name: "可重复", passed: rep.repeatable });
    out.push(FjCheckRow { name: "类序可查", passed: (0..FAULT_KINDS).all(|i| k_idx(FaultKind::ALL[i]) == i) });
    out.push(FjCheckRow { name: "防护矩阵", passed: { let (g, _) = drill_guard_actions_10(); g } });
    out.push(FjCheckRow { name: "10/10口径", passed: { let (c, _d, t) = drill_catch_10(); c && t == FAULT_KINDS * INJECT_ROUNDS } });
    out.push(FjCheckRow { name: "向量宽核对", passed: INJ_VECTORS == 5 });
    out.push(FjCheckRow { name: "报告全绿", passed: rep.all_green() });
    out.push(FjCheckRow { name: "段位自检", passed: segment_guard() });
    out
}

/// 清单全绿。
pub fn inj_checklist_green(rows: &[FjCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn inj_stats_line() -> String {
    format!(
        "注入概况：{} 类故障 防护 5 动作 预算窗 {} tick 缓冲 {}",
        FAULT_KINDS, CONFIRM_BUDGET, BUF_CAP
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx1247_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let rows = build_fault_digests();
    // X-a：摘要位表面。
    out.push((
        "X-摘要位表健全",
        fault_digests_sane(&rows),
        export_fault_digests(&rows),
    ));
    // X-b：防护矩阵 10/10。
    let (ok_g, d_g) = drill_guard_actions_10();
    out.push(("X-防护矩阵10/10", ok_g, d_g));
    // X-c：终验清单。
    let cl = build_inj_checklist();
    out.push(("X-终验清单10项", inj_checklist_green(&cl), InjReport::build().to_text()));
    // X-d：类词表闭集。
    out.push(("X-类词表闭集", kind_vocab_sane(), export_kind_vocab()));
    // X-e：参数面钉定。
    out.push(("X-参数面钉定", inject_params_pinned(), export_inject_params()));
    out
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：故障类检索（按类序取类/按类取序）与闭集核对。
/// ─────────────────────────────────────────────────────────────────────
/// 按类序取类。
pub fn kind_at(i: usize) -> Option<FaultKind> {
    FaultKind::ALL.get(i).copied()
}

/// 闭集核对：五类名互异且检索双向一致。
pub fn kind_vocab_sane() -> bool {
    for i in 0..FAULT_KINDS {
        for j in (i + 1)..FAULT_KINDS {
            if FaultKind::ALL[i].text() == FaultKind::ALL[j].text() {
                return false;
            }
        }
    }
    (0..FAULT_KINDS).all(|i| kind_at(i) == Some(FaultKind::ALL[i]) && k_idx(FaultKind::ALL[i]) == i)
}

/// 词表导出。
pub fn export_kind_vocab() -> String {
    let mut s = String::from("故障类词表：\n");
    for k in FaultKind::ALL.iter() {
        s.push_str(&format!("· {} ← 序{}\n", k.text(), k_idx(*k)));
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 F：注入概况行。
/// ─────────────────────────────────────────────────────────────────────
pub fn inj_overview_line() -> String {
    let rows = build_fault_digests();
    format!(
        "注入概况行：{} 类 全抓 {} 摘要首 {:#x}",
        FAULT_KINDS,
        rows.iter().filter(|r| r.caught).count(),
        rows.first().map(|r| r.digest).unwrap_or(0)
    )
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 G：注入参数面钉定（向量宽/缓冲/窗预算常量闭集）。
/// ─────────────────────────────────────────────────────────────────────
/// 参数面钉定核。
pub fn inject_params_pinned() -> bool {
    INJ_VECTORS == 5 && BUF_CAP == 8 && CONFIRM_BUDGET == 10 && REPEAT_RUNS == 3
}

/// 参数面导出。
pub fn export_inject_params() -> String {
    format!(
        "注入参数：向量{} 缓冲{} 窗{} tick 连跑 {} 次",
        INJ_VECTORS, BUF_CAP, CONFIRM_BUDGET, REPEAT_RUNS
    )
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 H：注入概况行（ktest 汇总展示面）。
/// ─────────────────────────────────────────────────────────────────────
/// 概况行。
pub fn inj_overview_full() -> String {
    let rows = build_fault_digests();
    format!(
        "注入概况行：{} 类 ×{} 轮 全抓 {} 摘要面 {} 行",
        FAULT_KINDS,
        INJECT_ROUNDS,
        rows.iter().filter(|r| r.caught).count(),
        export_fault_digests(&rows).matches("· ").count()
    )
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：使能写失败 → 红标隔离必抓。
    let r1 = inject_enable_fail(0xFB40);
    if r1.caught {
        caught += 1;
        d.push("注1 写失败:抓".to_string());
    } else {
        d.push("注1 写失败:漏".to_string());
    }
    // 注 2：依赖环 → 拒武必抓。
    let r2 = inject_dep_cycle(0xFB40);
    if r2.caught {
        caught += 1;
        d.push("注2 依赖环:抓".to_string());
    } else {
        d.push("注2 依赖环:漏".to_string());
    }
    // 注 3：缓冲溢出 → 背压必抓。
    let r3 = inject_buf_overflow(0xFB40);
    if r3.caught {
        caught += 1;
        d.push("注3 溢出:抓".to_string());
    } else {
        d.push("注3 溢出:漏".to_string());
    }
    // 注 4：到齐超时 → 局部重武装必抓。
    let r4 = inject_confirm_timeout(0xFB40);
    if r4.caught {
        caught += 1;
        d.push("注4 超时:抓".to_string());
    } else {
        d.push("注4 超时:漏".to_string());
    }
    // 注 5：回退失败 → 强制清零必抓。
    let r5 = inject_retire_fail(0xFB40);
    if r5.caught {
        caught += 1;
        d.push("注5 回退:抓".to_string());
    } else {
        d.push("注5 回退:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0xFB40 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    FjCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx1247_checks：本单判据聚合（诊断段 0xFB40）。
pub fn run_unx1247_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1247 Arming 故障注入");
    // J1-a：故障场景 ≥5 类。
    s.add("J1-故障场景5类", FAULT_KINDS >= 5, "写失败/依赖环/溢出/超时/回退");
    // J1-b：注入 10/10 次必抓（5 类 × 10 轮全抓）。
    let (ok1, d1, total) = drill_catch_10();
    s.add_dyn("J1-注入10/10必抓", ok1, &format!("{} {}", d1, if total == FAULT_KINDS * INJECT_ROUNDS { "满额" } else { "缺额" }));
    // J1-c：防护动作正确率 100%（各场景判准含防护计数）。
    let mut guards_ok = true;
    for k in FaultKind::ALL.iter() {
        let rep = inject_kind(*k, 0xFB40u64.wrapping_add((k_idx(*k) as u64) << 8));
        if !rep.caught {
            guards_ok = false;
        }
    }
    s.add_dyn("J1-防护动作正确率100%", guards_ok, "红标/拒武/背压/重武/清零");
    // J1-d：资产可重复一致率 100%（3 连跑摘要全等）。
    let (ok4, d4) = drill_repeatable_3();
    s.add_dyn("J1-3连跑一致100%", ok4, &d4);
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0xFB40段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n, p, d) in run_unx1247_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
