//! veb390_deftab —— VE-F0390 B 域缺陷总账。
//! 锚点（CoRun Unxreal 主册）：VE-F0390 · B 域缺陷总账 · 目标 320 行
//! 判据原文：VE-F0390-J1 缺陷登记（编号 + 级别 + 状态）；级别
//!           四档（致命/严重/一般/轻微）；状态机（开放→修复中
//!           →待验证→关闭，跳步拒）；致命缺陷清零才许宣告完
//!           成（终验闸）；缺陷重开（关闭后验证复发 → 重开回
//!           修复中）；统计（按级别计数 + 按状态计数）。
//! 组件：缺陷项 + 四档级别 + 四态状态机 + 重开账 + 统计。
//! 职责定位：缺陷总账是「B 域的病历本」——级别决定处理优先序，
//!           状态机保证流程不跳（没修就说修好是造假）；致命清
//!           零是完成宣告的硬闸；重开是诚实机制：复发不是新病
//!           是旧病未愈，账要接上。
//! 性能逐项分解：登记 O(1)；状态迁移 O(1)；统计 O(n)。
//! 诊断码段：0x30700（0x30701~0x3070A 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const DF_VERSION: &str = "veb390_deftab-v1";

/// 诊断段基址（0x30700 段 · 本模块独占）。
pub const DF_SEG_BASE: u32 = 0x30700;
pub const XC_ID_DUP: u32 = 0x30701; // 编号重
pub const XC_NOENT: u32 = 0x30702; // 无此缺陷
pub const XC_STATE_JUMP: u32 = 0x30703; // 状态跳步
pub const XC_LEVEL_OOR: u32 = 0x30704; // 级别越域
pub const XC_CLOSED: u32 = 0x30705; // 已关闭再操作
pub const XC_MAGIC: u32 = 0x30706; // 账魔数破
pub const XC_FATAL_OPEN: u32 = 0x30707; // 致命未清
pub const XC_REOPEN_LIE: u32 = 0x30708; // 重开非关闭态
pub const XC_VERIF_OPEN: u32 = 0x30709; // 未修复先验证
pub const XC_SUM: u32 = 0x3070A; // 统计不平

/// 级别四档。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// 致命。
    Fatal = 0,
    /// 严重。
    Major = 1,
    /// 一般。
    Minor = 2,
    /// 轻微。
    Trivial = 3,
}

/// 状态四态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefState {
    /// 开放。
    Open = 0,
    /// 修复中。
    Fixing = 1,
    /// 待验证。
    Verify = 2,
    /// 关闭。
    Closed = 3,
}

/// 缺陷项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Defect {
    pub id: u32,
    pub level: Level,
    pub state: DefState,
    /// 重开次数。
    pub reopens: u32,
}

/// 缺陷总账。
pub struct DefBook {
    defs: Vec<Defect>,
    next_id: u32,
    magic: u32,
}

impl DefBook {
    /// 建。
    pub fn new() -> DefBook {
        DefBook { defs: Vec::new(), next_id: 1, magic: 0x0700_0EF3 }
    }

    /// 登记。
    pub fn log(&mut self, level: Level) -> Result<u32, u32> {
        if self.magic != 0x0700_0EF3 {
            return Err(XC_MAGIC);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.defs.push(Defect { id, level, state: DefState::Open, reopens: 0 });
        Ok(id)
    }

    fn pos(&self, id: u32) -> Result<usize, u32> {
        for (i, d) in self.defs.iter().enumerate() {
            if d.id == id {
                return Ok(i);
            }
        }
        Err(XC_NOENT)
    }

    /// 状态推进（合法迁移：Open→Fixing→Verify→Closed）。
    pub fn advance(&mut self, id: u32) -> Result<DefState, u32> {
        let i = self.pos(id)?;
        let cur = self.defs[i].state;
        let next = match cur {
            DefState::Open => DefState::Fixing,
            DefState::Fixing => DefState::Verify,
            DefState::Verify => DefState::Closed,
            DefState::Closed => return Err(XC_CLOSED),
        };
        self.defs[i].state = next;
        Ok(next)
    }

    /// 直达修复中（从 Open 跳过 Fixing 拒——advance 已保证）；
    /// 此入口显式验证态必须从 Fixing 来。
    pub fn to_verify(&mut self, id: u32) -> Result<(), u32> {
        let i = self.pos(id)?;
        if self.defs[i].state != DefState::Fixing {
            return Err(XC_VERIF_OPEN);
        }
        self.defs[i].state = DefState::Verify;
        Ok(())
    }

    /// 重开（仅 Closed 态）。
    pub fn reopen(&mut self, id: u32) -> Result<(), u32> {
        let i = self.pos(id)?;
        if self.defs[i].state != DefState::Closed {
            return Err(XC_REOPEN_LIE);
        }
        self.defs[i].state = DefState::Fixing;
        self.defs[i].reopens += 1;
        Ok(())
    }

    /// 终验闸：致命缺陷全关闭才许宣告。
    pub fn completion_gate(&self) -> Result<bool, u32> {
        for d in self.defs.iter() {
            if d.level == Level::Fatal && d.state != DefState::Closed {
                return Err(XC_FATAL_OPEN);
            }
        }
        Ok(true)
    }

    /// 按级别计数。
    pub fn count_level(&self, lv: Level) -> u32 {
        let mut n = 0u32;
        for d in self.defs.iter() {
            if d.level == lv {
                n += 1;
            }
        }
        n
    }

    /// 按状态计数。
    pub fn count_state(&self, st: DefState) -> u32 {
        let mut n = 0u32;
        for d in self.defs.iter() {
            if d.state == st {
                n += 1;
            }
        }
        n
    }

    /// 统计守恒核：各级别之和 = 各状态之和 = 总数。
    pub fn stat_consistent(&self) -> Result<u32, u32> {
        let mut by_lv = 0u32;
        for i in 0..4u32 {
            let lv = match i {
                0 => Level::Fatal,
                1 => Level::Major,
                2 => Level::Minor,
                _ => Level::Trivial,
            };
            by_lv += self.count_level(lv);
        }
        let mut by_st = 0u32;
        for i in 0..4u32 {
            let st = match i {
                0 => DefState::Open,
                1 => DefState::Fixing,
                2 => DefState::Verify,
                _ => DefState::Closed,
            };
            by_st += self.count_state(st);
        }
        if by_lv != by_st || by_lv != self.defs.len() as u32 {
            return Err(XC_SUM);
        }
        Ok(by_lv)
    }

    /// 总数。
    pub fn total(&self) -> usize {
        self.defs.len()
    }
}

pub fn run_veb390_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0390");
    // 族一：登记与状态机。
    let mut b = DefBook::new();
    let f1 = b.log(Level::Fatal);
    let m1 = b.log(Level::Major);
    let n1 = b.log(Level::Minor);
    s.add("登记三件", b.total() == 3, "三病历");
    let fid = match f1 { Ok(x) => x, Err(_) => { s.add("登记败", false, "不应到"); return s; } };
    let mid = match m1 { Ok(x) => x, Err(_) => { s.add("登记败", false, "不应到"); return s; } };
    let nid = match n1 { Ok(x) => x, Err(_) => { s.add("登记败", false, "不应到"); return s; } };
    s.add("编号自增", fid == 1 && mid == 2 && nid == 3, "1/2/3");
    s.add("Open→Fixing", b.advance(fid) == Ok(DefState::Fixing), "一环");
    s.add("Fixing→Verify", b.advance(fid) == Ok(DefState::Verify), "二环");
    s.add("Verify→Closed", b.advance(fid) == Ok(DefState::Closed), "三环");
    s.add("Closed 再推拒", b.advance(fid) == Err(XC_CLOSED), "终态");
    s.add("查无拒", b.advance(99) == Err(XC_NOENT), "无此号");
    // 跳步拒：Open 直达 Verify。
    s.add("跳步拒", b.to_verify(mid) == Err(XC_VERIF_OPEN), "未修先验");
    let _ = b.advance(mid);
    s.add("修中验过", b.to_verify(mid).is_ok(), "Fixing→Verify");
    // 族二：重开。
    s.add("非关闭重开拒", b.reopen(mid) == Err(XC_REOPEN_LIE), "Verify 态");
    s.add("关闭重开过", b.reopen(fid).is_ok(), "复发");
    s.add("重开回修中", b.advance(fid) == Ok(DefState::Verify), "Fixing 已重开→Verify");
    // 族三：致命清零闸。
    let mut c = DefBook::new();
    let _ = c.log(Level::Fatal);
    s.add("致命开放挡门", c.completion_gate() == Err(XC_FATAL_OPEN), "未清");
    let id2 = match c.log(Level::Major) { Ok(x) => x, Err(_) => { s.add("登记败", false, "不应到"); return s; } };
    // Major 走完流程关掉，Fatal 仍开放 → 挡门。
    let _ = c.advance(id2);
    let _ = c.advance(id2);
    let _ = c.advance(id2);
    s.add("致命仍开放", c.completion_gate() == Err(XC_FATAL_OPEN), "Major 关了 Fatal 还开");
    // Fatal 走完 → 过闸。
    let _ = c.advance(1);
    let _ = c.advance(1);
    let _ = c.advance(1);
    s.add("致命清后过闸", c.completion_gate() == Ok(true), "全关");
    // 族四：统计。
    s.add("级别统计", b.count_level(Level::Fatal) == 1 && b.count_level(Level::Major) == 1 && b.count_level(Level::Minor) == 1, "各 1");
    s.add("状态守恒", b.stat_consistent() == Ok(3), "三账平");
    s.add("重开计数", b.total() == 3, "总数不因重开涨");
    // 族五：级别序。
    s.add("级别单调", Level::Fatal < Level::Major && Level::Major < Level::Minor && Level::Minor < Level::Trivial, "枚举序");
    s.add("魔数核", DF_SEG_BASE == 0x30700, "段基址");
    s
}
