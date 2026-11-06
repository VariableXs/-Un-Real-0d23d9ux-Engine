//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18452 · 世代表版本锚联动（ADR-UNX-008 消费） · 0x7ECC0 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18452-J1 锚基线（波 07 裁决）填充后世代表最高档断言联动，
//!       锚未定时机制空载可运行
//! 本模块结构清单：
//!   1. 冻结常量层：世代数 / 档位闭集 / 波 07 锚基线（世代 4·档 WIN11）
//!      / 判据轮数（推导注释写死）；
//!   2. 诊断码闭集 AncCode（code()=SEG_BASE+n，段内 0x40 窗口）；
//!   3. 数据结构一：锚寄存器 AnchorReg（wave/tier/gen 三元 + 状态机
//!      Unset→Pinned→Cascaded 单向，锚定后不可改写）；
//!   4. 数据结构二：世代表账 LineageLedger（8 世代行账 + 填充位面
//!      u16 位图——四类构件里的「位面」）；
//!   5. 联动规则：锚定后 cascade 对所有 gen >= 锚世代行写最高档
//!      =锚档位，填充位面逐位落账；最高档单调非降（回退档位注入拒）；
//!   6. 空载语义：锚未定时 cascade 幂等空转（Ok(0)，零填充零报错），
//!      机制不因缺锚而 panic 或误写——J1 后半句；
//!   7. 注入核 4 类×10 轮：锚后改锚 / 档位回退 / 世代越界 /
//!      二次级联换档，逐类断言被显式拒绝；
//!   8. 黄金全链 golden_run：空载探针→定锚→级联→联动断言→注入
//!      全抓→幂等复跑→双跑一致；
//!   9. run_unx18452_checks：15 条判据（断言性质）。
//! 为什么本卡先做空载：锚是波 07 裁决的迟到产物——机制必须能在
//! 锚缺席的桩期安全空转（不填、不崩、不误写），锚到位后一次级联
//! 把后世代表最高档全部拉齐到锚档位，联动断言可逐行复算。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7ECC0，AI-VX11-1105-G1·A06 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7ECC0;

// ── 冻结常量（推导注记） ─────────────────────────────────────────
/// 世代表世代数（冻结 8：Vista→Win11 按微软口径分 8 个声明世代）。
pub const GEN_MAX: usize = 8;
/// 档位闭集大小（冻结 9：0=NONE 哨兵 + 8 个命名档）。
pub const TIER_CLOSEN: u32 = 9;
/// 档位名闭集（与 F18442 GUID 世代表口径对齐）。
pub const TIER_NAMES: [&str; TIER_CLOSEN as usize] = [
    "NONE", "VISTA", "WIN7", "WIN8", "WIN81", "WIN10_1507", "WIN10_1809", "WIN10_21H2", "WIN11",
];
/// 波 07 裁决锚世代（冻结 4：波次账定锚在世代 4 之后的全部世代）。
pub const ANCHOR_GEN_BASELINE: usize = 4;
/// 波 07 裁决锚档位（冻结 8：最高档 = WIN11，取档位闭集顶）。
pub const ANCHOR_TIER_BASELINE: u32 = 8;
/// 波 07 裁决波号（冻结 7）。
pub const WAVE_BASELINE: u32 = 7;
/// 注入判定轮数（冻结：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AncCode {
    /// 锚档位越界（闭集 0..9 外）
    AnchorTierIllegal,
    /// 锚世代越界（0..8 外）
    AnchorGenIllegal,
    /// 波号非法（须为波 07 裁决口径）
    WaveIllegal,
    /// 锚已级联后改锚（冻结拒）
    AnchorFrozen,
    /// 档位回退（最高档单调非降被破坏）
    TierRegress,
    /// 联动断言失败（后世代表最高档 ≠ 锚档位）
    LinkageBroken,
    /// 级联幂等破（同锚二次级联结果不同）
    CascadeIdempotencyBroken,
    /// 世代行键重复
    GenDup,
    /// 填充位面与行账不一致
    PlaneRowMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl AncCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 诊断码别名：code() = SEG_BASE + n（与 val() 同值，契约骨架口径）。
    pub fn code(self) -> u32 {
        self.val()
    }
    pub fn detail(self) -> &'static str {
        match self {
            AncCode::AnchorTierIllegal => "锚档位越界：闭集 0..9 外",
            AncCode::AnchorGenIllegal => "锚世代越界：0..8 外",
            AncCode::WaveIllegal => "波号非法：只认波 07 裁决锚",
            AncCode::AnchorFrozen => "锚已级联：改锚拒，走会签流程",
            AncCode::TierRegress => "档位回退：最高档单调非降红线",
            AncCode::LinkageBroken => "联动断言失败：后世代表最高档未拉齐锚档位",
            AncCode::CascadeIdempotencyBroken => "级联幂等破：同锚二次级联结果漂移",
            AncCode::GenDup => "世代行键重复",
            AncCode::PlaneRowMismatch => "填充位面与行账不一致",
            AncCode::NotReproducible => "双跑不一致：确定性红线",
        }
    }
    pub const ALL: [AncCode; 10] = [
        AncCode::AnchorTierIllegal,
        AncCode::AnchorGenIllegal,
        AncCode::WaveIllegal,
        AncCode::AnchorFrozen,
        AncCode::TierRegress,
        AncCode::LinkageBroken,
        AncCode::CascadeIdempotencyBroken,
        AncCode::GenDup,
        AncCode::PlaneRowMismatch,
        AncCode::NotReproducible,
    ];
}

/// 按数值取中文文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < AncCode::ALL.len() {
        if AncCode::ALL[i].val() == code {
            return AncCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7ECC0 段闭集内"
}

// ── 锚状态机 ────────────────────────────────────────────────────
/// 锚三态：Unset→Pinned→Cascaded 单向；级联后锚冻结。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnchorState {
    /// 未定锚（桩期常态）
    Unset,
    /// 已定锚未级联
    Pinned,
    /// 已级联（锚冻结）
    Cascaded,
}

/// 锚寄存器：波号 + 档位 + 锚世代 + 状态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnchorReg {
    pub wave: u32,
    pub tier: u32,
    pub gen: usize,
    pub state: AnchorState,
}

impl AnchorReg {
    pub fn unset() -> AnchorReg {
        AnchorReg { wave: 0, tier: 0, gen: 0, state: AnchorState::Unset }
    }
}

// ── 世代行 + 位面 ───────────────────────────────────────────────
/// 世代表行：世代号 + 最高档 + 级联来源波号（0=本代基线非锚填）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GenRow {
    pub gen: usize,
    pub highest_tier: u32,
    /// 级联来源波号；0 表示该行是世代基线值、非锚填充。
    pub filled_wave: u32,
}

impl GenRow {
    /// 世代基线行：初始最高档 = min(gen+1, 8)——世代越靠后基线档越新，
    /// 世代 7 基线即 WIN11 前（推导：gen i 基线对应第 i+1 个命名档）。
    pub fn baseline(gen: usize) -> GenRow {
        let t = (gen + 1) as u32;
        GenRow {
            gen,
            highest_tier: if t >= TIER_CLOSEN { TIER_CLOSEN - 1 } else { t },
            filled_wave: 0,
        }
    }
}

// ── 数据结构二：世代表账 ────────────────────────────────────────
/// 世代表账：锚寄存器 + 世代行账 + 填充位面 + 级联计数。
pub struct LineageLedger {
    pub anchor: AnchorReg,
    pub rows: Vec<GenRow>,
    /// 填充位面：bit i = 世代 i 已被锚级联填充。
    pub filled_plane: u16,
    pub cascade_runs: u32,
    pub idle_runs: u32,
}

impl LineageLedger {
    /// 新账：8 世代基线行 + 锚未定。
    pub fn new() -> LineageLedger {
        let mut rows = Vec::new();
        let mut i = 0;
        while i < GEN_MAX {
            rows.push(GenRow::baseline(i));
            i += 1;
        }
        LineageLedger {
            anchor: AnchorReg::unset(),
            rows,
            filled_plane: 0,
            cascade_runs: 0,
            idle_runs: 0,
        }
    }

    /// 定锚：波号/档位/世代三重域守卫；Unset→Pinned 单向。
    pub fn pin(&mut self, wave: u32, tier: u32, gen: usize) -> Result<(), u32> {
        if self.anchor.state == AnchorState::Cascaded {
            return Err(AncCode::AnchorFrozen.val());
        }
        if tier == 0 || tier >= TIER_CLOSEN {
            return Err(AncCode::AnchorTierIllegal.val());
        }
        if gen >= GEN_MAX {
            return Err(AncCode::AnchorGenIllegal.val());
        }
        if wave != WAVE_BASELINE {
            return Err(AncCode::WaveIllegal.val());
        }
        self.anchor = AnchorReg { wave, tier, gen, state: AnchorState::Pinned };
        Ok(())
    }

    /// 级联：锚未定时空载（Ok(0) 零填充）；锚定时对 gen >= 锚世代行
    /// 挂波并拉齐最高档 = 锚档位，填充位面逐位落账。
    /// 落位判据 = 挂波号（filled_wave != 锚波 即落位，含本代基线档
    /// 恰等于锚档位的行——否则该行位面/波号缺口会让联动断言红）。
    /// 返回本次实际落位行数（幂等复跑为 0）。
    pub fn cascade(&mut self) -> Result<u32, u32> {
        if self.anchor.state == AnchorState::Unset {
            // 空载可运行：机制空转，零填充零报错。
            self.idle_runs = self.idle_runs.saturating_add(1);
            return Ok(0);
        }
        let atier = self.anchor.tier;
        let agen = self.anchor.gen;
        let awave = self.anchor.wave;
        let mut filled = 0u32;
        let mut plane = self.filled_plane;
        let mut i = agen;
        while i < GEN_MAX {
            let row = match self.rows.get(i) {
                Some(r) => *r,
                None => return Err(AncCode::AnchorGenIllegal.val()),
            };
            // 单调非降：现有最高档高于锚档位视为异常口径（拒）。
            if row.highest_tier > atier {
                return Err(AncCode::TierRegress.val());
            }
            // 挂波落位：凡未挂锚波的行（基线行含值恰等者）一律挂波
            // + 拉齐锚档位；已挂波行跳过（幂等）。
            if row.filled_wave != awave {
                if let Some(r) = self.rows.get_mut(i) {
                    r.highest_tier = atier;
                    r.filled_wave = awave;
                }
                filled = filled.saturating_add(1);
                plane = plane | (1u16 << i);
            }
            i += 1;
        }
        self.filled_plane = plane;
        self.cascade_runs = self.cascade_runs.saturating_add(1);
        self.anchor.state = AnchorState::Cascaded;
        Ok(filled)
    }

    /// 联动断言：锚已级联时，所有 gen >= 锚世代行最高档 == 锚档位
    /// 且位面逐位对齐；锚未定时空载放行（零行核对）。
    pub fn verify_linkage(&self) -> Result<u32, u32> {
        if self.anchor.state != AnchorState::Cascaded {
            if self.filled_plane != 0 {
                return Err(AncCode::PlaneRowMismatch.val());
            }
            return Ok(0);
        }
        let atier = self.anchor.tier;
        let agen = self.anchor.gen;
        let mut linked = 0u32;
        let mut i = agen;
        while i < GEN_MAX {
            let row = match self.rows.get(i) {
                Some(r) => *r,
                None => return Err(AncCode::AnchorGenIllegal.val()),
            };
            let bit_set = self.filled_plane & (1u16 << i) != 0;
            if row.highest_tier != atier {
                return Err(AncCode::LinkageBroken.val());
            }
            if row.filled_wave != WAVE_BASELINE || !bit_set {
                return Err(AncCode::PlaneRowMismatch.val());
            }
            linked = linked.saturating_add(1);
            i += 1;
        }
        // 位面高位（锚世代之前）必须全空。
        let mut j = 0;
        while j < agen {
            if self.filled_plane & (1u16 << j) != 0 {
                return Err(AncCode::PlaneRowMismatch.val());
            }
            j += 1;
        }
        Ok(linked)
    }

    /// 档位外部写入（探测面）：只许单调非降的合法档位。
    pub fn propose_tier(&mut self, gen: usize, tier: u32) -> Result<(), u32> {
        if gen >= GEN_MAX {
            return Err(AncCode::AnchorGenIllegal.val());
        }
        if tier == 0 || tier >= TIER_CLOSEN {
            return Err(AncCode::AnchorTierIllegal.val());
        }
        let cur = match self.rows.get(gen) {
            Some(r) => r.highest_tier,
            None => return Err(AncCode::AnchorGenIllegal.val()),
        };
        if tier < cur {
            return Err(AncCode::TierRegress.val());
        }
        if let Some(r) = self.rows.get_mut(gen) {
            r.highest_tier = tier;
        }
        Ok(())
    }
}

// ── 注入核（4 类 ×10 轮） ───────────────────────────────────────
/// 注入检出账：四槽 tries/caught 成对。
#[derive(Debug)]
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn missed(&self) -> u32 {
        let mut m = 0;
        let mut i = 0;
        while i < 4 {
            m += self.tries[i].saturating_sub(self.caught[i]);
            i += 1;
        }
        m
    }
}

/// 10 轮注入：槽 0 锚后改锚 / 槽 1 档位回退 / 槽 2 世代越界 /
/// 槽 3 二次级联换档（先毁锚再换档定锚的换轨注入）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽 0：锚已级联后再 pin —— 必拒 AnchorFrozen。
        let mut l = LineageLedger::new();
        let p0 = l.pin(WAVE_BASELINE, ANCHOR_TIER_BASELINE, ANCHOR_GEN_BASELINE);
        let c0 = if p0.is_ok() { l.cascade() } else { Err(AncCode::AnchorTierIllegal.val()) };
        let hit0 = p0.is_ok()
            && c0.is_ok()
            && l.pin(WAVE_BASELINE, ANCHOR_TIER_BASELINE, ANCHOR_GEN_BASELINE).err()
                == Some(AncCode::AnchorFrozen.val());
        led.record(0, hit0);
        // 槽 1：档位回退——锚级联拉到档 8 后再提档 6 必拒。
        let mut l1 = LineageLedger::new();
        l1.pin(WAVE_BASELINE, ANCHOR_TIER_BASELINE, ANCHOR_GEN_BASELINE).ok();
        l1.cascade().ok();
        let hit1 = l1.propose_tier(ANCHOR_GEN_BASELINE + 1, 6).err()
            == Some(AncCode::TierRegress.val());
        led.record(1, hit1);
        // 槽 2：世代越界——pin(gen=GEN_MAX) 必拒。
        let mut l2 = LineageLedger::new();
        let hit2 = l2.pin(WAVE_BASELINE, ANCHOR_TIER_BASELINE, GEN_MAX).err()
            == Some(AncCode::AnchorGenIllegal.val());
        led.record(2, hit2);
        // 槽 3：二次级联换档——级联后解冻重 pin 低档， propose 越过
        // 锚档位再 verify 联动必断（换档注入被联动断言抓住）。
        let mut l3 = LineageLedger::new();
        l3.pin(WAVE_BASELINE, ANCHOR_TIER_BASELINE, ANCHOR_GEN_BASELINE).ok();
        l3.cascade().ok();
        // 直接篡改一行最高档（模拟换档写穿），联动断言必红。
        let drift_slot = ANCHOR_GEN_BASELINE + 2;
        let hit3 = if let Some(row) = l3.rows.get_mut(drift_slot) {
            row.highest_tier = ANCHOR_TIER_BASELINE - 1;
            true
        } else {
            false
        };
        let verified = l3.verify_linkage();
        led.record(3, hit3 && verified.err() == Some(AncCode::LinkageBroken.val()));
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：空载探针→定锚→级联→联动断言→幂等→注入全抓→双跑一致。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    // 空载：锚未定级联 Ok(0) 且零填充（J1 后半句）。
    let mut l = LineageLedger::new();
    let idle = l.cascade()?;
    if idle != 0 || l.filled_plane != 0 || l.verify_linkage()? != 0 {
        return Err(AncCode::CascadeIdempotencyBroken.val());
    }
    // 定锚（波 07 裁决基线）+ 级联。
    l.pin(WAVE_BASELINE, ANCHOR_TIER_BASELINE, ANCHOR_GEN_BASELINE)?;
    let filled = l.cascade()?;
    // 世代 4..8 共 4 行被拉齐（推导：GEN_MAX(8) - 锚世代(4) = 4）。
    if filled != (GEN_MAX - ANCHOR_GEN_BASELINE) as u32 {
        return Err(AncCode::LinkageBroken.val());
    }
    let linked = l.verify_linkage()?;
    if linked != (GEN_MAX - ANCHOR_GEN_BASELINE) as u32 {
        return Err(AncCode::LinkageBroken.val());
    }
    // 幂等：二次级联零填充，账不变。
    let plane_before = l.filled_plane;
    let again = l.cascade()?;
    if again != 0 || l.filled_plane != plane_before {
        return Err(AncCode::CascadeIdempotencyBroken.val());
    }
    // 注入 10 轮全抓。
    let inj = inject_rounds(JUDGE_ROUNDS)?;
    if !inj.all_caught() || inj.missed() != 0 {
        return Err(AncCode::NotReproducible.val());
    }
    // 返回（空载轮数, 首次级联落位数, 联动核对数）——落位数取首次
    // 级联的 filled（=4），不是 cascade_runs 计数。
    Ok((l.idle_runs, filled, linked))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(AncCode::NotReproducible.val());
    }
    Ok(())
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 正探针：波 07 基线定锚级联，联动断言过。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut l = LineageLedger::new();
    l.pin(WAVE_BASELINE, ANCHOR_TIER_BASELINE, ANCHOR_GEN_BASELINE)?;
    l.cascade()?;
    l.verify_linkage()?;
    Ok(())
}

/// 负探针 1：波号非法（非 07 波锚拒）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut l = LineageLedger::new();
    match l.pin(WAVE_BASELINE + 1, ANCHOR_TIER_BASELINE, ANCHOR_GEN_BASELINE) {
        Err(c) if c == AncCode::WaveIllegal.val() => Ok(()),
        _ => Err(AncCode::WaveIllegal.val()),
    }
}

/// 负探针 2：锚档位越界（tier=0 哨兵与 tier=9 闭集外都拒）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut l = LineageLedger::new();
    let a = l.pin(WAVE_BASELINE, 0, ANCHOR_GEN_BASELINE);
    let b = l.pin(WAVE_BASELINE, TIER_CLOSEN, ANCHOR_GEN_BASELINE);
    if a.err() == Some(AncCode::AnchorTierIllegal.val())
        && b.err() == Some(AncCode::AnchorTierIllegal.val())
    {
        Ok(())
    } else {
        Err(AncCode::AnchorTierIllegal.val())
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < AncCode::ALL.len() {
        let v = AncCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

// ── 判据收口 ────────────────────────────────────────────────────
pub fn run_unx18452_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18452 世代表版本锚联动");
    // ① 黄金全链。
    let g = golden_run();
    s.add_dyn(
        "J1-黄金全链",
        g.is_ok(),
        &format!(
            "空载{:?} 级联{:?} 联动{:?}",
            g.as_ref().map(|r| r.0).unwrap_or(0),
            g.as_ref().map(|r| r.1).unwrap_or(0),
            g.as_ref().map(|r| r.2).unwrap_or(0)
        ),
    );
    // ② 空载可运行：锚未定级联 Ok(0)、零填充、零报错。
    let mut lidle = LineageLedger::new();
    let idle = lidle.cascade();
    s.add(
        "锚未定空载可运行",
        idle == Ok(0) && lidle.filled_plane == 0 && lidle.verify_linkage() == Ok(0),
        "空载级联零填充零报错，机制不崩",
    );
    // ③ 波 07 锚基线填充（J1 前半句）：级联填充数 = 4。
    s.add(
        "波07锚填充4行",
        matches!(g, Ok((_, c, _)) if c == (GEN_MAX - ANCHOR_GEN_BASELINE) as u32),
        "世代4..8 全部被锚级联填充",
    );
    // ④ 联动断言：后世代表最高档 == 锚档位（逐行复算）。
    let mut lchk = LineageLedger::new();
    lchk.pin(WAVE_BASELINE, ANCHOR_TIER_BASELINE, ANCHOR_GEN_BASELINE).ok();
    lchk.cascade().ok();
    let linked = lchk.verify_linkage();
    s.add_dyn(
        "最高档联动断言",
        linked == Ok((GEN_MAX - ANCHOR_GEN_BASELINE) as u32),
        &format!("联动{:?}", linked),
    );
    // ⑤ 填充位面与行账对齐（位面四类构件账）。
    let plane_ok = lchk.filled_plane == (0x00F0u16) && lchk.verify_linkage().is_ok();
    // 推导：gen4..7 共 4 位 = 位 4|5|6|7 = 0b1111_0000 = 0x00F0。
    s.add("填充位面0x00F0", plane_ok, "位 4..7 置位，与行账逐位对齐");
    // ⑥ 幂等：二次级联零填充。
    let before = lchk.filled_plane;
    let second = lchk.cascade();
    s.add("级联幂等", second == Ok(0) && lchk.filled_plane == before, "同锚复跑零漂移");
    // ⑦ 单调非降：锚档位 8 之下不许回退。
    let mut lmono = LineageLedger::new();
    lmono.pin(WAVE_BASELINE, ANCHOR_TIER_BASELINE, ANCHOR_GEN_BASELINE).ok();
    lmono.cascade().ok();
    s.add(
        "最高档单调非降",
        lmono.propose_tier(ANCHOR_GEN_BASELINE, 6).err() == Some(AncCode::TierRegress.val()),
        "档位回退注入必拒",
    );
    // ⑧ 注入四类 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn(
        "注入四类10/10",
        inj_ok,
        &format!(
            "{}/{}/{}/{}",
            inj.as_ref().map(|l| l.caught[0]).unwrap_or(0),
            inj.as_ref().map(|l| l.caught[1]).unwrap_or(0),
            inj.as_ref().map(|l| l.caught[2]).unwrap_or(0),
            inj.as_ref().map(|l| l.caught[3]).unwrap_or(0)
        ),
    );
    // ⑨ 漏检为零。
    s.add("注入漏检为零", match &inj {
        Ok(l) => l.missed() == 0,
        Err(_) => false,
    }, "注入=检出+漏检，漏检=0");
    // ⑩ 探针三序。
    s.add("探针三序",
        probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok(),
        "正锚/波号拒/档位越界拒");
    // ⑪ 锚冻结：级联后改锚拒。
    let mut lfz = LineageLedger::new();
    lfz.pin(WAVE_BASELINE, ANCHOR_TIER_BASELINE, ANCHOR_GEN_BASELINE).ok();
    lfz.cascade().ok();
    s.add(
        "锚级联后冻结",
        lfz.pin(WAVE_BASELINE, ANCHOR_TIER_BASELINE, ANCHOR_GEN_BASELINE).err()
            == Some(AncCode::AnchorFrozen.val()),
        "改锚走会签，直接改写拒",
    );
    // ⑫ 档位名闭集非空（联动断言的可读面）。
    let mut names_ok = true;
    let mut ni = 0;
    while (ni as u32) < TIER_CLOSEN {
        if TIER_NAMES[ni].is_empty() {
            names_ok = false;
        }
        ni += 1;
    }
    s.add("档位名闭集非空", names_ok, "9 档全有命名，与 F18442 世代口径对齐");
    // ⑬ 双跑一致。
    s.add("双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // ⑭ 基线行推导复算：世代基线 = min(gen+1, 8)。
    let mut baseline_ok = true;
    let mut bi = 0;
    while bi < GEN_MAX {
        let r = GenRow::baseline(bi);
        let expect = if bi + 1 >= TIER_CLOSEN as usize {
            TIER_CLOSEN - 1
        } else {
            (bi + 1) as u32
        };
        if r.highest_tier != expect || r.filled_wave != 0 {
            baseline_ok = false;
        }
        bi += 1;
    }
    s.add("世代基线推导复算", baseline_ok, "gen i 基线档 = min(i+1, 8)");
    // ⑮ 文案闭集 + 段位守卫。
    let mut closed = true;
    let mut ci = 0;
    while ci < AncCode::ALL.len() {
        if AncCode::ALL[ci].detail().is_empty() {
            closed = false;
        }
        ci += 1;
    }
    s.add("诊断文案闭集非空", closed, "10 码全有中文建议");
    s.add("段位守卫 0x7ECC0", segment_guard(), "SEG_BASE 0x40 对齐窗口");
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_when_anchor_unset() {
        let mut l = LineageLedger::new();
        assert_eq!(l.cascade().unwrap_or(1), 0, "锚未定级联零填充");
        assert_eq!(l.filled_plane, 0);
        assert_eq!(l.verify_linkage().unwrap_or(99), 0);
        assert_eq!(l.idle_runs, 1, "空载计数入账");
    }

    #[test]
    fn wave07_anchor_fills_four_rows() {
        let mut l = LineageLedger::new();
        assert!(l.pin(WAVE_BASELINE, ANCHOR_TIER_BASELINE, ANCHOR_GEN_BASELINE).is_ok());
        let filled = l.cascade().unwrap_or(0);
        assert_eq!(filled, 4, "世代 4..8 共 4 行被填充");
        assert_eq!(l.verify_linkage().unwrap_or(0), 4);
        for i in ANCHOR_GEN_BASELINE..GEN_MAX {
            // 先绑定兜底行再借用，避免 unwrap_or(&临时值) 的 E0716。
            let fb = GenRow::baseline(0);
            let r = l.rows.get(i).copied().unwrap_or(fb);
            assert_eq!(r.highest_tier, ANCHOR_TIER_BASELINE, "世代 {} 最高档拉齐", i);
            assert_eq!(r.filled_wave, WAVE_BASELINE);
        }
    }

    #[test]
    fn lower_generations_untouched() {
        let mut l = LineageLedger::new();
        l.pin(WAVE_BASELINE, ANCHOR_TIER_BASELINE, ANCHOR_GEN_BASELINE).ok();
        l.cascade().ok();
        let mut i = 0;
        while i < ANCHOR_GEN_BASELINE {
            // 先绑定兜底行再取值，避免临时借用 E0716。
            let fb = GenRow::baseline(i);
            let r = l.rows.get(i).copied().unwrap_or(fb);
            assert_eq!(r.highest_tier, GenRow::baseline(i).highest_tier, "锚前世代基线不动");
            assert_eq!(r.filled_wave, 0);
            i += 1;
        }
    }

    #[test]
    fn cascade_idempotent() {
        let mut l = LineageLedger::new();
        l.pin(WAVE_BASELINE, ANCHOR_TIER_BASELINE, ANCHOR_GEN_BASELINE).ok();
        l.cascade().ok();
        let plane = l.filled_plane;
        let runs = l.cascade_runs;
        assert_eq!(l.cascade().unwrap_or(1), 0, "二次级联零填充");
        assert_eq!(l.filled_plane, plane);
        assert_eq!(l.cascade_runs, runs + 1, "级联次数照记");
    }

    #[test]
    fn anchor_frozen_after_cascade() {
        let mut l = LineageLedger::new();
        l.pin(WAVE_BASELINE, ANCHOR_TIER_BASELINE, ANCHOR_GEN_BASELINE).ok();
        l.cascade().ok();
        assert_eq!(
            l.pin(WAVE_BASELINE, ANCHOR_TIER_BASELINE, ANCHOR_GEN_BASELINE).err(),
            Some(AncCode::AnchorFrozen.val())
        );
    }

    #[test]
    fn tier_regress_rejected_ten_rounds() {
        let mut r = 0;
        while r < JUDGE_ROUNDS {
            let mut l = LineageLedger::new();
            l.pin(WAVE_BASELINE, ANCHOR_TIER_BASELINE, ANCHOR_GEN_BASELINE).ok();
            l.cascade().ok();
            assert_eq!(
                l.propose_tier(ANCHOR_GEN_BASELINE + 1, 6).err(),
                Some(AncCode::TierRegress.val()),
                "第 {} 轮档位回退必拒",
                r
            );
            r += 1;
        }
    }

    #[test]
    fn gen_out_of_range_rejected_ten_rounds() {
        let mut r = 0;
        while r < JUDGE_ROUNDS {
            let mut l = LineageLedger::new();
            assert_eq!(
                l.pin(WAVE_BASELINE, ANCHOR_TIER_BASELINE, GEN_MAX).err(),
                Some(AncCode::AnchorGenIllegal.val()),
                "第 {} 轮世代越界必拒",
                r
            );
            r += 1;
        }
    }

    #[test]
    fn drift_injection_caught_by_linkage() {
        let mut l = LineageLedger::new();
        l.pin(WAVE_BASELINE, ANCHOR_TIER_BASELINE, ANCHOR_GEN_BASELINE).ok();
        l.cascade().ok();
        let slot = ANCHOR_GEN_BASELINE + 2;
        if let Some(row) = l.rows.get_mut(slot) {
            row.highest_tier = ANCHOR_TIER_BASELINE - 1;
        }
        assert_eq!(
            l.verify_linkage().err(),
            Some(AncCode::LinkageBroken.val()),
            "换档写穿必被联动断言抓"
        );
    }

    #[test]
    fn injection_ledger_all_caught_no_miss() {
        let inj = inject_rounds(10).unwrap_or_else(|_| InjectLedger::new());
        assert!(inj.all_caught(), "四类注入 10/10 全抓");
        assert_eq!(inj.missed(), 0);
    }

    #[test]
    fn baseline_derivation_recap() {
        let mut i = 0;
        while i < GEN_MAX {
            let r = GenRow::baseline(i);
            assert!(r.highest_tier >= 1 && r.highest_tier < TIER_CLOSEN, "档位在闭集内");
            i += 1;
        }
        assert_eq!(GenRow::baseline(0).highest_tier, 1, "世代 0 基线 = VISTA 档");
        assert_eq!(GenRow::baseline(GEN_MAX - 1).highest_tier, TIER_CLOSEN - 1);
    }

    #[test]
    fn probes_and_double_run() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert_eq!(golden_run(), golden_run(), "双跑一致");
    }

    #[test]
    fn checks_set_all_green() {
        let s = run_unx18452_checks();
        assert!(s.all_passed(), "本卡判据必须全绿: {}", s.red_items_text());
        assert!(s.len() >= 8 && s.len() <= 20, "判据条数 {} 在窗内", s.len());
    }
}
