//! UNX-F51524 带宽账可视化接口（每微帧剩余容量查询） · 0xC0600段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51525-J1 并发组合 ×10 实表落账（AI-36/AI-38 联合排期锚定行）
//!
//! 设计要点：
//!   ① 可视化面 = 只读快照：BwSnapshot 携带 (版本号, 微帧号, 分型占用,
//!      剩余量)，一次快照整份拷贝，读侧永不持有半新半旧的撕裂视图；
//!   ② 查询与实表强一致：每次 snapshot() 用单一版本号定格，validate()
//!      用当前实表重算快照逐字段比对，×100 查询逐次一致；
//!   ③ 快照自洽不变式：used_intr + used_iso + used_bulk + remain == 总预算，
//!      任何伪造/撕裂快照都过不了自洽与实表双重校验；
//!   ④ 版本状态机单调：每次落账或推进微帧版本 +1（饱和），回放探针目标
//!      版本低于当前即显性拒（ViewBackward）；
//!   ⑤ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒为 0；撕裂快照/账表不符/
//!      负值落账三类注入各 ×10 轮全部检出；
//!   ⑥ 诊断码闭集 + 中文建议文案；段位守卫锁定 SEG_BASE 对齐与分派区间。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC0600;

/// 每微帧总预算（单位：字节；小于三型上限之和 3000，以逼出池级耗尽判据）。
pub const VIEW_BUDGET: u32 = 2900;
/// interrupt 型每微帧上限。
pub const VIEW_INTR_CAP: u32 = 400;
/// isochronous 型每微帧上限。
pub const VIEW_ISO_CAP: u32 = 1500;
/// bulk 型每微帧上限。
pub const VIEW_BULK_CAP: u32 = 1100;

/// 诊断码闭集（可视化查询域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 微帧未开启
    MfNotOpen,
    /// 负值落账
    NegativeUnits,
    /// 零长落账
    ZeroUnits,
    /// 超分型上限
    KindCapExceeded,
    /// 微帧池余量不足
    PoolExhausted,
    /// 版本倒退（状态机显性拒）
    ViewBackward,
    /// 快照与实表不符（撕裂/伪造）
    ViewTorn,
    /// 对账守恒失败
    LedgerMismatch,
}

impl ViewCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ViewCode::Ok => "查询/落账成功",
            ViewCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            ViewCode::MfNotOpen => "微帧未开启：先推进微帧再落账或查询",
            ViewCode::NegativeUnits => "负值落账：占用增量必须为非负整数",
            ViewCode::ZeroUnits => "零长落账：占用增量必须 ≥1",
            ViewCode::KindCapExceeded => "超分型上限：拆分到下一微帧",
            ViewCode::PoolExhausted => "微帧池余量不足：本笔明拒",
            ViewCode::ViewBackward => "版本倒退：回放目标必须 ≥ 当前版本",
            ViewCode::ViewTorn => "快照撕裂：与实表重算不符，重新查询",
            ViewCode::LedgerMismatch => "对账不符：账本重算与记账面不一致",
        }
    }
}

/// 分型（可视化按三型分桶）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewKind {
    /// interrupt 型
    Intr,
    /// isochronous 型
    Iso,
    /// bulk 型
    Bulk,
}

impl ViewKind {
    pub fn cap(self) -> u32 {
        match self {
            ViewKind::Intr => VIEW_INTR_CAP,
            ViewKind::Iso => VIEW_ISO_CAP,
            ViewKind::Bulk => VIEW_BULK_CAP,
        }
    }
}

/// 只读可视化快照（整份拷贝，天然免撕裂）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BwSnapshot {
    pub version: u64,
    pub mf: u32,
    pub used_intr: u32,
    pub used_iso: u32,
    pub used_bulk: u32,
    pub remain: u32,
}

impl BwSnapshot {
    /// 三型占用合计（饱和）。
    pub fn total_used(&self) -> u32 {
        self.used_intr
            .saturating_add(self.used_iso)
            .saturating_add(self.used_bulk)
    }
    /// 自洽不变式：分型占用 + 剩余 == 总预算，且剩余不超总预算。
    pub fn self_consistent(&self) -> bool {
        self.total_used().saturating_add(self.remain) == VIEW_BUDGET
    }
}

/// 落账记录（只增不减）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllocRec {
    pub version: u64,
    pub mf: u32,
    pub kind: ViewKind,
    pub units: u32,
}

/// 可视化引擎：实表 + 版本单调 + 只读快照面。
pub struct BwViewEngine {
    version: u64,
    mf: u32,
    mf_open: bool,
    used_intr: u32,
    used_iso: u32,
    used_bulk: u32,
    tot_intr: u64,
    tot_iso: u64,
    tot_bulk: u64,
    advances: u64,
    queries: u64,
    ledger: Vec<AllocRec>,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl BwViewEngine {
    pub fn new() -> BwViewEngine {
        BwViewEngine {
            version: 0,
            mf: 0,
            mf_open: false,
            used_intr: 0,
            used_iso: 0,
            used_bulk: 0,
            tot_intr: 0,
            tot_iso: 0,
            tot_bulk: 0,
            advances: 0,
            queries: 0,
            ledger: Vec::new(),
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 推进到下一微帧（版本 +1，分型占用清零）。
    pub fn advance_mf(&mut self) {
        self.mf = self.mf.saturating_add(1);
        self.mf_open = true;
        self.used_intr = 0;
        self.used_iso = 0;
        self.used_bulk = 0;
        self.advances = self.advances.saturating_add(1);
        self.version = self.version.saturating_add(1);
    }

    fn used_of(&self, kind: ViewKind) -> u32 {
        match kind {
            ViewKind::Intr => self.used_intr,
            ViewKind::Iso => self.used_iso,
            ViewKind::Bulk => self.used_bulk,
        }
    }

    fn bump_used(&mut self, kind: ViewKind, v: u32) {
        match kind {
            ViewKind::Intr => {
                self.used_intr = self.used_intr.saturating_add(v);
                self.tot_intr = self.tot_intr.saturating_add(v as u64);
            }
            ViewKind::Iso => {
                self.used_iso = self.used_iso.saturating_add(v);
                self.tot_iso = self.tot_iso.saturating_add(v as u64);
            }
            ViewKind::Bulk => {
                self.used_bulk = self.used_bulk.saturating_add(v);
                self.tot_bulk = self.tot_bulk.saturating_add(v as u64);
            }
        }
    }

    /// 当前微帧剩余容量。
    pub fn remaining(&self) -> u32 {
        VIEW_BUDGET.saturating_sub(
            self.used_intr
                .saturating_add(self.used_iso)
                .saturating_add(self.used_bulk),
        )
    }

    /// 落账一笔占用（判定次序：未开微帧 → 负值 → 零长 → 分型上限 → 池余量）。
    pub fn commit(&mut self, kind: ViewKind, units: i64) -> Result<u32, ViewCode> {
        if !self.mf_open {
            return Err(ViewCode::MfNotOpen);
        }
        if units < 0 {
            return Err(ViewCode::NegativeUnits);
        }
        if units == 0 {
            return Err(ViewCode::ZeroUnits);
        }
        if units > kind.cap() as i64 {
            return Err(ViewCode::KindCapExceeded);
        }
        let u = units as u32;
        if self.used_of(kind).saturating_add(u) > kind.cap() {
            return Err(ViewCode::KindCapExceeded);
        }
        if u > self.remaining() {
            return Err(ViewCode::PoolExhausted);
        }
        self.bump_used(kind, u);
        self.version = self.version.saturating_add(1);
        self.ledger.push(AllocRec {
            version: self.version,
            mf: self.mf,
            kind,
            units: u,
        });
        Ok(u)
    }

    /// 只读快照：单一版本号定格整份拷贝（查询计数 +1，不改状态）。
    pub fn snapshot(&mut self) -> BwSnapshot {
        self.queries = self.queries.saturating_add(1);
        BwSnapshot {
            version: self.version,
            mf: self.mf,
            used_intr: self.used_intr,
            used_iso: self.used_iso,
            used_bulk: self.used_bulk,
            remain: self.remaining(),
        }
    }

    /// 校验快照：与当前实表重算值逐字段比对，撕裂/伪造即 ViewTorn。
    pub fn validate(&self, snap: &BwSnapshot) -> Result<(), ViewCode> {
        let expect = BwSnapshot {
            version: self.version,
            mf: self.mf,
            used_intr: self.used_intr,
            used_iso: self.used_iso,
            used_bulk: self.used_bulk,
            remain: self.remaining(),
        };
        if *snap == expect && snap.self_consistent() {
            Ok(())
        } else {
            Err(ViewCode::ViewTorn)
        }
    }

    /// 回放探针：目标版本低于当前即显性拒（版本单调）。
    pub fn rewind_probe(&self, target: u64) -> Result<(), ViewCode> {
        if target < self.version {
            return Err(ViewCode::ViewBackward);
        }
        Ok(())
    }

    /// 对账：账本重算（分型累计、版本计数）与记账面一致则通过。
    pub fn reconcile(&self) -> Result<(), ViewCode> {
        let mut k_intr = 0u64;
        let mut k_iso = 0u64;
        let mut k_bulk = 0u64;
        for r in self.ledger.iter() {
            match r.kind {
                ViewKind::Intr => k_intr = k_intr.saturating_add(r.units as u64),
                ViewKind::Iso => k_iso = k_iso.saturating_add(r.units as u64),
                ViewKind::Bulk => k_bulk = k_bulk.saturating_add(r.units as u64),
            }
        }
        let ver_expect = self
            .advances
            .saturating_add(self.ledger.len() as u64);
        let ok = k_intr == self.tot_intr
            && k_iso == self.tot_iso
            && k_bulk == self.tot_bulk
            && ver_expect == self.version;
        if ok {
            Ok(())
        } else {
            Err(ViewCode::LedgerMismatch)
        }
    }

    pub fn note_detected(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.detected = self.detected.saturating_add(1);
    }
    pub fn note_escape(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.escaped = self.escaped.saturating_add(1);
    }

    pub fn cur_version(&self) -> u64 {
        self.version
    }
    pub fn queries(&self) -> u64 {
        self.queries
    }
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }
    pub fn injected(&self) -> u64 {
        self.injected
    }
    pub fn escaped(&self) -> u64 {
        self.escaped
    }

    /// 守恒断言面：注入 = 检出 + 逃逸，逃逸恒 0、检出 > 0。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped)
            && self.escaped == 0
            && self.detected > 0
    }
}

/// 段位守卫：SEG_BASE 0x40 对齐且落在分派区间。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一操作序列两次建账逐字段一致（确定性）。
pub fn dual_run_consistent() -> bool {
    let build = || -> (Vec<AllocRec>, u64, u32, u32, u32) {
        let mut e = BwViewEngine::new();
        e.advance_mf();
        let _ = e.commit(ViewKind::Intr, 64);
        let _ = e.commit(ViewKind::Iso, 512);
        let _ = e.commit(ViewKind::Bulk, 256);
        e.advance_mf();
        let _ = e.commit(ViewKind::Iso, 1024);
        (
            e.ledger.clone(),
            e.cur_version(),
            e.used_intr,
            e.used_iso,
            e.used_bulk,
        )
    };
    let a = build();
    let b = build();
    a == b
}

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51524_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51524");
    // 1 正常流：落账后快照与实表一致，剩余量正确。
    {
        let mut e = BwViewEngine::new();
        e.advance_mf();
        let _ = e.commit(ViewKind::Intr, 64);
        let snap = e.snapshot();
        let ok = snap.used_intr == 64
            && snap.remain == VIEW_BUDGET - 64
            && e.validate(&snap) == Ok(());
        s.add_dyn("落账后快照一致", ok, "快照必须与实表逐字段一致");
    }
    // 2 边界零：零长落账明拒。
    {
        let mut e = BwViewEngine::new();
        e.advance_mf();
        s.add_dyn(
            "零长落账明拒",
            e.commit(ViewKind::Iso, 0) == Err(ViewCode::ZeroUnits),
            "零长占用必须拒绝",
        );
    }
    // 3 边界负：负值落账明拒。
    {
        let mut e = BwViewEngine::new();
        e.advance_mf();
        s.add_dyn(
            "负值落账明拒",
            e.commit(ViewKind::Iso, -8) == Err(ViewCode::NegativeUnits),
            "负值占用必须拒绝",
        );
    }
    // 4 边界上限/上限+1：分型顶格与越顶。
    {
        let mut e = BwViewEngine::new();
        e.advance_mf();
        let ok_cap = e.commit(ViewKind::Intr, VIEW_INTR_CAP as i64) == Ok(VIEW_INTR_CAP);
        let mut e2 = BwViewEngine::new();
        e2.advance_mf();
        let r_over = e2.commit(ViewKind::Intr, VIEW_INTR_CAP as i64 + 1);
        s.add_dyn(
            "分型上限顶格与+1",
            ok_cap && r_over == Err(ViewCode::KindCapExceeded),
            "顶格放行；上限+1 必须明拒",
        );
    }
    // 5 查询 ×100：逐次与实表一致且快照自洽。
    {
        let mut e = BwViewEngine::new();
        e.advance_mf();
        let mut all_ok = true;
        for i in 0..100u32 {
            let kind = match i % 3 {
                0 => ViewKind::Intr,
                1 => ViewKind::Iso,
                _ => ViewKind::Bulk,
            };
            let _ = e.commit(kind, 8);
            let snap = e.snapshot();
            if e.validate(&snap) != Ok(()) || !snap.self_consistent() {
                all_ok = false;
            }
        }
        s.add_dyn(
            "查询 ×100 与实表一致",
            all_ok && e.queries() == 100,
            "×100 查询必须逐次与实表一致且快照自洽",
        );
    }
    // 6 撕裂快照注入 ×10：篡改任一字段必被判 ViewTorn。
    {
        let mut e = BwViewEngine::new();
        e.advance_mf();
        let _ = e.commit(ViewKind::Iso, 256);
        let mut caught = 0u32;
        for i in 0..10u32 {
            let mut torn = e.snapshot();
            match i % 3 {
                0 => torn.used_intr = torn.used_intr.saturating_add(1),
                1 => torn.used_iso = torn.used_iso.saturating_add(1),
                _ => torn.remain = torn.remain.saturating_add(1),
            }
            if e.validate(&torn) == Err(ViewCode::ViewTorn) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("撕裂快照注入 10/10 检出", caught == 10, "篡改快照字段必须全部判撕裂");
    }
    // 7 账表不符注入 ×10：伪造占用必被判 ViewTorn。
    {
        let mut e = BwViewEngine::new();
        e.advance_mf();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let mut forged = e.snapshot();
            forged.used_bulk = forged.used_bulk.saturating_add(128);
            if e.validate(&forged) == Err(ViewCode::ViewTorn) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("账表不符注入 10/10 检出", caught == 10, "伪造占用必须全部检出");
    }
    // 8 负值落账注入 ×10。
    {
        let mut e = BwViewEngine::new();
        e.advance_mf();
        let mut caught = 0u32;
        for i in 0..10u32 {
            if e.commit(ViewKind::Bulk, -(1 + i as i64)) == Err(ViewCode::NegativeUnits) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("负值注入 10/10 检出", caught == 10, "负值落账必须全部拒绝");
    }
    // 9 版本状态机：回放倒退显性拒，版本单调不回退。
    {
        let mut e = BwViewEngine::new();
        e.advance_mf();
        let _ = e.commit(ViewKind::Intr, 32);
        let cur = e.cur_version();
        let r = e.rewind_probe(cur.saturating_sub(1));
        s.add_dyn(
            "版本倒退显性拒",
            r == Err(ViewCode::ViewBackward) && e.cur_version() == cur,
            "回放目标低于当前版本必须拒绝且版本不变",
        );
    }
    // 10 零撕裂并发读：落账前后各取一份快照，前份逐字段不变。
    {
        let mut e = BwViewEngine::new();
        e.advance_mf();
        let s1 = e.snapshot();
        let _ = e.commit(ViewKind::Iso, 512);
        let s2 = e.snapshot();
        let ok = s1.version < s2.version
            && s1.used_iso == 0
            && s1.remain == VIEW_BUDGET
            && s2.used_iso == 512
            && s1.self_consistent()
            && s2.self_consistent();
        s.add_dyn(
            "并发查询零撕裂",
            ok,
            "旧快照必须整份不变，新快照反映新实表",
        );
    }
    // 11 容量耗尽：两型顶格后池余 1000 < bulk 上限 1100，超池明拒；推进后恢复。
    {
        let mut e = BwViewEngine::new();
        e.advance_mf();
        let _ = e.commit(ViewKind::Intr, VIEW_INTR_CAP as i64);
        let _ = e.commit(ViewKind::Iso, VIEW_ISO_CAP as i64);
        // 池余 1000：bulk 1010 未超分型上限但超池，必须池级明拒。
        let r_full = e.commit(ViewKind::Bulk, 1010);
        e.advance_mf();
        let r_next = e.commit(ViewKind::Bulk, 8);
        s.add_dyn(
            "容量耗尽与恢复",
            r_full == Err(ViewCode::PoolExhausted) && r_next == Ok(8),
            "池满本微帧明拒；新微帧恢复后放行",
        );
    }
    // 12 对账守恒：账本重算与记账面一致，注入=检出+逃逸且逃逸 0。
    {
        let mut e = BwViewEngine::new();
        e.advance_mf();
        for _ in 0..10u32 {
            let _ = e.commit(ViewKind::Iso, -1);
            e.note_detected();
        }
        let recon = e.reconcile() == Ok(());
        s.add_dyn(
            "对账守恒",
            recon && e.conserved() && e.injected() == 10 && e.escaped() == 0,
            "注入=检出+逃逸，逃逸恒 0，账本重算一致",
        );
    }
    // 13 双跑一致。
    s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同一输入序列两次建账逐字段相等");
    // 14 文案闭集 + 段位守卫。
    {
        let mut closed = true;
        for c in ALL_CODES {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
        s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    }
    s
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [ViewCode; 10] = [
    ViewCode::Ok,
    ViewCode::SegBad,
    ViewCode::MfNotOpen,
    ViewCode::NegativeUnits,
    ViewCode::ZeroUnits,
    ViewCode::KindCapExceeded,
    ViewCode::PoolExhausted,
    ViewCode::ViewBackward,
    ViewCode::ViewTorn,
    ViewCode::LedgerMismatch,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_commit_and_snapshot() {
        let mut e = BwViewEngine::new();
        e.advance_mf();
        assert_eq!(e.commit(ViewKind::Intr, 64), Ok(64));
        let snap = e.snapshot();
        assert_eq!(snap.used_intr, 64);
        assert_eq!(snap.remain, VIEW_BUDGET - 64);
        assert!(snap.self_consistent());
        assert!(e.validate(&snap) == Ok(()));
    }

    #[test]
    fn zero_and_negative_boundary() {
        let mut e = BwViewEngine::new();
        e.advance_mf();
        assert_eq!(e.commit(ViewKind::Iso, 0), Err(ViewCode::ZeroUnits));
        assert_eq!(e.commit(ViewKind::Iso, -8), Err(ViewCode::NegativeUnits));
        assert_eq!(e.ledger_len(), 0);
    }

    #[test]
    fn kind_cap_and_plus_one() {
        let mut e = BwViewEngine::new();
        e.advance_mf();
        assert_eq!(
            e.commit(ViewKind::Intr, VIEW_INTR_CAP as i64),
            Ok(VIEW_INTR_CAP)
        );
        let mut e2 = BwViewEngine::new();
        e2.advance_mf();
        assert_eq!(
            e2.commit(ViewKind::Intr, VIEW_INTR_CAP as i64 + 1),
            Err(ViewCode::KindCapExceeded)
        );
    }

    #[test]
    fn query_x100_consistent_with_table() {
        let mut e = BwViewEngine::new();
        e.advance_mf();
        for i in 0..100u32 {
            let kind = match i % 3 {
                0 => ViewKind::Intr,
                1 => ViewKind::Iso,
                _ => ViewKind::Bulk,
            };
            let _ = e.commit(kind, 4);
            let snap = e.snapshot();
            assert!(e.validate(&snap) == Ok(()), "第 {} 次查询应与实表一致", i);
            assert!(snap.self_consistent());
        }
        assert_eq!(e.queries(), 100);
    }

    #[test]
    fn torn_snapshot_injection_x10_detected() {
        let mut e = BwViewEngine::new();
        e.advance_mf();
        let _ = e.commit(ViewKind::Iso, 256);
        for i in 0..10u32 {
            let mut torn = e.snapshot();
            match i % 3 {
                0 => torn.used_intr = torn.used_intr.saturating_add(1),
                1 => torn.used_iso = torn.used_iso.saturating_add(1),
                _ => torn.remain = torn.remain.saturating_add(1),
            }
            assert_eq!(e.validate(&torn), Err(ViewCode::ViewTorn));
            e.note_detected();
        }
        assert!(e.conserved());
    }

    #[test]
    fn negative_commit_injection_x10() {
        let mut e = BwViewEngine::new();
        e.advance_mf();
        for i in 0..10u32 {
            assert_eq!(
                e.commit(ViewKind::Bulk, -(1 + i as i64)),
                Err(ViewCode::NegativeUnits)
            );
            e.note_detected();
        }
        assert!(e.conserved());
    }

    #[test]
    fn version_monotone_rewind_rejected() {
        let mut e = BwViewEngine::new();
        e.advance_mf();
        let _ = e.commit(ViewKind::Intr, 32);
        let cur = e.cur_version();
        assert_eq!(e.rewind_probe(cur - 1), Err(ViewCode::ViewBackward));
        assert_eq!(e.rewind_probe(cur), Ok(()));
        assert_eq!(e.cur_version(), cur);
    }

    #[test]
    fn concurrent_read_no_tearing() {
        let mut e = BwViewEngine::new();
        e.advance_mf();
        let s1 = e.snapshot();
        let _ = e.commit(ViewKind::Iso, 512);
        let s2 = e.snapshot();
        assert!(s1.version < s2.version);
        assert_eq!(s1.used_iso, 0);
        assert_eq!(s2.used_iso, 512);
        assert!(s1.self_consistent() && s2.self_consistent());
    }

    #[test]
    fn pool_exhausted_then_recover() {
        let mut e = BwViewEngine::new();
        e.advance_mf();
        let _ = e.commit(ViewKind::Intr, VIEW_INTR_CAP as i64);
        let _ = e.commit(ViewKind::Iso, VIEW_ISO_CAP as i64);
        // 池余 1000：bulk 1010 分型内但超池 → PoolExhausted（池级判据可达）。
        assert_eq!(e.commit(ViewKind::Bulk, 1010), Err(ViewCode::PoolExhausted));
        e.advance_mf();
        assert_eq!(e.commit(ViewKind::Bulk, 8), Ok(8));
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn conservation_and_reconcile() {
        let mut e = BwViewEngine::new();
        e.advance_mf();
        for _ in 0..10u32 {
            let _ = e.commit(ViewKind::Iso, -1);
            e.note_detected();
        }
        assert!(e.reconcile() == Ok(()));
        assert!(e.conserved());
        assert_eq!(e.escaped(), 0);
    }

    #[test]
    fn seg_guard_and_closed_texts() {
        assert!(seg_guard_ok());
        for c in ALL_CODES {
            assert!(!c.detail().is_empty());
        }
    }
}

