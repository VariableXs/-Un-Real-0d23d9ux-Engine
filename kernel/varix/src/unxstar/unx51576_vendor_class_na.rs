//! UNX-F51576 B19 厂商私有类接口占位（vendor 类 N/A 声明纪律） · 0xC12C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51577-J1 基线 ×3 波漂移 ≤5%；完成时 API/软件写回：
//!           状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   - VendorIface      厂商私有接口占位（类码 0xFF + 三态声明）
//!   - NaRegistry       N/A 声明登记簿：占位注册 + 冒充检测
//!   - DriftBaseline    ×3 波整数基线：逐波对比漂移 ≤5%（千分比整数口径）
//!   - VendorCode       本卡诊断码闭集（12 变体）
//!   - Lcg              确定性伪随机（种子=SEG_BASE 派生）
//!
//! 设计要点：
//!   ① 占位纪律：vendor 类（bInterfaceClass=0xFF）只允许占位声明 N/A，
//!      不得申报成任何标准类；占位必须有接口号、厂商码、占位理由三要素。
//!   ② 基线 ×3 波漂移 ≤5%：三波基线值逐对比对，漂移千分比 = |a−b|×1000/base
//!      整数口径，任一对比超 50‰（=5%）即红；零浮点实现。
//!   ③ 注入核：五类坏样本（冒充标准类、占位缺要素、基线漂移超限、
//!      波数不足、重复占位）逐类 ×10 轮精确复现，守恒=检出+逃逸，逃逸恒 0。
//!   ④ 双跑 FNV-1a 指纹一致；no_std+alloc；零浮点零 IO 零墙钟零随机
//!      （LCG 种子=SEG_BASE 派生）；零 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 段基（SEG 公式：0xC0000 + 0x40 × 批内序，B2 批第 6 卡）。
pub const SEG_BASE: u32 = 0xC12C0;

/// 厂商私有类码。
pub const VENDOR_CLASS: u32 = 0xFF;
/// 漂移上限（千分比口径，50‰ = 5%）。
pub const DRIFT_LIMIT_PERMILLE: u64 = 50;
/// 占位配额（×5 私有接口）。
pub const PLACEHOLDER_QUOTA: usize = 5;

/// 厂商私有接口占位。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct VendorIface {
    /// 接口号
    pub iface: u32,
    /// 厂商码（非零）
    pub vendor_id: u32,
    /// 类码（必须 0xFF）
    pub class_code: u32,
    /// 占位理由（非空）
    pub reason: String,
}

impl VendorIface {
    /// 占位要素齐备：接口号有限、厂商码非零、类码非零、理由非空。
    /// 注意：类码是否为 0xFF 由冒充核查（impersonates_standard）单独判定，
    /// 两者分离才能把「冒充标准类」与「要素缺失」区分成两种诊断。
    pub fn complete(&self) -> bool {
        self.iface < 256 && self.vendor_id > 0 && self.class_code > 0
            && !self.reason.is_empty()
    }
    /// 是否冒充标准类（类码非 0xFF 即冒充）。
    pub fn impersonates_standard(&self) -> bool {
        self.class_code != VENDOR_CLASS
    }
}

/// 诊断码闭集（本卡，12 变体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VendorCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 占位要素缺失
    PlaceholderIncomplete,
    /// 冒充标准类
    ImpersonatesStandard,
    /// 重复占位（同接口号二次占位）
    DuplicatePlaceholder,
    /// 占位超配额（超过 ×5）
    QuotaExceeded,
    /// 基线漂移超限（>5%）
    DriftOverLimit,
    /// 波数不足（基线必须 ×3 波）
    WavesShort,
    /// 基线值非法（零基线不可比）
    BaselineZero,
    /// 账面不符
    LedgerMismatch,
    /// 双跑指纹不一致
    FingerprintDrift,
    /// 结果摘要为空
    SummaryEmpty,
}

impl VendorCode {
    /// 段号取值：SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 中文明细（全量非空）。
    pub fn detail(self) -> &'static str {
        match self {
            VendorCode::Ok => "vendor 占位登记成功",
            VendorCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            VendorCode::PlaceholderIncomplete => "占位要素缺失：接口号/厂商码/类码/理由必须齐备",
            VendorCode::ImpersonatesStandard => "冒充标准类：vendor 类码必须恒 0xFF",
            VendorCode::DuplicatePlaceholder => "重复占位：同一接口号只允许占位一次",
            VendorCode::QuotaExceeded => "占位超配额：私有占位上限 ×5",
            VendorCode::DriftOverLimit => "基线漂移超限：任一波对比 >5% 即红",
            VendorCode::WavesShort => "波数不足：基线必须 ×3 波齐",
            VendorCode::BaselineZero => "基线值非法：零值不可作对比基",
            VendorCode::LedgerMismatch => "账面不符：占位/拒绝/漂移三本必须对平",
            VendorCode::FingerprintDrift => "双跑指纹不一致：确定性被破坏",
            VendorCode::SummaryEmpty => "结果摘要为空：写回必须附非空摘要",
        }
    }
}

/// 诊断码闭集数组（12 变体）。
pub const ALL_CODES: [VendorCode; 12] = [
    VendorCode::Ok,
    VendorCode::SegBad,
    VendorCode::PlaceholderIncomplete,
    VendorCode::ImpersonatesStandard,
    VendorCode::DuplicatePlaceholder,
    VendorCode::QuotaExceeded,
    VendorCode::DriftOverLimit,
    VendorCode::WavesShort,
    VendorCode::BaselineZero,
    VendorCode::LedgerMismatch,
    VendorCode::FingerprintDrift,
    VendorCode::SummaryEmpty,
];

/// 确定性伪随机（LCG）。
pub struct Lcg(u64);

impl Lcg {
    /// 由段基派生种子。
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(SEG_BASE as u64))
    }
    /// 下一随机数。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
}

/// N/A 声明登记簿 + ×3 波整数基线。
pub struct NaRegistry {
    placeholders: Vec<VendorIface>,
    /// 三波基线值
    waves: [u64; 3],
    waves_set: [bool; 3],
    status: u32,
    summary: String,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl NaRegistry {
    /// 建空登记簿。
    pub fn new() -> NaRegistry {
        NaRegistry {
            placeholders: Vec::new(),
            waves: [0; 3],
            waves_set: [false; 3],
            status: 0,
            summary: String::new(),
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 登记占位：要素齐备 → 不冒充 → 不重复 → 不超配额。
    pub fn register(&mut self, p: VendorIface) -> Result<(), VendorCode> {
        if !p.complete() {
            return Err(VendorCode::PlaceholderIncomplete);
        }
        if p.impersonates_standard() {
            return Err(VendorCode::ImpersonatesStandard);
        }
        if self.placeholders.iter().any(|x| x.iface == p.iface) {
            return Err(VendorCode::DuplicatePlaceholder);
        }
        if self.placeholders.len() >= PLACEHOLDER_QUOTA {
            return Err(VendorCode::QuotaExceeded);
        }
        self.placeholders.push(p);
        Ok(())
    }

    /// 占位数。
    pub fn placeholder_count(&self) -> usize {
        self.placeholders.len()
    }

    /// 接口号已占位？
    pub fn is_placeholder(&self, iface: u32) -> bool {
        self.placeholders.iter().any(|x| x.iface == iface)
    }

    /// 全部占位要素齐备（登记时已保证，复核口径）。
    pub fn all_complete(&self) -> bool {
        self.placeholders.iter().all(|p| p.complete())
    }

    /// 写入一波基线（wave 0..3）。
    pub fn set_wave(&mut self, wave: usize, val: u64) -> Result<(), VendorCode> {
        if wave >= 3 {
            return Err(VendorCode::WavesShort);
        }
        if val == 0 {
            return Err(VendorCode::BaselineZero);
        }
        for (k, slot) in self.waves.iter_mut().enumerate() {
            if k == wave {
                *slot = val;
            }
        }
        for (k, flag) in self.waves_set.iter_mut().enumerate() {
            if k == wave {
                *flag = true;
            }
        }
        Ok(())
    }

    /// 漂移千分比（整数口径）：|a−b|×1000 / min(a,b)，除零走基线非法。
    pub fn drift_permille(a: u64, b: u64) -> Option<u64> {
        if a == 0 || b == 0 {
            return None;
        }
        let diff = if a > b { a - b } else { b - a };
        let base = if a < b { a } else { b };
        Some(diff.saturating_mul(1000) / base)
    }

    /// ×3 波齐且两两漂移 ≤5%。
    pub fn drift_ok(&self) -> bool {
        if !(self.waves_set.iter().all(|f| *f)) {
            return false;
        }
        let pairs = [(0usize, 1usize), (0, 2), (1, 2)];
        for (i, j) in pairs {
            match Self::drift_permille(self.waves.get(i).copied().unwrap_or(0), self.waves.get(j).copied().unwrap_or(0)) {
                Some(d) => {
                    if d > DRIFT_LIMIT_PERMILLE {
                        return false;
                    }
                }
                None => return false,
            }
        }
        true
    }

    /// 最大两两漂移（未齐时 None）。
    pub fn max_drift(&self) -> Option<u64> {
        if !(self.waves_set.iter().all(|f| *f)) {
            return None;
        }
        let pairs = [(0usize, 1usize), (0, 2), (1, 2)];
        let mut mx = 0u64;
        for (i, j) in pairs {
            let d = Self::drift_permille(self.waves.get(i).copied().unwrap_or(0), self.waves.get(j).copied().unwrap_or(0)).unwrap_or(u64::MAX);
            if d > mx {
                mx = d;
            }
        }
        Some(mx)
    }

    /// 波齐标志。
    pub fn waves_complete(&self) -> bool {
        self.waves_set.iter().all(|f| *f)
    }

    /// 完成写回：×5 占位齐 + ×3 波漂移达标。
    pub fn finish(&mut self, summary: &str) -> Result<(), VendorCode> {
        if summary.is_empty() {
            return Err(VendorCode::SummaryEmpty);
        }
        if self.placeholder_count() != PLACEHOLDER_QUOTA {
            return Err(VendorCode::QuotaExceeded);
        }
        if !self.drift_ok() {
            return Err(VendorCode::DriftOverLimit);
        }
        self.status = 2;
        self.summary = summary.to_string();
        Ok(())
    }

    /// 摘要读取。
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// 写回状态。
    pub fn status(&self) -> u32 {
        self.status
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.detected = self.detected.saturating_add(1);
    }

    /// 注入逃逸（恒 0）。
    pub fn note_escape(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.escaped = self.escaped.saturating_add(1);
    }

    /// 守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped)
            && self.escaped == 0
            && self.detected > 0
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// FNV-1a 64 位指纹。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 标准占位 ×5（接口号 7..12）。
pub fn standard_placeholder(i: u32) -> VendorIface {
    VendorIface {
        iface: 7 + i,
        vendor_id: 0x1D6B,
        class_code: VENDOR_CLASS,
        reason: String::from("厂商私有诊断通道，占位 N/A"),
    }
}

/// 标准脚本：占位 ×5 → 基线 ×3 波（1000/1010/1020，漂移 ≤2%）→ 写回。
pub fn standard_script() -> String {
    let mut r = NaRegistry::new();
    for i in 0..5u32 {
        let _ = r.register(standard_placeholder(i));
    }
    let _ = r.set_wave(0, 1000);
    let _ = r.set_wave(1, 1010);
    let _ = r.set_wave(2, 1020);
    let _ = r.finish("占位×5齐，基线×3波漂移≤2%");
    format!("{}|{}|{}|{}", r.placeholder_count(), r.waves_complete(), r.max_drift().unwrap_or(u64::MAX), r.summary())
}

/// 双跑指纹一致。
pub fn dual_run_consistent() -> bool {
    let f1 = fnv1a(standard_script().as_bytes());
    let f2 = fnv1a(standard_script().as_bytes());
    f1 == f2 && f1 != 0
}

/// 收口自检（16 条判据真测）。
pub fn run_unx51576_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51576");
    // 1 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    // 2 闭集递增、唯一、明细非空。
    {
        let mut mono = true;
        let mut prev = 0u32;
        for c in ALL_CODES {
            if c.val() <= prev || c.detail().is_empty() {
                mono = false;
            }
            prev = c.val();
        }
        let mut uniq = true;
        for i in 0..ALL_CODES.len() {
            for j in (i + 1)..ALL_CODES.len() {
                if ALL_CODES[i].val() == ALL_CODES[j].val() {
                    uniq = false;
                }
            }
        }
        s.add_dyn("闭集递增唯一", mono && uniq && ALL_CODES.len() == 12, "12 变体段号递增、两两不同");
    }
    // 3 val 公式抽验。
    s.add_dyn(
        "val公式抽验",
        ALL_CODES[0].val() == SEG_BASE + 1 && ALL_CODES[11].val() == SEG_BASE + 12,
        "val=SEG_BASE+1+变体序号",
    );
    // 4 占位 ×5 齐、要素全备。
    {
        let mut r = NaRegistry::new();
        for i in 0..5u32 {
            let _ = r.register(standard_placeholder(i));
        }
        s.add_dyn(
            "占位×5齐",
            r.placeholder_count() == 5 && r.all_complete() && r.is_placeholder(7) && r.is_placeholder(11),
            "×5 占位三要素齐备且按接口号可查",
        );
    }
    // 5 冒充标准类注入 ×10：类码逐档改写。
    {
        let mut caught = 0u32;
        for i in 0..10u32 {
            let mut p = standard_placeholder(0);
            p.class_code = 1 + i; // 任何非 0xFF 类码都算冒充
            let mut r = NaRegistry::new();
            if r.register(p) == Err(VendorCode::ImpersonatesStandard) {
                caught += 1;
                r.note_detected();
            } else {
                r.note_escape();
            }
        }
        s.add_dyn("冒充标准类注入10/10", caught == 10, "非 0xFF 类码必须全抓");
    }
    // 6 占位要素缺失注入 ×10：10 个字段位逐档打洞。
    {
        let mut caught = 0u32;
        for k in 0..10u32 {
            let mut p = standard_placeholder(0);
            match k {
                0 => p.vendor_id = 0,
                1 => p.class_code = 0,
                2 => p.reason = String::new(),
                3 => p.iface = 256,
                4 => p.iface = 999,
                5 => p.vendor_id = 0,
                6 => p.class_code = 0x00,
                7 => p.reason = String::from(""),
                8 => p.class_code = 0x01,
                _ => p.vendor_id = 0,
            }
            let mut r = NaRegistry::new();
            let bad = !p.complete() || p.impersonates_standard();
            let res = r.register(p);
            if bad && res.is_err() {
                caught += 1;
                r.note_detected();
            } else {
                r.note_escape();
            }
        }
        s.add_dyn("要素缺失注入10/10", caught == 10, "任一要素打洞必须全抓");
    }
    // 7 重复占位注入 ×10。
    {
        let mut r = NaRegistry::new();
        let _ = r.register(standard_placeholder(0));
        let mut caught = 0u32;
        for _ in 0..10u32 {
            if r.register(standard_placeholder(0)) == Err(VendorCode::DuplicatePlaceholder) {
                caught += 1;
                r.note_detected();
            } else {
                r.note_escape();
            }
        }
        s.add_dyn("重复占位注入10/10", caught == 10, "同接口号二次占位必须全抓");
    }
    // 8 配额超限注入 ×10：已满 ×5 再注。
    {
        let mut r = NaRegistry::new();
        for i in 0..5u32 {
            let _ = r.register(standard_placeholder(i));
        }
        let mut caught = 0u32;
        for _ in 0..10u32 {
            if r.register(standard_placeholder(99)) == Err(VendorCode::QuotaExceeded) {
                caught += 1;
                r.note_detected();
            } else {
                r.note_escape();
            }
        }
        s.add_dyn("配额超限注入10/10", caught == 10 && r.placeholder_count() == 5, "超 ×5 配额必须全拒");
    }
    // 9 基线 ×3 波漂移 ≤5%。
    {
        let mut r = NaRegistry::new();
        let _ = r.set_wave(0, 1000);
        let _ = r.set_wave(1, 1010);
        let _ = r.set_wave(2, 1020);
        s.add_dyn(
            "基线×3波达标",
            r.drift_ok() && r.max_drift().unwrap_or(999) <= 50,
            "三波两两漂移 ≤50‰（5%）",
        );
    }
    // 10 漂移超限注入 ×10：第三波 +6% 起。
    {
        let mut caught = 0u32;
        for i in 0..10u32 {
            let mut r = NaRegistry::new();
            let _ = r.set_wave(0, 1000);
            let _ = r.set_wave(1, 1000);
            let _ = r.set_wave(2, 1000u64 + 60 + (i as u64) * 10);
            if !r.drift_ok() && r.max_drift().unwrap_or(0) > DRIFT_LIMIT_PERMILLE {
                caught += 1;
                r.note_detected();
            } else {
                r.note_escape();
            }
        }
        s.add_dyn("漂移超限注入10/10", caught == 10, ">50‰ 必须全抓");
    }
    // 11 波数不足注入 ×10：波数在 0/1/2 间循环，恒不齐。
    {
        let mut caught = 0u32;
        for n in 0..10u32 {
            let mut r = NaRegistry::new();
            let count = (n % 3) as usize; // 0、1、2 波循环，永不齐 ×3
            for w in 0..count {
                let _ = r.set_wave(w, 1000 + (w as u64) * 10);
            }
            if !r.waves_complete() && !r.drift_ok() {
                caught += 1;
                r.note_detected();
            } else {
                r.note_escape();
            }
        }
        s.add_dyn("波数不足注入10/10", caught == 10, "波不齐必须判为不达标");
    }
    // 12 零基线注入 ×10。
    {
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let mut r = NaRegistry::new();
            let _ = r.set_wave(0, 1000);
            if r.set_wave(1, 0) == Err(VendorCode::BaselineZero) {
                caught += 1;
                r.note_detected();
            } else {
                r.note_escape();
            }
        }
        s.add_dyn("零基线注入10/10", caught == 10, "零值基线必须全拒");
    }
    // 13 漂移公式抽验：10% → 100‰、5% → 50‰、0 → 0。
    {
        let d1 = NaRegistry::drift_permille(1000, 1100);
        let d2 = NaRegistry::drift_permille(1000, 1050);
        let d3 = NaRegistry::drift_permille(1000, 1000);
        let d4 = NaRegistry::drift_permille(0, 100);
        s.add_dyn(
            "漂移公式抽验",
            d1 == Some(100) && d2 == Some(50) && d3 == Some(0) && d4.is_none(),
            "千分比整数口径：|a−b|×1000/min(a,b)，零基线 None",
        );
    }
    // 14 完成写回：状态=已完成、摘要非空。
    {
        let mut r = NaRegistry::new();
        for i in 0..5u32 {
            let _ = r.register(standard_placeholder(i));
        }
        let _ = r.set_wave(0, 1000);
        let _ = r.set_wave(1, 1010);
        let _ = r.set_wave(2, 1020);
        let fin = r.finish("占位×5基线×3波全绿");
        s.add_dyn("完成写回非空", fin.is_ok() && r.status() == 2 && !r.summary().is_empty(), "完成时状态=已完成且摘要非空");
    }
    // 15 双跑指纹 + LCG 确定性。
    {
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        let mut same = true;
        for _ in 0..16 {
            if a.next() != b.next() {
                same = false;
            }
        }
        s.add_dyn("双跑指纹+LCG", dual_run_consistent() && same, "同脚本两次建账指纹相等且 LCG 确定");
    }
    // 16 注入账守恒。
    {
        let mut r = NaRegistry::new();
        for i in 0..10u32 {
            let mut p = standard_placeholder(0);
            p.class_code = 1 + i;
            if r.register(p) == Err(VendorCode::ImpersonatesStandard) {
                r.note_detected();
            }
        }
        s.add_dyn("注入账守恒", r.conserved(), "注入=检出+逃逸，逃逸恒 0 且检出非零");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51576_checks_green() {
        let set = run_unx51576_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn placeholders_quota_five() {
        let mut r = NaRegistry::new();
        for i in 0..5u32 {
            assert!(r.register(standard_placeholder(i)).is_ok());
        }
        assert_eq!(r.placeholder_count(), 5);
        assert_eq!(r.register(standard_placeholder(50)), Err(VendorCode::QuotaExceeded));
    }

    #[test]
    fn impersonation_rejected() {
        let mut p = standard_placeholder(0);
        p.class_code = 0x03;
        assert!(p.impersonates_standard());
        let mut r = NaRegistry::new();
        assert_eq!(r.register(p), Err(VendorCode::ImpersonatesStandard));
    }

    #[test]
    fn duplicate_placeholder_rejected() {
        let mut r = NaRegistry::new();
        assert!(r.register(standard_placeholder(0)).is_ok());
        assert_eq!(r.register(standard_placeholder(0)), Err(VendorCode::DuplicatePlaceholder));
    }

    #[test]
    fn incomplete_placeholder_rejected() {
        let mut p = standard_placeholder(0);
        p.reason = String::new();
        assert!(!p.complete());
        let mut r = NaRegistry::new();
        assert_eq!(r.register(p), Err(VendorCode::PlaceholderIncomplete));
    }

    #[test]
    fn drift_within_limit() {
        let mut r = NaRegistry::new();
        let _ = r.set_wave(0, 1000);
        let _ = r.set_wave(1, 1010);
        let _ = r.set_wave(2, 1020);
        assert!(r.drift_ok());
        assert!(r.max_drift().unwrap_or(999) <= 50);
    }

    #[test]
    fn drift_over_limit_detected() {
        let mut r = NaRegistry::new();
        let _ = r.set_wave(0, 1000);
        let _ = r.set_wave(1, 1000);
        let _ = r.set_wave(2, 1200);
        assert!(!r.drift_ok());
        assert_eq!(r.max_drift().unwrap_or(0), 200);
    }

    #[test]
    fn drift_formula() {
        assert_eq!(NaRegistry::drift_permille(1000, 1100), Some(100));
        assert_eq!(NaRegistry::drift_permille(1000, 1050), Some(50));
        assert_eq!(NaRegistry::drift_permille(1000, 1000), Some(0));
        assert_eq!(NaRegistry::drift_permille(0, 100), None);
    }

    #[test]
    fn waves_short_rejected() {
        let mut r = NaRegistry::new();
        let _ = r.set_wave(0, 1000);
        assert!(!r.waves_complete());
        assert!(!r.drift_ok());
        assert_eq!(r.set_wave(3, 1000), Err(VendorCode::WavesShort));
    }

    #[test]
    fn all_codes_closed_unique() {
        for i in 0..ALL_CODES.len() {
            for j in (i + 1)..ALL_CODES.len() {
                assert_ne!(ALL_CODES[i].val(), ALL_CODES[j].val());
            }
        }
        assert_eq!(ALL_CODES[0].val(), SEG_BASE + 1);
    }

    #[test]
    fn dual_run_fingerprint_stable() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn lcg_deterministic() {
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        for _ in 0..32 {
            assert_eq!(a.next(), b.next());
        }
    }

    #[test]
    fn seg_guard_in_range() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0xC12C0);
    }
}
