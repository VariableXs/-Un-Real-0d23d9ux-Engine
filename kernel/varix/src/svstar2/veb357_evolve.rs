//! veb357_evolve —— VE-F0357 软渲版本演进策略。
//! 锚点：VE-F0357 · 目标 300 行。
//! 判据原文：四步接入（探测 → 内核适配 → 对照验证 → 矩阵
//!           登记，对照验证失败阻断）、公告制（呈现语义类变
//!           更提前一版公告，公告缺失阻断发布）、重调验证
//!           （瓦片参数重调未验证标注实验档）、历史可溯（版
//!           本史追加）、判据。
//! 组件：接入流程状态机 + 公告账 + 重调记录 + 版本史。
//! 职责定位：演进策略是「上新菜的流程」——试菜（对照验证）
//!           不过不许上菜单（接入阻断）；改了摆盘方式（呈现
//!           语义）要提前贴告示（公告制）；谁动过菜谱都记在
//!           账上（历史可溯）。
//! 性能逐项分解：接入 O(内核数)；公告 O(1)；历史 O(1) 追加。
//! 诊断码段：0x32240（0x32241~0x32249 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const EV_VERSION: &str = "veb357_evolve-v1";

/// 诊断段基址（0x32240 段 · 本模块独占）。
pub const EV_SEG_BASE: u32 = 0x32240;
pub const XC_STEP: u32 = 0x32241; // 步序错乱
pub const XC_ADOPT: u32 = 0x32242; // 对照验证失败（接入阻断）
pub const XC_NOANN: u32 = 0x32243; // 公告缺失（阻断发布）
pub const XC_NOVER: u32 = 0x32244; // 版本号非法

/// 接入四步。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum St {
    Probe,
    Adapt,
    Verify,
    Enroll,
}

/// 步序。
pub fn st_no(x: St) -> u32 {
    match x {
        St::Probe => 0,
        St::Adapt => 1,
        St::Verify => 2,
        St::Enroll => 3,
    }
}

/// 接入案。
pub struct Adopt {
    pub isa: u32,
    /// 已完成步掩码。
    mask: u32,
    /// 对照验证结果。
    pub verified: bool,
    /// 登记完成。
    pub enrolled: bool,
}

/// 演进账。
pub struct Evolver {
    cur: Option<Adopt>,
    /// 公告账（版本 → 公告代号）。
    pub anns: Vec<(u32, u32)>,
    /// 重调记录（代际 → 已验证）。
    pub retunes: Vec<(u32, bool)>,
    /// 版本史。
    pub hist: Vec<u32>,
}

impl Evolver {
    /// 空账。
    pub fn new() -> Evolver {
        Evolver {
            cur: None,
            anns: Vec::new(),
            retunes: Vec::new(),
            hist: Vec::new(),
        }
    }

    /// 开接入案。
    pub fn open(&mut self, isa: u32) -> Result<(), u32> {
        if self.cur.is_some() {
            return Err(XC_STEP); // 上一案未结
        }
        self.cur = Some(Adopt {
            isa,
            mask: 0,
            verified: false,
            enrolled: false,
        });
        Ok(())
    }

    /// 走一步：严格顺序。
    pub fn step(&mut self, x: St, verify_ok: bool) -> Result<u32, u32> {
        let a = match self.cur {
            Some(ref mut a) => a,
            None => return Err(XC_STEP),
        };
        let no = st_no(x);
        if a.mask & (1 << no) != 0 {
            return Err(XC_STEP); // 已完成步
        }
        let done = a.mask.count_ones();
        if no != done {
            return Err(XC_STEP); // 跳步
        }
        if x == St::Verify {
            if !verify_ok {
                a.verified = false;
                return Err(XC_ADOPT); // 对照验证失败阻断
            }
            a.verified = true;
        }
        a.mask |= 1 << no;
        if x == St::Enroll {
            a.enrolled = true;
        }
        Ok(a.mask)
    }

    /// 接入完成核。
    pub fn done(&self) -> bool {
        match self.cur {
            Some(ref a) => a.enrolled,
            None => false,
        }
    }

    /// 结案（历史追加）。
    pub fn close(&mut self, ver: u32) -> Result<(), u32> {
        if ver == 0 {
            return Err(XC_NOVER);
        }
        if !self.done() {
            return Err(XC_STEP);
        }
        self.hist.push(ver);
        self.cur = None;
        Ok(())
    }

    /// 公告：呈现语义类变更提前一版公告。
    pub fn announce(&mut self, ver: u32, code: u32) -> Result<(), u32> {
        if ver == 0 || code == 0 {
            return Err(XC_NOANN);
        }
        self.anns.push((ver, code));
        Ok(())
    }

    /// 发布核：变更版本必须有公告。
    pub fn publish_ok(&self, ver: u32) -> Result<(), u32> {
        if self.anns.iter().any(|a| a.0 == ver) {
            Ok(())
        } else {
            Err(XC_NOANN)
        }
    }

    /// 瓦片重调：代际变化时登记；未验证标实验档。
    pub fn retune(&mut self, gen: u32, verified: bool) -> Result<(), u32> {
        if gen == 0 {
            return Err(XC_NOVER);
        }
        if self.retunes.iter().any(|r| r.0 == gen) {
            return Err(XC_STEP); // 同代只调一次
        }
        self.retunes.push((gen, verified));
        Ok(())
    }

    /// 重调态：未验证 → 实验档。
    pub fn retune_exp(&self, gen: u32) -> bool {
        match self.retunes.iter().find(|r| r.0 == gen) {
            Some(r) => !r.1,
            None => false,
        }
    }
}

pub fn run_veb357_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0357");
    // 族零：四步接入。
    let mut e = Evolver::new();
    s.add("无案走步拒", e.step(St::Probe, true) == Err(XC_STEP), "先立案");
    s.add("立案过", e.open(6).is_ok(), "RVV 案");
    s.add("再立案拒", e.open(5) == Err(XC_STEP), "一案未结");
    s.add("跳步拒", e.step(St::Verify, true) == Err(XC_STEP), "先探测适配");
    s.add("探测过", e.step(St::Probe, true) == Ok(1), "步一");
    s.add("适配过", e.step(St::Adapt, true) == Ok(3), "步二");
    // 族一：对照验证阻断。
    s.add("验证败阻断", e.step(St::Verify, false) == Err(XC_ADOPT), "试菜不过");
    s.add("未登记", !e.done(), "接入悬置");
    // 验证重来（显性重试语义：步未完成可重试）。
    s.add("重试过", e.step(St::Verify, true) == Ok(7), "试菜通过");
    s.add("登记过", e.step(St::Enroll, true) == Ok(15), "步四");
    s.add("案结", e.done(), "矩阵登记完");
    // 族二：结案与版本史。
    s.add("零版结案拒", e.close(0) == Err(XC_NOVER), "无版");
    s.add("结案过", e.close(8).is_ok(), "v8");
    s.add("史一笔", e.hist == vec![8u32], "可溯");
    s.add("案清可再开", e.open(5).is_ok(), "AVX512 新案");
    // 族三：公告制。
    s.add("未公告发布拒", e.publish_ok(9) == Err(XC_NOANN), "先公告");
    s.add("公告过", e.announce(9, 7).is_ok(), "提前一版");
    s.add("发布过", e.publish_ok(9).is_ok(), "公告背书");
    s.add("零公告拒", e.announce(10, 0) == Err(XC_NOANN), "无代号");
    s.add("公告账一", e.anns.len() == 1, "在账");
    // 族四：重调验证。
    s.add("零代拒", e.retune(0, true) == Err(XC_NOVER), "无代");
    s.add("重调过", e.retune(3, false).is_ok(), "代 3 未验");
    s.add("实验档标注", e.retune_exp(3), "未验 = 实验");
    s.add("同代重调拒", e.retune(3, true) == Err(XC_STEP), "一次");
    s.add("代 4 已验", e.retune(4, true).is_ok() && !e.retune_exp(4), "正常档");
    s.add("查无代非实验", !e.retune_exp(9), "没调过");
    // 族五：多版本史。
    let _ = e.step(St::Probe, true);
    let _ = e.step(St::Adapt, true);
    let _ = e.step(St::Verify, true);
    let _ = e.step(St::Enroll, true);
    let _ = e.close(9);
    s.add("史两笔", e.hist == vec![8u32, 9u32], "版本演进可溯");
    s.add("魔数核", EV_SEG_BASE == 0x32240, "段基址");
    s
}
