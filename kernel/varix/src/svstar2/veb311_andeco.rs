//! veb311_andeco —— VE-F0311 Android 生态兼容语义评估。
//! 锚点（CoRun Unxreal 主册）：VE-F0311 · Android 生态兼容语义评估 · 目标 300 行
//! 判据原文：VE-F0311-J1 生态评估账：四维评估（HAL 版本 / AHB
//!           硬件缓冲 / SyncFD 同步栅 / Gralloc 用法）；逐维
//!           三态（齐 / 半 / 缺）；生态分（齐 25 + 半 10，满
//!           分 100）；准入门槛（≥ 60）；关键维否决（HAL 缺
//!           → 一票拒）；补齐机制（半维补测齐 → 升级重算）；
//!           评估冻结。
//! 组件：四维账 + 否决闸 + 补齐 + 冻结。
//! 职责定位：生态评估是「Android 的入住检查」——四维是四件
//!           家当：HAL 是房产证（没有就别谈入住）、AHB 是搬
//!           家车、SyncFD 是门钥匙、Gralloc 是家具清单；补齐
//!           是补办手续：半维补齐就升级，不来虚的。
//! 性能逐项分解：评估 O(4)。
//! 诊断码段：0x316C0（0x316C1~0x316C9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const AE_VERSION: &str = "veb311_andeco-v1";

/// 诊断段基址（0x316C0 段 · 本模块独占）。
pub const AE_SEG_BASE: u32 = 0x316C0;
pub const XC_MAGIC: u32 = 0x316C1; // 账魔数破
pub const XC_DIM: u32 = 0x316C2; // 维号越界
pub const XC_FROZEN: u32 = 0x316C3; // 已冻结
pub const XC_VETO: u32 = 0x316C4; // 关键维否决

/// 账魔数。
const MAGIC: u32 = 0x316C_1B21;

/// 四维。
pub const DIM_HAL: usize = 0;
pub const DIM_AHB: usize = 1;
pub const DIM_SYNC: usize = 2;
pub const DIM_GRA: usize = 3;
pub const NDIM: usize = 4;

/// 三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EState {
    Full,
    Half,
    Miss,
}

/// 维分。
pub fn dim_score(st: EState) -> u32 {
    match st {
        EState::Full => 25,
        EState::Half => 10,
        EState::Miss => 0,
    }
}

/// 生态评估簿。
pub struct EcoEval {
    pub states: [EState; NDIM],
    pub frozen: bool,
    magic: u32,
}

impl EcoEval {
    /// 建：默认全缺。
    pub fn new() -> EcoEval {
        EcoEval { states: [EState::Miss; NDIM], frozen: false, magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 记维。
    pub fn set(&mut self, dim: usize, st: EState) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if self.frozen {
            return Err(XC_FROZEN);
        }
        if dim >= NDIM {
            return Err(XC_DIM);
        }
        self.states[dim] = st;
        Ok(())
    }

    /// 生态分。
    pub fn score(&self) -> u32 {
        let mut t = 0;
        for st in self.states.iter() {
            t += dim_score(*st);
        }
        t
    }

    /// 关键维否决：HAL 缺 → 拒。
    pub fn veto(&self) -> Result<(), u32> {
        if self.states[DIM_HAL] == EState::Miss {
            return Err(XC_VETO);
        }
        Ok(())
    }

    /// 准入：先否决再分 ≥ 60。
    pub fn admit(&self) -> Result<u32, u32> {
        self.veto()?;
        let sc = self.score();
        if sc >= 60 {
            Ok(sc)
        } else {
            Err(sc)
        }
    }

    /// 补齐：半 → 齐。
    pub fn complete_dim(&mut self, dim: usize) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if self.frozen {
            return Err(XC_FROZEN);
        }
        if dim >= NDIM {
            return Err(XC_DIM);
        }
        if self.states[dim] == EState::Half {
            self.states[dim] = EState::Full;
        }
        Ok(())
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        self.frozen = true;
        Ok(())
    }
}

pub fn run_veb311_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0311");
    // 族零：维分口径。
    s.add("齐 25", dim_score(EState::Full) == 25, "四维满分百");
    s.add("半 10", dim_score(EState::Half) == 10, "半分");
    s.add("缺 0", dim_score(EState::Miss) == 0, "零分");
    // 族一：记维。
    let mut e = EcoEval::new();
    s.add("维 4 拒", e.set(4, EState::Full) == Err(XC_DIM), "越界");
    s.add("初全缺", e.score() == 0, "白板");
    s.add("HAL 齐", e.set(DIM_HAL, EState::Full).is_ok(), "房产证");
    s.add("AHB 半", e.set(DIM_AHB, EState::Half).is_ok(), "半家当");
    s.add("SyncFD 齐", e.set(DIM_SYNC, EState::Full).is_ok(), "钥匙");
    s.add("Gralloc 缺", e.set(DIM_GRA, EState::Miss).is_ok(), "没家具");
    s.add("分 60", e.score() == 60, "25+10+25");
    s.add("恰线准入", e.admit() == Ok(60), "≥60");
    // 族二：否决闸。
    let mut v = EcoEval::new();
    let _ = v.set(DIM_AHB, EState::Full);
    let _ = v.set(DIM_SYNC, EState::Full);
    let _ = v.set(DIM_GRA, EState::Full);
    s.add("HAL 缺否决", v.admit() == Err(XC_VETO), "一票");
    s.add("HAL 半不否决", {
        let _ = v.set(DIM_HAL, EState::Half);
        v.veto().is_ok()
    }, "半维过关");
    // 族三：补齐。
    let mut c = EcoEval::new();
    let _ = c.set(DIM_HAL, EState::Full);
    let _ = c.set(DIM_AHB, EState::Half);
    let _ = c.set(DIM_SYNC, EState::Half);
    let _ = c.set(DIM_GRA, EState::Half);
    s.add("补前 55", c.score() == 55, "25+30");
    s.add("补前拒", c.admit().is_err(), "55<60");
    s.add("补 AHB", c.complete_dim(DIM_AHB).is_ok(), "半→齐");
    s.add("补后 70", c.score() == 70, "25+25+10+10");
    s.add("补后准入", c.admit() == Ok(70), "过线");
    s.add("补齐维空转", c.complete_dim(DIM_AHB).is_ok() && c.score() == 70, "幂等");
    s.add("补缺维空转", c.complete_dim(DIM_GRA).is_ok() && c.score() == 70, "缺不动");
    // 三半全补 → 100。
    let _ = c.complete_dim(DIM_SYNC);
    let _ = c.complete_dim(DIM_GRA);
    s.add("全齐 100", c.score() == 100, "满分");
    // 族四：冻结。
    s.add("冻结", c.freeze().is_ok(), "出报告");
    s.add("冻后记拒", c.set(DIM_HAL, EState::Half) == Err(XC_FROZEN), "只读");
    s.add("冻后补拒", c.complete_dim(DIM_GRA) == Err(XC_FROZEN), "不可补");
    s.add("冻后再冻", c.freeze().is_ok(), "幂等");
    // 族五：缺维拒分。
    let m = EcoEval::new();
    s.add("白板否决", m.admit() == Err(XC_VETO), "HAL 缺先拒");
    s.add("魔数核", AE_SEG_BASE == 0x316C0, "段基址");
    s
}
