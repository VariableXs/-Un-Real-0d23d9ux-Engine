//! veb278_nvfall —— VE-F0278 NVIDIA 回退策略。
//! 锚点（CoRun Unxreal 主册）：VE-F0278 · NVIDIA 回退策略 · 目标 320 行
//! 判据原文：VE-F0278-J1 回退策略账：能力回退梯（四档：全速 /
//!           降档 / 兼容 / 安全模）；触发源三类（驱动加载败 /
//!           提交通路败 / 显示通音败）；回退一步（当前档
//!           降一级 + 记触发源）；恢复探测（连续 8 次探测过
//!           → 升一档）；梯底安全模（兼容档再败 → 安全模锁
//!           定，恢复探测关）；回退链核（四档全走一遍不改
//!           触发源归属）；锁核（安全模只进不出）。
//! 组件：回退梯 + 触发源账 + 恢复探测 + 锁。
//! 职责定位：回退策略是「NVIDIA 卡的救命梯」——四档不是
//!           功能等级是风险等级：每降一档换一分稳定；触发源
//!           归属不变是诊断的锚：降多少档都得知道为什么降；
//!           安全模只进不出是最后的墙：救不回来就停在能活
//!           的地方。
//! 性能逐项分解：回退/探测 O(1)。
//! 诊断码段：0x30E80（0x30E81~0x30E89 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const FB_VERSION: &str = "veb278_nvfall-v1";

/// 诊断段基址（0x30E80 段 · 本模块独占）。
pub const FB_SEG_BASE: u32 = 0x30E80;
pub const XC_MAGIC: u32 = 0x30E81; // 账魔数破
pub const XC_SRC: u32 = 0x30E82; // 触发源不合法
pub const XC_LOCKED: u32 = 0x30E83; // 安全模锁定
pub const XC_TOP: u32 = 0x30E84; // 已在梯顶

/// 账魔数。
const MAGIC: u32 = 0x0E80_1B99;

/// 触发源三类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// 驱动加载败。
    LoadFail,
    /// 提交通路败。
    SubmitFail,
    /// 显示通路败。
    DisplayFail,
}

/// 回退梯四档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 全速。
    Full,
    /// 降档。
    Reduced,
    /// 兼容。
    Compat,
    /// 安全模。
    SafeMode,
}

/// 回退策略账。
pub struct FallBook {
    tier: Tier,
    /// 本会话触发源（首次定归属，不随降级改）。
    pub owner: Option<Trigger>,
    /// 降级次数。
    pub drops: u32,
    /// 恢复探测连过计数。
    probe_ok: u32,
    /// 安全模锁定旗。
    pub locked: bool,
    magic: u32,
}

/// 恢复探测阈值。
pub const PROBE_TH: u32 = 8;

impl FallBook {
    /// 建账：全速起步。
    pub fn new() -> FallBook {
        FallBook {
            tier: Tier::Full,
            owner: None,
            drops: 0,
            probe_ok: 0,
            locked: false,
            magic: MAGIC,
        }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 回退一步：降一档并记触发源（归属只定一次）。
    pub fn drop_one(&mut self, src: Trigger) -> Result<Tier, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if self.locked {
            return Err(XC_LOCKED);
        }
        if self.owner.is_none() {
            self.owner = Some(src);
        }
        self.probe_ok = 0;
        self.drops += 1;
        self.tier = match self.tier {
            Tier::Full => Tier::Reduced,
            Tier::Reduced => Tier::Compat,
            // 梯底：兼容档再败 → 安全模锁定。
            Tier::Compat | Tier::SafeMode => {
                self.tier = Tier::SafeMode;
                self.locked = true;
                return Ok(Tier::SafeMode);
            }
        };
        Ok(self.tier)
    }

    /// 恢复探测：过则计数，连过 8 次升一档；锁定态探测关。
    pub fn probe_pass(&mut self) -> Result<Option<Tier>, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if self.locked {
            return Err(XC_LOCKED);
        }
        self.probe_ok += 1;
        if self.probe_ok >= PROBE_TH && self.tier != Tier::Full {
            self.probe_ok = 0;
            self.tier = match self.tier {
                Tier::SafeMode => Tier::Compat,
                Tier::Compat => Tier::Reduced,
                _ => Tier::Full,
            };
            return Ok(Some(self.tier));
        }
        Ok(None)
    }

    /// 当前档。
    pub fn tier(&self) -> Tier {
        self.tier
    }

    /// 探测连过数。
    pub fn probe_streak(&self) -> u32 {
        self.probe_ok
    }
}

pub fn run_veb278_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0278");
    // 族一：起步与归属。
    let mut b = FallBook::new();
    s.add("起步全速", b.tier() == Tier::Full, "梯顶");
    s.add("归属未定", b.owner.is_none(), "无事不降");
    s.add("首降记源", b.drop_one(Trigger::SubmitFail) == Ok(Tier::Reduced), "降一档");
    s.add("归属提交", b.owner == Some(Trigger::SubmitFail), "锚定");
    // 族二：回退链核——继续降，归属不变。
    s.add("再降兼容", b.drop_one(Trigger::DisplayFail) == Ok(Tier::Compat), "二档");
    s.add("归属不变", b.owner == Some(Trigger::SubmitFail), "首源锚");
    s.add("三降安全", b.drop_one(Trigger::LoadFail) == Ok(Tier::SafeMode), "梯底");
    s.add("降数 3", b.drops == 3, "三步");
    s.add("锁定立", b.locked, "只进不出");
    // 族三：锁核。
    s.add("锁后再降拒", b.drop_one(Trigger::LoadFail) == Err(XC_LOCKED), "锁死");
    s.add("锁后探测拒", b.probe_pass() == Err(XC_LOCKED), "探测关");
    s.add("档停安全", b.tier() == Tier::SafeMode, "不再动");
    // 族四：恢复探测（新账走一半）。
    let mut c = FallBook::new();
    let _ = c.drop_one(Trigger::LoadFail); // Reduced
    let mut promoted = false;
    for _ in 0..7 {
        if c.probe_pass() != Ok(None) {
            promoted = true;
        }
    }
    s.add("七探不升", promoted == false, "差一");
    s.add("连过 7", c.probe_streak() == 7, "计数核");
    // 第八次升档。
    s.add("八探升档", c.probe_pass() == Ok(Some(Tier::Full)), "回全速");
    s.add("梯顶再探", c.probe_pass() == Ok(None), "顶无升");
    // 族五：逐级回升核。
    let mut d = FallBook::new();
    let _ = d.drop_one(Trigger::SubmitFail);
    let _ = d.drop_one(Trigger::SubmitFail); // Compat
    let mut rises = Vec::new();
    // 手动推 8*2 次到 Reduced，再 8 次到 Full。中途记录每次升档。
    for _ in 0..16 {
        if d.probe_pass() != Ok(None) {
            rises.push(d.tier());
        }
    }
    s.add("先回降档", rises.first() == Some(&Tier::Reduced), "逐级");
    s.add("再回全速", rises.len() == 2, "两跳");
    // 中途失败清零：降后探测 3 次，再降 → 连过清零。
    let mut e = FallBook::new();
    let _ = e.drop_one(Trigger::DisplayFail);
    for _ in 0..3 {
        let _ = e.probe_pass();
    }
    let _ = e.drop_one(Trigger::DisplayFail); // Compat
    s.add("降级清探", e.probe_streak() == 0, "重计");
    s.add("魔数核", FB_SEG_BASE == 0x30E80, "段基址");
    s
}
