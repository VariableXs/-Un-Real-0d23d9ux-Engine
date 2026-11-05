//! UNX-F1597 移交完成宣告（域→消费域正式交接） · 0x79340段
//!
//! 判据原文（UNX-F1597-J1 摘录）：「移交完成宣告生效（包+总链+时点+签收
//! 四件齐备宣告），宣告不可撤销断言，宣告记录归档」；完成时 API/软件写回：
//! 状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 四件齐备硬断言：移交单四件——包指纹（非零）/总链指纹（非零）/时点
//!    （大于零）/签收（承接方非空且已签），任一件缺失在递交口即拒，
//!    未齐备宣告在宣告口拦截；
//! 2. 宣告不可撤销断言：宣告落档后撤回口十次尝试逐次显性拒（相位不动、
//!    记录不改），撤回零通道；
//! 3. 时点回拨防线：时点只进不退，回拨在时点口显性拒——交接时点一旦
//!    推进不得倒退重演；
//! 4. 宣告记录归档：宣告序 + 四件逐行 + 消费域承接方，记录非空可查，
//!    未宣告先取记录显性拒；
//! 5. 相位机 备盘中→四件齐→已宣告 单向，倒退显性拒；已宣告后改单全拒；
//! 6. 诊断码闭集 0x79341..0x7934C，Err 用 Result<T,u32>，中文诊断文案带
//!    可操作建议。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡 B09 给定值；本批窗口 0x79200..=0x793C0）。
pub const SEG_BASE: u32 = 0x79340;

/// 撤回尝试种数（宣告不可撤销断言 10/10）。
pub const REVOKE_PROBES: usize = 10;

/// 移交单四件数（包/总链/时点/签收）。
pub const SHEET_ITEMS: usize = 4;

/// 指纹折叠种子（FNV-1a 偏移基）。
pub const DIGEST_SEED: u64 = 0xCBF2_9CE4_8422_2325;

/// 指纹折叠质数（FNV-1a 质数）。
pub const DIGEST_PRIME: u64 = 0x100_0000_01B3;

/// 移交完成宣告诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 相位倒退（备盘中→四件齐→已宣告 单向）。
    EPhase,
    /// 包指纹缺失（零指纹）。
    EPkg,
    /// 总链指纹缺失（零指纹）。
    EChain,
    /// 时点非法（零时点）。
    EPoint,
    /// 签收缺失（承接方空或未签）。
    ESign,
    /// 未齐备先宣告。
    EOrder,
    /// 宣告不可撤销（撤回口一律拒）。
    EIrrevocable,
    /// 已宣告后改单拒。
    ESealed,
    /// 重复递交。
    EDupSubmit,
    /// 空单递交。
    EEmpty,
    /// 时点回拨（时点只进不退）。
    EReplay,
    /// 未宣告先取记录。
    ENoRecord,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EPhase => 0x01,
                Diag::EPkg => 0x02,
                Diag::EChain => 0x03,
                Diag::EPoint => 0x04,
                Diag::ESign => 0x05,
                Diag::EOrder => 0x06,
                Diag::EIrrevocable => 0x07,
                Diag::ESealed => 0x08,
                Diag::EDupSubmit => 0x09,
                Diag::EEmpty => 0x0a,
                Diag::EReplay => 0x0b,
                Diag::ENoRecord => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EPhase => "相位倒退：备盘中→四件齐→已宣告 单向，倒退即拒",
            Diag::EPkg => "包指纹缺失：移交包指纹为零视为无包，先取包指纹再递交",
            Diag::EChain => "总链指纹缺失：总链指纹为零视为无链，先取 F1592 主键再递交",
            Diag::EPoint => "时点非法：时点为零不构成交接时点，先定时点再递交",
            Diag::ESign => "签收缺失：承接方为空或未签，先取得消费域签收再递交",
            Diag::EOrder => "未齐备先宣告：四件齐备才宣告，逐件补齐再宣告",
            Diag::EIrrevocable => "宣告不可撤销：移交宣告一经落档不可撤回，异议走消费域回执面",
            Diag::ESealed => "已宣告后改单拒：移交单已随宣告封存，改动走下一轮移交",
            Diag::EDupSubmit => "重复递交：移交单已在册，勿重复递交覆盖",
            Diag::EEmpty => "空单递交：无件不构成移交单，先逐件备齐",
            Diag::EReplay => "时点回拨：交接时点只进不退，回拨即拒",
            Diag::ENoRecord => "未宣告先取记录：宣告记录在宣告后生成，先宣告再取",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EPhase,
    Diag::EPkg,
    Diag::EChain,
    Diag::EPoint,
    Diag::ESign,
    Diag::EOrder,
    Diag::EIrrevocable,
    Diag::ESealed,
    Diag::EDupSubmit,
    Diag::EEmpty,
    Diag::EReplay,
    Diag::ENoRecord,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x79341..0x7934C 闭集内"
}

/// 移交相位（备盘中→四件齐→已宣告 单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HVPhase {
    /// 备盘中：四件未齐。
    Preparing,
    /// 四件齐：移交单校验过。
    Ready,
    /// 已宣告：宣告落档不可撤销。
    Declared,
}

impl HVPhase {
    /// 序数（倒退检测用）。
    pub fn ord(self) -> u8 {
        match self {
            HVPhase::Preparing => 0,
            HVPhase::Ready => 1,
            HVPhase::Declared => 2,
        }
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            HVPhase::Preparing => "备盘中",
            HVPhase::Ready => "四件齐",
            HVPhase::Declared => "已宣告",
        }
    }
}

/// 移交单（四件：包/总链/时点/签收）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandoverSheet {
    /// 件一：移交包指纹（非零）。
    pub pkg_digest: u64,
    /// 件二：总链指纹（非零，取 F1592 主键）。
    pub chain_digest: u64,
    /// 件三：交接时点（大于零，只进不退）。
    pub point: u32,
    /// 件四之一：消费域承接方（非空）。
    pub consumer: &'static str,
    /// 件四之二：已签收。
    pub signed: bool,
}

impl HandoverSheet {
    /// 单据非空校验（至少带一件有效载荷）。
    pub fn non_empty(&self) -> bool {
        self.pkg_digest != 0
            || self.chain_digest != 0
            || self.point != 0
            || !self.consumer.is_empty()
            || self.signed
    }

    /// 逐件校验（返回第一件违约件名；全过返回空串）。
    pub fn first_fault(&self) -> &'static str {
        if self.pkg_digest == 0 {
            return "包指纹";
        }
        if self.chain_digest == 0 {
            return "总链指纹";
        }
        if self.point == 0 {
            return "时点";
        }
        if self.consumer.is_empty() || !self.signed {
            return "签收";
        }
        ""
    }

    /// 四件齐备（逐件无违约）。
    pub fn four_items_ok(&self) -> bool {
        self.first_fault().is_empty()
    }
}

/// 移交完成宣告机。
#[derive(Clone, Debug)]
pub struct HandoverMachine {
    phase: HVPhase,
    sheet: Option<HandoverSheet>,
    /// 交接时点（只进不退，宣告时冻结）。
    point: u32,
    /// 宣告记录（宣告后落档）。
    record: String,
    /// 结构性拒绝计数。
    rejects: u32,
}

impl HandoverMachine {
    /// 新宣告机（备盘中）。
    pub fn new() -> HandoverMachine {
        HandoverMachine {
            phase: HVPhase::Preparing,
            sheet: None,
            point: 0,
            record: String::new(),
            rejects: 0,
        }
    }

    /// 相位。
    pub fn phase(&self) -> HVPhase {
        self.phase
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 交接时点。
    pub fn point(&self) -> u32 {
        self.point
    }

    /// 宣告记录。
    pub fn record(&self) -> &str {
        &self.record
    }

    /// 递交移交单（空单/逐件违约/重复递交逐门拒）。
    pub fn submit(&mut self, sheet: HandoverSheet) -> Result<(), u32> {
        if self.phase == HVPhase::Declared {
            self.rejects += 1;
            return Err(Diag::ESealed.val());
        }
        if self.sheet.is_some() {
            self.rejects += 1;
            return Err(Diag::EDupSubmit.val());
        }
        if !sheet.non_empty() {
            self.rejects += 1;
            return Err(Diag::EEmpty.val());
        }
        if sheet.pkg_digest == 0 {
            self.rejects += 1;
            return Err(Diag::EPkg.val());
        }
        if sheet.chain_digest == 0 {
            self.rejects += 1;
            return Err(Diag::EChain.val());
        }
        if sheet.point == 0 {
            self.rejects += 1;
            return Err(Diag::EPoint.val());
        }
        if sheet.consumer.is_empty() || !sheet.signed {
            self.rejects += 1;
            return Err(Diag::ESign.val());
        }
        self.sheet = Some(sheet);
        self.point = sheet.point;
        self.phase = HVPhase::Ready;
        Ok(())
    }

    /// 时点推进（只进不退；回拨显性拒）。
    pub fn advance_point(&mut self, new_point: u32) -> Result<(), u32> {
        if new_point <= self.point {
            self.rejects += 1;
            return Err(Diag::EReplay.val());
        }
        self.point = new_point;
        Ok(())
    }

    /// 宣告（四件齐备才宣告；未齐备在宣告口拦截）。
    pub fn declare(&mut self) -> Result<String, u32> {
        if self.phase == HVPhase::Declared {
            self.rejects += 1;
            return Err(Diag::EPhase.val());
        }
        let sheet = match self.sheet {
            Some(s) => s,
            None => {
                self.rejects += 1;
                return Err(Diag::EOrder.val());
            }
        };
        if !sheet.four_items_ok() {
            self.rejects += 1;
            return Err(Diag::EOrder.val());
        }
        self.phase = HVPhase::Declared;
        self.record = self.build_record();
        Ok(self.record.clone())
    }

    /// 宣告记录构建（四件逐行 + 承接方 + 时点）。
    fn build_record(&self) -> String {
        let sheet = match self.sheet {
            Some(s) => s,
            None => return String::new(),
        };
        let mut out = format!("移交完成宣告 时点{}\n", sheet.point);
        out.push_str(&format!("件一 包指纹{:016x}\n", sheet.pkg_digest));
        out.push_str(&format!("件二 总链指纹{:016x}\n", sheet.chain_digest));
        out.push_str(&format!("件三 时点{}\n", sheet.point));
        out.push_str(&format!(
            "件四 签收 {} 已签{}\n",
            sheet.consumer, sheet.signed
        ));
        out
    }

    /// 宣告记录取档（未宣告显性拒——记录为空）。
    pub fn record_or_err(&self) -> Result<&str, u32> {
        if self.phase != HVPhase::Declared || self.record.is_empty() {
            return Err(Diag::ENoRecord.val());
        }
        Ok(&self.record)
    }

    /// 撤回口（宣告不可撤销：一律显性拒，相位与记录不动）。
    pub fn revoke(&mut self, _reason: &str) -> Result<(), u32> {
        self.rejects += 1;
        Err(Diag::EIrrevocable.val())
    }
}

/// 指纹折叠（FNV-1a 整数折叠）。
pub fn sheet_digest(consumer: &str, point: u32) -> u64 {
    let mut h = DIGEST_SEED;
    for b in consumer.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(DIGEST_PRIME);
    }
    h ^= point as u64;
    h.wrapping_mul(DIGEST_PRIME)
}

/// 标准移交单（四件齐备：包/总链/时点/签收全备）。
pub fn standard_sheet() -> HandoverSheet {
    HandoverSheet {
        pkg_digest: sheet_digest("移交包四十批", 2025),
        chain_digest: sheet_digest("收官总链", 2026),
        point: 2026,
        consumer: "消费域承接组",
        signed: true,
    }
}

/// 标准宣告机：四件齐递交（四件齐相位）。
pub fn build_machine() -> HandoverMachine {
    let mut m = HandoverMachine::new();
    let _ = m.submit(standard_sheet());
    m
}

/// 撤回尝试 10/10：宣告落档后十次撤回逐次显性拒，
/// 相位与记录逐次不动。返回十布尔（true=该次撤回被拒且记录未变）。
pub fn revoke_matrix() -> [bool; REVOKE_PROBES] {
    let mut m = build_machine();
    let _ = m.declare();
    let record_before = m.record().to_string();
    let mut out = [false; REVOKE_PROBES];
    let mut i = 0usize;
    while i < REVOKE_PROBES {
        let rejected = m.revoke("反悔撤回") == Err(Diag::EIrrevocable.val());
        let intact = m.phase() == HVPhase::Declared && m.record() == record_before;
        if let Some(cell) = out.get_mut(i) {
            *cell = rejected && intact;
        }
        i += 1;
    }
    out
}

/// 缺件注入：四件各缺一件的移交单逐件构造。
/// 返回四元（按 包/总链/时点/签收 序，true=该缺件单被递交口或宣告口拦截）。
pub fn missing_items_matrix() -> [bool; SHEET_ITEMS] {
    let std = standard_sheet();
    let mut out = [false; SHEET_ITEMS];
    // 缺件一：包指纹零。
    let mut m = HandoverMachine::new();
    let s = HandoverSheet { pkg_digest: 0, ..std };
    let intercepted = m.submit(s) == Err(Diag::EPkg.val()) && m.phase() == HVPhase::Preparing;
    if let Some(cell) = out.get_mut(0) {
        *cell = intercepted;
    }
    // 缺件二：总链指纹零。
    let mut m = HandoverMachine::new();
    let s = HandoverSheet { chain_digest: 0, ..std };
    let intercepted = m.submit(s) == Err(Diag::EChain.val());
    if let Some(cell) = out.get_mut(1) {
        *cell = intercepted;
    }
    // 缺件三：时点零。
    let mut m = HandoverMachine::new();
    let s = HandoverSheet { point: 0, ..std };
    let intercepted = m.submit(s) == Err(Diag::EPoint.val());
    if let Some(cell) = out.get_mut(2) {
        *cell = intercepted;
    }
    // 缺件四：未签收。
    let mut m = HandoverMachine::new();
    let s = HandoverSheet { signed: false, ..std };
    let intercepted = m.submit(s) == Err(Diag::ESign.val());
    if let Some(cell) = out.get_mut(3) {
        *cell = intercepted;
    }
    out
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（对齐 0x40；窗口按 B09 派工卡 0x79200..=0x793C0）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x79200 || SEG_BASE > 0x793C0 {
        return false;
    }
    SEG_BASE == 0x79340
}

/// 四件齐备宣告生效（已宣告 + 记录非空 + 四件逐行全名）。
fn declared_ok(m: &HandoverMachine) -> bool {
    if m.phase() != HVPhase::Declared || m.record().is_empty() {
        return false;
    }
    let record = m.record();
    record.contains("包指纹")
        && record.contains("总链指纹")
        && record.contains("时点")
        && record.contains("签收")
}

/// 宣告不可撤销 10/10。
fn revoke_all_ok() -> bool {
    revoke_matrix() == [true; REVOKE_PROBES]
}

/// 缺件拦截 4/4。
fn missing_items_all_ok() -> bool {
    missing_items_matrix() == [true; SHEET_ITEMS]
}

/// 双跑一致（同构输入两次构建宣告记录一致且非空）。
fn dual_run_ok() -> bool {
    let mut a = build_machine();
    let mut b = build_machine();
    let ra = a.declare();
    let rb = b.declare();
    ra == rb && ra.is_ok()
}

/// 移交完成宣告判据：UNX-F1597 移交完成宣告。
pub fn run_unx1597_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1597");
    let mut machine = build_machine();

    s.add_dyn(
        "F1597-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    // 标准单四件齐备。
    let sheet = standard_sheet();
    s.add_dyn(
        "F1597-02 标准单四件齐备",
        sheet.four_items_ok() && sheet.first_fault().is_empty(),
        &format!("fault={}", sheet.first_fault()),
    );

    // 正常流：递交转四件齐。
    s.add_dyn(
        "F1597-03 递交转四件齐",
        machine.phase() == HVPhase::Ready && machine.point() == sheet.point,
        &format!("phase={} point={}", machine.phase().name(), machine.point()),
    );

    // 缺件注入 4/4 拦截。
    s.add_dyn(
        "F1597-04 缺件注入四件全拦",
        missing_items_all_ok(),
        &format!("matrix={:?}", missing_items_matrix()),
    );

    // 空单与重复递交逐门拒。
    let mut g = HandoverMachine::new();
    let empty = g.submit(HandoverSheet {
        pkg_digest: 0,
        chain_digest: 0,
        point: 0,
        consumer: "",
        signed: false,
    }) == Err(Diag::EEmpty.val());
    let _ = g.submit(standard_sheet());
    let dup = g.submit(standard_sheet()) == Err(Diag::EDupSubmit.val());
    s.add_dyn(
        "F1597-05 空单与重复递交逐门拒",
        empty && dup,
        &format!("rejects={}", g.rejects()),
    );

    // 未齐备先宣告显性拒。
    let mut early = HandoverMachine::new();
    let early_rej = early.declare() == Err(Diag::EOrder.val());
    s.add_dyn(
        "F1597-06 未齐备先宣告显性拒",
        early_rej && early.phase() == HVPhase::Preparing,
        &format!("phase={}", early.phase().name()),
    );

    // 宣告落档：记录非空且四件逐行全名。
    let declared = machine.declare();
    s.add_dyn(
        "F1597-07 四件齐备宣告落档",
        declared.is_ok() && declared_ok(&machine),
        &format!("bytes={}", declared.as_ref().map(|t| t.len()).unwrap_or(0)),
    );

    // 宣告不可撤销 10/10。
    s.add_dyn(
        "F1597-08 宣告不可撤销十次全拒",
        revoke_all_ok(),
        &format!("matrix={:?}", revoke_matrix()),
    );

    // 已宣告后改单显性拒。
    let sealed = machine.submit(standard_sheet()) == Err(Diag::ESealed.val());
    s.add_dyn(
        "F1597-09 已宣告后改单显性拒",
        sealed && machine.phase().ord() == 2,
        &format!("phase_ord={}", machine.phase().ord()),
    );

    // 时点回拨防线：回拨显性拒、同点显性拒、推进放行。
    let mut p = build_machine();
    let backward = p.advance_point(p.point() - 1) == Err(Diag::EReplay.val());
    let same = p.advance_point(p.point()) == Err(Diag::EReplay.val());
    let forward = p.advance_point(p.point() + 1) == Ok(());
    s.add_dyn(
        "F1597-10 时点只进不退",
        backward && same && forward,
        &format!("point={}", p.point()),
    );

    // 宣告记录取档：未宣告显性拒、宣告后可取。
    let no_record = HandoverMachine::new().record_or_err() == Err(Diag::ENoRecord.val());
    let record_ok = machine.record_or_err().is_ok()
        && machine.record().contains("消费域承接组");
    s.add_dyn(
        "F1597-11 宣告记录归档可查",
        no_record && record_ok,
        &format!("record_bytes={}", machine.record().len()),
    );

    // 双跑一致与诊断码闭集。
    s.add_dyn(
        "F1597-12 双跑一致与诊断码闭集",
        dual_run_ok() && ALL.len() == 12 && detail(SEG_BASE + 0x0C) == Diag::ENoRecord.detail(),
        &format!("all={} seg=0x{:05x}", ALL.len(), SEG_BASE),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diag_codes_closed() {
        assert_eq!(ALL.len(), 12);
        assert_eq!(Diag::EPhase.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::ENoRecord.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert!(detail(SEG_BASE + 0x70).starts_with("未知诊断码"));
    }

    #[test]
    fn seg_guard() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x79340);
    }

    #[test]
    fn sheet_item_faults() {
        let std = standard_sheet();
        assert!(std.four_items_ok());
        let no_pkg = HandoverSheet { pkg_digest: 0, ..std };
        assert_eq!(no_pkg.first_fault(), "包指纹");
        let no_chain = HandoverSheet { chain_digest: 0, ..std };
        assert_eq!(no_chain.first_fault(), "总链指纹");
        let no_point = HandoverSheet { point: 0, ..std };
        assert_eq!(no_point.first_fault(), "时点");
        let no_sign = HandoverSheet { signed: false, ..std };
        assert_eq!(no_sign.first_fault(), "签收");
        let blank = HandoverSheet { pkg_digest: 0, chain_digest: 0, point: 0, consumer: "", signed: false };
        assert!(!blank.non_empty());
    }

    #[test]
    fn submit_gates() {
        let mut m = HandoverMachine::new();
        // 空单先拒（单据全空）。
        assert_eq!(
            m.submit(HandoverSheet { pkg_digest: 0, chain_digest: 0, point: 0, consumer: "", signed: false }),
            Err(Diag::EEmpty.val())
        );
        assert_eq!(m.submit(standard_sheet()), Ok(()));
        assert_eq!(m.phase(), HVPhase::Ready);
        assert_eq!(m.submit(standard_sheet()), Err(Diag::EDupSubmit.val()));
    }

    #[test]
    fn missing_items_intercepted() {
        assert_eq!(missing_items_matrix(), [true; SHEET_ITEMS]);
    }

    #[test]
    fn declare_requires_ready_sheet() {
        let mut m = HandoverMachine::new();
        assert_eq!(m.declare(), Err(Diag::EOrder.val()));
        let _ = m.submit(standard_sheet());
        assert!(m.declare().is_ok());
        assert_eq!(m.phase(), HVPhase::Declared);
    }

    #[test]
    fn revoke_ten_of_ten() {
        assert_eq!(revoke_matrix(), [true; REVOKE_PROBES]);
        let mut m = build_machine();
        let _ = m.declare();
        // 撤回在未宣告时同样显性拒（撤回口零通道）。
        let mut n = HandoverMachine::new();
        assert_eq!(n.revoke("未宣告撤回"), Err(Diag::EIrrevocable.val()));
    }

    #[test]
    fn declared_seals_sheet() {
        let mut m = build_machine();
        let _ = m.declare();
        assert_eq!(m.submit(standard_sheet()), Err(Diag::ESealed.val()));
        assert_eq!(m.phase(), HVPhase::Declared);
        assert_eq!(m.phase().ord(), 2);
    }

    #[test]
    fn point_no_rewind() {
        let mut m = build_machine();
        let p0 = m.point();
        assert_eq!(m.advance_point(p0 - 1), Err(Diag::EReplay.val()));
        assert_eq!(m.advance_point(p0), Err(Diag::EReplay.val()));
        assert_eq!(m.advance_point(p0 + 1), Ok(()));
        assert_eq!(m.point(), p0 + 1);
    }

    #[test]
    fn record_gates() {
        let m = HandoverMachine::new();
        assert_eq!(m.record_or_err(), Err(Diag::ENoRecord.val()));
        let mut m2 = build_machine();
        assert_eq!(m2.record_or_err(), Err(Diag::ENoRecord.val()), "未宣告记录为空");
        let _ = m2.declare();
        let rec = m2.record_or_err();
        assert!(rec.is_ok());
        let t = match rec {
            Ok(t) => t,
            Err(_) => "",
        };
        assert!(t.contains("移交完成宣告"));
        assert!(t.contains("件四"));
    }

    #[test]
    fn sheet_digest_stable() {
        let a = sheet_digest("消费域承接组", 2026);
        let b = sheet_digest("消费域承接组", 2026);
        let c = sheet_digest("消费域承接组", 2027);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, 0);
    }

    #[test]
    fn dual_run_consistency() {
        assert!(dual_run_ok());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx1597_checks();
        assert_eq!(s.len(), 12);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
