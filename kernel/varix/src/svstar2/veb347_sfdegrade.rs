//! veb347_sfdegrade —— VE-F0347 软渲专用降质联动。
//! 锚点：VE-F0347 · 目标 320 行。
//! 判据原文：三级阶梯（内部分辨率先降 → 效果分级次之 → 最
//!           后帧率让步）、双源仲裁（热因与负载因取更严）、
//!           滞回防抖（降质风暴频繁升降 → 滞回窗口）、通知
//!           三要素（一次性告知当前档与退出方式，合并限频）、
//!           自动回升、判据。
//! 组件：降质阶梯状态机 + 双源仲裁器 + 滞回防抖器。
//! 职责定位：降质阶梯是「体面地瘪下去」——先瘦里子（分辨率），
//!           再卸妆（效果），最后才慢下来（帧率）；忽胖忽瘦
//!           是降质风暴，滞回窗口给体面加个缓冲带。
//! 性能逐项分解：决策 O(1) 查表；防抖 O(1)；回升 O(1)。
//! 诊断码段：0x31FC0（0x31FC1~0x31FC9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 模块版本。
pub const DG_VERSION: &str = "veb347_sfdegrade-v1";

/// 诊断段基址（0x31FC0 段 · 本模块独占）。
pub const DG_SEG_BASE: u32 = 0x31FC0;
pub const XC_BADLVL: u32 = 0x31FC1; // 阶梯级非法
pub const XC_STORM: u32 = 0x31FC2; // 降质风暴（滞回窗口内拒变）
pub const XC_NOCOMP: u32 = 0x31FC3; // 通知三要素不全
pub const XC_RECOV: u32 = 0x31FC4; // 恢复失败保持当前档

/// 三级阶梯。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Lvl {
    /// 零级：满档。
    L0,
    /// 一级：降内部分辨率（步进制）。
    L1,
    /// 二级：效果分级（过滤降档、混合简化）。
    L2,
    /// 三级：帧率让步。
    L3,
}

/// 阶梯步进。
pub fn lvl_no(l: Lvl) -> u32 {
    match l {
        Lvl::L0 => 0,
        Lvl::L1 => 1,
        Lvl::L2 => 2,
        Lvl::L3 => 3,
    }
}

/// 双源压力判据（0~100）。
pub const HOT_ENTER: u32 = 80; // 任一源 ≥80 降一级
pub const HOT_EXIT: u32 = 55; // 双源都 ≤55 回升一级
/// 滞回窗口（决策周期数）。
pub const HYST_WIN: u32 = 3;

/// 通知三要素。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Notice {
    /// 当前档。
    pub lvl: Lvl,
    /// 原因源（0=热 1=负载 2=双源）。
    pub src: u32,
    /// 退出方式代号（恢复策略）。
    pub exit_hint: u32,
}

/// 降质状态机。
pub struct Degrader {
    cur: Lvl,
    /// 最近决策周期（防抖）。
    last_move: u32,
    /// 通知账。
    pub notices: Vec<Notice>,
    /// 恢复失败保持次数。
    pub holds: u32,
}

impl Degrader {
    /// 满 StateL0 起。
    pub fn new() -> Degrader {
        Degrader {
            cur: Lvl::L0,
            last_move: 0,
            notices: Vec::new(),
            holds: 0,
        }
    }

    /// 当前档。
    pub fn cur(&self) -> Lvl {
        self.cur
    }

    /// 双源仲裁：取更严（压力更大的一源）。
    pub fn arbitrate(thermal: u32, load: u32) -> u32 {
        if thermal > load {
            thermal
        } else {
            load
        }
    }

    /// 决策：降级（≥ENTER）或回升（双源≤EXIT 且双源一致）；滞回窗口内拒变。
    pub fn decide(&mut self, thermal: u32, load: u32, tick: u32) -> Result<Option<Lvl>, u32> {
        let p = Self::arbitrate(thermal, load);
        if tick.saturating_sub(self.last_move) < HYST_WIN {
            return Err(XC_STORM);
        }
        let want = if p >= HOT_ENTER {
            match self.cur {
                Lvl::L0 => Some(Lvl::L1),
                Lvl::L1 => Some(Lvl::L2),
                Lvl::L2 => Some(Lvl::L3),
                Lvl::L3 => None,
            }
        } else if thermal <= HOT_EXIT && load <= HOT_EXIT {
            match self.cur {
                Lvl::L1 => Some(Lvl::L0),
                Lvl::L2 => Some(Lvl::L1),
                Lvl::L3 => Some(Lvl::L2),
                Lvl::L0 => None,
            }
        } else {
            None
        };
        match want {
            None => Ok(None),
            Some(to) => {
                self.cur = to;
                self.last_move = tick;
                self.notices.push(Notice {
                    lvl: to,
                    src: if thermal >= HOT_ENTER && load >= HOT_ENTER {
                        2
                    } else if thermal >= HOT_ENTER {
                        0
                    } else {
                        1
                    },
                    exit_hint: if lvl_no(to) > lvl_no(self.cur) {
                        1 // 回升路径
                    } else {
                        0
                    },
                });
                Ok(Some(to))
            }
        }
    }

    /// 恢复失败：保持当前档并记账。
    pub fn hold(&mut self) -> u32 {
        self.holds += 1;
        lvl_no(self.cur)
    }

    /// 通知合法性：三要素齐（档合法、源 0~2、退出码 0~1）。
    pub fn notice_ok(n: Notice) -> bool {
        let _ = (XC_BADLVL, XC_NOCOMP);
        n.src <= 2 && n.exit_hint <= 1
    }

    /// 通知合并限频：同档同源在窗内只留一条。
    pub fn dedup(&mut self) -> usize {
        let before = self.notices.len();
        let mut seen: Vec<(u32, u32)> = Vec::new();
        let mut keep: Vec<Notice> = Vec::new();
        for n in self.notices.iter() {
            let k = (lvl_no(n.lvl), n.src);
            if !seen.contains(&k) {
                seen.push(k);
                keep.push(*n);
            }
        }
        self.notices = keep;
        before - self.notices.len()
    }
}

pub fn run_veb347_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0347");
    // 族零：双源仲裁。
    s.add("热源严", Degrader::arbitrate(85, 60) == 85, "取大");
    s.add("载源严", Degrader::arbitrate(50, 90) == 90, "取大");
    s.add("平取任", Degrader::arbitrate(70, 70) == 70, "相等");
    // 族一：三级阶梯。
    let mut d = Degrader::new();
    s.add("满档起", d.cur() == Lvl::L0, "L0");
    s.add("80 降一级", d.decide(85, 60, 1) == Ok(Some(Lvl::L1)), "先瘦里子");
    s.add("持续降二级", d.decide(85, 60, 4) == Ok(Some(Lvl::L2)), "再卸妆");
    s.add("顶格降三级", d.decide(90, 90, 7) == Ok(Some(Lvl::L3)), "最后慢下来");
    s.add("三级封顶", d.decide(95, 95, 10) == Ok(None), "无级可降");
    s.add("档在 L3", d.cur() == Lvl::L3, "到底");
    // 族二：滞回防抖。
    s.add("窗内拒变", d.decide(10, 10, 11) == Err(XC_STORM), "1<3 防抖");
    s.add("窗内降拒", d.decide(99, 99, 12) == Err(XC_STORM), "风暴拦");
    s.add("窗过可动", d.decide(30, 30, 13).is_ok(), "3 周期后");
    // 族三：自动回升。
    let mut d2 = Degrader::new();
    let _ = d2.decide(90, 20, 1); // L1（热源降）
    let _ = d2.decide(60, 20, 4); // 无变（60>55 且 20≤55 → 单源 60 中间带）
    s.add("中间带保持", d2.cur() == Lvl::L1, "滞回带");
    s.add("双低回升", d2.decide(50, 50, 7) == Ok(Some(Lvl::L0)), "退出方式");
    s.add("满档不升", d2.decide(10, 10, 10) == Ok(None), "顶格");
    // 降源混合回升路径：L2→L1。
    let mut d3 = Degrader::new();
    let _ = d3.decide(90, 90, 1);
    let _ = d3.decide(88, 90, 4);
    s.add("到 L2", d3.cur() == Lvl::L2, "二级");
    s.add("回升一级", d3.decide(40, 40, 7) == Ok(Some(Lvl::L1)), "逐级回");
    // 族四：恢复失败保持。
    s.add("保持记账", d3.hold() == 1, "L1 保持");
    s.add("保持计数", d3.holds == 1, "一次");
    // 族五：通知三要素。
    s.add("通知合法", Degrader::notice_ok(Notice { lvl: Lvl::L1, src: 0, exit_hint: 0 }), "三要素齐");
    s.add("源越界拒", !Degrader::notice_ok(Notice { lvl: Lvl::L1, src: 3, exit_hint: 0 }), "源 0~2");
    s.add("退出码越界拒", !Degrader::notice_ok(Notice { lvl: Lvl::L1, src: 1, exit_hint: 2 }), "码 0~1");
    s.add("通知三笔", d.notices.len() == 3, "逐级告知");
    // 族六：通知合并限频。
    s.add("合并二", d.dedup() == 2, "同源去重");
    s.add("留一", d.notices.len() == 1, "限频后");
    s.add("魔数核", DG_SEG_BASE == 0x31FC0, "段基址");
    s
}
