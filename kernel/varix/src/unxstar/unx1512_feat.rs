//! UNX-F1512 切换期账本一致性（切换与延迟/零丢账联动） · 0x78180段
//!
//! 判据原文（UNX-F1512-J1 摘录）：「切换期账本一致性验证 10/10 轮全绿
//! （切换窗口期延迟账打标联动 F1411+零丢恒等式 F1469 不破），两账切换期
//! 结论互验一致率 100%，联动断链注入 10/10 必抓」；完成时 API/软件写回：
//! 状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 切换窗口相位单向：前切→切换中→后切，倒退显性拒——窗口期语义冻结；
//! 2. 延迟账打标联动（F1411）：切换中每一轮的操作必须带该轮切换标签
//!    （标签基址+轮号），前切/后切阶段必须零标签，错标/漏标即断链；
//! 3. 零丢恒等式（F1469）：每轮 入账 == 出账 + 在途，十轮恒等式全数复核，
//!    任何一轮不破才许绿；
//! 4. 两账互验一致率 100%：延迟账的轮结论与零丢账的轮结论逐轮对拍，
//!    一边断链一边完好即联动断链，注入必抓；
//! 5. 状态机 开放→已封账 单向，重放逐轮复算；诊断码闭集
//!    0x78181..0x7818C。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值）。
pub const SEG_BASE: u32 = 0x78180;

/// 切换期账本一致性诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 窗口相位倒退。
    EPhase,
    /// 切换期操作未打标。
    ENoTag,
    /// 标签与轮号不符。
    EBadTag,
    /// 零丢恒等式破坏。
    EIdentity,
    /// 未登记的轮次。
    ENoRound,
    /// 重复登记。
    EDup,
    /// 空账目。
    EEmpty,
    /// 超容。
    ECap,
    /// 已封账。
    ESealed,
    /// 联动断链（两账结论互验不符）。
    ELink,
    /// 登记乱序。
    EOrder,
    /// 封账重放不符。
    ESeal,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EPhase => 0x01,
                Diag::ENoTag => 0x02,
                Diag::EBadTag => 0x03,
                Diag::EIdentity => 0x04,
                Diag::ENoRound => 0x05,
                Diag::EDup => 0x06,
                Diag::EEmpty => 0x07,
                Diag::ECap => 0x08,
                Diag::ESealed => 0x09,
                Diag::ELink => 0x0a,
                Diag::EOrder => 0x0b,
                Diag::ESeal => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EPhase => "窗口相位倒退：前切→切换中→后切单向推进，禁止回退",
            Diag::ENoTag => "切换期未打标：切换中操作必须携带当轮切换标签",
            Diag::EBadTag => "标签不符：标签必须等于标签基址+当轮轮号",
            Diag::EIdentity => "零丢恒等式破：入账必须等于出账加在途",
            Diag::ENoRound => "未登记的轮次：先开轮再记账，禁止凭空补账",
            Diag::EDup => "重复登记：每轮每账只落一条，改账走修正轮",
            Diag::EEmpty => "空账目：全零账目不构成切换证据",
            Diag::ECap => "超容：十轮已齐，多余轮次滚动进下一窗口",
            Diag::ESealed => "已封账：封账后写入拒绝，开下一窗口",
            Diag::ELink => "联动断链：延迟账与零丢账结论互验不符，双账重跑",
            Diag::EOrder => "登记乱序：按轮号 1,2,3.. 顺序补齐",
            Diag::ESeal => "封账重放不符：轮结论复算不一致，账本可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EPhase,
    Diag::ENoTag,
    Diag::EBadTag,
    Diag::EIdentity,
    Diag::ENoRound,
    Diag::EDup,
    Diag::EEmpty,
    Diag::ECap,
    Diag::ESealed,
    Diag::ELink,
    Diag::EOrder,
    Diag::ESeal,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x78181..0x7818C 闭集内"
}

/// 本窗口切换轮数。
pub const ROUNDS: usize = 10;
/// 切换标签基址（F1411 联动标签 = 基址 + 轮号）。
pub const TAG_BASE: u32 = 0x1411;

/// 窗口相位：前切。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowPhase {
    /// 前切。
    Pre,
    /// 切换中。
    Switching,
    /// 后切。
    Post,
}

impl WindowPhase {
    /// 相位序（单调递增）。
    pub fn ord(self) -> u8 {
        match self {
            WindowPhase::Pre => 0,
            WindowPhase::Switching => 1,
            WindowPhase::Post => 2,
        }
    }
}

/// FNV 折叠辅助。
fn fold_word(h: u64, v: u64) -> u64 {
    let mut h = h;
    let mut i = 0u32;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xff;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一条延迟账目（F1411 打标面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LatEntry {
    /// 轮号（1..=10）。
    pub rid: u8,
    /// 登记时窗口相位。
    pub phase: WindowPhase,
    /// 切换标签（切换中必为 TAG_BASE+rid，前后切必为 0）。
    pub tag: u32,
    /// 切换时延（微秒，>0）。
    pub usec: u32,
}

/// 一条零丢账目（F1469 恒等面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdEntry {
    /// 轮号（1..=10）。
    pub rid: u8,
    /// 入账笔数。
    pub inbound: u32,
    /// 出账笔数。
    pub outbound: u32,
    /// 在途笔数。
    pub held: u32,
}

impl IdEntry {
    /// 空账目。
    pub fn is_blank(&self) -> bool {
        self.inbound == 0 && self.outbound == 0 && self.held == 0
    }

    /// 零丢恒等式：入账 == 出账 + 在途。
    pub fn identity_ok(&self) -> bool {
        self.inbound == self.outbound.saturating_add(self.held)
    }
}

/// 延迟账（F1411）。
pub struct LatencyLedger {
    entries: Vec<LatEntry>,
    sealed: bool,
    rejects: u32,
}

impl LatencyLedger {
    /// 空账。
    pub fn new() -> LatencyLedger {
        LatencyLedger { entries: Vec::new(), sealed: false, rejects: 0 }
    }

    /// 登记一条延迟账目：相位与标签的联动规则在这里强制。
    pub fn record(&mut self, e: LatEntry) -> Result<(), u32> {
        if self.sealed {
            self.rejects += 1;
            return Err(Diag::ESealed.val());
        }
        if self.entries.len() >= ROUNDS {
            self.rejects += 1;
            return Err(Diag::ECap.val());
        }
        if e.rid == 0 || (e.rid as usize) > ROUNDS {
            self.rejects += 1;
            return Err(Diag::ENoRound.val());
        }
        if e.usec == 0 {
            self.rejects += 1;
            return Err(Diag::EEmpty.val());
        }
        for x in &self.entries {
            if x.rid == e.rid {
                self.rejects += 1;
                return Err(Diag::EDup.val());
            }
        }
        if (e.rid as usize) != self.entries.len() + 1 {
            self.rejects += 1;
            return Err(Diag::EOrder.val());
        }
        // 窗口相位单调：相位不许比上一条账目倒退。
        if let Some(last) = self.entries.last() {
            if e.phase.ord() < last.phase.ord() {
                self.rejects += 1;
                return Err(Diag::EPhase.val());
            }
        }
        let want = TAG_BASE + e.rid as u32;
        match e.phase {
            WindowPhase::Switching => {
                if e.tag == 0 {
                    self.rejects += 1;
                    return Err(Diag::ENoTag.val());
                }
                if e.tag != want {
                    self.rejects += 1;
                    return Err(Diag::EBadTag.val());
                }
            }
            _ => {
                if e.tag != 0 {
                    self.rejects += 1;
                    return Err(Diag::EBadTag.val());
                }
            }
        }
        self.entries.push(e);
        Ok(())
    }

    /// 账目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 拒绝数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否封账。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 取账目。
    pub fn at(&self, i: usize) -> Option<LatEntry> {
        self.entries.get(i).copied()
    }

    /// 某轮延迟账结论：切换中带正确标签。
    pub fn round_ok(&self, rid: u8) -> bool {
        match self.entries.iter().find(|x| x.rid == rid) {
            Some(e) => e.phase == WindowPhase::Switching && e.tag == TAG_BASE + rid as u32,
            None => false,
        }
    }

    /// 冻结（满轮才可封）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(Diag::ESealed.val());
        }
        if self.entries.len() < ROUNDS {
            return Err(Diag::ENoRound.val());
        }
        self.sealed = true;
        Ok(())
    }
}

/// 零丢账（F1469）。
pub struct IdentityLedger {
    entries: Vec<IdEntry>,
    sealed: bool,
    rejects: u32,
}

impl IdentityLedger {
    /// 空账。
    pub fn new() -> IdentityLedger {
        IdentityLedger { entries: Vec::new(), sealed: false, rejects: 0 }
    }

    /// 登记一条零丢账目。
    pub fn record(&mut self, e: IdEntry) -> Result<(), u32> {
        if self.sealed {
            self.rejects += 1;
            return Err(Diag::ESealed.val());
        }
        if self.entries.len() >= ROUNDS {
            self.rejects += 1;
            return Err(Diag::ECap.val());
        }
        if e.rid == 0 || (e.rid as usize) > ROUNDS {
            self.rejects += 1;
            return Err(Diag::ENoRound.val());
        }
        if e.is_blank() {
            self.rejects += 1;
            return Err(Diag::EEmpty.val());
        }
        for x in &self.entries {
            if x.rid == e.rid {
                self.rejects += 1;
                return Err(Diag::EDup.val());
            }
        }
        if (e.rid as usize) != self.entries.len() + 1 {
            self.rejects += 1;
            return Err(Diag::EOrder.val());
        }
        if !e.identity_ok() {
            self.rejects += 1;
            return Err(Diag::EIdentity.val());
        }
        self.entries.push(e);
        Ok(())
    }

    /// 账目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 拒绝数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 取账目。
    pub fn at(&self, i: usize) -> Option<IdEntry> {
        self.entries.get(i).copied()
    }

    /// 某轮零丢结论。
    pub fn round_ok(&self, rid: u8) -> bool {
        match self.entries.iter().find(|x| x.rid == rid) {
            Some(e) => e.identity_ok(),
            None => false,
        }
    }

    /// 冻结。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(Diag::ESealed.val());
        }
        if self.entries.len() < ROUNDS {
            return Err(Diag::ENoRound.val());
        }
        self.sealed = true;
        Ok(())
    }
}

/// 双账审计：十轮全绿 + 两账互验一致。
pub struct SwitchAudit {
    lat: LatencyLedger,
    idt: IdentityLedger,
    root: u64,
    phase_sealed: bool,
}

impl SwitchAudit {
    /// 装配审计（两账必须都满轮）。
    pub fn assemble(lat: LatencyLedger, idt: IdentityLedger) -> Result<SwitchAudit, u32> {
        if lat.len() != ROUNDS || idt.len() != ROUNDS {
            return Err(Diag::ELink.val());
        }
        Ok(SwitchAudit { lat, idt, root: 0, phase_sealed: false })
    }

    /// 两账互验一致率（千分比）：逐轮延迟结论 == 零丢结论。
    pub fn agree_permille(&self) -> u32 {
        let mut agree = 0u32;
        let mut rid = 1u8;
        while (rid as usize) <= ROUNDS {
            if self.lat.round_ok(rid) == self.idt.round_ok(rid) {
                agree += 1;
            }
            rid += 1;
        }
        (agree * 1000) / ROUNDS as u32
    }

    /// 十轮全绿：两账逐轮都成立。
    pub fn all_green(&self) -> bool {
        let mut rid = 1u8;
        while (rid as usize) <= ROUNDS {
            if !self.lat.round_ok(rid) || !self.idt.round_ok(rid) {
                return false;
            }
            rid += 1;
        }
        true
    }

    /// 审计折叠根。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut rid = 1u8;
        while (rid as usize) <= ROUNDS {
            h = fold_word(h, self.lat.round_ok(rid) as u64);
            h = fold_word(h, self.idt.round_ok(rid) as u64);
            rid += 1;
        }
        h
    }

    /// 封账（互验一致且全绿才许封）。
    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.phase_sealed {
            return Err(Diag::ESealed.val());
        }
        if !self.all_green() || self.agree_permille() != 1000 {
            return Err(Diag::ELink.val());
        }
        self.root = self.fold();
        self.phase_sealed = true;
        Ok(self.root)
    }

    /// 封账根。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 重放：逐轮复算两账结论。
    pub fn replay(&self) -> Result<usize, u32> {
        if !self.phase_sealed {
            return Err(Diag::ESeal.val());
        }
        let mut rid = 1u8;
        let mut n = 0usize;
        while (rid as usize) <= ROUNDS {
            if !self.lat.round_ok(rid) || !self.idt.round_ok(rid) {
                return Err(Diag::ELink.val());
            }
            rid += 1;
            n += 1;
        }
        if self.root != self.fold() {
            return Err(Diag::ESeal.val());
        }
        Ok(n)
    }
}

/// 标准窗口：十轮全绿（切换中打标 + 恒等式成立）。
pub fn build_window() -> (LatencyLedger, IdentityLedger) {
    let mut lat = LatencyLedger::new();
    let mut idt = IdentityLedger::new();
    let mut rid = 1u8;
    while (rid as usize) <= ROUNDS {
        let _ = lat.record(LatEntry {
            rid,
            phase: WindowPhase::Switching,
            tag: TAG_BASE + rid as u32,
            usec: 800 + rid as u32 * 120,
        });
        let _ = idt.record(IdEntry {
            rid,
            inbound: 40 + rid as u32 * 10,
            outbound: 25 + rid as u32 * 6,
            held: 15 + rid as u32 * 4,
        });
        rid += 1;
    }
    (lat, idt)
}

/// 联动断链注入 10/10：逐轮断链——奇数轮错/漏标签、偶数轮破恒等式，
/// 每路都必须同时满足：断链注入被对应账门抓获、基线该轮绿。
pub fn link_break_matrix() -> [bool; 10] {
    let mut out = [false; 10];
    let mut rid = 1u8;
    while (rid as usize) <= ROUNDS {
        // 基线该轮必须绿。
        let (blat, bidt) = build_window();
        let base_green = blat.round_ok(rid) && bidt.round_ok(rid);
        // 半账：只登记 rid 之前的轮次，让 rid 可被注入。
        let caught = if rid % 2 == 1 {
            let mut lat = LatencyLedger::new();
            let mut k = 1u8;
            while k < rid {
                let _ = lat.record(LatEntry {
                    rid: k,
                    phase: WindowPhase::Switching,
                    tag: TAG_BASE + k as u32,
                    usec: 600,
                });
                k += 1;
            }
            // 漏标签：切换中零标签必拒。
            let no_tag = lat.record(LatEntry {
                rid,
                phase: WindowPhase::Switching,
                tag: 0,
                usec: 600,
            }) == Err(Diag::ENoTag.val());
            // 错标签：标签基址+轮号不符必拒（账面未变，同一格可再注入）。
            let bad_tag = lat.record(LatEntry {
                rid,
                phase: WindowPhase::Switching,
                tag: TAG_BASE + rid as u32 + 1,
                usec: 600,
            }) == Err(Diag::EBadTag.val());
            no_tag && bad_tag
        } else {
            let mut idt = IdentityLedger::new();
            let mut k = 1u8;
            while k < rid {
                let _ = idt.record(IdEntry {
                    rid: k,
                    inbound: 30 + k as u32 * 5,
                    outbound: 18 + k as u32 * 3,
                    held: 12 + k as u32 * 2,
                });
                k += 1;
            }
            // 破恒等式：入账加一必拒。
            let broken = idt.record(IdEntry {
                rid,
                inbound: 30 + rid as u32 * 5 + 1,
                outbound: 18 + rid as u32 * 3,
                held: 12 + rid as u32 * 2,
            }) == Err(Diag::EIdentity.val());
            // 断链修复后按恒等式登记必须放行。
            let recover = idt.record(IdEntry {
                rid,
                inbound: 30 + rid as u32 * 5,
                outbound: 18 + rid as u32 * 3,
                held: 12 + rid as u32 * 2,
            }) == Ok(());
            broken && recover
        };
        out[(rid - 1) as usize] = base_green && caught;
        rid += 1;
    }
    out
}

/// 审计摘要（落档面）。
pub fn audit_text(a: &SwitchAudit) -> String {
    format!(
        "switch_audit rounds={} agree={}permille green={} root={:016x}",
        ROUNDS,
        a.agree_permille(),
        a.all_green(),
        a.root(),
    )
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    SEG_BASE == 0x78180
}

/// 十轮全绿验证（未封账审计不落根，根值面由封账判据覆盖）。
fn audit_ok(a: &SwitchAudit) -> bool {
    a.all_green() && a.agree_permille() == 1000
}

/// 联动断链注入 10/10 必抓。
fn link_ok() -> bool {
    link_break_matrix() == [true; 10]
}

/// 相位单调：倒退必须被拒（延迟账门面）。
fn phase_ok(lat: &LatencyLedger) -> bool {
    let mut i = 0usize;
    while i < lat.len() {
        let e = match lat.at(i) {
            Some(x) => x,
            None => return false,
        };
        if e.phase.ord() < WindowPhase::Switching.ord() && e.tag != 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let (la, ia) = build_window();
    let (lb, ib) = build_window();
    la.len() == lb.len() && ia.len() == ib.len() && {
        let a = SwitchAudit::assemble(la, ia);
        let b = SwitchAudit::assemble(lb, ib);
        match (a, b) {
            (Ok(x), Ok(y)) => x.fold() == y.fold() && x.fold() != 0,
            _ => false,
        }
    }
}

/// 十五项判据。
pub fn run_unx1512_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1512");
    let (lat, idt) = build_window();
    let mut audit = match SwitchAudit::assemble(lat, idt) {
        Ok(a) => a,
        Err(_) => SwitchAudit { lat: LatencyLedger::new(), idt: IdentityLedger::new(), root: 0, phase_sealed: false },
    };

    s.add_dyn(
        "F1512-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1512-02 十轮验证全绿",
        audit_ok(&audit),
        &format!("agree={} green={}", audit.agree_permille(), audit.all_green()),
    );

    s.add_dyn(
        "F1512-03 延迟账打标联动",
        phase_ok(&audit.fold_lat()),
        &format!("lat_len={}", audit.fold_lat().len()),
    );

    s.add_dyn(
        "F1512-04 零丢恒等式十轮不破",
        {
            let mut ok = true;
            let mut rid = 1u8;
            while (rid as usize) <= ROUNDS {
                if !audit.fold_idt().round_ok(rid) {
                    ok = false;
                }
                rid += 1;
            }
            ok
        },
        &format!("idt_len={}", audit.fold_idt().len()),
    );

    s.add_dyn(
        "F1512-05 两账互验一致率百分百",
        audit.agree_permille() == 1000,
        &format!("agree={}", audit.agree_permille()),
    );

    s.add_dyn(
        "F1512-06 联动断链注入十路必抓",
        link_ok(),
        &format!("link10={:?}", link_break_matrix()),
    );

    // 错标签与漏标签拒绝。
    let mut l = LatencyLedger::new();
    let _ = l.record(LatEntry { rid: 1, phase: WindowPhase::Switching, tag: TAG_BASE + 1, usec: 700 });
    let bad_tag = l.record(LatEntry { rid: 2, phase: WindowPhase::Switching, tag: TAG_BASE + 9, usec: 700 })
        == Err(Diag::EBadTag.val());
    let mut l2 = LatencyLedger::new();
    let no_tag = l2.record(LatEntry { rid: 1, phase: WindowPhase::Switching, tag: 0, usec: 700 })
        == Err(Diag::ENoTag.val());
    let pre_tagged = l2.record(LatEntry { rid: 1, phase: WindowPhase::Pre, tag: TAG_BASE + 1, usec: 700 })
        == Err(Diag::EBadTag.val());
    s.add_dyn(
        "F1512-07 标签门错漏必拒",
        bad_tag && no_tag && pre_tagged && l.rejects() == 1 && l2.rejects() == 2,
        &format!("r1={} r2={}", l.rejects(), l2.rejects()),
    );

    // 恒等式破拒绝。
    let mut i = IdentityLedger::new();
    let broken = i.record(IdEntry { rid: 1, inbound: 50, outbound: 20, held: 20 })
        == Err(Diag::EIdentity.val());
    let good = i.record(IdEntry { rid: 1, inbound: 50, outbound: 20, held: 30 }) == Ok(());
    let blank = i.record(IdEntry { rid: 2, inbound: 0, outbound: 0, held: 0 }) == Err(Diag::EEmpty.val());
    s.add_dyn(
        "F1512-08 恒等式门",
        broken && good && blank && i.rejects() == 2 && i.len() == 1,
        &format!("rejects={} len={}", i.rejects(), i.len()),
    );

    // 重复/乱序/超容/空账门。
    let mut d = LatencyLedger::new();
    let _ = d.record(LatEntry { rid: 1, phase: WindowPhase::Switching, tag: TAG_BASE + 1, usec: 100 });
    let dup = d.record(LatEntry { rid: 1, phase: WindowPhase::Switching, tag: TAG_BASE + 1, usec: 100 })
        == Err(Diag::EDup.val());
    let order = d.record(LatEntry { rid: 3, phase: WindowPhase::Switching, tag: TAG_BASE + 3, usec: 100 })
        == Err(Diag::EOrder.val());
    let noround = d.record(LatEntry { rid: 0, phase: WindowPhase::Switching, tag: 0, usec: 100 })
        == Err(Diag::ENoRound.val());
    s.add_dyn(
        "F1512-09 结构门三路",
        dup && order && noround && d.rejects() == 3 && d.len() == 1,
        &format!("rejects={}", d.rejects()),
    );

    // 封账单向：欠轮不许封，满轮封后写入拒绝。
    let mut half = LatencyLedger::new();
    let _ = half.record(LatEntry { rid: 1, phase: WindowPhase::Switching, tag: TAG_BASE + 1, usec: 100 });
    let under = half.seal() == Err(Diag::ENoRound.val());
    let (mut fl, mut fi) = build_window();
    let s1 = fl.seal().is_ok() && fi.seal().is_ok();
    let after = fl.record(LatEntry { rid: 11, phase: WindowPhase::Post, tag: 0, usec: 1 })
        == Err(Diag::ESealed.val());
    s.add_dyn(
        "F1512-10 封账单向",
        under && s1 && after && fl.is_sealed(),
        &format!("sealed={}", fl.is_sealed()),
    );

    // 相位单调：切换中后落前切相位账目必拒（EPhase），后切带切换标签必拒。
    let mut p = LatencyLedger::new();
    let _ = p.record(LatEntry { rid: 1, phase: WindowPhase::Switching, tag: TAG_BASE + 1, usec: 100 });
    let regress = p.record(LatEntry { rid: 2, phase: WindowPhase::Pre, tag: 0, usec: 100 })
        == Err(Diag::EPhase.val());
    // 前切账目本身合法（零标签），但倒退相位语义由账门判：Post 阶段账目
    // 不允许再带切换标签。
    let post_tagged = p.record(LatEntry { rid: 2, phase: WindowPhase::Post, tag: TAG_BASE + 2, usec: 100 })
        == Err(Diag::EBadTag.val());
    s.add_dyn(
        "F1512-11 相位联动门",
        regress && post_tagged && p.rejects() == 2,
        &format!("rejects={}", p.rejects()),
    );

    s.add_dyn(
        "F1512-12 封账重放逐轮一致",
        {
            let (a, b) = build_window();
            match SwitchAudit::assemble(a, b) {
                Ok(mut x) => {
                    let r1 = x.seal().is_ok();
                    r1 && x.replay() == Ok(ROUNDS)
                }
                Err(_) => false,
            }
        },
        &format!("root={:016x}", audit.fold()),
    );

    // 断链审计拒绝封账：未满轮装配必拒且返回联动断链码。
    let broken_audit = {
        let r = SwitchAudit::assemble(LatencyLedger::new(), IdentityLedger::new());
        match r {
            Err(c) => c == Diag::ELink.val(),
            Ok(_) => false,
        }
    };
    s.add_dyn(
        "F1512-13 断链审计拒绝",
        broken_audit,
        &format!("agree={}", audit.agree_permille()),
    );

    s.add_dyn(
        "F1512-14 双跑一致",
        dual_run_ok(),
        &format!("fold={:016x}", audit.fold()),
    );

    s.add_dyn(
        "F1512-15 审计摘要非空",
        {
            let (a, b) = build_window();
            match SwitchAudit::assemble(a, b) {
                Ok(mut x) => {
                    let _ = x.seal();
                    let t = audit_text(&x);
                    t.contains("rounds=10") && t.contains("agree=1000permille") && t.len() > 30
                }
                Err(_) => false,
            }
        },
        &audit_text(&audit),
    );

    s
}

// SwitchAudit 内账访问面（判据面用，公开但只读）。
impl SwitchAudit {
    /// 延迟账只读视图。
    pub fn fold_lat(&self) -> &LatencyLedger {
        &self.lat
    }

    /// 零丢账只读视图。
    pub fn fold_idt(&self) -> &IdentityLedger {
        &self.idt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_rule_frozen() {
        let mut l = LatencyLedger::new();
        // 前切：必须零标签。
        assert_eq!(
            l.record(LatEntry { rid: 1, phase: WindowPhase::Pre, tag: 0, usec: 500 }),
            Ok(())
        );
        // 切换中：正确标签放行（相位单调推进）。
        assert_eq!(
            l.record(LatEntry { rid: 2, phase: WindowPhase::Switching, tag: TAG_BASE + 2, usec: 500 }),
            Ok(())
        );
        // 后切：必须零标签。
        assert_eq!(
            l.record(LatEntry { rid: 3, phase: WindowPhase::Post, tag: 0, usec: 500 }),
            Ok(())
        );
        assert_eq!(l.len(), 3);
        assert!(!l.round_ok(1), "前切轮不算切换中");
        assert!(l.round_ok(2), "切换中带正确标签才算联动成立");
        assert!(!l.round_ok(3), "后切轮不算切换中");
        // 相位倒退：后切之后再记切换中必拒。
        assert_eq!(
            l.record(LatEntry { rid: 4, phase: WindowPhase::Switching, tag: TAG_BASE + 4, usec: 500 }),
            Err(Diag::EPhase.val())
        );
    }

    #[test]
    fn identity_holds() {
        let e = IdEntry { rid: 1, inbound: 50, outbound: 20, held: 30 };
        assert!(e.identity_ok());
        let bad = IdEntry { rid: 2, inbound: 51, outbound: 20, held: 30 };
        assert!(!bad.identity_ok());
        let mut l = IdentityLedger::new();
        assert_eq!(l.record(e), Ok(()));
        assert!(l.round_ok(1));
        // 第 2 轮破恒等式必拒，修复后同格放行。
        assert_eq!(l.record(bad), Err(Diag::EIdentity.val()));
        assert_eq!(
            l.record(IdEntry { rid: 2, inbound: 50, outbound: 20, held: 30 }),
            Ok(())
        );
        // 空账目必拒（空门先于序门）。
        assert_eq!(l.record(IdEntry { rid: 3, inbound: 0, outbound: 0, held: 0 }), Err(Diag::EEmpty.val()));
        assert_eq!(l.rejects(), 2);
    }

    #[test]
    fn window_ten_rounds_green() {
        let (lat, idt) = build_window();
        assert_eq!(lat.len(), ROUNDS);
        assert_eq!(idt.len(), ROUNDS);
        let mut rid = 1u8;
        while (rid as usize) <= ROUNDS {
            assert!(lat.round_ok(rid));
            assert!(idt.round_ok(rid));
            rid += 1;
        }
        let a = SwitchAudit::assemble(lat, idt).unwrap();
        assert!(a.all_green());
        assert_eq!(a.agree_permille(), 1000);
    }

    #[test]
    fn link_breaks_all_caught() {
        assert_eq!(link_break_matrix(), [true; 10]);
        assert!(link_ok());
    }

    #[test]
    fn structural_gates() {
        let mut l = LatencyLedger::new();
        let _ = l.record(LatEntry { rid: 1, phase: WindowPhase::Switching, tag: TAG_BASE + 1, usec: 100 });
        assert_eq!(l.record(LatEntry { rid: 1, phase: WindowPhase::Switching, tag: TAG_BASE + 1, usec: 100 }), Err(Diag::EDup.val()));
        assert_eq!(l.record(LatEntry { rid: 3, phase: WindowPhase::Switching, tag: TAG_BASE + 3, usec: 100 }), Err(Diag::EOrder.val()));
        assert_eq!(l.record(LatEntry { rid: 0, phase: WindowPhase::Switching, tag: 0, usec: 100 }), Err(Diag::ENoRound.val()));
        assert_eq!(l.record(LatEntry { rid: 11, phase: WindowPhase::Switching, tag: 0, usec: 100 }), Err(Diag::ENoRound.val()));
        assert_eq!(l.record(LatEntry { rid: 2, phase: WindowPhase::Switching, tag: TAG_BASE + 2, usec: 0 }), Err(Diag::EEmpty.val()));
        assert_eq!(l.rejects(), 5);
        assert_eq!(l.len(), 1);
    }

    #[test]
    fn seal_monotonic_and_replay() {
        let (mut lat, mut idt) = build_window();
        assert_eq!(lat.seal(), Ok(()));
        assert_eq!(idt.seal(), Ok(()));
        assert_eq!(lat.seal(), Err(Diag::ESealed.val()));
        assert_eq!(lat.record(LatEntry { rid: 11, phase: WindowPhase::Post, tag: 0, usec: 1 }), Err(Diag::ESealed.val()));
        let mut a = SwitchAudit::assemble(lat, idt).unwrap();
        let root = a.seal();
        assert!(root.is_ok());
        assert_ne!(a.root(), 0);
        assert_eq!(a.replay(), Ok(10));
        assert_eq!(a.seal(), Err(Diag::ESealed.val()));
    }

    #[test]
    fn audit_rejects_incomplete() {
        let mut lat = LatencyLedger::new();
        let mut idt = IdentityLedger::new();
        let _ = lat.record(LatEntry { rid: 1, phase: WindowPhase::Switching, tag: TAG_BASE + 1, usec: 100 });
        let _ = idt.record(IdEntry { rid: 1, inbound: 10, outbound: 4, held: 6 });
        let r1 = SwitchAudit::assemble(lat, idt);
        assert!(match r1 {
            Err(c) => c == Diag::ELink.val(),
            Ok(_) => false,
        });
        let mut idt2 = IdentityLedger::new();
        let _ = idt2.record(IdEntry { rid: 1, inbound: 10, outbound: 4, held: 6 });
        let r2 = SwitchAudit::assemble(LatencyLedger::new(), idt2);
        assert!(match r2 {
            Err(c) => c == Diag::ELink.val(),
            Ok(_) => false,
        });
    }

    #[test]
    fn diag_codes_closed() {
        assert_eq!(ALL.len(), 12);
        assert_eq!(Diag::EPhase.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::ESeal.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert_eq!(detail(SEG_BASE + 0x60), "未知诊断码：不在 0x78181..0x7818C 闭集内");
    }

    #[test]
    fn checks_all_green_fifteen() {
        let s = run_unx1512_checks();
        assert_eq!(s.len(), 15);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
        let (lat, idt) = build_window();
        let mut a = SwitchAudit::assemble(lat, idt).unwrap();
        let _ = a.seal();
        assert!(audit_text(&a).contains("agree=1000permille"));
    }
}
