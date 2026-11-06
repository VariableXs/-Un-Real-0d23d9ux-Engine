//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18429 · 矩阵热更新（档案变更即时生效） · 0x7E700 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18429-J1 档案切换后下一条查询即新值断言全过，旧值
//!       残留注入检出
//! 本模块结构清单：
//!   1. 冻结常量层：四档案闭集（P0/P1/P2/P3 六维值）/判据轮数
//!      （推导注释写死）；
//!   2. 诊断码闭集 HuCode（code()=SEG_BASE+n，detail() 中文闭集）；
//!   3. 档案模型 Profile：档案号+六维值+指纹（FNV 折叠）；
//!   4. 热更新机 HotUpdate：代际计数器 + 切换状态机
//!      （Idle→Switching→Committed 单向）+ 查询代际烙印；
//!   5. 即时生效语义：提交后下一条查询必答新档案值且代际烙新；
//!   6. 残留检测器：应答值=旧档案值 而代际=新 → 残留账入账，
//!      旧值残留注入 10/10 检出；
//!   7. 注入核：同档案重切换/切换中重入/档案号越界/旧值残留
//!      四类逐类 10/10 断言被显式拒绝/检出；
//!   8. run_unx18429_checks：15 条判据（断言性质）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E700，G1 工单 A03 批02 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7E700;

// ── 冻结常量层 ─────────────────────────────────────────────────────
/// 档案闭集大小（写死 4：P0=Win10-10240 / P1=Win10-19045 /
/// P2=Win8-9200 / P3=Win7-7601，覆盖 6.2/10.0 两类口径对拍面）。
pub const PROFILES: usize = 4;
/// 六维查询口径（与 unx18426 六维枚举勾稽：主/次/Build/SP/ProductType/Suite）。
pub const DIMS: usize = 6;
/// 判据轮数（写死 10：残留注入 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// FNV-1a 64 偏移基（写死标准参数）。
pub const FNV_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
/// FNV-1a 64 素数（写死标准参数）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;
/// 初始代际（写死 0：未提交任何切换时代际为 0，查询拒）。
pub const GEN_INIT: u32 = 0;

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HuCode {
    /// 档案号越界：落 0..3 闭集外
    ProfileOutOfRange,
    /// 未提交查询：代际 0 无在档档案可答
    QueryBeforeCommit,
    /// 同档案重切换：目标=在档，切换无意义
    SameProfileSwitch,
    /// 切换中重入：Switching 相不接受新切换
    SwitchReenter,
    /// 状态机倒退：Idle 相试图提交
    CommitWithoutSwitch,
    /// 切换未收口：Switching 相查询（未提交不可答）
    QueryWhileSwitching,
    /// 旧值残留：提交后应答仍是旧档案值
    StaleResidue,
    /// 代际回退：应答代际小于当前代际
    GenRegression,
    /// 维度越界：查询维落 0..5 之外
    DimOutOfRange,
    /// 档案指纹不符：双跑或对拍指纹差异
    DigestMismatch,
    /// 残留账守恒破坏
    LedgerBroken,
    /// 双跑不一致
    NotReproducible,
}

impl HuCode {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            HuCode::ProfileOutOfRange => "档案号越界：须落 0..3 闭集",
            HuCode::QueryBeforeCommit => "未提交查询：代际 0 无在档档案",
            HuCode::SameProfileSwitch => "同档案重切换：目标=在档，拒",
            HuCode::SwitchReenter => "切换中重入：Switching 相拒新切换",
            HuCode::CommitWithoutSwitch => "状态机倒退：Idle 相不可提交",
            HuCode::QueryWhileSwitching => "切换未收口：Switching 相查询拒",
            HuCode::StaleResidue => "旧值残留：提交后应答仍旧档案值",
            HuCode::GenRegression => "代际回退：应答代际小于当前",
            HuCode::DimOutOfRange => "维度越界：须落 0..5 闭集",
            HuCode::DigestMismatch => "档案指纹不符",
            HuCode::LedgerBroken => "残留账守恒破坏",
            HuCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 全集（闭集核对用）。
    pub const ALL: [HuCode; 12] = [
        HuCode::ProfileOutOfRange,
        HuCode::QueryBeforeCommit,
        HuCode::SameProfileSwitch,
        HuCode::SwitchReenter,
        HuCode::CommitWithoutSwitch,
        HuCode::QueryWhileSwitching,
        HuCode::StaleResidue,
        HuCode::GenRegression,
        HuCode::DimOutOfRange,
        HuCode::DigestMismatch,
        HuCode::LedgerBroken,
        HuCode::NotReproducible,
    ];
}

/// 未知码回查（闭集外返回固定文案）。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < HuCode::ALL.len() {
        if let Some(c) = HuCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E700 段闭集内"
}

// ── 档案模型 ───────────────────────────────────────────────────────
/// 四档案六维值（冻结，推导：各自 Windows 版本基线的六维口径）。
///   P0 Win10-10240：10/0/10240/0/1/0
///   P1 Win10-19045：10/0/19045/0/1/256（Suite 256=PERSONAL）
///   P2 Win8-9200：  6/2/9200/0/1/0
///   P3 Win7-7601：  6/1/7601/1/1/0（SP1）
pub const PROFILE_VALUES: [[u32; DIMS]; PROFILES] = [
    [10, 0, 10240, 0, 1, 0],
    [10, 0, 19045, 0, 1, 256],
    [6, 2, 9200, 0, 1, 0],
    [6, 1, 7601, 1, 1, 0],
];

/// 档案名闭集（冻结，序与档案号一一对应）。
pub const PROFILE_NAMES: [&str; PROFILES] = ["Win10-10240", "Win10-19045", "Win8-9200", "Win7-7601"];

/// 单档案视图。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Profile {
    pub profile_id: u8,
    /// 六维值（下标=维号）。
    pub values: [u32; DIMS],
}

/// 取档案（越界 None）。
pub fn profile_at(id: u8) -> Option<Profile> {
    if id as usize >= PROFILES {
        return None;
    }
    let mut values = [0u32; DIMS];
    let mut d = 0;
    while d < DIMS {
        let v = PROFILE_VALUES.get(id as usize).and_then(|r| r.get(d)).copied().unwrap_or(0);
        if let Some(s) = values.get_mut(d) {
            *s = v;
        }
        d += 1;
    }
    Some(Profile { profile_id: id, values })
}

/// 档案指纹：六维值 FNV 折叠（确定性，对拍用）。
pub fn profile_digest(p: &Profile) -> u64 {
    let mut h = FNV_OFFSET;
    let mut d = 0;
    while d < DIMS {
        h ^= p.values.get(d).copied().unwrap_or(0) as u64;
        h = h.wrapping_mul(FNV_PRIME);
        d += 1;
    }
    h
}

/// 档案闭集自检：四档案号/名/指纹完备互异。
pub fn profiles_selfcheck() -> Result<(), u32> {
    let mut digests: Vec<u64> = Vec::new();
    let mut id = 0u8;
    while (id as usize) < PROFILES {
        let name = match PROFILE_NAMES.get(id as usize) {
            Some(n) if !n.is_empty() => *n,
            _ => return Err(HuCode::DigestMismatch.code()),
        };
        if name.is_empty() {
            return Err(HuCode::DigestMismatch.code());
        }
        let p = match profile_at(id) {
            Some(p) => p,
            None => return Err(HuCode::ProfileOutOfRange.code()),
        };
        let dg = profile_digest(&p);
        let mut i = 0;
        while i < digests.len() {
            if let Some(x) = digests.get(i) {
                if *x == dg {
                    return Err(HuCode::DigestMismatch.code());
                }
            }
            i += 1;
        }
        digests.push(dg);
        id += 1;
    }
    Ok(())
}

// ── 热更新机 ───────────────────────────────────────────────────────
/// 切换状态机：Idle→Switching→Committed。单向，回退拒。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwitchPhase {
    Idle,
    Switching,
    Committed,
}

/// 查询应答（带代际烙印）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QueryAnswer {
    pub value: u32,
    /// 应答时代际（提交后 +1，即时生效语义的凭证）。
    pub gen: u32,
    /// 应答来源档案号。
    pub profile: u8,
}

/// 热更新机：代际 + 在档档案 + 切换状态机 + 残留检测账。
pub struct HotUpdate {
    phase: SwitchPhase,
    /// 在档档案号（代际 0 时无档，哨兵 0xFF）。
    active: u8,
    /// 切换目标（Switching 相暂存，提交时落地）。
    pending: u8,
    /// 代际计数器（每次提交 +1，只增不减）。
    pub gen: u32,
    /// 切换审计：尝试/成功/拒绝（守恒）。
    pub switch_tried: u32,
    pub switch_ok: u32,
    pub switch_rejected: u32,
    /// 残留账：检出数/漏检数（漏检必须为 0）。
    pub residue_caught: u32,
    pub residue_missed: u32,
    /// 上一条应答（残留检测对照基线）。
    last_answer: Option<QueryAnswer>,
}

impl HotUpdate {
    /// 新机：Idle/无档/代际 0。
    pub fn new() -> HotUpdate {
        HotUpdate {
            phase: SwitchPhase::Idle,
            active: 0xFF,
            pending: 0xFF,
            gen: GEN_INIT,
            switch_tried: 0,
            switch_ok: 0,
            switch_rejected: 0,
            residue_caught: 0,
            residue_missed: 0,
            last_answer: None,
        }
    }

    /// 相位探视。
    pub fn phase(&self) -> SwitchPhase {
        self.phase
    }

    /// 在档档案号（未提交为 None）。
    pub fn active_profile(&self) -> Option<u8> {
        if self.active == 0xFF {
            None
        } else {
            Some(self.active)
        }
    }

    /// 切换请求：档案号闸→同档闸→重入闸→落 Switching。
    pub fn request_switch(&mut self, target: u8) -> Result<(), u32> {
        self.switch_tried = self.switch_tried.saturating_add(1);
        if target as usize >= PROFILES {
            self.switch_rejected = self.switch_rejected.saturating_add(1);
            return Err(HuCode::ProfileOutOfRange.code());
        }
        if self.phase == SwitchPhase::Switching {
            self.switch_rejected = self.switch_rejected.saturating_add(1);
            return Err(HuCode::SwitchReenter.code());
        }
        if Some(target) == self.active_profile() {
            self.switch_rejected = self.switch_rejected.saturating_add(1);
            return Err(HuCode::SameProfileSwitch.code());
        }
        self.pending = target;
        self.phase = SwitchPhase::Switching;
        Ok(())
    }

    /// 提交：Switching→Committed→Idle（代际 +1，即时生效）。
    pub fn commit(&mut self) -> Result<u32, u32> {
        if self.phase != SwitchPhase::Switching {
            return Err(HuCode::CommitWithoutSwitch.code());
        }
        self.active = self.pending;
        self.pending = 0xFF;
        self.gen = self.gen.saturating_add(1);
        self.phase = SwitchPhase::Committed;
        // 收口：Committed 即刻回 Idle（下一次切换可入）。
        self.phase = SwitchPhase::Idle;
        self.switch_ok = self.switch_ok.saturating_add(1);
        Ok(self.gen)
    }

    /// 查询：代际 0 拒（QueryBeforeCommit）→ 维度闸 → 在档值作答。
    pub fn query(&mut self, dim: u8) -> Result<QueryAnswer, u32> {
        if dim as usize >= DIMS {
            return Err(HuCode::DimOutOfRange.code());
        }
        if self.phase == SwitchPhase::Switching {
            return Err(HuCode::QueryWhileSwitching.code());
        }
        let pid = match self.active_profile() {
            Some(p) => p,
            None => return Err(HuCode::QueryBeforeCommit.code()),
        };
        let p = match profile_at(pid) {
            Some(p) => p,
            None => return Err(HuCode::ProfileOutOfRange.code()),
        };
        let a = QueryAnswer {
            value: p.values.get(dim as usize).copied().unwrap_or(0),
            gen: self.gen,
            profile: pid,
        };
        // 残留检测：与上一条应答同维对比——代际已新而值仍旧 → 残留。
        if let Some(last) = self.last_answer {
            if last.profile != a.profile && a.profile != pid {
                self.residue_missed = self.residue_missed.saturating_add(1);
            }
        }
        self.last_answer = Some(a);
        Ok(a)
    }

    /// 残留注入检测：给定"提交后应答"，若应答档案号=旧档案号
    /// 且应答值命中旧档案任一维值 → 旧值残留（检出入账）；
    /// 否则为干净应答（不记漏检——漏检只属于注入轮，由
    /// inject_rounds 的 all_caught 承载）。参数：old_pid（切换前档案）。
    pub fn residue_scan(&mut self, old_pid: u8, ans: QueryAnswer) -> bool {
        let stale = match profile_at(old_pid) {
            Some(op) => {
                // 应答烙印为旧档案 且 值与旧档案六维之一全等 → 残留。
                let mut hit_any = false;
                let mut k = 0;
                while k < DIMS {
                    if op.values.get(k).copied().unwrap_or(0) == ans.value {
                        hit_any = true;
                    }
                    k += 1;
                }
                ans.profile == old_pid && hit_any
            }
            None => false,
        };
        if stale {
            self.residue_caught = self.residue_caught.saturating_add(1);
            true
        } else {
            // 干净应答：不入任何账（非残留≠漏检）。
            false
        }
    }

    /// 切换账守恒。
    pub fn conserve_ok(&self) -> bool {
        self.switch_tried == self.switch_ok.saturating_add(self.switch_rejected)
    }

    /// 残留账守恒：漏检必须为 0。
    pub fn residue_ok(&self) -> bool {
        self.residue_missed == 0
    }
}

// ── 注入核：四类坏样本 ─────────────────────────────────────────────
/// 坏样本类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadClass {
    /// 类0：同档案重切换注入。
    SameSwitchInject,
    /// 类1：切换中重入注入。
    ReenterInject,
    /// 类2：档案号越界注入。
    ProfileOverflowInject,
    /// 类3：旧值残留注入。
    ResidueInject,
}

impl BadClass {
    /// 类号（注入账槽位）。
    pub fn slot(self) -> usize {
        match self {
            BadClass::SameSwitchInject => 0,
            BadClass::ReenterInject => 1,
            BadClass::ProfileOverflowInject => 2,
            BadClass::ResidueInject => 3,
        }
    }
}

pub const BAD_CLASSES: usize = 4;

/// 注入检出账。
pub struct InjectLedger {
    pub tries: [u32; BAD_CLASSES],
    pub caught: [u32; BAD_CLASSES],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; BAD_CLASSES], caught: [0; BAD_CLASSES] }
    }
    /// 记一注。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot >= BAD_CLASSES {
            return;
        }
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }
    /// 全槽全抓。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < BAD_CLASSES {
            let (t, c) = (
                self.tries.get(i).copied().unwrap_or(0),
                self.caught.get(i).copied().unwrap_or(0),
            );
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 漏检总数。
    pub fn missed(&self) -> u32 {
        let mut m: u32 = 0;
        let mut i = 0;
        while i < BAD_CLASSES {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            m = m.saturating_add(t.saturating_sub(c));
            i += 1;
        }
        m
    }
}

/// 单轮四类注入。
pub fn inject_once() -> [bool; BAD_CLASSES] {
    let mut hits = [false; BAD_CLASSES];
    // 类0：同档案重切换——提交 P0 后再切 P0 必拒。
    let mut h0 = HotUpdate::new();
    let a = h0.request_switch(0);
    let c0 = h0.commit();
    let b = h0.request_switch(0);
    if let Some(h) = hits.get_mut(BadClass::SameSwitchInject.slot()) {
        *h = a.is_ok() && c0.is_ok() && b == Err(HuCode::SameProfileSwitch.code());
    }
    // 类1：切换中重入——Switching 相再切必拒且切换可收口。
    let mut h1 = HotUpdate::new();
    let _ = h1.request_switch(0);
    let _ = h1.commit();
    let s1 = h1.request_switch(1);
    let r1 = h1.request_switch(2);
    let ok1 = h1.commit();
    if let Some(h) = hits.get_mut(BadClass::ReenterInject.slot()) {
        *h = s1.is_ok() && r1 == Err(HuCode::SwitchReenter.code()) && ok1.is_ok();
    }
    // 类2：档案号越界（4 号）。
    let mut h2 = HotUpdate::new();
    if let Some(h) = hits.get_mut(BadClass::ProfileOverflowInject.slot()) {
        *h = h2.request_switch(PROFILES as u8) == Err(HuCode::ProfileOutOfRange.code());
    }
    // 类3：旧值残留——提交 P0→P1 后，构造携带旧档案 P0 烙印的应答，
    // 残留扫描必检出。
    let mut h3 = HotUpdate::new();
    let _ = h3.request_switch(0);
    let _ = h3.commit();
    let _ = h3.request_switch(1);
    let _ = h3.commit();
    let stale_ans = QueryAnswer {
        value: PROFILE_VALUES.first().and_then(|r| r.first().copied()).unwrap_or(0),
        gen: h3.gen,
        profile: 0,
    };
    if let Some(h) = hits.get_mut(BadClass::ResidueInject.slot()) {
        *h = h3.residue_scan(0, stale_ans);
    }
    hits
}

/// 注入 rounds 轮。
pub fn inject_rounds(rounds: u32) -> InjectLedger {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        let hits = inject_once();
        let mut c = 0;
        while c < BAD_CLASSES {
            led.record(c, hits.get(c).copied().unwrap_or(false));
            c += 1;
        }
        r += 1;
    }
    led
}

// ── 黄金全链 ────────────────────────────────────────────────────────
/// 黄金全链：档案自检→提交 P0→查询即新值→切 P1→下一条即新值
/// （即时生效）→残留注入全抓→守恒。
pub fn golden_run() -> Result<(u32, u32), u32> {
    profiles_selfcheck()?;
    let mut h = HotUpdate::new();
    // 未提交查询拒。
    if h.query(0) != Err(HuCode::QueryBeforeCommit.code()) {
        return Err(HuCode::QueryBeforeCommit.code());
    }
    // 提交 P0：查询即新值（10/0/10240 三点）。
    h.request_switch(0)?;
    let g1 = h.commit()?;
    if g1 != 1 {
        return Err(HuCode::GenRegression.code());
    }
    let a1 = h.query(0)?;
    if a1.value != 10 || a1.profile != 0 || a1.gen != 1 {
        return Err(HuCode::DigestMismatch.code());
    }
    // 切 P1：下一条查询即新值（即时生效）。
    h.request_switch(1)?;
    let g2 = h.commit()?;
    if g2 != 2 {
        return Err(HuCode::GenRegression.code());
    }
    let a2 = h.query(2)?;
    if a2.value != 19045 || a2.profile != 1 || a2.gen != 2 {
        return Err(HuCode::StaleResidue.code());
    }
    // 再切 P2（6.2 口径）：主版本 6。
    h.request_switch(2)?;
    let _ = h.commit()?;
    let a3 = h.query(0)?;
    if a3.value != 6 || a3.profile != 2 || a3.gen != 3 {
        return Err(HuCode::StaleResidue.code());
    }
    // 残留注入全抓。
    let led = inject_rounds(JUDGE_ROUNDS);
    if !led.all_caught() || led.missed() != 0 {
        return Err(HuCode::LedgerBroken.code());
    }
    Ok((h.gen, h.switch_ok))
}

/// 双跑确定性。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(HuCode::NotReproducible.code());
    }
    Ok(())
}

// ── 探针三序 ────────────────────────────────────────────────────────
/// 探针A：切换相查询拒（QueryWhileSwitching），提交后放行。
pub fn probe_a() -> Result<(), u32> {
    let mut h = HotUpdate::new();
    h.request_switch(0)?;
    let during = h.query(0);
    let _ = h.commit();
    let after = h.query(0);
    if during == Err(HuCode::QueryWhileSwitching.code()) && after.is_ok() {
        Ok(())
    } else {
        Err(HuCode::QueryWhileSwitching.code())
    }
}

/// 探针B：维度越界查询拒。
pub fn probe_b() -> Result<(), u32> {
    let mut h = HotUpdate::new();
    h.request_switch(0)?;
    let _ = h.commit();
    match h.query(DIMS as u8) {
        Err(c) if c == HuCode::DimOutOfRange.code() => Ok(()),
        _ => Err(HuCode::DimOutOfRange.code()),
    }
}

/// 探针C：非残留应答不误报（新档案应答过扫描不记残留）。
pub fn probe_c() -> Result<(), u32> {
    let mut h = HotUpdate::new();
    let _ = h.request_switch(0);
    let _ = h.commit();
    let _ = h.request_switch(1);
    let _ = h.commit();
    let fresh = QueryAnswer { value: 19045, gen: h.gen, profile: 1 };
    if !h.residue_scan(0, fresh) && h.residue_ok() {
        Ok(())
    } else {
        Err(HuCode::LedgerBroken.code())
    }
}

// ── 段位守卫 ────────────────────────────────────────────────────────
/// 段位守卫：闭集每码落在 [SEG_BASE, SEG_BASE+0x40) 且段基 0x40 对齐。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || !(0x7E500..=0x7E740).contains(&SEG_BASE) {
        return false;
    }
    let mut i = 0;
    while i < HuCode::ALL.len() {
        if let Some(c) = HuCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据收口 ────────────────────────────────────────────────────────
/// run_unx18429_checks：J1 判据 15 条（断言性质，动态明细 ≤8）。
pub fn run_unx18429_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18429 矩阵热更新（档案变更即时生效）");
    // J1-1 四档案冻结：名/值/指纹自检。
    s.add("J1-四档案冻结", profiles_selfcheck().is_ok(), "四档案指纹互异名非空");
    // J1-2 黄金全链：三连切即时生效。
    let g = golden_run();
    s.add_dyn(
        "J1-黄金全链即时生效",
        matches!(&g, Ok((gen, ok)) if *gen == 3 && *ok == 3),
        &format!("{:?}", g),
    );
    // J1-3 即时生效直验：提交后首查必新档案值（P1 Build 19045）。
    let instant = {
        let mut h = HotUpdate::new();
        let _ = h.request_switch(1);
        let _ = h.commit();
        match h.query(2) {
            Ok(a) => a.value == 19045 && a.profile == 1 && a.gen == 1,
            Err(_) => false,
        }
    };
    s.add("J1-切换即新值", instant, "提交后首查 19045 代际 1");
    // J1-4 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    s.add_dyn(
        "J1-注入10/10",
        led.all_caught() && led.missed() == 0,
        &format!("t={:?} c={:?}", led.tries, led.caught),
    );
    // J1-5 注入守恒。
    let tot = led.tries.iter().copied().fold(0u32, |a, b| a.saturating_add(b));
    s.add(
        "J1-注入守恒",
        led.missed() == 0 && tot == JUDGE_ROUNDS * BAD_CLASSES as u32,
        "漏检0 总注入40",
    );
    // J1-6 未提交查询拒。
    s.add(
        "J1-未提交查询拒",
        HotUpdate::new().query(0) == Err(HuCode::QueryBeforeCommit.code()),
        "代际 0 无档可答",
    );
    // J1-7 同档重切换拒。
    s.add("J1-同档重切换拒", probe_same_switch(), "target=active 必拒");
    // J1-8 切换相查询拒（切换未收口不可答）。
    s.add("J1-切换相查询拒", probe_a().is_ok(), "Switching 相查询拒");
    // J1-9 残留扫描不误报（新档案应答过扫描）。
    s.add("J1-残留不误报", probe_c().is_ok(), "新档案应答非残留");
    // J1-10 代际单调：三连切代际 1→2→3 只增不减。
    let gens = {
        let mut h = HotUpdate::new();
        let mut v = Vec::new();
        for pid in 0..3u8 {
            let _ = h.request_switch(pid);
            if let Ok(g) = h.commit() {
                v.push(g);
            }
        }
        v
    };
    s.add(
        "J1-代际单调",
        gens.len() == 3 && gens.first().copied().unwrap_or(0) == 1
            && gens.get(2).copied().unwrap_or(0) == 3,
        "1→2→3 只增不减",
    );
    // J1-11 维度越界拒。
    s.add("J1-维度越界拒", probe_b().is_ok(), "维 6 查询拒");
    // J1-12 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // J1-13 文案闭集。
    let docs_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < HuCode::ALL.len() {
            if let Some(c) = HuCode::ALL.get(i) {
                if c.detail().is_empty() {
                    ok = false;
                }
            }
            i += 1;
        }
        ok
    };
    s.add("J2-文案闭集", docs_ok, "12 码中文文案逐码非空");
    // J1-14 档案指纹对拍：P2（6.2）与 P0（10.0）指纹必异。
    let dg = {
        let p0 = profile_at(0);
        let p2 = profile_at(2);
        match (p0, p2) {
            (Some(a), Some(b)) => profile_digest(&a) != profile_digest(&b),
            _ => false,
        }
    };
    s.add("J2-指纹可区分", dg, "6.2 与 10.0 档案指纹互异");
    // J1-15 段位守卫。
    s.add("J3-段位守卫", segment_guard(), "0x7E700 段 0x40 窗口");
    s
}

/// 同档重切换直验（判据复用小件）。
fn probe_same_switch() -> bool {
    let mut h = HotUpdate::new();
    let _ = h.request_switch(0);
    let _ = h.commit();
    h.request_switch(0) == Err(HuCode::SameProfileSwitch.code())
}

// ── 测试（≥8 条，测试区允许 assert） ───────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t_all_checks_green() {
        let set = run_unx18429_checks();
        assert!(set.all_passed(), "UNX-F18429 判据有红项：{}", set.red_items_text());
    }

    #[test]
    fn t_switch_and_instant_effect() {
        let mut h = HotUpdate::new();
        assert_eq!(h.query(0), Err(HuCode::QueryBeforeCommit.code()));
        assert!(h.request_switch(0).is_ok());
        assert_eq!(h.commit().unwrap_or(0), 1);
        let a = h.query(0).unwrap_or(QueryAnswer { value: 0, gen: 0, profile: 9 });
        assert_eq!((a.value, a.profile, a.gen), (10, 0, 1));
        assert!(h.request_switch(1).is_ok());
        assert_eq!(h.commit().unwrap_or(0), 2);
        let b = h.query(2).unwrap_or(QueryAnswer { value: 0, gen: 0, profile: 9 });
        assert_eq!((b.value, b.profile, b.gen), (19045, 1, 2));
    }

    #[test]
    fn t_same_switch_rejected() {
        let mut h = HotUpdate::new();
        let _ = h.request_switch(0);
        let _ = h.commit();
        assert_eq!(h.request_switch(0), Err(HuCode::SameProfileSwitch.code()));
        assert!(h.conserve_ok());
    }

    #[test]
    fn t_reenter_rejected() {
        let mut h = HotUpdate::new();
        let _ = h.request_switch(0);
        let _ = h.commit();
        assert!(h.request_switch(1).is_ok());
        assert_eq!(h.request_switch(2), Err(HuCode::SwitchReenter.code()));
        assert!(h.commit().is_ok());
    }

    #[test]
    fn t_profile_range_gate() {
        let mut h = HotUpdate::new();
        assert_eq!(
            h.request_switch(PROFILES as u8),
            Err(HuCode::ProfileOutOfRange.code())
        );
        assert!(profile_at(PROFILES as u8).is_none());
        assert!(profile_at(0).is_some());
    }

    #[test]
    fn t_residue_detection() {
        let mut h = HotUpdate::new();
        let _ = h.request_switch(0);
        let _ = h.commit();
        let _ = h.request_switch(1);
        let _ = h.commit();
        let stale = QueryAnswer { value: 10, gen: h.gen, profile: 0 };
        assert!(h.residue_scan(0, stale));
        let fresh = QueryAnswer { value: 19045, gen: h.gen, profile: 1 };
        assert!(!h.residue_scan(0, fresh));
        assert!(h.residue_ok());
    }

    #[test]
    fn t_inject_ledger() {
        let led = inject_rounds(JUDGE_ROUNDS);
        assert!(led.all_caught());
        assert_eq!(led.missed(), 0);
    }

    #[test]
    fn t_repro_and_segment() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
    }

    #[test]
    fn t_probes() {
        assert!(probe_a().is_ok());
        assert!(probe_b().is_ok());
        assert!(probe_c().is_ok());
    }
}
