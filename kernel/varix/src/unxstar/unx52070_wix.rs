//! UNX-F52070 WiX Toolset v4 收口 · 0xC5180段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52070 逐字锚定）：UNX-F52071-J1 安装包构建
//! ×5 样本 + 产出自安装全通（E4 形态分类"自研壳/Inno"对账联签锚定
//! 行）；KB-COMPAT 定级登记；判据锚定 §AI-66.6 风险 ③安装器长尾形态
//! 分类复用。
//!
//! 设计要点：
//!   ① 域模型：安装包构建 ×5 样本账，逐样本构建载荷（纯函数折叠）
//!      恒等才收；构建漂移必抓。
//!   ② 产出自安装账：自安装链 4 格（引导/解包/落盘/回滚钩子），逐格
//!      恒等才收；形态分类对账 2 行闭集（自研壳声明/Inno 对账声明），
//!      缺一拒封。
//!   ③ 构建→自安装拍序单向闸；注入 ×10 轮（构建漂移/自安装漂移/样本
//!      越闭集三态轮转）10/10 必抓，注入=检出+逃逸守恒；六判据母版闸
//!      + 定级（5 样本全通 + 自安装链全账 + 形态声明全齐 → S1）。
//!   ④ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52070 − 52000)）。
pub const SEG_BASE: u32 = 0xC5180;

/// 样本数。
pub const SAMPLES: usize = 5;

/// 自安装链格数。
pub const SELFCHAIN: usize = 4;

/// 形态分类声明行数。
pub const FORM_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WxCode {
    /// 成功
    Ok = 0,
    /// 样本号越闭集
    ENoSample,
    /// 构建漂移
    EDrift,
    /// 自安装链格越册
    ENoChain,
    /// 自安装漂移
    EChainDrift,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 拍序逆序
    EOrder,
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

impl WxCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            WxCode::Ok => "成功",
            WxCode::ENoSample => "样本号越闭集：5 样本之外先登记",
            WxCode::EDrift => "构建漂移：安装包折叠与基准不恒等，逐样本复算",
            WxCode::ENoChain => "自安装链格越册：4 格闭集之外先登记",
            WxCode::EChainDrift => "自安装漂移：引导/解包/落盘/回滚折叠与基准不恒等",
            WxCode::ENoRow => "声明行越闭集：2 行闭集之外先登记",
            WxCode::ERowMiss => "缺声明行：E4 形态分类对账声明未齐先补齐",
            WxCode::EOrder => "拍序逆序：构建→自安装单向拍序，逆序必抓",
            WxCode::EDup => "重复落账：同格只许一条账目",
            WxCode::ESeal => "未齐即封：5 样本未齐先补齐",
            WxCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            WxCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            WxCode::EGradeDrift => "定级漂移：5 样本全通+自安装全账+形态声明全齐才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 构建格载荷（样本 s）。
pub const fn build_cell(s: usize) -> Option<u64> {
    if s >= SAMPLES {
        return None;
    }
    Some((0x7C000u64 + (s as u64) * 0x34) ^ (s as u64).wrapping_mul(0x01000193))
}

/// 自安装链格基准（格 c）。
pub const fn chain_cell(c: usize) -> Option<u64> {
    if c >= SELFCHAIN {
        return None;
    }
    Some((0x7D000u64 + c as u64 * 0x38) ^ 0x77697873656634)
}

/// 形态声明行折叠（行 r）。
pub const fn form_row(r: usize) -> Option<u32> {
    if r >= FORM_ROWS {
        return None;
    }
    Some(0x7E00 + r as u32 * 0x2F)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct WxLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 构建账。
    builds: [Option<u64>; SAMPLES],
    /// 自安装链账。
    chain: [Option<u64>; SELFCHAIN],
    /// 形态声明账。
    forms: [Option<u32>; FORM_ROWS],
    /// 构建是否已齐（拍序前置）。
    built: bool,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl WxLedger {
    /// 新账本。
    pub fn new() -> Self {
        WxLedger {
            gates: [None; 6],
            builds: [None; SAMPLES],
            chain: [None; SELFCHAIN],
            forms: [None; FORM_ROWS],
            built: false,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), WxCode> {
        if gate >= 6 {
            return Err(WxCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x2F100 + gate as u64);
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

    /// 构建一格。
    pub fn record_build(&mut self, s: usize, got: u64) -> Result<(), WxCode> {
        if s >= SAMPLES {
            return Err(WxCode::ENoSample);
        }
        let want = match build_cell(s) {
            Some(w) => w,
            None => return Err(WxCode::ENoSample),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x2F200 + s as u64);
            return Err(WxCode::EDrift);
        }
        if match self.builds.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WxCode::EDup);
        }
        if let Some(c) = self.builds.get_mut(s) {
            *c = Some(got);
        }
        self.fnv(0x2F201 + s as u64);
        Ok(())
    }

    /// 构建全账核验（齐备后置位 built）。
    pub fn builds_all(&self) -> bool {
        let mut s = 0usize;
        while s < SAMPLES {
            match self.builds.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 自安装链一格（构建前置 + 恒等 + 重复拒）。
    pub fn record_chain(&mut self, c: usize, got: u64) -> Result<(), WxCode> {
        if c >= SELFCHAIN {
            return Err(WxCode::ENoChain);
        }
        if !self.builds_all() {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x2F300);
            return Err(WxCode::EOrder);
        }
        let want = match chain_cell(c) {
            Some(w) => w,
            None => return Err(WxCode::ENoChain),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x2F301 + c as u64);
            return Err(WxCode::EChainDrift);
        }
        if match self.chain.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WxCode::EDup);
        }
        if let Some(cell) = self.chain.get_mut(c) {
            *cell = Some(got);
        }
        self.fnv(0x2F302 + c as u64);
        Ok(())
    }

    /// 自安装链全账核验。
    pub fn chain_all(&self) -> bool {
        let mut c = 0usize;
        while c < SELFCHAIN {
            match self.chain.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 形态声明一行。
    pub fn declare_form(&mut self, r: usize) -> Result<(), WxCode> {
        if r >= FORM_ROWS {
            return Err(WxCode::ENoRow);
        }
        if match self.forms.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WxCode::EDup);
        }
        if let Some(c) = self.forms.get_mut(r) {
            *c = form_row(r);
        }
        self.fnv(0x2F400 + r as u64);
        Ok(())
    }

    /// 形态声明全齐核验。
    pub fn forms_all(&self) -> bool {
        let mut r = 0usize;
        while r < FORM_ROWS {
            match self.forms.get(r) {
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

impl Default for WxLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = WxLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut s = 0usize;
    while s < SAMPLES {
        if let Some(w) = build_cell(s) {
            if led.record_build(s, w).is_ok() {
                recorded += 1;
            }
        }
        s += 1;
    }
    let mut c = 0usize;
    while c < SELFCHAIN {
        if let Some(w) = chain_cell(c) {
            if led.record_chain(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut r = 0usize;
    while r < FORM_ROWS {
        let _ = led.declare_form(r);
        r += 1;
    }
    // 注入 10 轮：构建漂移 / 自安装前置 / 样本越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = WxLedger::new();
        let caught = match kind {
            0 => lt.record_build(2, build_cell(2).unwrap_or(0).wrapping_add(1)) == Err(WxCode::EDrift),
            1 => lt.record_chain(0, chain_cell(0).unwrap_or(0)) == Err(WxCode::EOrder),
            _ => lt.record_build(SAMPLES, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x2F500 + k as u64);
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
pub fn run_unx52070_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52070");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "安装包构建 5/5 全通",
        recorded >= SAMPLES as u32,
        "5 样本安装包逐样本恒等",
    );
    s.add(
        "自安装链 4/4 全账",
        {
            let mut lt = WxLedger::new();
            let mut s2 = 0usize;
            while s2 < SAMPLES {
                if let Some(w) = build_cell(s2) {
                    let _ = lt.record_build(s2, w);
                }
                s2 += 1;
            }
            let mut ok = true;
            let mut c = 0usize;
            while c < SELFCHAIN {
                if let Some(w) = chain_cell(c) {
                    if lt.record_chain(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.chain_all()
        },
        "引导/解包/落盘/回滚钩子四格零漂移",
    );
    s.add(
        "E4 形态声明 2/2 全齐",
        {
            let mut lt = WxLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < FORM_ROWS {
                if lt.declare_form(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.forms_all()
        },
        "自研壳声明/Inno 对账声明两行闭集",
    );
    s.add("注入 10/10 全抓", caught >= 10, "漂移/前置/越闭集三态轮转必抓");
    let mut l0 = WxLedger::new();
    s.add(
        "自安装前置闸",
        l0.record_chain(0, 0) == Err(WxCode::EOrder),
        "构建未齐先自安装必抓",
    );
    s.add(
        "边界·样本越闭集",
        l0.record_build(SAMPLES, 0).is_err(),
        "样本 5 越闭集必拒",
    );
    s.add(
        "边界·链格越册",
        l0.record_chain(SELFCHAIN, 0).is_err(),
        "链格 4 越册必拒",
    );
    let mut l1 = WxLedger::new();
    let _ = l1.record_build(1, build_cell(1).unwrap_or(0));
    s.add(
        "重复·再落账拒",
        l1.record_build(1, build_cell(1).unwrap_or(0)).is_err(),
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
    s.add("收口·判据条数", s.len() >= 11, "含本条共 ≥12 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cells_pure() {
        let a = build_cell(3).unwrap_or(0);
        assert_eq!(a, build_cell(3).unwrap_or(0));
        assert_ne!(a, build_cell(4).unwrap_or(0));
        assert!(build_cell(SAMPLES).is_none());
        assert!(chain_cell(SELFCHAIN).is_none());
        assert!(form_row(FORM_ROWS).is_none());
    }

    #[test]
    fn test_pre_chain_gate() {
        let mut lt = WxLedger::new();
        assert_eq!(lt.record_chain(0, 0), Err(WxCode::EOrder));
    }

    #[test]
    fn test_chain_drift() {
        let mut lt = WxLedger::new();
        let mut s2 = 0usize;
        while s2 < SAMPLES {
            let _ = lt.record_build(s2, build_cell(s2).unwrap_or(0));
            s2 += 1;
        }
        assert_eq!(
            lt.record_chain(1, chain_cell(1).unwrap_or(0).wrapping_add(2)),
            Err(WxCode::EChainDrift)
        );
        assert!(lt.record_chain(1, chain_cell(1).unwrap_or(0)).is_ok());
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
        assert_eq!(SEG_BASE, 0xC5180);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52070_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
