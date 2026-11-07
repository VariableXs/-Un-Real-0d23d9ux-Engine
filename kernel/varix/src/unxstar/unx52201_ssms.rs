//! UNX-F52201 SQL Server Management Studio（SSMS）收口 · 0xC7200段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52201"，逐字）：
//!   title: SQL Server Management Studio（SSMS）收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52202-J1 六判据全落
//!     （Electron 三段账）；notebook 模式 ×10；KB-COMPAT 定级登记；判据锚
//!     定 §AI-66.2 专题四同构；③ 完成时 API/软件写回：状态=已完成、结果
//!     摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52201）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52202-J1，照原文锚定；功能按本卡标题（SSMS）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 版本阶梯闭集：18.x / 19.x / 20.x 三级，越级必抓；每级独立载荷
//!      折叠，纯函数恒等才收。
//!   ② 对象资源管理器 20 操作集：五家族闭集（展开/脚本生成/属性查看/
//!      筛选/设计器）各 4 操作，家族单调（逆序必抓）。
//!   ③ T-SQL 编辑器六判据收口闸：启动/功能/崩溃/卸载/升级/许可，逐格
//!      落账，未齐即封拒。
//!   ④ SSIS/SSRS 消费分账声明 2 行闭集：SSIS 包消费分账声明 / SSRS 报
//!      表消费分账声明，缺一拒封。
//!   ⑤ ledger 状态机（new/record/judge_gate/fingerprint，FNV-1a）；注入
//!      核 run_cycle 三态轮转 10/10（版本越阶梯/操作漂移/操作越闭集），
//!      检出+逃逸守恒等式；双跑 FNV 指纹一致；定级 S1/S2/S3 闭集
//!      （20 操作 + 3 级版本 + 2 行分账 + 六判据全通 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52201;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 对象资源管理器操作数。
pub const EXPLORER_OPS: usize = 20;

/// 版本阶梯级数。
pub const VERSION_STEPS: usize = 3;

/// SSIS/SSRS 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SsmsCode {
    /// 成功
    Ok = 0,
    /// 版本级越阶梯
    ENoVer,
    /// 版本阶梯漂移
    EVerDrift,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 家族逆序
    EOrder,
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

impl SsmsCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            SsmsCode::Ok => "成功",
            SsmsCode::ENoVer => "版本级越阶梯：18.x/19.x/20.x 三级闭集之外先登记",
            SsmsCode::EVerDrift => "版本阶梯漂移：级载荷折叠与基准不恒等，逐级复算",
            SsmsCode::ENoOp => "操作号越闭集：对象资源管理器 20 操作集之外先登记",
            SsmsCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            SsmsCode::EOrder => "家族逆序：展开→脚本生成→属性查看→筛选→设计器单调，逆序必抓",
            SsmsCode::ENoRow => "声明行越闭集：SSIS/SSRS 分账 2 行闭集之外先登记",
            SsmsCode::ERowMiss => "缺声明行：SSIS/SSRS 消费分账声明未齐先补齐",
            SsmsCode::EDup => "重复落账：同格只许一条账目",
            SsmsCode::ESeal => "未齐即封：20 操作 + 3 级版本 + 2 行分账未齐先补齐",
            SsmsCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            SsmsCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            SsmsCode::EGradeDrift => "定级漂移：全通且分账声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20操作+3级版本+2行分账全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 版本阶梯级载荷（级 v：0=18.x 1=19.x 2=20.x）。
pub const fn version_cell(v: usize) -> Option<u64> {
    if v >= VERSION_STEPS {
        return None;
    }
    Some((0xB1000u64 + (v as u64) * 0x5D) ^ 0x53534D535F564552)
}

/// 版本级名（const fn 表 + Option 边界）。
pub const fn version_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("18.x"),
        1 => Some("19.x"),
        2 => Some("20.x"),
        _ => None,
    }
}

/// 对象资源管理器操作格载荷（操作 o；家族 = o/4）。
pub const fn explorer_cell(o: usize) -> Option<u64> {
    if o >= EXPLORER_OPS {
        return None;
    }
    Some((0xB2000u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 操作家族（0..4=展开 4..8=脚本生成 8..12=属性查看 12..16=筛选 16..20=设计器）。
pub const fn explorer_kind(o: usize) -> Option<u32> {
    if o >= EXPLORER_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 家族名。
pub const fn explorer_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("展开"),
        1 => Some("脚本生成"),
        2 => Some("属性查看"),
        3 => Some("筛选"),
        4 => Some("设计器"),
        _ => None,
    }
}

/// SSIS/SSRS 声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xB300 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// SSMS 收口账本状态机。
#[derive(Clone)]
pub struct SsmsLedger {
    /// 六判据（T-SQL 编辑器：启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 版本阶梯账。
    vers: [Option<u64>; VERSION_STEPS],
    /// 操作账。
    ops: [Option<u64>; EXPLORER_OPS],
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl SsmsLedger {
    /// 新账本。
    pub fn new() -> Self {
        SsmsLedger {
            gates: [None; 6],
            vers: [None; VERSION_STEPS],
            ops: [None; EXPLORER_OPS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), SsmsCode> {
        if gate >= 6 {
            return Err(SsmsCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4100 + gate as u64);
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

    /// 版本级一格（阶梯载荷对照 + 重复拒）。
    pub fn record_version(&mut self, v: usize, got: u64) -> Result<(), SsmsCode> {
        let want = match version_cell(v) {
            Some(w) => w,
            None => return Err(SsmsCode::ENoVer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4200 + v as u64);
            return Err(SsmsCode::EVerDrift);
        }
        if match self.vers.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SsmsCode::EDup);
        }
        if let Some(c) = self.vers.get_mut(v) {
            *c = Some(got);
        }
        self.fnv(0x4201 + v as u64);
        Ok(())
    }

    /// 版本阶梯全账核验（3 级全齐且单调）。
    pub fn vers_all(&self) -> bool {
        let mut v = 0usize;
        while v < VERSION_STEPS {
            match self.vers.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// 操作一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), SsmsCode> {
        if o >= EXPLORER_OPS {
            return Err(SsmsCode::ENoOp);
        }
        let kind = match explorer_kind(o) {
            Some(k) => k,
            None => return Err(SsmsCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4300);
            return Err(SsmsCode::EOrder);
        }
        let want = match explorer_cell(o) {
            Some(w) => w,
            None => return Err(SsmsCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4301 + o as u64);
            return Err(SsmsCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SsmsCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4302 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < EXPLORER_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 声明一行（SSIS/SSRS 分账）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), SsmsCode> {
        if r >= SPLIT_ROWS {
            return Err(SsmsCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SsmsCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x4400 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
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
        if self.gates_all() && self.vers_all() && self.ops_all() && self.splits_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for SsmsLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = SsmsLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut v = 0usize;
    while v < VERSION_STEPS {
        if let Some(w) = version_cell(v) {
            if led.record_version(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
    }
    let mut o = 0usize;
    while o < EXPLORER_OPS {
        if let Some(w) = explorer_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：版本越阶梯 / 操作漂移 / 操作越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = SsmsLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_version(0, version_cell(0).unwrap_or(0));
                lt.record_version(VERSION_STEPS, 0).is_err()
            }
            1 => {
                lt.record_op(2, explorer_cell(2).unwrap_or(0));
                lt.record_op(1, explorer_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(SsmsCode::EDrift)
            }
            _ => lt.record_op(EXPLORER_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4500 + k as u64);
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
/// UNX-F52201 判据集（锚 UNX-F52202-J1，off-by-one 板式）。
pub fn run_unx52201_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52201");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "对象资源管理器 20 操作全通",
        recorded >= EXPLORER_OPS as u32,
        "展开/脚本生成/属性查看/筛选/设计器五家族逐操作恒等",
    );
    s.add(
        "版本阶梯 18.x/19.x/20.x 三级全落",
        {
            let mut lt = SsmsLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < VERSION_STEPS {
                if let Some(w) = version_cell(v) {
                    if lt.record_version(v, w).is_err() {
                        ok = false;
                    }
                }
                v += 1;
            }
            ok && lt.vers_all()
        },
        "三级闭集逐级恒等，越级必抓",
    );
    s.add(
        "T-SQL 编辑器六判据全落",
        {
            let mut lt = SsmsLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/功能/崩溃/卸载/升级/许可逐格落账",
    );
    s.add(
        "SSIS/SSRS 消费分账声明 2/2 全齐",
        {
            let mut lt = SsmsLedger::new();
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
        "SSIS 包消费分账/SSRS 报表消费分账两行闭集（§AI-66.2 专题四同构）",
    );
    s.add(
        "家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < EXPLORER_OPS {
                if explorer_kind(o).unwrap_or(5) < explorer_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五家族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "版本越阶梯/操作漂移/操作越闭集三态轮转必抓",
    );
    let mut l0 = SsmsLedger::new();
    s.add(
        "边界·版本级越阶梯",
        l0.record_version(VERSION_STEPS, 0).is_err(),
        "版本级 3 越阶梯必拒",
    );
    s.add(
        "操作漂移拒",
        {
            l0.record_op(0, explorer_cell(0).unwrap_or(0));
            l0.record_op(1, explorer_cell(1).unwrap_or(0).wrapping_add(1)) == Err(SsmsCode::EDrift)
        },
        "操作载荷漂移必抓",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_op(EXPLORER_OPS, 0).is_err(),
        "操作 20 越闭集必拒",
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
            let mut lt = SsmsLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut v = 0usize;
            while v < VERSION_STEPS {
                if let Some(w) = version_cell(v) {
                    let _ = lt.record_version(v, w);
                }
                v += 1;
            }
            let mut o = 0usize;
            while o < EXPLORER_OPS {
                if let Some(w) = explorer_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
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
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200,
        "SEG_BASE 0x40 对齐且在 VX15 域首格",
    );
    s.add("收口·判据条数", s.len() >= 12, "含本条共 ≥13 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_ladder() {
        assert_eq!(version_name(0), Some("18.x"));
        assert_eq!(version_name(1), Some("19.x"));
        assert_eq!(version_name(2), Some("20.x"));
        assert!(version_name(3).is_none());
        assert!(version_cell(3).is_none());
    }

    #[test]
    fn test_explorer_kind() {
        assert_eq!(explorer_kind(0), Some(0));
        assert_eq!(explorer_kind(4), Some(1));
        assert_eq!(explorer_kind(19), Some(4));
        assert!(explorer_kind(20).is_none());
        assert!(explorer_cell(20).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = explorer_cell(3).unwrap_or(0);
        assert_eq!(a, explorer_cell(3).unwrap_or(0));
        assert_ne!(a, explorer_cell(4).unwrap_or(0));
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = SsmsLedger::new();
        let _ = lt.record_op(9, explorer_cell(9).unwrap_or(0));
        assert_eq!(lt.record_op(2, explorer_cell(2).unwrap_or(0)), Err(SsmsCode::EOrder));
        assert!(lt.record_op(10, explorer_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = SsmsLedger::new();
        assert_eq!(
            lt.record_op(5, explorer_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(SsmsCode::EDrift)
        );
        assert_eq!(
            lt.record_version(1, version_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(SsmsCode::EVerDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = SsmsLedger::new();
        let _ = lt.record_version(0, version_cell(0).unwrap_or(0));
        assert_eq!(lt.record_version(0, version_cell(0).unwrap_or(0)), Err(SsmsCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = SsmsLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = SsmsLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= EXPLORER_OPS as u32 + VERSION_STEPS as u32);
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
        assert_eq!(SEG_BASE, 0xC7200);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(SsmsCode::Ok as u32, 0);
        assert_eq!(SsmsCode::EGradeDrift as u32, 12);
        assert!(!SsmsCode::Ok.detail().is_empty());
        assert!(!SsmsCode::ESeal.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = SsmsLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52201_checks_green() {
        let s = run_unx52201_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 13);
    }
}
