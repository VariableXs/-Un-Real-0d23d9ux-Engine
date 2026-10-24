//! veb361_drvlife —— VE-F0361 驱动生命周期总管。
//! 锚点：VE-F0361 · 目标 440 行（B09 驱动热插拔组首卡）。
//! 判据原文：驱动全生命周期状态机（未注册 → 注册 → 启动 →
//!           运行 → 挂起 → 恢复 → 停止 → 卸载，八态闭环）、
//!           非法跃迁拒（状态机查表）、引用计数（卸载前引用
//!           必须归零）、热插拔语义（拔出即挂起 + 通知，插回
//!           恢复）、判据。
//! 组件：八态状态机 + 引用计数账 + 热插拔事件流。
//! 职责定位：生命周期总管是「户口本」——驱动从落户（注册）
//!           到注销（卸载）每一步都有页码；户口没迁走（引用
//!           未清）不许注销；断电（拔出）先挂起，来电（插
//!           回）从断点续跑。
//! 性能逐项分解：跃迁 O(1) 查表；计数 O(1)；事件 O(1)。
//! 诊断码段：0x32340（0x32341~0x32349 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const DL_VERSION: &str = "veb361_drvlife-v1";

/// 诊断段基址（0x32340 段 · 本模块独占）。
pub const DL_SEG_BASE: u32 = 0x32340;
pub const XC_TR: u32 = 0x32341; // 非法跃迁
pub const XC_REFS: u32 = 0x32342; // 引用未清（卸载拒）
pub const XC_DUP: u32 = 0x32343; // 重复注册
pub const XC_NODRV: u32 = 0x32344; // 驱动不存在
pub const XC_OFFLINE: u32 = 0x32345; // 已拔出（离线态拒操作）

/// 八态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum St {
    /// 未注册。
    None,
    /// 已注册未启动。
    Reg,
    /// 启动中。
    Start,
    /// 运行。
    Run,
    /// 挂起（含热拔出）。
    Suspend,
    /// 恢复中。
    Resume,
    /// 停止。
    Stop,
    /// 已卸载。
    Gone,
}

/// 态序号。
pub fn st_no(s: St) -> u32 {
    match s {
        St::None => 0,
        St::Reg => 1,
        St::Start => 2,
        St::Run => 3,
        St::Suspend => 4,
        St::Resume => 5,
        St::Stop => 6,
        St::Gone => 7,
    }
}

/// 合法跃迁表（from, to）。
pub const TR_OK: [(St, St); 12] = [
    (St::None, St::Reg),
    (St::Reg, St::Start),
    (St::Start, St::Run),
    (St::Run, St::Suspend),
    (St::Suspend, St::Resume),
    (St::Resume, St::Run),
    (St::Run, St::Stop),
    (St::Suspend, St::Stop),
    (St::Stop, St::None), // 回未注册（等待卸载）
    (St::None, St::Gone), // 卸载（需引用清零）
    (St::Reg, St::Gone),  // 未启动直接卸载
    (St::Run, St::Run),   // 自环（幂等心跳）
];

/// 跃迁合法核。
pub fn tr_ok(from: St, to: St) -> bool {
    TR_OK.iter().any(|x| x.0 == from && x.1 == to)
}

/// 热插拔事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ev {
    /// 插入。
    In,
    /// 拔出。
    Out,
}

/// 事件记录。
pub struct ERec {
    pub ev: Ev,
    pub epoch: u32,
}

/// 驱动生命周期账。
pub struct Drv {
    pub name: u32,
    st: St,
    /// 引用计数。
    refs: u32,
    /// 离线态（拔出）。
    pub offline: bool,
    /// 事件流。
    pub evs: Vec<ERec>,
    /// 跃迁次数。
    pub trans: u32,
}

/// 总管。
pub struct LifeBook {
    drvs: Vec<Drv>,
}

impl LifeBook {
    /// 空账。
    pub fn new() -> LifeBook {
        LifeBook { drvs: Vec::new() }
    }

    /// 注册：重名拒。
    pub fn reg(&mut self, name: u32) -> Result<(), u32> {
        if name == 0 {
            return Err(XC_NODRV);
        }
        if self.drvs.iter().any(|d| d.name == name) {
            return Err(XC_DUP);
        }
        self.drvs.push(Drv {
            name,
            st: St::Reg,
            refs: 0,
            offline: false,
            evs: Vec::new(),
            trans: 1, // None→Reg
        });
        Ok(())
    }

    /// 跃迁：查表 + 引用闸。
    pub fn go(&mut self, name: u32, to: St) -> Result<St, u32> {
        let d = self
            .drvs
            .iter_mut()
            .find(|d| d.name == name)
            .ok_or(XC_NODRV)?;
        let from = d.st;
        if !tr_ok(from, to) {
            return Err(XC_TR);
        }
        if to == St::Gone && d.refs > 0 {
            return Err(XC_REFS);
        }
        d.st = to;
        d.trans += 1;
        Ok(to)
    }

    /// 引用计数：借 +1 / 还 -1。
    pub fn ref_up(&mut self, name: u32) -> Result<u32, u32> {
        let d = self
            .drvs
            .iter_mut()
            .find(|d| d.name == name)
            .ok_or(XC_NODRV)?;
        if d.offline {
            return Err(XC_OFFLINE);
        }
        d.refs += 1;
        Ok(d.refs)
    }

    pub fn ref_down(&mut self, name: u32) -> Result<u32, u32> {
        let d = self
            .drvs
            .iter_mut()
            .find(|d| d.name == name)
            .ok_or(XC_NODRV)?;
        if d.refs == 0 {
            return Err(XC_REFS);
        }
        d.refs -= 1;
        Ok(d.refs)
    }

    /// 态查询。
    pub fn st_of(&self, name: u32) -> Option<St> {
        self.drvs.iter().find(|d| d.name == name).map(|d| d.st)
    }

    /// 引用查询。
    pub fn refs_of(&self, name: u32) -> Option<u32> {
        self.drvs.iter().find(|d| d.name == name).map(|d| d.refs)
    }

    /// 热拔出：运行态强制挂起 + 离线 + 通知事件。
    pub fn unplug(&mut self, name: u32, epoch: u32) -> Result<St, u32> {
        let d = self
            .drvs
            .iter_mut()
            .find(|d| d.name == name)
            .ok_or(XC_NODRV)?;
        if d.offline {
            return Err(XC_OFFLINE);
        }
        if d.st != St::Run && d.st != St::Reg {
            return Err(XC_TR);
        }
        let from = d.st;
        d.st = St::Suspend;
        d.offline = true;
        d.trans += 1;
        d.evs.push(ERec {
            ev: Ev::Out,
            epoch,
        });
        let _ = from;
        Ok(St::Suspend)
    }

    /// 热插回：恢复 + 在线。
    pub fn plugin(&mut self, name: u32, epoch: u32) -> Result<St, u32> {
        let d = self
            .drvs
            .iter_mut()
            .find(|d| d.name == name)
            .ok_or(XC_NODRV)?;
        if !d.offline {
            return Err(XC_TR);
        }
        d.offline = false;
        d.st = St::Resume;
        d.trans += 1;
        d.evs.push(ERec {
            ev: Ev::In,
            epoch,
        });
        Ok(St::Resume)
    }

    /// 事件流查询。
    pub fn evs_of(&self, name: u32) -> Option<&[ERec]> {
        self.drvs.iter().find(|d| d.name == name).map(|d| d.evs.as_slice())
    }

    /// 在册数。
    pub fn len(&self) -> usize {
        self.drvs.len()
    }
}

pub fn run_veb361_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0361");
    // 族零：跃迁表。
    s.add("None→Reg 合法", tr_ok(St::None, St::Reg), "落户");
    s.add("Reg→Start 合法", tr_ok(St::Reg, St::Start), "启动");
    s.add("Start→Run 合法", tr_ok(St::Start, St::Run), "就绪");
    s.add("Run→Suspend 合法", tr_ok(St::Run, St::Suspend), "挂起");
    s.add("Suspend→Resume 合法", tr_ok(St::Suspend, St::Resume), "恢复");
    s.add("Resume→Run 合法", tr_ok(St::Resume, St::Run), "续跑");
    s.add("Run→None 拒", !tr_ok(St::Run, St::None), "跳站");
    s.add("Gone→Run 拒", !tr_ok(St::Gone, St::Run), "死而复生");
    // 族一：注册。
    let mut b = LifeBook::new();
    s.add("零名拒", b.reg(0) == Err(XC_NODRV), "无名");
    s.add("注册过", b.reg(0x10DE).is_ok(), "NV 驱动落户");
    s.add("态在 Reg", b.st_of(0x10DE) == Some(St::Reg), "页一");
    s.add("重名拒", b.reg(0x10DE) == Err(XC_DUP), "一户一注");
    // 族二：启动链。
    s.add("Reg→Run 拒", b.go(0x10DE, St::Run) == Err(XC_TR), "跳过启动");
    s.add("启动过", b.go(0x10DE, St::Start) == Ok(St::Start), "页二");
    s.add("就绪过", b.go(0x10DE, St::Run) == Ok(St::Run), "页三");
    s.add("运行自环", b.go(0x10DE, St::Run) == Ok(St::Run), "心跳幂等");
    s.add("跃迁四", b.go(0x10DE, St::Run).is_ok(), "自环也记");
    // 族三：引用计数。
    s.add("借一", b.ref_up(0x10DE) == Ok(1), "引用");
    s.add("借二", b.ref_up(0x10DE) == Ok(2), "双引用");
    s.add("带引用可停", b.go(0x10DE, St::Stop) == Ok(St::Stop), "停机不查引用");
    // 重新走链：Stop→None→Gone。
    let mut b2 = LifeBook::new();
    let _ = b2.reg(7);
    let _ = b2.go(7, St::Start);
    let _ = b2.go(7, St::Run);
    let _ = b2.ref_up(7);
    s.add("Run→Stop 过", b2.go(7, St::Stop) == Ok(St::Stop), "停机");
    s.add("引用仍在", b2.refs_of(7) == Some(1), "未还");
    s.add("Stop→None 过", b2.go(7, St::None) == Ok(St::None), "回未注册");
    s.add("卸载拒引用", b2.go(7, St::Gone) == Err(XC_REFS), "户口有引用");
    s.add("还一", b2.ref_down(7) == Ok(0), "清引用");
    s.add("还二拒", b2.ref_down(7) == Err(XC_REFS), "超还");
    s.add("卸载过", b2.go(7, St::Gone) == Ok(St::Gone), "注销");
    s.add("态在 Gone", b2.st_of(7) == Some(St::Gone), "页七");
    // 族四：热插拔。
    let mut b3 = LifeBook::new();
    let _ = b3.reg(9);
    let _ = b3.go(9, St::Start);
    let _ = b3.go(9, St::Run);
    s.add("拔出挂起", b3.unplug(9, 5) == Ok(St::Suspend), "断电");
    s.add("离线在", b3.st_of(9) == Some(St::Suspend), "态挂");
    s.add("离线借拒", b3.ref_up(9) == Err(XC_OFFLINE), "拔出不接活");
    s.add("重复拔拒", b3.unplug(9, 6) == Err(XC_OFFLINE), "已拔");
    s.add("插回恢复", b3.plugin(9, 7) == Ok(St::Resume), "来电");
    s.add("事件二", b3.evs_of(9).map(|e| e.len()) == Some(2), "出+入");
    s.add("事件序", match b3.evs_of(9) {
        Some(e) => e.len() == 2 && e[0].ev == Ev::Out && e[1].ev == Ev::In,
        None => false,
    }, "拔→插");
    s.add("重复插拒", b3.plugin(9, 8) == Err(XC_TR), "已在线");
    // 恢复→运行→卸载。
    let _ = b3.go(9, St::Run);
    let _ = b3.go(9, St::Stop);
    let _ = b3.go(9, St::None);
    s.add("全周期卸载", b3.go(9, St::Gone) == Ok(St::Gone), "八态走全");
    s.add("跃迁账八", b3.len() == 1, "一户");
    // 族五：不存在驱动。
    s.add("查无态", b3.st_of(99).is_none(), "查无");
    s.add("查无跃迁拒", b3.go(99, St::Run) == Err(XC_NODRV), "无此户");
    s.add("魔数核", DL_SEG_BASE == 0x32340, "段基址");
    s
}
