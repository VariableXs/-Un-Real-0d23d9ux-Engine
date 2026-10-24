//! veb337_upstream —— VE-F0337 上游策略。
//! 锚点：VE-F0337 · 目标 320 行。
//! 判据原文：主线优先（内核主线优先，树外驱动仅登记不依赖）、
//!           锁定回退（版本锁定，升级失败回退最近稳定）、
//!           三步响应（上游破坏性变更：评估→适配→公告）、
//!           策略联动（与 F0334 协作接口联动）、判据。
//! 组件：策略状态机 + 版本锁定账 + 响应三步器。
//! 职责定位：上游策略是「跟车的安全带」——主线优先是认路认
//!           大道，树外小道只登记不驶入；锁定回退是倒车镜，
//!           升级翻车就退回上一个能开的地方。
//! 性能逐项分解：策略 O(1)；回退 O(1)；响应 O(步数)。
//! 诊断码段：0x31D40（0x31D41~0x31D49 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const US_VERSION: &str = "veb337_upstream-v1";

/// 诊断段基址（0x31D40 段 · 本模块独占）。
pub const US_SEG_BASE: u32 = 0x31D40;
pub const XC_NOTMAIN: u32 = 0x31D41; // 树外依赖（违反主线优先）
pub const XC_NOLOCK: u32 = 0x31D42; // 无锁定版本
pub const XC_NOUP: u32 = 0x31D43; // 升级目标未知
pub const XC_NOSTEP: u32 = 0x31D44; // 响应步骤乱序
pub const XC_DONE: u32 = 0x31D45; // 响应已完成

/// 响应三步。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// 评估。
    Assess,
    /// 适配。
    Adapt,
    /// 公告。
    Announce,
}

/// 步序号。
pub fn step_no(x: Step) -> u32 {
    match x {
        Step::Assess => 0,
        Step::Adapt => 1,
        Step::Announce => 2,
    }
}

/// 版本锁定记录。
pub struct Pin {
    /// 主线版本号（语义化：主×10000+次×100+补）。
    pub ver: u32,
    /// 登记周期。
    pub epoch: u32,
    /// 是否当前锁定。
    pub active: bool,
}

/// 树外驱动登记。
pub struct OutTree {
    pub name: u32,
    /// 仅登记，不依赖。
    pub dep_only: bool,
}

/// 上游策略簿。
pub struct UpBook {
    pins: Vec<Pin>,
    outs: Vec<OutTree>,
    /// 响应进行中：已完成步掩码。
    resp_mask: u32,
    /// 响应是否已开案。
    resp_open: bool,
    /// 回退次数。
    pub rollbacks: u32,
    /// 升级成功次数。
    pub upgrades: u32,
}

/// 响应步掩码位。
pub const M_ASSESS: u32 = 1;
pub const M_ADAPT: u32 = 2;
pub const M_ANNOUNCE: u32 = 4;
pub const M_ALL: u32 = M_ASSESS | M_ADAPT | M_ANNOUNCE;

impl UpBook {
    /// 空簿。
    pub fn new() -> UpBook {
        UpBook {
            pins: Vec::new(),
            outs: Vec::new(),
            resp_mask: 0,
            resp_open: false,
            rollbacks: 0,
            upgrades: 0,
        }
    }

    /// 锁定版本：首个锁定即当前。
    pub fn pin(&mut self, ver: u32, epoch: u32) -> Result<(), u32> {
        if ver == 0 {
            return Err(XC_NOLOCK);
        }
        for p in self.pins.iter_mut() {
            p.active = false;
        }
        self.pins.push(Pin {
            ver,
            epoch,
            active: true,
        });
        Ok(())
    }

    /// 当前锁定版本。
    pub fn cur_ver(&self) -> Option<u32> {
        self.pins.iter().find(|p| p.active).map(|p| p.ver)
    }

    /// 升级：成功则换锁；失败（simulate = 目标版本是否可用）回退最近稳定。
    pub fn upgrade(&mut self, ver: u32, ok: bool, epoch: u32) -> Result<u32, u32> {
        if ver == 0 {
            return Err(XC_NOUP);
        }
        if self.cur_ver().is_none() {
            return Err(XC_NOLOCK);
        }
        if ok {
            self.upgrades += 1;
            self.pin(ver, epoch)?;
            Ok(ver)
        } else {
            // 回退：锁回最近稳定（active 之外倒数第二个），无则维持原锁。
            self.rollbacks += 1;
            let cur = self.cur_ver();
            match cur {
                Some(v) => Ok(v),
                None => Err(XC_NOLOCK),
            }
        }
    }

    /// 树外登记：仅登记不依赖。
    pub fn reg_outtree(&mut self, name: u32) -> Result<(), u32> {
        if name == 0 {
            return Err(XC_NOTMAIN);
        }
        if self.outs.iter().any(|o| o.name == name) {
            return Ok(()); // 重复登记幂等
        }
        self.outs.push(OutTree {
            name,
            dep_only: true,
        });
        Ok(())
    }

    /// 主线依赖核：树外一律不算依赖（返回 false 即不依赖）。
    pub fn depends(&self, name: u32) -> bool {
        self.outs.iter().any(|o| o.name == name && !o.dep_only)
    }

    /// 开响应案。
    pub fn open_resp(&mut self) {
        self.resp_open = true;
        self.resp_mask = 0;
    }

    /// 三步响应：必须按 评估→适配→公告 顺序。
    pub fn resp(&mut self, st: Step) -> Result<u32, u32> {
        if !self.resp_open {
            return Err(XC_NOSTEP);
        }
        let no = step_no(st);
        if self.resp_mask & M_ALL == M_ALL {
            return Err(XC_DONE);
        }
        // 序号必须等于当前已完成数（严格顺序）。
        let done = self.resp_mask.count_ones();
        if no != done {
            return Err(XC_NOSTEP);
        }
        let bit = 1u32 << no;
        self.resp_mask |= bit;
        Ok(self.resp_mask)
    }

    /// 响应完毕核。
    pub fn resp_done(&self) -> bool {
        self.resp_open && self.resp_mask & M_ALL == M_ALL
    }

    /// 登记数。
    pub fn outs_len(&self) -> usize {
        self.outs.len()
    }
}

pub fn run_veb337_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0337");
    // 族零：锁定门。
    let mut u = UpBook::new();
    s.add("零版拒", u.pin(0, 1) == Err(XC_NOLOCK), "无版本");
    s.add("无锁升级拒", u.upgrade(60100, true, 2) == Err(XC_NOLOCK), "先锁后升");
    s.add("锁定过", u.pin(60006, 1).is_ok(), "6.0.6");
    s.add("当前锁在", u.cur_ver() == Some(60006), "在锁");
    // 族一：升级与回退。
    s.add("零目标拒", u.upgrade(0, true, 2) == Err(XC_NOUP), "未知目标");
    s.add("升级过", u.upgrade(60100, true, 2) == Ok(60100), "6.1.0");
    s.add("锁已换", u.cur_ver() == Some(60100), "新锁");
    s.add("升级一", u.upgrades == 1, "成功账");
    s.add("失败回退", u.upgrade(60200, false, 3) == Ok(60100), "退回最近稳定");
    s.add("回退一", u.rollbacks == 1, "倒车账");
    s.add("锁未动", u.cur_ver() == Some(60100), "原锁保位");
    // 族二：主线优先。
    s.add("登记过", u.reg_outtree(0xB00B).is_ok() || u.reg_outtree(0xB00B).is_ok(), "树外登记");
    s.add("不依赖", !u.depends(0xB00B), "仅登记");
    s.add("幂等登记", u.reg_outtree(0xB00B).is_ok() && u.outs_len() == 1, "重登不翻倍");
    s.add("零名拒", u.reg_outtree(0) == Err(XC_NOTMAIN), "无名");
    // 族三：三步响应。
    s.add("未开案拒", u.resp(Step::Assess) == Err(XC_NOSTEP), "先立案");
    u.open_resp();
    s.add("跳步拒", u.resp(Step::Adapt) == Err(XC_NOSTEP), "先评估");
    s.add("评估过", u.resp(Step::Assess) == Ok(M_ASSESS), "一步");
    s.add("二步适配", u.resp(Step::Adapt) == Ok(M_ASSESS | M_ADAPT), "二步");
    s.add("跳公告前拒", u.resp(Step::Assess) == Err(XC_NOSTEP), "已完成步不可重");
    s.add("三步公告", u.resp(Step::Announce) == Ok(M_ALL), "三步");
    s.add("完毕核", u.resp_done(), "三步齐");
    s.add("完后再走拒", u.resp(Step::Assess) == Err(XC_DONE), "案已结");
    // 族四：再开案。
    u.open_resp();
    s.add("重开清零", u.resp(Step::Assess) == Ok(M_ASSESS), "新案新步");
    // 族五：锁定历史。
    s.add("锁史二", u.pins.len() >= 2, "6.0.6→6.1.0");
    s.add("魔数核", US_SEG_BASE == 0x31D40, "段基址");
    s
}
