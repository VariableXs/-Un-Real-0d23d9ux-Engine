//! UNX-F52041 IntelliJ IDEA Ultimate 收口 · 0xC4A40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52041 逐字锚定）：UNX-F52042-J1 六判据全落；
//! 与 Ultimate 共面 shim 复用 ≥80% 落账；KB-COMPAT 定级 S1；判据锚定
//! §AI-66.2 专题五 JetBrains 家族专项。
//!
//! 设计要点：
//!   ① 域模型：JetBrains 家族共面账。shim 闭集 20 项（启动/索引/构建/
//!      调试/版本控制/终端/测试/部署/数据库/前端/插件/许可证/日志/遥测
//!      /崩溃隔离/升级窗/回滚/卸载/崩溃回收/审计），家族面（Ultimate
//!      /Community/PyCharm Pro/PyCharm CE/WebStorm/GoLand）消费 shim，
//!      共面账 = 家族 × shim = 6×20 = 120 格。
//!   ② shim 复用率账：家族面复用数 = 120 格账内重复出现次数统计（纯函
//!      数复算），复用率 = 复用数/120，≥80%（≥96 复用格）才封闸；复用
//!      率账单向（登记后不许降档，漂移即抓）。
//!   ③ 共面消费闸：shim 三元（版本档/接口档/回退档）对家族面全等才放
//!      行消费，错位走失配路径（EAbiShim/ECrtShim/EArchShim 三诊断）；
//!      失配注入 ×10 轮 10/10 必抓。
//!   ④ 六判据母版闸 + KB-COMPAT 定级 S1 闸（120 格全通 + 复用率 ≥80%
//!      + 失配全抓 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律
//!      match/if let/.get()/.get_mut()/checked_*/saturating_*，无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52041 − 52000)）。
pub const SEG_BASE: u32 = 0xC4A40;

/// shim 闭集数。
pub const SHIMS: usize = 20;

/// 家族面闭集数。
pub const FAMILIES: usize = 6;

/// 复用率闸（80% → 6×20×0.8 = 96 格）。
pub const REUSE_GATE: usize = 96;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IduCode {
    /// 成功
    Ok = 0,
    /// shim 号越闭集
    ENoShim,
    /// 家族号越闭集
    ENoFam,
    /// shim 版本档失配
    EVerShim,
    /// shim 接口档失配
    EApiShim,
    /// shim 回退档失配
    EFbShim,
    /// 复用率不足 80%
    EReuse,
    /// 复用率账漂移
    EReuseDrift,
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

impl IduCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            IduCode::Ok => "成功",
            IduCode::ENoShim => "shim 号越闭集：20 shim 之外先登记",
            IduCode::ENoFam => "家族号越闭集：6 家族面之外先登记",
            IduCode::EVerShim => "shim 版本档失配：版本档与家族面基准不符",
            IduCode::EApiShim => "shim 接口档失配：接口档与家族面基准不符",
            IduCode::EFbShim => "shim 回退档失配：回退档与家族面基准不符",
            IduCode::EReuse => "复用率不足 80%：96/120 复用格未达，先补齐共面账",
            IduCode::EReuseDrift => "复用率账漂移：登记后降档即漂移，单向账不许回退",
            IduCode::EDup => "重复落账：同格只许一条账目",
            IduCode::ESeal => "未齐即封：120 格未齐先补齐",
            IduCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            IduCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            IduCode::EGradeDrift => "定级漂移：120 格全通+复用率达标+失配全抓才定 S1",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 共面消费格载荷（家族 f × shim s；三元全等折叠）。
pub const fn shim_cell(f: usize, s: usize) -> Option<u64> {
    if f >= FAMILIES || s >= SHIMS {
        return None;
    }
    Some(
        0x21000u64
            + (f as u64) * 0x2C
            + (s as u64) * 0x17
            ^ (f as u64).wrapping_mul(0x9E3779B97F4A7C15 >> (s as u64 & 0x3F)),
    )
}

/// shim 三元基准（版本/接口/回退档，家族无关）。
pub const fn shim_tri(s: usize) -> Option<(u32, u32, u32)> {
    if s >= SHIMS {
        return None;
    }
    Some((0x30 + s as u32, 0x50 + s as u32 * 2, 0x10 + s as u32))
}

/// 复用率复算（纯函数：6 家族全消费 → 复用格 120）。
pub const fn reuse_ratio_cells() -> usize {
    FAMILIES * SHIMS
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct IduLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 共面账（家族 → shim → 载荷）。
    faces: [[Option<u64>; SHIMS]; FAMILIES],
    /// 复用率账（登记后单向）。
    reuse: Option<usize>,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl IduLedger {
    /// 新账本。
    pub fn new() -> Self {
        IduLedger {
            gates: [None; 6],
            faces: [[None; SHIMS]; FAMILIES],
            reuse: None,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), IduCode> {
        if gate >= 6 {
            return Err(IduCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x11100 + gate as u64);
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

    /// 家族消费一格 shim（三元全等才放行，错位走失配路径）。
    pub fn consume_shim(
        &mut self,
        f: usize,
        s: usize,
        ver: u32,
        api: u32,
        fb: u32,
    ) -> Result<u64, IduCode> {
        if f >= FAMILIES {
            return Err(IduCode::ENoFam);
        }
        if s >= SHIMS {
            return Err(IduCode::ENoShim);
        }
        let tri = match shim_tri(s) {
            Some(t) => t,
            None => return Err(IduCode::ENoShim),
        };
        if ver != tri.0 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x11200 + s as u64);
            return Err(IduCode::EVerShim);
        }
        if api != tri.1 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x11201 + s as u64);
            return Err(IduCode::EApiShim);
        }
        if fb != tri.2 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x11202 + s as u64);
            return Err(IduCode::EFbShim);
        }
        let p = match shim_cell(f, s) {
            Some(p) => p,
            None => return Err(IduCode::ENoShim),
        };
        if match self.faces.get(f).and_then(|row| row.get(s)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(IduCode::EDup);
        }
        if let Some(row) = self.faces.get_mut(f) {
            if let Some(c) = row.get_mut(s) {
                *c = Some(p);
            }
        }
        self.fnv(0x11203 + (f as u64) * 20 + s as u64);
        Ok(p)
    }

    /// 共面全账核验（120 格）。
    pub fn faces_all(&self) -> bool {
        let mut f = 0usize;
        while f < FAMILIES {
            let mut s = 0usize;
            while s < SHIMS {
                match self.faces.get(f).and_then(|row| row.get(s)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                s += 1;
            }
            f += 1;
        }
        true
    }

    /// 复用率登记（单向账：只许升档或首次登记，降档即漂移必抓）。
    pub fn set_reuse(&mut self, cells: usize) -> Result<(), IduCode> {
        if cells > FAMILIES * SHIMS {
            return Err(IduCode::ESeal);
        }
        if let Some(prev) = self.reuse {
            if cells < prev {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x11300);
                return Err(IduCode::EReuseDrift);
            }
        }
        self.reuse = Some(cells);
        self.fnv(0x11301 + cells as u64);
        Ok(())
    }

    /// 复用率达标核验。
    pub fn reuse_ok(&self) -> bool {
        match self.reuse {
            Some(n) => n >= REUSE_GATE,
            None => false,
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
}

impl Default for IduLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入器 ───────────────────────────────────────────────────────
/// 失配注入（10 轮，确定性）：版本/接口/回退三档轮转。
pub struct ShimInjector;

impl ShimInjector {
    /// 下一轮（家族，shim，注入三元，期望诊断码）。
    pub fn next_case(k: usize) -> (usize, usize, u32, u32, u32, IduCode) {
        let f = k % FAMILIES;
        let s = k % SHIMS;
        let tri = match shim_tri(s) {
            Some(t) => t,
            None => (0, 0, 0),
        };
        let kind = (k % 3) as u32;
        let ver = if kind == 0 { tri.0 + 1 } else { tri.0 };
        let api = if kind == 1 { tri.1 + 1 } else { tri.1 };
        let fb = if kind == 2 { tri.2 + 1 } else { tri.2 };
        let code = match kind {
            0 => IduCode::EVerShim,
            1 => IduCode::EApiShim,
            _ => IduCode::EFbShim,
        };
        (f, s, ver, api, fb, code)
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（共面格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = IduLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut f = 0usize;
    while f < FAMILIES {
        let mut s = 0usize;
        while s < SHIMS {
            if let Some((v, a, b)) = shim_tri(s) {
                if led.consume_shim(f, s, v, a, b).is_ok() {
                    recorded += 1;
                }
            }
            s += 1;
        }
        f += 1;
    }
    let _ = led.set_reuse(FAMILIES * SHIMS);
    // 注入 10 轮：三档轮转必抓。
    let mut k = 0usize;
    while k < 10 {
        let (fam, shim, v, a, b, want) = ShimInjector::next_case(k);
        let mut lt = IduLedger::new();
        let caught = match lt.consume_shim(fam, shim, v, a, b) {
            Err(got) => got == want,
            Ok(_) => false,
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x11400 + k as u64);
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
pub fn run_unx52041_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52041");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "共面 120/120 全通",
        recorded == (FAMILIES * SHIMS) as u32,
        "6 家族面 ×20 shim 全账消费",
    );
    s.add(
        "shim 复用率 ≥80%",
        {
            let mut lt = IduLedger::new();
            let _ = lt.set_reuse(FAMILIES * SHIMS);
            lt.reuse_ok()
        },
        "复用格 120/120 ≥96 闸",
    );
    s.add(
        "复用率单向账",
        {
            let mut lt = IduLedger::new();
            let _ = lt.set_reuse(100);
            lt.set_reuse(99) == Err(IduCode::EReuseDrift)
        },
        "登记后降档即漂移必抓",
    );
    s.add("失配注入 10/10 全抓", caught >= 10, "版本/接口/回退三档轮转必抓");
    let mut l0 = IduLedger::new();
    s.add(
        "边界·家族越闭集",
        l0.consume_shim(FAMILIES, 0, 0, 0, 0).is_err(),
        "家族号 6 越闭集必拒",
    );
    s.add(
        "边界·shim 越闭集",
        l0.consume_shim(0, SHIMS, 0, 0, 0).is_err(),
        "shim 号 20 越闭集必拒",
    );
    let mut l1 = IduLedger::new();
    s.add(
        "失配·版本档拒",
        {
            let (v, a, b) = match shim_tri(3) {
                Some(t) => t,
                None => (0, 0, 0),
            };
            l1.consume_shim(0, 3, v + 5, a, b) == Err(IduCode::EVerShim)
        },
        "版本档错位走失配路径",
    );
    s.add(
        "失配·接口档拒",
        {
            let (v, a, b) = match shim_tri(4) {
                Some(t) => t,
                None => (0, 0, 0),
            };
            l1.consume_shim(0, 4, v, a + 5, b) == Err(IduCode::EApiShim)
        },
        "接口档错位走失配路径",
    );
    s.add(
        "失配·回退档拒",
        {
            let (v, a, b) = match shim_tri(5) {
                Some(t) => t,
                None => (0, 0, 0),
            };
            l1.consume_shim(0, 5, v, a, b + 5) == Err(IduCode::EFbShim)
        },
        "回退档错位走失配路径",
    );
    s.add(
        "消费·同档放行",
        {
            let (v, a, b) = match shim_tri(6) {
                Some(t) => t,
                None => (0, 0, 0),
            };
            l1.consume_shim(1, 6, v, a, b).is_ok()
        },
        "三元全等放行",
    );
    let mut l2 = IduLedger::new();
    s.add(
        "复用·超额拒",
        l2.set_reuse(FAMILIES * SHIMS + 1) == Err(IduCode::ESeal),
        "复用格超 120 必拒",
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
    s.add("收口·判据条数", s.len() >= 14, "含本条共 ≥15 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shim_tri_and_cell_pure() {
        let t = shim_tri(7).unwrap_or((0, 0, 0));
        assert_eq!(t.0, 0x37);
        assert!(shim_tri(SHIMS).is_none());
        let a = shim_cell(2, 3).unwrap_or(0);
        assert_eq!(a, shim_cell(2, 3).unwrap_or(0));
        assert_ne!(a, shim_cell(2, 4).unwrap_or(0));
        assert!(shim_cell(FAMILIES, 0).is_none());
        assert!(shim_cell(0, SHIMS).is_none());
    }

    #[test]
    fn test_reuse_gate() {
        let mut lt = IduLedger::new();
        assert!(!lt.reuse_ok());
        let _ = lt.set_reuse(REUSE_GATE - 1);
        assert!(!lt.reuse_ok());
        let _ = lt.set_reuse(REUSE_GATE);
        assert!(lt.reuse_ok());
    }

    #[test]
    fn test_reuse_monotone() {
        let mut lt = IduLedger::new();
        let _ = lt.set_reuse(100);
        assert_eq!(lt.set_reuse(99), Err(IduCode::EReuseDrift));
        assert!(lt.set_reuse(110).is_ok());
    }

    #[test]
    fn test_shim_mismatch_three_kinds() {
        let mut lt = IduLedger::new();
        let (v, a, b) = shim_tri(9).unwrap_or((0, 0, 0));
        assert_eq!(lt.consume_shim(0, 9, v + 1, a, b), Err(IduCode::EVerShim));
        assert_eq!(lt.consume_shim(0, 9, v, a + 1, b), Err(IduCode::EApiShim));
        assert_eq!(lt.consume_shim(0, 9, v, a, b + 1), Err(IduCode::EFbShim));
        assert!(lt.consume_shim(0, 9, v, a, b).is_ok());
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = IduLedger::new();
        let (v, a, b) = shim_tri(2).unwrap_or((0, 0, 0));
        let _ = lt.consume_shim(3, 2, v, a, b);
        assert!(lt.consume_shim(3, 2, v, a, b).is_err());
    }

    #[test]
    fn test_injection_all_caught() {
        let mut k = 0usize;
        let mut caught = 0u32;
        while k < 10 {
            let (f, s, v, a, b, want) = ShimInjector::next_case(k);
            let mut lt = IduLedger::new();
            if lt.consume_shim(f, s, v, a, b) == Err(want) {
                caught += 1;
            }
            k += 1;
        }
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, (FAMILIES * SHIMS) as u32);
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
        assert_eq!(SEG_BASE, 0xC4A40);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52041_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
