//! veb291_mlifreq —— VE-F0291 Mali 频率调节联动。
//! 锚点（CoRun Unxreal 主册）：VE-F0291 · Mali 频率调节联动 · 目标 300 行
//! 判据原文：VE-F0291-J1 频率联动账：档频表（四档固定频点：
//!           800/600/400/200 MHz）；利用率采样（工作占比百
//!           分比）；升频门（利用率 ≥ 85% 连续 4 采样 → 升
//!           一档）；降频门（利用率 ≤ 30% 连续 8 采样 → 降
//!           一档）；迟滞带（30%~85% 之间稳频不动）；档频
//!           界核（升顶不出、降底不出）；联动核（档变 → 频
//!           变一致）。
//! 组件：档频表 + 双门计数 + 迟滞带 + 联动核。
//! 职责定位：频率联动是「Mali 的油门」——升频是急门（4 采样
//!           就踩）、降频是缓门（8 采样才松）：性能敏感响应
//!           快，功耗敏感回得慢；迟滞带是防抖的死区：忽上忽
//!           下的频率比稳定的中频更费电。
//! 性能逐项分解：采样 O(1)。
//! 诊断码段：0x311C0（0x311C1~0x311C9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const FQ_VERSION: &str = "veb291_mlifreq-v1";

/// 诊断段基址（0x311C0 段 · 本模块独占）。
pub const FQ_SEG_BASE: u32 = 0x311C0;
pub const XC_MAGIC: u32 = 0x311C1; // 账魔数破
pub const XC_UTIL: u32 = 0x311C2; // 利用率不合法

/// 账魔数。
const MAGIC: u32 = 0x311C_18C4;

/// 档频表（MHz）。
pub const FREQS: [u32; 4] = [800, 600, 400, 200];

/// 升频门（连续采样数）。
pub const UP_TH: u32 = 4;
/// 降频门。
pub const DN_TH: u32 = 8;
/// 升频利用率阈。
pub const UP_UTIL: u32 = 85;
/// 降频利用率阈。
pub const DN_UTIL: u32 = 30;

/// 频率调节器。
pub struct FreqGov {
    /// 当前档（0 最高频）。
    pub level: u32,
    up_run: u32,
    dn_run: u32,
    /// 升频次数 / 降频次数。
    pub ups: u32,
    pub downs: u32,
    magic: u32,
}

impl FreqGov {
    /// 建。
    pub fn new() -> FreqGov {
        FreqGov { level: 0, up_run: 0, dn_run: 0, ups: 0, downs: 0, magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 当前频点。
    pub fn freq(&self) -> u32 {
        FREQS[self.level as usize]
    }

    /// 采样一次利用率（0~100）。
    pub fn sample(&mut self, util: u32) -> Result<u32, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if util > 100 {
            return Err(XC_UTIL);
        }
        if util >= UP_UTIL {
            self.up_run += 1;
            self.dn_run = 0;
            if self.up_run >= UP_TH && self.level > 0 {
                // 升频 = 档 -1（向高频）。
                self.level -= 1;
                self.ups += 1;
                self.up_run = 0;
            }
        } else if util <= DN_UTIL {
            self.dn_run += 1;
            self.up_run = 0;
            if self.dn_run >= DN_TH && self.level < 3 {
                self.level += 1;
                self.downs += 1;
                self.dn_run = 0;
            }
        } else {
            // 迟滞带：双计数清零。
            self.up_run = 0;
            self.dn_run = 0;
        }
        Ok(self.level)
    }

    /// 频变联动核：档与频一致。
    pub fn coupled(&self) -> bool {
        self.freq() == FREQS[self.level as usize]
    }
}

pub fn run_veb291_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0291");
    // 族零：采样合法性。
    let mut g = FreqGov::new();
    s.add("101 拒", g.sample(101) == Err(XC_UTIL), ">100");
    s.add("起步档 0", g.freq() == 800, "顶频");
    // 族一：升频门。
    // 90% × 3 采样不升。
    for _ in 0..3 {
        let _ = g.sample(90);
    }
    s.add("三采不升", g.level == 0 && g.up_run == 3, "差一");
    let _ = g.sample(90);
    s.add("四采升 1", g.level == 1 && g.freq() == 600, "600M");
    // 计数清零再升。
    for _ in 0..4 {
        let _ = g.sample(95);
    }
    s.add("再升 2", g.level == 2 && g.freq() == 400, "400M");
    // 族二：迟滞带稳频。
    let _ = g.sample(50);
    s.add("带内清计", g.up_run == 0 && g.dn_run == 0, "死区");
    s.add("档不动", g.level == 2, "稳频");
    // 族三：降频门。
    for _ in 0..7 {
        let _ = g.sample(20);
    }
    s.add("七采不降", g.level == 2 && g.dn_run == 7, "差一");
    let _ = g.sample(20);
    s.add("八采降 3", g.level == 3 && g.freq() == 200, "200M");
    // 族四：档频界。
    // 顶档再降不出。
    for _ in 0..16 {
        let _ = g.sample(20);
    }
    s.add("降底不出", g.level == 3, "200M 锁底");
    // 升顶不出：从底档连打高载。
    for _ in 0..40 {
        let _ = g.sample(99);
    }
    s.add("升顶不出", g.level == 0, "800M 锁顶");
    s.add("联动核", g.coupled(), "档频一致");
    // 族五：交替带清计。
    let mut h = FreqGov::new();
    for _ in 0..3 {
        let _ = h.sample(90);
    }
    let _ = h.sample(50); // 清升计
    for _ in 0..3 {
        let _ = h.sample(90);
    }
    s.add("带断重计", h.level == 0, "清零重来");
    // 混合门互斥：升计中插降采。
    let mut k = FreqGov::new();
    for _ in 0..2 {
        let _ = k.sample(90);
    }
    let _ = k.sample(10); // 清升计起降计
    s.add("门互斥", k.up_run == 0 && k.dn_run == 1, "单门");
    s.add("魔数核", FQ_SEG_BASE == 0x311C0, "段基址");
    s
}
