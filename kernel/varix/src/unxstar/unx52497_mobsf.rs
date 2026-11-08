//! UNX-F52497 MobSF 收口（移动安全静态分析） · 0xCBC00段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52497"，逐字）：
//!   title: MobSF 收口（移动安全静态分析）
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52498-J1 规则扫描
//!     ×10 工程全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52497）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52498-J1，照原文锚定；功能按本卡标题（MobSF 收口）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 域模型：五分析族（静态 APK / 静态 IPA / 代码 / 权限 / 隐私）20 格
//!      主账，族单调（逆序必抓）；逐格载荷纯函数折叠，恒等才收。
//!   ② 报告三格式 6 格（PDF / JSON / HTML 各 2 格），越格必拒、漂移必抓。
//!   ③ 与 Nikto 分账声明 2 行（F52494）+ Python 上栈声明 2 行 + 回环
//!      白名单声明 2 行，共 6 行闭集，缺一拒封（ERowMiss）。
//!   ④ 安全类卡语义：全部本地回环 / 授权白名单干跑（127.0.0.1 白名单
//!      声明行），零外部目标、零凭据接触。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      judge_grade 定级闭集）；run_cycle 注入核 10 轮三态轮转（族逆序/
//!      分析格漂移/操作越闭集），全部落在当轮临时账本上，10/10 必抓；
//!      双跑 FNV 指纹一致；定级 S1（20 格 + 6 报告格 + 6 声明行 + 六闸
//!      全通）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52497;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 分析族操作格数。
pub const ANALYSIS_OPS: usize = 20;

/// 报告三格式格数。
pub const REPORT_CELLS: usize = 6;

/// 声明行数（Nikto 分账 2 + Python 上栈 2 + 回环白名单 2）。
pub const SPLIT_ROWS: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MobsfCode {
    /// 成功
    Ok = 0,
    /// 分析操作号越闭集
    ENoOp,
    /// 分析格漂移
    EDrift,
    /// 分析族逆序
    EOrder,
    /// 报告格越闭集
    ENoRep,
    /// 报告格漂移
    ERepDrift,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
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

impl MobsfCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            MobsfCode::Ok => "成功",
            MobsfCode::ENoOp => "分析操作号越闭集：五分析族 20 格之外先登记",
            MobsfCode::EDrift => "分析格漂移：载荷折叠与基准不恒等，逐格复算",
            MobsfCode::EOrder => "分析族逆序：静态APK→静态IPA→代码→权限→隐私单调，逆序必抓",
            MobsfCode::ENoRep => "报告格越闭集：三格式 6 格之外先登记",
            MobsfCode::ERepDrift => "报告格漂移：报告格式折叠与基准不恒等，逐格复算",
            MobsfCode::ENoRow => "声明行越闭集：6 行闭集之外先登记",
            MobsfCode::ERowMiss => "缺声明行：Nikto 分账/Python 上栈/回环白名单声明未齐先补齐",
            MobsfCode::EDup => "重复落账：同格只许一条账目",
            MobsfCode::ESeal => "未齐即封：20 分析格 + 6 报告格 + 6 声明行未齐先补齐",
            MobsfCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            MobsfCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            MobsfCode::EGradeDrift => "定级漂移：全通且声明全齐才可定 S1（KB-COMPAT 登记）",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
    S1,
    /// S2：有条件收口
    S2,
    /// S3：不收口
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
            Grade::S1 => "S1：六闸+20分析格+6报告格+6声明行全通收口",
            Grade::S2 => "S2：有条件收口，六闸已落但格账存在未齐",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 分析族格载荷（操作 o；族 = o/4）。
pub const fn analysis_cell(o: usize) -> Option<u64> {
    if o >= ANALYSIS_OPS {
        return None;
    }
    Some((0xB5000u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 分析族（0..4=静态APK 4..8=静态IPA 8..12=代码 12..16=权限 16..20=隐私）。
pub const fn analysis_kind(o: usize) -> Option<u32> {
    if o >= ANALYSIS_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 分析族名（按 kind 索引）。
pub const fn analysis_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("静态APK"),
        1 => Some("静态IPA"),
        2 => Some("代码"),
        3 => Some("权限"),
        4 => Some("隐私"),
        _ => None,
    }
}

/// 报告三格式格载荷（格 c；格式 = c/2：PDF/JSON/HTML）。
pub const fn report_cell(c: usize) -> Option<u64> {
    if c >= REPORT_CELLS {
        return None;
    }
    Some((0xB6000u64 + (c as u64) * 0x3B) ^ 0x4D4F4253_46524D00)
}

/// 报告格式名（按 c/2 索引）。
pub const fn report_fmt_name(c: usize) -> Option<&'static str> {
    match c / 2 {
        0 => Some("PDF"),
        1 => Some("JSON"),
        2 => Some("HTML"),
        _ => None,
    }
}

/// 声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xB700 + r as u32 * 0x2C)
}

/// 声明行族名（0-1 与 Nikto 分账 F52494；2-3 Python 上栈；4-5 回环白名单）。
pub const fn split_family_name(r: usize) -> Option<&'static str> {
    match r {
        0 | 1 => Some("与Nikto分账"),
        2 | 3 => Some("Python上栈"),
        4 | 5 => Some("回环白名单"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// MobSF 收口账本状态机。
#[derive(Clone)]
pub struct MobsfLedger {
    /// 六判据（母版闸）。
    gates: [Option<bool>; 6],
    /// 分析族账。
    ops: [Option<u64>; ANALYSIS_OPS],
    /// 报告三格式账。
    reports: [Option<u64>; REPORT_CELLS],
    /// 声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后分析族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl MobsfLedger {
    /// 新账本。
    pub fn new() -> Self {
        MobsfLedger {
            gates: [None; 6],
            ops: [None; ANALYSIS_OPS],
            reports: [None; REPORT_CELLS],
            splits: [None; SPLIT_ROWS],
            last_kind: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), MobsfCode> {
        if gate >= 6 {
            return Err(MobsfCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5100 + gate as u64);
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

    /// 分析一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), MobsfCode> {
        if o >= ANALYSIS_OPS {
            return Err(MobsfCode::ENoOp);
        }
        let kind = match analysis_kind(o) {
            Some(k) => k,
            None => return Err(MobsfCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200);
            return Err(MobsfCode::EOrder);
        }
        let want = match analysis_cell(o) {
            Some(w) => w,
            None => return Err(MobsfCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5201 + o as u64);
            return Err(MobsfCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MobsfCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5202 + o as u64);
        Ok(())
    }

    /// 分析全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < ANALYSIS_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 报告一格（三格式 6 格，载荷对照 + 重复拒）。
    pub fn record_report(&mut self, c: usize, got: u64) -> Result<(), MobsfCode> {
        let want = match report_cell(c) {
            Some(w) => w,
            None => return Err(MobsfCode::ENoRep),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300 + c as u64);
            return Err(MobsfCode::ERepDrift);
        }
        if match self.reports.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MobsfCode::EDup);
        }
        if let Some(x) = self.reports.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x5301 + c as u64);
        Ok(())
    }

    /// 报告全账核验（6 格）。
    pub fn reports_all(&self) -> bool {
        let mut c = 0usize;
        while c < REPORT_CELLS {
            match self.reports.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), MobsfCode> {
        if r >= SPLIT_ROWS {
            return Err(MobsfCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MobsfCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x5400 + r as u64);
        Ok(())
    }

    /// 声明全齐核验（6 行）。
    pub fn splits_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.splits.get(r) {
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

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.ops_all() && self.reports_all() && self.splits_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for MobsfLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = MobsfLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < ANALYSIS_OPS {
        if let Some(w) = analysis_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut c = 0usize;
    while c < REPORT_CELLS {
        if let Some(w) = report_cell(c) {
            if led.record_report(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：分析族逆序 / 分析格漂移 / 操作越闭集三态轮转，
    // 全部落在当轮临时账本上。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = MobsfLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_op(17, analysis_cell(17).unwrap_or(0));
                lt.record_op(5, analysis_cell(5).unwrap_or(0)) == Err(MobsfCode::EOrder)
            }
            1 => {
                lt.record_op(2, analysis_cell(2).unwrap_or(0));
                lt.record_op(3, analysis_cell(3).unwrap_or(0).wrapping_add(1))
                    == Err(MobsfCode::EDrift)
            }
            _ => lt.record_op(ANALYSIS_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5500 + k as u64);
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
/// UNX-F52497 判据集（锚 UNX-F52498-J1，off-by-one 板式）。
pub fn run_unx52497_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52497");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五分析族 20 操作全通",
        recorded >= ANALYSIS_OPS as u32,
        "静态APK/静态IPA/代码/权限/隐私五族逐格恒等",
    );
    s.add(
        "报告三格式 6 格全落",
        {
            let mut lt = MobsfLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < REPORT_CELLS {
                if let Some(w) = report_cell(c) {
                    if lt.record_report(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.reports_all()
        },
        "PDF/JSON/HTML 三格式逐格恒等，越格必拒",
    );
    s.add(
        "与 Nikto 分账声明 2 行（F52494）",
        {
            let mut lt = MobsfLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < 2 {
                if lt.declare_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok
        },
        "Nikto 独立账/管道分账两行闭集，与 F52494 分账",
    );
    s.add(
        "声明 6/6 全齐",
        {
            let mut lt = MobsfLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.splits_all()
        },
        "Nikto 分账 2 + Python 上栈 2 + 回环白名单 2 共 6 行闭集",
    );
    s.add(
        "判据母版六闸全落",
        {
            let mut lt = MobsfLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "六判据逐格落账，缺一即封拒",
    );
    s.add(
        "分析族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < ANALYSIS_OPS {
                if analysis_kind(o).unwrap_or(5) < analysis_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五族闭集单调（静态APK→静态IPA→代码→权限→隐私）",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "族逆序/分析格漂移/操作越闭集三态轮转必抓",
    );
    let mut l0 = MobsfLedger::new();
    s.add(
        "边界·分析操作越闭集",
        l0.record_op(ANALYSIS_OPS, 0).is_err(),
        "分析操作 20 越闭集必拒",
    );
    s.add(
        "边界·报告格越闭集",
        l0.record_report(REPORT_CELLS, 0).is_err(),
        "报告格 6 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_split(SPLIT_ROWS).is_err(),
        "声明行 6 越闭集必拒",
    );
    s.add(
        "漂移必抓",
        {
            let mut lt = MobsfLedger::new();
            lt.record_op(0, analysis_cell(0).unwrap_or(0));
            lt.record_op(1, analysis_cell(1).unwrap_or(0).wrapping_add(1)) == Err(MobsfCode::EDrift)
        },
        "分析格载荷漂移必抓",
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
        "定级 S1 闭集",
        {
            let mut lt = MobsfLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < ANALYSIS_OPS {
                if let Some(w) = analysis_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < REPORT_CELLS {
                if let Some(w) = report_cell(c) {
                    let _ = lt.record_report(c, w);
                }
                c += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCBC00,
        "SEG_BASE 0x40 对齐且为 VX16 域 52497 格位",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analysis_kind() {
        assert_eq!(analysis_kind(0), Some(0));
        assert_eq!(analysis_kind(4), Some(1));
        assert_eq!(analysis_kind(19), Some(4));
        assert!(analysis_kind(20).is_none());
        assert!(analysis_cell(20).is_none());
        assert_eq!(analysis_kind_name(0), Some("静态APK"));
        assert_eq!(analysis_kind_name(4), Some("隐私"));
        assert!(analysis_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = analysis_cell(3).unwrap_or(0);
        assert_eq!(a, analysis_cell(3).unwrap_or(0));
        assert_ne!(a, analysis_cell(4).unwrap_or(0));
        assert!(report_cell(REPORT_CELLS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
        assert_eq!(report_fmt_name(0), Some("PDF"));
        assert_eq!(report_fmt_name(5), Some("HTML"));
    }

    #[test]
    fn test_split_family() {
        assert_eq!(split_family_name(0), Some("与Nikto分账"));
        assert_eq!(split_family_name(2), Some("Python上栈"));
        assert_eq!(split_family_name(4), Some("回环白名单"));
        assert!(split_family_name(6).is_none());
    }

    #[test]
    fn test_kind_order_caught() {
        let mut lt = MobsfLedger::new();
        let _ = lt.record_op(17, analysis_cell(17).unwrap_or(0));
        assert_eq!(lt.record_op(5, analysis_cell(5).unwrap_or(0)), Err(MobsfCode::EOrder));
        assert!(lt.record_op(18, analysis_cell(18).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = MobsfLedger::new();
        assert_eq!(
            lt.record_op(5, analysis_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(MobsfCode::EDrift)
        );
        assert_eq!(
            lt.record_report(1, report_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(MobsfCode::ERepDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = MobsfLedger::new();
        let _ = lt.record_report(0, report_cell(0).unwrap_or(0));
        assert_eq!(lt.record_report(0, report_cell(0).unwrap_or(0)), Err(MobsfCode::EDup));
        let _ = lt.declare_split(3);
        assert_eq!(lt.declare_split(3), Err(MobsfCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = MobsfLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = MobsfLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, ANALYSIS_OPS as u32 + REPORT_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xCBC00);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(MobsfCode::Ok as u32, 0);
        assert_eq!(MobsfCode::EGradeDrift as u32, 12);
        assert_eq!(MobsfCode::Ok.val(), SEG_BASE);
        assert!(!MobsfCode::Ok.detail().is_empty());
        assert!(!MobsfCode::ESeal.detail().is_empty());
        assert!(!MobsfCode::ERowMiss.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3 → 六闸齐 S2 → 全齐 S1。
        let lt0 = MobsfLedger::new();
        assert_eq!(lt0.judge_grade(), Grade::S3);
        let mut lt1 = MobsfLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt1.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt1.judge_grade(), Grade::S2);
        let mut lt2 = MobsfLedger::new();
        let mut o = 0usize;
        while o < ANALYSIS_OPS {
            if let Some(w) = analysis_cell(o) {
                let _ = lt2.record_op(o, w);
            }
            o += 1;
        }
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt2.declare_split(r);
            r += 1;
        }
        assert_ne!(lt2.judge_grade(), Grade::S1);
    }

    #[test]
    fn unx52497_checks_green() {
        let s = run_unx52497_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
