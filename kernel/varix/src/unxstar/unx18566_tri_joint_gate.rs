//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18566 · 三段联合通判据（锚 2 前半集成跑） · 0x80940段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18566-J1 三段同开场景 20 例集成判定断言全过，单段关
//!       断矩阵 4 态覆盖
//! 设计要点：
//!   - 三段小型检查器闭集：声明段 / 兼容层段 / 档案段，各段按场景行
//!     的违约旗（0=正常 1=违约）给出过/不过，段判码互异（Seg0/1/2Fail）；
//!   - 场景裁决表 20 行冻结：14 行三段全开好例 + 6 行违约例（含关断
//!     位下的违约行），逐行期望判码先行写死，跑后对拍——期望与实判
//!     不等即 CaseVerdictMismatch，20/20 全对才算过；
//!   - 单段关断矩阵 4 态：全开(111)/关声明(011)/关兼容层(101)/
//!     关档案(110)，每态用全好例跑一次，覆盖旗入账；矩阵四态缺一
//!     即 MatrixStateMissing；
//!   - 贡献账守恒：每次 run 只有 mask 放行的段可入贡献账，关断位下
//!     的违约段仍想贡献 → SegDisabledContrib 必抓；mask=0 或 >7 →
//!     MaskIllegal；
//!   - 注入核三类：关断位贡献 / 非法掩码 / 场景判码漂移，各 10 轮
//!     全抓，漏检为零；
//!   - 边界：场景号越界/重复场景/矩阵态缺；
//!   - LCG 种子写死 = SEG_BASE（0x80940）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! 结构清单：
//!   数据结构：ScenarioRow（场景行）/ JointGate（联合闸+贡献账+矩阵账）
//!   状态机  ：GatePhase（冷→跑→收 单调三态）
//!   账本    ：贡献账（3 槽）+ 矩阵覆盖旗（4 态）+ 判定对拍账 + 注入检出账
//!   错误路径：TriCode 12 码闭集，code() = SEG_BASE + n（0x80940 窗）
//!   注入核  ：关断贡献/非法掩码/判码漂移三类逐类断言被拒（10/10）
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80940，AI-VX11-1105 批09 第 6 槽，0x40 对齐独占窗）。
pub const SEG_BASE: u32 = 0x80940;

// ── 冻结常量（写死并注明推导） ──────────────────────────────────
/// 集成场景数（判据原文钉死：20 例）。
pub const SCEN_N: usize = 20;
/// 三段闭集（判据原文：三段同开）。
pub const SEG_N: usize = 3;
/// 关断矩阵态数（判据原文：4 态 = 全开 + 三种单段关断）。
pub const MATRIX_N: usize = 4;
/// 关断矩阵掩码表（推导：全开 7；关声明=7&~1=6；关兼容层=7&~2=5；
/// 关档案=7&~4=3——位 k 对应段 k）。
pub const MATRIX_MASKS: [u32; MATRIX_N] = [7, 6, 5, 3];
/// 判据迭代轮数（写死：注入 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 判定种子（√2 前 64 位，冻结推导）。
pub const FP_SEED: u64 = 0x6A09_E667_F3BC_C908 ^ 0x0450;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TriCode {
    /// 非法掩码：mask=0 或 >7
    MaskIllegal,
    /// 场景号越界：0..20 之外
    CaseOutOfRange,
    /// 重复场景：同号二跑
    DupCase,
    /// 关断位贡献：关断段仍想入贡献账
    SegDisabledContrib,
    /// 场景判码漂移：期望判码与实判不符
    CaseVerdictMismatch,
    /// 矩阵态缺：4 态未全覆盖
    MatrixStateMissing,
    /// 声明段违约
    Seg0Fail,
    /// 兼容层段违约
    Seg1Fail,
    /// 档案段违约
    Seg2Fail,
    /// 账目守恒破
    ConservationBreak,
    /// 双跑漂移
    NotReproducible,
    /// 未知诊断码兜底
    Unknown,
}

impl TriCode {
    /// 诊断码 = SEG_BASE + 序号（段内闭集，const 供静态裁决表）。
    pub const fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文建议（文案闭集，每码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            TriCode::MaskIllegal => "非法掩码：mask 只在 1..=7 内取",
            TriCode::CaseOutOfRange => "场景号越界：场景只在 0..20 内取",
            TriCode::DupCase => "重复场景：一场景一跑，重跑先清账",
            TriCode::SegDisabledContrib => "关断位贡献：关断段不许入贡献账",
            TriCode::CaseVerdictMismatch => "场景判码漂移：期望与实判对拍重跑",
            TriCode::MatrixStateMissing => "矩阵态缺：4 态全覆盖才收口",
            TriCode::Seg0Fail => "声明段违约：按场景行修违约旗",
            TriCode::Seg1Fail => "兼容层段违约：按场景行修违约旗",
            TriCode::Seg2Fail => "档案段违约：按场景行修违约旗",
            TriCode::ConservationBreak => "账目不守恒：贡献账对不上掩码放行数",
            TriCode::NotReproducible => "双跑漂移：冻结种子重跑对拍",
            TriCode::Unknown => "未知诊断码：不在 0x80940 段闭集内",
        }
    }
    pub const ALL: [TriCode; 12] = [
        TriCode::MaskIllegal,
        TriCode::CaseOutOfRange,
        TriCode::DupCase,
        TriCode::SegDisabledContrib,
        TriCode::CaseVerdictMismatch,
        TriCode::MatrixStateMissing,
        TriCode::Seg0Fail,
        TriCode::Seg1Fail,
        TriCode::Seg2Fail,
        TriCode::ConservationBreak,
        TriCode::NotReproducible,
        TriCode::Unknown,
    ];
}

/// 码值反查中文建议（段外码落 Unknown 兜底）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < TriCode::ALL.len() {
        if TriCode::ALL[i].code() == code {
            return TriCode::ALL[i].detail();
        }
        i += 1;
    }
    TriCode::Unknown.detail()
}

// ── 指纹原语 ────────────────────────────────────────────────────
/// 64 位雪崩混合（写死常量，双跑必同）。
pub fn mix64(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^= x >> 33;
    x
}

/// 场景判定指纹：场景号、掩码、三段违约旗四元混合。
pub fn case_fp(cid: u32, mask: u32, m0: u32, m1: u32, m2: u32) -> u64 {
    let a = ((cid as u64) << 40) ^ ((mask as u64) << 32) ^ ((m0 as u64) << 16)
        ^ ((m1 as u64) << 8) ^ (m2 as u64);
    mix64(a ^ FP_SEED)
}

// ── 闸相位状态机（单调三态） ────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GatePhase {
    Cold = 0,
    Running = 1,
    Sealed = 2,
}

impl GatePhase {
    pub fn rank(self) -> u32 {
        self as u32
    }
    /// 单调 +1 放行；倒退/越级拒 ConservationBreak（相位即账面）。
    pub fn advance(self, to: GatePhase) -> Result<GatePhase, u32> {
        let from = self.rank();
        let dst = to.rank();
        if dst == from + 1 {
            Ok(to)
        } else {
            Err(TriCode::ConservationBreak.code())
        }
    }
}

// ── 数据结构：场景行与联合闸 ────────────────────────────────────
/// 场景行：(场景号, 掩码, 三段违约旗, 期望判码 0=过)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScenarioRow {
    pub cid: u32,
    pub mask: u32,
    pub modes: [u32; SEG_N],
    pub expect: u32,
}

impl ScenarioRow {
    pub const fn new(cid: u32, mask: u32, m0: u32, m1: u32, m2: u32, expect: u32) -> ScenarioRow {
        ScenarioRow { cid, mask, modes: [m0, m1, m2], expect }
    }
}

/// 场景裁决表 20 行（冻结）：
///   0..14  三段全开好例（mask=7 全 0 旗，期望过）；
///   15..17 三段全开违约例（逐段各一违约，期望 SegNFail）；
///   18..19 关断位好例（关一跑二，期望过）。
/// 期望判码推导：违约段 k 的段判码 Seg{k}Fail；全好例 0。
pub const SCEN_TABLE: [ScenarioRow; SCEN_N] = [
    ScenarioRow::new(0, 7, 0, 0, 0, 0),
    ScenarioRow::new(1, 7, 0, 0, 0, 0),
    ScenarioRow::new(2, 7, 0, 0, 0, 0),
    ScenarioRow::new(3, 7, 0, 0, 0, 0),
    ScenarioRow::new(4, 7, 0, 0, 0, 0),
    ScenarioRow::new(5, 7, 0, 0, 0, 0),
    ScenarioRow::new(6, 7, 0, 0, 0, 0),
    ScenarioRow::new(7, 7, 0, 0, 0, 0),
    ScenarioRow::new(8, 7, 0, 0, 0, 0),
    ScenarioRow::new(9, 7, 0, 0, 0, 0),
    ScenarioRow::new(10, 7, 0, 0, 0, 0),
    ScenarioRow::new(11, 7, 0, 0, 0, 0),
    ScenarioRow::new(12, 7, 0, 0, 0, 0),
    ScenarioRow::new(13, 7, 0, 0, 0, 0),
    ScenarioRow::new(14, 7, 1, 0, 0, TriCode::Seg0Fail.code()),
    ScenarioRow::new(15, 7, 0, 1, 0, TriCode::Seg1Fail.code()),
    ScenarioRow::new(16, 7, 0, 0, 1, TriCode::Seg2Fail.code()),
    ScenarioRow::new(17, 6, 1, 0, 0, TriCode::SegDisabledContrib.code()),
    ScenarioRow::new(18, 5, 0, 0, 1, TriCode::SegDisabledContrib.code()),
    ScenarioRow::new(19, 3, 1, 0, 0, TriCode::SegDisabledContrib.code()),
];

/// 联合闸：贡献账 + 矩阵覆盖旗 + 对拍账。
pub struct JointGate {
    /// 闸相位状态机。
    pub phase: GatePhase,
    /// 贡献账：每段被放行运行的次数（对守恒轴）。
    pub contrib: [u32; SEG_N],
    /// 矩阵覆盖旗：4 态各跑过一次全好例。
    pub matrix_cover: [bool; MATRIX_N],
    /// 对拍账：(场景号, 实判码)。
    pub verdicts: Vec<(u32, u32)>,
    /// 对拍不平计数（收口必须为 0）。
    pub mismatch_n: u32,
}

impl JointGate {
    pub fn new() -> JointGate {
        JointGate {
            phase: GatePhase::Cold,
            contrib: [0; SEG_N],
            matrix_cover: [false; MATRIX_N],
            verdicts: Vec::new(),
            mismatch_n: 0,
        }
    }

    /// 单次联合判定：掩码放行段逐段跑；违约段给段判码；关断位违约
    /// 段拒 SegDisabledContrib；掩码非法拒 MaskIllegal。
    pub fn run(&mut self, mask: u32, modes: [u32; SEG_N]) -> Result<(), u32> {
        if mask == 0 || mask > 7 {
            return Err(TriCode::MaskIllegal.code());
        }
        // 相位推进（冷→跑）。
        if self.phase == GatePhase::Cold {
            self.phase = self.phase.advance(GatePhase::Running)?;
        }
        let mut k = 0;
        while k < SEG_N {
            let bit = (mask >> k) & 1;
            let mode = modes[k];
            if bit == 1 {
                // 放行段：违约即段判码，正常入贡献账。
                if mode == 1 {
                    return Err(seg_fail_code(k));
                }
                self.contrib[k] = self.contrib[k].saturating_add(1);
            } else if mode == 1 {
                // 关断位下仍想贡献：必抓。
                return Err(TriCode::SegDisabledContrib.code());
            }
            k += 1;
        }
        Ok(())
    }

    /// 矩阵态跑：全好例过一次，覆盖旗置位（态号域守卫）。
    pub fn run_matrix_state(&mut self, st: usize) -> Result<(), u32> {
        if st >= MATRIX_N {
            return Err(TriCode::CaseOutOfRange.code());
        }
        let mask = MATRIX_MASKS[st];
        self.run(mask, [0; SEG_N])?;
        self.matrix_cover[st] = true;
        Ok(())
    }

    /// 场景表逐例集成跑：实判与期望逐行对拍，不平记账不中断。
    pub fn run_scenarios(&mut self) -> Result<u32, u32> {
        let mut ok_n = 0u32;
        let mut i = 0;
        while i < SCEN_N {
            let row = SCEN_TABLE[i];
            // 重复场景守卫。
            let mut dup = false;
            let mut j = 0;
            while j < self.verdicts.len() {
                match self.verdicts.get(j) {
                    Some((c, _)) => {
                        if *c == row.cid {
                            dup = true;
                        }
                    }
                    None => return Err(TriCode::ConservationBreak.code()),
                }
                j += 1;
            }
            if dup {
                return Err(TriCode::DupCase.code());
            }
            let got = match self.run(row.mask, row.modes) {
                Ok(()) => 0,
                Err(c) => c,
            };
            if got != row.expect {
                self.mismatch_n = self.mismatch_n.saturating_add(1);
            } else {
                ok_n = ok_n.saturating_add(1);
            }
            self.verdicts.push((row.cid, got));
            i += 1;
        }
        Ok(ok_n)
    }

    /// 贡献守恒：跑场景后贡献账逐段 == 全开好例数 + 矩阵态放行数。
    /// 推导：场景表 0..14 全开好例贡献 14/段；17/18/19 行有掩码放行
    /// 段旗全 0（关断位旗=1 走 SegDisabledContrib 不入账），18 行掩码
    /// 5=101 放行段 0/2，19 行掩码 3=011 放行段 0/1；矩阵 4 态全好例
    /// 放行 7+6+5+3=21 次（段 0 三态、段 1 三态、段 2 四态——按位拆）。
    pub fn conserve(&self) -> Result<(), u32> {
        // 场景表逐段放行计数（静态复算）。
        let mut want = [0u32; SEG_N];
        let mut i = 0;
        while i < SCEN_N {
            let row = SCEN_TABLE[i];
            let mut k = 0;
            while k < SEG_N {
                if (row.mask >> k) & 1 == 1 && row.modes[k] == 0 {
                    want[k] = want[k].saturating_add(1);
                }
                k += 1;
            }
            i += 1;
        }
        // 矩阵态逐段放行计数。
        let mut st = 0;
        while st < MATRIX_N {
            let mask = MATRIX_MASKS[st];
            let mut k = 0;
            while k < SEG_N {
                if (mask >> k) & 1 == 1 {
                    want[k] = want[k].saturating_add(1);
                }
                k += 1;
            }
            st += 1;
        }
        let mut k = 0;
        while k < SEG_N {
            if self.contrib[k] != want[k] {
                return Err(TriCode::ConservationBreak.code());
            }
            k += 1;
        }
        Ok(())
    }

    /// 收口闸（跑→收）：矩阵 4 态全覆盖 + 对拍不平为零。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase != GatePhase::Running {
            return Err(TriCode::ConservationBreak.code());
        }
        let mut st = 0;
        while st < MATRIX_N {
            if !self.matrix_cover[st] {
                return Err(TriCode::MatrixStateMissing.code());
            }
            st += 1;
        }
        if self.mismatch_n != 0 {
            return Err(TriCode::CaseVerdictMismatch.code());
        }
        self.phase = self.phase.advance(GatePhase::Sealed)?;
        Ok(())
    }
}

/// 段判码映射（段号 → 互异段判码；越界落 Unknown）。
pub fn seg_fail_code(seg: usize) -> u32 {
    match seg {
        0 => TriCode::Seg0Fail.code(),
        1 => TriCode::Seg1Fail.code(),
        2 => TriCode::Seg2Fail.code(),
        _ => TriCode::Unknown.code(),
    }
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：矩阵 4 态先跑全覆盖 → 场景 20 例对拍 20/20 → 贡献守恒
/// → 收口。
pub fn golden_run() -> Result<(u32, u32), u32> {
    let mut g = JointGate::new();
    let mut st = 0;
    while st < MATRIX_N {
        g.run_matrix_state(st)?;
        st += 1;
    }
    let ok_n = g.run_scenarios()?;
    if ok_n != SCEN_N as u32 {
        return Err(TriCode::CaseVerdictMismatch.code());
    }
    g.conserve()?;
    g.seal()?;
    Ok((ok_n, g.mismatch_n))
}

// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    pub fn missed(&self) -> u32 {
        let mut m = 0u32;
        let mut i = 0;
        while i < 3 {
            m += self.tries[i].saturating_sub(self.caught[i]);
            i += 1;
        }
        m
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入核：三类违约各 JUDGE_ROUNDS 轮，逐类断言被显式拒绝。
///   类 0：关断位贡献（mask=6 且段 0 旗=1 → SegDisabledContrib）
///   类 1：非法掩码（mask=0 / mask=8 → MaskIllegal）
///   类 2：场景判码漂移（期望值被篡改 → 对拍不平，seal 拒）
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        // 类 0：关断位贡献。
        let mut a = JointGate::new();
        let hit_a = match a.run(6, [1, 0, 0]) {
            Err(c) if c == TriCode::SegDisabledContrib.code() => true,
            _ => false,
        };
        led.record(0, hit_a);
        // 类 1：非法掩码（下界 0 与上界外 8）。
        let mut b = JointGate::new();
        let hit_b1 = match b.run(0, [0; SEG_N]) {
            Err(c) if c == TriCode::MaskIllegal.code() => true,
            _ => false,
        };
        let hit_b2 = match b.run(8, [0; SEG_N]) {
            Err(c) if c == TriCode::MaskIllegal.code() => true,
            _ => false,
        };
        led.record(1, hit_b1 && hit_b2);
        // 类 2：场景判码漂移（好例期望被改成段判码）。
        let mut c = JointGate::new();
        let mut st = 0;
        while st < MATRIX_N {
            c.run_matrix_state(st)?;
            st += 1;
        }
        c.run_scenarios()?;
        c.mismatch_n = 1; // 注入：伪造一次对拍不平。
        let hit_c = match c.seal() {
            Err(cc) if cc == TriCode::CaseVerdictMismatch.code() => true,
            _ => false,
        };
        led.record(2, hit_c);
        r += 1;
    }
    Ok(led)
}

// ── 边界与探针 ──────────────────────────────────────────────────
/// 边界探针：场景号越界（矩阵态）/ 段判码映射闭集 / 重复场景。
pub fn boundary_probe() -> Result<(), u32> {
    let mut g = JointGate::new();
    // 矩阵态越界。
    match g.run_matrix_state(MATRIX_N) {
        Err(c) if c == TriCode::CaseOutOfRange.code() => {}
        _ => return Err(TriCode::CaseOutOfRange.code()),
    }
    // 段判码映射闭集：三码互异。
    let c0 = seg_fail_code(0);
    let c1 = seg_fail_code(1);
    let c2 = seg_fail_code(2);
    if c0 == c1 || c1 == c2 || c0 == c2 {
        return Err(TriCode::ConservationBreak.code());
    }
    // 重复场景：同账二跑场景表必拒。
    let mut h = JointGate::new();
    h.run_scenarios()?;
    match h.run_scenarios() {
        Err(c) if c == TriCode::DupCase.code() => Ok(()),
        _ => Err(TriCode::DupCase.code()),
    }
}

/// 探针 A：矩阵态缺（只跑 3 态即收口）必拒。
pub fn probe_matrix_missing() -> Result<(), u32> {
    let mut g = JointGate::new();
    let mut st = 0;
    while st < MATRIX_N - 1 {
        g.run_matrix_state(st)?;
        st += 1;
    }
    g.run_scenarios()?;
    match g.seal() {
        Err(c) if c == TriCode::MatrixStateMissing.code() => Ok(()),
        _ => Err(TriCode::MatrixStateMissing.code()),
    }
}

/// 探针 B：相位越级（冷账直取收口）必拒。
pub fn probe_phase_skip() -> Result<(), u32> {
    let g = JointGate::new();
    let r = match GatePhase::Cold.advance(GatePhase::Sealed) {
        Err(c) if c == TriCode::ConservationBreak.code() => Ok(()),
        _ => Err(TriCode::ConservationBreak.code()),
    };
    let _ = g;
    r
}

/// 探针 C：段判码逐段命中（好例过、逐段违约给对应段判码）。
pub fn probe_seg_codes() -> Result<(), u32> {
    let mut k = 0;
    while k < SEG_N {
        let mut modes = [0u32; SEG_N];
        modes[k] = 1;
        let mut g = JointGate::new();
        match g.run(7, modes) {
            Err(c) if c == seg_fail_code(k) => {}
            _ => return Err(seg_fail_code(k)),
        }
        k += 1;
    }
    Ok(())
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(TriCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫：SEG 对齐/窗位 + 码闭集段内 + 文案非空。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x84000).contains(&SEG_BASE) {
        return false;
    }
    let mut i = 0;
    while i < TriCode::ALL.len() {
        let v = TriCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        if TriCode::ALL[i].detail().len() == 0 {
            return false;
        }
        i += 1;
    }
    // 场景表守恒：20 行、场景号连续、三段旗二元闭集。
    if SCEN_TABLE.len() != SCEN_N {
        return false;
    }
    let mut i = 0;
    while i < SCEN_N {
        let row = SCEN_TABLE[i];
        if row.cid != i as u32 {
            return false;
        }
        let mut k = 0;
        while k < SEG_N {
            if row.modes[k] > 1 {
                return false;
            }
            k += 1;
        }
        i += 1;
    }
    true
}

/// 运行期摘要（动态明细消费）。
pub fn summary() -> String {
    let g = golden_run();
    format!("{:?}", g)
}

// ── 检查入口（判据 8–20 条） ────────────────────────────────────
pub fn run_unx18566_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18566 三段联合通判据（锚 2 前半集成跑）");

    // ── 静态明细（主体） ─────────────────────────────────────
    let golden = golden_run();
    s.add("J1-三段同开20例集成全过", golden.is_ok(), "矩阵4态+场景20例对拍+守恒+收口");
    let ok2 = match &golden {
        Ok((n, mis)) => *n == SCEN_N as u32 && *mis == 0,
        Err(_) => false,
    };
    s.add("J1-20例对拍20/20", ok2, "期望判码与实判逐行相等、不平为零");
    s.add("J3-矩阵态缺必拒", probe_matrix_missing().is_ok(), "只跑3态收口被 MatrixStateMissing 拒");
    s.add("J3-相位越级必拒", probe_phase_skip().is_ok(), "冷账直取收口被 ConservationBreak 拒");
    s.add("J3-段判码逐段命中", probe_seg_codes().is_ok(), "三段违约各给互异段判码");
    s.add("J4-边界（矩阵态越界/判码互异/重复场景）", boundary_probe().is_ok(), "三类界外全部显式拒");
    s.add("J7-段位与场景表守卫", segment_guard(), "0x80940 段 0x40 窗、20 行连续、旗二元闭集");

    // ── 动态明细（≤8 槽） ────────────────────────────────────
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    let (t, c) = match &led {
        Ok(l) => (l.tries[0] + l.tries[1] + l.tries[2], l.caught[0] + l.caught[1] + l.caught[2]),
        Err(_) => (0u32, 0u32),
    };
    s.add_dyn("J2-注入10/10全抓", led_ok, &format!("三类各{}轮 抓{}/{}", JUDGE_ROUNDS, c, t));
    s.add_dyn("J2-漏检为零", match &led { Ok(l) => l.missed() == 0, Err(_) => false }, &format!("漏检{}", match &led { Ok(l) => l.missed(), Err(_) => 99u32 }));
    s.add_dyn("J6-双跑一致", repro_check().is_ok(), &format!("golden={:?}", summary()));
    s.add_dyn("J5-贡献账守恒", conserve_probe().is_ok(), &format!("守恒={:?}", conserve_probe().err()));
    s
}

/// 贡献账守恒探针（判据推导复算：矩阵 4 态逐段放行 7+6+5+3 拆位）。
pub fn conserve_probe() -> Result<(), u32> {
    let g = golden_run()?;
    let _ = g;
    let mut h = JointGate::new();
    let mut st = 0;
    while st < MATRIX_N {
        h.run_matrix_state(st)?;
        st += 1;
    }
    // 矩阵单独守恒：段 0 放行 3 态（7/5/3 位 0）、段 1 三态（7/6/3 位 1）、
    // 段 2 四态（7/6/5/3 位 2）。
    h.conserve()?;
    Ok(())
}

// ── 测试区 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checks_all_passed() {
        let set = run_unx18566_checks();
        assert!(set.all_passed(), "UNX-F18566 判据集必须全绿: {}", set.red_items_text());
        assert!(set.len() >= 8 && set.len() <= 20);
    }

    #[test]
    fn test_golden_run_green() {
        let g = golden_run();
        assert!(g.is_ok());
        let (ok_n, mis) = g.unwrap_or((0, 1));
        assert_eq!(ok_n, SCEN_N as u32);
        assert_eq!(mis, 0);
    }

    #[test]
    fn test_matrix_four_states_covered() {
        let mut g = JointGate::new();
        let mut st = 0;
        while st < MATRIX_N {
            assert!(g.run_matrix_state(st).is_ok());
            st += 1;
        }
        for f in g.matrix_cover.iter() {
            assert!(*f);
        }
    }

    #[test]
    fn test_mask_illegal_rejected() {
        let mut g = JointGate::new();
        assert_eq!(g.run(0, [0; SEG_N]).err(), Some(TriCode::MaskIllegal.code()));
        assert_eq!(g.run(8, [0; SEG_N]).err(), Some(TriCode::MaskIllegal.code()));
        assert!(g.run(7, [0; SEG_N]).is_ok());
    }

    #[test]
    fn test_disabled_contrib_rejected() {
        let mut g = JointGate::new();
        assert_eq!(
            g.run(6, [1, 0, 0]).err(),
            Some(TriCode::SegDisabledContrib.code())
        );
        assert!(g.run(6, [0, 0, 0]).is_ok());
    }

    #[test]
    fn test_seg_codes_distinct_and_hit() {
        assert!(probe_seg_codes().is_ok());
        assert_ne!(TriCode::Seg0Fail.code(), TriCode::Seg1Fail.code());
        assert_ne!(TriCode::Seg1Fail.code(), TriCode::Seg2Fail.code());
    }

    #[test]
    fn test_scenario_table_shape() {
        assert_eq!(SCEN_TABLE.len(), 20);
        let mut i = 0;
        while i < SCEN_N {
            assert_eq!(SCEN_TABLE[i].cid, i as u32);
            assert!(SCEN_TABLE[i].mask >= 1 && SCEN_TABLE[i].mask <= 7);
            i += 1;
        }
    }

    #[test]
    fn test_boundary_and_dup() {
        assert!(boundary_probe().is_ok());
        assert!(probe_matrix_missing().is_ok());
        assert!(probe_phase_skip().is_ok());
    }

    #[test]
    fn test_inject_rounds_all_caught() {
        let led = inject_rounds(JUDGE_ROUNDS);
        let l = led.unwrap_or(InjectLedger::new());
        assert!(l.all_caught());
        assert_eq!(l.missed(), 0);
        for i in 0..3 {
            assert_eq!(l.tries[i], JUDGE_ROUNDS);
            assert_eq!(l.caught[i], JUDGE_ROUNDS);
        }
    }

    #[test]
    fn test_repro_dual_run() {
        assert!(repro_check().is_ok());
        let a = golden_run().unwrap_or((0, 1));
        let b = golden_run().unwrap_or((1, 0));
        assert_eq!(a, b);
    }

    #[test]
    fn test_segment_guard_and_detail() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80940);
        assert_eq!(SEG_BASE % 0x40, 0);
        for c in TriCode::ALL.iter() {
            assert_eq!(detail(c.code()), c.detail());
        }
        assert!(detail(SEG_BASE + 0x3F).contains("未知"));
    }
}
