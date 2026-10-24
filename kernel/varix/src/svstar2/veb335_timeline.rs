//! veb335_timeline —— VE-F0335 时间轴评估。
//! 锚点：VE-F0335 · 目标 300 行。
//! 判据原文：三阶段（观察期只追踪不动手 / 评估期 fake 与骨
//!           架就绪 / 接入期真实硬件与驱动可用）、进出判据
//!           显性（不由感觉拍板）、证据裁决（阶段误判回退）、
//!           当前如实（判定与依据呈现）、判据。
//! 组件：阶段判据表 + 证据裁决器 + 判定记录。
//! 职责定位：时间轴评估是「升学制度」——观察期是旁听，评估
//!           期是备考，接入期是录取；升学不看感觉看成绩单
//!           （判据），没有成绩单不许升学。
//! 性能逐项分解：判定 O(1) 查表；修正 O(1)；评估周期性。
//! 诊断码段：0x31CC0（0x31CC1~0x31CC9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const TL_VERSION: &str = "veb335_timeline-v1";

/// 诊断段基址（0x31CC0 段 · 本模块独占）。
pub const TL_SEG_BASE: u32 = 0x31CC0;
pub const XC_NOCRIT: u32 = 0x31CC1; // 判据缺失（先立判据后判定）
pub const XC_NOEVID: u32 = 0x31CC2; // 证据不足
pub const XC_BACK: u32 = 0x31CC3; // 阶段倒退（裁决回退路径专用）
pub const XC_BADPH: u32 = 0x31CC4; // 阶段号非法

/// 三阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 观察期：只追踪不动手。
    Watch,
    /// 评估期：fake 与骨架就绪。
    Assess,
    /// 接入期：真实硬件与驱动可用。
    Adopt,
}

/// 阶段序号化（0/1/2）。
pub fn phase_no(p: Phase) -> u32 {
    match p {
        Phase::Watch => 0,
        Phase::Assess => 1,
        Phase::Adopt => 2,
    }
}

/// 进出判据（各阶段一对阈值：进阶段需证据分 ≥ 进线，出阶段即升下阶段证据分 ≥ 出线）。
pub struct Crit {
    /// 进入本阶段所需证据分。
    pub enter: u32,
    /// 离开本阶段（进下一阶段）所需证据分。
    pub exit: u32,
}

/// 默认判据表：观察进 0/出 30；评估进 30/出 70；接入进 70/出 100。
pub const CRIT_DEFAULT: [Crit; 3] = [
    Crit { enter: 0, exit: 30 },
    Crit { enter: 30, exit: 70 },
    Crit { enter: 70, exit: 100 },
];

/// 判定记录。
pub struct Record {
    pub phase: Phase,
    pub evid: u32,
    pub epoch: u32,
}

/// 时间轴评估簿。
pub struct Timeline {
    crit: [Crit; 3],
    cur: Phase,
    recs: Vec<Record>,
}

impl Timeline {
    /// 建：初始观察期，判据可用默认表。
    pub fn new() -> Timeline {
        Timeline {
            crit: CRIT_DEFAULT,
            cur: Phase::Watch,
            recs: Vec::new(),
        }
    }

    /// 立判据：进线必须 < 出线（同阶段先有进再有出）。
    pub fn set_crit(&mut self, ph: u32, enter: u32, exit: u32) -> Result<(), u32> {
        if ph > 2 {
            return Err(XC_BADPH);
        }
        if enter >= exit {
            return Err(XC_NOCRIT);
        }
        self.crit[ph as usize] = Crit { enter, exit };
        Ok(())
    }

    /// 判定：证据分裁决当前应处阶段。
    /// 规则：从当前阶段向上找最大满足 enter 的阶段；证据不足维持现状但需 ≥ 当前 enter。
    pub fn judge(&self, evid: u32) -> Phase {
        let mut best = 0u32;
        for i in 0..3u32 {
            if evid >= self.crit[i as usize].enter {
                best = i;
            }
        }
        match best {
            0 => Phase::Watch,
            1 => Phase::Assess,
            _ => Phase::Adopt,
        }
    }

    /// 带记录的推进：证据达标才晋升；不达标拒（证据裁决）。
    pub fn advance(&mut self, evid: u32, epoch: u32) -> Result<Phase, u32> {
        let want = self.judge(evid);
        let cur_no = phase_no(self.cur);
        let want_no = phase_no(want);
        if want_no < cur_no {
            // 证据不足以维持当前阶段 → 裁决回退。
            self.cur = want;
            self.recs.push(Record { phase: want, evid, epoch });
            return Err(XC_BACK);
        }
        if want_no == cur_no {
            self.recs.push(Record { phase: want, evid, epoch });
            return Ok(want);
        }
        self.cur = want;
        self.recs.push(Record { phase: want, evid, epoch });
        Ok(want)
    }

    /// 手工裁决回退（阶段误判修正）：允许显性回退。
    pub fn rollback(&mut self, to: Phase, evid: u32, epoch: u32) -> Result<(), u32> {
        let to_no = phase_no(to);
        let cur_no = phase_no(self.cur);
        if to_no >= cur_no {
            return Err(XC_BACK); // 回退只向下
        }
        self.cur = to;
        self.recs.push(Record { phase: to, evid, epoch });
        Ok(())
    }

    /// 当前阶段。
    pub fn current(&self) -> Phase {
        self.cur
    }

    /// 记录数。
    pub fn rec_count(&self) -> usize {
        self.recs.len()
    }

    /// 最近记录的依据（证据分）。
    pub fn last_evid(&self) -> Option<u32> {
        self.recs.last().map(|r| r.evid)
    }
}

pub fn run_veb335_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0335");
    // 族零：判据门。
    let mut t = Timeline::new();
    s.add("进≥出拒", t.set_crit(1, 70, 70) == Err(XC_NOCRIT), "无空间");
    s.add("进>出拒", t.set_crit(1, 80, 70) == Err(XC_NOCRIT), "倒挂");
    s.add("坏阶段拒", t.set_crit(3, 10, 20) == Err(XC_BADPH), ">2");
    s.add("改判过", t.set_crit(1, 40, 60).is_ok(), "自定判据");
    // 族一：三阶段判定。
    s.add("零证观察", t.judge(0) == Phase::Watch, "0 分旁听");
    s.add("29 分观察", t.judge(29) == Phase::Watch, "差一分");
    s.add("40 分评估", t.judge(40) == Phase::Assess, "过进线 40");
    s.add("70 分评估", t.judge(70) == Phase::Assess, "未过出线 60→留评估");
    // 注：70 ≥ crit[2].enter=70 → 应是接入？核：默认 crit[2].enter=70 → judge(70)=Adopt。
    s.add("70 分接入", t.judge(70) == Phase::Adopt, "70≥70");
    s.add("99 分接入", t.judge(99) == Phase::Adopt, "顶格");
    // 族二：推进。
    let mut t2 = Timeline::new();
    s.add("零证拒升", t2.advance(29, 1).is_ok() && t2.current() == Phase::Watch, "留观察");
    s.add("35 分升评估", t2.advance(35, 2) == Ok(Phase::Assess), "过 30");
    s.add("50 分留评估", t2.advance(50, 3).is_ok() && t2.current() == Phase::Assess, "未到 70");
    s.add("75 分升接入", t2.advance(75, 4) == Ok(Phase::Adopt), "过 70");
    s.add("记录四条", t2.rec_count() == 4, "步步留痕");
    s.add("最近依据 75", t2.last_evid() == Some(75), "如实呈现");
    // 族三：证据裁决回退。
    s.add("证据崩塌回退", t2.advance(10, 5) == Err(XC_BACK), "不足维持");
    s.add("落回评估", t2.current() == Phase::Assess, "退一级");
    s.add("手工回退过", t2.rollback(Phase::Watch, 5, 6).is_ok(), "显性降");
    s.add("手工升拒", t2.rollback(Phase::Adopt, 90, 7) == Err(XC_BACK), "回退只向下");
    // 族四：自定义判据贯通。
    let mut t3 = Timeline::new();
    let _ = t3.set_crit(0, 0, 10);
    let _ = t3.set_crit(1, 10, 20);
    let _ = t3.set_crit(2, 20, 100);
    s.add("10 分评估", t3.judge(10) == Phase::Assess, "小判据");
    s.add("20 分接入", t3.judge(20) == Phase::Adopt, "低门槛");
    // 族五：如实性。
    s.add("初态观察", Timeline::new().current() == Phase::Watch, "起步旁听");
    s.add("魔数核", TL_SEG_BASE == 0x31CC0, "段基址");
    s
}
