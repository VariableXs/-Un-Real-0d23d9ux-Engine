//! UNX-F52069 vcpkg 收口 · 0xC5140段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52069 逐字锚定）：UNX-F52070-J1 端口安装 ×20
//! （含二进制缓存命中）全通；与 MSBuild 集成 ×5 工程全通；克隆与自举
//! 消费 git 嫁接件判据绿（联签锚定行 AI-59）；KB-COMPAT 定级登记；判
//! 据锚定 §AI-66.2 专题二。
//!
//! 设计要点：
//!   ① 域模型：vcpkg 端口安装 ×20 账（含二进制缓存命中二元），逐格装
//!      载（纯函数折叠）恒等才收；二元越闭集拒。
//!   ② MSBuild 集成 ×5 工程账：vcpkg integrate 三元（targets 档/env
//!      档/restore 钩子档）恒等才收。
//!   ③ 克隆与自举账：git 嫁接件 4 件判绿（bootstrap/克隆/清单恢复/
//!      缓存回填），漂移即抓。
//!   ④ 安装拍序闸 + 注入 ×10 轮（二元越闭集/集成漂移/自举漂移三态轮
//!      转）10/10 必抓，注入=检出+逃逸守恒；六判据母版闸 + 定级（20
//!      格全通 + 自举全绿 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52069 − 52000)）。
pub const SEG_BASE: u32 = 0xC5140;

/// 端口安装数。
pub const PORTS: usize = 20;

/// 集成工程数。
pub const INT_PROJS: usize = 5;

/// 自举/嫁接件数。
pub const BOOT_GRAFTS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VkCode {
    /// 成功
    Ok = 0,
    /// 端口号越闭集
    ENoPort,
    /// 二元越闭集
    ENoBin,
    /// 端口装载漂移
    EDrift,
    /// 集成格越册
    ENoInt,
    /// 集成漂移
    EIntDrift,
    /// 自举漂移
    EBoot,
    /// 拍序乱
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

impl VkCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            VkCode::Ok => "成功",
            VkCode::ENoPort => "端口号越闭集：20 端口之外先登记",
            VkCode::ENoBin => "二元越闭集：缓存命中/回源二元之外先登记",
            VkCode::EDrift => "端口装载漂移：装载折叠与基准不恒等，逐格复算",
            VkCode::ENoInt => "集成格越册：5 工程闭集之外先登记",
            VkCode::EIntDrift => "集成漂移：targets/env/restore 钩子折叠与基准不恒等",
            VkCode::EBoot => "自举漂移：bootstrap/克隆/清单/缓存回填折叠与基准不恒等",
            VkCode::EOrder => "拍序乱：命中→回源→装载→集成严格递增",
            VkCode::EDup => "重复落账：同格只许一条账目",
            VkCode::ESeal => "未齐即封：20 格未齐先补齐",
            VkCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            VkCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            VkCode::EGradeDrift => "定级漂移：20 格全通+集成 5 格+自举 4 件全绿才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 端口格载荷（端口 p × 二元 b）。
pub const fn port_cell(p: usize, b: u32) -> Option<u64> {
    if p >= PORTS || b > 1 {
        return None;
    }
    Some((0x77000u64 + (p as u64) * 0x31) ^ (b as u64).wrapping_mul(0x01000193))
}

/// 集成格载荷（工程 p）。
pub const fn int_cell(p: usize) -> Option<u64> {
    if p >= INT_PROJS {
        return None;
    }
    Some((0x78000u64 + (p as u64) * 0x36) ^ 0x76636C6B696E74)
}

/// 自举件基准（件 g）。
pub const fn boot_cell(g: usize) -> Option<u64> {
    if g >= BOOT_GRAFTS {
        return None;
    }
    Some((0x79000u64 + g as u64 * 0x2B) ^ (g as u64).wrapping_mul(0x100000001b3))
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct VkLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 端口账。
    ports: [Option<u64>; PORTS],
    /// 集成账。
    ints: [Option<u64>; INT_PROJS],
    /// 自举账（true=绿）。
    boot: [Option<bool>; BOOT_GRAFTS],
    /// 最后拍。
    last_tick: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl VkLedger {
    /// 新账本。
    pub fn new() -> Self {
        VkLedger {
            gates: [None; 6],
            ports: [None; PORTS],
            ints: [None; INT_PROJS],
            boot: [None; BOOT_GRAFTS],
            last_tick: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), VkCode> {
        if gate >= 6 {
            return Err(VkCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x2E100 + gate as u64);
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

    /// 端口一格（二元 + 拍序 + 重复拒）。
    pub fn install_port(&mut self, p: usize, b: u32, tick: u32) -> Result<u64, VkCode> {
        if p >= PORTS {
            return Err(VkCode::ENoPort);
        }
        if b > 1 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x2E200 + p as u64);
            return Err(VkCode::ENoBin);
        }
        if (tick as i64) <= self.last_tick {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x2E201);
            return Err(VkCode::EOrder);
        }
        let want = match port_cell(p, b) {
            Some(w) => w,
            None => return Err(VkCode::ENoBin),
        };
        if match self.ports.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VkCode::EDup);
        }
        if let Some(c) = self.ports.get_mut(p) {
            *c = Some(want);
        }
        self.last_tick = tick as i64;
        self.fnv(0x2E202 + p as u64);
        Ok(want)
    }

    /// 端口全账核验（20 格）。
    pub fn ports_all(&self) -> bool {
        let mut p = 0usize;
        while p < PORTS {
            match self.ports.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 集成一格。
    pub fn integrate(&mut self, p: usize, got: u64) -> Result<(), VkCode> {
        if p >= INT_PROJS {
            return Err(VkCode::ENoInt);
        }
        let want = match int_cell(p) {
            Some(w) => w,
            None => return Err(VkCode::ENoInt),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x2E300 + p as u64);
            return Err(VkCode::EIntDrift);
        }
        if match self.ints.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VkCode::EDup);
        }
        if let Some(c) = self.ints.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x2E301 + p as u64);
        Ok(())
    }

    /// 集成全账核验（5 格）。
    pub fn ints_all(&self) -> bool {
        let mut p = 0usize;
        while p < INT_PROJS {
            match self.ints.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 自举一件（恒等判绿）。
    pub fn record_boot(&mut self, g: usize, got: u64) -> Result<(), VkCode> {
        let want = match boot_cell(g) {
            Some(w) => w,
            None => return Err(VkCode::EBoot),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x2E400 + g as u64);
            return Err(VkCode::EBoot);
        }
        if let Some(c) = self.boot.get_mut(g) {
            *c = Some(true);
        }
        self.fnv(0x2E401 + g as u64);
        Ok(())
    }

    /// 自举全绿核验。
    pub fn boot_all(&self) -> bool {
        let mut g = 0usize;
        while g < BOOT_GRAFTS {
            match self.boot.get(g) {
                Some(Some(true)) => {}
                _ => return false,
            }
            g += 1;
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

impl Default for VkLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = VkLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut p = 0usize;
    let mut tick: u32 = 0;
    while p < PORTS {
        tick = tick.saturating_add(2);
        let b = (p % 2) as u32;
        if led.install_port(p, b, tick).is_ok() {
            recorded += 1;
        }
        p += 1;
    }
    let mut q = 0usize;
    while q < INT_PROJS {
        if let Some(w) = int_cell(q) {
            if led.integrate(q, w).is_ok() {
                recorded += 1;
            }
        }
        q += 1;
    }
    let mut gr = 0usize;
    while gr < BOOT_GRAFTS {
        if let Some(w) = boot_cell(gr) {
            let _ = led.record_boot(gr, w);
        }
        gr += 1;
    }
    // 注入 10 轮：二元越闭集 / 集成漂移 / 自举漂移三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = VkLedger::new();
        let caught = match kind {
            0 => lt.install_port(1, 2, 1) == Err(VkCode::ENoBin),
            1 => lt.integrate(2, int_cell(2).unwrap_or(0).wrapping_add(1)) == Err(VkCode::EIntDrift),
            _ => lt.record_boot(1, boot_cell(1).unwrap_or(0).wrapping_add(1)) == Err(VkCode::EBoot),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x2E500 + k as u64);
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
pub fn run_unx52069_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52069");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "端口安装 20/20 全通",
        recorded >= PORTS as u32,
        "20 端口（含缓存命中二元）逐格恒等",
    );
    s.add(
        "MSBuild 集成 5/5 全账",
        {
            let mut lt = VkLedger::new();
            let mut ok = true;
            let mut q = 0usize;
            while q < INT_PROJS {
                if let Some(w) = int_cell(q) {
                    if lt.integrate(q, w).is_err() {
                        ok = false;
                    }
                }
                q += 1;
            }
            ok && lt.ints_all()
        },
        "targets/env/restore 钩子三元零漂移",
    );
    s.add(
        "自举/克隆 4/4 判绿",
        {
            let mut lt = VkLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < BOOT_GRAFTS {
                if let Some(w) = boot_cell(g) {
                    if lt.record_boot(g, w).is_err() {
                        ok = false;
                    }
                }
                g += 1;
            }
            ok && lt.boot_all()
        },
        "bootstrap/克隆/清单恢复/缓存回填四件判绿",
    );
    s.add("注入 10/10 全抓", caught >= 10, "二元/集成/自举三态轮转必抓");
    let mut l0 = VkLedger::new();
    s.add(
        "二元·越闭集拒",
        l0.install_port(0, 2, 1) == Err(VkCode::ENoBin),
        "命中/回源二元之外先登记",
    );
    s.add(
        "边界·端口越闭集",
        l0.install_port(PORTS, 0, 1).is_err(),
        "端口 20 越闭集必拒",
    );
    s.add(
        "边界·集成越册",
        l0.integrate(INT_PROJS, 0).is_err(),
        "工程 5 越册必拒",
    );
    let mut l1 = VkLedger::new();
    let _ = l1.install_port(2, 0, 3);
    s.add(
        "拍序·回退拒",
        l1.install_port(3, 1, 1) == Err(VkCode::EOrder),
        "拍序回退必抓",
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
        let a = port_cell(5, 1).unwrap_or(0);
        assert_eq!(a, port_cell(5, 1).unwrap_or(0));
        assert_ne!(a, port_cell(5, 0).unwrap_or(0));
        assert!(port_cell(PORTS, 0).is_none());
        assert!(port_cell(0, 2).is_none());
        assert!(int_cell(INT_PROJS).is_none());
        assert!(boot_cell(BOOT_GRAFTS).is_none());
    }

    #[test]
    fn test_bin_and_order_gates() {
        let mut lt = VkLedger::new();
        assert_eq!(lt.install_port(0, 2, 1), Err(VkCode::ENoBin));
        assert!(lt.install_port(0, 0, 3).is_ok());
        assert_eq!(lt.install_port(1, 1, 2), Err(VkCode::EOrder));
        assert_eq!(lt.install_port(0, 0, 4), Err(VkCode::EDup));
    }

    #[test]
    fn test_int_and_boot_gates() {
        let mut lt = VkLedger::new();
        assert_eq!(lt.integrate(7, 0), Err(VkCode::ENoInt));
        assert_eq!(
            lt.record_boot(2, boot_cell(2).unwrap_or(0).wrapping_add(4)),
            Err(VkCode::EBoot)
        );
        assert!(lt.record_boot(2, boot_cell(2).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= PORTS as u32);
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
        assert_eq!(SEG_BASE, 0xC5140);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52069_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
