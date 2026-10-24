//! veb339_groupcons —— VE-F0339 组内一致性骨架。
//! 锚点：VE-F0339 · 目标 300 行。
//! 判据原文：全链贯通（识别→接入→软渲渲染→调试通道四环）、
//!           断言序定义、首错定位（贯通失败定位首个不一致
//!           环节）、N/A 显性（全链 N/A 当环境缺格）、判据。
//! 组件：链式用例结构 + 断言序 + 首错定位器。
//! 职责定位：一致性骨架是「流水线的质检点」——四个环逐环过
//!           关卡，坏在哪一环就报哪一环，不把下游的锅甩给
//!           上游，也不许带病闯关。
//! 性能逐项分解：用例 O(链路长)；定位 O(1) 断言序。
//! 诊断码段：0x31DC0（0x31DC1~0x31DC9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const GC_VERSION: &str = "veb339_groupcons-v1";

/// 诊断段基址（0x31DC0 段 · 本模块独占）。
pub const GC_SEG_BASE: u32 = 0x31DC0;
pub const XC_SEQ: u32 = 0x31DC1; // 断言序错乱（跳环）
pub const XC_CLOSED: u32 = 0x31DC2; // 链已断（断环后续跑）
pub const XC_BADLINK: u32 = 0x31DC3; // 环号非法
pub const XC_OPEN: u32 = 0x31DC4; // 链未断再报错

/// 四环。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    /// 识别。
    Identify,
    /// 接入。
    Attach,
    /// 软渲渲染。
    Soft,
    /// 调试通道。
    Debug,
}

/// 环号。
pub fn link_no(l: Link) -> u32 {
    match l {
        Link::Identify => 0,
        Link::Attach => 1,
        Link::Soft => 2,
        Link::Debug => 3,
    }
}

/// 环结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkRes {
    /// 通过。
    Pass,
    /// 失败。
    Fail,
    /// 环境缺格（N/A 显性）。
    NA,
}

/// 链式用例。
pub struct ChainCase {
    /// 环结果（四环序）。
    links: [Option<LinkRes>; 4],
    /// 已跑环数（严格顺序游标）。
    cursor: u32,
    /// 首错环（None = 无错或未跑完）。
    pub first_fail: Option<u32>,
    /// 链是否已断。
    pub broken: bool,
    /// 全链 N/A。
    pub all_na: bool,
}

impl ChainCase {
    /// 新链：全空、游标零。
    pub fn new() -> ChainCase {
        ChainCase {
            links: [None; 4],
            cursor: 0,
            first_fail: None,
            broken: false,
            all_na: false,
        }
    }

    /// 跑一环：必须按序；链断后续跑拒；结果如实记账。
    pub fn step(&mut self, want: Link, res: LinkRes) -> Result<(), u32> {
        let no = link_no(want);
        if self.broken {
            return Err(XC_CLOSED);
        }
        if no != self.cursor {
            return Err(XC_SEQ);
        }
        self.links[no as usize] = Some(res);
        self.cursor += 1;
        if res == LinkRes::Fail {
            self.first_fail = Some(no);
            self.broken = true;
        }
        if self.cursor == 4 {
            self.all_na = self.links.iter().all(|x| *x == Some(LinkRes::NA));
        }
        Ok(())
    }

    /// 首错定位：失败环号；全链贯通返回 None。
    pub fn locate(&self) -> Option<u32> {
        self.first_fail
    }

    /// 贯通核：四环皆 Pass。
    pub fn through(&self) -> bool {
        self.links.iter().all(|x| *x == Some(LinkRes::Pass))
    }

    /// N/A 显性：全链 N/A → 环境缺格（不算失败）。
    pub fn na_env(&self) -> bool {
        self.cursor == 4 && self.all_na
    }

    /// 判定：贯通 → 1；全 N/A → 0；首错 → -1；未跑完 → -2。
    pub fn verdict(&self) -> i8 {
        if self.through() {
            return 1;
        }
        if self.broken {
            return -1;
        }
        if self.cursor == 4 && self.all_na {
            return 0;
        }
        -2
    }

    /// 已跑环数。
    pub fn done(&self) -> u32 {
        self.cursor
    }
}

/// 首错定位器：给定断言序（四环的期望结果序列），返回首个不一致环。
pub fn first_mismatch(want: &[LinkRes; 4], got: &[LinkRes; 4]) -> Option<u32> {
    for i in 0..4u32 {
        if want[i as usize] != got[i as usize] {
            return Some(i);
        }
    }
    None
}

pub fn run_veb339_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0339");
    // 族零：断言序。
    let mut c = ChainCase::new();
    s.add("跳环拒", c.step(Link::Attach, LinkRes::Pass) == Err(XC_SEQ), "0 号未跑");
    s.add("识别过", c.step(Link::Identify, LinkRes::Pass).is_ok(), "环 0");
    s.add("再识别拒", c.step(Link::Identify, LinkRes::Pass) == Err(XC_SEQ), "不可重跑");
    // 族一：全链贯通。
    let mut c2 = ChainCase::new();
    let _ = c2.step(Link::Identify, LinkRes::Pass);
    let _ = c2.step(Link::Attach, LinkRes::Pass);
    let _ = c2.step(Link::Soft, LinkRes::Pass);
    s.add("四环过", c2.step(Link::Debug, LinkRes::Pass).is_ok(), "环 3");
    s.add("贯通", c2.through(), "四绿");
    s.add("无首错", c2.locate().is_none(), "定位空");
    s.add("判一", c2.verdict() == 1, "全过");
    s.add("跑满", c2.done() == 4, "游标 4");
    // 族二：首错定位。
    let mut c3 = ChainCase::new();
    let _ = c3.step(Link::Identify, LinkRes::Pass);
    let _ = c3.step(Link::Attach, LinkRes::Fail);
    s.add("链断拒续", c3.step(Link::Soft, LinkRes::Pass) == Err(XC_CLOSED), "带病闯关");
    s.add("首错环一", c3.locate() == Some(1), "接入环坏");
    s.add("判负一", c3.verdict() == -1, "断了");
    s.add("跑二", c3.done() == 2, "游标停在断处");
    // 更上游坏 → 定位环 0。
    let mut c4 = ChainCase::new();
    let _ = c4.step(Link::Identify, LinkRes::Fail);
    s.add("首错环零", c4.locate() == Some(0), "识别环坏");
    // 族三：N/A 显性。
    let mut c5 = ChainCase::new();
    let _ = c5.step(Link::Identify, LinkRes::NA);
    let _ = c5.step(Link::Attach, LinkRes::NA);
    let _ = c5.step(Link::Soft, LinkRes::NA);
    let _ = c5.step(Link::Debug, LinkRes::NA);
    s.add("全 NA 显性", c5.na_env(), "环境缺格");
    s.add("NA 判零", c5.verdict() == 0, "不算失败");
    s.add("NA 非贯通", !c5.through(), "N/A 不是过");
    // 半 N/A 不算全缺格。
    let mut c6 = ChainCase::new();
    let _ = c6.step(Link::Identify, LinkRes::Pass);
    let _ = c6.step(Link::Attach, LinkRes::NA);
    let _ = c6.step(Link::Soft, LinkRes::NA);
    let _ = c6.step(Link::Debug, LinkRes::NA);
    s.add("半 NA 非全缺", !c6.na_env(), "有一环真跑");
    s.add("半 NA 判负二", c6.verdict() == -2, "未断未过");
    // 族四：首错定位器。
    let want = [LinkRes::Pass, LinkRes::Pass, LinkRes::Pass, LinkRes::Pass];
    let got = [LinkRes::Pass, LinkRes::Fail, LinkRes::Pass, LinkRes::Fail];
    s.add("对拍环一", first_mismatch(&want, &got) == Some(1), "第一个不一致");
    let got2 = [LinkRes::Pass, LinkRes::Pass, LinkRes::Pass, LinkRes::Pass];
    s.add("对拍全合", first_mismatch(&want, &got2).is_none(), "无差异");
    let got3 = [LinkRes::Fail, LinkRes::Fail, LinkRes::Fail, LinkRes::Fail];
    s.add("对拍环零", first_mismatch(&want, &got3) == Some(0), "首环即坏");
    // 族五：零环链。
    let c7 = ChainCase::new();
    s.add("空链未跑", c7.done() == 0, "游标零");
    s.add("空链判负二", c7.verdict() == -2, "没跑不算");
    s.add("魔数核", GC_SEG_BASE == 0x31DC0, "段基址");
    s
}
