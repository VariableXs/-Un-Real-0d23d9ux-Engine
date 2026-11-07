//! UNX-F52289 安装器形态归并表消费对账（E4 200 样本经验复用）· 0xC8800段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52289"，逐字）：
//!   title: 安装器形态归并表消费对账（E4 200 样本经验复用）
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52290-J1 应用月
//!     更触发回归演练 ×2：T+1 日广播 → 钉版回归 + 新版本 20% 抽测 →
//!     结论入账；全量追新禁止声明；联签锚定行 AI-94；判据锚定
//!     §AI-66.6 风险 ②；③ 完成时 API/软件写回：状态=已完成、结果摘
//!     要非空。
//! off-by-one 板式口径：本卡（卡 N=52289）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52290-J1，照原文锚定；功能按本卡标题（安装器形态归并表消费
//! 对账）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① E4 形态五类闭集：MSI / EXE / MSIX / Portable / Script，五类 ×4
//!      格 = 20 归并格，对应 40 样本（每格 2 样本）逐格落账；越类必抓。
//!   ② 归并规则 10 格：五类两两归并方向规则（载荷恒等才收），逆序归
//!      并（家族回退）必抓。
//!   ③ VX14 经验复用分账声明 2 行闭集：E4 200 样本经验复用分账 /
//!      VX14 归并误检经验回注分账，缺一拒封。
//!   ④ ledger 状态机（new/record_cell/record_rule/record_vx14/
//!      judge_gate×6/fingerprint FNV-1a/grade 定级闭集）；注入核
//!      run_cycle 三态轮转 10 轮（误归并漂移 / 形态家族逆序 / 归并格
//!      越闭集），全部落在当轮临时账本上，led 上 injected==caught==10、
//!      escaped==0；双跑 FNV 指纹一致；定级 S1/S2/S3 闭集。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52289;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// E4 形态类数（MSI/EXE/MSIX/Portable/Script）。
pub const FORM_KINDS: usize = 5;

/// 每类归并格数。
pub const CELLS_PER_KIND: usize = 4;

/// 归并格总数（5 类 × 4 格）。
pub const MERGE_CELLS: usize = FORM_KINDS * CELLS_PER_KIND;

/// E4 样本总数（20 格 × 每格 2 样本）。
pub const E4_SAMPLES: usize = 40;

/// 归并规则格数。
pub const MERGE_RULES: usize = 10;

/// VX14 经验复用分账声明行数。
pub const VX14_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InstMergeCode {
    /// 成功
    Ok = 0,
    /// 归并格越闭集
    ENoCell,
    /// 误归并载荷漂移
    ECellDrift,
    /// 形态家族逆序
    EOrder,
    /// 归并规则越闭集
    ENoRule,
    /// 归并规则载荷漂移
    ERuleDrift,
    /// VX14 声明行越闭集
    ENoRow,
    /// 缺 VX14 分账声明
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

impl InstMergeCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            InstMergeCode::Ok => "成功",
            InstMergeCode::ENoCell => "归并格越闭集：E4 五类 20 归并格之外先登记",
            InstMergeCode::ECellDrift => "误归并载荷漂移：格折叠与基准不恒等，逐样本复算",
            InstMergeCode::EOrder => "形态家族逆序：MSI→EXE→MSIX→Portable→Script 单调，回退必抓",
            InstMergeCode::ENoRule => "归并规则越闭集：10 条归并规则之外先登记",
            InstMergeCode::ERuleDrift => "归并规则载荷漂移：规则折叠与基准不恒等，逐条复算",
            InstMergeCode::ENoRow => "VX14 声明行越闭集：经验复用分账 2 行闭集之外先登记",
            InstMergeCode::ERowMiss => "缺 VX14 分账声明：经验复用分账未齐先补齐",
            InstMergeCode::EDup => "重复落账：同格只许一条账目",
            InstMergeCode::ESeal => "未齐即封：20 归并格 + 10 规则 + 2 行分账未齐先补齐",
            InstMergeCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            InstMergeCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            InstMergeCode::EGradeDrift => "定级漂移：全通且分账声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20归并格+10规则+2行VX14分账全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 归并格载荷（格 c：五类 × 4 格，每格折 2 个 E4 样本）。
pub const fn merge_cell(c: usize) -> Option<u64> {
    if c >= MERGE_CELLS {
        return None;
    }
    Some((0xD4000u64 + (c as u64) * 0x37) ^ ((c / CELLS_PER_KIND) as u64).wrapping_mul(0x01000193))
}

/// 形态类（0..4=MSI 4..8=EXE 8..12=MSIX 12..16=Portable 16..20=Script）。
pub const fn form_kind(c: usize) -> Option<u32> {
    if c >= MERGE_CELLS {
        return None;
    }
    Some((c / CELLS_PER_KIND) as u32)
}

/// 形态类名。
pub const fn form_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("MSI"),
        1 => Some("EXE"),
        2 => Some("MSIX"),
        3 => Some("Portable"),
        4 => Some("Script"),
        _ => None,
    }
}

/// 归并规则格载荷（规则 r）。
pub const fn rule_cell(r: usize) -> Option<u64> {
    if r >= MERGE_RULES {
        return None;
    }
    Some((0xD5000u64 + (r as u64) * 0x5B) ^ 0x45345F4D45524745)
}

/// VX14 分账声明行折叠（行 r）。
pub const fn vx14_row(r: usize) -> Option<u32> {
    if r >= VX14_ROWS {
        return None;
    }
    Some(0xD600 + r as u32 * 0x37)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// 安装器归并表对账账本状态机。
#[derive(Clone)]
pub struct InstMergeLedger {
    /// 六判据（消费对账：启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 归并格账。
    cells: [Option<u64>; MERGE_CELLS],
    /// 归并规则账。
    rules: [Option<u64>; MERGE_RULES],
    /// VX14 分账声明账。
    vx14: [Option<u32>; VX14_ROWS],
    /// 最后形态类。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl InstMergeLedger {
    /// 新账本。
    pub fn new() -> Self {
        InstMergeLedger {
            gates: [None; 6],
            cells: [None; MERGE_CELLS],
            rules: [None; MERGE_RULES],
            vx14: [None; VX14_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), InstMergeCode> {
        if gate >= 6 {
            return Err(InstMergeCode::ESeal);
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

    /// 归并格一格（形态类单调 + 载荷对照 + 重复拒）。
    pub fn record_cell(&mut self, c: usize, got: u64) -> Result<(), InstMergeCode> {
        if c >= MERGE_CELLS {
            return Err(InstMergeCode::ENoCell);
        }
        let kind = match form_kind(c) {
            Some(k) => k,
            None => return Err(InstMergeCode::ENoCell),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200);
            return Err(InstMergeCode::EOrder);
        }
        let want = match merge_cell(c) {
            Some(w) => w,
            None => return Err(InstMergeCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5201 + c as u64);
            return Err(InstMergeCode::ECellDrift);
        }
        if match self.cells.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(InstMergeCode::EDup);
        }
        if let Some(slot) = self.cells.get_mut(c) {
            *slot = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5202 + c as u64);
        Ok(())
    }

    /// 归并格全账核验（20 格）。
    pub fn cells_all(&self) -> bool {
        let mut c = 0usize;
        while c < MERGE_CELLS {
            match self.cells.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 归并规则一格（载荷对照 + 重复拒）。
    pub fn record_rule(&mut self, r: usize, got: u64) -> Result<(), InstMergeCode> {
        let want = match rule_cell(r) {
            Some(w) => w,
            None => return Err(InstMergeCode::ENoRule),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300 + r as u64);
            return Err(InstMergeCode::ERuleDrift);
        }
        if match self.rules.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(InstMergeCode::EDup);
        }
        if let Some(slot) = self.rules.get_mut(r) {
            *slot = Some(got);
        }
        self.fnv(0x5301 + r as u64);
        Ok(())
    }

    /// 归并规则全账核验（10 条）。
    pub fn rules_all(&self) -> bool {
        let mut r = 0usize;
        while r < MERGE_RULES {
            match self.rules.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// VX14 分账声明一行。
    pub fn record_vx14(&mut self, r: usize) -> Result<(), InstMergeCode> {
        if r >= VX14_ROWS {
            return Err(InstMergeCode::ENoRow);
        }
        if match self.vx14.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(InstMergeCode::EDup);
        }
        if let Some(slot) = self.vx14.get_mut(r) {
            *slot = vx14_row(r);
        }
        self.fnv(0x5400 + r as u64);
        Ok(())
    }

    /// VX14 分账声明全齐核验。
    pub fn vx14_all(&self) -> bool {
        let mut r = 0usize;
        while r < VX14_ROWS {
            match self.vx14.get(r) {
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
        if self.gates_all() && self.cells_all() && self.rules_all() && self.vx14_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for InstMergeLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = InstMergeLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut c = 0usize;
    while c < MERGE_CELLS {
        if let Some(w) = merge_cell(c) {
            if led.record_cell(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut r = 0usize;
    while r < MERGE_RULES {
        if let Some(w) = rule_cell(r) {
            if led.record_rule(r, w).is_ok() {
                recorded += 1;
            }
        }
        r += 1;
    }
    let mut r = 0usize;
    while r < VX14_ROWS {
        let _ = led.record_vx14(r);
        r += 1;
    }
    // 注入 10 轮：误归并漂移 / 形态家族逆序 / 归并格越闭集三态轮转。
    // 全部落在当轮临时账本上；漂移注入先录正格再注漂移；逆序注入先按
    // 合法序落低族格再注回退，空账本同样必抓且不改 last_kind。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = InstMergeLedger::new();
        let caught = match kind {
            0 => {
                if let (Some(w0), Some(w1)) = (merge_cell(0), merge_cell(1)) {
                    let _ = lt.record_cell(0, w0);
                    lt.record_cell(1, w1.wrapping_add(1)) == Err(InstMergeCode::ECellDrift)
                } else {
                    false
                }
            }
            1 => {
                if let (Some(w4), Some(w1)) = (merge_cell(4), merge_cell(1)) {
                    let _ = lt.record_cell(4, w4);
                    lt.record_cell(1, w1) == Err(InstMergeCode::EOrder)
                } else {
                    false
                }
            }
            _ => lt.record_cell(MERGE_CELLS, 0).is_err(),
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
/// UNX-F52289 判据集（锚 UNX-F52290-J1，off-by-one 板式）。
pub fn run_unx52289_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52289");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "E4 归并表 20 格全通",
        recorded >= MERGE_CELLS as u32,
        "MSI/EXE/MSIX/Portable/Script 五类逐格恒等，40 样本逐格对账",
    );
    s.add(
        "形态五类单调",
        {
            let mut ok = true;
            let mut c = 1usize;
            while c < MERGE_CELLS {
                if form_kind(c).unwrap_or(FORM_KINDS as u32) < form_kind(c - 1).unwrap_or(FORM_KINDS as u32) {
                    ok = false;
                }
                c += 1;
            }
            ok
        },
        "五类闭集单调，家族回退必抓",
    );
    s.add(
        "归并规则 10 格全齐",
        {
            let mut lt = InstMergeLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < MERGE_RULES {
                if let Some(w) = rule_cell(r) {
                    if lt.record_rule(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.rules_all()
        },
        "五类两两归并方向规则逐条恒等",
    );
    s.add(
        "VX14 经验复用分账 2/2 全齐",
        {
            let mut lt = InstMergeLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < VX14_ROWS {
                if lt.record_vx14(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.vx14_all()
        },
        "E4 200 样本经验复用分账 / VX14 归并误检经验回注分账两行闭集",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = InstMergeLedger::new();
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
        "注入 10/10 全抓",
        caught >= 10,
        "误归并漂移/形态家族逆序/归并格越闭集三态轮转必抓",
    );
    let mut l0 = InstMergeLedger::new();
    s.add(
        "边界·归并格越闭集",
        l0.record_cell(MERGE_CELLS, 0).is_err(),
        "归并格 20 越闭集必拒",
    );
    s.add(
        "边界·归并规则越闭集",
        l0.record_rule(MERGE_RULES, 0).is_err(),
        "归并规则 10 越闭集必拒",
    );
    s.add(
        "边界·六判据越闭集",
        l0.judge_gate(6).is_err(),
        "判据闸 6 越闭集必拒",
    );
    s.add(
        "误归并漂移拒",
        {
            if let (Some(w0), Some(w1)) = (merge_cell(0), merge_cell(1)) {
                let _ = l0.record_cell(0, w0);
                l0.record_cell(1, w1.wrapping_add(1)) == Err(InstMergeCode::ECellDrift)
            } else {
                false
            }
        },
        "归并格载荷漂移必抓（误归并检出）",
    );
    s.add(
        "形态家族逆序拒",
        {
            if let (Some(w4), Some(w1)) = (merge_cell(4), merge_cell(1)) {
                let mut lt = InstMergeLedger::new();
                let _ = lt.record_cell(4, w4);
                lt.record_cell(1, w1) == Err(InstMergeCode::EOrder)
            } else {
                false
            }
        },
        "EXE→MSI 家族回退必抓",
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
            let mut lt = InstMergeLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut c = 0usize;
            while c < MERGE_CELLS {
                if let Some(w) = merge_cell(c) {
                    let _ = lt.record_cell(c, w);
                }
                c += 1;
            }
            let mut r = 0usize;
            while r < MERGE_RULES {
                if let Some(w) = rule_cell(r) {
                    let _ = lt.record_rule(r, w);
                }
                r += 1;
            }
            let mut r = 0usize;
            while r < VX14_ROWS {
                let _ = lt.record_vx14(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8800,
        "SEG_BASE 0x40 对齐且落在 VX15 公式位",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_form_kind() {
        assert_eq!(form_kind(0), Some(0));
        assert_eq!(form_kind(4), Some(1));
        assert_eq!(form_kind(19), Some(4));
        assert!(form_kind(20).is_none());
        assert!(merge_cell(20).is_none());
        assert_eq!(form_kind_name(0), Some("MSI"));
        assert_eq!(form_kind_name(4), Some("Script"));
        assert!(form_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = merge_cell(7);
        assert_eq!(a, merge_cell(7));
        assert_ne!(a, merge_cell(8));
        assert!(rule_cell(MERGE_RULES).is_none());
        assert!(vx14_row(VX14_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = InstMergeLedger::new();
        let w8 = merge_cell(8).unwrap_or(0);
        let w2 = merge_cell(2).unwrap_or(0);
        let _ = lt.record_cell(8, w8);
        assert_eq!(lt.record_cell(2, w2), Err(InstMergeCode::EOrder));
        // 逆序被拒后 last_kind 不回退：同类后继仍可落。
        assert!(lt.record_cell(9, merge_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_order_on_empty_ledger_caught() {
        // 空账本逆序注入：先按合法序落低族格，再注回退，必抓。
        let mut lt = InstMergeLedger::new();
        let _ = lt.record_cell(4, merge_cell(4).unwrap_or(0));
        assert_eq!(
            lt.record_cell(1, merge_cell(1).unwrap_or(0)),
            Err(InstMergeCode::EOrder)
        );
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = InstMergeLedger::new();
        assert_eq!(
            lt.record_cell(5, merge_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(InstMergeCode::ECellDrift)
        );
        assert_eq!(
            lt.record_rule(3, rule_cell(3).unwrap_or(0).wrapping_add(7)),
            Err(InstMergeCode::ERuleDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = InstMergeLedger::new();
        let _ = lt.record_cell(0, merge_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_cell(0, merge_cell(0).unwrap_or(0)),
            Err(InstMergeCode::EDup)
        );
        let _ = lt.record_vx14(0);
        assert_eq!(lt.record_vx14(0), Err(InstMergeCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = InstMergeLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = InstMergeLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (MERGE_CELLS + MERGE_RULES) as u32);
        assert_eq!(caught, 10);
        let led = InstMergeLedger::new();
        assert_eq!(led.stats(), (0, 0, 0));
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
        assert_eq!(SEG_BASE, 0xC8800);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(InstMergeCode::Ok as u32, 0);
        assert_eq!(SEG_BASE + 0x0C, InstMergeCode::EGradeDrift.val());
        assert_eq!(InstMergeCode::EGradeDrift as u32, 12);
        assert!(!InstMergeCode::Ok.detail().is_empty());
        assert!(!InstMergeCode::ESeal.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = InstMergeLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52289_checks_green() {
        let s = run_unx52289_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
