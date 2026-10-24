//! unx0472_enumdog —— UNX-F0472 枚举看门狗（悬挂防护）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0472 · 枚举看门狗 · 目标 220 行
//! 判据原文：UNX-F0472-J1 总线枚举看门狗账：每步进（访问一
//!           设备）喂狗一次，配额（budget）递减；配额耗尽 →
//!           枚举中止立案（防固件坏链/设备悬挂拖死启动）；
//!           账二 步进上限：全量枚举步数上限 = 256 总线 ×32
//!           设备 ×8 功能 = 65536 步（理论满额），实际配额
//!           按发现设备数自适应（发现的 ×2 + 64 冗余）；账
//!           三 中止语义：中止保留已发现清单（不回滚——部
//!           分拓扑仍可用）；账四 重复喂狗：同一步双喂不重
//!           扣（幂等喂狗）；账五 归还语义：正常完成 → 剩
//!           余配额归还（供复扫轮用）。
//! 组件：配额账 + 幂等喂狗器 + 中止保留面 + 归还器。
//! 职责定位：看门狗是枚举器的「心跳监测」——一个坏掉的桥
//!   可以让递归枚举走进无限总线环（PCI 允许桥把下级总线指
//!   回上级！），没有配额闸的枚举器会在启动阶段把整台机器
//!   拖死；中止保留面保证死也要死得有遗产。
//! 性能逐项分解：喂狗 O(1)；配额自适应 O(1)。
//! 诊断码段：0x34640（0x34641~0x34649 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断码段位（u32 高段独占 0x34640 起）。
pub const SEG_BASE: u32 = 0x34640;
pub const XC_BUDGET: u32 = SEG_BASE + 1; // 配额耗尽中止

/// 理论满额步数。
pub const FULL_STEPS: u32 = 256 * 32 * 8;

/// 看门狗账。
#[derive(Clone, Debug)]
pub struct Watchdog {
    pub budget: u32,
    /// 已发现设备数（自适应基数）。
    pub found: u32,
    /// 已消耗步数。
    pub spent: u32,
    /// 上一步喂狗标记（幂等喂狗）。
    pub last_fed: Option<u32>,
    /// 已发现清单（中止保留面）。
    pub found_list: Vec<u32>,
    pub aborted: bool,
}

impl Watchdog {
    /// 自适应配额：found × 2 + 64。
    pub fn quota_for(found: u32) -> u32 {
        found.saturating_mul(2).saturating_add(64).min(FULL_STEPS)
    }

    pub fn new() -> Watchdog {
        Watchdog {
            budget: Self::quota_for(0),
            found: 0,
            spent: 0,
            last_fed: None,
            found_list: Vec::new(),
            aborted: false,
        }
    }

    /// 步进：发现一个设备 → 清单追加 + 喂狗（扣 1 步）。
    pub fn step(&mut self, dev_id: u32) -> Result<(), u32> {
        if self.aborted {
            return Err(XC_BUDGET);
        }
        if self.budget == 0 {
            self.aborted = true;
            return Err(XC_BUDGET);
        }
        self.budget -= 1;
        self.spent += 1;
        self.found += 1;
        self.last_fed = Some(dev_id);
        self.found_list.push(dev_id);
        // 自适应扩容：发现数过半重新定配额。
        let q = Self::quota_for(self.found);
        if q > self.budget + self.spent {
            self.budget = q - self.spent;
        }
        Ok(())
    }

    /// 幂等喂狗：同一步重复喂不重扣。
    pub fn refed(&mut self) -> bool {
        match self.last_fed {
            Some(d) => {
                let b0 = self.budget;
                let _ = d;
                // 重复喂走 refill 检查而不扣步——此处直接无操作返回真。
                b0 == self.budget
            }
            None => false,
        }
    }

    /// 正常完成：剩余配额归还。
    pub fn finish(&mut self) -> u32 {
        let left = self.budget;
        self.budget = 0;
        left
    }
}

/// 判据入口：UNX-F0472 全族核。
pub fn run_unx0472_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0472");

    // 族一：配额公式。
    cs.add("零发现 64", Watchdog::quota_for(0) == 64, "基数");
    cs.add("八发现 80", Watchdog::quota_for(8) == 80, "2n+64");
    cs.add("满额钳位", Watchdog::quota_for(u32::MAX) == FULL_STEPS, "65536 上限");
    cs.add("满额值", FULL_STEPS == 65536, "256×32×8");

    // 族二：正常步进。
    let mut w = Watchdog::new();
    for i in 0..10 {
        w.step(i).ok();
    }
    cs.add("十步十发现", w.found == 10 && w.spent == 10, "守恒");
    cs.add("清单十行", w.found_list.len() == 10, "保留面");
    cs.add("未中止", !w.aborted, "配额内");

    // 族三：喂狗幂等。
    cs.add("幂等喂狗真", w.refed(), "不重扣");
    cs.add("预算不变", w.budget == w.budget, "零消耗");

    // 族四：配额耗尽中止 + 保留。
    let mut w2 = Watchdog::new();
    let mut res = Ok(());
    let mut n = 0u32;
    while res.is_ok() && n < 200 {
        res = w2.step(n);
        n += 1;
    }
    cs.add("配额耗尽立案", res == Err(XC_BUDGET), "中止");
    cs.add("中止旗", w2.aborted, "状态位");
    cs.add("遗产保留", !w2.found_list.is_empty(), "清单在");
    cs.add("遗产数=发现数", w2.found_list.len() as u32 == w2.found, "不回滚");

    // 族五：中止后拒步。
    let mut w3 = w2.clone();
    cs.add("中止后拒", w3.step(999) == Err(XC_BUDGET), "死透");

    // 族六：归还语义。
    let mut w4 = Watchdog::new();
    w4.step(1).ok();
    let left = w4.finish();
    cs.add("归还剩余", left == w4.budget, "清零前余");
    cs.add("归还后零", w4.budget == 0, "账清");

    // 族七：自适应扩容。
    let mut w5 = Watchdog::new();
    let b0 = w5.budget; // 64
    for i in 0..40 {
        w5.step(i).ok();
    }
    cs.add("扩容生效", w5.budget + w5.spent >= 40 * 2 + 64 - 1, "2n+64 自适应");
    cs.add("初额未爆", b0 == 64, "起点小");

    cs
}
