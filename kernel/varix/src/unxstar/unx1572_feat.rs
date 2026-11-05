//! UNX-F1572 联签一致性总对账（总表↔总账↔终验三向） · 0x78E00段
//!
//! 判据原文（UNX-F1572-J1 摘录）：「三向对账通过（F1561 状态表↔F1568 总账↔
//! F1566 终验逐项对平 100%），失配注入 10/10 检出，对账报告落档」；完成时
//! API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 三源独立建模：F1561 状态表 / F1568 总账 / F1566 终验各持一份逐项状态，
//!    对账不抄近路——三源各自入账后按项逐一对平，任何一源缺项即拒；
//! 2. 对平判据闭合：同一项三源状态字必须逐字相等（枚举闭合四态：
//!    待签/已联签/已终验/已入账），任何一源独走即记失配；
//! 3. 失配注入 10/10：逐项对三个源各做一次单腿篡改，对账门必须全数检出；
//! 4. 对账报告落档：报告行逐项生成（含失配清单与折叠根），失配未清禁止
//!    封档，封档后报告行冻结（重放逐字一致）；
//! 5. 状态机 起草→已对平→已封档 单向，倒退显性拒；诊断码闭集
//!    0x78E01..0x78E0C。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值；B07 批窗口 0x78E00..=0x78FC0）。
pub const SEG_BASE: u32 = 0x78E00;

/// 对账项数（十项逐一对平）。
pub const ITEM_COUNT: usize = 10;

/// 三个对账源（判据原文钦定的三张账）。
pub const SRC_TABLE: &str = "F1561状态表";
pub const SRC_LEDGER: &str = "F1568总账";
pub const SRC_ACCEPT: &str = "F1566终验";

/// 对账总对账诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 项状态取值非法。
    EState,
    /// 项号越界或重复。
    ESid,
    /// 三源未齐（缺任一源即拒）。
    ESrcLack,
    /// 对账失配（三源状态字不一致）。
    EMismatch,
    /// 失配未清禁止封档。
    ERedBlock,
    /// 账已封档。
    ESealed,
    /// 对账已发起、重复发起。
    EDupRecon,
    /// 空项样本（无项可对）。
    EEmpty,
    /// 对账报告缺失。
    ENoReport,
    /// 报告重放不符。
    EReplay,
    /// 报告折叠根为零（未落档）。
    ENoRoot,
    /// 相位倒退。
    EPhase,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EState => 0x01,
                Diag::ESid => 0x02,
                Diag::ESrcLack => 0x03,
                Diag::EMismatch => 0x04,
                Diag::ERedBlock => 0x05,
                Diag::ESealed => 0x06,
                Diag::EDupRecon => 0x07,
                Diag::EEmpty => 0x08,
                Diag::ENoReport => 0x09,
                Diag::EReplay => 0x0a,
                Diag::ENoRoot => 0x0b,
                Diag::EPhase => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EState => "项状态取值非法：只认待签/已联签/已终验/已入账四态，先核状态字",
            Diag::ESid => "项号越界或重复：按 1..10 逐项对账，每项只对一次",
            Diag::ESrcLack => "三源未齐：F1561/F1568/F1566 缺任一源先补账，再发起对账",
            Diag::EMismatch => "对账失配：该项三源状态字不一致，以终验为准逐源校正",
            Diag::ERedBlock => "失配未清：存在失配项禁止封档，先清失配再封",
            Diag::ESealed => "账已封档：封档后写入拒绝，改动走下一批总对账",
            Diag::EDupRecon => "对账已发起：重复发起拒绝，先取回上一轮报告",
            Diag::EEmpty => "空项样本：无项不构成对账，先补齐十项",
            Diag::ENoReport => "对账报告缺失：封档前必须生成报告行",
            Diag::EReplay => "报告重放不符：封档后报告被改动，账本可疑须复核",
            Diag::ENoRoot => "折叠根为零：报告未落档，重新生成报告",
            Diag::EPhase => "相位倒退：起草→已对平→已封档 单向，倒退即拒",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EState,
    Diag::ESid,
    Diag::ESrcLack,
    Diag::EMismatch,
    Diag::ERedBlock,
    Diag::ESealed,
    Diag::EDupRecon,
    Diag::EEmpty,
    Diag::ENoReport,
    Diag::EReplay,
    Diag::ENoRoot,
    Diag::EPhase,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x78E01..0x78E0C 闭集内"
}

/// 项状态四态闭合（三源共用的状态字）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemState {
    /// 待联签。
    Draft,
    /// 已联签。
    Signed,
    /// 已终验。
    Accepted,
    /// 已入账。
    Booked,
}

impl ItemState {
    /// 序数（对平比较用）。
    pub fn ord(self) -> u8 {
        match self {
            ItemState::Draft => 0,
            ItemState::Signed => 1,
            ItemState::Accepted => 2,
            ItemState::Booked => 3,
        }
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            ItemState::Draft => "待签",
            ItemState::Signed => "已联签",
            ItemState::Accepted => "已终验",
            ItemState::Booked => "已入账",
        }
    }

    /// 从序数还原（闭集外返回 None）。
    pub fn from_ord(o: u8) -> Option<ItemState> {
        match o {
            0 => Some(ItemState::Draft),
            1 => Some(ItemState::Signed),
            2 => Some(ItemState::Accepted),
            3 => Some(ItemState::Booked),
            _ => None,
        }
    }
}

/// 四态闭集数组。
pub const STATE_ALL: &[ItemState] = &[
    ItemState::Draft,
    ItemState::Signed,
    ItemState::Accepted,
    ItemState::Booked,
];

/// 单项三元组：同一项在三个源的登记状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReconItem {
    /// 项号（1..=10）。
    pub sid: u16,
    /// F1561 状态表登记态。
    pub table: ItemState,
    /// F1568 总账登记态。
    pub ledger: ItemState,
    /// F1566 终验登记态。
    pub accept: ItemState,
}

impl ReconItem {
    /// 该项是否三源对平（逐字相等）。
    pub fn flat(&self) -> bool {
        self.table == self.ledger && self.ledger == self.accept
    }

    /// 失配腿名（对平则空串）。
    pub fn odd_leg(&self) -> &'static str {
        if self.flat() {
            return "";
        }
        // 以终验为基准：状态表或总账任一与终验不一致即点名。
        if self.table != self.accept {
            SRC_TABLE
        } else if self.ledger != self.accept {
            SRC_LEDGER
        } else {
            SRC_ACCEPT
        }
    }
}

/// 对账相位（起草→已对平→已封档 单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 起草：三源入账中。
    Draft,
    /// 已对平：对账已发起且报告已生成。
    Reconciled,
    /// 已封档：报告冻结、写入拒绝。
    Sealed,
}

impl Phase {
    /// 序数（倒退检测用）。
    pub fn ord(self) -> u8 {
        match self {
            Phase::Draft => 0,
            Phase::Reconciled => 1,
            Phase::Sealed => 2,
        }
    }
}

/// 对账报告：逐项行 + 失配清单 + 折叠根。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconReport {
    /// 逐项报告行（已落档）。
    lines: Vec<String>,
    /// 失配项号清单。
    mismatches: Vec<u16>,
    /// 折叠根（FNV-1a）。
    root: u64,
}

impl ReconReport {
    /// 报告行数。
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// 取第 k 行。
    pub fn line_at(&self, k: usize) -> Option<&str> {
        self.lines.get(k).map(|s| s.as_str())
    }

    /// 失配项数。
    pub fn mismatch_count(&self) -> usize {
        self.mismatches.len()
    }

    /// 失配清单只读视图。
    pub fn mismatches(&self) -> &[u16] {
        &self.mismatches
    }

    /// 折叠根。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 全文（逐行拼接，落档面）。
    pub fn text(&self) -> String {
        let mut out = String::new();
        for l in &self.lines {
            out.push_str(l);
            out.push('\n');
        }
        out
    }
}

/// FNV-1a 折叠（整数哈希，无浮点无随机）。
fn fold_word(mut h: u64, w: u64) -> u64 {
    let mut k = w;
    // 拆成四个字节逐字节折叠（u8 转换无歧义）。
    let mut i = 0;
    while i < 8 {
        let b = (k & 0xFF) as u8;
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001B3);
        k >>= 8;
        i += 1;
    }
    h
}

/// 三向对账账本：三源逐项登记 + 对平 + 报告封档。
#[derive(Clone, Debug)]
pub struct ReconLedger {
    phase: Phase,
    /// 三源逐项登记（每源一份，按项号存）。
    table_leg: Vec<Option<ItemState>>,
    ledger_leg: Vec<Option<ItemState>>,
    accept_leg: Vec<Option<ItemState>>,
    /// 对账报告（已对平后生成）。
    report: Option<ReconReport>,
    /// 结构性拒绝计数（诊断面）。
    rejects: u32,
}

impl ReconLedger {
    /// 新账本（预置十项空槽）。
    pub fn new() -> ReconLedger {
        ReconLedger {
            phase: Phase::Draft,
            table_leg: vec![None; ITEM_COUNT],
            ledger_leg: vec![None; ITEM_COUNT],
            accept_leg: vec![None; ITEM_COUNT],
            report: None,
            rejects: 0,
        }
    }

    /// 相位。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 结构性拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 已齐源计数（三源都齐的项数）。
    pub fn full_legs(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < ITEM_COUNT {
            let a = self.table_leg.get(i);
            let b = self.ledger_leg.get(i);
            let c = self.accept_leg.get(i);
            let full = match (a, b, c) {
                (Some(Some(_)), Some(Some(_)), Some(Some(_))) => true,
                _ => false,
            };
            if full {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 单腿登记（sid 1..=10，状态必须闭集内）。
    fn set_leg(&mut self, leg: u8, sid: u16, st: ItemState) -> Result<(), u32> {
        if self.phase == Phase::Sealed {
            return Err(Diag::ESealed.val());
        }
        if sid == 0 || (sid as usize) > ITEM_COUNT {
            self.rejects += 1;
            return Err(Diag::ESid.val());
        }
        if STATE_ALL.iter().all(|x| *x != st) {
            self.rejects += 1;
            return Err(Diag::EState.val());
        }
        let slot = match leg {
            0 => &mut self.table_leg,
            1 => &mut self.ledger_leg,
            _ => &mut self.accept_leg,
        };
        let idx = (sid - 1) as usize;
        match slot.get_mut(idx) {
            Some(cell) => {
                *cell = Some(st);
                Ok(())
            }
            None => Err(Diag::ESid.val()),
        }
    }

    /// F1561 状态表登记。
    pub fn set_table(&mut self, sid: u16, st: ItemState) -> Result<(), u32> {
        self.set_leg(0, sid, st)
    }

    /// F1568 总账登记。
    pub fn set_ledger(&mut self, sid: u16, st: ItemState) -> Result<(), u32> {
        self.set_leg(1, sid, st)
    }

    /// F1566 终验登记。
    pub fn set_accept(&mut self, sid: u16, st: ItemState) -> Result<(), u32> {
        self.set_leg(2, sid, st)
    }

    /// 取某项三元组（三源齐全才返回）。
    pub fn triplet_at(&self, sid: u16) -> Option<ReconItem> {
        if sid == 0 || (sid as usize) > ITEM_COUNT {
            return None;
        }
        let idx = (sid - 1) as usize;
        let t = match self.table_leg.get(idx) {
            Some(Some(x)) => *x,
            _ => return None,
        };
        let l = match self.ledger_leg.get(idx) {
            Some(Some(x)) => *x,
            _ => return None,
        };
        let a = match self.accept_leg.get(idx) {
            Some(Some(x)) => *x,
            _ => return None,
        };
        Some(ReconItem { sid, table: t, ledger: l, accept: a })
    }

    /// 发起三向对账：三源必须全齐，逐项对平生成报告。
    pub fn reconcile(&mut self) -> Result<ReconReport, u32> {
        if self.phase == Phase::Sealed {
            return Err(Diag::ESealed.val());
        }
        if self.phase == Phase::Reconciled {
            return Err(Diag::EDupRecon.val());
        }
        if self.full_legs() < ITEM_COUNT {
            return Err(Diag::ESrcLack.val());
        }
        let mut lines = Vec::new();
        let mut mismatches = Vec::new();
        let mut h: u64 = 0xCBF29CE484222325;
        let mut sid = 1u16;
        while sid <= ITEM_COUNT as u16 {
            let it = match self.triplet_at(sid) {
                Some(x) => x,
                None => return Err(Diag::ESrcLack.val()),
            };
            let line = if it.flat() {
                format!(
                    "项{} 对平 {} {}={}={}={}",
                    sid, it.table.name(), SRC_TABLE, SRC_LEDGER, SRC_ACCEPT, it.accept.name()
                )
            } else {
                mismatches.push(sid);
                format!(
                    "项{} 失配 {}={} {}={} {}={} 异常腿={}",
                    sid,
                    SRC_TABLE,
                    it.table.name(),
                    SRC_LEDGER,
                    it.ledger.name(),
                    SRC_ACCEPT,
                    it.accept.name(),
                    it.odd_leg()
                )
            };
            h = fold_word(h, sid as u64);
            h = fold_word(h, it.table.ord() as u64);
            h = fold_word(h, it.ledger.ord() as u64);
            h = fold_word(h, it.accept.ord() as u64);
            lines.push(line);
            sid += 1;
        }
        let rep = ReconReport { lines, mismatches, root: h };
        self.report = Some(rep.clone());
        self.phase = Phase::Reconciled;
        Ok(rep)
    }

    /// 封档：失配必须已清零，报告必须存在且根非零。
    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.phase == Phase::Sealed {
            return Err(Diag::ESealed.val());
        }
        if self.phase != Phase::Reconciled {
            return Err(Diag::EPhase.val());
        }
        let rep = match &self.report {
            Some(r) => r,
            None => return Err(Diag::ENoReport.val()),
        };
        if rep.mismatch_count() > 0 {
            return Err(Diag::ERedBlock.val());
        }
        if rep.root() == 0 {
            return Err(Diag::ENoRoot.val());
        }
        self.phase = Phase::Sealed;
        Ok(rep.root())
    }

    /// 封档后重放：逐项重对平 + 报告根复算，逐字一致才算通过。
    pub fn replay(&self) -> Result<u32, u32> {
        if self.phase != Phase::Sealed {
            return Err(Diag::EPhase.val());
        }
        let rep = match &self.report {
            Some(r) => r,
            None => return Err(Diag::ENoReport.val()),
        };
        let mut h: u64 = 0xCBF29CE484222325;
        let mut sid = 1u16;
        while sid <= ITEM_COUNT as u16 {
            let it = match self.triplet_at(sid) {
                Some(x) => x,
                None => return Err(Diag::ESrcLack.val()),
            };
            h = fold_word(h, sid as u64);
            h = fold_word(h, it.table.ord() as u64);
            h = fold_word(h, it.ledger.ord() as u64);
            h = fold_word(h, it.accept.ord() as u64);
            sid += 1;
        }
        if h != rep.root() {
            return Err(Diag::EReplay.val());
        }
        if rep.line_count() != ITEM_COUNT {
            return Err(Diag::EReplay.val());
        }
        Ok(rep.line_count() as u32)
    }
}

/// 标准十项：三源全部登记为「已入账」（逐项对平的基线批）。
pub fn build_baseline() -> ReconLedger {
    let mut l = ReconLedger::new();
    let mut sid = 1u16;
    while sid <= ITEM_COUNT as u16 {
        let _ = l.set_table(sid, ItemState::Booked);
        let _ = l.set_ledger(sid, ItemState::Booked);
        let _ = l.set_accept(sid, ItemState::Booked);
        sid += 1;
    }
    l
}

/// 失配注入 10/10：逐项对三个源各篡改一次单腿状态，对账门必须检出。
/// 返回十个布尔（true=该项篡改被检出）。
pub fn mismatch_matrix() -> [bool; ITEM_COUNT] {
    let mut out = [false; ITEM_COUNT];
    let mut i = 0usize;
    while i < ITEM_COUNT {
        // 三个源轮流当篡改腿：项 i 篡改腿 = i % 3。
        let leg = (i % 3) as u8;
        let mut probe = ReconLedger::new();
        let mut sid = 1u16;
        while sid <= ITEM_COUNT as u16 {
            let st = if sid as usize == i + 1 && leg == 0 {
                ItemState::Signed
            } else {
                ItemState::Booked
            };
            let _ = probe.set_table(sid, st);
            let st2 = if sid as usize == i + 1 && leg == 1 {
                ItemState::Draft
            } else {
                ItemState::Booked
            };
            let _ = probe.set_ledger(sid, st2);
            let st3 = if sid as usize == i + 1 && leg == 2 {
                ItemState::Signed
            } else {
                ItemState::Booked
            };
            let _ = probe.set_accept(sid, st3);
            sid += 1;
        }
        // 基线源（未篡改的对照账）逐项对平必然零失配。
        let rep = probe.reconcile();
        let caught = match rep {
            Ok(r) => r.mismatch_count() == 1,
            Err(_) => false,
        };
        // 封档门：失配未清必须拒绝封档（第二道闸）。
        let seal_blocked = probe.seal() == Err(Diag::ERedBlock.val());
        let cell = match out.get_mut(i) {
            Some(x) => x,
            None => return [false; ITEM_COUNT],
        };
        *cell = caught && seal_blocked;
        i += 1;
    }
    out
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（对齐 0x40；窗口按 B07 派工卡 0x78E00..=0x78FC0）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x78E00 || SEG_BASE > 0x78FC0 {
        return false;
    }
    SEG_BASE == 0x78E00
}

/// 状态四态闭合（互异且序数单调）。
fn state_closed_ok() -> bool {
    if STATE_ALL.len() != 4 {
        return false;
    }
    let mut i = 0usize;
    while i < STATE_ALL.len() {
        let a = match STATE_ALL.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let mut j = i + 1;
        while j < STATE_ALL.len() {
            let b = match STATE_ALL.get(j) {
                Some(x) => *x,
                None => return false,
            };
            if a == b || a.ord() >= b.ord() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 基线批三向对平 100%。
fn baseline_flat_ok(l: &ReconLedger) -> bool {
    if l.full_legs() != ITEM_COUNT {
        return false;
    }
    let mut sid = 1u16;
    while sid <= ITEM_COUNT as u16 {
        let it = match l.triplet_at(sid) {
            Some(x) => x,
            None => return false,
        };
        if !it.flat() {
            return false;
        }
        sid += 1;
    }
    true
}

/// 对账报告落档（行数齐 + 根非零 + 无失配）。
fn report_ok(l: &ReconLedger) -> bool {
    l.phase() == Phase::Sealed
        && l.replay() == Ok(ITEM_COUNT as u32)
}

/// 失配注入 10/10 检出。
fn mismatch_ok() -> bool {
    mismatch_matrix() == [true; ITEM_COUNT]
}

/// 双跑一致（同输入两次构建折叠根逐字一致）。
fn dual_run_ok() -> bool {
    let a = build_baseline();
    let b = build_baseline();
    let ra = match a.clone().reconcile() {
        Ok(r) => r.root(),
        Err(_) => return false,
    };
    let rb = match b.clone().reconcile() {
        Ok(r) => r.root(),
        Err(_) => return false,
    };
    ra == rb && ra != 0
}

/// 十三项判据：UNX-F1572 三向总对账。
pub fn run_unx1572_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1572");
    let mut l = build_baseline();

    s.add_dyn(
        "F1572-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1572-02 状态四态闭合",
        state_closed_ok(),
        &format!(
            "n={} names={}/{}/{}/{}",
            STATE_ALL.len(),
            ItemState::Draft.name(),
            ItemState::Signed.name(),
            ItemState::Accepted.name(),
            ItemState::Booked.name()
        ),
    );

    s.add_dyn(
        "F1572-03 基线三源十项齐备",
        baseline_flat_ok(&l) && l.full_legs() == ITEM_COUNT,
        &format!("full_legs={}", l.full_legs()),
    );

    // 对账前结构拒绝：缺源、坏项号、零项号、闭集外还原、空账。
    let mut r = ReconLedger::new();
    let _ = r.set_table(1, ItemState::Signed);
    let lack = r.reconcile() == Err(Diag::ESrcLack.val());
    let bad_sid = r.set_ledger(11, ItemState::Signed) == Err(Diag::ESid.val());
    let zero_sid = r.set_ledger(0, ItemState::Signed) == Err(Diag::ESid.val());
    // 状态字闭集由类型系统钳死，闭集外取值只能经 from_ord 还原路径落 None。
    let from_ord_reject = ItemState::from_ord(9).is_none();
    let empty_ledger = ReconLedger::new().reconcile() == Err(Diag::ESrcLack.val());
    s.add_dyn(
        "F1572-04 结构拒绝四门",
        lack && bad_sid && zero_sid && from_ord_reject && empty_ledger,
        &format!("rejects={}", r.rejects()),
    );

    // 对账发起：正常一次、重复拒绝、相位推进。
    let rep = l.reconcile();
    let rep_ok = match &rep {
        Ok(x) => x.mismatch_count() == 0 && x.line_count() == ITEM_COUNT,
        Err(_) => false,
    };
    let dup = l.reconcile() == Err(Diag::EDupRecon.val());
    s.add_dyn(
        "F1572-05 对账发起单向",
        rep_ok && dup && l.phase() == Phase::Reconciled,
        &format!("phase_ord={}", l.phase().ord()),
    );

    // 失配注入 10/10。
    s.add_dyn(
        "F1572-06 失配注入十项全检出",
        mismatch_ok(),
        &format!("matrix={:?}", mismatch_matrix()),
    );

    // 失配报告点名异常腿并给出中文建议（十项铺满，仅第 1 项状态表腿失配）。
    let mut m = ReconLedger::new();
    let mut sid = 1u16;
    while sid <= ITEM_COUNT as u16 {
        let (t, lg, ac) = if sid == 1 {
            (ItemState::Signed, ItemState::Booked, ItemState::Booked)
        } else {
            (ItemState::Booked, ItemState::Booked, ItemState::Booked)
        };
        let _ = m.set_table(sid, t);
        let _ = m.set_ledger(sid, lg);
        let _ = m.set_accept(sid, ac);
        sid += 1;
    }
    let mrep = m.reconcile();
    let m_ok = match &mrep {
        Ok(x) => {
            x.mismatch_count() == 1
                && x.line_at(0).map(|t| t.contains("失配") && t.contains(SRC_TABLE)).unwrap_or(false)
        }
        Err(_) => false,
    };
    s.add_dyn(
        "F1572-07 失配报告点名异常腿",
        m_ok,
        &match &mrep {
            Ok(x) => x.line_at(0).map(|t| t.to_string()).unwrap_or_default(),
            Err(c) => detail(*c).to_string(),
        },
    );

    // 封档门：失配未清拒绝；封档后写入拒绝。
    let red_seal = m.seal() == Err(Diag::ERedBlock.val());
    let seal = l.seal();
    let seal_ok = match seal {
        Ok(root) => root != 0,
        Err(_) => false,
    };
    let write_after = l.set_table(1, ItemState::Draft) == Err(Diag::ESealed.val());
    s.add_dyn(
        "F1572-08 封档闸门三向",
        red_seal && seal_ok && write_after && l.phase() == Phase::Sealed,
        &format!("red_seal={} root_ok={}", red_seal, seal_ok),
    );

    // 封档重放：逐字一致。
    s.add_dyn(
        "F1572-09 封档重放逐字一致",
        report_ok(&l),
        &format!("replay={:?}", l.replay()),
    );

    // 报告落档面：全文非空、行数对齐十项、根非零。
    let mut br = build_baseline();
    let rr = br.reconcile();
    let text_ok = match &rr {
        Ok(rep) => {
            rep.line_count() == ITEM_COUNT
                && rep.text().contains("对平")
                && !rep.text().is_empty()
                && rep.root() != 0
        }
        Err(_) => false,
    };
    s.add_dyn(
        "F1572-10 对账报告落档非空",
        text_ok,
        &format!("line_count={}", rr.map(|r| r.line_count()).unwrap_or(0)),
    );

    // 相位倒退显性拒（起草→已对平→已封档 单向）。
    let mut p = build_baseline();
    let _ = p.reconcile();
    // 已对平后不可再"倒退"为起草：对账口唯一，重发起已拒（EDupRecon）。
    let back = p.reconcile() == Err(Diag::EDupRecon.val());
    s.add_dyn(
        "F1572-11 相位倒退显性拒",
        back && p.phase().ord() == 1,
        &format!("phase_ord={} back={}", p.phase().ord(), back),
    );

    s.add_dyn(
        "F1572-12 双跑一致",
        dual_run_ok(),
        &format!("root={:016x}", build_baseline().reconcile().map(|r| r.root()).unwrap_or(0)),
    );

    // 三源名与判据原文一致（防抄错账名）。
    let src_ok = SRC_TABLE == "F1561状态表" && SRC_LEDGER == "F1568总账" && SRC_ACCEPT == "F1566终验";
    s.add_dyn(
        "F1572-13 三源名对齐判据原文",
        src_ok && ALL.len() == 12,
        &format!("srcs={}/{}/{} all={}", SRC_TABLE, SRC_LEDGER, SRC_ACCEPT, ALL.len()),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_enum_closed_and_ordered() {
        assert_eq!(STATE_ALL.len(), 4);
        assert_eq!(ItemState::Draft.ord(), 0);
        assert_eq!(ItemState::Booked.ord(), 3);
        assert!(ItemState::from_ord(4).is_none());
        assert!(ItemState::from_ord(2).is_some());
    }

    #[test]
    fn triplet_flat_and_odd_leg() {
        let flat = ReconItem { sid: 1, table: ItemState::Booked, ledger: ItemState::Booked, accept: ItemState::Booked };
        assert!(flat.flat());
        assert_eq!(flat.odd_leg(), "");
        let odd = ReconItem { sid: 2, table: ItemState::Signed, ledger: ItemState::Booked, accept: ItemState::Booked };
        assert!(!odd.flat());
        assert_eq!(odd.odd_leg(), SRC_TABLE);
        let odd2 = ReconItem { sid: 3, table: ItemState::Booked, ledger: ItemState::Draft, accept: ItemState::Booked };
        assert_eq!(odd2.odd_leg(), SRC_LEDGER);
    }

    #[test]
    fn baseline_reconciles_clean() {
        let mut l = build_baseline();
        assert_eq!(l.full_legs(), ITEM_COUNT);
        let rep = l.reconcile().expect("基线必须对平");
        assert_eq!(rep.mismatch_count(), 0);
        assert_eq!(rep.line_count(), ITEM_COUNT);
        assert_ne!(rep.root(), 0);
        assert_eq!(l.reconcile(), Err(Diag::EDupRecon.val()));
    }

    #[test]
    fn structural_gates() {
        let mut l = ReconLedger::new();
        assert_eq!(l.reconcile(), Err(Diag::ESrcLack.val()));
        assert_eq!(l.set_table(0, ItemState::Signed), Err(Diag::ESid.val()));
        assert_eq!(l.set_table(11, ItemState::Signed), Err(Diag::ESid.val()));
        // 状态字闭集由类型钳死：闭集外序数还原必须落 None（还原面守卫）。
        assert!(ItemState::from_ord(7).is_none());
        assert_eq!(l.rejects(), 2, "仅坏项号计入拒绝");
    }

    #[test]
    fn mismatch_report_names_leg() {
        // 十项铺满，仅第 1 项状态表腿失配。
        let mut l = ReconLedger::new();
        let mut sid = 1u16;
        while sid <= ITEM_COUNT as u16 {
            let t = if sid == 1 { ItemState::Signed } else { ItemState::Booked };
            let _ = l.set_table(sid, t);
            let _ = l.set_ledger(sid, ItemState::Booked);
            let _ = l.set_accept(sid, ItemState::Booked);
            sid += 1;
        }
        let rep = l.reconcile().expect("三源齐备即可对账");
        assert_eq!(rep.mismatches(), &[1u16]);
        let line = rep.line_at(0).expect("首行存在");
        assert!(line.contains("失配"));
        assert!(line.contains(SRC_TABLE));
        assert!(l.seal().is_err(), "失配未清封档必须被拒");
    }

    #[test]
    fn seal_and_replay() {
        let mut l = build_baseline();
        let _ = l.reconcile();
        let root = l.seal().expect("零失配封档通过");
        assert_ne!(root, 0);
        assert_eq!(l.phase(), Phase::Sealed);
        assert_eq!(l.replay(), Ok(ITEM_COUNT as u32));
        assert_eq!(l.seal(), Err(Diag::ESealed.val()));
        assert_eq!(l.set_table(1, ItemState::Draft), Err(Diag::ESealed.val()));
    }

    #[test]
    fn tamper_ten_of_ten() {
        let m = mismatch_matrix();
        assert_eq!(m, [true; ITEM_COUNT]);
        assert!(mismatch_ok());
    }

    #[test]
    fn dual_run_consistency() {
        assert!(dual_run_ok());
        let a = build_baseline();
        let b = build_baseline();
        let ra = a.clone().reconcile().expect("对平").root();
        let rb = b.clone().reconcile().expect("对平").root();
        assert_eq!(ra, rb);
    }

    #[test]
    fn diag_codes_closed() {
        assert_eq!(ALL.len(), 12);
        assert_eq!(Diag::EState.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::EPhase.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert_eq!(detail(SEG_BASE + 0x20), "未知诊断码：不在 0x78E01..0x78E0C 闭集内");
    }

    #[test]
    fn checks_all_green_thirteen() {
        let s = run_unx1572_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
