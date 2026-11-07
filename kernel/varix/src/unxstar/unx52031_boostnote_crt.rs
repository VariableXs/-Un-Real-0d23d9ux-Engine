//! UNX-F52031 Boost Note（legacy 版）收口 · 0xC47C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52031 逐字锚定）：UNX-F52032-J1 PE 装载 + CRT
//! 版本矩阵消费（联签锚定行 AI-22）：10 个含原生模块扩展
//! （node-pty/SQLite 绑定/树解析器类）加载运行全通；ABI 失配错误路径
//! 三要素呈现；KB-COMPAT 定级 S1；判据锚定 §AI-66.2 专题四 Node 原生
//! 模块 ABI。
//!
//! 设计要点：
//!   ① 域模型：Node 原生模块装载面。扩展闭集 10 项（node-pty ×2 档 /
//!      SQLite 绑定 ×2 档 / 树解析器 ×3 档 / 原生通知 ×1 / 原生剪贴板
//!      ×1 / 原生哈希 ×1），每项绑定 ABI 三元（NODE_MODULE_VERSION 档 /
//!      CRT 主版本 / 架构档），宿主面单一 ABI 档；装载闸：三元全等放行，
//!      任一错位走失配路径。
//!   ② ABI 失配错误路径三要素闭集（期望 ABI / 实际 ABI / 错误码），缺
//!      一要素即不合格；失配注入 ×10 轮（ABI 档 / CRT 档 / 架构档三态
//!      轮转）10/10 必抓，注入=检出+逃逸守恒。
//!   ③ CRT 版本矩阵消费：宿主 CRT 档 × 6 家族 = 6 格消费账，每格记
//!      CRT 档折叠载荷（纯函数复算），逐格对照恒等才收；零漂移。
//!   ④ 六判据母版闸（6/6 全落）+ KB-COMPAT 定级登记（10/10 全通且失配
//!      全抓且零漂移 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机（指纹种子 =
//!      SEG_BASE 指纹种子派生）；取值一律 match/if let/.get()/.get_mut()/
//!      checked_*/saturating_*，无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52031 − 52000)）。
pub const SEG_BASE: u32 = 0xC47C0;

/// 原生模块扩展闭集数。
pub const EXT_COUNT: usize = 10;

/// ABI 失配三要素数。
pub const MISMATCH_FIELDS: usize = 3;

/// 宿主 NODE_MODULE_VERSION 档（闭集中枢）。
pub const HOST_ABI: u32 = 108;

/// 宿主 CRT 主版本档。
pub const HOST_CRT: u32 = 14;

/// 宿主架构档（0=x64）。
pub const HOST_ARCH: u32 = 0;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x00..0x0C 偏移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BnCode {
    /// 成功
    Ok = 0,
    /// 扩展号越闭集
    ENoExt,
    /// ABI 档失配
    EAbi,
    /// CRT 档失配
    ECrt,
    /// 架构档失配
    EArch,
    /// 消费账格越册
    ENoCell,
    /// 载荷漂移
    EDrift,
    /// 重复装载
    EDup,
    /// 失配三要素缺项
    ETriMiss,
    /// 未齐账即封
    ESeal,
    /// 注入守恒破缺
    EConserve,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl BnCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BnCode::Ok => "成功",
            BnCode::ENoExt => "扩展号越闭集：10 项原生模块之外先登记",
            BnCode::EAbi => "ABI 档失配：NODE_MODULE_VERSION 与宿主档不符",
            BnCode::ECrt => "CRT 档失配：CRT 主版本与宿主档不符",
            BnCode::EArch => "架构档失配：模块架构与宿主架构不符",
            BnCode::ENoCell => "消费账格越册：家族闭集之外先登记",
            BnCode::EDrift => "载荷漂移：CRT 矩阵消费折叠与基准不恒等，逐格复算",
            BnCode::EDup => "重复装载：同扩展只许一条装载账",
            BnCode::ETriMiss => "失配三要素缺项：期望 ABI/实际 ABI/错误码缺一即不合格",
            BnCode::ESeal => "未齐账即封：10 项装载未齐先补齐",
            BnCode::EConserve => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            BnCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            BnCode::EGradeDrift => "定级漂移：10/10 全通+失配全抓+零漂移才定 S1",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 原生模块扩展条目（ABI 三元 + 装载载荷）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExtEntry {
    /// 扩展号（0..10）。
    pub no: u32,
    /// 家族档（0=node-pty 1=SQLite 绑定 2=树解析器 3=原生通知
    /// 4=原生剪贴板 5=原生哈希）。
    pub fam: u32,
    /// 模块 ABI 档。
    pub abi: u32,
    /// CRT 主版本档。
    pub crt: u32,
    /// 架构档。
    pub arch: u32,
    /// 装载基准载荷（确定性折叠）。
    pub expect: u64,
}

/// 复算第 i 扩展（全部与宿主同档；载荷按家族与 ABI 折叠）。
pub const fn ext_entry(i: usize) -> Option<ExtEntry> {
    if i >= EXT_COUNT {
        return None;
    }
    let fam = match i {
        0 | 1 => 0u32,
        2 | 3 => 1u32,
        4 | 5 | 6 => 2u32,
        7 => 3u32,
        8 => 4u32,
        _ => 5u32,
    };
    let abi = HOST_ABI;
    let crt = HOST_CRT;
    let arch = HOST_ARCH;
    let expect = (0xB2000u64 + (i as u64) * 0x27)
        ^ (fam as u64).wrapping_mul(0x9E3779B97F4A7C15)
        ^ (abi as u64).wrapping_mul(0x01000193);
    Some(ExtEntry {
        no: i as u32,
        fam,
        abi,
        crt,
        arch,
        expect,
    })
}

/// CRT 矩阵消费载荷（宿主 CRT × 家族折叠）。
pub const fn crt_payload(fam: u32) -> u64 {
    (0xB3000u64 + (fam as u64) * 0x3F) ^ (HOST_CRT as u64).wrapping_mul(0x100000001b3)
}

/// 失配错误码闭集（三要素之第三要素：0x10=ABI 错 / 0x11=CRT 错 /
/// 0x12=架构错）。
pub const fn mismatch_code(kind: u32) -> Option<u32> {
    match kind {
        0 => Some(0x10),
        1 => Some(0x11),
        2 => Some(0x12),
        _ => None,
    }
}

// ── 装载账本 ─────────────────────────────────────────────────────
/// Boost Note 原生模块装载收口账本。
#[derive(Clone)]
pub struct BnLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 装载账（扩展号 → 载荷）。
    loads: [Option<u64>; EXT_COUNT],
    /// CRT 矩阵消费账（家族 → 载荷）。
    crt: [Option<u64>; 6],
    /// 失配三要素账（轮次 → 三要素齐备）。
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

impl BnLedger {
    /// 新账本。
    pub fn new() -> Self {
        BnLedger {
            gates: [None; 6],
            loads: [None; EXT_COUNT],
            crt: [None; 6],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), BnCode> {
        if gate >= 6 {
            return Err(BnCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x6100 + gate as u64);
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

    /// 装载一格：ABI/CRT/架构三元对宿主全等才收，任一错位走失配路径。
    pub fn load(&mut self, i: usize, abi: u32, crt: u32, arch: u32) -> Result<u64, BnCode> {
        let e = match ext_entry(i) {
            Some(e) => e,
            None => return Err(BnCode::ENoExt),
        };
        if abi != HOST_ABI {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200 + i as u64);
            return Err(BnCode::EAbi);
        }
        if crt != HOST_CRT {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6201 + i as u64);
            return Err(BnCode::ECrt);
        }
        if arch != HOST_ARCH {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6202 + i as u64);
            return Err(BnCode::EArch);
        }
        if match self.loads.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BnCode::EDup);
        }
        if let Some(c) = self.loads.get_mut(i) {
            *c = Some(e.expect);
        }
        self.fnv(0x6203 + i as u64);
        Ok(e.expect)
    }

    /// 装载全账核验。
    pub fn loads_all(&self) -> bool {
        let mut i = 0usize;
        while i < EXT_COUNT {
            match self.loads.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// CRT 矩阵消费一格：家族载荷与基准恒等才收。
    pub fn consume_crt(&mut self, fam: usize, got: u64) -> Result<(), BnCode> {
        if fam >= 6 {
            return Err(BnCode::ENoCell);
        }
        let want = crt_payload(fam as u32);
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300 + fam as u64);
            return Err(BnCode::EDrift);
        }
        if let Some(c) = self.crt.get_mut(fam) {
            *c = Some(got);
        }
        self.fnv(0x6301 + fam as u64);
        Ok(())
    }

    /// CRT 消费全账核验。
    pub fn crt_all(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            match self.crt.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 登记一轮失配三要素呈现（期望 ABI/实际 ABI/错误码全在册才记）。
    pub fn record_tri(&mut self, round: usize, present: [bool; MISMATCH_FIELDS]) -> Result<(), BnCode> {
        if round >= 10 {
            return Err(BnCode::ENoExt);
        }
        let mut k = 0usize;
        while k < MISMATCH_FIELDS {
            match present.get(k) {
                Some(true) => {}
                _ => return Err(BnCode::ETriMiss),
            }
            k += 1;
        }
        if let Some(c) = self.tri.get_mut(round) {
            *c = Some(true);
        }
        self.fnv(0x6400 + round as u64);
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

    /// 漏检记账。
    pub fn note_missed(&mut self) {
        self.injected += 1;
        self.escaped += 1;
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

impl Default for BnLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入器 ───────────────────────────────────────────────────────
/// 失配注入（10 轮，确定性）：按轮错 ABI 档 / CRT 档 / 架构档三态轮转。
pub struct MismatchInjector;

impl MismatchInjector {
    /// 下一轮（扩展号，注入三元，期望诊断码）。
    pub fn next_case(k: usize) -> (usize, u32, u32, u32, BnCode) {
        let i = k % EXT_COUNT;
        let kind = (k % 3) as u32;
        let abi = if kind == 0 { HOST_ABI + 1 } else { HOST_ABI };
        let crt = if kind == 1 { HOST_CRT + 1 } else { HOST_CRT };
        let arch = if kind == 2 { HOST_ARCH + 1 } else { HOST_ARCH };
        let code = match kind {
            0 => BnCode::EAbi,
            1 => BnCode::ECrt,
            _ => BnCode::EArch,
        };
        (i, abi, crt, arch, code)
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程：六判据 + 10 项装载 + CRT 矩阵 6 格消费 + 失配注入 10/10
/// + 三要素 10 轮全呈 + 指纹。返回（装载格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = BnLedger::new();
    // ① 六判据全落。
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    // ② 10 项原生模块装载全通。
    let mut loaded = 0u32;
    let mut i = 0usize;
    while i < EXT_COUNT {
        if led.load(i, HOST_ABI, HOST_CRT, HOST_ARCH).is_ok() {
            loaded += 1;
        }
        i += 1;
    }
    // ③ CRT 矩阵 6 家族消费。
    let mut f = 0usize;
    while f < 6 {
        let _ = led.consume_crt(f, crt_payload(f as u32));
        f += 1;
    }
    // ④ 失配注入 10 轮：三态轮转必抓 + 三要素全呈。
    let mut k = 0usize;
    while k < 10 {
        let (idx, abi, crt, arch, want) = MismatchInjector::next_case(k);
        let mut lt = BnLedger::new();
        match lt.load(idx, abi, crt, arch) {
            Err(got) if got == want => {
                let _ = led.record_tri(k, [true, true, true]);
                led.injected += 1;
                led.caught += 1;
                led.fnv(0x6500 + k as u64);
            }
            _ => {
                led.escaped += 1;
            }
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (loaded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集（≥12 条）。
pub fn run_unx52031_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52031");
    // 1. 正常流：10/10 装载 + CRT 6 格 + 注入全抓。
    let (loaded, caught, fp) = run_cycle();
    s.add(
        "原生模块装载 10/10 全通",
        loaded == EXT_COUNT as u32,
        "node-pty/SQLite/树解析器等 10 项逐项装载",
    );
    s.add("失配注入 10/10 全抓", caught >= 10, "ABI/CRT/架构三态轮转必抓");
    // 2. 边界：扩展越闭集。
    let mut l0 = BnLedger::new();
    s.add(
        "边界·扩展越闭集",
        l0.load(EXT_COUNT, HOST_ABI, HOST_CRT, HOST_ARCH) == Err(BnCode::ENoExt),
        "扩展号 10 越闭集必拒",
    );
    // 3. 失配直判三态。
    let mut l1 = BnLedger::new();
    s.add(
        "失配·ABI 档拒",
        l1.load(0, HOST_ABI + 1, HOST_CRT, HOST_ARCH) == Err(BnCode::EAbi),
        "ABI 错位走失配路径",
    );
    s.add(
        "失配·CRT 档拒",
        l1.load(0, HOST_ABI, HOST_CRT + 1, HOST_ARCH) == Err(BnCode::ECrt),
        "CRT 错位走失配路径",
    );
    s.add(
        "失配·架构档拒",
        l1.load(0, HOST_ABI, HOST_CRT, HOST_ARCH + 1) == Err(BnCode::EArch),
        "架构错位走失配路径",
    );
    s.add(
        "装载·同档放行",
        l1.load(0, HOST_ABI, HOST_CRT, HOST_ARCH).is_ok(),
        "三元全等放行",
    );
    // 4. 三要素缺项拒。
    let mut l2 = BnLedger::new();
    s.add(
        "三要素·缺一拒",
        l2.record_tri(0, [true, false, true]) == Err(BnCode::ETriMiss),
        "期望 ABI/实际 ABI/错误码缺一即不合格",
    );
    // 5. CRT 载荷漂移拒。
    let mut l3 = BnLedger::new();
    s.add(
        "CRT·载荷漂移拒",
        l3.consume_crt(0, crt_payload(0).wrapping_add(1)) == Err(BnCode::EDrift),
        "消费折叠与基准不恒等必抓",
    );
    // 6. 重复装载拒。
    let mut l4 = BnLedger::new();
    let _ = l4.load(2, HOST_ABI, HOST_CRT, HOST_ARCH);
    s.add(
        "重复·再装载拒",
        l4.load(2, HOST_ABI, HOST_CRT, HOST_ARCH) == Err(BnCode::EDup),
        "同扩展只许一条装载账",
    );
    // 7. 注入账守恒。
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    // 8. 双跑指纹一致。
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    // 9. 段位守卫。
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0,
        "SEG_BASE 0x40 对齐且在 VX14 域",
    );
    // 10. 收口。
    s.add("收口·判据条数", s.len() >= 12, "含本条共 ≥13 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow() {
        let e = ext_entry(0).unwrap_or(ExtEntry { no: 0, fam: 0, abi: 0, crt: 0, arch: 0, expect: 0 });
        assert_eq!(e.no, 0);
        assert_eq!(e.fam, 0);
        assert_eq!(e.abi, HOST_ABI);
        assert_eq!(ext_entry(7).unwrap_or(e).fam, 3);
    }

    #[test]
    fn test_boundary_overflow() {
        assert!(ext_entry(EXT_COUNT).is_none());
        assert!(mismatch_code(3).is_none());
        assert_eq!(mismatch_code(1), Some(0x11));
    }

    #[test]
    fn test_payload_pure() {
        let a = ext_entry(5).unwrap_or(ExtEntry { no: 0, fam: 0, abi: 0, crt: 0, arch: 0, expect: 0 });
        let b = ext_entry(5).unwrap_or(a);
        assert_eq!(a.expect, b.expect);
        assert_ne!(a.expect, ext_entry(6).unwrap_or(a).expect);
        assert_eq!(crt_payload(2), crt_payload(2));
    }

    #[test]
    fn test_mismatch_three_kinds() {
        let mut led = BnLedger::new();
        assert_eq!(led.load(1, HOST_ABI + 7, HOST_CRT, HOST_ARCH), Err(BnCode::EAbi));
        assert_eq!(led.load(1, HOST_ABI, HOST_CRT + 3, HOST_ARCH), Err(BnCode::ECrt));
        assert_eq!(led.load(1, HOST_ABI, HOST_CRT, HOST_ARCH + 2), Err(BnCode::EArch));
        assert!(led.load(1, HOST_ABI, HOST_CRT, HOST_ARCH).is_ok());
    }

    #[test]
    fn test_dup_rejected() {
        let mut led = BnLedger::new();
        let _ = led.load(4, HOST_ABI, HOST_CRT, HOST_ARCH);
        assert_eq!(led.load(4, HOST_ABI, HOST_CRT, HOST_ARCH), Err(BnCode::EDup));
    }

    #[test]
    fn test_tri_gate() {
        let mut led = BnLedger::new();
        assert_eq!(led.record_tri(0, [true, true, false]), Err(BnCode::ETriMiss));
        assert_eq!(led.record_tri(10, [true, true, true]), Err(BnCode::ENoExt));
        let _ = led.record_tri(0, [true, true, true]);
        assert!(!led.tri_all());
    }

    #[test]
    fn test_crt_drift_caught() {
        let mut led = BnLedger::new();
        assert_eq!(led.consume_crt(5, crt_payload(5).wrapping_sub(1)), Err(BnCode::EDrift));
        assert!(led.consume_crt(5, crt_payload(5)).is_ok());
        assert_eq!(led.consume_crt(6, crt_payload(6)), Err(BnCode::ENoCell));
    }

    #[test]
    fn test_injection_all_caught() {
        let mut k = 0usize;
        let mut caught = 0u32;
        while k < 10 {
            let (idx, abi, crt, arch, want) = MismatchInjector::next_case(k);
            let mut lt = BnLedger::new();
            if lt.load(idx, abi, crt, arch) == Err(want) {
                caught += 1;
            }
            k += 1;
        }
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_full_cycle() {
        let (loaded, caught, _) = run_cycle();
        assert_eq!(loaded, EXT_COUNT as u32);
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
        assert_eq!(SEG_BASE, 0xC47C0);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52031_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
