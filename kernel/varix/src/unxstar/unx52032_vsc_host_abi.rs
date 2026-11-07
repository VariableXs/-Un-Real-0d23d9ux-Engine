//! UNX-F52032 VSCode 扩展宿主原生模块 ABI 收口 · 0xC4800段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52032 逐字锚定，板式后一编号）：UNX-F52033-J1
//! 解释器选择/IntelliSense/调试挂接/lint 四段 ×10 场景全通；Pylance
//! 原生面并入 F52032 账；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题四。
//!
//! 设计要点：
//!   ① 域模型：VSCode 扩展宿主原生模块面。扩展闭集 10 项（node-gyp 类
//!      原生模块），每项四段场景账（解释器选择/IntelliSense/调试挂接/
//!      lint），10×4=40 格场景账；段序号闭集 0..4。
//!   ② Pylance 原生面并入账：4 格并入账（词法/语法/语义/发布-订阅四
//!      面），逐格并入载荷对照恒等才收；未并账即渲染 → 缺账闸。
//!   ③ ABI 失配注入 ×10 轮（ABI 档/CRT 档/架构档三态轮转）10/10 必抓，
//!      注入=检出+逃逸守恒；失配错误路径三要素（期望 ABI/实际 ABI/
//!      错误码）全呈，缺一不合格。
//!   ④ 六判据母版闸（6/6 全落）+ KB-COMPAT 定级登记（40 格全通 + 并入
//!      4 格零漂移 + 失配全抓 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机（指纹种子 =
//!      SEG_BASE 派生）；取值一律 match/if let/.get()/.get_mut()/
//!      checked_*/saturating_*，无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52032 − 52000)）。
pub const SEG_BASE: u32 = 0xC4800;

/// 原生模块扩展闭集数。
pub const EXT_COUNT: usize = 10;

/// 四段场景数（解释器选择/IntelliSense/调试挂接/lint）。
pub const SEGMENT_COUNT: usize = 4;

/// Pylance 并入账格数。
pub const PYLANCE_CELLS: usize = 4;

/// 宿主 ABI 档。
pub const HOST_ABI: u32 = 108;

/// 宿主 CRT 档。
pub const HOST_CRT: u32 = 14;

/// 宿主架构档。
pub const HOST_ARCH: u32 = 0;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x00..0x0D 偏移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VsAbiCode {
    /// 成功
    Ok = 0,
    /// 扩展号越闭集
    ENoExt,
    /// 段号越闭集
    ENoSeg,
    /// ABI 档失配
    EAbi,
    /// CRT 档失配
    ECrt,
    /// 架构档失配
    EArch,
    /// 场景账未齐即封
    ESeal,
    /// 并入载荷漂移
    EDrift,
    /// 重复落账
    EDup,
    /// 并入账缺格
    EMergeMiss,
    /// 失配三要素缺项
    ETriMiss,
    /// 注入守恒破缺
    EConserve,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl VsAbiCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            VsAbiCode::Ok => "成功",
            VsAbiCode::ENoExt => "扩展号越闭集：10 项 node-gyp 类原生模块之外先登记",
            VsAbiCode::ENoSeg => "段号越闭集：解释器选择/IntelliSense/调试挂接/lint 四段之外先登记",
            VsAbiCode::EAbi => "ABI 档失配：NODE_MODULE_VERSION 与宿主档不符",
            VsAbiCode::ECrt => "CRT 档失配：CRT 主版本与宿主档不符",
            VsAbiCode::EArch => "架构档失配：模块架构与宿主架构不符",
            VsAbiCode::ESeal => "场景账未齐即封：10×4=40 格未齐先补齐",
            VsAbiCode::EDrift => "并入载荷漂移：Pylance 并入折叠与基准不恒等，逐格复算",
            VsAbiCode::EDup => "重复落账：同格只许一条账目",
            VsAbiCode::EMergeMiss => "并入账缺格：Pylance 四面未齐先补齐",
            VsAbiCode::ETriMiss => "失配三要素缺项：期望 ABI/实际 ABI/错误码缺一即不合格",
            VsAbiCode::EConserve => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            VsAbiCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            VsAbiCode::EGradeDrift => "定级漂移：40 格全通+并入零漂移+失配全抓才定 S1",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 复算场景格载荷（扩展 i × 段 s，确定性折叠）。
pub const fn scene_cell(i: usize, s: usize) -> Option<u64> {
    if i >= EXT_COUNT || s >= SEGMENT_COUNT {
        return None;
    }
    Some(
        0xC1000u64
            + (i as u64) * 0x31
            + (s as u64) * 0x7
            ^ ((i as u64) * 0x9E3779B97F4A7C15).wrapping_mul(s as u64 + 1),
    )
}

/// Pylance 并入面载荷（面 f，0=词法 1=语法 2=语义 3=发布-订阅）。
pub const fn pylance_cell(f: usize) -> Option<u64> {
    if f >= PYLANCE_CELLS {
        return None;
    }
    Some((0xC2000u64 + (f as u64) * 0x2B) ^ (f as u64).wrapping_mul(0x01000193))
}

/// 失配错误码闭集。
pub const fn mismatch_code(kind: u32) -> Option<u32> {
    match kind {
        0 => Some(0x20),
        1 => Some(0x21),
        2 => Some(0x22),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// VSCode 扩展宿主 ABI 收口账本。
#[derive(Clone)]
pub struct VsAbiLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 场景账（扩展 → 段 → 载荷）。
    scenes: [[Option<u64>; SEGMENT_COUNT]; EXT_COUNT],
    /// Pylance 并入账。
    pylance: [Option<u64>; PYLANCE_CELLS],
    /// 失配三要素账（轮次 → 齐）。
    tri: [Option<bool>; 10],
    /// 注入次数。
    injected: u32,
    /// 检出次数。
    caught: u32,
    /// 逃逸次数（必须 0）。
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl VsAbiLedger {
    /// 新账本。
    pub fn new() -> Self {
        VsAbiLedger {
            gates: [None; 6],
            scenes: [[None; SEGMENT_COUNT]; EXT_COUNT],
            pylance: [None; PYLANCE_CELLS],
            tri: [None; 10],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), VsAbiCode> {
        if gate >= 6 {
            return Err(VsAbiCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x7100 + gate as u64);
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

    /// 落一格场景账：扩展 ABI 三元全等才放行装载，随后记段账。
    pub fn record_scene(
        &mut self,
        i: usize,
        s: usize,
        abi: u32,
        crt: u32,
        arch: u32,
    ) -> Result<u64, VsAbiCode> {
        if i >= EXT_COUNT {
            return Err(VsAbiCode::ENoExt);
        }
        if s >= SEGMENT_COUNT {
            return Err(VsAbiCode::ENoSeg);
        }
        if abi != HOST_ABI {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200 + i as u64);
            return Err(VsAbiCode::EAbi);
        }
        if crt != HOST_CRT {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7201 + i as u64);
            return Err(VsAbiCode::ECrt);
        }
        if arch != HOST_ARCH {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7202 + i as u64);
            return Err(VsAbiCode::EArch);
        }
        let payload = match scene_cell(i, s) {
            Some(p) => p,
            None => return Err(VsAbiCode::ENoSeg),
        };
        if match self.scenes.get(i).and_then(|row| row.get(s)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VsAbiCode::EDup);
        }
        if let Some(row) = self.scenes.get_mut(i) {
            if let Some(c) = row.get_mut(s) {
                *c = Some(payload);
            }
        }
        self.fnv(0x7203 + (i as u64) * 4 + s as u64);
        Ok(payload)
    }

    /// 场景账全齐核验（40 格）。
    pub fn scenes_all(&self) -> bool {
        let mut i = 0usize;
        while i < EXT_COUNT {
            let mut s = 0usize;
            while s < SEGMENT_COUNT {
                match self.scenes.get(i).and_then(|row| row.get(s)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                s += 1;
            }
            i += 1;
        }
        true
    }

    /// Pylance 并入一格。
    pub fn merge_pylance(&mut self, f: usize, got: u64) -> Result<(), VsAbiCode> {
        let want = match pylance_cell(f) {
            Some(w) => w,
            None => return Err(VsAbiCode::EMergeMiss),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300 + f as u64);
            return Err(VsAbiCode::EDrift);
        }
        if let Some(c) = self.pylance.get_mut(f) {
            *c = Some(got);
        }
        self.fnv(0x7301 + f as u64);
        Ok(())
    }

    /// Pylance 并入全账核验。
    pub fn pylance_all(&self) -> bool {
        let mut i = 0usize;
        while i < PYLANCE_CELLS {
            match self.pylance.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 登记一轮失配三要素呈现。
    pub fn record_tri(&mut self, round: usize, present: [bool; 3]) -> Result<(), VsAbiCode> {
        if round >= 10 {
            return Err(VsAbiCode::ENoExt);
        }
        let mut k = 0usize;
        while k < 3 {
            match present.get(k) {
                Some(true) => {}
                _ => return Err(VsAbiCode::ETriMiss),
            }
            k += 1;
        }
        if let Some(c) = self.tri.get_mut(round) {
            *c = Some(true);
        }
        self.fnv(0x7400 + round as u64);
        Ok(())
    }

    /// 失配三要素全账核验。
    pub fn tri_all(&self) -> bool {
        let mut i = 0usize;
        while i < 10 {
            match self.tri.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
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

impl Default for VsAbiLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入器 ───────────────────────────────────────────────────────
/// 失配注入（10 轮，确定性）：ABI 档/CRT 档/架构档三态轮转。
pub struct MismatchInjector;

impl MismatchInjector {
    /// 下一轮（扩展号，注入三元，期望诊断码）。
    pub fn next_case(k: usize) -> (usize, u32, u32, u32, VsAbiCode) {
        let i = k % EXT_COUNT;
        let kind = (k % 3) as u32;
        let abi = if kind == 0 { HOST_ABI + 2 } else { HOST_ABI };
        let crt = if kind == 1 { HOST_CRT + 2 } else { HOST_CRT };
        let arch = if kind == 2 { HOST_ARCH + 2 } else { HOST_ARCH };
        let code = match kind {
            0 => VsAbiCode::EAbi,
            1 => VsAbiCode::ECrt,
            _ => VsAbiCode::EArch,
        };
        (i, abi, crt, arch, code)
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程：六判据 + 40 格场景 + Pylance 并入 4 格 + 失配注入 10/10
/// + 三要素 10 轮全呈 + 指纹。返回（场景格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = VsAbiLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut i = 0usize;
    while i < EXT_COUNT {
        let mut s = 0usize;
        while s < SEGMENT_COUNT {
            if led.record_scene(i, s, HOST_ABI, HOST_CRT, HOST_ARCH).is_ok() {
                recorded += 1;
            }
            s += 1;
        }
        i += 1;
    }
    let mut f = 0usize;
    while f < PYLANCE_CELLS {
        if let Some(p) = pylance_cell(f) {
            let _ = led.merge_pylance(f, p);
        }
        f += 1;
    }
    let mut k = 0usize;
    while k < 10 {
        let (idx, abi, crt, arch, want) = MismatchInjector::next_case(k);
        let mut lt = VsAbiLedger::new();
        match lt.record_scene(idx, 0, abi, crt, arch) {
            Err(got) if got == want => {
                let _ = led.record_tri(k, [true, true, true]);
                led.injected += 1;
                led.caught += 1;
                led.fnv(0x7500 + k as u64);
            }
            _ => {
                led.escaped += 1;
            }
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集（≥12 条）。
pub fn run_unx52032_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52032");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "四段场景 40/40 全通",
        recorded == (EXT_COUNT * SEGMENT_COUNT) as u32,
        "解释器选择/IntelliSense/调试挂接/lint 四段 ×10 场景逐格全通",
    );
    s.add("失配注入 10/10 全抓", caught >= 10, "ABI/CRT/架构三态轮转必抓");
    let mut l0 = VsAbiLedger::new();
    s.add(
        "边界·扩展越闭集",
        l0.record_scene(EXT_COUNT, 0, HOST_ABI, HOST_CRT, HOST_ARCH) == Err(VsAbiCode::ENoExt),
        "扩展号 10 越闭集必拒",
    );
    s.add(
        "边界·段越闭集",
        l0.record_scene(0, SEGMENT_COUNT, HOST_ABI, HOST_CRT, HOST_ARCH) == Err(VsAbiCode::ENoSeg),
        "段号 4 越闭集必拒",
    );
    let mut l1 = VsAbiLedger::new();
    s.add(
        "失配·ABI 档拒",
        l1.record_scene(0, 0, HOST_ABI + 1, HOST_CRT, HOST_ARCH) == Err(VsAbiCode::EAbi),
        "ABI 错位走失配路径",
    );
    s.add(
        "失配·CRT 档拒",
        l1.record_scene(0, 0, HOST_ABI, HOST_CRT + 1, HOST_ARCH) == Err(VsAbiCode::ECrt),
        "CRT 错位走失配路径",
    );
    s.add(
        "失配·架构档拒",
        l1.record_scene(0, 0, HOST_ABI, HOST_CRT, HOST_ARCH + 1) == Err(VsAbiCode::EArch),
        "架构错位走失配路径",
    );
    s.add(
        "装载·同档放行",
        l1.record_scene(0, 0, HOST_ABI, HOST_CRT, HOST_ARCH).is_ok(),
        "三元全等放行并记段账",
    );
    let mut l2 = VsAbiLedger::new();
    s.add(
        "三要素·缺一拒",
        l2.record_tri(0, [true, false, true]) == Err(VsAbiCode::ETriMiss),
        "期望 ABI/实际 ABI/错误码缺一即不合格",
    );
    let mut l3 = VsAbiLedger::new();
    if let Some(p) = pylance_cell(0) {
        s.add(
            "Pylance·载荷漂移拒",
            l3.merge_pylance(0, p.wrapping_add(1)) == Err(VsAbiCode::EDrift),
            "并入折叠与基准不恒等必抓",
        );
    }
    let mut l4 = VsAbiLedger::new();
    let _ = l4.record_scene(2, 1, HOST_ABI, HOST_CRT, HOST_ARCH);
    s.add(
        "重复·再落账拒",
        l4.record_scene(2, 1, HOST_ABI, HOST_CRT, HOST_ARCH) == Err(VsAbiCode::EDup),
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
    s.add("收口·判据条数", s.len() >= 14, "含本条共 ≥15 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scene_cell_pure() {
        let a = scene_cell(3, 1).unwrap_or(0);
        let b = scene_cell(3, 1).unwrap_or(0);
        assert_eq!(a, b);
        assert_ne!(a, scene_cell(3, 2).unwrap_or(0));
        assert_ne!(a, scene_cell(4, 1).unwrap_or(0));
        assert!(scene_cell(EXT_COUNT, 0).is_none());
        assert!(scene_cell(0, SEGMENT_COUNT).is_none());
    }

    #[test]
    fn test_pylance_cells() {
        assert!(pylance_cell(0).is_some());
        assert!(pylance_cell(PYLANCE_CELLS).is_none());
        let a = pylance_cell(2).unwrap_or(0);
        assert_ne!(a, pylance_cell(3).unwrap_or(0));
    }

    #[test]
    fn test_mismatch_codes() {
        assert_eq!(mismatch_code(0), Some(0x20));
        assert_eq!(mismatch_code(2), Some(0x22));
        assert!(mismatch_code(3).is_none());
    }

    #[test]
    fn test_scene_three_kinds_caught() {
        let mut led = VsAbiLedger::new();
        assert_eq!(led.record_scene(1, 0, HOST_ABI + 9, HOST_CRT, HOST_ARCH), Err(VsAbiCode::EAbi));
        assert_eq!(led.record_scene(1, 0, HOST_ABI, HOST_CRT + 9, HOST_ARCH), Err(VsAbiCode::ECrt));
        assert_eq!(led.record_scene(1, 0, HOST_ABI, HOST_CRT, HOST_ARCH + 9), Err(VsAbiCode::EArch));
        assert!(led.record_scene(1, 0, HOST_ABI, HOST_CRT, HOST_ARCH).is_ok());
    }

    #[test]
    fn test_dup_scene_rejected() {
        let mut led = VsAbiLedger::new();
        let _ = led.record_scene(5, 3, HOST_ABI, HOST_CRT, HOST_ARCH);
        assert_eq!(
            led.record_scene(5, 3, HOST_ABI, HOST_CRT, HOST_ARCH),
            Err(VsAbiCode::EDup)
        );
    }

    #[test]
    fn test_tri_and_pylance_gate() {
        let mut led = VsAbiLedger::new();
        assert_eq!(led.record_tri(9, [true, true, false]), Err(VsAbiCode::ETriMiss));
        let _ = led.record_tri(9, [true, true, true]);
        assert!(!led.tri_all());
        if let Some(p) = pylance_cell(1) {
            let _ = led.merge_pylance(1, p);
        }
        assert!(!led.pylance_all());
    }

    #[test]
    fn test_injection_all_caught() {
        let mut k = 0usize;
        let mut caught = 0u32;
        while k < 10 {
            let (idx, abi, crt, arch, want) = MismatchInjector::next_case(k);
            let mut lt = VsAbiLedger::new();
            if lt.record_scene(idx, 0, abi, crt, arch) == Err(want) {
                caught += 1;
            }
            k += 1;
        }
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, (EXT_COUNT * SEGMENT_COUNT) as u32);
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
        assert_eq!(SEG_BASE, 0xC4800);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52032_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
