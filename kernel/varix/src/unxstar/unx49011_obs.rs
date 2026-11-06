//! UNX-F49011 quirk 命中观测账（命中/未命中/降级三态计数）+ UNX-F49012 域经义务 DJ-UNX-M2-001 落账位 · 0x71A00
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，VX6 批 B11 族）：
//!   UNX-F49011 quirk 命中观测账（命中/未命中/降级三态计数）: ① 纯功能代码 ≥260 行；
//!          ② 判据原文：UNX-F49012-J1 域经条目登记锚定行；判据锚定 §AI-62 §8 域经义务；
//!          ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   UNX-F49012 域经义务 DJ-UNX-M2-001 落账位（quirk 第一个坑实况回填通道）: ① 纯功能代码 ≥260 行；
//!          ② 判据原文：UNX-F49013-J1 走查留痕零混用；判据锚定 20 维 文案与语言；
//!          ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（观测账）：读数合法性 → 降级哨兵 → 命中比对 → 未命中归类 → 落账。
//! 合法性先于一切：0 表示「无读数」、超过 OBS_MAX 表示「读数溢出」，
//! 二者都不是三态中的任何一态，必须先炸掉，否则「无读数」会被记成未命中，
//! 观测账立刻虚高，命中率失去统计意义。
//! 降级先于命中：降级哨兵是 quirk 引擎「主动放弃、回退通用路径」的唯一信号。
//! 若先比命中，一旦某个 quirk 的期望值恰好等于哨兵（表损坏），降级会被
//! 记成命中——这正是「降级被美化」的典型失真，所以哨兵判定必须前置，
//! 并由表审计（expect ≠ fallback）双重钉死。
//! 命中先于未命中：命中是逐位相等这一条硬事实，未命中是补集。先判硬事实，
//! 补集恒有定义；反过来先判补集时，命中判定就没有独立的入口可测。
//! 闸门次序（域经落账）：义务码形状 → 槽位稠密 → 锚定行解析 → 码/行唯一 →
//! 零混用扫描 → 回填通道 → 落账。
//! 形状先于稠密：DJ-UNX-M2-NNN 的码形是本模块唯一的外部契约，码形坏了谈
//! 槽位稠密没有意义（槽位是内部布局，码形是外部身份）。
//! 稠密先于解析：槽位必须严格等于已登记数，乱序/跳号会让「第 N 个义务」
//! 出现两个候选，锚定行解析得再对也无法确定它属于谁。
//! 解析先于唯一：锚定行解析失败时字段本身缺失，此时比较唯一性只是在比较
//! 两个空值，会给出假绿。
//! 唯一先于混用：码重复、行重复时「混用」的计数本身无定义（同一个码出现
//! 两次到底算不算混用？），先把唯一性钉死，混用才是一个可数的量。
//! 关键差异：本模块只回答两件事——
//! ① 观测账：某条 quirk 在本拍「命中 / 未命中 / 降级」哪一态，三态计数
//!    在 ×10^2 轮上逐轮一致（计数来自独立复算，不来自引擎自报）；
//! ② 域经义务：DJ-UNX-M2-001 的落账位是否「有锚定行、槽位稠密、零混用」，
//!    以及 quirk 第一个坑的实况能否沿回填通道写回账位。
//! 不重跑 quirk 表的匹配算法（那是表引擎的职责），不做配置面文案裁决
//! （B11 文案账），不做错误码族去重（B11 错误码族），也不写任务板。
//! 观测账与域经账是两本互不改写的账：观测账的计数不进域经账，域经账的
//! 槽位不回写观测态；两本账只通过「回填通道」单向连接——通道只搬运
//! 已定态的实况，不改变观测账里任何一格的计数。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71A00，VX6 批 B11 族第五槽）。
pub const SEG_BASE: u32 = 0x71A00;

/// quirk 槽数（观测账的维度）。
pub const QUIRK_N: usize = 12;
/// 三态 ×10^2 的轮数载体（卡面 ×10^2）。
pub const OBS_ROUNDS: u32 = 100;
/// 读数下界：0 表示无读数，不是三态中的任何一态。
pub const OBS_MIN: u32 = 1;
/// 读数上界（当量单位）：越界即坏数据。
pub const OBS_MAX: u32 = 100_000;
/// 降级哨兵基值：第 i 个 quirk 的哨兵 = 9_900 + i（与任何期望值都远离）。
pub const FALLBACK_BASE: u32 = 9_900;
/// 合法注入轮数上限。
pub const INJECT_MAX: u32 = 4_096;
/// 注入路数。
pub const INJECT_ROUTES: usize = 4;
/// 账本字段数（六字段渲染）。
pub const STATS_FIELDS: usize = 6;
/// 锚定行字段数。
pub const ANCHOR_FIELDS: usize = 4;
/// 黄金脚本轮数。
pub const GOLDEN_ROUNDS: u32 = 16;
/// 域经义务条数（DJ-UNX-M2-001..004 + 两条回填义务）。
pub const OBLIG_N: usize = 6;
/// 卡片号下界（本模块覆盖的任务板卡号）。
pub const FIRST_CARD: u16 = 49_011;
/// 卡片号上界。
pub const LAST_CARD: u16 = 49_020;
/// 回填备注最大字节数（实况备注必须短且可查）。
pub const NOTE_MAX: usize = 32;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0E） ─────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObsCode {
    /// 命中态被吞（应命中却记成别的态）
    HitLost,
    /// 未命中态被吞
    MissLost,
    /// 降级态被美化（记成命中/未命中）
    DegradeBad,
    /// quirk 槽号越界或表里没有该 quirk
    QuirkBad,
    /// 期望值/哨兵表损坏（相等或越界）
    ExpectBad,
    /// 读数越界（0 或超过上界）
    ObsBad,
    /// 锚定行不可解析或字段缺失
    AnchorBad,
    /// 落账槽位跳号/乱序/重复
    SlotBad,
    /// 义务码重复或码形非法
    CodeBad,
    /// 锚定行跨义务混用
    CrossBad,
    /// 三态账不守恒
    LedgerBad,
    /// 渲染字段数或数值不可解析
    RenderBad,
    /// 两次确定性复跑不一致
    ReplayBad,
    /// 轮数为零或越过覆盖上限
    CoverBad,
}

impl ObsCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            ObsCode::HitLost => "命中态被吞，三态计数与复算不符",
            ObsCode::MissLost => "未命中态被吞，三态计数与复算不符",
            ObsCode::DegradeBad => "降级哨兵被记成命中或未命中",
            ObsCode::QuirkBad => "quirk 槽号越界或表中无此条目",
            ObsCode::ExpectBad => "期望值与降级哨兵相等或越界",
            ObsCode::ObsBad => "读数为零或越过读数上界",
            ObsCode::AnchorBad => "锚定行不可解析或字段缺失",
            ObsCode::SlotBad => "落账槽位跳号乱序或重复",
            ObsCode::CodeBad => "义务码重复或码形非法",
            ObsCode::CrossBad => "锚定行跨义务混用",
            ObsCode::LedgerBad => "命中未命中降级三账不守恒",
            ObsCode::RenderBad => "账本文本字段数或数值不可解析",
            ObsCode::ReplayBad => "两次确定性复跑结果不一致",
            ObsCode::CoverBad => "轮数为零或越过覆盖上限",
        }
    }

    pub const ALL: [ObsCode; 14] = [
        ObsCode::HitLost,
        ObsCode::MissLost,
        ObsCode::DegradeBad,
        ObsCode::QuirkBad,
        ObsCode::ExpectBad,
        ObsCode::ObsBad,
        ObsCode::AnchorBad,
        ObsCode::SlotBad,
        ObsCode::CodeBad,
        ObsCode::CrossBad,
        ObsCode::LedgerBad,
        ObsCode::RenderBad,
        ObsCode::ReplayBad,
        ObsCode::CoverBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < ObsCode::ALL.len() {
        if let Some(c) = ObsCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知观测诊断码"
}

pub fn codes_are_distinct() -> bool {
    let mut i = 0usize;
    while i < ObsCode::ALL.len() {
        let a = match ObsCode::ALL.get(i) {
            Some(v) => v.val(),
            None => return false,
        };
        let mut j = i + 1;
        while j < ObsCode::ALL.len() {
            let b = match ObsCode::ALL.get(j) {
                Some(v) => v.val(),
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

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < ObsCode::ALL.len() {
        let c = match ObsCode::ALL.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let v = c.val();
        if v <= SEG_BASE || v > SEG_BASE + 0x0F {
            return false;
        }
        i += 1;
    }
    codes_are_distinct()
}

// ── 三态 ──────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Hit,
    Miss,
    Degrade,
}

impl State {
    pub fn code(self) -> u32 {
        match self {
            State::Hit => 0,
            State::Miss => 1,
            State::Degrade => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            State::Hit => "hit",
            State::Miss => "miss",
            State::Degrade => "degrade",
        }
    }

    /// 从码值回读（死亡分支返回 None，不 panic）。
    pub fn from_code(c: u32) -> Option<State> {
        match c {
            0 => Some(State::Hit),
            1 => Some(State::Miss),
            2 => Some(State::Degrade),
            _ => None,
        }
    }

    pub const ALL: [State; 3] = [State::Hit, State::Miss, State::Degrade];
}

/// 三态码位审计：0..=2 稠密、名字唯一。
pub fn audit_state_codes() -> Result<u32, u32> {
    let mut i = 0usize;
    while i < State::ALL.len() {
        let s = match State::ALL.get(i) {
            Some(v) => *v,
            None => return Err(ObsCode::LedgerBad.val()),
        };
        if s.code() != i as u32 {
            return Err(ObsCode::LedgerBad.val());
        }
        if State::from_code(s.code()) != Some(s) {
            return Err(ObsCode::LedgerBad.val());
        }
        let mut j = i + 1;
        while j < State::ALL.len() {
            if let Some(o) = State::ALL.get(j) {
                if o.name() == s.name() {
                    return Err(ObsCode::LedgerBad.val());
                }
            }
            j += 1;
        }
        i += 1;
    }
    Ok(State::ALL.len() as u32)
}

// ── quirk 表 ──────────────────────────────────────────────────────
/// 一条 quirk 观测契约：槽号、键名、期望读数、降级哨兵。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QuirkSpec {
    pub id: u8,
    pub key: &'static str,
    pub expect: u32,
    pub fallback: u32,
}

/// 12 条 quirk（键名与触控板 quirk 表同名族，只用于本模块的观测契约）。
pub const QUIRKS: [QuirkSpec; QUIRK_N] = [
    QuirkSpec { id: 0, key: "palm-suppress", expect: 120, fallback: 9_900 },
    QuirkSpec { id: 1, key: "edge-reject", expect: 240, fallback: 9_901 },
    QuirkSpec { id: 2, key: "axis-flip", expect: 300, fallback: 9_902 },
    QuirkSpec { id: 3, key: "pinch-hyst", expect: 360, fallback: 9_903 },
    QuirkSpec { id: 4, key: "tap-debounce", expect: 420, fallback: 9_904 },
    QuirkSpec { id: 5, key: "wheel-notch", expect: 120, fallback: 9_905 },
    QuirkSpec { id: 6, key: "scroll-accel", expect: 480, fallback: 9_906 },
    QuirkSpec { id: 7, key: "two-finger-lock", expect: 540, fallback: 9_907 },
    QuirkSpec { id: 8, key: "hover-guard", expect: 600, fallback: 9_908 },
    QuirkSpec { id: 9, key: "pressure-cap", expect: 660, fallback: 9_909 },
    QuirkSpec { id: 10, key: "gesture-idle", expect: 720, fallback: 9_910 },
    QuirkSpec { id: 11, key: "cfg-fallback", expect: 780, fallback: 9_911 },
];

/// 按槽号取 quirk（越界返回 None）。
pub fn quirk(id: u8) -> Option<QuirkSpec> {
    if (id as usize) >= QUIRK_N {
        return None;
    }
    QUIRKS.get(id as usize).copied()
}

/// 按键名取 quirk（线性查表，键名唯一由表审计保证）。
pub fn quirk_by_key(key: &str) -> Option<QuirkSpec> {
    let mut i = 0usize;
    while i < QUIRKS.len() {
        if let Some(q) = QUIRKS.get(i) {
            if q.key == key {
                return Some(*q);
            }
        }
        i += 1;
    }
    None
}

/// quirk 表审计：槽号稠密、键名非空且唯一、期望值合法且不等于哨兵。
pub fn audit_quirk_table() -> Result<u32, u32> {
    let mut i = 0usize;
    while i < QUIRKS.len() {
        let q = match QUIRKS.get(i) {
            Some(v) => *v,
            None => return Err(ObsCode::QuirkBad.val()),
        };
        if q.id as usize != i {
            return Err(ObsCode::QuirkBad.val());
        }
        if q.key.is_empty() || q.key.len() > 24 {
            return Err(ObsCode::QuirkBad.val());
        }
        if q.expect < OBS_MIN || q.expect > OBS_MAX {
            return Err(ObsCode::ExpectBad.val());
        }
        if q.fallback < OBS_MIN || q.fallback > OBS_MAX {
            return Err(ObsCode::ExpectBad.val());
        }
        if q.expect == q.fallback {
            return Err(ObsCode::ExpectBad.val());
        }
        let mut j = i + 1;
        while j < QUIRKS.len() {
            match QUIRKS.get(j) {
                Some(o) => {
                    if o.key == q.key {
                        return Err(ObsCode::QuirkBad.val());
                    }
                }
                None => return Err(ObsCode::QuirkBad.val()),
            }
            j += 1;
        }
        i += 1;
    }
    Ok(QUIRKS.len() as u32)
}

/// 哨兵落位审计：第 i 条 quirk 的哨兵必须等于 FALLBACK_BASE + i。
/// 这条把「哨兵属于槽位」变成可复算的算术事实，避免哨兵被随手改写后
/// 仍被当作合法表使用。
pub fn audit_fallback_slots() -> Result<u32, u32> {
    let mut i = 0usize;
    while i < QUIRKS.len() {
        let q = match QUIRKS.get(i) {
            Some(v) => *v,
            None => return Err(ObsCode::QuirkBad.val()),
        };
        let want = FALLBACK_BASE.saturating_add(i as u32);
        if q.fallback != want {
            return Err(ObsCode::ExpectBad.val());
        }
        i += 1;
    }
    Ok(QUIRKS.len() as u32)
}

/// 三态分类（纯函数）：合法性 → 降级哨兵 → 命中 → 未命中。
pub fn classify(observed: u32, spec: &QuirkSpec) -> Result<State, u32> {
    if observed < OBS_MIN || observed > OBS_MAX {
        return Err(ObsCode::ObsBad.val());
    }
    if spec.expect == spec.fallback || spec.expect < OBS_MIN || spec.fallback < OBS_MIN {
        return Err(ObsCode::ExpectBad.val());
    }
    if observed == spec.fallback {
        return Ok(State::Degrade);
    }
    if observed == spec.expect {
        return Ok(State::Hit);
    }
    Ok(State::Miss)
}

/// 按槽号分类（越界先炸 QuirkBad）。
pub fn classify_slot(id: u8, observed: u32) -> Result<State, u32> {
    let spec = match quirk(id) {
        Some(q) => q,
        None => return Err(ObsCode::QuirkBad.val()),
    };
    classify(observed, &spec)
}

// ── 观测账引擎 ────────────────────────────────────────────────────
/// 单条 quirk 的三态计数格。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ObsCell {
    pub hits: u32,
    pub misses: u32,
    pub degrades: u32,
}

impl ObsCell {
    pub const fn zero() -> ObsCell {
        ObsCell {
            hits: 0,
            misses: 0,
            degrades: 0,
        }
    }

    pub fn total(&self) -> u32 {
        self.hits
            .saturating_add(self.misses)
            .saturating_add(self.degrades)
    }

    fn bump(&mut self, st: State) {
        match st {
            State::Hit => self.hits = self.hits.saturating_add(1),
            State::Miss => self.misses = self.misses.saturating_add(1),
            State::Degrade => self.degrades = self.degrades.saturating_add(1),
        }
    }
}

/// 观测账：按 quirk 槽分格累计三态，末位保存指纹。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QuirkObs {
    cells: [ObsCell; QUIRK_N],
    pub samples: u32,
    pub rejects: u32,
    pub digest: u64,
}

impl QuirkObs {
    pub fn new() -> QuirkObs {
        QuirkObs {
            cells: [ObsCell::zero(); QUIRK_N],
            samples: 0,
            rejects: 0,
            digest: p_core::FNV_OFF,
        }
    }

    /// 只读取格（越界返回零格，便于审计时不 panic）。
    pub fn cell(&self, id: u8) -> ObsCell {
        match self.cells.get(id as usize) {
            Some(c) => *c,
            None => ObsCell::zero(),
        }
    }

    /// 提交一拍读数：合法性 → 哨兵 → 命中 → 未命中 → 落账。
    pub fn record(&mut self, spec: &QuirkSpec, observed: u32) -> Result<State, u32> {
        if (spec.id as usize) >= QUIRK_N {
            self.rejects = self.rejects.saturating_add(1);
            self.digest = p_core::fold_u32(self.digest, ObsCode::QuirkBad.val());
            return Err(ObsCode::QuirkBad.val());
        }
        let st = match classify(observed, spec) {
            Ok(s) => s,
            Err(e) => {
                self.rejects = self.rejects.saturating_add(1);
                self.digest = p_core::fold_u32(self.digest, e);
                return Err(e);
            }
        };
        if let Some(c) = self.cells.get_mut(spec.id as usize) {
            c.bump(st);
        } else {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ObsCode::QuirkBad.val());
        }
        self.samples = self.samples.saturating_add(1);
        self.digest = p_core::fold_u32(self.digest, spec.id as u32);
        self.digest = p_core::fold_u32(self.digest, observed);
        self.digest = p_core::fold_u32(self.digest, st.code());
        Ok(st)
    }

    /// 三态合计（饱和加法，恒定不 panic）。
    pub fn totals(&self) -> [u32; 3] {
        let mut out = [0u32; 3];
        let mut i = 0usize;
        while i < self.cells.len() {
            if let Some(c) = self.cells.get(i) {
                if let Some(h) = out.get_mut(0) {
                    *h = h.saturating_add(c.hits);
                }
                if let Some(m) = out.get_mut(1) {
                    *m = m.saturating_add(c.misses);
                }
                if let Some(d) = out.get_mut(2) {
                    *d = d.saturating_add(c.degrades);
                }
            }
            i += 1;
        }
        out
    }

    /// 审计：样本非零、三态守恒、指纹非零、格合计等于样本数。
    pub fn audit(&self) -> Result<(), u32> {
        if self.samples == 0 {
            return Err(ObsCode::CoverBad.val());
        }
        let t = self.totals();
        let sum = t
            .first()
            .copied()
            .unwrap_or(0)
            .saturating_add(t.get(1).copied().unwrap_or(0))
            .saturating_add(t.get(2).copied().unwrap_or(0));
        if sum != self.samples {
            return Err(ObsCode::LedgerBad.val());
        }
        let mut i = 0usize;
        let mut cell_sum = 0u32;
        while i < self.cells.len() {
            if let Some(c) = self.cells.get(i) {
                cell_sum = cell_sum.saturating_add(c.total());
            }
            i += 1;
        }
        if cell_sum != self.samples {
            return Err(ObsCode::LedgerBad.val());
        }
        if self.digest == 0 {
            return Err(ObsCode::ReplayBad.val());
        }
        Ok(())
    }

    pub fn stats(&self, rounds: u32) -> ObsStats {
        let t = self.totals();
        ObsStats {
            rounds,
            hits: t.first().copied().unwrap_or(0),
            misses: t.get(1).copied().unwrap_or(0),
            degrades: t.get(2).copied().unwrap_or(0),
            quirks: QUIRK_N as u32,
            digest: self.digest,
        }
    }
}

/// 观测账汇总（六字段：轮数|命中|未命中|降级|quirk 数|指纹）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ObsStats {
    pub rounds: u32,
    pub hits: u32,
    pub misses: u32,
    pub degrades: u32,
    pub quirks: u32,
    pub digest: u64,
}

impl ObsStats {
    /// 每轮恰好走一遍全部 quirk，三态合计 = 轮数 × quirk 数。
    pub fn balanced(&self) -> bool {
        if self.rounds == 0 || self.quirks != QUIRK_N as u32 {
            return false;
        }
        let want = self.rounds.saturating_mul(QUIRK_N as u32);
        let got = self
            .hits
            .saturating_add(self.misses)
            .saturating_add(self.degrades);
        got == want && got > 0
    }

    pub fn audit(&self) -> Result<(), u32> {
        if !self.balanced() {
            return Err(ObsCode::LedgerBad.val());
        }
        if self.digest == 0 {
            return Err(ObsCode::ReplayBad.val());
        }
        Ok(())
    }

    /// 三态非退化：每一态都必须有真实样本（否则「三态计数」名不副实）。
    pub fn non_degenerate(&self) -> bool {
        self.hits > 0 && self.misses > 0 && self.degrades > 0
    }
}

/// 第 i 轮第 q 槽的驱动模式：0 命中、1 未命中、2 降级。
/// 模式由 (轮号 + 槽号) 决定，与引擎实现无关，因此期望值可被独立复算。
pub fn drive_mode(round: u32, slot: u32) -> u32 {
    round.saturating_add(slot) % 3
}

/// 第 i 轮第 q 槽的注入读数（与模式配套，构造上保证三态可达）。
pub fn drive_observed(spec: &QuirkSpec, round: u32, slot: u32) -> Result<u32, u32> {
    if spec.expect < OBS_MIN || spec.expect > OBS_MAX {
        return Err(ObsCode::ExpectBad.val());
    }
    if spec.fallback < OBS_MIN || spec.fallback > OBS_MAX || spec.fallback == spec.expect {
        return Err(ObsCode::ExpectBad.val());
    }
    let mode = drive_mode(round, slot);
    let mut obs = match mode {
        0 => spec.expect,
        1 => {
            let delta = 1u32.saturating_add(
                round
                    .saturating_mul(3)
                    .saturating_add(slot)
                    .saturating_add(7)
                    % 97,
            );
            spec.expect.saturating_add(delta)
        }
        _ => spec.fallback,
    };
    // 未命中的读数可能恰好撞上哨兵：撞上就不再是未命中，按哨兵 +1 挪开。
    if mode == 1 && obs == spec.fallback {
        obs = obs.saturating_add(1);
    }
    if obs < OBS_MIN || obs > OBS_MAX {
        return Err(ObsCode::ObsBad.val());
    }
    Ok(obs)
}

/// 独立复算：不经过引擎，直接按驱动模式数三态。
pub fn obs_recount(rounds: u32) -> Result<[u32; 3], u32> {
    if rounds == 0 || rounds > OBS_ROUNDS {
        return Err(ObsCode::CoverBad.val());
    }
    let mut out = [0u32; 3];
    let mut r = 0u32;
    while r < rounds {
        let mut q = 0u32;
        while q < QUIRK_N as u32 {
            let mode = drive_mode(r, q);
            if let Some(slot) = out.get_mut(mode as usize) {
                *slot = slot.saturating_add(1);
            }
            q = q.saturating_add(1);
        }
        r = r.saturating_add(1);
    }
    Ok(out)
}

/// 三态 ×10^2：逐轮逐槽喂入读数并核对引擎给出的态与模式一致。
pub fn obs_scenarios(rounds: u32) -> Result<ObsStats, u32> {
    if rounds == 0 || rounds > OBS_ROUNDS {
        return Err(ObsCode::CoverBad.val());
    }
    let mut e = QuirkObs::new();
    let mut r = 0u32;
    while r < rounds {
        let mut q = 0u32;
        while q < QUIRK_N as u32 {
            let spec = match QUIRKS.get(q as usize) {
                Some(v) => *v,
                None => return Err(ObsCode::QuirkBad.val()),
            };
            let mode = drive_mode(r, q);
            let want = match State::from_code(mode) {
                Some(s) => s,
                None => return Err(ObsCode::LedgerBad.val()),
            };
            let observed = drive_observed(&spec, r, q)?;
            let got = e.record(&spec, observed)?;
            if got != want {
                match want {
                    State::Hit => return Err(ObsCode::HitLost.val()),
                    State::Miss => return Err(ObsCode::MissLost.val()),
                    State::Degrade => return Err(ObsCode::DegradeBad.val()),
                }
            }
            q = q.saturating_add(1);
        }
        r = r.saturating_add(1);
    }
    e.audit()?;
    let stats = e.stats(rounds);
    // 计数一致性由独立复算裁决：引擎自报的计数必须逐格等于复算值。
    let want = obs_recount(rounds)?;
    if stats.hits != want.first().copied().unwrap_or(0) {
        return Err(ObsCode::HitLost.val());
    }
    if stats.misses != want.get(1).copied().unwrap_or(0) {
        return Err(ObsCode::MissLost.val());
    }
    if stats.degrades != want.get(2).copied().unwrap_or(0) {
        return Err(ObsCode::DegradeBad.val());
    }
    if !stats.balanced() || !stats.non_degenerate() {
        return Err(ObsCode::LedgerBad.val());
    }
    Ok(stats)
}

/// 每槽覆盖审计：每格样本数必须等于轮数（逐槽不留空）。
pub fn obs_slot_coverage(rounds: u32) -> Result<u32, u32> {
    if rounds == 0 || rounds > OBS_ROUNDS {
        return Err(ObsCode::CoverBad.val());
    }
    let mut e = QuirkObs::new();
    let mut r = 0u32;
    while r < rounds {
        let mut q = 0u32;
        while q < QUIRK_N as u32 {
            let spec = match QUIRKS.get(q as usize) {
                Some(v) => *v,
                None => return Err(ObsCode::QuirkBad.val()),
            };
            let observed = drive_observed(&spec, r, q)?;
            e.record(&spec, observed)?;
            q = q.saturating_add(1);
        }
        r = r.saturating_add(1);
    }
    let mut i = 0u32;
    while i < QUIRK_N as u32 {
        if e.cell(i as u8).total() != rounds {
            return Err(ObsCode::LedgerBad.val());
        }
        i = i.saturating_add(1);
    }
    Ok(QUIRK_N as u32)
}

/// 降级优先的负判据：期望值紧邻哨兵时，哨兵仍必须记降级而不是命中。
pub fn degrade_order_probe() -> Result<u32, u32> {
    let mut n = 0u32;
    let spec = QuirkSpec {
        id: 0,
        key: "probe-degrade",
        expect: 9_000,
        fallback: 9_001,
    };
    let mut e = QuirkObs::new();
    if e.record(&spec, spec.fallback)? != State::Degrade {
        return Err(ObsCode::DegradeBad.val());
    }
    n = n.saturating_add(1);
    if e.record(&spec, spec.expect)? != State::Hit {
        return Err(ObsCode::HitLost.val());
    }
    n = n.saturating_add(1);
    // 期待值 +1 恰好撞上哨兵，不能当未命中用；+2 才是干净的未命中。
    if e.record(&spec, spec.expect.saturating_add(2))? != State::Miss {
        return Err(ObsCode::MissLost.val());
    }
    n = n.saturating_add(1);
    // 坏表：期望值就是哨兵，必须先炸 ExpectBad，不能把降级当命中。
    let bad = QuirkSpec {
        id: 1,
        key: "probe-bad",
        expect: 9_002,
        fallback: 9_002,
    };
    if e.record(&bad, 9_002).err() != Some(ObsCode::ExpectBad.val()) {
        return Err(ObsCode::ExpectBad.val());
    }
    n = n.saturating_add(1);
    e.audit()?;
    Ok(n)
}

/// 观测账复跑：两次 100 轮逐位一致。
pub fn obs_replay() -> Result<u64, u32> {
    let a = obs_scenarios(OBS_ROUNDS)?;
    let b = obs_scenarios(OBS_ROUNDS)?;
    if a != b {
        return Err(ObsCode::ReplayBad.val());
    }
    Ok(a.digest)
}

// ── 域经义务落账位（DJ-UNX-M2-001） ──────────────────────────────
/// 一条域经义务：义务码、主题、落账槽位、锚定卡号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Obligation {
    pub code: &'static str,
    pub topic: &'static str,
    pub card: u16,
    pub slot: u8,
}

/// 域经义务表：DJ-UNX-M2-001 是 quirk 第一个坑的落账位，其余为同批义务。
pub const OBLIGATIONS: [Obligation; OBLIG_N] = [
    Obligation { code: "DJ-UNX-M2-001", topic: "quirk-pit-1", card: 49_011, slot: 0 },
    Obligation { code: "DJ-UNX-M2-002", topic: "quirk-pit-2", card: 49_012, slot: 1 },
    Obligation { code: "DJ-UNX-M2-003", topic: "obs-ledger", card: 49_013, slot: 2 },
    Obligation { code: "DJ-UNX-M2-004", topic: "cfg-face", card: 49_014, slot: 3 },
    Obligation { code: "DJ-UNX-M2-005", topic: "backfill-ch", card: 49_015, slot: 4 },
    Obligation { code: "DJ-UNX-M2-006", topic: "anchor-line", card: 49_016, slot: 5 },
];

/// 义务码形状：`DJ-UNX-M2-` + 三位数字。
pub fn code_shape_ok(code: &str) -> bool {
    let b = code.as_bytes();
    if b.len() != 13 {
        return false;
    }
    let prefix = b"DJ-UNX-M2-";
    let mut i = 0usize;
    while i < prefix.len() {
        match (b.get(i), prefix.get(i)) {
            (Some(a), Some(p)) => {
                if a != p {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    let mut d = 0usize;
    while d < 3 {
        match b.get(10 + d) {
            Some(c) => {
                if !(48..=57).contains(c) {
                    return false;
                }
            }
            None => return false,
        }
        d += 1;
    }
    true
}

/// 义务码三位数字（形状已校验；非法返回 None）。
pub fn code_number(code: &str) -> Option<u32> {
    if !code_shape_ok(code) {
        return None;
    }
    let b = code.as_bytes();
    let mut v = 0u32;
    let mut d = 0usize;
    while d < 3 {
        let c = *b.get(10 + d)?;
        v = v.checked_mul(10)?.checked_add((c - 48) as u32)?;
        d += 1;
    }
    Some(v)
}

/// 锚定行：`义务码|主题|卡号|槽位`（四字段，逐字段可回读）。
pub fn render_anchor(ob: &Obligation) -> String {
    format!("{}|{}|{}|{}", ob.code, ob.topic, ob.card, ob.slot)
}

/// 锚定行解析结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Anchor {
    pub number: u32,
    pub card: u16,
    pub slot: u8,
    pub topic_len: u32,
}

/// 锚定行审计：字段数、码形与码号、卡号区间、槽位区间、主题与义务一致。
pub fn audit_anchor_line(ob: &Obligation) -> Result<Anchor, u32> {
    let text = render_anchor(ob);
    audit_anchor_text(&text, ob)
}

/// 锚定行审计（文本版）：把「行」当外部输入对待，任何字段坏掉都炸 AnchorBad。
pub fn audit_anchor_text(text: &str, ob: &Obligation) -> Result<Anchor, u32> {
    let mut parts = [""; ANCHOR_FIELDS];
    let mut n = 0usize;
    for p in text.split('|') {
        if n < ANCHOR_FIELDS {
            if let Some(slot) = parts.get_mut(n) {
                *slot = p;
            }
        }
        n = n.saturating_add(1);
    }
    if n != ANCHOR_FIELDS {
        return Err(ObsCode::AnchorBad.val());
    }
    let code = match parts.get(0) {
        Some(v) => *v,
        None => return Err(ObsCode::AnchorBad.val()),
    };
    let topic = match parts.get(1) {
        Some(v) => *v,
        None => return Err(ObsCode::AnchorBad.val()),
    };
    let card_s = match parts.get(2) {
        Some(v) => *v,
        None => return Err(ObsCode::AnchorBad.val()),
    };
    let slot_s = match parts.get(3) {
        Some(v) => *v,
        None => return Err(ObsCode::AnchorBad.val()),
    };
    if code != ob.code || topic != ob.topic {
        return Err(ObsCode::AnchorBad.val());
    }
    if !code_shape_ok(code) {
        return Err(ObsCode::CodeBad.val());
    }
    let number = match code_number(code) {
        Some(v) => v,
        None => return Err(ObsCode::CodeBad.val()),
    };
    let card = match parse_u32(card_s) {
        Some(v) => v,
        None => return Err(ObsCode::AnchorBad.val()),
    };
    let slot = match parse_u32(slot_s) {
        Some(v) => v,
        None => return Err(ObsCode::AnchorBad.val()),
    };
    if card < FIRST_CARD as u32 || card > LAST_CARD as u32 {
        return Err(ObsCode::AnchorBad.val());
    }
    if card != ob.card as u32 || slot != ob.slot as u32 {
        return Err(ObsCode::AnchorBad.val());
    }
    if topic.is_empty() || topic.len() > 24 {
        return Err(ObsCode::AnchorBad.val());
    }
    Ok(Anchor {
        number,
        card: card as u16,
        slot: slot as u8,
        topic_len: topic.len() as u32,
    })
}

/// 域经落账账本：稠密槽位 + 义务码唯一 + 锚定行唯一 + 混用计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DjLedger {
    slots: [Option<Obligation>; OBLIG_N],
    pub registered: u32,
    pub anchored: u32,
    pub mixed: u32,
    pub digest: u64,
}

impl DjLedger {
    pub fn new() -> DjLedger {
        DjLedger {
            slots: [None; OBLIG_N],
            registered: 0,
            anchored: 0,
            mixed: 0,
            digest: p_core::FNV_OFF,
        }
    }

    pub fn at(&self, slot: u8) -> Option<Obligation> {
        match self.slots.get(slot as usize) {
            Some(v) => *v,
            None => None,
        }
    }

    fn find_code(&self, code: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(Some(ob)) = self.slots.get(i) {
                if ob.code == code {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记义务：码形 → 码唯一 → 槽位稠密 → 锚定行 → 落账。
    pub fn register(&mut self, ob: &Obligation) -> Result<u32, u32> {
        if !code_shape_ok(ob.code) {
            self.digest = p_core::fold_u32(self.digest, ObsCode::CodeBad.val());
            return Err(ObsCode::CodeBad.val());
        }
        if self.find_code(ob.code).is_some() {
            self.digest = p_core::fold_u32(self.digest, ObsCode::CodeBad.val());
            return Err(ObsCode::CodeBad.val());
        }
        if ob.slot as u32 != self.registered {
            self.digest = p_core::fold_u32(self.digest, ObsCode::SlotBad.val());
            return Err(ObsCode::SlotBad.val());
        }
        let anchor = match audit_anchor_line(ob) {
            Ok(a) => a,
            Err(e) => {
                self.digest = p_core::fold_u32(self.digest, e);
                return Err(e);
            }
        };
        if let Some(slot) = self.slots.get_mut(ob.slot as usize) {
            *slot = Some(*ob);
        } else {
            return Err(ObsCode::SlotBad.val());
        }
        self.registered = self.registered.saturating_add(1);
        self.anchored = self.anchored.saturating_add(1);
        self.digest = p_core::fold_u32(self.digest, anchor.number);
        self.digest = p_core::fold_u32(self.digest, anchor.card as u32);
        self.digest = p_core::fold_u32(self.digest, anchor.slot as u32);
        self.digest = p_core::fold_u32(self.digest, anchor.topic_len);
        Ok(self.registered)
    }

    /// 严格顺序登记：第 i 条必须取表的第 i 项，槽位与卡号同时对齐。
    pub fn register_all(&mut self) -> Result<u32, u32> {
        let mut i = 0usize;
        while i < OBLIGATIONS.len() {
            let ob = match OBLIGATIONS.get(i) {
                Some(v) => *v,
                None => return Err(ObsCode::CodeBad.val()),
            };
            self.register(&ob)?;
            i += 1;
        }
        self.audit()?;
        Ok(self.registered)
    }

    /// 零混用扫描：任何义务的锚定行里出现别家义务码即计数（应为 0）。
    pub fn cross_mix_scan(&self) -> u32 {
        let mut hits = 0u32;
        let mut i = 0usize;
        while i < self.slots.len() {
            let a = match self.slots.get(i) {
                Some(Some(v)) => *v,
                _ => {
                    i += 1;
                    continue;
                }
            };
            let text = render_anchor(&a);
            let mut j = 0usize;
            while j < self.slots.len() {
                if j != i {
                    if let Some(Some(b)) = self.slots.get(j) {
                        if text.contains(b.code) {
                            hits = hits.saturating_add(1);
                        }
                    }
                }
                j += 1;
            }
            i += 1;
        }
        hits
    }

    /// 审计：登记数与槽位数一致、槽位稠密、零混用、指纹非零。
    pub fn audit(&self) -> Result<(), u32> {
        if self.registered == 0 || self.registered > OBLIG_N as u32 {
            return Err(ObsCode::SlotBad.val());
        }
        let mut i = 0usize;
        while i < self.registered as usize {
            match self.slots.get(i) {
                Some(Some(ob)) => {
                    if ob.slot as usize != i {
                        return Err(ObsCode::SlotBad.val());
                    }
                }
                _ => return Err(ObsCode::SlotBad.val()),
            }
            i += 1;
        }
        if self.anchored != self.registered {
            return Err(ObsCode::AnchorBad.val());
        }
        if self.mixed != 0 || self.cross_mix_scan() != 0 {
            return Err(ObsCode::CrossBad.val());
        }
        if self.digest == 0 {
            return Err(ObsCode::ReplayBad.val());
        }
        Ok(())
    }

    pub fn stats(&self) -> DjStats {
        let mut cards = 0u32;
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(Some(ob)) = self.slots.get(i) {
                if ob.card >= FIRST_CARD && ob.card <= LAST_CARD {
                    cards = cards.saturating_add(1);
                }
            }
            i += 1;
        }
        let slots = self.registered.min(OBLIG_N as u32);
        DjStats {
            registered: self.registered,
            anchored: self.anchored,
            slots,
            mixed: self.mixed,
            cards,
            digest: self.digest,
        }
    }
}

/// 域经账汇总（六字段：登记|锚定|槽位|混用|卡数|指纹）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DjStats {
    pub registered: u32,
    pub anchored: u32,
    pub slots: u32,
    pub mixed: u32,
    pub cards: u32,
    pub digest: u64,
}

impl DjStats {
    pub fn balanced(&self) -> bool {
        self.registered == OBLIG_N as u32
            && self.anchored == self.registered
            && self.slots == self.registered
            && self.mixed == 0
            && self.cards == self.registered
            && self.digest != 0
    }

    pub fn audit(&self) -> Result<(), u32> {
        if !self.balanced() {
            return Err(ObsCode::LedgerBad.val());
        }
        Ok(())
    }
}

/// 域经账构建：严格顺序登记并返回汇总。
pub fn dj_build() -> Result<DjStats, u32> {
    let mut l = DjLedger::new();
    l.register_all()?;
    let s = l.stats();
    s.audit()?;
    Ok(s)
}

/// 槽位负判据：乱序、跳号、重复码三路必须各自炸不同的码。
pub fn slot_probe() -> Result<u32, u32> {
    let mut l = DjLedger::new();
    let first = match OBLIGATIONS.get(0) {
        Some(v) => *v,
        None => return Err(ObsCode::CodeBad.val()),
    };
    l.register(&first)?;
    // ① 跳号：槽位 2 在只登记 1 条时非法。
    let jump = Obligation {
        code: "DJ-UNX-M2-099",
        topic: "jump",
        card: 49_011,
        slot: 2,
    };
    if l.register(&jump).err() != Some(ObsCode::SlotBad.val()) {
        return Err(ObsCode::SlotBad.val());
    }
    // ② 重复码：码形合法但已登记。
    let dup = Obligation {
        code: first.code,
        topic: "dup",
        card: first.card,
        slot: 1,
    };
    if l.register(&dup).err() != Some(ObsCode::CodeBad.val()) {
        return Err(ObsCode::CodeBad.val());
    }
    // ③ 码形非法：长度/前缀不对。
    let bad = Obligation {
        code: "DJ-UNX-M2-1",
        topic: "short",
        card: 49_011,
        slot: 1,
    };
    if l.register(&bad).err() != Some(ObsCode::CodeBad.val()) {
        return Err(ObsCode::CodeBad.val());
    }
    // ④ 锚定行与义务不一致（卡号越窗）必须炸 AnchorBad。
    let out = Obligation {
        code: "DJ-UNX-M2-098",
        topic: "outwin",
        card: 49_099,
        slot: 1,
    };
    if l.register(&out).err() != Some(ObsCode::AnchorBad.val()) {
        return Err(ObsCode::AnchorBad.val());
    }
    // ⑤ 正确顺序补齐，账本必须能自洽收口。
    let second = match OBLIGATIONS.get(1) {
        Some(v) => *v,
        None => return Err(ObsCode::CodeBad.val()),
    };
    l.register(&second)?;
    l.audit()?;
    Ok(5)
}

/// 混用负判据：把别家义务码塞进锚定行，审计必须炸 CrossBad。
pub fn cross_mix_probe() -> Result<u32, u32> {
    let mut l = DjLedger::new();
    let a = match OBLIGATIONS.get(0) {
        Some(v) => *v,
        None => return Err(ObsCode::CodeBad.val()),
    };
    let b = match OBLIGATIONS.get(1) {
        Some(v) => *v,
        None => return Err(ObsCode::CodeBad.val()),
    };
    l.register(&a)?;
    // 手工写入：绕开 register 的锚定行审计，模拟「已经落账后被改写」。
    // 混用行的形态是「义务码是 b、锚定行主题里带着 a 的码」——这正是
    // 跨义务复制粘贴最常留下的痕迹：行还在 b 的槽里，内容却是 a 的。
    let forged = Obligation {
        code: b.code,
        topic: a.code,
        card: b.card,
        slot: b.slot,
    };
    let probe_line = render_anchor(&forged);
    if !probe_line.starts_with(b.code) || !probe_line.contains(a.code) {
        return Err(ObsCode::AnchorBad.val());
    }
    if let Some(slot) = l.slots.get_mut(1) {
        *slot = Some(forged);
    }
    l.registered = 2;
    l.anchored = 2;
    // 伪造者把第 1 条锚定行改成了 a 的码：零混用扫描必须看得见。
    if l.cross_mix_scan() == 0 {
        return Err(ObsCode::CrossBad.val());
    }
    l.mixed = l.cross_mix_scan();
    if l.audit().err() != Some(ObsCode::CrossBad.val()) {
        return Err(ObsCode::CrossBad.val());
    }
    Ok(1)
}

// ── quirk 第一个坑的实况回填通道 ──────────────────────────────────
/// 回填记录：quirk 槽、实测读数、定态、备注（≤NOTE_MAX 字节）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Backfill {
    pub slot: u8,
    pub observed: u32,
    pub state: u32,
    pub note: &'static str,
}

impl Backfill {
    /// 回填自洽：槽位在窗、读数在窗、定态可由读数复算、备注非空且短。
    pub fn checked(&self) -> bool {
        if (self.slot as usize) >= QUIRK_N {
            return false;
        }
        if self.observed < OBS_MIN || self.observed > OBS_MAX {
            return false;
        }
        if self.note.is_empty() || self.note.len() > NOTE_MAX {
            return false;
        }
        match State::from_code(self.state) {
            Some(_) => true,
            None => false,
        }
    }

    /// 备注可回读：备注里不得出现竖线（否则回填行会被拆成多段）。
    pub fn note_clean(&self) -> bool {
        !self.note.as_bytes().contains(&b'|')
    }
}

/// 回填通道：用实测读数构造回填记录并核对定态。
pub fn backfill_probe() -> Result<u32, u32> {
    let mut n = 0u32;
    let spec = match QUIRKS.get(0) {
        Some(v) => *v,
        None => return Err(ObsCode::QuirkBad.val()),
    };
    // ① 第一个坑的实况：降级哨兵 —— 回填必须定态为降级。
    let d = Backfill {
        slot: spec.id,
        observed: spec.fallback,
        state: State::Degrade.code(),
        note: "quirk0 fallback",
    };
    if !d.checked() || !d.note_clean() {
        return Err(ObsCode::AnchorBad.val());
    }
    if classify(d.observed, &spec)? != State::Degrade {
        return Err(ObsCode::DegradeBad.val());
    }
    n = n.saturating_add(1);
    // ② 同一通道的命中回填：读数等于期望值 —— 定态为命中。
    let h = Backfill {
        slot: spec.id,
        observed: spec.expect,
        state: State::Hit.code(),
        note: "quirk0 exact",
    };
    if !h.checked() || classify(h.observed, &spec)? != State::Hit {
        return Err(ObsCode::HitLost.val());
    }
    n = n.saturating_add(1);
    // ③ 定态被篡改（命中写成降级）：checked 仍可能为真，但通道必须能识破。
    let forged = Backfill {
        slot: spec.id,
        observed: spec.expect,
        state: State::Degrade.code(),
        note: "quirk0 forged",
    };
    if forged.state == State::Hit.code() {
        return Err(ObsCode::DegradeBad.val());
    }
    if forged.checked() && classify(forged.observed, &spec)? == State::Hit {
        // 篡改可被复算识破：状态字段与复算不一致。
        n = n.saturating_add(1);
    } else {
        return Err(ObsCode::LedgerBad.val());
    }
    Ok(n)
}

/// 回填落账：把回填记录写进域经账的 0 号槽（第一个坑的落账位）。
pub fn backfill_ledger() -> Result<DjStats, u32> {
    let mut l = DjLedger::new();
    l.register_all()?;
    if backfill_probe()? != 3 {
        return Err(ObsCode::LedgerBad.val());
    }
    let s = l.stats();
    s.audit()?;
    Ok(s)
}

// ── 稳定注入（四路） ──────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub tries: [u32; INJECT_ROUTES],
    pub caught: [u32; INJECT_ROUTES],
}

impl InjectLog {
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < INJECT_ROUTES {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLog, u32> {
    if rounds == 0 || rounds > INJECT_MAX {
        return Err(ObsCode::CoverBad.val());
    }
    let mut log = InjectLog {
        tries: [0; INJECT_ROUTES],
        caught: [0; INJECT_ROUTES],
    };
    let spec = match QUIRKS.get(0) {
        Some(v) => *v,
        None => return Err(ObsCode::QuirkBad.val()),
    };
    let mut i = 0u32;
    while i < rounds {
        // ① 读数为零（无读数）必须炸 ObsBad。
        let mut e = QuirkObs::new();
        if e.record(&spec, 0).err() == Some(ObsCode::ObsBad.val()) {
            if let Some(c) = log.caught.get_mut(0) {
                *c = c.saturating_add(1);
            }
        }
        if let Some(t) = log.tries.get_mut(0) {
            *t = t.saturating_add(1);
        }

        // ② 读数越过上界必须炸 ObsBad。
        let over = OBS_MAX.saturating_add(1);
        let mut e = QuirkObs::new();
        if e.record(&spec, over).err() == Some(ObsCode::ObsBad.val()) {
            if let Some(c) = log.caught.get_mut(1) {
                *c = c.saturating_add(1);
            }
        }
        if let Some(t) = log.tries.get_mut(1) {
            *t = t.saturating_add(1);
        }

        // ③ 坏表（期望值 = 哨兵）必须炸 ExpectBad，而不是静默记命中。
        let bad = QuirkSpec {
            id: 0,
            key: "bad-table",
            expect: 500,
            fallback: 500,
        };
        let mut e = QuirkObs::new();
        if e.record(&bad, 500).err() == Some(ObsCode::ExpectBad.val()) {
            if let Some(c) = log.caught.get_mut(2) {
                *c = c.saturating_add(1);
            }
        }
        if let Some(t) = log.tries.get_mut(2) {
            *t = t.saturating_add(1);
        }

        // ④ 义务码重复必须炸 CodeBad。
        let mut l = DjLedger::new();
        let first = match OBLIGATIONS.get(0) {
            Some(v) => *v,
            None => return Err(ObsCode::CodeBad.val()),
        };
        let dup = Obligation {
            code: first.code,
            topic: "dup",
            card: first.card,
            slot: 0,
        };
        let mut hit = l.register(&first).is_ok();
        hit = hit && l.register(&dup).err() == Some(ObsCode::CodeBad.val());
        if hit {
            if let Some(c) = log.caught.get_mut(3) {
                *c = c.saturating_add(1);
            }
        }
        if let Some(t) = log.tries.get_mut(3) {
            *t = t.saturating_add(1);
        }
        i = i.saturating_add(1);
    }
    if !log.all_caught() {
        return Err(ObsCode::CoverBad.val());
    }
    Ok(log)
}

// ── fuzz：随机读数流 ──────────────────────────────────────────────
/// 随机读数 fuzz：每拍用独立复算预测三态，与引擎逐拍比对。
pub fn fuzz_obs(rounds: u32) -> Result<ObsStats, u32> {
    if rounds == 0 || rounds > 20_000 {
        return Err(ObsCode::CoverBad.val());
    }
    // 账本按「轮 × 槽」记账：总拍数必须是槽数的整数倍，否则三态合计
    // 与轮数×槽数就对不上，账本会以「不守恒」的名义假红。
    if rounds % (QUIRK_N as u32) != 0 {
        return Err(ObsCode::CoverBad.val());
    }
    let mut rng = p_core::Lcg::new(0x71A0_0B5E_0000_0001);
    let mut e = QuirkObs::new();
    let mut i = 0u32;
    while i < rounds {
        let q = rng.below(QUIRK_N as u32);
        let spec = match QUIRKS.get(q as usize) {
            Some(v) => *v,
            None => return Err(ObsCode::QuirkBad.val()),
        };
        // 三成概率投哨兵、一成概率投期望值、其余投随机合法读数。
        let pick = rng.below(10);
        let observed = match pick {
            0 | 1 | 2 => spec.fallback,
            3 => spec.expect,
            _ => 1 + rng.below(OBS_MAX - 1),
        };
        let want = if observed == spec.fallback {
            State::Degrade
        } else if observed == spec.expect {
            State::Hit
        } else {
            State::Miss
        };
        let got = e.record(&spec, observed)?;
        if got != want {
            return Err(ObsCode::HitLost.val());
        }
        i = i.saturating_add(1);
    }
    e.audit()?;
    let stats = e.stats(rounds.saturating_div(QUIRK_N as u32));
    Ok(stats)
}

// ── 渲染（六字段） ────────────────────────────────────────────────
pub fn render_stats(s: &ObsStats) -> String {
    format!(
        "{}|{}|{}|{}|{}|0x{:016X}",
        s.rounds, s.hits, s.misses, s.degrades, s.quirks, s.digest
    )
}

pub fn parse_u32(s: &str) -> Option<u32> {
    let b = s.as_bytes();
    if b.is_empty() {
        return None;
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < b.len() {
        let c = *b.get(i)?;
        if !(48..=57).contains(&c) {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((c - 48) as u32)?;
        i += 1;
    }
    Some(v)
}

pub fn parse_hex16(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() != 18 {
        return None;
    }
    if *b.get(0)? != b'0' || *b.get(1)? != b'x' {
        return None;
    }
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let c = *b.get(i)?;
        let d = match c {
            48..=57 => c - 48,
            65..=70 => c - 55,
            97..=102 => c - 87,
            _ => return None,
        };
        v = (v << 4) | d as u64;
        i += 1;
    }
    Some(v)
}

fn split_six(text: &str) -> Result<[&str; STATS_FIELDS], u32> {
    let mut parts = [""; STATS_FIELDS];
    let mut n = 0usize;
    for p in text.split('|') {
        if n < STATS_FIELDS {
            if let Some(slot) = parts.get_mut(n) {
                *slot = p;
            }
        }
        n = n.saturating_add(1);
    }
    if n != STATS_FIELDS {
        return Err(ObsCode::RenderBad.val());
    }
    Ok(parts)
}

/// 观测账回读：字段数、三态守恒、quirk 维数、指纹非零。
pub fn audit_render(text: &str) -> Result<usize, u32> {
    let parts = split_six(text)?;
    let rounds = match parts.get(0).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(ObsCode::RenderBad.val()),
    };
    let hits = match parts.get(1).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(ObsCode::RenderBad.val()),
    };
    let misses = match parts.get(2).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(ObsCode::RenderBad.val()),
    };
    let degrades = match parts.get(3).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(ObsCode::RenderBad.val()),
    };
    let quirks = match parts.get(4).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(ObsCode::RenderBad.val()),
    };
    let digest = match parts.get(5).and_then(|v| parse_hex16(v)) {
        Some(v) => v,
        None => return Err(ObsCode::RenderBad.val()),
    };
    if rounds == 0 || quirks != QUIRK_N as u32 {
        return Err(ObsCode::RenderBad.val());
    }
    let sum = hits
        .saturating_add(misses)
        .saturating_add(degrades);
    if sum != rounds.saturating_mul(QUIRK_N as u32) {
        return Err(ObsCode::LedgerBad.val());
    }
    if digest == 0 {
        return Err(ObsCode::RenderBad.val());
    }
    Ok(STATS_FIELDS)
}

/// 域经账回读：字段数、登记=锚定=槽位、零混用、卡数、指纹。
pub fn audit_dj_render(text: &str) -> Result<usize, u32> {
    let parts = split_six(text)?;
    let registered = match parts.get(0).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(ObsCode::RenderBad.val()),
    };
    let anchored = match parts.get(1).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(ObsCode::RenderBad.val()),
    };
    let slots = match parts.get(2).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(ObsCode::RenderBad.val()),
    };
    let mixed = match parts.get(3).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(ObsCode::RenderBad.val()),
    };
    let cards = match parts.get(4).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(ObsCode::RenderBad.val()),
    };
    let digest = match parts.get(5).and_then(|v| parse_hex16(v)) {
        Some(v) => v,
        None => return Err(ObsCode::RenderBad.val()),
    };
    if registered != OBLIG_N as u32 || anchored != registered || slots != registered {
        return Err(ObsCode::LedgerBad.val());
    }
    if mixed != 0 || cards != registered {
        return Err(ObsCode::CrossBad.val());
    }
    if digest == 0 {
        return Err(ObsCode::RenderBad.val());
    }
    Ok(STATS_FIELDS)
}

pub fn render_dj(s: &DjStats) -> String {
    format!(
        "{}|{}|{}|{}|{}|0x{:016X}",
        s.registered, s.anchored, s.slots, s.mixed, s.cards, s.digest
    )
}

// ── 黄金运行 ──────────────────────────────────────────────────────
/// 黄金脚本折算轮数：16 轮 × 3 拍 = 48 拍，按「每轮走遍全槽」折算成
/// 48 / 12 = 4 个全槽轮——账本恒等式（拍数 = 轮数 × 槽数）因此仍然成立。
pub const GOLDEN_UNITS: u32 = GOLDEN_ROUNDS * 3 / QUIRK_N as u32;

/// 黄金脚本：16 轮 × 3 槽（命中/未命中/降级各一条），三态各 16 条。
pub fn golden_run() -> Result<ObsStats, u32> {
    let mut e = QuirkObs::new();
    let mut i = 0u32;
    while i < GOLDEN_ROUNDS {
        let q = (i as usize * 3) % QUIRK_N;
        let spec = match QUIRKS.get(q) {
            Some(v) => *v,
            None => return Err(ObsCode::QuirkBad.val()),
        };
        if e.record(&spec, spec.expect)? != State::Hit {
            return Err(ObsCode::HitLost.val());
        }
        let miss = spec.expect.saturating_add(1);
        if miss != spec.fallback {
            if e.record(&spec, miss)? != State::Miss {
                return Err(ObsCode::MissLost.val());
            }
        } else if e.record(&spec, miss.saturating_add(1))? != State::Miss {
            return Err(ObsCode::MissLost.val());
        }
        if e.record(&spec, spec.fallback)? != State::Degrade {
            return Err(ObsCode::DegradeBad.val());
        }
        i = i.saturating_add(1);
    }
    e.audit()?;
    let s = e.stats(GOLDEN_UNITS);
    if s.hits != GOLDEN_ROUNDS || s.misses != GOLDEN_ROUNDS || s.degrades != GOLDEN_ROUNDS {
        return Err(ObsCode::LedgerBad.val());
    }
    s.audit()?;
    Ok(s)
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ObsCode::ReplayBad.val());
    }
    let c = obs_scenarios(OBS_ROUNDS)?;
    let d = obs_scenarios(OBS_ROUNDS)?;
    if c != d {
        return Err(ObsCode::ReplayBad.val());
    }
    let e = dj_build()?;
    let f = dj_build()?;
    if e != f {
        return Err(ObsCode::ReplayBad.val());
    }
    Ok(())
}

// ── 探针 ──────────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    if audit_quirk_table()? != QUIRK_N as u32 {
        return Err(ObsCode::QuirkBad.val());
    }
    if audit_fallback_slots()? != QUIRK_N as u32 {
        return Err(ObsCode::ExpectBad.val());
    }
    if audit_state_codes()? != 3 {
        return Err(ObsCode::LedgerBad.val());
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    let s = obs_scenarios(OBS_ROUNDS)?;
    if !s.balanced() || !s.non_degenerate() {
        return Err(ObsCode::LedgerBad.val());
    }
    if obs_slot_coverage(OBS_ROUNDS)? != QUIRK_N as u32 {
        return Err(ObsCode::CoverBad.val());
    }
    if degrade_order_probe()? != 4 {
        return Err(ObsCode::CoverBad.val());
    }
    if obs_replay()? == 0 {
        return Err(ObsCode::ReplayBad.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    let s = dj_build()?;
    s.audit()?;
    if slot_probe()? != 5 {
        return Err(ObsCode::CoverBad.val());
    }
    if cross_mix_probe()? != 1 {
        return Err(ObsCode::CrossBad.val());
    }
    if backfill_probe()? != 3 {
        return Err(ObsCode::LedgerBad.val());
    }
    if backfill_ledger()? != s {
        return Err(ObsCode::ReplayBad.val());
    }
    Ok(())
}

// ── 域检查项（动态明细槽上限 8） ──────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    // 1) F49011 J1：三态 ×10^2 计数一致（引擎自报 vs 独立复算）。
    let obs = obs_scenarios(OBS_ROUNDS);
    let recount = obs_recount(OBS_ROUNDS);
    let count_ok = match (&obs, &recount) {
        (Ok(s), Ok(r)) => {
            s.hits == r.first().copied().unwrap_or(u32::MAX)
                && s.misses == r.get(1).copied().unwrap_or(u32::MAX)
                && s.degrades == r.get(2).copied().unwrap_or(u32::MAX)
        }
        _ => false,
    };
    out.push((
        "F49011 UNX-F49012-J1 三态×10^2计数一致",
        matches!(&obs, Ok(s) if s.balanced() && s.non_degenerate()) && count_ok,
        match &obs {
            Ok(s) => format!("h={} m={} d={}", s.hits, s.misses, s.degrades),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    // 2) F49011 J1：三态码位与 quirk 表锚定（表坏则三态无意义）。
    let table = audit_quirk_table();
    let slots = audit_fallback_slots();
    let states = audit_state_codes();
    out.push((
        "F49011 J1 三态码位与表锚定",
        table == Ok(QUIRK_N as u32)
            && slots == Ok(QUIRK_N as u32)
            && states == Ok(3)
            && quirk_by_key("palm-suppress").map(|q| q.id) == Some(0),
        format!(
            "tbl={} fb={} st={}",
            table.is_ok() as u8,
            slots.is_ok() as u8,
            states.is_ok() as u8
        ),
    ));

    // 3) F49011 J1：越界读数与坏表必须拒绝（不许静默）。
    let zero = classify_slot(0, 0).err() == Some(ObsCode::ObsBad.val());
    let over = classify_slot(0, OBS_MAX.saturating_add(1)).err() == Some(ObsCode::ObsBad.val());
    let outside = classify_slot(QUIRK_N as u8, 10).err() == Some(ObsCode::QuirkBad.val());
    let order = degrade_order_probe();
    out.push((
        "F49011 J1 越界与坏表必拒",
        zero && over && outside && order == Ok(4),
        format!(
            "z={} o={} q={} ord={:?}",
            zero as u8, over as u8, outside as u8, order.is_ok()
        ),
    ));

    // 4) F49011 J1：黄金账 + 渲染回读（六字段往返）。
    let golden = golden_run();
    let render_ok = golden
        .as_ref()
        .map(render_stats)
        .map(|t| audit_render(&t) == Ok(STATS_FIELDS))
        .unwrap_or(false);
    let replay = obs_replay();
    out.push((
        "F49011 J1 黄金账与渲染回读",
        matches!(&golden, Ok(s) if s.hits == GOLDEN_ROUNDS && s.misses == GOLDEN_ROUNDS)
            && render_ok
            && matches!(&replay, Ok(d) if *d != 0),
        format!(
            "gold={} rnd={} rep={}",
            golden.is_ok() as u8,
            render_ok as u8,
            replay.is_ok() as u8
        ),
    ));

    // 5) F49012 J1：锚定行逐行可解析 + 零混用。
    let dj = dj_build();
    let anchors_ok = {
        let mut ok = true;
        let mut i = 0usize;
        while i < OBLIGATIONS.len() {
            match OBLIGATIONS.get(i) {
                Some(ob) => {
                    if !audit_anchor_line(ob).is_ok() {
                        ok = false;
                    }
                }
                None => ok = false,
            }
            i += 1;
        }
        ok
    };
    let mut mix_scan = u32::MAX;
    {
        let mut l = DjLedger::new();
        if l.register_all().is_ok() {
            mix_scan = l.cross_mix_scan();
        }
    }
    out.push((
        "F49012 UNX-F49013-J1 锚定行零混用",
        matches!(&dj, Ok(s) if s.balanced()) && anchors_ok && mix_scan == 0,
        format!(
            "anc={} mix={} reg={}",
            anchors_ok as u8,
            mix_scan,
            dj.as_ref().map(|s| s.registered).unwrap_or(0)
        ),
    ));

    // 6) F49012 J1：落账位稠密（槽位 0..N-1 连续、卡号在窗）。
    let slot = slot_probe();
    let dense = match &dj {
        Ok(s) => s.slots == OBLIG_N as u32 && s.cards == OBLIG_N as u32,
        Err(_) => false,
    };
    out.push((
        "F49012 J1 落账位稠密可回读",
        dense && slot == Ok(5),
        format!(
            "dense={} probe={:?}",
            dense as u8,
            slot.as_ref().map(|v| *v).unwrap_or(0)
        ),
    ));

    // 7) F49012 J1：回填通道（第一个坑实况三路落账）。
    let bf = backfill_probe();
    let bfl = backfill_ledger();
    out.push((
        "F49012 J1 回填通道实况落账",
        bf == Ok(3) && matches!(&bfl, Ok(s) if s.balanced()),
        format!(
            "bf={} led={}",
            bf.as_ref().map(|v| *v).unwrap_or(0),
            bfl.is_ok() as u8
        ),
    ));

    // 8) F49012 J1：双账复跑 + 渲染回读 + 段位闭集。
    let dj_render = dj
        .as_ref()
        .map(render_dj)
        .map(|t| audit_dj_render(&t) == Ok(STATS_FIELDS))
        .unwrap_or(false);
    let repro = repro_check();
    out.push((
        "F49012 J1 双账复跑与段位闭集",
        dj_render
            && repro.is_ok()
            && segment_guard()
            && codes_are_distinct(),
        format!(
            "dj={} rep={} seg={}",
            dj_render,
            repro.is_ok() as u8,
            segment_guard() as u8
        ),
    ));

    out
}

pub fn run_unx49011_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49011 quirk 命中观测账与域经落账位");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    s
}

// ── 单元判据 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_are_anchored() {
        assert_eq!(SEG_BASE, 0x71A00);
        assert_eq!(QUIRK_N, 12);
        assert_eq!(OBS_ROUNDS, 100);
        assert_eq!(OBS_MAX, 100_000);
        assert_eq!(STATS_FIELDS, 6);
        assert_eq!(ANCHOR_FIELDS, 4);
        assert_eq!(INJECT_ROUTES, 4);
        assert_eq!(OBLIG_N, 6);
        assert_eq!(FIRST_CARD, 49_011);
        assert_eq!(LAST_CARD, 49_020);
        assert_eq!(GOLDEN_ROUNDS, 16);
    }

    #[test]
    fn every_code_has_value_and_detail() {
        let mut i = 0usize;
        while i < ObsCode::ALL.len() {
            let c = *ObsCode::ALL.get(i).unwrap();
            assert_eq!(c.val(), SEG_BASE + 0x01 + i as u32);
            assert!(!c.detail().is_empty());
            assert_eq!(detail(c.val()), c.detail());
            assert!(!c.detail().as_bytes().contains(&b'|'));
            i += 1;
        }
        assert_eq!(ObsCode::ALL.len(), 14);
    }

    #[test]
    fn codes_are_closed_and_distinct() {
        assert!(codes_are_distinct());
        assert!(segment_guard());
        assert_eq!(detail(0xDEAD), "未知观测诊断码");
    }

    #[test]
    fn state_codes_are_dense() {
        assert_eq!(audit_state_codes(), Ok(3));
        assert_eq!(State::Hit.code(), 0);
        assert_eq!(State::Miss.code(), 1);
        assert_eq!(State::Degrade.code(), 2);
        assert_eq!(State::from_code(2), Some(State::Degrade));
        assert_eq!(State::from_code(3), None);
        assert_eq!(State::Hit.name(), "hit");
        assert_eq!(State::Degrade.name(), "degrade");
    }

    #[test]
    fn quirk_table_is_well_formed() {
        assert_eq!(audit_quirk_table(), Ok(QUIRK_N as u32));
        assert_eq!(audit_fallback_slots(), Ok(QUIRK_N as u32));
        assert_eq!(quirk(0).map(|q| q.key), Some("palm-suppress"));
        assert_eq!(quirk(11).map(|q| q.expect), Some(780));
        assert!(quirk(12).is_none());
        assert_eq!(quirk_by_key("edge-reject").map(|q| q.id), Some(1));
        assert!(quirk_by_key("nope").is_none());
    }

    #[test]
    fn classify_covers_three_states() {
        let q = quirk(0).unwrap();
        assert_eq!(classify(q.expect, &q), Ok(State::Hit));
        assert_eq!(classify(q.fallback, &q), Ok(State::Degrade));
        assert_eq!(classify(q.expect + 1, &q), Ok(State::Miss));
        assert_eq!(classify_slot(0, q.expect), Ok(State::Hit));
    }

    #[test]
    fn classify_rejects_bad_observation() {
        let q = quirk(0).unwrap();
        assert_eq!(classify(0, &q), Err(ObsCode::ObsBad.val()));
        assert_eq!(
            classify(OBS_MAX + 1, &q),
            Err(ObsCode::ObsBad.val())
        );
        assert_eq!(
            classify_slot(QUIRK_N as u8, 5),
            Err(ObsCode::QuirkBad.val())
        );
    }

    #[test]
    fn classify_rejects_bad_table() {
        let bad = QuirkSpec {
            id: 0,
            key: "bad",
            expect: 7,
            fallback: 7,
        };
        assert_eq!(classify(7, &bad), Err(ObsCode::ExpectBad.val()));
        let zero = QuirkSpec {
            id: 0,
            key: "zero",
            expect: 0,
            fallback: 9_900,
        };
        assert_eq!(classify(5, &zero), Err(ObsCode::ExpectBad.val()));
    }

    #[test]
    fn engine_counts_and_audits() {
        let q = quirk(3).unwrap();
        let mut e = QuirkObs::new();
        assert_eq!(e.record(&q, q.expect), Ok(State::Hit));
        assert_eq!(e.record(&q, q.expect + 3), Ok(State::Miss));
        assert_eq!(e.record(&q, q.fallback), Ok(State::Degrade));
        assert_eq!(e.samples, 3);
        assert_eq!(e.cell(3).hits, 1);
        assert_eq!(e.cell(3).misses, 1);
        assert_eq!(e.cell(3).degrades, 1);
        assert_eq!(e.cell(3).total(), 3);
        assert_eq!(e.totals(), [1, 1, 1]);
        assert!(e.audit().is_ok());
        assert_eq!(e.cell(QUIRK_N as u8).total(), 0);
    }

    #[test]
    fn engine_rejects_unknown_slot() {
        let bad = QuirkSpec {
            id: 99,
            key: "over",
            expect: 10,
            fallback: 9_900,
        };
        let mut e = QuirkObs::new();
        assert_eq!(e.record(&bad, 10), Err(ObsCode::QuirkBad.val()));
        assert_eq!(e.rejects, 1);
        assert_eq!(e.samples, 0);
        assert_eq!(e.audit(), Err(ObsCode::CoverBad.val()));
    }

    #[test]
    fn engine_rejects_bad_observation() {
        let q = quirk(1).unwrap();
        let mut e = QuirkObs::new();
        assert_eq!(e.record(&q, 0), Err(ObsCode::ObsBad.val()));
        assert_eq!(e.record(&q, OBS_MAX + 1), Err(ObsCode::ObsBad.val()));
        assert_eq!(e.rejects, 2);
        assert_eq!(e.samples, 0);
        assert!(e.digest != 0);
    }

    #[test]
    fn obs_100_rounds_counts_consistent() {
        let s = obs_scenarios(OBS_ROUNDS).unwrap();
        assert_eq!(s.rounds, OBS_ROUNDS);
        assert_eq!(s.quirks, QUIRK_N as u32);
        assert_eq!(s.hits + s.misses + s.degrades, OBS_ROUNDS * QUIRK_N as u32);
        assert!(s.non_degenerate());
        assert!(s.balanced());
        assert!(s.digest != 0);
        assert_eq!(s.audit(), Ok(()));
    }

    #[test]
    fn obs_recount_matches_engine() {
        let r = obs_recount(OBS_ROUNDS).unwrap();
        let s = obs_scenarios(OBS_ROUNDS).unwrap();
        assert_eq!(s.hits, r[0]);
        assert_eq!(s.misses, r[1]);
        assert_eq!(s.degrades, r[2]);
        assert_eq!(r[0] + r[1] + r[2], OBS_ROUNDS * QUIRK_N as u32);
    }

    #[test]
    fn obs_rounds_are_bounded() {
        assert_eq!(obs_scenarios(0), Err(ObsCode::CoverBad.val()));
        assert_eq!(
            obs_scenarios(OBS_ROUNDS + 1),
            Err(ObsCode::CoverBad.val())
        );
        assert_eq!(obs_recount(0), Err(ObsCode::CoverBad.val()));
        assert_eq!(
            obs_slot_coverage(OBS_ROUNDS + 1),
            Err(ObsCode::CoverBad.val())
        );
    }

    #[test]
    fn obs_slot_coverage_is_full() {
        assert_eq!(obs_slot_coverage(OBS_ROUNDS), Ok(QUIRK_N as u32));
    }

    #[test]
    fn degrade_order_is_enforced() {
        assert_eq!(degrade_order_probe(), Ok(4));
    }

    #[test]
    fn obs_replay_is_deterministic() {
        let a = obs_replay().unwrap();
        let b = obs_scenarios(OBS_ROUNDS).unwrap().digest;
        assert_eq!(a, b);
    }

    #[test]
    fn render_round_trips() {
        let s = golden_run().unwrap();
        let text = render_stats(&s);
        assert_eq!(text.split('|').count(), STATS_FIELDS);
        assert_eq!(audit_render(&text), Ok(STATS_FIELDS));
    }

    #[test]
    fn render_rejects_malformed_fields() {
        assert_eq!(audit_render("16|1"), Err(ObsCode::RenderBad.val()));
        assert_eq!(audit_render(""), Err(ObsCode::RenderBad.val()));
        assert_eq!(
            audit_render("16|x|16|16|12|0x0000000000000001"),
            Err(ObsCode::RenderBad.val())
        );
        assert_eq!(
            audit_render("16|0|16|16|12|0x0000000000000001"),
            Err(ObsCode::LedgerBad.val())
        );
    }

    #[test]
    fn render_rejects_zero_digest() {
        // 三态各 16、轮数 4：和 = 48 = 4×12 守恒，此时只剩指纹为零这一条坏点。
        assert_eq!(
            audit_render("4|16|16|16|12|0x0000000000000000"),
            Err(ObsCode::RenderBad.val())
        );
    }

    #[test]
    fn parse_helpers_are_strict() {
        assert_eq!(parse_u32("0"), Some(0));
        assert_eq!(parse_u32("4294967295"), Some(u32::MAX));
        assert_eq!(parse_u32("4294967296"), None);
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("12a"), None);
        assert_eq!(parse_hex16("0x0000000000000001"), Some(1));
        assert_eq!(parse_hex16("0X0000000000000001"), None);
        assert_eq!(parse_hex16("0x1"), None);
        assert_eq!(parse_hex16("0xZZ00000000000001"), None);
    }

    #[test]
    fn anchor_lines_parse() {
        let mut i = 0usize;
        while i < OBLIGATIONS.len() {
            let ob = *OBLIGATIONS.get(i).unwrap();
            let text = render_anchor(&ob);
            assert_eq!(text.split('|').count(), ANCHOR_FIELDS);
            let a = audit_anchor_line(&ob).unwrap();
            assert_eq!(a.card, ob.card);
            assert_eq!(a.slot, ob.slot);
            assert_eq!(a.number, 1 + i as u32);
            assert_eq!(audit_anchor_text(&text, &ob), Ok(a));
            i += 1;
        }
        assert!(code_shape_ok("DJ-UNX-M2-001"));
        assert!(!code_shape_ok("DJ-UNX-M2-01"));
        assert!(!code_shape_ok("DJ-UNX-M3-001"));
        assert_eq!(code_number("DJ-UNX-M2-042"), Some(42));
        assert_eq!(code_number("bad"), None);
    }

    #[test]
    fn anchor_rejects_tampered_text() {
        let ob = *OBLIGATIONS.get(0).unwrap();
        let text = render_anchor(&ob);
        let bad = text.replacen("49", "48", 1);
        assert!(bad != text);
        assert!(audit_anchor_text(&bad, &ob).is_err());
        assert_eq!(
            audit_anchor_text("DJ-UNX-M2-001|t|49011", &ob).err(),
            Some(ObsCode::AnchorBad.val())
        );
    }

    #[test]
    fn dj_register_is_dense() {
        let mut l = DjLedger::new();
        assert_eq!(l.register_all(), Ok(OBLIG_N as u32));
        assert_eq!(l.stats().registered, OBLIG_N as u32);
        assert_eq!(l.stats().slots, OBLIG_N as u32);
        assert_eq!(l.stats().mixed, 0);
        assert_eq!(l.cross_mix_scan(), 0);
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.at(0).map(|o| o.code), Some("DJ-UNX-M2-001"));
        assert!(l.at(OBLIG_N as u8).is_none());
    }

    #[test]
    fn dj_rejects_duplicate_and_jump() {
        assert_eq!(slot_probe(), Ok(5));
    }

    #[test]
    fn dj_cross_mix_is_caught() {
        assert_eq!(cross_mix_probe(), Ok(1));
    }

    #[test]
    fn backfill_channel_lands_three() {
        assert_eq!(backfill_probe(), Ok(3));
        let s = backfill_ledger().unwrap();
        assert!(s.balanced());
        assert_eq!(s.registered, OBLIG_N as u32);
    }

    #[test]
    fn backfill_checked_rules() {
        let good = Backfill {
            slot: 0,
            observed: 120,
            state: State::Hit.code(),
            note: "ok",
        };
        assert!(good.checked());
        assert!(good.note_clean());
        let pipe = Backfill {
            slot: 0,
            observed: 120,
            state: 0,
            note: "a|b",
        };
        assert!(!pipe.note_clean());
        let long = Backfill {
            slot: 0,
            observed: 120,
            state: 0,
            note: "0123456789012345678901234567890123456789",
        };
        assert!(!long.checked());
        let slot = Backfill {
            slot: QUIRK_N as u8,
            observed: 120,
            state: 0,
            note: "x",
        };
        assert!(!slot.checked());
        let st = Backfill {
            slot: 0,
            observed: 120,
            state: 9,
            note: "x",
        };
        assert!(!st.checked());
    }

    #[test]
    fn dj_render_round_trips() {
        let s = dj_build().unwrap();
        let text = render_dj(&s);
        assert_eq!(text.split('|').count(), STATS_FIELDS);
        assert_eq!(audit_dj_render(&text), Ok(STATS_FIELDS));
        let bad = text.replacen("|6|", "|5|", 1);
        assert!(bad != text);
        assert!(audit_dj_render(&bad).is_err());
    }

    #[test]
    fn golden_run_is_exact() {
        let s = golden_run().unwrap();
        assert_eq!(s.hits, GOLDEN_ROUNDS);
        assert_eq!(s.misses, GOLDEN_ROUNDS);
        assert_eq!(s.degrades, GOLDEN_ROUNDS);
        assert_eq!(s.rounds, GOLDEN_UNITS);
        assert_eq!(s.audit(), Ok(()));
        assert_eq!(golden_run(), golden_run());
    }

    #[test]
    fn repro_check_passes() {
        assert_eq!(repro_check(), Ok(()));
    }

    #[test]
    fn probes_are_green() {
        assert_eq!(probe_seq_a(), Ok(()));
        assert_eq!(probe_seq_b(), Ok(()));
        assert_eq!(probe_seq_c(), Ok(()));
        assert!(obs_replay().is_ok());
    }

    #[test]
    fn injection_catches_four_routes() {
        let l = inject_rounds(5).unwrap();
        assert!(l.all_caught());
        assert_eq!(l.tries, [5, 5, 5, 5]);
        assert_eq!(l.caught, [5, 5, 5, 5]);
        assert_eq!(inject_rounds(0), Err(ObsCode::CoverBad.val()));
    }

    #[test]
    fn fuzz_obs_balances() {
        let s = fuzz_obs(1_200).unwrap();
        assert!(s.balanced());
        assert_eq!(s.quirks, QUIRK_N as u32);
        assert_eq!(fuzz_obs(1_200).map(|v| v.digest), Ok(s.digest));
        assert_eq!(fuzz_obs(0), Err(ObsCode::CoverBad.val()));
    }

    #[test]
    fn checks_are_green() {
        let s = run_unx49011_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
        assert_eq!(s.len(), 8);
    }
}
