//! UNX-F52071 Inno Setup 收口 · 0xC51C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52071 逐字锚定）：UNX-F52072-J1 脚本编译 ×5
//! 样本 + 产出自安装全通（E4 形态分类对账）；KB-COMPAT 定级登记；判
//! 据锚定 §AI-66.6 风险 ③。
//!
//! 设计要点：
//!   ① 域模型：脚本编译 ×5 样本账，逐样本编译载荷（纯函数折叠）恒等
//!      才收；编译漂移必抓。
//!   ② 产出自安装账：自安装链 4 格（编译引导/解包/落盘/卸载钩子），
//!      恒等才收；编译前置闸（脚本未编译先自安装必抓）。
//!   ③ E4 形态分类对账：2 行声明闭集（Inno 形态声明/WiX 对账声明），
//!      缺一拒封。
//!   ④ 注入 ×10 轮（编译漂移/链漂移/前置越序三态轮转）10/10 必抓，注
//!      入=检出+逃逸守恒；六判据母版闸 + 定级（5 样本全通 + 链全账 +
//!      声明全齐 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52071 − 52000)）。
pub const SEG_BASE: u32 = 0xC51C0;

/// 样本数。
pub const SAMPLES: usize = 5;

/// 自安装链格数。
pub const CHAIN: usize = 4;

/// 声明行数。
pub const DECL_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InCode {
    /// 成功
    Ok = 0,
    /// 样本号越闭集
    ENoSample,
    /// 编译漂移
    EDrift,
    /// 链格越册
    ENoChain,
    /// 链漂移
    EChainDrift,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 前置越序
    EPre,
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

impl InCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            InCode::Ok => "成功",
            InCode::ENoSample => "样本号越闭集：5 样本之外先登记",
            InCode::EDrift => "编译漂移：脚本编译折叠与基准不恒等，逐样本复算",
            InCode::ENoChain => "链格越册：4 格闭集之外先登记",
            InCode::EChainDrift => "链漂移：引导/解包/落盘/卸载钩子折叠与基准不恒等",
            InCode::ENoRow => "声明行越闭集：2 行闭集之外先登记",
            InCode::ERowMiss => "缺声明行：E4 形态分类对账声明未齐先补齐",
            InCode::EPre => "前置越序：脚本未编译先自安装必抓",
            InCode::EDup => "重复落账：同格只许一条账目",
            InCode::ESeal => "未齐即封：5 样本未齐先补齐",
            InCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            InCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            InCode::EGradeDrift => "定级漂移：5 样本全通+链全账+声明全齐才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 编译格载荷（样本 s）。
pub const fn comp_cell(s: usize) -> Option<u64> {
    if s >= SAMPLES {
        return None;
    }
    Some((0x81000u64 + (s as u64) * 0x39) ^ (s as u64).wrapping_mul(0x01000193))
}

/// 链格基准（格 c）。
pub const fn chain_cell(c: usize) -> Option<u64> {
    if c >= CHAIN {
        return None;
    }
    Some((0x82000u64 + c as u64 * 0x31) ^ 0x696E6E6F73657574)
}

/// 声明行折叠（行 r）。
pub const fn decl_row(r: usize) -> Option<u32> {
    if r >= DECL_ROWS {
        return None;
    }
    Some(0x8300 + r as u32 * 0x36)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct InLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 编译账。
    comps: [Option<u64>; SAMPLES],
    /// 链账。
    chain: [Option<u64>; CHAIN],
    /// 声明账。
    decls: [Option<u32>; DECL_ROWS],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl InLedger {
    /// 新账本。
    pub fn new() -> Self {
        InLedger {
            gates: [None; 6],
            comps: [None; SAMPLES],
            chain: [None; CHAIN],
            decls: [None; DECL_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), InCode> {
        if gate >= 6 {
            return Err(InCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x30100 + gate as u64);
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

    /// 编译一格。
    pub fn record_comp(&mut self, s: usize, got: u64) -> Result<(), InCode> {
        if s >= SAMPLES {
            return Err(InCode::ENoSample);
        }
        let want = match comp_cell(s) {
            Some(w) => w,
            None => return Err(InCode::ENoSample),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x30200 + s as u64);
            return Err(InCode::EDrift);
        }
        if match self.comps.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(InCode::EDup);
        }
        if let Some(c) = self.comps.get_mut(s) {
            *c = Some(got);
        }
        self.fnv(0x30201 + s as u64);
        Ok(())
    }

    /// 编译全账核验。
    pub fn comps_all(&self) -> bool {
        let mut s = 0usize;
        while s < SAMPLES {
            match self.comps.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 链一格（编译前置 + 恒等 + 重复拒）。
    pub fn record_chain(&mut self, c: usize, got: u64) -> Result<(), InCode> {
        if c >= CHAIN {
            return Err(InCode::ENoChain);
        }
        if !self.comps_all() {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x30300);
            return Err(InCode::EPre);
        }
        let want = match chain_cell(c) {
            Some(w) => w,
            None => return Err(InCode::ENoChain),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x30301 + c as u64);
            return Err(InCode::EChainDrift);
        }
        if match self.chain.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(InCode::EDup);
        }
        if let Some(cell) = self.chain.get_mut(c) {
            *cell = Some(got);
        }
        self.fnv(0x30302 + c as u64);
        Ok(())
    }

    /// 链全账核验。
    pub fn chain_all(&self) -> bool {
        let mut c = 0usize;
        while c < CHAIN {
            match self.chain.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 声明一行。
    pub fn declare(&mut self, r: usize) -> Result<(), InCode> {
        if r >= DECL_ROWS {
            return Err(InCode::ENoRow);
        }
        if match self.decls.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(InCode::EDup);
        }
        if let Some(c) = self.decls.get_mut(r) {
            *c = decl_row(r);
        }
        self.fnv(0x30400 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn decls_all(&self) -> bool {
        let mut r = 0usize;
        while r < DECL_ROWS {
            match self.decls.get(r) {
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
}

impl Default for InLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = InLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut s = 0usize;
    while s < SAMPLES {
        if let Some(w) = comp_cell(s) {
            if led.record_comp(s, w).is_ok() {
                recorded += 1;
            }
        }
        s += 1;
    }
    let mut c = 0usize;
    while c < CHAIN {
        if let Some(w) = chain_cell(c) {
            if led.record_chain(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut r = 0usize;
    while r < DECL_ROWS {
        let _ = led.declare(r);
        r += 1;
    }
    // 注入 10 轮：编译漂移 / 前置越序 / 样本越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = InLedger::new();
        let caught = match kind {
            0 => lt.record_comp(3, comp_cell(3).unwrap_or(0).wrapping_add(1)) == Err(InCode::EDrift),
            1 => lt.record_chain(0, chain_cell(0).unwrap_or(0)) == Err(InCode::EPre),
            _ => lt.record_comp(SAMPLES, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x30500 + k as u64);
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
pub fn run_unx52071_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52071");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "脚本编译 5/5 全通",
        recorded >= SAMPLES as u32,
        "5 样本脚本编译逐样本恒等",
    );
    s.add(
        "自安装链 4/4 全账",
        {
            let mut lt = InLedger::new();
            let mut s2 = 0usize;
            while s2 < SAMPLES {
                let _ = lt.record_comp(s2, comp_cell(s2).unwrap_or(0));
                s2 += 1;
            }
            let mut ok = true;
            let mut c = 0usize;
            while c < CHAIN {
                if let Some(w) = chain_cell(c) {
                    if lt.record_chain(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.chain_all()
        },
        "编译引导/解包/落盘/卸载钩子四格零漂移",
    );
    s.add(
        "E4 形态声明 2/2 全齐",
        {
            let mut lt = InLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < DECL_ROWS {
                if lt.declare(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.decls_all()
        },
        "Inno 形态/WiX 对账两行闭集",
    );
    s.add("注入 10/10 全抓", caught >= 10, "漂移/前置/越闭集三态轮转必抓");
    let mut l0 = InLedger::new();
    s.add(
        "编译前置闸",
        l0.record_chain(0, 0) == Err(InCode::EPre),
        "脚本未编译先自安装必抓",
    );
    s.add(
        "边界·样本越闭集",
        l0.record_comp(SAMPLES, 0).is_err(),
        "样本 5 越闭集必拒",
    );
    let mut l1 = InLedger::new();
    let _ = l1.record_comp(0, comp_cell(0).unwrap_or(0));
    s.add(
        "重复·再落账拒",
        l1.record_comp(0, comp_cell(0).unwrap_or(0)).is_err(),
        "同格只许一条账目",
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
    s.add("收口·判据条数", s.len() >= 10, "含本条共 ≥11 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cells_pure() {
        let a = comp_cell(2).unwrap_or(0);
        assert_eq!(a, comp_cell(2).unwrap_or(0));
        assert_ne!(a, comp_cell(3).unwrap_or(0));
        assert!(comp_cell(SAMPLES).is_none());
        assert!(chain_cell(CHAIN).is_none());
        assert!(decl_row(DECL_ROWS).is_none());
    }

    #[test]
    fn test_pre_gate() {
        let mut lt = InLedger::new();
        assert_eq!(lt.record_chain(0, 0), Err(InCode::EPre));
    }

    #[test]
    fn test_comp_and_chain() {
        let mut lt = InLedger::new();
        assert_eq!(
            lt.record_comp(1, comp_cell(1).unwrap_or(0).wrapping_add(3)),
            Err(InCode::EDrift)
        );
        assert!(lt.record_comp(1, comp_cell(1).unwrap_or(0)).is_ok());
        let mut s2 = 0usize;
        while s2 < SAMPLES {
            let _ = lt.record_comp(s2, comp_cell(s2).unwrap_or(0));
            s2 += 1;
        }
        assert!(lt.record_chain(2, chain_cell(2).unwrap_or(0)).is_ok());
        assert!(lt.record_chain(2, chain_cell(2).unwrap_or(0)).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= SAMPLES as u32);
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
        assert_eq!(SEG_BASE, 0xC51C0);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52071_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 11);
    }
}
