//! UNX-F1574 移交时点声明（移交窗口与责任切换点） · 0x78E80段
//!
//! 判据原文（UNX-F1574-J1 摘录）：「移交时点声明生效（移交窗口/责任切换点/
//! 过渡期规则三声明落档），切换点前后责任归属可判 10/10，声明哈希落档」；
//! 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 三声明齐备才生效：移交窗口（起/止刻）、责任切换点、过渡期规则表——
//!    缺任一声明拒绝生效，三声明一并落档冻结；
//! 2. 责任归属纯整数判定：刻 < 切换点 → 交付方；刻 ≥ 切换点 → 接收方；
//!    过渡期规则覆盖的刻以规则归属（联合值守）优先，规则必须落在窗口内；
//! 3. 归属可判 10/10：十路探针刻（切换点前 4/切换点本身/切换点后/过渡期内）
//!    逐路归属必须可判且与声明一致，歧义刻即拒；
//! 4. 声明哈希落档：三声明 + 规则表整体 FNV-1a 折叠，冻结后哈希不可变，
//!    重放复算逐字一致；哈希为零视为未落档；
//! 5. 状态机 拟稿→已声明→已冻结 单向，倒退显性拒；诊断码闭集
//!    0x78E81..0x78E8C。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值；B07 批窗口 0x78E00..=0x78FC0）。
pub const SEG_BASE: u32 = 0x78E80;

/// 归属探针路数（10/10 可判）。
pub const PROBE_TICKS: usize = 10;

/// 标准声明参数（交付批 B39 钦定）：窗口起点。
pub const WIN_START: u64 = 1000;
/// 窗口终点。
pub const WIN_END: u64 = 2000;
/// 责任切换点。
pub const SWITCH_TICK: u64 = 1500;
/// 过渡期（规则覆盖段）起止。
pub const TRANS_FROM: u64 = 1500;
pub const TRANS_TO: u64 = 1600;

/// 移交时点声明诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 刻序非法（起点大于终点等）。
    EOrder,
    /// 窗口声明非法（零宽或倒挂）。
    EWindow,
    /// 切换点不在窗口内。
    ESwitch,
    /// 过渡期规则非法（越窗/倒挂/覆盖空段）。
    ERule,
    /// 归属取值非法。
    EOwner,
    /// 声明未生效就冻结。
    ENoDeclare,
    /// 重复发起声明。
    EDupDecl,
    /// 已冻结。
    EFrozen,
    /// 空声明样本。
    EEmpty,
    /// 声明哈希缺失。
    ENoHash,
    /// 哈希重放不符。
    EReplay,
    /// 相位倒退。
    EPhase,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EOrder => 0x01,
                Diag::EWindow => 0x02,
                Diag::ESwitch => 0x03,
                Diag::ERule => 0x04,
                Diag::EOwner => 0x05,
                Diag::ENoDeclare => 0x06,
                Diag::EDupDecl => 0x07,
                Diag::EFrozen => 0x08,
                Diag::EEmpty => 0x09,
                Diag::ENoHash => 0x0a,
                Diag::EReplay => 0x0b,
                Diag::EPhase => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EOrder => "刻序非法：窗口起点必须不大于终点，先核刻度再声明",
            Diag::EWindow => "窗口声明非法：零宽或倒挂窗口拒收，重开一个正序窗口",
            Diag::ESwitch => "切换点不在窗口内：切换点必须落在 [起,止] 闭区间",
            Diag::ERule => "过渡期规则非法：规则段必须落在窗口内且正序非空",
            Diag::EOwner => "归属取值非法：只认交付方/接收方/联合值守三态",
            Diag::ENoDeclare => "声明未生效：先三声明齐备生效，再谈冻结",
            Diag::EDupDecl => "声明已发起：重复声明拒绝，改动走规则表修订",
            Diag::EFrozen => "已冻结：冻结后写入拒绝，改动走下一批移交声明",
            Diag::EEmpty => "空声明样本：无窗口无切换点不构成声明",
            Diag::ENoHash => "声明哈希缺失：生效前必须落哈希",
            Diag::EReplay => "哈希重放不符：声明被改动，冻结账本可疑须复核",
            Diag::EPhase => "相位倒退：拟稿→已声明→已冻结 单向，倒退即拒",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EOrder,
    Diag::EWindow,
    Diag::ESwitch,
    Diag::ERule,
    Diag::EOwner,
    Diag::ENoDeclare,
    Diag::EDupDecl,
    Diag::EFrozen,
    Diag::EEmpty,
    Diag::ENoHash,
    Diag::EReplay,
    Diag::EPhase,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x78E81..0x78E8C 闭集内"
}

/// 责任归属三态闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    /// 交付方（切换点前）。
    Deliverer,
    /// 接收方（切换点起）。
    Receiver,
    /// 联合值守（过渡期规则钦定）。
    Joint,
}

impl Owner {
    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Owner::Deliverer => "交付方",
            Owner::Receiver => "接收方",
            Owner::Joint => "联合值守",
        }
    }
}

/// 归属三态闭集数组。
pub const OWNER_ALL: &[Owner] = &[Owner::Deliverer, Owner::Receiver, Owner::Joint];

/// 过渡期规则：覆盖段 [from, to] 内归属改判为 owner。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransRule {
    /// 规则名（如「联合值守过渡」）。
    pub name: &'static str,
    /// 覆盖段起点（含）。
    pub from_tick: u64,
    /// 覆盖段终点（含）。
    pub to_tick: u64,
    /// 段内归属。
    pub owner: Owner,
}

impl TransRule {
    /// 段是否正序非空。
    pub fn well_formed(&self) -> bool {
        self.from_tick <= self.to_tick
    }

    /// 刻是否被本规则覆盖。
    pub fn covers(&self, tick: u64) -> bool {
        self.well_formed() && tick >= self.from_tick && tick <= self.to_tick
    }
}

/// 声明相位（拟稿→已声明→已冻结 单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 拟稿：三声明登记中。
    Draft,
    /// 已声明：三声明齐备生效、哈希落档。
    Declared,
    /// 已冻结：哈希冻结、写入拒绝。
    Frozen,
}

impl Phase {
    /// 序数（倒退检测用）。
    pub fn ord(self) -> u8 {
        match self {
            Phase::Draft => 0,
            Phase::Declared => 1,
            Phase::Frozen => 2,
        }
    }
}

/// FNV-1a 折叠（整数哈希）。
fn fold_word(mut h: u64, w: u64) -> u64 {
    let mut k = w;
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

fn fold_str(h: u64, s: &str) -> u64 {
    let mut acc = h;
    for b in s.as_bytes() {
        acc ^= *b as u64;
        acc = acc.wrapping_mul(0x100000001B3);
    }
    acc
}

/// 移交时点声明账本：三声明登记 + 生效 + 冻结。
#[derive(Clone, Debug)]
pub struct HandoverDecl {
    phase: Phase,
    /// 移交窗口（起点、终点）。
    win: Option<(u64, u64)>,
    /// 责任切换点。
    switch_tick: Option<u64>,
    /// 过渡期规则表（按登记序）。
    rules: Vec<TransRule>,
    /// 生效哈希（三声明整体折叠）。
    hash: Option<u64>,
    /// 结构性拒绝计数。
    rejects: u32,
}

impl HandoverDecl {
    /// 新声明账本。
    pub fn new() -> HandoverDecl {
        HandoverDecl {
            phase: Phase::Draft,
            win: None,
            switch_tick: None,
            rules: Vec::new(),
            hash: None,
            rejects: 0,
        }
    }

    /// 相位。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 登记移交窗口（正序非零宽）。
    pub fn set_window(&mut self, start: u64, end: u64) -> Result<(), u32> {
        if self.phase != Phase::Draft {
            self.rejects += 1;
            return Err(Diag::EFrozen.val());
        }
        if start >= end {
            self.rejects += 1;
            return Err(Diag::EWindow.val());
        }
        self.win = Some((start, end));
        Ok(())
    }

    /// 登记责任切换点（必须落在已登记窗口内）。
    pub fn set_switch(&mut self, tick: u64) -> Result<(), u32> {
        if self.phase != Phase::Draft {
            self.rejects += 1;
            return Err(Diag::EFrozen.val());
        }
        let w = match self.win {
            Some(x) => x,
            None => {
                self.rejects += 1;
                return Err(Diag::EWindow.val());
            }
        };
        if tick < w.0 || tick > w.1 {
            self.rejects += 1;
            return Err(Diag::ESwitch.val());
        }
        self.switch_tick = Some(tick);
        Ok(())
    }

    /// 登记一条过渡期规则（必须落在窗口内且正序非空）。
    pub fn add_rule(&mut self, rule: TransRule) -> Result<(), u32> {
        if self.phase != Phase::Draft {
            self.rejects += 1;
            return Err(Diag::EFrozen.val());
        }
        if !rule.well_formed() || rule.name.is_empty() {
            self.rejects += 1;
            return Err(Diag::ERule.val());
        }
        let w = match self.win {
            Some(x) => x,
            None => {
                self.rejects += 1;
                return Err(Diag::EWindow.val());
            }
        };
        if rule.from_tick < w.0 || rule.to_tick > w.1 {
            self.rejects += 1;
            return Err(Diag::ERule.val());
        }
        self.rules.push(rule);
        Ok(())
    }

    /// 三声明是否齐备。
    pub fn complete(&self) -> bool {
        self.win.is_some() && self.switch_tick.is_some() && !self.rules.is_empty()
    }

    /// 规则表只读视图。
    pub fn rules(&self) -> &[TransRule] {
        &self.rules
    }

    /// 生效：三声明齐备、哈希折叠落档。
    pub fn declare(&mut self) -> Result<u64, u32> {
        if self.phase == Phase::Frozen {
            return Err(Diag::EFrozen.val());
        }
        if self.phase == Phase::Declared {
            return Err(Diag::EDupDecl.val());
        }
        if !self.complete() {
            return Err(Diag::EEmpty.val());
        }
        let w = match self.win {
            Some(x) => x,
            None => return Err(Diag::EWindow.val()),
        };
        let sw = match self.switch_tick {
            Some(x) => x,
            None => return Err(Diag::ESwitch.val()),
        };
        let mut h: u64 = 0xCBF29CE484222325;
        h = fold_str(h, "窗口");
        h = fold_word(h, w.0);
        h = fold_word(h, w.1);
        h = fold_str(h, "切换点");
        h = fold_word(h, sw);
        h = fold_word(h, self.rules.len() as u64);
        for r in &self.rules {
            h = fold_str(h, r.name);
            h = fold_word(h, r.from_tick);
            h = fold_word(h, r.to_tick);
            h = fold_word(h, match r.owner {
                Owner::Deliverer => 0,
                Owner::Receiver => 1,
                Owner::Joint => 2,
            });
        }
        self.hash = Some(h);
        self.phase = Phase::Declared;
        Ok(h)
    }

    /// 生效哈希。
    pub fn hash(&self) -> Option<u64> {
        self.hash
    }

    /// 责任归属判定：过渡期规则优先，其次按切换点二分。
    /// 判定不可判（三声明未齐）返回 Err。
    pub fn owner_at(&self, tick: u64) -> Result<Owner, u32> {
        if self.phase == Phase::Draft {
            return Err(Diag::ENoDeclare.val());
        }
        let sw = match self.switch_tick {
            Some(x) => x,
            None => return Err(Diag::ENoDeclare.val()),
        };
        for r in &self.rules {
            if r.covers(tick) {
                return Ok(r.owner);
            }
        }
        if tick < sw {
            Ok(Owner::Deliverer)
        } else {
            Ok(Owner::Receiver)
        }
    }

    /// 归属声明行（落档面，逐字含中文归属名）。
    pub fn owner_line(&self, tick: u64) -> Result<String, u32> {
        let o = self.owner_at(tick)?;
        Ok(format!("刻{} 归属{}", tick, o.name()))
    }

    /// 冻结：已声明才可冻结，哈希必须已落档且非零。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.phase == Phase::Frozen {
            return Err(Diag::EFrozen.val());
        }
        if self.phase != Phase::Declared {
            return Err(Diag::ENoDeclare.val());
        }
        let h = match self.hash {
            Some(x) if x != 0 => x,
            _ => return Err(Diag::ENoHash.val()),
        };
        self.phase = Phase::Frozen;
        Ok(h)
    }

    /// 冻结后重放：复算哈希与逐路归属，逐字一致才算通过。
    pub fn replay(&self) -> Result<u32, u32> {
        if self.phase != Phase::Frozen {
            return Err(Diag::EPhase.val());
        }
        let h = match self.hash {
            Some(x) => x,
            None => return Err(Diag::ENoHash.val()),
        };
        let mut fresh = HandoverDecl {
            phase: Phase::Draft,
            win: self.win,
            switch_tick: self.switch_tick,
            rules: Vec::new(),
            hash: None,
            rejects: 0,
        };
        for r in &self.rules {
            fresh.rules.push(*r);
        }
        let rh = match fresh.declare() {
            Ok(x) => x,
            Err(_) => return Err(Diag::EReplay.val()),
        };
        if rh != h {
            return Err(Diag::EReplay.val());
        }
        // 十路归属复判必须与冻结前一致。
        let mut t = WIN_START;
        while t <= WIN_END {
            let a = self.owner_at(t);
            let b = fresh.owner_at(t);
            if a != b {
                return Err(Diag::EReplay.val());
            }
            t += 100;
        }
        Ok(self.rules.len() as u32)
    }
}

/// 标准声明：窗口 1000..2000、切换点 1500、过渡规则 1500..1600 联合值守。
pub fn build_decl() -> HandoverDecl {
    let mut d = HandoverDecl::new();
    let _ = d.set_window(WIN_START, WIN_END);
    let _ = d.set_switch(SWITCH_TICK);
    let _ = d.add_rule(TransRule {
        name: "联合值守过渡",
        from_tick: TRANS_FROM,
        to_tick: TRANS_TO,
        owner: Owner::Joint,
    });
    d
}

/// 归属探针 10/10：十路刻的期望归属（判据钦定的归属表）。
/// 返回十布尔（true=该路归属可判且与声明一致）。
pub fn probe_matrix() -> [bool; PROBE_TICKS] {
    // (刻, 期望归属)：切换点前四路、切换点本身、切换点后两路、过渡期两路。
    let probes: [(u64, Owner); PROBE_TICKS] = [
        (1000, Owner::Deliverer),
        (1200, Owner::Deliverer),
        (1400, Owner::Deliverer),
        (1499, Owner::Deliverer),
        (1500, Owner::Joint),
        (1550, Owner::Joint),
        (1600, Owner::Joint),
        (1601, Owner::Receiver),
        (1800, Owner::Receiver),
        (2000, Owner::Receiver),
    ];
    let mut d = build_decl();
    // 归属判定以「已声明」相位为前提：先落哈希生效。
    let _ = d.declare();
    let mut out = [false; PROBE_TICKS];
    let mut i = 0usize;
    while i < PROBE_TICKS {
        let (tick, want) = match probes.get(i) {
            Some(x) => *x,
            None => break,
        };
        let cell = match out.get_mut(i) {
            Some(x) => x,
            None => break,
        };
        // 可判（Ok）且归属与声明一致；声明行同步可落档。
        let got = d.owner_at(tick);
        let line_ok = match d.owner_line(tick) {
            Ok(s) => s.contains(want.name()) && s.contains("归属"),
            Err(_) => false,
        };
        *cell = got == Ok(want) && line_ok;
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
    SEG_BASE == 0x78E80
}

/// 归属三态闭集。
fn owner_closed_ok() -> bool {
    if OWNER_ALL.len() != 3 {
        return false;
    }
    let mut i = 0usize;
    while i < OWNER_ALL.len() {
        let a = match OWNER_ALL.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let mut j = i + 1;
        while j < OWNER_ALL.len() {
            let b = match OWNER_ALL.get(j) {
                Some(x) => *x,
                None => return false,
            };
            if a == b {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 三声明齐备生效、哈希落档非零。
fn declared_ok(d: &HandoverDecl) -> bool {
    d.complete() && d.phase() == Phase::Declared && d.hash().map(|x| x != 0).unwrap_or(false)
}

/// 归属可判 10/10。
fn probe_ok() -> bool {
    probe_matrix() == [true; PROBE_TICKS]
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let a = build_decl();
    let b = build_decl();
    let ha = a.clone().declare();
    let hb = b.clone().declare();
    match (ha, hb) {
        (Ok(x), Ok(y)) => x == y && x != 0,
        _ => false,
    }
}

/// 十四项判据：UNX-F1574 移交时点声明。
pub fn run_unx1574_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1574");
    let mut d = build_decl();

    s.add_dyn(
        "F1574-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1574-02 归属三态闭集",
        owner_closed_ok(),
        &format!(
            "n={} owners={}/{}/{}",
            OWNER_ALL.len(),
            Owner::Deliverer.name(),
            Owner::Receiver.name(),
            Owner::Joint.name()
        ),
    );

    // 结构拒绝：倒挂窗口、未开窗先记切换点、切换点越窗、规则越窗。
    let mut r = HandoverDecl::new();
    let bad_win = r.set_window(2000, 1000) == Err(Diag::EWindow.val());
    let zero_win = r.set_window(1500, 1500) == Err(Diag::EWindow.val());
    let sw_no_win = r.set_switch(1500) == Err(Diag::EWindow.val());
    let _ = r.set_window(WIN_START, WIN_END);
    let sw_out = r.set_switch(3000) == Err(Diag::ESwitch.val());
    let rule_out = r.add_rule(TransRule { name: "越窗", from_tick: 0, to_tick: 500, owner: Owner::Joint }) == Err(Diag::ERule.val());
    let rule_bad = r.add_rule(TransRule { name: "倒挂", from_tick: 1600, to_tick: 1500, owner: Owner::Joint }) == Err(Diag::ERule.val());
    s.add_dyn(
        "F1574-03 结构拒绝六门",
        bad_win && zero_win && sw_no_win && sw_out && rule_out && rule_bad,
        &format!("rejects={}", r.rejects()),
    );

    // 未生效先判归属：显性拒。
    let early = build_decl();
    let early_judge = early.owner_at(1500) == Err(Diag::ENoDeclare.val());
    s.add_dyn(
        "F1574-04 未生效判归属被拒",
        early_judge && early.phase() == Phase::Draft,
        &format!("phase_ord={}", early.phase().ord()),
    );

    // 生效：三声明齐、哈希落档、重复声明拒绝。
    let h = d.declare();
    let h_ok = match h {
        Ok(x) => x != 0,
        Err(_) => false,
    };
    let dup = d.declare() == Err(Diag::EDupDecl.val());
    s.add_dyn(
        "F1574-05 三声明生效落哈希",
        h_ok && dup && declared_ok(&d),
        &format!("hash={:016x}", d.hash().unwrap_or(0)),
    );

    // 归属可判 10/10。
    s.add_dyn(
        "F1574-06 切换点前后归属可判十路",
        probe_ok(),
        &format!("matrix={:?}", probe_matrix()),
    );

    // 规则优先：过渡段内归属改判联合值守，切换点本身归联合。
    let joint = d.owner_at(SWITCH_TICK) == Ok(Owner::Joint);
    let just_before = d.owner_at(SWITCH_TICK - 1) == Ok(Owner::Deliverer);
    let just_after_trans = d.owner_at(TRANS_TO + 1) == Ok(Owner::Receiver);
    s.add_dyn(
        "F1574-07 过渡规则优先改判",
        joint && just_before && just_after_trans,
        &format!("joint={} before={} after={}", joint, just_before, just_after_trans),
    );

    // 归属声明行落档面（逐字含归属名）。
    let line = d.owner_line(SWITCH_TICK);
    let line_ok = match &line {
        Ok(t) => t.contains("刻1500") && t.contains("联合值守"),
        Err(_) => false,
    };
    s.add_dyn(
        "F1574-08 归属声明行落档",
        line_ok,
        &match line {
            Ok(t) => t,
            Err(c) => detail(c).to_string(),
        },
    );

    // 冻结闸门：未生效冻结拒绝；生效后冻结放行；冻结后写入拒绝。
    let mut draft = build_decl();
    let draft_freeze = draft.freeze() == Err(Diag::ENoDeclare.val());
    let fz = d.freeze();
    let fz_ok = match fz {
        Ok(x) => x == d.hash().unwrap_or(0),
        Err(_) => false,
    };
    let write_after = d.set_switch(1600) == Err(Diag::EFrozen.val());
    s.add_dyn(
        "F1574-09 冻结闸门三向",
        draft_freeze && fz_ok && write_after && d.phase() == Phase::Frozen,
        &format!("draft_freeze={} fz_ok={}", draft_freeze, fz_ok),
    );

    // 冻结重放：哈希复算 + 十路归属复判逐字一致。
    s.add_dyn(
        "F1574-10 冻结重放逐字一致",
        d.replay() == Ok(1),
        &format!("replay={:?}", d.replay()),
    );

    // 三声明摘要落档面（窗口/切换点/规则数齐）。
    let summary = format!(
        "窗口{}..{} 切换点{} 规则{}条",
        WIN_START, WIN_END, SWITCH_TICK, d.rules().len()
    );
    s.add_dyn(
        "F1574-11 三声明摘要落档",
        d.complete() && summary.contains("切换点1500") && d.rules().len() == 1,
        &summary,
    );

    // 相位倒退显性拒（冻结后判归属仍可读但写入全拒、声明口全拒）。
    let back = d.declare() == Err(Diag::EFrozen.val());
    s.add_dyn(
        "F1574-12 相位倒退显性拒",
        back && d.phase().ord() == 2,
        &format!("phase_ord={}", d.phase().ord()),
    );

    s.add_dyn(
        "F1574-13 双跑一致与闭集守卫",
        dual_run_ok() && ALL.len() == 12,
        &format!("hash={:016x} all={}", build_decl().declare().unwrap_or(0), ALL.len()),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_enum_closed() {
        assert_eq!(OWNER_ALL.len(), 3);
        assert_ne!(Owner::Deliverer, Owner::Receiver);
        assert_ne!(Owner::Receiver, Owner::Joint);
    }

    #[test]
    fn rule_covers_and_well_formed() {
        let r = TransRule { name: "过渡", from_tick: 1500, to_tick: 1600, owner: Owner::Joint };
        assert!(r.well_formed());
        assert!(r.covers(1500) && r.covers(1600) && r.covers(1550));
        assert!(!r.covers(1499) && !r.covers(1601));
        let bad = TransRule { name: "倒挂", from_tick: 1600, to_tick: 1500, owner: Owner::Joint };
        assert!(!bad.well_formed());
        assert!(!bad.covers(1550), "倒挂规则不覆盖任何刻");
    }

    #[test]
    fn structural_gates() {
        let mut d = HandoverDecl::new();
        assert_eq!(d.set_window(2000, 1000), Err(Diag::EWindow.val()));
        assert_eq!(d.set_switch(1500), Err(Diag::EWindow.val()), "未开窗先记切换点");
        let _ = d.set_window(WIN_START, WIN_END);
        assert_eq!(d.set_switch(3000), Err(Diag::ESwitch.val()));
        assert_eq!(
            d.add_rule(TransRule { name: "越窗", from_tick: 0, to_tick: 500, owner: Owner::Joint }),
            Err(Diag::ERule.val())
        );
        assert_eq!(d.rejects(), 4);
        assert!(!d.complete());
    }

    #[test]
    fn declare_and_dup() {
        let mut d = build_decl();
        assert!(d.complete());
        let h = d.declare().expect("三声明齐备生效");
        assert_ne!(h, 0);
        assert_eq!(d.declare(), Err(Diag::EDupDecl.val()));
        assert_eq!(d.phase(), Phase::Declared);
    }

    #[test]
    fn incomplete_declare_rejected() {
        let mut d = HandoverDecl::new();
        let _ = d.set_window(WIN_START, WIN_END);
        let _ = d.set_switch(SWITCH_TICK);
        // 缺过渡规则：三声明不齐。
        assert_eq!(d.declare(), Err(Diag::EEmpty.val()));
    }

    #[test]
    fn ownership_ten_of_ten() {
        let m = probe_matrix();
        assert_eq!(m, [true; PROBE_TICKS]);
        let mut d = build_decl();
        let _ = d.declare();
        assert_eq!(d.owner_at(1000), Ok(Owner::Deliverer));
        assert_eq!(d.owner_at(1499), Ok(Owner::Deliverer));
        assert_eq!(d.owner_at(1500), Ok(Owner::Joint));
        assert_eq!(d.owner_at(1601), Ok(Owner::Receiver));
        assert_eq!(d.owner_at(2000), Ok(Owner::Receiver));
    }

    #[test]
    fn freeze_gates() {
        let mut d = build_decl();
        assert_eq!(d.freeze(), Err(Diag::ENoDeclare.val()));
        let h = d.declare().expect("生效");
        let got = d.freeze().expect("冻结");
        assert_eq!(h, got);
        assert_eq!(d.phase(), Phase::Frozen);
        assert_eq!(d.set_window(1, 2), Err(Diag::EFrozen.val()));
        assert_eq!(d.set_switch(1500), Err(Diag::EFrozen.val()));
        assert_eq!(d.declare(), Err(Diag::EFrozen.val()));
        assert_eq!(d.rejects(), 2, "冻结后两次写入计入拒绝");
    }

    #[test]
    fn replay_after_freeze() {
        let mut d = build_decl();
        let _ = d.declare();
        let _ = d.freeze();
        assert_eq!(d.replay(), Ok(1));
    }

    #[test]
    fn dual_run_consistency() {
        assert!(dual_run_ok());
    }

    #[test]
    fn diag_codes_closed() {
        assert_eq!(ALL.len(), 12);
        assert_eq!(Diag::EOrder.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::EPhase.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert_eq!(detail(SEG_BASE + 0x40), "未知诊断码：不在 0x78E81..0x78E8C 闭集内");
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx1574_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
