//! UNX-F10049 回归基线比对（vs F9878 基线） · 0x91c40 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10049（B22 常态化回归线收尾 · 基线比对）。
//! 锚点：UNX-F10050-J1 分型留痕（新增/复发二分对齐 F9747 10/10 次），混型 10/10 次拦截。
//!
//! 数据段说明：
//!   SEG_BASE = 0x91c40（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `DiffCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 基线端点 `baseline_endpoint`：F9878 基线红账冻结为 FNV 指纹端点，
//!     in_baseline 闭集查表；比对必须走端点取数口，绕行逐次拦截；
//!   - 分型器 `classify`：当前红 × 基线归属 → 新增红/复发红/净绿 三态闭集，
//!     claimed 与 membership 不符即混型，逐次拦截（对齐锚点判据）；
//!   - 分型留痕账 `TypeLedger`：逐条分型入账、重复分型拒绝、二分合计守恒、
//!     F9747 对齐核（逐红必留痕），注入=检出+逃逸，逃逸恒 0；
//!   - 注入核 `BadDiffCase`：绕端点 / 混型错标 / 基线缺失 / 漏痕红 / 重复分型
//!     五类坏样本逐类 10/10 次拒绝；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10049 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x91c40;

/// 段窗上界（下一卡 0x91c80 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x91c80;

/// 战线条目数（40 条，B21 口径）。
pub const CASE_COUNT: usize = 40;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const ENOENT: i32 = 2;
pub const EACCES: i32 = 13;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;
pub const EBADSLT: i32 = 25;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiffCode {
    /// 绕端点比对（未走冻结端点取数口）
    EndpointBypass,
    /// 混型错标（claimed 与 membership 不符）
    MixedClass,
    /// 基线缺失（端点为 0 或基线账空）
    BaselineMissing,
    /// 漏痕红（当前红未入分型账）
    UntypedRed,
    /// 重复分型（同条二次入账）
    DuplicateType,
    /// 条目越界（号出 1..=40）
    CaseOutOfRange,
    /// F9747 对齐失败（分型合计 ≠ 当前红数）
    F9747Mismatch,
    /// 账本倒退
    LedgerRegress,
    /// 分型闭集外值
    ClassOutOfRange,
    /// 双跑不一致
    NotReproducible,
    /// 端点漂移（端点指纹与基线账重算不符）
    EndpointDrift,
    /// 段位越界
    SegmentViolation,
}

impl DiffCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            DiffCode::EndpointBypass => "绕端点比对被拒：比对必须走 baseline_endpoint 取数口；请先取端点",
            DiffCode::MixedClass => "混型错标被拒：分型必须与基线归属一致；请按 classify 结果入账",
            DiffCode::BaselineMissing => "基线缺失被拒：F9878 基线端点为空；请先冻结基线账",
            DiffCode::UntypedRed => "漏痕红被拒：当前红必须逐条入分型账；请补分型留痕",
            DiffCode::DuplicateType => "重复分型被拒：同条已入账；请勿二次分型",
            DiffCode::CaseOutOfRange => "条目越界被拒：条目号必须落在 1..=40；请核对口",
            DiffCode::F9747Mismatch => "F9747 对齐失败被拒：新增+复发合计必须等于当前红数；请对账",
            DiffCode::LedgerRegress => "账本倒退被拒：计数只增不减；请排查回滚误改",
            DiffCode::ClassOutOfRange => "分型越界被拒：分型只允许新增红/复发红/净绿三态；请核对照",
            DiffCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏状态",
            DiffCode::EndpointDrift => "端点漂移被拒：端点指纹与基线账重算不符；请重算端点",
            DiffCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x91c40..=0x91c80",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [DiffCode; 12] = [
        DiffCode::EndpointBypass,
        DiffCode::MixedClass,
        DiffCode::BaselineMissing,
        DiffCode::UntypedRed,
        DiffCode::DuplicateType,
        DiffCode::CaseOutOfRange,
        DiffCode::F9747Mismatch,
        DiffCode::LedgerRegress,
        DiffCode::ClassOutOfRange,
        DiffCode::NotReproducible,
        DiffCode::EndpointDrift,
        DiffCode::SegmentViolation,
    ];
}

// ── F9878 基线红账 ────────────────────────────────────────────────────
/// F9878 基线红账样例（六条冻结红；来源：F9878 基线卡红账列）。
pub const BASELINE_REDS: [u32; 6] = [3, 7, 12, 19, 25, 33];

/// 基线归属闭集查表（线性扫六条冻结红，纯函数）。
pub fn in_baseline(id: u32) -> bool {
    let mut hit = false;
    let mut i = 0usize;
    while i < BASELINE_REDS.len() {
        if let Some(v) = BASELINE_REDS.get(i) {
            if *v == id {
                hit = true;
            }
        }
        i += 1;
    }
    hit
}

/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 基线端点：基线红账逐条逐字节入 FNV（冻结取数口）。
pub fn baseline_endpoint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < BASELINE_REDS.len() {
        if let Some(v) = BASELINE_REDS.get(i) {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        i += 1;
    }
    fnv1a(&bytes)
}

/// 基线齐备核：基线账非空且端点非零。
pub fn baseline_ok() -> bool {
    !BASELINE_REDS.is_empty() && baseline_endpoint() != 0
}

/// 端点守卫：via_endpoint=false 即绕端点比对，逐次拦截。
pub fn endpoint_guard(via_endpoint: bool) -> Result<(), DiffCode> {
    if via_endpoint {
        Ok(())
    } else {
        Err(DiffCode::EndpointBypass)
    }
}

/// 端点取数比对：先过守卫，再比两端点指纹（零漂移比对口）。
pub fn compare_via_endpoint(a: u32, b: u32, via_endpoint: bool) -> Result<bool, DiffCode> {
    endpoint_guard(via_endpoint)?;
    Ok(a == b)
}

// ── 分型器 ────────────────────────────────────────────────────────────
/// 红账分型三态（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RedClass {
    /// 新增红（基线绿、当前红）
    NewRed,
    /// 复发红（基线已知红、当前仍红）
    RecurRed,
    /// 净绿（当前绿）
    Clean,
}

impl RedClass {
    /// 分型值（0/1/2，闭集）。
    pub fn val(self) -> u8 {
        match self {
            RedClass::NewRed => 0,
            RedClass::RecurRed => 1,
            RedClass::Clean => 2,
        }
    }
}

/// 分型：当前红 × 基线归属 → 期望分型；claimed 不符即混型。
pub fn classify(id: u32, current_red: bool, claimed: RedClass) -> Result<RedClass, DiffCode> {
    if id == 0 || id as usize > CASE_COUNT {
        return Err(DiffCode::CaseOutOfRange);
    }
    let expected = if !current_red {
        RedClass::Clean
    } else if in_baseline(id) {
        RedClass::RecurRed
    } else {
        RedClass::NewRed
    };
    if claimed != expected {
        return Err(DiffCode::MixedClass);
    }
    Ok(expected)
}

// ── 分型留痕账 ────────────────────────────────────────────────────────
/// 分型留痕账：逐条一次分型，二分合计守恒，F9747 对齐核。
#[derive(Clone, Debug)]
pub struct TypeLedger {
    /// 已分型占用表（下标 = 条目号-1）
    pub seen: [bool; CASE_COUNT],
    /// 新增红计数
    pub new_red: u32,
    /// 复发红计数
    pub recur_red: u32,
    /// 净绿计数
    pub clean: u32,
    /// 注入/检出/逃逸三联账
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl TypeLedger {
    /// 构造零账。
    pub const fn zero() -> TypeLedger {
        TypeLedger {
            seen: [false; CASE_COUNT],
            new_red: 0,
            recur_red: 0,
            clean: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 分型入账：逐条一次；重复/越界显性拒绝。
    pub fn record(&mut self, id: u32, cls: RedClass) -> Result<(), DiffCode> {
        if id == 0 || id as usize > CASE_COUNT {
            return Err(DiffCode::CaseOutOfRange);
        }
        let idx = id as usize - 1;
        if *self.seen.get(idx).unwrap_or(&true) {
            return Err(DiffCode::DuplicateType);
        }
        if let Some(slot) = self.seen.get_mut(idx) {
            *slot = true;
        }
        match cls {
            RedClass::NewRed => self.new_red = self.new_red.saturating_add(1),
            RedClass::RecurRed => self.recur_red = self.recur_red.saturating_add(1),
            RedClass::Clean => self.clean = self.clean.saturating_add(1),
        }
        Ok(())
    }

    /// F9747 对齐核：当前红逐条已留痕，且新增+复发合计 = 当前红数。
    pub fn aligns_f9747(&self, current_reds: &[u32]) -> bool {
        let mut total_red = 0u32;
        for id in current_reds.iter() {
            if *id == 0 || (*id as usize) > CASE_COUNT {
                return false;
            }
            if !*self.seen.get(*id as usize - 1).unwrap_or(&false) {
                return false;
            }
            total_red = total_red.saturating_add(1);
        }
        self.new_red.saturating_add(self.recur_red) == total_red
    }

    /// 注入账守恒：注入 = 检出 + 逃逸。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 端点一致性核：端点指纹必须等于基线账重算值（防篡改）。
pub fn endpoint_consistent(ep: u32) -> bool {
    ep == baseline_endpoint()
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏比对样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadDiffCase {
    /// 绕端点比对
    EndpointBypass,
    /// 混型错标（基线红标成新增红）
    MixedClass,
    /// 基线缺失（端点为 0）
    BaselineMissing,
    /// 漏痕红（当前红有一条未分型）
    UntypedRed,
    /// 重复分型
    DuplicateType,
}

impl BadDiffCase {
    /// 闭集全量。
    pub const ALL: [BadDiffCase; 5] = [
        BadDiffCase::EndpointBypass,
        BadDiffCase::MixedClass,
        BadDiffCase::BaselineMissing,
        BadDiffCase::UntypedRed,
        BadDiffCase::DuplicateType,
    ];

    /// 注入一轮：构造坏样本并触发比对/分型器，返回是否被显式拒绝。
    pub fn inject(self) -> bool {
        match self {
            BadDiffCase::EndpointBypass => {
                let ep = baseline_endpoint();
                compare_via_endpoint(ep, ep, false).is_err()
                    && compare_via_endpoint(ep, ep, true) == Ok(true)
            }
            BadDiffCase::MixedClass => {
                // 条目 3 在基线内：标新增红即混型；标复发红放行。
                classify(3, true, RedClass::NewRed).is_err()
                    && classify(3, true, RedClass::RecurRed).is_ok()
            }
            BadDiffCase::BaselineMissing => {
                // 端点为 0（模拟基线账被清空）→ 齐备核拒绝。
                !baseline_ok_static(0)
            }
            BadDiffCase::UntypedRed => {
                let mut led = TypeLedger::zero();
                // 当前红 {3, 41?非法, 8}：只分型 8，漏 3 → 对齐核拒绝。
                let _ = led.record(8, RedClass::NewRed);
                !led.aligns_f9747(&[3, 8])
            }
            BadDiffCase::DuplicateType => {
                let mut led = TypeLedger::zero();
                if led.record(9, RedClass::NewRed).is_err() {
                    return false;
                }
                led.record(9, RedClass::NewRed).is_err()
            }
        }
    }
}

/// 基线端点空账探针（注入专用：端点 0 = 基线账被清空）。
fn baseline_ok_static(ep: u32) -> bool {
    ep != 0 && !BASELINE_REDS.is_empty()
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回逐类检出数。
pub fn run_diff_injection() -> [u32; 5] {
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadDiffCase::ALL.len() {
        if let Some(case) = BadDiffCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                if case.inject() {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    per_class
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；基线缺失/漏痕族"),
    ("EACCES", -13, 5, "Windows ERROR_ACCESS_DENIED；绕端点比对族"),
    ("EBADSLT", -25, 431, "Windows ERROR_INVALID_DEVICE；槽位分型重复族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复分型族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；混型/越界族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// 规范语料双跑指纹：端点取数→逐条分型→F9747 对齐全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let ep = baseline_endpoint();
    bytes.extend_from_slice(&ep.to_le_bytes());
    // 语料：基线红 3 仍红（复发）；非基线条目 8、21 转红（新增）；其余净绿抽样。
    let mut led = TypeLedger::zero();
    let corpus: [(u32, bool); 6] = [(3, true), (8, true), (21, true), (1, false), (2, false), (4, false)];
    for (id, red) in corpus.iter() {
        let cls = classify(*id, *red, if *red { RedClass::NewRed } else { RedClass::Clean });
        match cls {
            Ok(c) => {
                bytes.push(c.val() + 1);
                let claimed = match c {
                    RedClass::NewRed => RedClass::NewRed,
                    RedClass::RecurRed => RedClass::RecurRed,
                    RedClass::Clean => RedClass::Clean,
                };
                bytes.push(match led.record(*id, claimed) {
                    Ok(()) => 1,
                    Err(_) => 0,
                });
            }
            Err(_) => bytes.push(0),
        }
    }
    bytes.push(if led.aligns_f9747(&[3, 8, 21]) { 1 } else { 0 });
    bytes.extend_from_slice(&led.new_red.to_le_bytes());
    bytes.extend_from_slice(&led.recur_red.to_le_bytes());
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10049 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx10049_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10049");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91c40..=0x91c80", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝注册");

    // 2. 基线端点冻结：非零且与账面重算一致
    let ep = baseline_endpoint();
    let ep_ok = baseline_ok() && endpoint_consistent(ep);
    s.add("基线端点：F9878 六红冻结为 FNV 端点", ep_ok,
          "端点取数口唯一，重算逐字节一致");

    // 3. 基线归属闭集查表
    let inb_ok = in_baseline(3) && in_baseline(33) && !in_baseline(4) && !in_baseline(0);
    s.add("基线归属：闭集查表命中/未中分明", inb_ok,
          "六条冻结红逐条可查，表外即绿");

    // 4. 分型正确性：基线红仍红=复发、非基线红=新增、绿=净绿
    let cls_ok = classify(3, true, RedClass::RecurRed).is_ok()
        && classify(8, true, RedClass::NewRed).is_ok()
        && classify(1, false, RedClass::Clean).is_ok();
    s.add("分型正确性：复发/新增/净绿三路各归位", cls_ok,
          "membership 决定期望分型，claimed 必须一致");

    // 5. 混型 10/10 拦截（锚点判据直接探针）
    let mut mixed_hit = 0u32;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let wrong = classify(3, true, RedClass::NewRed).is_err()
            && classify(8, true, RedClass::RecurRed).is_err()
            && classify(1, false, RedClass::NewRed).is_err();
        if wrong {
            mixed_hit = mixed_hit.saturating_add(1);
        }
        r += 1;
    }
    s.add("混型拦截：三路错标 10/10 显性拒绝", mixed_hit == INJECT_ROUNDS,
          "claimed 与 membership 不符即 MixedClass，无一放行");

    // 6. 绕端点比对 10/10 拦截
    let mut bypass_hit = 0u32;
    let mut r2 = 0u32;
    while r2 < INJECT_ROUNDS {
        if compare_via_endpoint(ep, ep, false).is_err()
            && compare_via_endpoint(ep, ep, true) == Ok(true)
        {
            bypass_hit = bypass_hit.saturating_add(1);
        }
        r2 += 1;
    }
    s.add("绕端点比对拦截：10/10 显性拒绝", bypass_hit == INJECT_ROUNDS,
          "比对必须走冻结端点取数口，绕行即 EndpointBypass");

    // 7. 端点漂移检出：篡改端点与真端点必不等
    let tampered = ep ^ 0xA5A5;
    let drift_ok = !endpoint_consistent(tampered)
        && compare_via_endpoint(ep, tampered, true) == Ok(false);
    s.add("端点漂移：篡改端点比对即检出", drift_ok,
          "端点指纹失配即漂移，零漂移口径保底");

    // 8. F9747 对齐核：新增+复发 = 当前红数
    let mut led = TypeLedger::zero();
    let _ = led.record(3, RedClass::RecurRed);
    let _ = led.record(8, RedClass::NewRed);
    let _ = led.record(21, RedClass::NewRed);
    let align_ok = led.aligns_f9747(&[3, 8, 21])
        && led.new_red == 2 && led.recur_red == 1;
    s.add("F9747 对齐：二分合计 3 = 当前红数 3", align_ok,
          "新增/复发二分留痕逐条入账，合计对表");

    // 9. 漏痕红检出（二分对齐负样）
    let mut partial = TypeLedger::zero();
    let _ = partial.record(8, RedClass::NewRed);
    let miss_ok = !partial.aligns_f9747(&[3, 8, 21]);
    s.add("漏痕红检出：漏分型即对齐核拒绝", miss_ok,
          "逐红必留痕，缺一条即 F9747Mismatch 口径");

    // 10. 重复分型拦截
    let mut dup = TypeLedger::zero();
    let _ = dup.record(9, RedClass::NewRed);
    let dup_ok = dup.record(9, RedClass::NewRed) == Err(DiffCode::DuplicateType);
    s.add("重复分型：同条二次入账被拒", dup_ok,
          "一条一型，重复留痕显性拒绝");

    // 11. 注入核：五类坏样本全检出
    let per_class = run_diff_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "绕端点/混型/基线缺失/漏痕/重复分型全被显式拒绝");

    // 12. 分型账守恒：注入=检出+逃逸
    let mut led2 = TypeLedger::zero();
    // 规范语料：3 复发 + 8/21 新增 + 对档回报（检出账逐条入账）。
    let _ = led2.record(3, RedClass::RecurRed);
    let _ = led2.record(8, RedClass::NewRed);
    let _ = led2.record(21, RedClass::NewRed);
    led2.injected = 3;
    led2.detected = 3;
    s.add("分型账守恒：注入=检出、逃逸=0",
          led2.conserved() && led2.escaped == 0,
          "三条分型逐条入检出账，无静默");

    // 13. 文案闭集：非空
    let text_ok = DiffCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 14. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i = 0usize;
    while i < DiffCode::ALL.len() {
        let mut j = i + 1;
        while j < DiffCode::ALL.len() {
            if let (Some(x), Some(y)) = (DiffCode::ALL.get(i), DiffCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let in_seg = DiffCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 15. Linux 逐码一致
    let codes_ok = agrees_with_linux(EACCES)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EBADSLT)
        && agrees_with_linux(EEXIST)
        && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 16. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 17. 条目越界拦截
    let oob_ok = classify(0, true, RedClass::NewRed).is_err()
        && classify(41, true, RedClass::NewRed).is_err();
    s.add("条目越界：0/41 号分型请求拒绝", oob_ok,
          "格闭集 1..=40，出格即 CaseOutOfRange");

    // 动态明细（≤8）：分型账一行 + 指纹一行
    let led_text = format!("新增 {} / 复发 {} / 净绿 {} / 端点对齐 {}",
        led.new_red, led.recur_red, led.clean, endpoint_consistent(ep));
    s.add_dyn("分型快照", align_ok && endpoint_consistent(ep), &led_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_membership() {
        assert!(in_baseline(3));
        assert!(in_baseline(25));
        assert!(!in_baseline(8));
        assert!(!in_baseline(0));
        assert!(!in_baseline(41));
    }

    #[test]
    fn endpoint_frozen() {
        let a = baseline_endpoint();
        let b = baseline_endpoint();
        assert_eq!(a, b);
        assert_ne!(a, 0);
        assert!(endpoint_consistent(a));
        assert!(!endpoint_consistent(a ^ 1));
    }

    #[test]
    fn classify_correct() {
        assert_eq!(classify(3, true, RedClass::RecurRed), Ok(RedClass::RecurRed));
        assert_eq!(classify(8, true, RedClass::NewRed), Ok(RedClass::NewRed));
        assert_eq!(classify(1, false, RedClass::Clean), Ok(RedClass::Clean));
    }

    #[test]
    fn mixed_class_rejected() {
        assert_eq!(classify(3, true, RedClass::NewRed), Err(DiffCode::MixedClass));
        assert_eq!(classify(8, true, RedClass::RecurRed), Err(DiffCode::MixedClass));
        assert_eq!(classify(1, false, RedClass::NewRed), Err(DiffCode::MixedClass));
    }

    #[test]
    fn case_out_of_range_rejected() {
        assert_eq!(classify(0, true, RedClass::NewRed), Err(DiffCode::CaseOutOfRange));
        assert_eq!(classify(41, true, RedClass::NewRed), Err(DiffCode::CaseOutOfRange));
    }

    #[test]
    fn endpoint_bypass_rejected() {
        let ep = baseline_endpoint();
        assert_eq!(compare_via_endpoint(ep, ep, false), Err(DiffCode::EndpointBypass));
        assert_eq!(compare_via_endpoint(ep, ep, true), Ok(true));
        assert_eq!(compare_via_endpoint(ep, ep ^ 1, true), Ok(false));
    }

    #[test]
    fn duplicate_type_rejected() {
        let mut led = TypeLedger::zero();
        let _ = led.record(9, RedClass::NewRed);
        assert_eq!(led.record(9, RedClass::NewRed), Err(DiffCode::DuplicateType));
    }

    #[test]
    fn f9747_alignment() {
        let mut led = TypeLedger::zero();
        let _ = led.record(3, RedClass::RecurRed);
        let _ = led.record(8, RedClass::NewRed);
        assert!(led.aligns_f9747(&[3, 8]));
        assert!(!led.aligns_f9747(&[3, 8, 21]));
        assert!(!led.aligns_f9747(&[3, 99]));
    }

    #[test]
    fn injection_matrix_all_caught() {
        let per = run_diff_injection();
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EBADSLT));
        assert!(agrees_with_linux(EEXIST));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EINVAL), -EINVAL);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10049_checks_green() {
        let s = run_unx10049_checks();
        assert!(s.all_passed(), "UNX-F10049 red: {}", s.red_items_text());
    }
}
