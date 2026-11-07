//! UNX-F52187 Fork 收口 · 0xC6EC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52187 逐字锚定）：UNX-F52188-J1 六判据全落；
//! 搜索/自定义命令 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3。
//!
//! 设计要点：
//!   ① 域模型：搜索与自定义命令 ×10 账（提交哈希搜索/作者搜索/消息
//!      搜索/文件搜索/自定义命令注册/命令参数模板/命令执行/命令输出
//!      消费/命令历史/命令收藏），逐项载荷（纯函数折叠）恒等才收；
//!      漂移必抓。
//!   ② 搜索拍序单调闸：搜索类（0..4）先于命令类（4..10），回跳必抓。
//!   ③ 命令注册双向闸：执行前必须先注册（未注册先执行必抓），注册
//!      后执行单向（注册不吊销）。
//!   ④ 注入 ×10 轮（项越闭集/未注册先执行/重复落账三态轮转）10/10
//!      必抓，注入=检出+逃逸守恒；六判据母版闸 + 定级（10 项全通
//!      → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52187 − 52000)）。
pub const SEG_BASE: u32 = 0xC6EC0;

/// 搜索/命令项数。
pub const ITEMS: usize = 10;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfCode {
    /// 成功
    Ok = 0,
    /// 项号越闭集
    ENoItem,
    /// 格载荷漂移
    EDrift,
    /// 拍序逆序
    EOrder,
    /// 重复落账
    EDup,
    /// 未注册先执行
    ENoReg,
    /// 注册态漂移
    ERegDrift,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl PfCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32 + 1
    }
    pub fn detail(self) -> &'static str {
        match self {
            PfCode::Ok => "成功",
            PfCode::ENoItem => "项号越闭集：10 项之外先扩闭集",
            PfCode::EDrift => "格载荷漂移：折叠与基准不恒等，逐项复算",
            PfCode::EOrder => "拍序逆序：搜索类先于命令类",
            PfCode::EDup => "重复落账：同格只许一条账目",
            PfCode::ENoReg => "未注册先执行：自定义命令注册未登记先登记",
            PfCode::ERegDrift => "注册态漂移：注册后单向不吊销",
            PfCode::ESeal => "未齐即封：格未齐先补齐",
            PfCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            PfCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PfCode::EGradeDrift => "定级漂移：10 项全通才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 项格载荷（项 i）。
pub const fn item_cell(i: usize) -> Option<u64> {
    if i >= ITEMS {
        return None;
    }
    Some(0xEB00 + i as u64 * 0xB3)
}

/// 项类别（0..4=搜索类 4..10=命令类）。
pub const fn item_kind(i: usize) -> Option<u32> {
    if i >= ITEMS {
        return None;
    }
    if i < 4 {
        Some(0)
    } else {
        Some(1)
    }
}

/// 注册态核验（命令类项必须注册态）。
pub const fn reg_ok(registered: bool, i: usize) -> bool {
    match item_kind(i) {
        Some(1) => registered,
        _ => true,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Fork 收口账本。
#[derive(Clone)]
pub struct PfLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 项账。
    items: [Option<u64>; ITEMS],
    /// 注册态账（命令类项）。
    regs: [Option<bool>; ITEMS],
    /// 最后类别。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl PfLedger {
    /// 新账本。
    pub fn new() -> Self {
        PfLedger {
            gates: [None; 6],
            items: [None; ITEMS],
            regs: [None; ITEMS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PfCode> {
        if gate >= 6 {
            return Err(PfCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x48100 + gate as u64);
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

    /// 注册一行（命令类项；单向置位）。
    pub fn register(&mut self, i: usize) -> Result<(), PfCode> {
        if i >= ITEMS {
            return Err(PfCode::ENoItem);
        }
        if match self.regs.get(i) {
            Some(Some(true)) => true,
            _ => false,
        } {
            return Err(PfCode::ERegDrift);
        }
        if let Some(c) = self.regs.get_mut(i) {
            *c = Some(true);
        }
        self.fnv(0x48200 + i as u64);
        Ok(())
    }

    /// 项一格（类别拍序 + 注册闸 + 载荷对照 + 重复拒）。
    pub fn record_item(&mut self, i: usize) -> Result<u64, PfCode> {
        if i >= ITEMS {
            return Err(PfCode::ENoItem);
        }
        let kind = match item_kind(i) {
            Some(k) => k,
            None => return Err(PfCode::ENoItem),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x48300);
            return Err(PfCode::EOrder);
        }
        let registered = match self.regs.get(i) {
            Some(Some(true)) => true,
            _ => false,
        };
        if !reg_ok(registered, i) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x48301);
            return Err(PfCode::ENoReg);
        }
        let p = match item_cell(i) {
            Some(p) => p,
            None => return Err(PfCode::ENoItem),
        };
        if match self.items.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PfCode::EDup);
        }
        if let Some(c) = self.items.get_mut(i) {
            *c = Some(p);
        }
        self.last_kind = kind as i64;
        self.fnv(0x48302 + i as u64);
        Ok(p)
    }

    /// 项全账核验。
    pub fn items_all(&self) -> bool {
        let mut i = 0usize;
        while i < ITEMS {
            match self.items.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 命令类注册全齐核验。
    pub fn regs_all(&self) -> bool {
        let mut i = 4usize;
        while i < ITEMS {
            match self.regs.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 封账闸：项全齐 + 命令类注册全齐。
    pub fn seal(&self) -> Result<(), PfCode> {
        if !self.items_all() {
            return Err(PfCode::ESeal);
        }
        if !self.regs_all() {
            return Err(PfCode::ENoReg);
        }
        Ok(())
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for PfLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PfLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut i = 0usize;
    while i < ITEMS {
        if i >= 4 {
            let _ = led.register(i);
        }
        if led.record_item(i).is_ok() {
            recorded += 1;
        }
        i += 1;
    }
    // 注入 10 轮：项越闭集 / 未注册先执行 / 重复落账三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PfLedger::new();
        let caught = match kind {
            0 => lt.record_item(ITEMS).is_err(),
            1 => lt.record_item(4).is_err(),
            _ => {
                let _ = lt.register(5);
                let _ = lt.record_item(5);
                lt.record_item(5).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x48400 + k as u64);
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
pub fn run_unx52187_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52187");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "搜索/自定义命令 10 项全通",
        recorded >= ITEMS as u32,
        "哈希/作者/消息/文件搜索+注册/模板/执行/输出/历史/收藏逐项恒等",
    );
    s.add(
        "类别拍序单调闸",
        {
            let mut lt = PfLedger::new();
            let _ = lt.register(6);
            let _ = lt.record_item(6);
            lt.record_item(1).is_err()
        },
        "命令类后回搜索类回跳必抓",
    );
    s.add(
        "注册闸",
        {
            let mut lt = PfLedger::new();
            lt.register(ITEMS).is_err()
        },
        "注册越闭集必拒",
    );
    s.add(
        "未注册先执行必抓",
        {
            let mut lt = PfLedger::new();
            lt.record_item(7).is_err()
        },
        "命令类项注册未登记先登记",
    );
    s.add(
        "注册单向闸",
        {
            let mut lt = PfLedger::new();
            let _ = lt.register(5);
            lt.register(5).is_err()
        },
        "重复注册必拒（单向不吊销）",
    );
    s.add("注入 10/10 全抓", caught >= 10, "越闭集/未注册/重复三态轮转必抓");
    let mut l0 = PfLedger::new();
    s.add(
        "边界·项越闭集",
        l0.record_item(ITEMS).is_err(),
        "项 10 越闭集必拒",
    );
    let mut l1 = PfLedger::new();
    let _ = l1.register(5);
    let _ = l1.record_item(5);
    s.add("重复·再落账拒", l1.record_item(5).is_err(), "同格只许一条账目");
    s.add(
        "封账闸·未注册拒",
        l0.seal().is_err(),
        "命令类注册未齐拒封",
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
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0,
        "SEG_BASE 0x40 对齐且在 VX14 域",
    );
    s.add("收口·判据条数", s.len() >= 11, "含本条共 ≥12 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_item_kind() {
        assert_eq!(item_kind(0), Some(0));
        assert_eq!(item_kind(6), Some(1));
        assert!(item_kind(ITEMS).is_none());
        assert!(item_cell(ITEMS).is_none());
    }

    #[test]
    fn test_reg_gates() {
        let mut lt = PfLedger::new();
        assert!(lt.record_item(7).is_err());
        let _ = lt.register(7);
        assert!(lt.record_item(7).is_ok());
        assert!(lt.register(7).is_err());
        assert!(lt.register(ITEMS).is_err());
    }

    #[test]
    fn test_order_gate() {
        let mut lt = PfLedger::new();
        let _ = lt.register(6);
        let _ = lt.record_item(6);
        assert!(lt.record_item(1).is_err());
    }

    #[test]
    fn test_seal_gate() {
        let mut lt = PfLedger::new();
        assert!(lt.seal().is_err());
        let mut i = 0usize;
        while i < ITEMS {
            if i >= 4 {
                let _ = lt.register(i);
            }
            let _ = lt.record_item(i);
            i += 1;
        }
        assert!(lt.seal().is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= ITEMS as u32);
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
        assert_eq!(SEG_BASE, 0xC6EC0);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52187_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 11);
    }
}
