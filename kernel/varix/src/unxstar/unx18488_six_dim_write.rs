//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18488 · 覆写维度集（六维全支持） · 0x7F5C0 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18488-J1 六维（主/次/Build/SP/ProductType/Suite）逐维覆写
//!       断言全过
//! 本模块结构清单：
//!   1. 冻结常量层：六维闭集 / 逐维值域（主 1..=10，次 0..=99，
//!      Build 0..=65535，SP 0..=15，ProductType 1..=6，Suite 0..=0xFF）/
//!      判据轮数 10；
//!   2. 诊断码闭集 SdCode（code()=SEG_BASE+n，detail() 中文闭集逐码非空）；
//!   3. 数据结构：
//!      - 数组账本 DimBoard：[DimSlot; 6]，每维挂基线值/覆写值/写计数；
//!      - 位面 OvrPlane：u8 低六位，第 d 位=维 d 在覆写态；
//!      - 状态机 DimState：Base→Written→Restored 三态单向（再写须先还原
//!        ——v1 单写纪律）；
//!   4. 逐维判定器 validate(dim,value)：域界按维定制（四类边界各自落位）；
//!   5. 错误路径：维号越界 / 值越界（逐维）/ 覆写态重复写 / 幽灵还原
//!      / 位面失衡，全部显式拒绝码；
//!   6. 注入核：值越界（六维各一个越界样本轮换）/ 维号越界 / 覆写态
//!      重复写 / 幽灵还原 四类坏样本逐类 10/10 断言被检出；
//!   7. run_unx18488_checks：14 条判据（逐维往返/守恒/单调/确定性/非法必拒）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段基址（0x7F5C0，G1 工单 A09 批05 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7F5C0;

// ── 冻结常量层：六维闭集 ───────────────────────────────────────────
/// 维号：0=主版本 / 1=次版本 / 2=Build / 3=SP / 4=ProductType / 5=Suite。
pub const DIM_MAJOR: usize = 0;
pub const DIM_MINOR: usize = 1;
pub const DIM_BUILD: usize = 2;
pub const DIM_SP: usize = 3;
pub const DIM_PT: usize = 4;
pub const DIM_SUITE: usize = 5;
/// 维数（冻结 6）。
pub const DIM_N: usize = 6;
/// 逐维值域闭集（冻结 v1，推导：主档对齐 18404 版本矩阵域）。
/// 主版本 1..=10；次版本 0..=99；Build 0..=65535（u16 域）；
/// SP 0..=15（4 位域）；ProductType 1..=6（SKU 闭集）；Suite 0..=0xFF。
pub const DIM_MIN: [u32; DIM_N] = [1, 0, 0, 0, 1, 0];
pub const DIM_MAX: [u32; DIM_N] = [10, 99, 65535, 15, 6, 0xFF];
/// 判据轮数（写死 10：判据原文"逐维覆写断言全过"取 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 六维语义版本号（冻结 v1）。
pub const SDIM_VER_V1: u32 = 1;

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SdCode {
    /// 维号越界：超出 0..=5 六维闭集
    DimOutOfRange,
    /// 值越界：逐维域界外
    ValueOutOfRange,
    /// 覆写态重复写：Written 态未还原再写
    DupWrite,
    /// 幽灵还原：Base 态还原（无覆写可还原）
    GhostClear,
    /// 位面失衡：覆写位面计数与 Written 维数不符
    PlaneMismatch,
    /// 回读失配：写入值与读出值不一致（往返违例）
    ReadbackMismatch,
    /// 守恒破坏：写次数=覆写+还原+拒收失衡
    ConservationBroken,
    /// 幽灵读：对未写维要求覆写值
    GhostRead,
    /// 幽灵维：维号未挂基线
    GhostDim,
    /// 双跑不一致
    NotReproducible,
    /// 域界表失配：MIN/MAX 表长度≠6（自检）
    BoundsTableBroken,
    /// 六维回读不全：六维写后应全覆写态
    SixDimIncomplete,
}

impl SdCode {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            SdCode::DimOutOfRange => "维号越界：超出 0..=5 六维闭集",
            SdCode::ValueOutOfRange => "值越界：逐维域界外，按维闭集校验",
            SdCode::DupWrite => "覆写态重复写：Written 态未还原再写，v1 单写纪律",
            SdCode::GhostClear => "幽灵还原：Base 态还原，无覆写可还原",
            SdCode::PlaneMismatch => "位面失衡：覆写位面计数与 Written 维数不符",
            SdCode::ReadbackMismatch => "回读失配：写入值与读出值不一致",
            SdCode::ConservationBroken => "守恒破坏：写次数=覆写+还原+拒收失衡",
            SdCode::GhostRead => "幽灵读：对未写维要求覆写值",
            SdCode::GhostDim => "幽灵维：维号未挂基线",
            SdCode::NotReproducible => "双跑不一致：复现红线",
            SdCode::BoundsTableBroken => "域界表失配：MIN/MAX 表长度≠6",
            SdCode::SixDimIncomplete => "六维回读不全：六维写后应全覆写态",
        }
    }
    pub const ALL: [SdCode; 12] = [
        SdCode::DimOutOfRange,
        SdCode::ValueOutOfRange,
        SdCode::DupWrite,
        SdCode::GhostClear,
        SdCode::PlaneMismatch,
        SdCode::ReadbackMismatch,
        SdCode::ConservationBroken,
        SdCode::GhostRead,
        SdCode::GhostDim,
        SdCode::NotReproducible,
        SdCode::BoundsTableBroken,
        SdCode::SixDimIncomplete,
    ];
}

/// 未知码回查（闭集外返回固定文案）。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < SdCode::ALL.len() {
        if let Some(c) = SdCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7F5C0 段闭集内"
}

// ── 维状态机（Base→Written→Restored 单向） ─────────────────────────
/// 维三态（冻结 v1，只许单向推进；Restored 为该轮终态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DimState {
    /// 基线：未覆写。
    Base,
    /// 已覆写：覆写值在场。
    Written,
    /// 已还原：覆写已撤销（本轮终态）。
    Restored,
}

/// 单维槽。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DimSlot {
    /// 维号（0..=5）。
    pub dim: usize,
    /// 基线值（下级源）。
    pub baseline: u32,
    /// 覆写值（Written 态有效）。
    pub ovr_val: u32,
    /// 维状态。
    pub state: DimState,
    /// 本维累计写次数。
    pub write_count: u32,
}

impl DimSlot {
    pub const fn empty() -> DimSlot {
        DimSlot { dim: 0, baseline: 0, ovr_val: 0, state: DimState::Base, write_count: 0 }
    }
}

// ── 覆写位面（低六位，第 d 位=维 d 在覆写态） ──────────────────────
/// 覆写位面：6 维用 u8 低六位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OvrPlane(pub u8);

impl OvrPlane {
    pub const fn new() -> OvrPlane {
        OvrPlane(0)
    }
    pub fn set(&mut self, dim: usize) {
        if dim < DIM_N {
            self.0 |= 1u8 << dim;
        }
    }
    pub fn clear(&mut self, dim: usize) {
        if dim < DIM_N {
            self.0 &= !(1u8 << dim);
        }
    }
    pub fn get(self, dim: usize) -> bool {
        if dim < DIM_N {
            (self.0 >> dim) & 1u8 == 1u8
        } else {
            false
        }
    }
    pub fn count(self) -> u32 {
        let mut n = 0u32;
        let mut b = self.0;
        while b != 0 {
            if b & 1u8 == 1u8 {
                n = n.saturating_add(1);
            }
            b >>= 1;
        }
        n
    }
}

// ── 逐维判定器 ─────────────────────────────────────────────────────
/// 逐维值域校验：MIN/MAX 表按维取界。越界返回显式码。
pub fn validate_value(dim: usize, value: u32) -> Result<(), u32> {
    if dim >= DIM_N {
        return Err(SdCode::DimOutOfRange.code());
    }
    let lo = match DIM_MIN.get(dim) {
        Some(v) => *v,
        None => return Err(SdCode::BoundsTableBroken.code()),
    };
    let hi = match DIM_MAX.get(dim) {
        Some(v) => *v,
        None => return Err(SdCode::BoundsTableBroken.code()),
    };
    if value < lo || value > hi {
        return Err(SdCode::ValueOutOfRange.code());
    }
    Ok(())
}

/// 域界表自检：MIN/MAX 表长度与单调序（lo ≤ hi）双查。
pub fn bounds_table_audit() -> bool {
    if DIM_MIN.len() != DIM_N || DIM_MAX.len() != DIM_N {
        return false;
    }
    let mut i = 0;
    while i < DIM_N {
        let lo = match DIM_MIN.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let hi = match DIM_MAX.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if lo > hi {
            return false;
        }
        i += 1;
    }
    true
}

// ── 六维写板 ───────────────────────────────────────────────────────
/// 六维写板：定容 6 维 + 位面 + 守恒账。
pub struct DimBoard {
    pub slots: [DimSlot; DIM_N],
    pub plane: OvrPlane,
    /// 覆写写总次数。
    pub write_total: u32,
    /// 还原总次数。
    pub restore_total: u32,
    /// 被拒操作总数。
    pub rejected_total: u32,
    /// 拒因账。
    pub reject_log: Vec<u32>,
}

impl DimBoard {
    /// 建板：六维基线由调用方冻结注入（逐维必须过域界校验）。
    pub fn new(baselines: [u32; DIM_N]) -> Result<DimBoard, u32> {
        if !bounds_table_audit() {
            return Err(SdCode::BoundsTableBroken.code());
        }
        let mut d = 0;
        while d < DIM_N {
            let b = match baselines.get(d) {
                Some(v) => *v,
                None => return Err(SdCode::BoundsTableBroken.code()),
            };
            validate_value(d, b)?;
            d += 1;
        }
        let mut slots = [DimSlot::empty(); DIM_N];
        let mut i = 0;
        while i < DIM_N {
            if let Some(s) = slots.get_mut(i) {
                *s = DimSlot {
                    dim: i,
                    baseline: match baselines.get(i) {
                        Some(v) => *v,
                        None => 0,
                    },
                    ovr_val: 0,
                    state: DimState::Base,
                    write_count: 0,
                };
            }
            i += 1;
        }
        Ok(DimBoard {
            slots,
            plane: OvrPlane::new(),
            write_total: 0,
            restore_total: 0,
            rejected_total: 0,
            reject_log: Vec::new(),
        })
    }

    /// 读当前生效值：Written 返回覆写值，否则返回基线。
    pub fn effective(&self, dim: usize) -> Result<u32, u32> {
        if dim >= DIM_N {
            return Err(SdCode::DimOutOfRange.code());
        }
        match self.slots.get(dim) {
            Some(s) => {
                if s.state == DimState::Written {
                    Ok(s.ovr_val)
                } else {
                    Ok(s.baseline)
                }
            }
            None => Err(SdCode::GhostDim.code()),
        }
    }

    /// 覆写写：基线态才可写（单写纪律），值过逐维域界。
    pub fn write(&mut self, dim: usize, value: u32) -> Result<(), u32> {
        validate_value(dim, value)?;
        let state = match self.slots.get(dim) {
            Some(s) => s.state,
            None => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(SdCode::GhostDim.code());
                return Err(SdCode::GhostDim.code());
            }
        };
        match state {
            DimState::Base => {
                if let Some(s) = self.slots.get_mut(dim) {
                    s.ovr_val = value;
                    s.state = DimState::Written;
                    s.write_count = s.write_count.saturating_add(1);
                }
                self.plane.set(dim);
                self.write_total = self.write_total.saturating_add(1);
                Ok(())
            }
            DimState::Written => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(SdCode::DupWrite.code());
                Err(SdCode::DupWrite.code())
            }
            DimState::Restored => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(SdCode::DupWrite.code());
                Err(SdCode::DupWrite.code())
            }
        }
    }

    /// 还原：Written→Restored，生效值回基线。
    pub fn restore(&mut self, dim: usize) -> Result<u32, u32> {
        if dim >= DIM_N {
            return Err(SdCode::DimOutOfRange.code());
        }
        let state = match self.slots.get(dim) {
            Some(s) => s.state,
            None => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(SdCode::GhostDim.code());
                return Err(SdCode::GhostDim.code());
            }
        };
        if state != DimState::Written {
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(SdCode::GhostClear.code());
            return Err(SdCode::GhostClear.code());
        }
        let base = match self.slots.get(dim) {
            Some(s) => s.baseline,
            None => 0,
        };
        if let Some(s) = self.slots.get_mut(dim) {
            s.state = DimState::Restored;
            s.ovr_val = 0;
        }
        self.plane.clear(dim);
        self.restore_total = self.restore_total.saturating_add(1);
        Ok(base)
    }

    /// 位面对账：位面计数 = Written 维数。
    pub fn plane_audit(&self) -> Result<(), u32> {
        let mut n = 0u32;
        let mut i = 0;
        while i < DIM_N {
            if let Some(s) = self.slots.get(i) {
                if s.state == DimState::Written {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        if self.plane.count() != n {
            return Err(SdCode::PlaneMismatch.code());
        }
        Ok(())
    }

    /// 守恒：write_total = Written 数 + Restored 数。
    pub fn conserve(&self) -> Result<(), u32> {
        let mut live = 0u32;
        let mut done = 0u32;
        let mut i = 0;
        while i < DIM_N {
            if let Some(s) = self.slots.get(i) {
                if s.state == DimState::Written {
                    live = live.saturating_add(1);
                } else if s.state == DimState::Restored {
                    done = done.saturating_add(1);
                }
            }
            i += 1;
        }
        if self.write_total != live.saturating_add(done) {
            return Err(SdCode::ConservationBroken.code());
        }
        Ok(())
    }
}

// ── 黄金全链：六维逐维覆写往返 ─────────────────────────────────────
/// 黄金全链：六维各写各的中界值→逐维回读=覆写值→逐维还原→回读=基线。
pub fn golden_run() -> Result<(u32, u32), u32> {
    // 基线：全维取各维下界（合法）。
    let mut baselines = [0u32; DIM_N];
    let mut d = 0;
    while d < DIM_N {
        if let Some(v) = baselines.get_mut(d) {
            *v = match DIM_MIN.get(d) {
                Some(x) => *x,
                None => 0,
            };
        }
        d += 1;
    }
    let mut b = DimBoard::new(baselines)?;
    // 逐维写中界（(MIN+MAX)/2）。
    let mut d = 0;
    while d < DIM_N {
        let lo = match DIM_MIN.get(d) {
            Some(v) => *v,
            None => 0,
        };
        let hi = match DIM_MAX.get(d) {
            Some(v) => *v,
            None => 0,
        };
        let mid = lo + (hi - lo) / 2;
        b.write(d, mid)?;
        // 回读断言（判据核心：逐维覆写回读一致）。
        if b.effective(d)? != mid {
            return Err(SdCode::ReadbackMismatch.code());
        }
        d += 1;
    }
    if b.plane.count() != DIM_N as u32 {
        return Err(SdCode::SixDimIncomplete.code());
    }
    b.plane_audit()?;
    // 逐维还原。
    let mut d = 0;
    while d < DIM_N {
        let base = b.restore(d)?;
        let lo = match DIM_MIN.get(d) {
            Some(v) => *v,
            None => 0,
        };
        if base != lo || b.effective(d)? != lo {
            return Err(SdCode::ReadbackMismatch.code());
        }
        d += 1;
    }
    b.conserve()?;
    if b.write_total != DIM_N as u32 || b.restore_total != DIM_N as u32 {
        return Err(SdCode::ConservationBroken.code());
    }
    Ok((b.write_total, b.restore_total))
}

// ── 注入检出账 ─────────────────────────────────────────────────────
/// 四槽注入账。
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
            if let Some(t) = self.tries.get_mut(slot) {
                *t = t.saturating_add(1);
            }
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => 0,
            };
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn missed_total(&self) -> u32 {
        let mut m = 0u32;
        let mut i = 0;
        while i < 4 {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => 0,
            };
            m = m.saturating_add(t.saturating_sub(c));
            i += 1;
        }
        m
    }
}

/// 注入核：四类坏样本逐类 10/10 断言被检出。
///   槽 0：值越界——六维各一个上界+1 样本，按轮次轮换，10 轮全抓
///   槽 1：维号越界（dim=6）
///   槽 2：覆写态重复写
///   槽 3：幽灵还原（Base 态 restore）
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        // 槽 0：值越界——第 r 轮取维 (r % 6)，写上界+1。
        let dim = (r % DIM_N as u32) as usize;
        let hi = match DIM_MAX.get(dim) {
            Some(v) => *v,
            None => 0,
        };
        let mut b0 = match DimBoard::new([1, 0, 0, 0, 1, 0]) {
            Ok(b) => b,
            Err(_) => return Err(SdCode::BoundsTableBroken.code()),
        };
        let hit0 = match b0.write(dim, hi + 1) {
            Err(c) if c == SdCode::ValueOutOfRange.code() => true,
            _ => false,
        };
        led.record(0, hit0);
        // 槽 1：维号越界。
        let mut b1 = match DimBoard::new([1, 0, 0, 0, 1, 0]) {
            Ok(b) => b,
            Err(_) => return Err(SdCode::BoundsTableBroken.code()),
        };
        let hit1 = match b1.write(DIM_N, 1) {
            Err(c) if c == SdCode::DimOutOfRange.code() => true,
            _ => false,
        };
        led.record(1, hit1);
        // 槽 2：覆写态重复写。
        let mut b2 = match DimBoard::new([1, 0, 0, 0, 1, 0]) {
            Ok(b) => b,
            Err(_) => return Err(SdCode::BoundsTableBroken.code()),
        };
        let _ = b2.write(0, 5);
        let hit2 = match b2.write(0, 6) {
            Err(c) if c == SdCode::DupWrite.code() => true,
            _ => false,
        };
        led.record(2, hit2);
        // 槽 3：幽灵还原。
        let mut b3 = match DimBoard::new([1, 0, 0, 0, 1, 0]) {
            Ok(b) => b,
            Err(_) => return Err(SdCode::BoundsTableBroken.code()),
        };
        let hit3 = match b3.restore(0) {
            Err(c) if c == SdCode::GhostClear.code() => true,
            _ => false,
        };
        led.record(3, hit3);
        r = r.saturating_add(1);
    }
    Ok(led)
}

// ── 探针三序 ───────────────────────────────────────────────────────
/// 探针 A：单维往返全绿。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut b = DimBoard::new([3, 2, 100, 1, 2, 0x0F])?;
    b.write(0, 7)?;
    if b.effective(0)? != 7 {
        return Err(SdCode::ReadbackMismatch.code());
    }
    b.restore(0)?;
    Ok(())
}

/// 探针 B：维号越界必拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let b = DimBoard::new([3, 2, 100, 1, 2, 0x0F])?;
    b.effective(6).map(|_| ())
}

/// 探针 C：主维 0 值必拒（主版本下界 1）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut b = DimBoard::new([3, 2, 100, 1, 2, 0x0F])?;
    b.write(0, 0)
}

// ── 双跑确定性对拍 ─────────────────────────────────────────────────
/// 双跑：同黄金链两次逐字段相等。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SdCode::NotReproducible.code());
    }
    Ok(())
}

// ── 段位守卫 ───────────────────────────────────────────────────────
/// 段位守卫：SEG_BASE 对齐且全部诊断码落段内 0x40 窗口。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < SdCode::ALL.len() {
        if let Some(c) = SdCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 逐维四类边界探测：每维取 MIN（下界，收）/ MIN-1（拒）/ MAX（收）/
/// MAX+1（拒），6 维 × 4 类 = 24 探针全数落位。
pub fn four_bound_probe() -> Result<u32, u32> {
    let mut hits = 0u32;
    let mut d = 0;
    while d < DIM_N {
        let lo = match DIM_MIN.get(d) {
            Some(v) => *v,
            None => return Err(SdCode::BoundsTableBroken.code()),
        };
        let hi = match DIM_MAX.get(d) {
            Some(v) => *v,
            None => return Err(SdCode::BoundsTableBroken.code()),
        };
        // MIN 收（用新板每维一次，写后即弃）。
        let mut b = DimBoard::new([1, 0, 0, 0, 1, 0])?;
        if b.write(d, lo).is_ok() {
            hits = hits.saturating_add(1);
        }
        // MIN-1 拒（MIN=0 的维跳过下溢探针——0 已是下界，改验 lo 拒 0xFFFFFFFF）。
        let mut b2 = DimBoard::new([1, 0, 0, 0, 1, 0])?;
        if lo > 0 {
            if b2.write(d, lo - 1).is_err() {
                hits = hits.saturating_add(1);
            }
        } else if b2.write(d, 0xFFFF_FFFF).is_err() {
            hits = hits.saturating_add(1);
        }
        // MAX 收。
        let mut b3 = DimBoard::new([1, 0, 0, 0, 1, 0])?;
        if b3.write(d, hi).is_ok() {
            hits = hits.saturating_add(1);
        }
        // MAX+1 拒。
        let mut b4 = DimBoard::new([1, 0, 0, 0, 1, 0])?;
        if b4.write(d, hi + 1).is_err() {
            hits = hits.saturating_add(1);
        }
        d += 1;
    }
    if hits != (DIM_N as u32) * 4 {
        return Err(SdCode::ConservationBroken.code());
    }
    Ok(hits)
}

// ── 判据收口 ───────────────────────────────────────────────────────
/// 判据产出（14 条）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链-六维逐维往返", g.is_ok(), format!("{:?}", g)));
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-六维越界注入10/10",
        inj_ok,
        format!(
            "tries={:?} caught={:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    let miss = match &inj {
        Ok(l) => l.missed_total(),
        Err(_) => 1,
    };
    v.push(("J1-注入守恒漏检零", miss == 0, format!("missed={}", miss)));
    // 六维全覆盖复验：六维各写独立值，全覆写态，位面=6。
    let mut b = match DimBoard::new([2, 5, 1000, 1, 3, 0x11]) {
        Ok(x) => x,
        Err(_) => return v, // 不可达：基线合法
    };
    let _ = b.write(DIM_MAJOR, 9);
    let _ = b.write(DIM_MINOR, 88);
    let _ = b.write(DIM_BUILD, 60000);
    let _ = b.write(DIM_SP, 14);
    let _ = b.write(DIM_PT, 5);
    let _ = b.write(DIM_SUITE, 0xF0);
    let six_ok = b.plane.count() == DIM_N as u32
        && b.effective(DIM_MAJOR) == Ok(9)
        && b.effective(DIM_SUITE) == Ok(0xF0);
    v.push(("J2-六维全覆盖写读", six_ok, String::from("主/次/Build/SP/PT/Suite")));
    // 位面对账 + 守恒。
    let plane_ok = b.plane_audit().is_ok();
    v.push(("J2-覆写位面对账", plane_ok, String::from("plane.count=Written维数")));
    let conserve_ok = b.conserve().is_ok();
    v.push(("J2-写账守恒", conserve_ok, String::from("write=Written+Restored")));
    // 单调：还原后终态（Restored 再写必拒）。
    let _ = b.restore(DIM_MAJOR);
    let wt = b.write_total;
    let _ = b.write(DIM_MAJOR, 1);
    let mono = b.write_total == wt;
    v.push(("J3-还原后终态不重写", mono, format!("before={} after={}", wt, b.write_total)));
    // 逐维四类边界。
    let fb = four_bound_probe();
    v.push(("J4-逐维四类边界24探", fb.is_ok(), format!("{:?}", fb)));
    // 域界表自检。
    v.push(("J4-域界表自检", bounds_table_audit(), String::from("MIN/MAX 长度与序")));
    // 探针三序。
    let pa = probe_seq_a().is_ok();
    let pb = probe_seq_b().is_err();
    let pc = probe_seq_c().is_err();
    v.push(("J4-探针三序", pa && pb && pc, format!("a={} b!={} c!={}", pa, pb, pc)));
    // 双跑一致。
    v.push(("J5-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // 段位守卫。
    v.push(("J5-段位守卫", segment_guard(), String::from("0x7F5C0 窗口闭集")));
    // 文案闭集。
    let mut det_ok = true;
    let mut k = 0;
    while k < SdCode::ALL.len() {
        if let Some(c) = SdCode::ALL.get(k) {
            if detail_of(c.code()) != c.detail() || c.detail().len() == 0 {
                det_ok = false;
            }
        }
        k += 1;
    }
    v.push(("J5-诊断文案闭集非空", det_ok, format!("codes={}", SdCode::ALL.len())));
    v.push(("J5-判据收口", v.len() + 1 == 14, format!("n={}", v.len() + 1)));
    v
}

/// 探针入口：F18488 覆写维度集自检（14 条）。
pub fn run_unx18488_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18488 six_dim_write");
    let items = extra_checks();
    let mut i = 0;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i = i.saturating_add(1);
    }
    s
}

// ── 测试（8 个） ───────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 测试专用建板（基线合法，构造必成）。
    fn mk_board(baselines: [u32; DIM_N]) -> DimBoard {
        match DimBoard::new(baselines) {
            Ok(b) => b,
            Err(e) => panic!("建板应成功：code=0x{:X}", e),
        }
    }

    #[test]
    fn normal_flow_golden_chain() {
        // 正常流：六维逐维写→回读→还原。
        assert!(golden_run().is_ok(), "黄金全链应全绿");
    }

    #[test]
    fn six_dim_each_dim_roundtrip() {
        // 六维逐维独立往返：写→读=覆写值→还原→读=基线。
        let mut b = mk_board([4, 10, 2000, 2, 4, 0x33]);
        let cases: [(usize, u32, u32); 6] = [
            (DIM_MAJOR, 8, 4),
            (DIM_MINOR, 77, 10),
            (DIM_BUILD, 12345, 2000),
            (DIM_SP, 9, 2),
            (DIM_PT, 6, 4),
            (DIM_SUITE, 0xAA, 0x33),
        ];
        let mut j = 0;
        while j < cases.len() {
            let (dim, ovr, base) = match cases.get(j) {
                Some(c) => *c,
                None => break,
            };
            assert!(b.write(dim, ovr).is_ok(), "维 {} 覆写应过", dim);
            assert!(b.effective(dim) == Ok(ovr), "维 {} 回读应为覆写值", dim);
            assert!(b.restore(dim) == Ok(base), "维 {} 还原应回基线", dim);
            assert!(b.effective(dim) == Ok(base), "维 {} 还原后应回基线", dim);
            j += 1;
        }
    }

    #[test]
    fn injection_all_caught() {
        // 注入必抓：越界/维越界/重复写/幽灵还原 10/10。
        let led = inject_rounds(JUDGE_ROUNDS);
        assert!(led.is_ok());
        if let Ok(l) = led {
            assert!(l.all_caught(), "四槽必须全抓");
            assert!(l.missed_total() == 0, "漏检必须为 0");
            assert!(l.tries[0] == JUDGE_ROUNDS, "越界槽应满轮");
        }
    }

    #[test]
    fn per_dim_bounds_exhaustive() {
        // 逐维四类边界 24 探针全数落位。
        let fb = four_bound_probe();
        assert!(fb.is_ok());
        if let Ok(hits) = fb {
            assert!(hits == (DIM_N as u32) * 4, "24 探针应全数落位");
        }
        assert!(bounds_table_audit(), "域界表自检应过");
    }

    #[test]
    fn dup_write_and_ghost_clear() {
        // 单写纪律 + 幽灵还原。
        let mut b = mk_board([1, 0, 0, 0, 1, 0]);
        assert!(b.write(1, 50).is_ok());
        assert!(b.write(1, 51).err() == Some(SdCode::DupWrite.code()));
        assert!(b.restore(2).err() == Some(SdCode::GhostClear.code()));
        assert!(b.restore(1) == Ok(0), "已写维还原应回基线");
        assert!(b.restore(1).err() == Some(SdCode::GhostClear.code()));
    }

    #[test]
    fn dim_out_of_range() {
        // 维号越界：读写双向都拒。
        let mut b = mk_board([1, 0, 0, 0, 1, 0]);
        assert!(b.write(6, 1).err() == Some(SdCode::DimOutOfRange.code()));
        assert!(b.effective(6).err() == Some(SdCode::DimOutOfRange.code()));
        assert!(b.restore(6).err() == Some(SdCode::DimOutOfRange.code()));
        assert!(b.write(255, 1).err() == Some(SdCode::DimOutOfRange.code()));
    }

    #[test]
    fn double_run_deterministic() {
        // 双跑一致。
        assert!(repro_check().is_ok(), "双跑应逐字段一致");
    }

    #[test]
    fn segment_guard_and_checkset() {
        // 段位守卫 + 判据收口全绿。
        assert!(segment_guard(), "段位守卫应过");
        let set = run_unx18488_checks();
        assert!(set.all_passed(), "14 条判据应全绿");
        assert!(set.len() == 14, "判据条数应冻结 14");
    }
}
