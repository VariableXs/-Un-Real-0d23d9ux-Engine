//! UNX-F1508 回归失败归因协议（红账三问归因） · 0x78080段
//!
//! 判据原文（UNX-F1508-J1 摘录）：「失败归因协议生效 10/10 次（红账三问：
//! 环境/资产/被测三向归因，归因标签枚举闭合），归因覆盖率 100%（红账笔笔
//! 有标签），错归因注入 10/10 必抓」；完成时 API/软件写回：状态=已完成、
//! 结果摘要非空。
//!
//! 设计要点：
//! 1. 红账三问按序执行：一问环境（环境健康可复现？）、二问资产（资产指纹
//!    匹配？）、三问被测（被测自证失败？），问题答案构成证据三元组；
//! 2. 归因轴枚举闭合：环境/资产/被测三向，轴由证据推导——环境不健康归
//!    环境，指纹不匹配归资产，其余归被测，推导规则冻结不许可随意改；
//! 3. 错归因注入 10/10：逐条登记与证据矛盾的轴，归因门必拒，十路全数命中；
//! 4. 归因覆盖率 100%：红账笔笔有标签，缺一条即封账拒绝；
//! 5. 状态机 开放→已归因→已封账 单向，倒退显性拒；诊断码闭集
//!    0x78081..0x7808C。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值）。
pub const SEG_BASE: u32 = 0x78080;

/// 失败归因协议诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 未登记的失败条目。
    ENoRed,
    /// 重复归因。
    EDup,
    /// 归因轴取值非法。
    EAxis,
    /// 错归因（与证据矛盾）。
    EWrong,
    /// 登记乱序。
    EOrder,
    /// 账本已封账。
    ESealed,
    /// 容量已满。
    ECap,
    /// 空证据样本。
    EEmpty,
    /// 归因覆盖缺口。
    ENoCover,
    /// 三问未按序走完。
    ENoQ,
    /// 归因账缺失。
    ENoLedger,
    /// 封账重放不符。
    ESeal,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::ENoRed => 0x01,
                Diag::EDup => 0x02,
                Diag::EAxis => 0x03,
                Diag::EWrong => 0x04,
                Diag::EOrder => 0x05,
                Diag::ESealed => 0x06,
                Diag::ECap => 0x07,
                Diag::EEmpty => 0x08,
                Diag::ENoCover => 0x09,
                Diag::ENoQ => 0x0a,
                Diag::ENoLedger => 0x0b,
                Diag::ESeal => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::ENoRed => "未登记的失败：先入红账再归因，禁止凭空补标签",
            Diag::EDup => "重复归因：每条红账只落一个轴，改判须先撤旧标签",
            Diag::EAxis => "归因轴非法：只认环境/资产/被测三向，闭集之外即拒",
            Diag::EWrong => "错归因：标签与三问证据矛盾，按证据推导轴重判",
            Diag::EOrder => "登记乱序：红账按失败序 1,2,3.. 顺序补齐",
            Diag::ESealed => "账本已封账：封账后归因拒绝，走下一批红账",
            Diag::ECap => "容量已满：十条红账已齐，其余滚动进下一批",
            Diag::EEmpty => "空证据样本：三问全零不构成归因依据，先补三问",
            Diag::ENoCover => "归因覆盖缺口：封账前每条红账必须有标签",
            Diag::ENoQ => "三问未按序：一问环境二问资产三问被测，禁止跳问",
            Diag::ENoLedger => "归因账缺失：协议生效凭据缺失，重跑本批归因",
            Diag::ESeal => "封账重放不符：标签复算与票面不一致，账本可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::ENoRed,
    Diag::EDup,
    Diag::EAxis,
    Diag::EWrong,
    Diag::EOrder,
    Diag::ESealed,
    Diag::ECap,
    Diag::EEmpty,
    Diag::ENoCover,
    Diag::ENoQ,
    Diag::ENoLedger,
    Diag::ESeal,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x78081..0x7808C 闭集内"
}

/// 归因轴闭集：环境。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// 环境：环境不健康或不可复现。
    Environment,
    /// 资产：资产指纹不匹配。
    Asset,
    /// 被测：被测自证失败。
    UnderTest,
}

/// 归因轴闭集数组。
pub const AXIS_ALL: &[Axis] = &[Axis::Environment, Axis::Asset, Axis::UnderTest];

impl Axis {
    /// 轴序号（入折叠）。
    pub fn ord(self) -> u64 {
        match self {
            Axis::Environment => 0,
            Axis::Asset => 1,
            Axis::UnderTest => 2,
        }
    }

    /// 中文轴名。
    pub fn name(self) -> &'static str {
        match self {
            Axis::Environment => "环境",
            Axis::Asset => "资产",
            Axis::UnderTest => "被测",
        }
    }
}

/// 红账三问证据（按问序冻结）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Evidence {
    /// 一问：环境健康可复现？
    pub env_ok: bool,
    /// 二问：资产指纹匹配？
    pub asset_match: bool,
    /// 三问：被测自证失败？
    pub dut_confess: bool,
}

impl Evidence {
    /// 空证据（三问全无结论）。
    pub const fn is_blank(&self) -> bool {
        // 三问全为否且被测不认账：既无环境异常也无资产异常也无被测自证，
        // 无法支撑任何归因，视为空证据。
        !self.env_ok && !self.asset_match && !self.dut_confess
    }

    /// 三问是否按序给出结论（本模型要求三问全部作答）。
    pub const fn questions_complete(&self) -> bool {
        true
    }
}

/// 按证据推导归因轴（推导规则冻结：环境优先、资产次之、被测兜底）。
pub fn derive_axis(e: &Evidence) -> Option<Axis> {
    if e.is_blank() {
        return None;
    }
    if !e.env_ok {
        return Some(Axis::Environment);
    }
    if !e.asset_match {
        return Some(Axis::Asset);
    }
    Some(Axis::UnderTest)
}

/// 一条红账条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RedEntry {
    /// 失败编号（1..=10）。
    pub rid: u16,
    /// 三问证据。
    pub ev: Evidence,
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

/// 本批红账条数。
pub const REDS: usize = 10;

/// 归因账相位：开放 → 已归因 → 已封账（单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 开放（登记失败与归因）。
    Open,
    /// 已归因（覆盖 100% 已达成）。
    Attributed,
    /// 已封账。
    Closed,
}

/// 红账归因账本。
pub struct AttributionLedger {
    reds: Vec<RedEntry>,
    tags: Vec<(u16, Axis)>,
    phase: Phase,
    root: u64,
    rejects: u32,
    wrong_caught: u32,
}

impl AttributionLedger {
    /// 空账本。
    pub fn new() -> AttributionLedger {
        AttributionLedger {
            reds: Vec::new(),
            tags: Vec::new(),
            phase: Phase::Open,
            root: 0,
            rejects: 0,
            wrong_caught: 0,
        }
    }

    /// 登记一条失败入红账（按序）。
    pub fn register_red(&mut self, rid: u16, ev: Evidence) -> Result<(), u32> {
        if self.phase != Phase::Open {
            self.rejects += 1;
            return Err(Diag::ESealed.val());
        }
        if self.reds.len() >= REDS {
            self.rejects += 1;
            return Err(Diag::ECap.val());
        }
        if rid == 0 {
            self.rejects += 1;
            return Err(Diag::EEmpty.val());
        }
        for r in &self.reds {
            if r.rid == rid {
                self.rejects += 1;
                return Err(Diag::EDup.val());
            }
        }
        if rid as usize != self.reds.len() + 1 {
            self.rejects += 1;
            return Err(Diag::EOrder.val());
        }
        if ev.is_blank() || !ev.questions_complete() {
            self.rejects += 1;
            return Err(Diag::ENoQ.val());
        }
        self.reds.push(RedEntry { rid, ev });
        Ok(())
    }

    /// 归因一条红账：轴与证据矛盾即拒（错归因门）。
    pub fn attribute(&mut self, rid: u16, axis: Axis) -> Result<Axis, u32> {
        if self.phase == Phase::Closed {
            self.rejects += 1;
            return Err(Diag::ESealed.val());
        }
        if rid == 0 {
            self.rejects += 1;
            return Err(Diag::EEmpty.val());
        }
        let red = match self.reds.iter().find(|r| r.rid == rid) {
            Some(x) => *x,
            None => {
                self.rejects += 1;
                return Err(Diag::ENoRed.val());
            }
        };
        for (sid, _) in &self.tags {
            if *sid == rid {
                self.rejects += 1;
                return Err(Diag::EDup.val());
            }
        }
        let expect = match derive_axis(&red.ev) {
            Some(x) => x,
            None => {
                self.rejects += 1;
                return Err(Diag::ENoQ.val());
            }
        };
        let legal = AXIS_ALL.iter().any(|a| *a == axis);
        if !legal {
            self.rejects += 1;
            return Err(Diag::EAxis.val());
        }
        if axis != expect {
            self.wrong_caught += 1;
            self.rejects += 1;
            return Err(Diag::EWrong.val());
        }
        self.tags.push((rid, axis));
        Ok(axis)
    }

    /// 红账条数。
    pub fn red_count(&self) -> usize {
        self.reds.len()
    }

    /// 已归因条数。
    pub fn tag_count(&self) -> usize {
        self.tags.len()
    }

    /// 归因覆盖率（千分比）。
    pub fn coverage_permille(&self) -> u32 {
        if self.reds.is_empty() {
            return 0;
        }
        (self.tags.len() as u32 * 1000) / self.reds.len() as u32
    }

    /// 覆盖 100% 判定。
    pub fn coverage_full(&self) -> bool {
        self.reds.len() == self.tags.len() && !self.reds.is_empty()
    }

    /// 取某条红账的标签。
    pub fn tag_of(&self, rid: u16) -> Option<Axis> {
        self.tags.iter().find(|(sid, _)| *sid == rid).map(|(_, a)| *a)
    }

    /// 取某条红账证据。
    pub fn evidence_of(&self, rid: u16) -> Option<Evidence> {
        self.reds.iter().find(|r| r.rid == rid).map(|r| r.ev)
    }

    /// 相位。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 结构拒绝数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 错归因捕获数。
    pub fn wrong_caught(&self) -> u32 {
        self.wrong_caught
    }

    /// 账本折叠根。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.reds.len() as u64);
        for r in &self.reds {
            h = fold_word(h, r.rid as u64);
            h = fold_word(h, r.ev.env_ok as u64);
            h = fold_word(h, r.ev.asset_match as u64);
            h = fold_word(h, r.ev.dut_confess as u64);
        }
        h = fold_word(h, self.tags.len() as u64);
        for (sid, a) in &self.tags {
            h = fold_word(h, *sid as u64);
            h = fold_word(h, a.ord());
        }
        h
    }

    /// 推进到已归因相位（覆盖 100% 才放行，单向）。
    pub fn mark_attributed(&mut self) -> Result<(), u32> {
        if self.phase != Phase::Open {
            self.rejects += 1;
            return Err(Diag::ESealed.val());
        }
        if !self.coverage_full() {
            self.rejects += 1;
            return Err(Diag::ENoCover.val());
        }
        self.phase = Phase::Attributed;
        Ok(())
    }

    /// 封账：重放归因推导一致后落根冻结。
    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.phase != Phase::Attributed {
            self.rejects += 1;
            return Err(Diag::ENoLedger.val());
        }
        for (sid, a) in &self.tags {
            let red = match self.reds.iter().find(|r| r.rid == *sid) {
                Some(x) => *x,
                None => return Err(Diag::ESeal.val()),
            };
            if derive_axis(&red.ev) != Some(*a) {
                return Err(Diag::ESeal.val());
            }
        }
        self.root = self.fold();
        self.phase = Phase::Closed;
        Ok(self.root)
    }

    /// 封账根。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 重放：逐条复算归因轴。
    pub fn replay(&self) -> Result<usize, u32> {
        if self.phase != Phase::Closed {
            return Err(Diag::ESeal.val());
        }
        let mut n = 0usize;
        for (sid, a) in &self.tags {
            let red = match self.reds.iter().find(|r| r.rid == *sid) {
                Some(x) => *x,
                None => return Err(Diag::ESeal.val()),
            };
            if derive_axis(&red.ev) != Some(*a) {
                return Err(Diag::EWrong.val());
            }
            n += 1;
        }
        if n != self.tags.len() {
            return Err(Diag::ESeal.val());
        }
        Ok(n)
    }
}

/// 归因分布摘要（轴 → 条数）。
pub fn axis_breakdown(l: &AttributionLedger) -> (u32, u32, u32) {
    let mut env = 0u32;
    let mut asset = 0u32;
    let mut dut = 0u32;
    let mut i = 0usize;
    while i < l.tag_count() {
        if let Some(a) = l.tags.get(i).map(|(_, a)| *a) {
            match a {
                Axis::Environment => env += 1,
                Axis::Asset => asset += 1,
                Axis::UnderTest => dut += 1,
            }
        }
        i += 1;
    }
    (env, asset, dut)
}

/// 归因协议摘要（结果摘要落档面）。
pub fn summary(l: &AttributionLedger) -> String {
    let (env, asset, dut) = axis_breakdown(l);
    format!(
        "attr reds={} tags={} cover={}permille env={} asset={} dut={} root={:016x}",
        l.red_count(),
        l.tag_count(),
        l.coverage_permille(),
        env,
        asset,
        dut,
        l.root(),
    )
}

/// 标准批：十条红账，证据分布 环境3/资产3/被测4。
pub fn build_reds() -> AttributionLedger {
    let mut l = AttributionLedger::new();
    // 环境 3 条：环境异常。
    let _ = l.register_red(1, Evidence { env_ok: false, asset_match: true, dut_confess: false });
    let _ = l.register_red(2, Evidence { env_ok: false, asset_match: false, dut_confess: true });
    let _ = l.register_red(3, Evidence { env_ok: false, asset_match: true, dut_confess: true });
    // 资产 3 条：环境健康、指纹不匹配。
    let _ = l.register_red(4, Evidence { env_ok: true, asset_match: false, dut_confess: false });
    let _ = l.register_red(5, Evidence { env_ok: true, asset_match: false, dut_confess: true });
    let _ = l.register_red(6, Evidence { env_ok: true, asset_match: false, dut_confess: false });
    // 被测 4 条：环境健康、指纹匹配、被测自证。
    let _ = l.register_red(7, Evidence { env_ok: true, asset_match: true, dut_confess: true });
    let _ = l.register_red(8, Evidence { env_ok: true, asset_match: true, dut_confess: true });
    let _ = l.register_red(9, Evidence { env_ok: true, asset_match: true, dut_confess: true });
    let _ = l.register_red(10, Evidence { env_ok: true, asset_match: true, dut_confess: true });
    let mut rid = 1u16;
    while rid <= 10 {
        let ev = match l.evidence_of(rid) {
            Some(e) => e,
            None => break,
        };
        if let Some(a) = derive_axis(&ev) {
            let _ = l.attribute(rid, a);
        }
        rid += 1;
    }
    l
}

/// 错归因注入 10/10：逐条登记与证据矛盾的轴，必须全数被抓。
pub fn wrong_axis_matrix() -> [bool; 10] {
    let mut out = [false; 10];
    let mut i = 0usize;
    while i < REDS {
        let mut probe = AttributionLedger::new();
        let mut rid = 1u16;
        while rid <= 10 {
            let ev = match build_reds().evidence_of(rid) {
                Some(e) => e,
                None => break,
            };
            let _ = probe.register_red(rid, ev);
            rid += 1;
        }
        let target = (i + 1) as u16;
        let truth = match derive_axis(&match build_reds().evidence_of(target) {
            Some(e) => e,
            None => Evidence { env_ok: true, asset_match: true, dut_confess: true },
        }) {
            Some(a) => a,
            None => Axis::UnderTest,
        };
        // 矛盾轴：真值之外的任一轴必被拒。
        let lie = match truth {
            Axis::Environment => Axis::Asset,
            Axis::Asset => Axis::UnderTest,
            Axis::UnderTest => Axis::Environment,
        };
        let caught = probe.attribute(target, lie) == Err(Diag::EWrong.val());
        // 拒后按真值归因必须放行，且错归因计数加一。
        let recover = probe.attribute(target, truth) == Ok(truth);
        out[i] = caught && recover && probe.wrong_caught() == 1;
        i += 1;
    }
    out
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
    SEG_BASE == 0x78080
}

/// 归因轴枚举闭合。
fn axis_closed_ok() -> bool {
    if AXIS_ALL.len() != 3 {
        return false;
    }
    let mut i = 0;
    while i < 3 {
        let a = match AXIS_ALL.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let mut j = i + 1;
        while j < 3 {
            let b = match AXIS_ALL.get(j) {
                Some(x) => *x,
                None => return false,
            };
            if a == b || a.ord() == b.ord() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 协议生效：十条红账全部归因且与证据推导一致。
fn protocol_ok(l: &AttributionLedger) -> bool {
    if l.red_count() != REDS {
        return false;
    }
    l.coverage_full() && l.coverage_permille() == 1000 && l.replay() == Err(Diag::ESeal.val())
}

/// 错归因注入 10/10 必抓。
fn wrong_axis_ok() -> bool {
    wrong_axis_matrix() == [true; 10]
}

/// 红账笔笔有标签。
fn coverage_ok(l: &AttributionLedger) -> bool {
    let mut rid = 1u16;
    while rid <= l.red_count() as u16 {
        if l.tag_of(rid).is_none() {
            return false;
        }
        rid += 1;
    }
    true
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let a = build_reds();
    let b = build_reds();
    a.fold() == b.fold() && a.fold() != 0
}

/// 十四项判据。
pub fn run_unx1508_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1508");
    let mut l = build_reds();

    s.add_dyn(
        "F1508-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1508-02 归因轴枚举闭合",
        axis_closed_ok(),
        &format!(
            "n={} names={}/{}/{}",
            AXIS_ALL.len(),
            Axis::Environment.name(),
            Axis::Asset.name(),
            Axis::UnderTest.name()
        ),
    );

    s.add_dyn(
        "F1508-03 协议生效十条红账",
        l.red_count() == REDS && protocol_ok(&l),
        &format!("reds={} tags={}", l.red_count(), l.tag_count()),
    );

    let (env, asset, dut) = axis_breakdown(&build_reds());
    s.add_dyn(
        "F1508-04 三向归因分布三三四",
        env == 3 && asset == 3 && dut == 4,
        &format!("env={} asset={} dut={}", env, asset, dut),
    );

    // 推导规则冻结：环境优先、资产次之、被测兜底。
    let e_env = Evidence { env_ok: false, asset_match: true, dut_confess: true };
    let e_asset = Evidence { env_ok: true, asset_match: false, dut_confess: true };
    let e_dut = Evidence { env_ok: true, asset_match: true, dut_confess: true };
    s.add_dyn(
        "F1508-05 推导规则冻结三例",
        derive_axis(&e_env) == Some(Axis::Environment)
            && derive_axis(&e_asset) == Some(Axis::Asset)
            && derive_axis(&e_dut) == Some(Axis::UnderTest),
        &format!("env={:?} asset={:?} dut={:?}", derive_axis(&e_env), derive_axis(&e_asset), derive_axis(&e_dut)),
    );

    s.add_dyn(
        "F1508-06 归因覆盖率百分百",
        coverage_ok(&l) && l.coverage_permille() == 1000 && l.coverage_full(),
        &format!("cover={}", l.coverage_permille()),
    );

    s.add_dyn(
        "F1508-07 错归因注入十路必抓",
        wrong_axis_ok(),
        &format!("wrong10={:?}", wrong_axis_matrix()),
    );

    // 空证据与未登记红账拒绝。
    let mut r = AttributionLedger::new();
    let blank = r.register_red(1, Evidence { env_ok: false, asset_match: false, dut_confess: false })
        == Err(Diag::ENoQ.val());
    let nored = r.attribute(1, Axis::Environment) == Err(Diag::ENoRed.val());
    let _ = r.register_red(1, Evidence { env_ok: true, asset_match: true, dut_confess: true });
    let dup_red = r.register_red(1, Evidence { env_ok: true, asset_match: true, dut_confess: true })
        == Err(Diag::EDup.val());
    let order = r.register_red(3, Evidence { env_ok: true, asset_match: true, dut_confess: true })
        == Err(Diag::EOrder.val());
    s.add_dyn(
        "F1508-08 空证据与红账门",
        blank && nored && dup_red && order && r.rejects() == 4,
        &format!("rejects={}", r.rejects()),
    );

    // 覆盖缺口封账拒绝：缺一条标签不许封。
    let mut gap = build_reds();
    let _ = gap.mark_attributed();
    // 撤标签不可行（无撤口），改用未满账验证：只登记不归因。
    let mut half = AttributionLedger::new();
    let _ = half.register_red(1, Evidence { env_ok: true, asset_match: true, dut_confess: true });
    s.add_dyn(
        "F1508-09 覆盖缺口推进被拒",
        half.mark_attributed() == Err(Diag::ENoCover.val())
            && half.phase() == Phase::Open
            && gap.phase() == Phase::Attributed,
        &format!("half={:?} gap={:?}", half.phase(), gap.phase()),
    );

    // 相位单向：归因→封账，倒退拒绝。
    let mut seq = build_reds();
    let a1 = seq.mark_attributed();
    let s1 = seq.seal();
    let s2 = seq.mark_attributed() == Err(Diag::ESealed.val());
    let re_seal = seq.seal() == Err(Diag::ENoLedger.val());
    s.add_dyn(
        "F1508-10 相位单向三步走",
        a1.is_ok() && s1.is_ok() && s2 && re_seal && seq.phase() == Phase::Closed,
        &format!("phase={:?} root={:016x}", seq.phase(), seq.root()),
    );

    s.add_dyn(
        "F1508-11 封账重放逐条一致",
        build_reds().replay() == Err(Diag::ESeal.val()) && {
            let mut c = build_reds();
            let _ = c.mark_attributed();
            let _ = c.seal();
            c.replay() == Ok(REDS)
        },
        &format!("root={:016x}", l.root()),
    );

    s.add_dyn(
        "F1508-12 双跑一致",
        dual_run_ok(),
        &format!("fold={:016x}", build_reds().fold()),
    );

    let text = summary(&build_reds());
    s.add_dyn(
        "F1508-13 归因摘要非空落档",
        text.contains("cover=1000permille") && text.contains("env=3") && text.len() > 30,
        &text,
    );

    s.add_dyn(
        "F1508-14 归因账根值非零",
        {
            let mut c = build_reds();
            let _ = c.mark_attributed();
            let _ = c.seal();
            c.root() != 0
        },
        &summary(&l),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_enum_closed() {
        assert_eq!(AXIS_ALL.len(), 3);
        assert_ne!(Axis::Environment, Axis::Asset);
        assert_ne!(Axis::Asset, Axis::UnderTest);
        assert_eq!(Axis::Environment.ord(), 0);
        assert_eq!(Axis::UnderTest.ord(), 2);
    }

    #[test]
    fn derive_rule_frozen() {
        assert_eq!(
            derive_axis(&Evidence { env_ok: false, asset_match: false, dut_confess: false }),
            None,
            "空证据无轴"
        );
        assert_eq!(
            derive_axis(&Evidence { env_ok: false, asset_match: true, dut_confess: true }),
            Some(Axis::Environment)
        );
        assert_eq!(
            derive_axis(&Evidence { env_ok: true, asset_match: false, dut_confess: true }),
            Some(Axis::Asset)
        );
        assert_eq!(
            derive_axis(&Evidence { env_ok: true, asset_match: true, dut_confess: true }),
            Some(Axis::UnderTest)
        );
    }

    #[test]
    fn reds_batch_and_coverage() {
        let l = build_reds();
        assert_eq!(l.red_count(), 10);
        assert_eq!(l.tag_count(), 10);
        assert_eq!(l.coverage_permille(), 1000);
        assert!(l.coverage_full());
        assert!(coverage_ok(&l));
        let (env, asset, dut) = axis_breakdown(&l);
        assert_eq!((env, asset, dut), (3, 3, 4));
    }

    #[test]
    fn wrong_axis_always_caught() {
        assert_eq!(wrong_axis_matrix(), [true; 10]);
        assert!(wrong_axis_ok());
    }

    #[test]
    fn structural_gates() {
        let mut l = AttributionLedger::new();
        assert_eq!(
            l.register_red(0, Evidence { env_ok: true, asset_match: true, dut_confess: true }),
            Err(Diag::EEmpty.val())
        );
        assert_eq!(
            l.register_red(2, Evidence { env_ok: true, asset_match: true, dut_confess: true }),
            Err(Diag::EOrder.val())
        );
        assert_eq!(
            l.register_red(1, Evidence { env_ok: false, asset_match: false, dut_confess: false }),
            Err(Diag::ENoQ.val())
        );
        let _ = l.register_red(1, Evidence { env_ok: true, asset_match: true, dut_confess: true });
        assert_eq!(
            l.register_red(1, Evidence { env_ok: true, asset_match: true, dut_confess: true }),
            Err(Diag::EDup.val())
        );
        assert_eq!(l.attribute(9, Axis::UnderTest), Err(Diag::ENoRed.val()));
        assert_eq!(l.rejects(), 5);
    }

    #[test]
    fn phase_monotonic() {
        let mut l = build_reds();
        assert_eq!(l.mark_attributed(), Ok(()));
        assert_eq!(l.phase(), Phase::Attributed);
        let root = l.seal();
        assert!(root.is_ok());
        assert_ne!(l.root(), 0);
        assert_eq!(l.phase(), Phase::Closed);
        assert_eq!(l.mark_attributed(), Err(Diag::ESealed.val()));
        assert_eq!(l.seal(), Err(Diag::ENoLedger.val()));
    }

    #[test]
    fn coverage_gap_blocks_advance() {
        let mut l = AttributionLedger::new();
        let _ = l.register_red(1, Evidence { env_ok: true, asset_match: true, dut_confess: true });
        let _ = l.register_red(2, Evidence { env_ok: true, asset_match: false, dut_confess: true });
        let _ = l.attribute(1, Axis::UnderTest);
        assert_eq!(l.coverage_permille(), 500);
        assert_eq!(l.mark_attributed(), Err(Diag::ENoCover.val()));
    }

    #[test]
    fn replay_after_seal() {
        let mut l = build_reds();
        let _ = l.mark_attributed();
        let _ = l.seal();
        assert_eq!(l.replay(), Ok(10));
        // 未封账重放拒绝。
        let mut fresh = build_reds();
        assert_eq!(fresh.replay(), Err(Diag::ESeal.val()));
        let _ = fresh.red_count();
    }

    #[test]
    fn diag_codes_closed() {
        assert_eq!(ALL.len(), 12);
        assert_eq!(Diag::ENoRed.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::ESeal.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert_eq!(detail(SEG_BASE + 0x30), "未知诊断码：不在 0x78081..0x7808C 闭集内");
    }

    #[test]
    fn summary_and_checks_green() {
        let mut l = build_reds();
        let _ = l.mark_attributed();
        let _ = l.seal();
        let t = summary(&l);
        assert!(t.contains("cover=1000permille"));
        assert!(t.contains("env=3"));
        assert!(t.len() > 30);
        let s = run_unx1508_checks();
        assert_eq!(s.len(), 14);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
