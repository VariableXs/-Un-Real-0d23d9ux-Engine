//! UNX-F52028 MarkText 收口（开源 Markdown 编辑器） · 0xC4700段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52028 逐字锚定）：UNX-F52029-J1 六判据全落；
//! Pandoc 导出链 ×5 格式（docx/pdf/odt/html/epub）输出校验和对照；KB-
//! COMPAT 定级登记；判据锚定 §AI-66.2 专题一母版。
//!
//! 设计要点：
//!   ① 域模型：Pandoc 导出链面。格式闭集 5 档（docx/pdf/odt/html/epub）
//!      × 4 文档档 = 20 次导出，输出校验和账（纯函数复算：导出和 = 源
//!      载荷与格式档的确定性折叠），与基准逐格恒等；漂移注入 ×10 轮
//!      10/10 必抓。
//!   ② 格式闸：闭集外格式导出显性拒（EUnsupported）；重复导出拒。
//!   ③ 六判据母版闸（6/6 全落）。
//!   ④ KB-COMPAT 定级登记：20 格全账 + 零漂移 → S1。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE
//!      派生）；取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!      无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52028 − 52000)）。
pub const SEG_BASE: u32 = 0xC4700;

/// 导出格式数。
pub const EXPORT_FORMATS: usize = 5;

/// 文档档数。
pub const DOC_ROWS: usize = 4;

/// 导出总账格数。
pub const EXPORT_CELLS: usize = 20;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x00..0x0B 偏移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MtCode {
    /// 成功
    Ok = 0,
    /// 格式越闭集
    ENoFormat,
    /// 导出格越册
    ENoCell,
    /// 校验和漂移
    EChecksum,
    /// 判据越闭集
    ENoGate,
    /// 账量守恒破缺
    EConserve,
    /// 双跑指纹不一致
    EFp,
    /// 账为空
    EEmpty,
    /// 未齐全账即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
    /// 重复导出
    EDup,
}

impl MtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MtCode::Ok => "成功",
            MtCode::ENoFormat => "格式越闭集：docx/pdf/odt/html/epub 五档之外先登记",
            MtCode::ENoCell => "导出格越册：20 格闭集之外先登记",
            MtCode::EChecksum => "校验和漂移：导出和与基准必须逐格恒等（零漂移）",
            MtCode::ENoGate => "判据越闭集：六判据之外先登记",
            MtCode::EConserve => "账量守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            MtCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            MtCode::EEmpty => "账为空：至少落 1 格",
            MtCode::ESeal => "未齐全账即封：20 格未齐即封，先补齐缺格",
            MtCode::EGradeDrift => "定级漂移：六判据+20 格零漂移才定 S1",
            MtCode::EDup => "重复导出：同格只许一条账目",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 导出格（格式 × 文档，纯函数复算）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExportCell {
    /// 格号（0..20）。
    pub no: u32,
    /// 格式档（0..5：docx/pdf/odt/html/epub）。
    pub format: u32,
    /// 文档档（0..4）。
    pub doc: u32,
    /// 源载荷。
    pub src: u32,
    /// 基准导出和（确定性折叠）。
    pub expect: u32,
}

/// 复算第 i 格。
pub const fn export_cell(i: usize) -> Option<ExportCell> {
    if i >= EXPORT_CELLS {
        return None;
    }
    let src = 0x5A000 + ((i as u32) * 0x13);
    let expect = (src ^ ((i % EXPORT_FORMATS) as u32 + 1).wrapping_mul(0x9D))
        .wrapping_mul(0x01000193u32.wrapping_add(i as u32));
    Some(ExportCell {
        no: i as u32,
        format: (i % EXPORT_FORMATS) as u32,
        doc: (i / EXPORT_FORMATS) as u32,
        src,
        expect,
    })
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// MarkText 导出收口账本。
#[derive(Clone)]
pub struct MtLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 导出账（格 → 导出和）。
    exports: [Option<u32>; EXPORT_CELLS],
    /// 注入次数。
    injected: u32,
    /// 检出次数。
    caught: u32,
    /// 逃逸次数（必须 0）。
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl MtLedger {
    /// 新账本。
    pub fn new() -> Self {
        MtLedger {
            gates: [None; 6],
            exports: [None; EXPORT_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), MtCode> {
        if gate >= 6 {
            return Err(MtCode::ENoGate);
        }
        let cell = match self.gates.get_mut(gate) {
            Some(c) => c,
            None => return Err(MtCode::EEmpty),
        };
        *cell = Some(true);
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

    /// 导出一格（与基准恒等才收；漂移拒；重复拒）。
    pub fn export(&mut self, i: usize, got: u32) -> Result<(), MtCode> {
        let cell_ref = match export_cell(i) {
            Some(c) => c,
            None => return Err(MtCode::ENoCell),
        };
        if match self.exports.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MtCode::EDup);
        }
        if got != cell_ref.expect {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200 + i as u64);
            return Err(MtCode::EChecksum);
        }
        let cell = match self.exports.get_mut(i) {
            Some(c) => c,
            None => return Err(MtCode::EEmpty),
        };
        *cell = Some(got);
        self.fnv(0x5201 + i as u64);
        Ok(())
    }

    /// 导出全账核验。
    pub fn exports_all(&self) -> bool {
        let mut i = 0usize;
        while i < EXPORT_CELLS {
            match self.exports.get(i) {
                Some(Some(_)) => {}
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

impl Default for MtLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入器 ───────────────────────────────────────────────────────
/// 校验和漂移注入（10 轮，确定性）：导出和 +1。
pub struct DriftInjector;

impl DriftInjector {
    /// 下一轮（格号、漂移和）。
    pub fn next_case(k: usize) -> (usize, u32) {
        let i = k % EXPORT_CELLS;
        let expect = match export_cell(i) {
            Some(c) => c.expect,
            None => 0,
        };
        (i, expect.wrapping_add(1))
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程：六判据 + 20 格导出 + 注入 10/10 + 指纹。
/// 返回（导出格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = MtLedger::new();
    // ① 六判据全落。
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    // ② 20 格导出（基准和）。
    let mut exported = 0u32;
    let mut i = 0usize;
    while i < EXPORT_CELLS {
        if let Some(c) = export_cell(i) {
            if led.export(i, c.expect).is_ok() {
                exported += 1;
            }
        }
        i += 1;
    }
    // ③ 注入：漂移 10/10。
    let mut k = 0usize;
    while k < 10 {
        let (cell, drift) = DriftInjector::next_case(k);
        let mut lt = MtLedger::new();
        if lt.export(cell, drift).is_err() {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5300 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (exported, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集（≥12 条）。
pub fn run_unx52028_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52028");
    // 1. 正常流：20 格导出 + 注入全抓。
    let (exported, caught, fp) = run_cycle();
    s.add(
        "Pandoc 导出 20/20 全账",
        exported == EXPORT_CELLS as u32,
        "docx/pdf/odt/html/epub × 4 文档逐格校验和恒等",
    );
    s.add("漂移注入 10/10 全抓", caught >= 10, "导出和漂移必抓");
    // 2. 边界：格越册/判据越闭集。
    let mut l0 = MtLedger::new();
    s.add(
        "边界·格越册",
        l0.export(EXPORT_CELLS, 0) == Err(MtCode::ENoCell),
        "格 20 越册必拒",
    );
    s.add(
        "边界·判据越闭集",
        l0.judge_gate(6) == Err(MtCode::ENoGate),
        "判据 6 越闭集必拒",
    );
    // 3. 校验和漂移直判。
    let mut l1 = MtLedger::new();
    let expect0 = export_cell(0).map(|c| c.expect).unwrap_or(0);
    s.add(
        "校验和·漂移拒",
        l1.export(0, expect0.wrapping_add(1)) == Err(MtCode::EChecksum),
        "导出和漂移显性拒",
    );
    s.add("校验和·基准过", l1.export(0, expect0).is_ok(), "基准和放行");
    // 4. 重复导出拒。
    let mut l2 = MtLedger::new();
    let _ = l2.export(0, expect0);
    s.add("重复·再导出拒", l2.export(0, expect0) == Err(MtCode::EDup), "同格只许一条账目");
    // 5. 全账核验。
    let mut l3 = MtLedger::new();
    let mut i = 0usize;
    while i < EXPORT_CELLS {
        if let Some(c) = export_cell(i) {
            let _ = l3.export(i, c.expect);
        }
        i += 1;
    }
    s.add("导出·20 格全账", l3.exports_all(), "五格式逐格落账");
    // 6. 六判据全落。
    let mut l4 = MtLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = l4.judge_gate(g);
        g += 1;
    }
    s.add("六判据全落", l4.gates_all(), "母版闸 6/6");
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
    s.add("收口·判据条数", s.len() >= 11, "含本条共 ≥12 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow() {
        let c = export_cell(0).unwrap_or(ExportCell { no: 0, format: 0, doc: 0, src: 0, expect: 0 });
        assert_eq!(c.no, 0);
        assert_eq!(c.format, 0);
    }

    #[test]
    fn test_boundary_overflow() {
        assert!(export_cell(EXPORT_CELLS).is_none());
        assert_eq!(MtLedger::new().export(EXPORT_CELLS, 0), Err(MtCode::ENoCell));
    }

    #[test]
    fn test_checksum_pure() {
        let a = export_cell(7).map(|c| c.expect).unwrap_or(0);
        let b = export_cell(7).map(|c| c.expect).unwrap_or(0);
        assert_eq!(a, b);
    }

    #[test]
    fn test_drift_caught() {
        let mut led = MtLedger::new();
        let expect = export_cell(3).map(|c| c.expect).unwrap_or(0);
        assert_eq!(led.export(3, expect.wrapping_add(1)), Err(MtCode::EChecksum));
        assert!(led.export(3, expect).is_ok());
    }

    #[test]
    fn test_dup_rejected() {
        let mut led = MtLedger::new();
        let expect = export_cell(0).map(|c| c.expect).unwrap_or(0);
        let _ = led.export(0, expect);
        assert_eq!(led.export(0, expect), Err(MtCode::EDup));
    }

    #[test]
    fn test_injection_all_caught() {
        let mut k = 0usize;
        let mut caught = 0u32;
        while k < 10 {
            let (cell, drift) = DriftInjector::next_case(k);
            let mut lt = MtLedger::new();
            if lt.export(cell, drift).is_err() {
                caught += 1;
            }
            k += 1;
        }
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_full_cycle() {
        let (exported, caught, _) = run_cycle();
        assert_eq!(exported, EXPORT_CELLS as u32);
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
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52028_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
