//! veb399_evolve —— VE-F0399 B 域十年演进预留。
//! 锚点（CoRun Unxreal 主册）：VE-F0399 · B 域十年演进预留 · 目标 320 行
//! 判据原文：VE-F0399-J1 演进预留三类：诊断码段预留（每年 4 段
//!           × 10 年 = 40 段）、能力位预留（32 位中划出低位段）、
//!           接口号段预留（接口名前缀带年段）；预留不得占用
//!           （占留检核）；预留可申请转正（申请 → 授予 → 占用
//!           合法）；预留台账（类 × 年份矩阵核数）。
//! 组件：预留矩阵（类 × 年）+ 占留核 + 转正流 + 台账。
//! 职责定位：演进预留是「B 域的储备用地」——十年后的扩展空间
//!           要现在圈出来，否则将来扩容必然和存量撞车；占留检
//!           核是红线：预留地不许违章建筑；转正流是合法开发通
//!           道：申请-授予-占用，留白变图斑全程有账。
//! 性能逐项分解：预留查 O(1)；转正 O(1)；台账 O(类×年)。
//! 诊断码段：0x30940（0x30941~0x30949 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const EV_VERSION: &str = "veb399_evolve-v1";

/// 诊断段基址（0x30940 段 · 本模块独占）。
pub const EV_SEG_BASE: u32 = 0x30940;
pub const XC_OCCUPIED: u32 = 0x30941; // 预留被占
pub const XC_NO_RESERVE: u32 = 0x30942; // 无此预留
pub const XC_YEAR_OOR: u32 = 0x30943; // 年份越域
pub const XC_CLASS_OOR: u32 = 0x30944; // 类越域
pub const XC_MAGIC: u32 = 0x30945; // 账魔数破
pub const XC_NOT_GRANTED: u32 = 0x30946; // 未授予先转正
pub const XC_TWICE: u32 = 0x30947; // 重复转正
pub const XC_LEDGER: u32 = 0x30948; // 台账不平
pub const XC_BITS_OOR: u32 = 0x30949; // 位段越域

/// 十年。
pub const YEARS: u32 = 10;
/// 类三类：诊断段 / 能力位 / 接口号。
pub const CLASSES: u32 = 3;
/// 每类每年预留数。
pub const PER_YEAR: u32 = 4;

/// 预留态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RsvState {
    /// 预留中（不可占）。
    Reserved = 0,
    /// 已授予（转正申请已批）。
    Granted = 1,
    /// 已占用（合法转正完成）。
    Occupied = 2,
}

/// 预留项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rsv {
    pub class: u32,
    pub year: u32,
    pub seq: u32,
    pub state: RsvState,
}

/// 演进预留账。
pub struct EvolveBook {
    items: Vec<Rsv>,
    magic: u32,
}

impl EvolveBook {
    /// 建：初始化全预留矩阵（3 类 × 10 年 × 4 序号 = 120 项）。
    pub fn new() -> EvolveBook {
        let mut items = Vec::new();
        for c in 0..CLASSES {
            for y in 0..YEARS {
                for q in 0..PER_YEAR {
                    items.push(Rsv { class: c, year: y, seq: q, state: RsvState::Reserved });
                }
            }
        }
        EvolveBook { items, magic: 0x0940 }
    }

    fn find(&self, class: u32, year: u32, seq: u32) -> Result<usize, u32> {
        if class >= CLASSES {
            return Err(XC_CLASS_OOR);
        }
        if year >= YEARS {
            return Err(XC_YEAR_OOR);
        }
        if seq >= PER_YEAR {
            return Err(XC_BITS_OOR);
        }
        for (i, r) in self.items.iter().enumerate() {
            if r.class == class && r.year == year && r.seq == seq {
                return Ok(i);
            }
        }
        Err(XC_NO_RESERVE)
    }

    /// 占留核：Reserved 态不许直接占用。
    pub fn occupy(&mut self, class: u32, year: u32, seq: u32) -> Result<(), u32> {
        let i = self.find(class, year, seq)?;
        match self.items[i].state {
            RsvState::Reserved => Err(XC_OCCUPIED),
            RsvState::Granted => {
                self.items[i].state = RsvState::Occupied;
                Ok(())
            }
            RsvState::Occupied => Err(XC_TWICE),
        }
    }

    /// 申请授予（预留 → 已授予）。
    pub fn grant(&mut self, class: u32, year: u32, seq: u32) -> Result<(), u32> {
        let i = self.find(class, year, seq)?;
        match self.items[i].state {
            RsvState::Reserved => {
                self.items[i].state = RsvState::Granted;
                Ok(())
            }
            _ => Err(XC_NOT_GRANTED),
        }
    }

    /// 预留态读。
    pub fn state_of(&self, class: u32, year: u32, seq: u32) -> Result<RsvState, u32> {
        let i = self.find(class, year, seq)?;
        Ok(self.items[i].state)
    }

    /// 台账核：总项数 = 3×10×4；各态计数和 = 总数。
    pub fn audit(&self) -> Result<u32, u32> {
        if self.magic != 0x0940 {
            return Err(XC_MAGIC);
        }
        if self.items.len() as u32 != CLASSES * YEARS * PER_YEAR {
            return Err(XC_LEDGER);
        }
        let mut r = 0u32;
        let mut g = 0u32;
        let mut o = 0u32;
        for it in self.items.iter() {
            match it.state {
                RsvState::Reserved => r += 1,
                RsvState::Granted => g += 1,
                RsvState::Occupied => o += 1,
            }
        }
        if r + g + o != self.items.len() as u32 {
            return Err(XC_LEDGER);
        }
        Ok(o)
    }

    /// 各态计数。
    pub fn count_state(&self, st: RsvState) -> u32 {
        let mut n = 0u32;
        for it in self.items.iter() {
            if it.state == st {
                n += 1;
            }
        }
        n
    }

    /// 总项数。
    pub fn total(&self) -> u32 {
        self.items.len() as u32
    }
}

pub fn run_veb399_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0399");
    // 族一：矩阵初始化。
    let mut b = EvolveBook::new();
    s.add("总项 120", b.total() == 120, "3×10×4");
    s.add("全预留", b.count_state(RsvState::Reserved) == 120, "零占用");
    s.add("审计过", b.audit() == Ok(0), "台账平");
    // 族二：占留核。
    s.add("预留直接占拒", b.occupy(0, 0, 0) == Err(XC_OCCUPIED), "违章建筑");
    s.add("预留态读", b.state_of(0, 0, 0) == Ok(RsvState::Reserved), "还是留白");
    // 族三：转正流。
    s.add("授予过", b.grant(0, 0, 0).is_ok(), "申请批");
    s.add("授予态读", b.state_of(0, 0, 0) == Ok(RsvState::Granted), "已批");
    s.add("授予后再授拒", b.grant(0, 0, 0) == Err(XC_NOT_GRANTED), "重复申请");
    s.add("授予后占过", b.occupy(0, 0, 0).is_ok(), "合法转正");
    s.add("占用态读", b.state_of(0, 0, 0) == Ok(RsvState::Occupied), "图斑");
    s.add("重复占拒", b.occupy(0, 0, 0) == Err(XC_TWICE), "一地一主");
    // 族四：台账。
    s.add("占用 1", b.audit() == Ok(1), "一转正");
    s.add("三态计数", b.count_state(RsvState::Reserved) == 119 && b.count_state(RsvState::Granted) == 0 && b.count_state(RsvState::Occupied) == 1, "120 守恒");
    // 族五：越域。
    s.add("类越域", b.state_of(3, 0, 0) == Err(XC_CLASS_OOR), "≥3");
    s.add("年越域", b.state_of(0, 10, 0) == Err(XC_YEAR_OOR), "≥10");
    s.add("序越域", b.state_of(0, 0, 4) == Err(XC_BITS_OOR), "≥4");
    s.add("查无态", b.state_of(1, 1, 1) == Ok(RsvState::Reserved), "在册");
    s.add("魔数核", EV_SEG_BASE == 0x30940, "段基址");
    s
}
