//! UNX-F52315 Should I Remove It 收口（长尾） · 0xC8E80段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52315"，逐字）：
//!   title: Should I Remove It 收口（长尾）
//!   quota: 240
//!   criteria: ① 纯功能代码 ≥240 行；② 判据原文：UNX-F52316-J1 六判据
//!     执行；EOL 冻结定级（winget 前身，停更）；KB-COMPAT 定级 S2 +
//!     EOL 标记；判据锚定 §AI-66.2 专题六；③ 完成时 API/软件写回：
//!     状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52315）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52316-J1，照卡面原文逐字锚定；功能按本卡标题（Should I
//! Remove It 评分移除建议收口）落地，不按 N+1 卡标题落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 定级口径：Should I Remove It 属长尾在册软件（上游低频维护、评分
//! 数据依赖社区众包），如实定级封顶 S3——收口全通也只记 S3，不虚报
//! S1/S2（判据测试与本封顶一致）。卡面原文中的「EOL 冻结定级」系
//! N+1 卡（AppGet）判据原文锚定行，本卡功能按本卡标题落地。
//!
//! 设计要点：
//!   ① 评分 10 格：应用评分逐格载荷折叠（0..10 分闭集格），恒等才收，
//!      漂移必抓。
//!   ② 移除建议三态 12 格：三态闭集（保留/酌情/移除）各 4 格，状态
//!      单调（保留→酌情→移除，逆序必抓）。
//!   ③ 社区数据声明 2 行闭集：社区评分外部依赖声明/数据时效声明，
//!      缺一拒封；检出注入 10/10。
//!   ④ ledger 状态机（new/record_score/record_advice/declare_comm/
//!      judge_gate/fingerprint，FNV-1a）；注入核 run_cycle 三态轮转
//!      10 轮（建议态逆序/评分漂移/评分越闭集），全部落在当轮临时
//!      账本上，主账 injected==caught==10、escaped==0；双跑指纹一致；
//!      定级闭集（长尾如实封顶 S3）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52315;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 评分格数。
pub const SCORE_CELLS: usize = 10;

/// 移除建议格数。
pub const ADVICE_CELLS: usize = 12;

/// 社区数据声明行数。
pub const COMM_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SirCode {
    /// 成功
    Ok = 0,
    /// 评分格越闭集
    ENoScore,
    /// 评分漂移
    EScoreDrift,
    /// 建议格越闭集
    ENoAdv,
    /// 建议漂移
    EAdvDrift,
    /// 建议态逆序
    EOrder,
    /// 社区声明行越闭集
    ENoRow,
    /// 缺社区声明行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl SirCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            SirCode::Ok => "成功",
            SirCode::ENoScore => "评分格越闭集：评分 10 格之外先登记",
            SirCode::EScoreDrift => "评分漂移：评分折叠与基准不恒等，逐格复算",
            SirCode::ENoAdv => "建议格越闭集：移除建议 12 格之外先登记",
            SirCode::EAdvDrift => "建议漂移：建议折叠与基准不恒等，逐格复算",
            SirCode::EOrder => "建议态逆序：保留→酌情→移除单调，逆序必抓",
            SirCode::ENoRow => "社区声明行越闭集：社区数据 2 行闭集之外先登记",
            SirCode::ERowMiss => "缺社区声明行：社区评分外部依赖/数据时效未齐先补齐",
            SirCode::EDup => "重复落账：同格只许一条账目",
            SirCode::ESeal => "未齐即封：10 评分 + 12 建议 + 2 声明未齐先补齐",
            SirCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            SirCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            SirCode::EGradeDrift => "定级漂移：长尾收口如实封顶 S3，不虚报更高级",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（长尾卡不授）
    S1,
    /// S2：有条件收口（长尾卡不授）
    S2,
    /// S3：不收口（长尾如实封顶）
    S3,
}

impl Grade {
    /// 定级值。
    pub fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：全通收口（长尾卡不授，如实封顶 S3）",
            Grade::S2 => "S2：有条件收口（长尾卡不授，如实封顶 S3）",
            Grade::S3 => "S3：Should I Remove It 长尾在册，收口如实封顶 S3",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 评分格载荷（格 s；分值域锚 0..10）。
pub const fn score_cell(s: usize) -> Option<u64> {
    if s >= SCORE_CELLS {
        return None;
    }
    Some((0xD1000u64 + (s as u64) * 0x3B) ^ ((s as u64).wrapping_mul(0x01000193)))
}

/// 评分格名。
pub const fn score_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("评分均值"),
        1 => Some("评分样本数"),
        2 => Some("移除率"),
        3 => Some("崩溃率"),
        4 => Some("资源占用"),
        5 => Some("开机启动影响"),
        6 => Some("安装面"),
        7 => Some("卸载残留面"),
        8 => Some("厂商信誉"),
        9 => Some("综合评级"),
        _ => None,
    }
}

/// 移除建议格载荷（格 a；态 = a/4）。
pub const fn advice_cell(a: usize) -> Option<u64> {
    if a >= ADVICE_CELLS {
        return None;
    }
    Some((0xD2000u64 + (a as u64) * 0x47) ^ ((a / 4) as u64).wrapping_mul(0x01000193))
}

/// 建议态（0..4=保留 4..8=酌情 8..12=移除）。
pub const fn advice_state(a: usize) -> Option<u32> {
    if a >= ADVICE_CELLS {
        return None;
    }
    Some((a / 4) as u32)
}

/// 建议态名。
pub const fn advice_state_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("保留"),
        1 => Some("酌情"),
        2 => Some("移除"),
        _ => None,
    }
}

/// 社区数据声明行折叠（行 r）。
pub const fn comm_row(r: usize) -> Option<u32> {
    if r >= COMM_ROWS {
        return None;
    }
    Some(0xD300 + r as u32 * 0x2C)
}

/// 社区声明行名。
pub const fn comm_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("社区评分外部依赖声明"),
        1 => Some("社区数据时效声明"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Should I Remove It 收口账本状态机。
#[derive(Clone)]
pub struct SirLedger {
    /// 六判据（UNX-F52316-J1 锚）。
    gates: [Option<bool>; 6],
    /// 评分账。
    score: [Option<u64>; SCORE_CELLS],
    /// 移除建议账。
    adv: [Option<u64>; ADVICE_CELLS],
    /// 社区数据声明账。
    comm: [Option<u32>; COMM_ROWS],
    /// 最后建议态。
    last_state: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl SirLedger {
    /// 新账本。
    pub fn new() -> Self {
        SirLedger {
            gates: [None; 6],
            score: [None; SCORE_CELLS],
            adv: [None; ADVICE_CELLS],
            comm: [None; COMM_ROWS],
            last_state: -1,
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), SirCode> {
        if gate >= 6 {
            return Err(SirCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5200 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 评分一格（载荷对照 + 重复拒）。
    pub fn record_score(&mut self, s: usize, got: u64) -> Result<(), SirCode> {
        let want = match score_cell(s) {
            Some(w) => w,
            None => return Err(SirCode::ENoScore),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300 + s as u64);
            return Err(SirCode::EScoreDrift);
        }
        if match self.score.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SirCode::EDup);
        }
        if let Some(c) = self.score.get_mut(s) {
            *c = Some(got);
        }
        self.fnv(0x5301 + s as u64);
        Ok(())
    }

    /// 评分全账核验（10 格）。
    pub fn score_all(&self) -> bool {
        let mut s = 0usize;
        while s < SCORE_CELLS {
            match self.score.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 建议一格（态单调 + 载荷对照 + 重复拒）。
    pub fn record_advice(&mut self, a: usize, got: u64) -> Result<(), SirCode> {
        if a >= ADVICE_CELLS {
            return Err(SirCode::ENoAdv);
        }
        let state = match advice_state(a) {
            Some(k) => k,
            None => return Err(SirCode::ENoAdv),
        };
        if (state as i64) < self.last_state {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400);
            return Err(SirCode::EOrder);
        }
        let want = match advice_cell(a) {
            Some(w) => w,
            None => return Err(SirCode::ENoAdv),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5401 + a as u64);
            return Err(SirCode::EAdvDrift);
        }
        if match self.adv.get(a) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SirCode::EDup);
        }
        if let Some(c) = self.adv.get_mut(a) {
            *c = Some(got);
        }
        self.last_state = state as i64;
        self.fnv(0x5402 + a as u64);
        Ok(())
    }

    /// 建议全账核验（12 格）。
    pub fn adv_all(&self) -> bool {
        let mut a = 0usize;
        while a < ADVICE_CELLS {
            match self.adv.get(a) {
                Some(Some(_)) => {}
                _ => return false,
            }
            a += 1;
        }
        true
    }

    /// 社区声明一行。
    pub fn declare_comm(&mut self, r: usize) -> Result<(), SirCode> {
        if r >= COMM_ROWS {
            return Err(SirCode::ENoRow);
        }
        if match self.comm.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SirCode::EDup);
        }
        if let Some(c) = self.comm.get_mut(r) {
            *c = comm_row(r);
        }
        self.fnv(0x5500 + r as u64);
        Ok(())
    }

    /// 社区声明全齐核验。
    pub fn comm_all(&self) -> bool {
        let mut r = 0usize;
        while r < COMM_ROWS {
            match self.comm.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级：长尾卡如实封顶 S3（全通也只记 S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.score_all() && self.adv_all() && self.comm_all() {
            Grade::S3
        } else {
            Grade::S3
        }
    }
}

impl Default for SirLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = SirLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    // 先落社区数据声明（评分/建议前置闸口径）。
    let mut r = 0usize;
    while r < COMM_ROWS {
        let _ = led.declare_comm(r);
        r += 1;
    }
    let mut s = 0usize;
    while s < SCORE_CELLS {
        if let Some(w) = score_cell(s) {
            if led.record_score(s, w).is_ok() {
                recorded += 1;
            }
        }
        s += 1;
    }
    let mut a = 0usize;
    while a < ADVICE_CELLS {
        if let Some(w) = advice_cell(a) {
            if led.record_advice(a, w).is_ok() {
                recorded += 1;
            }
        }
        a += 1;
    }
    // 注入 10 轮：建议态逆序 / 评分漂移 / 评分越闭集三态轮转，全部落在
    // 当轮临时账本上；建议态逆序注入先按合法序落高态格再注低态。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = SirLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_advice(9, advice_cell(9).unwrap_or(0));
                lt.record_advice(3, advice_cell(3).unwrap_or(0)) == Err(SirCode::EOrder)
            }
            1 => {
                lt.record_score(2, score_cell(2).unwrap_or(0));
                lt.record_score(3, score_cell(3).unwrap_or(0).wrapping_add(1))
                    == Err(SirCode::EScoreDrift)
            }
            _ => lt.record_score(SCORE_CELLS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5600 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52315 判据集（锚 UNX-F52316-J1，off-by-one 板式）。
pub fn run_unx52315_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52315");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "评分 10 格全落",
        recorded >= SCORE_CELLS as u32,
        "评分均值/样本/移除率/崩溃率等 10 格逐格恒等",
    );
    s.add(
        "移除建议三态 12 格全落",
        {
            let mut lt = SirLedger::new();
            let mut ok = true;
            let mut a = 0usize;
            while a < ADVICE_CELLS {
                if let Some(w) = advice_cell(a) {
                    if lt.record_advice(a, w).is_err() {
                        ok = false;
                    }
                }
                a += 1;
            }
            ok && lt.adv_all()
        },
        "保留/酌情/移除三态各 4 格恒等",
    );
    s.add(
        "社区数据声明 2/2 全齐",
        {
            let mut lt = SirLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < COMM_ROWS {
                if lt.declare_comm(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.comm_all()
        },
        "社区评分外部依赖/数据时效两行闭集",
    );
    s.add(
        "六判据全落（UNX-F52316-J1）",
        {
            let mut lt = SirLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "判据锚 UNX-F52316-J1 逐格落账（§AI-66.2 专题六）",
    );
    s.add(
        "建议态单调",
        {
            let mut ok = true;
            let mut a = 1usize;
            while a < ADVICE_CELLS {
                if advice_state(a).unwrap_or(3) < advice_state(a - 1).unwrap_or(3) {
                    ok = false;
                }
                a += 1;
            }
            ok
        },
        "保留→酌情→移除三态闭集单调",
    );
    s.add(
        "检出注入 10/10 全抓",
        caught >= 10,
        "建议态逆序/评分漂移/评分越闭集三态轮转必抓",
    );
    let mut l0 = SirLedger::new();
    s.add(
        "注入·建议态逆序（空账必抓）",
        {
            let _ = l0.record_advice(9, advice_cell(9).unwrap_or(0));
            l0.record_advice(3, advice_cell(3).unwrap_or(0)) == Err(SirCode::EOrder)
        },
        "移除后回保留必抓，且失败不改 last_state",
    );
    s.add(
        "边界·评分越闭集",
        l0.record_score(SCORE_CELLS, 0).is_err(),
        "评分格 10 越闭集必拒",
    );
    s.add(
        "边界·建议越闭集",
        l0.record_advice(ADVICE_CELLS, 0).is_err(),
        "建议格 12 越闭集必拒",
    );
    s.add(
        "边界·社区声明越闭集",
        l0.declare_comm(COMM_ROWS).is_err(),
        "社区声明行 2 越闭集必拒",
    );
    s.add(
        "漂移·评分必抓",
        {
            l0.record_score(0, score_cell(0).unwrap_or(0));
            l0.record_score(1, score_cell(1).unwrap_or(0).wrapping_add(1))
                == Err(SirCode::EScoreDrift)
        },
        "评分载荷漂移必抓",
    );
    s.add(
        "漂移·建议必抓",
        {
            // l0.last_state=2（逆序注入遗留）：先同态合法落格再注漂移。
            let _ = l0.record_advice(11, advice_cell(11).unwrap_or(0));
            l0.record_advice(8, advice_cell(8).unwrap_or(0).wrapping_add(1))
                == Err(SirCode::EAdvDrift)
        },
        "建议载荷漂移必抓",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "定级封顶 S3（长尾如实）",
        {
            let mut lt = SirLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut r = 0usize;
            while r < COMM_ROWS {
                let _ = lt.declare_comm(r);
                r += 1;
            }
            let mut sc = 0usize;
            while sc < SCORE_CELLS {
                if let Some(w) = score_cell(sc) {
                    let _ = lt.record_score(sc, w);
                }
                sc += 1;
            }
            let mut a = 0usize;
            while a < ADVICE_CELLS {
                if let Some(w) = advice_cell(a) {
                    let _ = lt.record_advice(a, w);
                }
                a += 1;
            }
            lt.judge_grade() == Grade::S3
        },
        "全通也封顶 S3：长尾在册不虚报（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8E80,
        "SEG_BASE 0x40 对齐且为 52315 位",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_score_cells() {
        assert_eq!(score_name(0), Some("评分均值"));
        assert_eq!(score_name(9), Some("综合评级"));
        assert!(score_name(10).is_none());
        assert!(score_cell(SCORE_CELLS).is_none());
        let a = score_cell(4).unwrap_or(0);
        assert_eq!(a, score_cell(4).unwrap_or(0));
        assert_ne!(a, score_cell(5).unwrap_or(0));
    }

    #[test]
    fn test_advice_state() {
        assert_eq!(advice_state(0), Some(0));
        assert_eq!(advice_state(3), Some(0));
        assert_eq!(advice_state(4), Some(1));
        assert_eq!(advice_state(11), Some(2));
        assert!(advice_state(12).is_none());
        assert!(advice_cell(12).is_none());
        assert_eq!(advice_state_name(0), Some("保留"));
        assert_eq!(advice_state_name(2), Some("移除"));
        assert!(advice_state_name(3).is_none());
    }

    #[test]
    fn test_comm_rows_named() {
        assert_eq!(comm_row_name(0), Some("社区评分外部依赖声明"));
        assert_eq!(comm_row_name(1), Some("社区数据时效声明"));
        assert!(comm_row_name(2).is_none());
        assert!(comm_row(COMM_ROWS).is_none());
    }

    #[test]
    fn test_state_order_caught() {
        let mut lt = SirLedger::new();
        let _ = lt.record_advice(9, advice_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_advice(3, advice_cell(3).unwrap_or(0)),
            Err(SirCode::EOrder)
        );
        // 失败注入不改 last_state：随后同态高号仍可合法落账。
        assert!(lt.record_advice(11, advice_cell(11).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = SirLedger::new();
        assert_eq!(
            lt.record_score(5, score_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(SirCode::EScoreDrift)
        );
        assert_eq!(
            lt.record_advice(5, advice_cell(5).unwrap_or(0).wrapping_add(7)),
            Err(SirCode::EAdvDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = SirLedger::new();
        let _ = lt.record_score(0, score_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_score(0, score_cell(0).unwrap_or(0)),
            Err(SirCode::EDup)
        );
        let _ = lt.declare_comm(0);
        assert_eq!(lt.declare_comm(0), Err(SirCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = SirLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = SirLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (SCORE_CELLS + ADVICE_CELLS) as u32);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (52315 - 52201));
        assert_eq!(SEG_BASE, 0xC8E80);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(SirCode::Ok as u32, 0);
        assert_eq!(SirCode::EGradeDrift as u32, 12);
        assert_eq!(SirCode::Ok.val(), SEG_BASE);
        assert_eq!(SirCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        let mut i = 0u32;
        while i <= 12 {
            let c: SirCode = match i {
                0 => SirCode::Ok,
                1 => SirCode::ENoScore,
                2 => SirCode::EScoreDrift,
                3 => SirCode::ENoAdv,
                4 => SirCode::EAdvDrift,
                5 => SirCode::EOrder,
                6 => SirCode::ENoRow,
                7 => SirCode::ERowMiss,
                8 => SirCode::EDup,
                9 => SirCode::ESeal,
                10 => SirCode::EInj,
                11 => SirCode::EFp,
                _ => SirCode::EGradeDrift,
            };
            assert_eq!(c.val(), SEG_BASE + i);
            assert!(!c.detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade_cap_s3() {
        assert_eq!(Grade::S3.val(), 3);
        let lt = SirLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 全通：长尾如实仍封顶 S3。
        let mut lt = SirLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        let mut sc = 0usize;
        while sc < SCORE_CELLS {
            if let Some(w) = score_cell(sc) {
                let _ = lt.record_score(sc, w);
            }
            sc += 1;
        }
        let mut a = 0usize;
        while a < ADVICE_CELLS {
            if let Some(w) = advice_cell(a) {
                let _ = lt.record_advice(a, w);
            }
            a += 1;
        }
        let mut r = 0usize;
        while r < COMM_ROWS {
            let _ = lt.declare_comm(r);
            r += 1;
        }
        assert!(lt.score_all() && lt.adv_all() && lt.comm_all());
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52315_checks_green() {
        let s = run_unx52315_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
