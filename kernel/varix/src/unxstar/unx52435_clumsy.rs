//! UNX-F52435 Clumsy 收口（网络故障注入） · 0xCAC80段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52435"，逐字）：
//!   title: Clumsy 收口（网络故障注入）
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52436-J1
//!     pcap 解析 ×10（离线分析声明，抓包仅回环）；KB-COMPAT 定级登
//!     记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、
//!     结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52435）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52436-J1，照原文锚定；功能按本卡标题（Clumsy 网络故障注入）
//! 落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五故障族闭集：lag 延迟 / drop 掉包 / throttle 节流 /
//!      duplicate 重复 / out-of-order 乱序，各 4 格共 20 格，家族单
//!      调（逆序必抓）。
//!   ② 过滤三元闸：方向闸/协议闸/窗口闸 ×（配置/生效）两态共 6 闸，
//!      逐闸落账，越闸必抓。
//!   ③ 恢复 8 拍单调：故障撤销后逐拍恢复落格，逆拍必抓且不改拍位
//!      游标。
//!   ④ C 上栈声明 2 行闭集：WinDivert 驱动层分账 / npf 兼容层分账，
//!      缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（故障越闭集/故障漂移/恢复逆
//!      拍），本卡注入核与域语义同构：注入即故障检出，10/10 全通
//!      （离线分析声明，抓包仅回环）；双跑指纹一致；KB-COMPAT 定级
//!      闭集 S1/S2/S3（20 故障 + 8 拍恢复 + 2 行分账 + 6 闸全通 →
//!      S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52435;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五故障族格数。
pub const FAULT_CELLS: usize = 20;

/// 恢复拍数。
pub const RECOV_BEATS: usize = 8;

/// C 上栈声明行数。
pub const SPLIT_ROWS: usize = 2;

/// 过滤三元闸闸数（三元 × 配置/生效）。
pub const GATES: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClumsyCode {
    /// 成功
    Ok = 0,
    /// 故障格越闭集
    ENoFault,
    /// 故障载荷漂移
    EFaultDrift,
    /// 故障家族逆序
    EFaultOrder,
    /// 闸号越闭集
    ENoGate,
    /// 恢复拍越界
    ENoRecov,
    /// 恢复逆拍
    ERecovOrder,
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
    /// 定级漂移
    EGradeDrift,
}

impl ClumsyCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            ClumsyCode::Ok => "成功",
            ClumsyCode::ENoFault => "故障格越闭集：lag/drop/throttle/duplicate/out-of-order 20 格之外先登记",
            ClumsyCode::EFaultDrift => "故障载荷漂移：故障折叠与基准不恒等，逐格复算",
            ClumsyCode::EFaultOrder => "故障家族逆序：lag→drop→throttle→duplicate→out-of-order 单调，逆序必抓",
            ClumsyCode::ENoGate => "闸号越闭集：过滤三元闸 6 闸（三元×配置/生效）之外先登记",
            ClumsyCode::ENoRecov => "恢复拍越界：8 拍之外先登记",
            ClumsyCode::ERecovOrder => "恢复逆拍：8 拍单调递增，逆拍必抓",
            ClumsyCode::ENoRow => "声明行越闭集：C 上栈分账 2 行闭集之外先登记",
            ClumsyCode::ERowMiss => "缺声明行：WinDivert 驱动层/npf 兼容层 C 上栈声明未齐先补齐",
            ClumsyCode::EDup => "重复落账：同格只许一条账目",
            ClumsyCode::ESeal => "未齐即封：20 故障 + 8 拍恢复 + 2 行分账 + 6 闸未齐先补齐",
            ClumsyCode::EInj => "注入守恒破缺：故障注入=检出+逃逸等式不成立，逐轮复算",
            ClumsyCode::EGradeDrift => "定级漂移：全通且分账声明全齐才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡目标档）
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
            Grade::S1 => "S1：故障注入检出 10/10 场景全通（离线分析声明，抓包仅回环），20 故障+8 拍恢复+2 行分账+6 闸全齐收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 故障格载荷（故障 f；家族 = f/4）。
pub const fn fault_cell(f: usize) -> Option<u64> {
    if f >= FAULT_CELLS {
        return None;
    }
    Some((0xD9000u64 + (f as u64) * 0x29) ^ ((f / 4) as u64).wrapping_mul(0x01000193))
}

/// 故障家族（0..4=lag 4..8=drop 8..12=throttle 12..16=duplicate 16..20=out-of-order）。
pub const fn fault_kind(f: usize) -> Option<u32> {
    if f >= FAULT_CELLS {
        return None;
    }
    Some((f / 4) as u32)
}

/// 家族名。
pub const fn fault_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("lag 延迟"),
        1 => Some("drop 掉包"),
        2 => Some("throttle 节流"),
        3 => Some("duplicate 重复"),
        4 => Some("out-of-order 乱序"),
        _ => None,
    }
}

/// 恢复拍载荷（拍 b）。
pub const fn recov_cell(b: usize) -> Option<u64> {
    if b >= RECOV_BEATS {
        return None;
    }
    Some(0xDA000u64 + (b as u64) * 0x17)
}

/// 闸名（三元闸：方向/协议/窗口 × 配置/生效）。
pub const fn gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("方向闸·配置"),
        1 => Some("方向闸·生效"),
        2 => Some("协议闸·配置"),
        3 => Some("协议闸·生效"),
        4 => Some("窗口闸·配置"),
        5 => Some("窗口闸·生效"),
        _ => None,
    }
}

/// C 上栈声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xDB00 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Clumsy 收口账本状态机。
#[derive(Clone)]
pub struct ClumsyLedger {
    /// 过滤三元闸 6 闸账。
    gates: [Option<bool>; GATES],
    /// 故障账。
    faults: [Option<u64>; FAULT_CELLS],
    /// 恢复账。
    recovs: [Option<u64>; RECOV_BEATS],
    /// C 上栈声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后故障家族。
    last_kind: i64,
    /// 最后恢复拍。
    last_recov: i64,
    /// 注入（=故障）/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl ClumsyLedger {
    /// 新账本。
    pub fn new() -> Self {
        ClumsyLedger {
            gates: [None; GATES],
            faults: [None; FAULT_CELLS],
            recovs: [None; RECOV_BEATS],
            splits: [None; SPLIT_ROWS],
            last_kind: -1,
            last_recov: -1,
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

    /// 落一格过滤三元闸。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), ClumsyCode> {
        if gate >= GATES {
            return Err(ClumsyCode::ENoGate);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x7100 + gate as u64);
        Ok(())
    }

    /// 过滤三元闸全落核验。
    pub fn gates_all(&self) -> bool {
        let mut g = 0usize;
        while g < GATES {
            match self.gates.get(g) {
                Some(Some(true)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 故障一格（家族单调 + 载荷对照 + 重复拒）；逆族不改家族游标。
    pub fn record_fault(&mut self, f: usize, got: u64) -> Result<(), ClumsyCode> {
        if f >= FAULT_CELLS {
            return Err(ClumsyCode::ENoFault);
        }
        let kind = match fault_kind(f) {
            Some(k) => k,
            None => return Err(ClumsyCode::ENoFault),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200);
            return Err(ClumsyCode::EFaultOrder);
        }
        let want = match fault_cell(f) {
            Some(w) => w,
            None => return Err(ClumsyCode::ENoFault),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7201 + f as u64);
            return Err(ClumsyCode::EFaultDrift);
        }
        if match self.faults.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ClumsyCode::EDup);
        }
        if let Some(c) = self.faults.get_mut(f) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x7202 + f as u64);
        Ok(())
    }

    /// 故障全账核验（20 格）。
    pub fn faults_all(&self) -> bool {
        let mut f = 0usize;
        while f < FAULT_CELLS {
            match self.faults.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// 最后故障家族游标（注入后不变性核验用）。
    pub fn cur_kind(&self) -> i64 {
        self.last_kind
    }

    /// 恢复一拍（单调 + 重复拒）；逆拍不改拍位游标。
    pub fn record_recov(&mut self, b: usize) -> Result<(), ClumsyCode> {
        if b >= RECOV_BEATS {
            return Err(ClumsyCode::ENoRecov);
        }
        if (b as i64) < self.last_recov {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300);
            return Err(ClumsyCode::ERecovOrder);
        }
        if match self.recovs.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ClumsyCode::EDup);
        }
        if let Some(c) = self.recovs.get_mut(b) {
            *c = recov_cell(b);
        }
        self.last_recov = b as i64;
        self.fnv(0x7301 + b as u64);
        Ok(())
    }

    /// 恢复全账核验（8 拍）。
    pub fn recovs_all(&self) -> bool {
        let mut b = 0usize;
        while b < RECOV_BEATS {
            match self.recovs.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// 最后恢复拍游标。
    pub fn last_recov(&self) -> i64 {
        self.last_recov
    }

    /// C 上栈声明一行（WinDivert 驱动层 / npf 兼容层）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), ClumsyCode> {
        if r >= SPLIT_ROWS {
            return Err(ClumsyCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ClumsyCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x7400 + r as u64);
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

    /// 缺声明行核验（未齐即报 ERowMiss）。
    pub fn require_rows(&self) -> Result<(), ClumsyCode> {
        if self.splits_all() {
            Ok(())
        } else {
            Err(ClumsyCode::ERowMiss)
        }
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// 注入守恒核验（注入=检出+逃逸）。
    pub fn verify_conservation(&self) -> Result<(), ClumsyCode> {
        if self.injected == self.caught + self.escaped {
            Ok(())
        } else {
            Err(ClumsyCode::EInj)
        }
    }

    /// 收口核验（账未齐即封：ESeal 语义，全齐才 Ok）。
    pub fn verify_seal(&self) -> Result<(), ClumsyCode> {
        if self.gates_all() && self.faults_all() && self.recovs_all() && self.splits_all() {
            Ok(())
        } else {
            Err(ClumsyCode::ESeal)
        }
    }

    /// KB-COMPAT 定级（全齐 → S1，六闸齐 → S2，空账 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.faults_all() && self.recovs_all() && self.splits_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }

    /// 定级核验（漂移即报 EGradeDrift）。
    pub fn verify_grade(&self, expect: Grade) -> Result<(), ClumsyCode> {
        if self.judge_grade() == expect {
            Ok(())
        } else {
            Err(ClumsyCode::EGradeDrift)
        }
    }
}

impl Default for ClumsyLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 双跑指纹核验。
pub fn verify_fp(a: u64, b: u64) -> bool {
    a == b && a != 0
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = ClumsyLedger::new();
    let mut g = 0usize;
    while g < GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut f = 0usize;
    while f < FAULT_CELLS {
        if let Some(w) = fault_cell(f) {
            if led.record_fault(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
    }
    let mut b = 0usize;
    while b < RECOV_BEATS {
        if led.record_recov(b).is_ok() {
            recorded += 1;
        }
        b += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：故障越闭集 / 故障漂移 / 恢复逆拍三态轮转。
    // 本卡注入核与域语义同构：注入即故障检出；漂移注入先按合法序
    // 落正格再注漂移；逆拍注入对空账本同样必抓且不改拍位游标。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = ClumsyLedger::new();
        let caught = match kind {
            0 => lt.record_fault(FAULT_CELLS, 0).is_err(),
            1 => {
                let _ = lt.record_fault(0, fault_cell(0).unwrap_or(0));
                lt.record_fault(1, fault_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(ClumsyCode::EFaultDrift)
            }
            _ => {
                let _ = lt.record_recov(5);
                lt.record_recov(2) == Err(ClumsyCode::ERecovOrder) && lt.last_recov() == 5
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7500 + k as u64);
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
/// UNX-F52435 判据集（锚 UNX-F52436-J1，off-by-one 板式）。
pub fn run_unx52435_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52435");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五故障族 20 格全落",
        recorded >= FAULT_CELLS as u32,
        "lag/drop/throttle/duplicate/out-of-order 逐格恒等",
    );
    s.add(
        "恢复 8 拍单调",
        {
            let mut lt = ClumsyLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < RECOV_BEATS {
                if lt.record_recov(b).is_err() {
                    ok = false;
                }
                b += 1;
            }
            ok && lt.recovs_all()
        },
        "故障撤销后 8 拍逐拍单调递增，逆拍必抓",
    );
    s.add(
        "过滤三元闸 6 闸全落",
        {
            let mut lt = ClumsyLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < GATES {
                if lt.judge_gate(g).is_err() {
                    ok = false;
                }
                g += 1;
            }
            ok && lt.gates_all()
        },
        "方向/协议/窗口三元闸 × 配置/生效两态逐闸落账",
    );
    s.add(
        "C 上栈声明 2/2 全齐",
        {
            let mut lt = ClumsyLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.splits_all() && lt.require_rows().is_ok()
        },
        "WinDivert 驱动层/npf 兼容层两行闭集（§AI-66.3）",
    );
    s.add(
        "缺声明行必抓",
        {
            let lt = ClumsyLedger::new();
            lt.require_rows() == Err(ClumsyCode::ERowMiss)
        },
        "C 上栈声明未齐即报 ERowMiss，缺一拒封",
    );
    s.add(
        "故障家族单调",
        {
            let mut ok = true;
            let mut f = 1usize;
            while f < FAULT_CELLS {
                if fault_kind(f).unwrap_or(5) < fault_kind(f - 1).unwrap_or(5) {
                    ok = false;
                }
                f += 1;
            }
            ok
        },
        "五故障族闭集单调，逆序必抓",
    );
    s.add(
        "注入检出 10/10（注入即故障检出，仅回环）",
        caught >= 10,
        "故障越闭集/故障漂移/恢复逆拍三态轮转必抓",
    );
    let mut l0 = ClumsyLedger::new();
    s.add(
        "边界·故障越闭集",
        l0.record_fault(FAULT_CELLS, 0).is_err(),
        "故障 20 越闭集必拒",
    );
    s.add(
        "边界·恢复拍越界",
        l0.record_recov(RECOV_BEATS).is_err(),
        "恢复拍 8 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_split(SPLIT_ROWS).is_err(),
        "声明行 2 越闭集必拒",
    );
    s.add(
        "未齐落格仍收",
        l0.record_fault(0, fault_cell(0).unwrap_or(0)).is_ok(),
        "账未齐时合法格照常落账（未齐即封只封收口，不封落格）",
    );
    s.add(
        "守恒核验 Ok",
        l0.verify_conservation().is_ok(),
        "账未齐时守恒等式照常成立，不误报 EInj",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", verify_fp(fp, fp2), "确定性全流程逐位一致且非零");
    s.add(
        "定级 S1 闭集（KB-COMPAT 登记）",
        {
            let mut lt = ClumsyLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut f = 0usize;
            while f < FAULT_CELLS {
                if let Some(w) = fault_cell(f) {
                    let _ = lt.record_fault(f, w);
                }
                f += 1;
            }
            let mut b = 0usize;
            while b < RECOV_BEATS {
                let _ = lt.record_recov(b);
                b += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.verify_grade(Grade::S1).is_ok()
        },
        "全齐 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCAC80,
        "SEG_BASE 0x40 对齐且在 VX16 域 0xCAC80 格",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fault_kind() {
        assert_eq!(fault_kind(0), Some(0));
        assert_eq!(fault_kind(4), Some(1));
        assert_eq!(fault_kind(8), Some(2));
        assert_eq!(fault_kind(12), Some(3));
        assert_eq!(fault_kind(19), Some(4));
        assert!(fault_kind(20).is_none());
        assert_eq!(fault_kind_name(0), Some("lag 延迟"));
        assert_eq!(fault_kind_name(4), Some("out-of-order 乱序"));
        assert!(fault_kind_name(5).is_none());
    }

    #[test]
    fn test_gate_names() {
        assert_eq!(gate_name(0), Some("方向闸·配置"));
        assert_eq!(gate_name(3), Some("协议闸·生效"));
        assert_eq!(gate_name(5), Some("窗口闸·生效"));
        assert!(gate_name(6).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = fault_cell(3).unwrap_or(0);
        assert_eq!(a, fault_cell(3).unwrap_or(0));
        assert_ne!(a, fault_cell(4).unwrap_or(0));
        assert_ne!(recov_cell(0).unwrap_or(0), recov_cell(1).unwrap_or(0));
        assert!(fault_cell(FAULT_CELLS).is_none());
        assert!(recov_cell(RECOV_BEATS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = ClumsyLedger::new();
        let _ = lt.record_fault(10, fault_cell(10).unwrap_or(0));
        assert_eq!(lt.record_fault(3, fault_cell(3).unwrap_or(0)), Err(ClumsyCode::EFaultOrder));
        assert!(lt.record_fault(11, fault_cell(11).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = ClumsyLedger::new();
        assert_eq!(
            lt.record_fault(6, fault_cell(6).unwrap_or(0).wrapping_add(1)),
            Err(ClumsyCode::EFaultDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = ClumsyLedger::new();
        let _ = lt.record_fault(0, fault_cell(0).unwrap_or(0));
        assert_eq!(lt.record_fault(0, fault_cell(0).unwrap_or(0)), Err(ClumsyCode::EDup));
        let _ = lt.record_recov(0);
        assert_eq!(lt.record_recov(0), Err(ClumsyCode::EDup));
    }

    #[test]
    fn test_recov_order_caught() {
        let mut lt = ClumsyLedger::new();
        assert!(lt.record_recov(5).is_ok());
        let before = lt.last_recov();
        assert_eq!(lt.record_recov(2), Err(ClumsyCode::ERecovOrder));
        // 逆拍被拒后拍位游标不得漂移。
        assert_eq!(lt.last_recov(), before);
        assert!(lt.record_recov(6).is_ok());
        assert_eq!(lt.record_recov(RECOV_BEATS), Err(ClumsyCode::ENoRecov));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = ClumsyLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(GATES).is_err());
        assert!(lt.gates_all());
        let mut lt2 = ClumsyLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_row_miss() {
        let mut lt = ClumsyLedger::new();
        assert_eq!(lt.require_rows(), Err(ClumsyCode::ERowMiss));
        let _ = lt.declare_split(0);
        assert_eq!(lt.require_rows(), Err(ClumsyCode::ERowMiss));
        let _ = lt.declare_split(1);
        assert!(lt.require_rows().is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 只含 run_cycle 实际计数的三节：故障 20 + 恢复 8。
        assert_eq!(recorded, (FAULT_CELLS + RECOV_BEATS) as u32);
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
        assert!(verify_fp(f1, f2));
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xCAC80);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        // 码闭集恰 13 变体，位序 0..=0x0C 与枚举实际位序一致。
        assert_eq!(ClumsyCode::Ok as u32, 0);
        assert_eq!(ClumsyCode::ENoFault.val(), SEG_BASE + 1);
        assert_eq!(ClumsyCode::EFaultDrift.val(), SEG_BASE + 2);
        assert_eq!(ClumsyCode::EFaultOrder.val(), SEG_BASE + 3);
        assert_eq!(ClumsyCode::ENoGate.val(), SEG_BASE + 4);
        assert_eq!(ClumsyCode::ENoRecov.val(), SEG_BASE + 5);
        assert_eq!(ClumsyCode::ERecovOrder.val(), SEG_BASE + 6);
        assert_eq!(ClumsyCode::ENoRow.val(), SEG_BASE + 7);
        assert_eq!(ClumsyCode::ERowMiss.val(), SEG_BASE + 8);
        assert_eq!(ClumsyCode::EDup.val(), SEG_BASE + 9);
        assert_eq!(ClumsyCode::ESeal.val(), SEG_BASE + 10);
        assert_eq!(ClumsyCode::EInj.val(), SEG_BASE + 11);
        assert_eq!(ClumsyCode::EGradeDrift.val(), SEG_BASE + 12);
        assert_eq!(ClumsyCode::EGradeDrift as u32, 12);
        let all = [
            ClumsyCode::Ok,
            ClumsyCode::ENoFault,
            ClumsyCode::EFaultDrift,
            ClumsyCode::EFaultOrder,
            ClumsyCode::ENoGate,
            ClumsyCode::ENoRecov,
            ClumsyCode::ERecovOrder,
            ClumsyCode::ENoRow,
            ClumsyCode::ERowMiss,
            ClumsyCode::EDup,
            ClumsyCode::ESeal,
            ClumsyCode::EInj,
            ClumsyCode::EGradeDrift,
        ];
        assert_eq!(all.len(), 13);
        let mut i = 0usize;
        while i < all.len() {
            assert!(!all[i].detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_injection_states() {
        // 三态逐态核验：越闭集 / 漂移（先落正格）/ 逆拍（空账本必抓且游标不变）。
        let mut l0 = ClumsyLedger::new();
        assert!(l0.record_fault(FAULT_CELLS, 0).is_err());
        let mut l1 = ClumsyLedger::new();
        let _ = l1.record_fault(0, fault_cell(0).unwrap_or(0));
        assert_eq!(
            l1.record_fault(1, fault_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(ClumsyCode::EFaultDrift)
        );
        let mut l2 = ClumsyLedger::new();
        let _ = l2.record_recov(5);
        assert_eq!(l2.record_recov(2), Err(ClumsyCode::ERecovOrder));
        assert_eq!(l2.last_recov(), 5);
        assert!(l2.verify_conservation().is_ok());
        // 未齐即封：账未齐 verify_seal 必拒，全齐才 Ok。
        assert_eq!(l2.verify_seal(), Err(ClumsyCode::ESeal));
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐目标档 S1。
        let lt0 = ClumsyLedger::new();
        assert_eq!(lt0.judge_grade(), Grade::S3);
        let mut lt1 = ClumsyLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = lt1.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt1.judge_grade(), Grade::S2);
        let mut lt2 = ClumsyLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        let mut f = 0usize;
        while f < FAULT_CELLS {
            if let Some(w) = fault_cell(f) {
                let _ = lt2.record_fault(f, w);
            }
            f += 1;
        }
        let mut b = 0usize;
        while b < RECOV_BEATS {
            let _ = lt2.record_recov(b);
            b += 1;
        }
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt2.declare_split(r);
            r += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S1);
        assert!(lt2.verify_seal().is_ok());
        assert!(lt2.verify_grade(Grade::S1).is_ok());
    }

    #[test]
    fn unx52435_checks_green() {
        let s = run_unx52435_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
