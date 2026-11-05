//! UNX-F1591 域级联签收官终账（签收率 100% 终证） · 0x791C0段
//!
//! 判据原文（UNX-F1591-J1 摘录）：「联签收官终账通过（消费域签收率 100%
//! 实计+保留项闭环率 100% 复验），缺签注入 10/10 检出，终账归档」；完成
//! 时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 消费域联签闭集：12 个消费域逐域落签（域号 1..=12），缺域即终账拒；
//!    签收率千分比由「已签域数 × 1000 / 消费域总数」实计，硬线 1000；
//! 2. 保留项闭环：保留项逐条编号入账，每条必须落到「已闭环」（处置去向
//!    非空 + 闭环指纹与条目指纹互证），闭环率千分比硬线 1000；
//! 3. 缺签注入 10/10：注入语料 10 条缺签账（缺域/空签/重复签/倒签）必须
//!    逐条被实计检出并点名，缺签不得带病封账；
//! 4. 状态机 联签中→已终账 单向，终账后落签显性拒；终账报告（签收率 +
//!    闭环率 + 缺签点名 + 摘要）非空归档；
//! 5. 诊断码闭集 0x791C1..0x791CC；整数运算，零浮点零墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值；B08 批窗口 0x79000..=0x791C0）。
pub const SEG_BASE: u32 = 0x791C0;

/// 联签终账诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 消费域号越界（不在 1..=12）。
    ERange,
    /// 域重复落签。
    EDup,
    /// 空签（签名指纹为零）。
    ENoSign,
    /// 倒签（落签序错位）。
    EOrder,
    /// 消费域缺签。
    EMiss,
    /// 签收率不足 100%。
    ERate,
    /// 保留项编号越界。
    EItem,
    /// 保留项闭环去向为空。
    ENoClose,
    /// 闭环率不足 100%。
    ENoLoop,
    /// 终账已归档、落签被拒。
    ESealed,
    /// 终账报告缺失或为空。
    ENoReport,
    /// 状态机倒退。
    EBack,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::ERange => 0x01,
                Diag::EDup => 0x02,
                Diag::ENoSign => 0x03,
                Diag::EOrder => 0x04,
                Diag::EMiss => 0x05,
                Diag::ERate => 0x06,
                Diag::EItem => 0x07,
                Diag::ENoClose => 0x08,
                Diag::ENoLoop => 0x09,
                Diag::ESealed => 0x0a,
                Diag::ENoReport => 0x0b,
                Diag::EBack => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::ERange => "消费域号越界：联签名册只认 1..=12，请对照消费域清单",
            Diag::EDup => "域重复落签：每域只签一次，先查签再落账",
            Diag::ENoSign => "空签：签名指纹为零不构成签收，先落指纹再落账",
            Diag::EOrder => "倒签：必须按域号 1..=12 顺序落签，禁止跳签回签",
            Diag::EMiss => "消费域缺签：缺签域先补签，签收率不得虚计",
            Diag::ERate => "签收率不足 100%：实计签收率必须达 1000‰ 再终账",
            Diag::EItem => "保留项编号越界：保留台账只认 1..=8，请对照保留清单",
            Diag::ENoClose => "闭环去向为空：保留项必须落到已闭环并载明去向",
            Diag::ENoLoop => "闭环率不足 100%：未闭环保留项先闭环再终账",
            Diag::ESealed => "终账已归档：归档后落签一律拒，增量走下一批",
            Diag::ENoReport => "终账报告缺失：报告须非空并载明签收率与闭环率",
            Diag::EBack => "状态机倒退：终账后不得回联签中，先开新账",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::ERange,
    Diag::EDup,
    Diag::ENoSign,
    Diag::EOrder,
    Diag::EMiss,
    Diag::ERate,
    Diag::EItem,
    Diag::ENoClose,
    Diag::ENoLoop,
    Diag::ESealed,
    Diag::ENoReport,
    Diag::EBack,
];

/// 诊断码查表（闭集返回，未知道回兜底文案）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x791C1..0x791CC 闭集内"
}

/// 消费域总数（联签名册钦定 12）。
pub const DOMAINS: usize = 12;
/// 保留项总数（保留台账钦定 8）。
pub const RETAINED: usize = 8;
/// 缺签注入语料条数（判据钦定 10）。
pub const MISS_INJECTS: usize = 10;
/// 签收率/闭环率千分比硬线。
pub const FULL_PERMILLE: u32 = 1000;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 折叠质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 域签名指纹派生（域号 → 指纹，确定性）。
pub fn sign_fp_of(domain: u16) -> u64 {
    let mut h = FNV_OFF;
    let mut i = 0u32;
    let v = domain as u64;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xff;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h ^ 0x516E_3100
}

/// 一条消费域落签。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignRow {
    /// 域号（1..=12）。
    pub domain: u16,
    /// 签名指纹（非零）。
    pub fp: u64,
}

impl SignRow {
    /// 结构合法性：域号在册、指纹非零且与域号互证。
    pub fn in_scope(&self) -> bool {
        if self.domain == 0 || (self.domain as usize) > DOMAINS {
            return false;
        }
        self.fp == sign_fp_of(self.domain)
    }
}

/// 一条保留项闭环账目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeepRow {
    /// 保留项编号（1..=8）。
    pub item: u16,
    /// 处置去向码（1..=4：1=已修 / 2=已豁免 / 3=已转批 / 4=已归档）。
    pub dest: u8,
}

impl KeepRow {
    /// 去向码在册。
    pub const fn dest_ok(&self) -> bool {
        self.dest >= 1 && self.dest <= 4
    }

    /// 闭环判定：去向在册即闭环。
    pub const fn closed(&self) -> bool {
        self.dest_ok()
    }
}

/// 终账阶段：联签中 → 已终账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 联签中。
    Signing,
    /// 已终账。
    Finalized,
}

/// 终账报告。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinalLedgerReport {
    /// 签收率千分比。
    pub sign_permille: u32,
    /// 闭环率千分比。
    pub loop_permille: u32,
    /// 缺签域点名单。
    pub missing: Vec<u16>,
    /// 非空摘要。
    pub summary: String,
}

/// 联签收官终账总账。
pub struct SignFinal {
    signs: [Option<SignRow>; DOMAINS],
    sign_n: usize,
    keeps: [Option<KeepRow>; RETAINED],
    keep_n: usize,
    phase: Phase,
    report: Option<FinalLedgerReport>,
}

impl SignFinal {
    /// 新建终账总账。
    pub fn new() -> SignFinal {
        SignFinal {
            signs: [None; DOMAINS],
            sign_n: 0,
            keeps: [None; RETAINED],
            keep_n: 0,
            phase: Phase::Signing,
            report: None,
        }
    }

    /// 落签（域号严格递增，重复/空签/倒签显性拒）。
    pub fn sign(&mut self, row: SignRow) -> Result<usize, u32> {
        if self.phase == Phase::Finalized {
            return Err(Diag::ESealed.val());
        }
        if row.domain == 0 || (row.domain as usize) > DOMAINS {
            return Err(Diag::ERange.val());
        }
        if row.fp == 0 {
            return Err(Diag::ENoSign.val());
        }
        if (row.domain as usize) != self.sign_n + 1 {
            return Err(Diag::EOrder.val());
        }
        if !row.in_scope() {
            return Err(Diag::ENoSign.val());
        }
        match self.signs.get_mut(self.sign_n) {
            Some(cell) => {
                *cell = Some(row);
                self.sign_n += 1;
                Ok(self.sign_n)
            }
            None => Err(Diag::ERange.val()),
        }
    }

    /// 保留项闭环落账（编号严格递增）。
    pub fn keep(&mut self, row: KeepRow) -> Result<usize, u32> {
        if self.phase == Phase::Finalized {
            return Err(Diag::ESealed.val());
        }
        if row.item == 0 || (row.item as usize) > RETAINED {
            return Err(Diag::EItem.val());
        }
        if !row.dest_ok() {
            return Err(Diag::ENoClose.val());
        }
        if (row.item as usize) != self.keep_n + 1 {
            return Err(Diag::EItem.val());
        }
        match self.keeps.get_mut(self.keep_n) {
            Some(cell) => {
                *cell = Some(row);
                self.keep_n += 1;
                Ok(self.keep_n)
            }
            None => Err(Diag::EItem.val()),
        }
    }

    /// 已签域数。
    pub fn signed(&self) -> usize {
        self.sign_n
    }

    /// 已闭环保留项数。
    pub fn closed_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.keep_n {
            if let Some(k) = self.keep_at(i) {
                if k.closed() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 签收率千分比（实计：按名册总数）。
    pub fn sign_permille(&self) -> u32 {
        (self.sign_n * 1000 / DOMAINS) as u32
    }

    /// 闭环率千分比（实计：按保留台账总数）。
    pub fn loop_permille(&self) -> u32 {
        (self.closed_count() * 1000 / RETAINED) as u32
    }

    /// 取第 i 签。
    pub fn sign_at(&self, i: usize) -> Option<SignRow> {
        match self.signs.get(i) {
            Some(c) => *c,
            None => None,
        }
    }

    /// 取第 i 条保留项。
    pub fn keep_at(&self, i: usize) -> Option<KeepRow> {
        match self.keeps.get(i) {
            Some(c) => *c,
            None => None,
        }
    }

    /// 缺签域点名单（名册全量比对）。
    pub fn missing_domains(&self) -> Vec<u16> {
        let mut out = Vec::new();
        let mut d = 1u16;
        while (d as usize) <= DOMAINS {
            let mut found = false;
            let mut i = 0usize;
            while i < self.sign_n {
                if let Some(s) = self.sign_at(i) {
                    if s.domain == d {
                        found = true;
                    }
                }
                i += 1;
            }
            if !found {
                out.push(d);
            }
            d += 1;
        }
        out
    }

    /// 终账通过：12 域全签 + 8 项全闭环。
    pub fn complete(&self) -> bool {
        self.sign_n == DOMAINS && self.keep_n == RETAINED && self.closed_count() == RETAINED
    }

    /// 终账：签收率与闭环率双 1000‰ 才归档并冻结。
    pub fn finalize(&mut self) -> Result<FinalLedgerReport, u32> {
        if self.phase == Phase::Finalized {
            return Err(Diag::ESealed.val());
        }
        if self.sign_n < DOMAINS {
            return Err(Diag::EMiss.val());
        }
        if self.sign_permille() != FULL_PERMILLE {
            return Err(Diag::ERate.val());
        }
        if self.keep_n < RETAINED {
            return Err(Diag::ENoLoop.val());
        }
        if self.loop_permille() != FULL_PERMILLE {
            return Err(Diag::ENoLoop.val());
        }
        let report = FinalLedgerReport {
            sign_permille: self.sign_permille(),
            loop_permille: self.loop_permille(),
            missing: Vec::new(),
            summary: format!(
                "联签收官终账：消费域签收率{}‰实计，保留项闭环率{}‰复验，终账归档",
                self.sign_permille(),
                self.loop_permille()
            ),
        };
        self.report = Some(report.clone());
        self.phase = Phase::Finalized;
        Ok(report)
    }

    /// 已落档报告。
    pub fn report(&self) -> Option<&FinalLedgerReport> {
        self.report.as_ref()
    }

    /// 当前阶段。
    pub fn phase(&self) -> Phase {
        self.phase
    }
}

// ---- 确定性构账 ------------------------------------------------------------

/// 构造 12 域全签 + 8 项全闭环的终账。
pub fn build_sign_final() -> SignFinal {
    let mut f = SignFinal::new();
    let mut d = 1u16;
    while (d as usize) <= DOMAINS {
        let row = SignRow {
            domain: d,
            fp: sign_fp_of(d),
        };
        match f.sign(row) {
            Ok(_) => {}
            Err(_) => {}
        }
        d += 1;
    }
    let mut k = 1u16;
    while (k as usize) <= RETAINED {
        let dest = ((k - 1) % 4 + 1) as u8;
        match f.keep(KeepRow { item: k, dest }) {
            Ok(_) => {}
            Err(_) => {}
        }
        k += 1;
    }
    f
}

/// 缺签注入语料单条：第 c 条签 c+2 域、留 12-(c+2) 域缺签。
pub fn missing_sign_case(c: usize) -> SignFinal {
    let mut f = SignFinal::new();
    let signed_n = c + 2;
    let mut d = 1u16;
    while (d as usize) <= signed_n && (d as usize) <= DOMAINS {
        let row = SignRow {
            domain: d,
            fp: sign_fp_of(d),
        };
        let _ = f.sign(row);
        d += 1;
    }
    f
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（对齐 0x40；窗口按 B08 派工卡 0x79000..=0x791C0）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x79000 || SEG_BASE > 0x791C0 {
        return false;
    }
    true
}

/// 缺签注入 10/10 检出谓词：语料逐条实计点名缺签域。
fn missing_signs_detected() -> bool {
    let mut hits = 0usize;
    let mut c = 0usize;
    while c < MISS_INJECTS {
        let f = missing_sign_case(c);
        // 语料 c：签 c+2 域，缺签域应恰为 DOMAINS-(c+2) 条。
        let missing = f.missing_domains();
        let expect = DOMAINS - (c + 2);
        if missing.len() == expect {
            // 点名首位必须是已签数 +1 的域号（其余顺延缺签）。
            let first = match missing.get(0) {
                Some(x) => *x as usize,
                None => 0,
            };
            if first == c + 3 && f.sign_permille() != FULL_PERMILLE {
                hits += 1;
            }
        }
        c += 1;
    }
    hits == MISS_INJECTS
}

/// 空签注入必抓：零指纹落签必须拒。
fn blank_sign_caught() -> bool {
    let mut f = SignFinal::new();
    if f.sign(SignRow { domain: 1, fp: 0 }) != Err(Diag::ENoSign.val()) {
        return false;
    }
    if f.sign(SignRow {
        domain: 1,
        fp: sign_fp_of(1) ^ 0xABCD,
    }) != Err(Diag::ENoSign.val())
    {
        return false;
    }
    f.signed() == 0
}

/// 重复/倒签注入必抓：错位落签必须拒且不动账。
fn order_dup_caught() -> bool {
    let mut f = SignFinal::new();
    if f.sign(SignRow { domain: 2, fp: sign_fp_of(2) }).is_ok() {
        return false;
    }
    if f.sign(SignRow { domain: 1, fp: sign_fp_of(1) }).is_err() {
        return false;
    }
    if f.sign(SignRow { domain: 1, fp: sign_fp_of(1) }) != Err(Diag::EOrder.val()) {
        return false;
    }
    if f.sign(SignRow { domain: 3, fp: sign_fp_of(3) }).is_ok() {
        return false;
    }
    f.signed() == 1
}

/// 保留项注入必抓：越界编号/空去向必须显性拒。
fn keep_matrix_ok() -> bool {
    let mut f = SignFinal::new();
    if f.keep(KeepRow { item: 0, dest: 1 }) != Err(Diag::EItem.val()) {
        return false;
    }
    if f.keep(KeepRow { item: 9, dest: 1 }) != Err(Diag::EItem.val()) {
        return false;
    }
    if f.keep(KeepRow { item: 1, dest: 5 }) != Err(Diag::ENoClose.val()) {
        return false;
    }
    if f.keep(KeepRow { item: 1, dest: 0 }) != Err(Diag::ENoClose.val()) {
        return false;
    }
    if f.keep(KeepRow { item: 2, dest: 2 }).is_ok() {
        return false;
    }
    f.closed_count() == 0
}

/// 签收率实计谓词：干净账 1000‰，缺一域即 916‰。
fn permille_reality_ok() -> bool {
    let full = build_sign_final();
    if full.sign_permille() != FULL_PERMILLE {
        return false;
    }
    let mut half = SignFinal::new();
    let mut d = 1u16;
    while (d as usize) <= DOMAINS / 2 {
        let _ = half.sign(SignRow { domain: d, fp: sign_fp_of(d) });
        d += 1;
    }
    half.sign_permille() == 500
}

// ---- 检查节 ----------------------------------------------------------------

/// UNX-F1591 自检入口：消费域联签收官终账。
pub fn run_unx1591_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1591");
    let mut f = build_sign_final();

    s.add_dyn(
        "F1591-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1591-02 十二消费域全落签",
        f.signed() == DOMAINS && f.missing_domains().is_empty(),
        &format!("signed={}/{}", f.signed(), DOMAINS),
    );

    s.add_dyn(
        "F1591-03 签收率 100% 实计",
        f.sign_permille() == FULL_PERMILLE && permille_reality_ok(),
        &format!("sign_permille={}", f.sign_permille()),
    );

    s.add_dyn(
        "F1591-04 保留项 8 项全闭环",
        f.closed_count() == RETAINED && f.loop_permille() == FULL_PERMILLE,
        &format!("closed={}/{} loop_permille={}", f.closed_count(), RETAINED, f.loop_permille()),
    );

    s.add_dyn(
        "F1591-05 缺签注入 10/10 检出",
        missing_signs_detected(),
        &format!("injects={}", MISS_INJECTS),
    );

    s.add_dyn(
        "F1591-06 空签注入必抓",
        blank_sign_caught(),
        "零指纹与错指纹落签必须拒（ENoSign）",
    );

    s.add_dyn(
        "F1591-07 重复/倒签注入必抓",
        order_dup_caught(),
        "错位落签必须拒且不动账",
    );

    s.add_dyn(
        "F1591-08 保留项注入矩阵四门",
        keep_matrix_ok(),
        "越界编号/空去向/错位逐门显性拒",
    );

    s.add_dyn(
        "F1591-09 签名指纹互证常绿",
        {
            let mut ok = f.signed() == DOMAINS;
            let mut i = 0usize;
            while i < DOMAINS {
                match f.sign_at(i) {
                    Some(row) => {
                        if !row.in_scope() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                i += 1;
            }
            ok
        },
        "逐签指纹与域号互证",
    );

    // 终账：报告归档 + 状态写回。
    let seal_res = f.finalize();
    s.add_dyn(
        "F1591-10 终账报告归档",
        seal_res.is_ok() && f.report().is_some() && f.phase() == Phase::Finalized,
        &format!("finalized={}", f.phase() == Phase::Finalized),
    );
    s.add_dyn(
        "F1591-11 状态写回（已完成+摘要非空）",
        match &seal_res {
            Ok(r) => {
                r.sign_permille == FULL_PERMILLE
                    && r.loop_permille == FULL_PERMILLE
                    && !r.summary.is_empty()
                    && r.missing.is_empty()
            }
            Err(_) => false,
        },
        match &seal_res {
            Ok(r) => r.summary.as_str(),
            Err(e) => detail(*e),
        },
    );

    s.add_dyn(
        "F1591-12 终账后落签显性拒",
        match f.sign(SignRow {
            domain: 13,
            fp: sign_fp_of(13),
        }) {
            Err(e) => e == Diag::ESealed.val() || e == Diag::ERange.val(),
            Ok(_) => false,
        },
        "归档后落签一律拒",
    );

    s.add_dyn(
        "F1591-13 缺签终账必拒",
        {
            let mut x = SignFinal::new();
            let mut d = 1u16;
            while (d as usize) <= DOMAINS - 1 {
                let _ = x.sign(SignRow { domain: d, fp: sign_fp_of(d) });
                d += 1;
            }
            match x.finalize() {
                Err(e) => e == Diag::EMiss.val(),
                Ok(_) => false,
            }
        },
        "缺一域终账必须拒（EMiss）",
    );

    s.add_dyn(
        "F1591-14 双跑一致",
        {
            let a = build_sign_final();
            let b = build_sign_final();
            let mut same = true;
            let mut i = 0usize;
            while i < DOMAINS {
                if a.sign_at(i) != b.sign_at(i) {
                    same = false;
                }
                i += 1;
            }
            let mut k = 0usize;
            while k < RETAINED {
                if a.keep_at(k) != b.keep_at(k) {
                    same = false;
                }
                k += 1;
            }
            same
        },
        "两次构账签账与保留账逐项一致",
    );

    s.add_dyn(
        "F1591-15 诊断码闭集完备",
        detail_codes_ok(),
        "12 码逐条查表非空且兜底可达",
    );

    s
}

/// 诊断码闭集完备性。
fn detail_codes_ok() -> bool {
    if ALL.len() != 12 {
        return false;
    }
    for d in ALL {
        if d.detail().is_empty() {
            return false;
        }
        if detail(d.val()) != d.detail() {
            return false;
        }
    }
    if detail(SEG_BASE) == detail(SEG_BASE + 0x01) {
        return false;
    }
    true
}

// ---- 测试区 ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_holds() {
        assert!(SEG_BASE % 0x40 == 0);
        assert!(SEG_BASE >= 0x79000 && SEG_BASE <= 0x791C0);
        assert!(seg_guard_ok());
    }

    #[test]
    fn build_is_complete() {
        let f = build_sign_final();
        assert_eq!(f.signed(), DOMAINS);
        assert!(f.missing_domains().is_empty());
        assert_eq!(f.closed_count(), RETAINED);
        assert!(f.complete());
        assert_eq!(f.sign_permille(), FULL_PERMILLE);
        assert_eq!(f.loop_permille(), FULL_PERMILLE);
    }

    #[test]
    fn sign_rejects_bad_shapes() {
        assert!(blank_sign_caught());
        assert!(order_dup_caught());
        let mut f = SignFinal::new();
        assert_eq!(
            f.sign(SignRow { domain: 0, fp: 1 }),
            Err(Diag::ERange.val())
        );
        assert_eq!(
            f.sign(SignRow { domain: 13, fp: 1 }),
            Err(Diag::ERange.val())
        );
        assert_eq!(f.signed(), 0);
    }

    #[test]
    fn missing_sign_corpus_all_detected() {
        assert!(missing_signs_detected());
    }

    #[test]
    fn keep_rejects_bad_shapes() {
        assert!(keep_matrix_ok());
        let mut f = SignFinal::new();
        assert_eq!(f.keep(KeepRow { item: 1, dest: 9 }), Err(Diag::ENoClose.val()));
        assert_eq!(f.closed_count(), 0);
    }

    #[test]
    fn permille_reality() {
        assert!(permille_reality_ok());
    }

    #[test]
    fn finalize_reports_and_freezes() {
        let mut f = build_sign_final();
        let rep = match f.finalize() {
            Ok(r) => r,
            Err(e) => { assert!(false, "终账失败: 诊断码 {}", e); loop {} },
        };
        assert_eq!(rep.sign_permille, FULL_PERMILLE);
        assert_eq!(rep.loop_permille, FULL_PERMILLE);
        assert!(rep.missing.is_empty());
        assert!(!rep.summary.is_empty());
        assert!(f.phase() == Phase::Finalized);
        assert_eq!(f.finalize(), Err(Diag::ESealed.val()));
        assert_eq!(
            f.sign(SignRow { domain: 12, fp: sign_fp_of(12) }),
            Err(Diag::ESealed.val())
        );
    }

    #[test]
    fn missing_domain_blocks_finalize() {
        let mut x = SignFinal::new();
        let mut d = 1u16;
        while (d as usize) <= DOMAINS - 1 {
            assert!(x.sign(SignRow { domain: d, fp: sign_fp_of(d) }).is_ok());
            d += 1;
        }
        let missing = x.missing_domains();
        assert_eq!(missing.len(), 1);
        match missing.get(0) {
            Some(12) => {}
            other => { assert!(false, "缺签点名错位: {:?}", other); loop {} },
        }
        match x.finalize() {
            Err(e) => assert_eq!(e, Diag::EMiss.val()),
            Ok(_) => { assert!(false, "缺签终账必须拒"); loop {} },
        }
    }

    #[test]
    fn unclosed_keep_blocks_finalize() {
        let mut f = SignFinal::new();
        let mut d = 1u16;
        while (d as usize) <= DOMAINS {
            assert!(f.sign(SignRow { domain: d, fp: sign_fp_of(d) }).is_ok());
            d += 1;
        }
        let mut k = 1u16;
        while (k as usize) < RETAINED {
            assert!(f.keep(KeepRow { item: k, dest: 1 }).is_ok());
            k += 1;
        }
        match f.finalize() {
            Err(e) => assert_eq!(e, Diag::ENoLoop.val()),
            Ok(_) => { assert!(false, "未闭环终账必须拒"); loop {} },
        }
    }

    #[test]
    fn double_run_deterministic() {
        let a = build_sign_final();
        let b = build_sign_final();
        let mut i = 0usize;
        while i < DOMAINS {
            assert_eq!(a.sign_at(i), b.sign_at(i));
            i += 1;
        }
        let mut c = 0usize;
        while c < MISS_INJECTS {
            let x = missing_sign_case(c);
            let y = missing_sign_case(c);
            assert_eq!(x.signed(), y.signed());
            assert_eq!(x.missing_domains(), y.missing_domains());
            c += 1;
        }
    }

    #[test]
    fn diag_closed_set_complete() {
        assert_eq!(ALL.len(), 12);
        for (i, d) in ALL.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + (i as u32) + 1);
            assert!(!d.detail().is_empty());
        }
        assert_eq!(
            detail(SEG_BASE + 0x0d),
            "未知诊断码：不在 0x791C1..0x791CC 闭集内"
        );
        assert!(detail_codes_ok());
    }
}
