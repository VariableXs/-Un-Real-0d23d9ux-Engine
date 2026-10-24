//! veb360_b8close —— VE-F0360 组收口与 VE-C 域移交。
//! 锚点：VE-F0360 · 目标 340 行。
//! 判据原文：十九件证据（注册/2D 覆盖/瓦片/SIMD/边界/SOA/
//!           降质/伸缩/4K/功耗/对拍/云场景/混合/诊断/全绿/
//!           下限/演进/规避/文档）、缺陷总账（红清零/黄闭环/
//!           绿登记）、双签移交 VE-C、经验包（fallback 注册
//!           语义/期望管理三清单/诚实功耗声明方法论）、判据。
//! 组件：十九件证据账 + 缺陷三色总账 + 移交包。
//! 职责定位：B8 段收口是「十九道菜上齐」——菜单一道不缺，
//!           红烧缺陷必须清零，黄焖缺陷必须闭环，青-green 登
//!           记在册；双签上桌，VE-C 域接灶。
//! 性能逐项分解：收口一次性 O(条目)；证据 O(1) 指针。
//! 诊断码段：0x32300（0x32301~0x32309 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const BC_VERSION: &str = "veb360_b8close-v1";

/// 诊断段基址（0x32300 段 · 本模块独占）。
pub const BC_SEG_BASE: u32 = 0x32300;
pub const XC_MISSING: u32 = 0x32301; // 证据缺项（阻断）
pub const XC_REDBUG: u32 = 0x32302; // 红缺陷未清零
pub const XC_YELLOW: u32 = 0x32303; // 黄缺陷未闭环
pub const XC_SAMESIG: u32 = 0x32304; // 双签同人
pub const XC_NOHAND: u32 = 0x32305; // 未签即移交

/// 十九件证据（位图位号 0~18）。
pub const E_REG: u32 = 0; // 注册
pub const E_2D: u32 = 1; // 2D 覆盖
pub const E_TILE: u32 = 2; // 瓦片
pub const E_SIMD: u32 = 3; // SIMD
pub const E_BOUND: u32 = 4; // 边界
pub const E_SOA: u32 = 5; // SOA
pub const E_DEG: u32 = 6; // 降质
pub const E_SCALE: u32 = 7; // 伸缩
pub const E_4K: u32 = 8; // 4K
pub const E_PWR: u32 = 9; // 功耗
pub const E_AB: u32 = 10; // 对拍
pub const E_CLOUD: u32 = 11; // 云场景
pub const E_MIX: u32 = 12; // 混合
pub const E_DIAG: u32 = 13; // 诊断
pub const E_GREEN: u32 = 14; // 全绿
pub const E_FLOOR: u32 = 15; // 下限
pub const E_EVOLVE: u32 = 16; // 演进
pub const E_WA: u32 = 17; // 规避
pub const E_DOC: u32 = 18; // 文档
pub const E_ALL: u32 = (1 << 19) - 1;

/// 缺陷三色。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bug {
    /// 红：阻断级。
    Red,
    /// 黄：需闭环。
    Yellow,
    /// 绿：登记观察。
    Green,
}

/// 缺陷条目。
pub struct Defect {
    pub id: u32,
    pub color: Bug,
    /// 状态：登记（绿恒在）／闭环（黄已完成）。
    pub closed: bool,
}

/// 收口簿。
pub struct Close8 {
    /// 证据位图。
    evid: u32,
    bugs: Vec<Defect>,
    sig: Option<(u32, u32)>,
    pub handed: bool,
    /// 经验包三条。
    pub lessons: u32,
}

impl Close8 {
    /// 新簿。
    pub fn new() -> Close8 {
        Close8 {
            evid: 0,
            bugs: Vec::new(),
            sig: None,
            handed: false,
            lessons: 0,
        }
    }

    /// 记证据件（幂等）。
    pub fn put(&mut self, bit: u32) {
        if bit < 19 {
            self.evid |= 1 << bit;
        }
    }

    /// 缺项数。
    pub fn missing(&self) -> u32 {
        (!self.evid & E_ALL).count_ones()
    }

    /// 证据齐。
    pub fn evid_full(&self) -> bool {
        self.evid & E_ALL == E_ALL
    }

    /// 记缺陷。
    pub fn bug(&mut self, id: u32, color: Bug) {
        self.bugs.push(Defect {
            id,
            color,
            closed: false,
        });
    }

    /// 闭环一条缺陷。
    pub fn fix(&mut self, id: u32) -> bool {
        let d = match self.bugs.iter_mut().find(|d| d.id == id) {
            Some(d) => d,
            None => return false,
        };
        if d.closed {
            return false;
        }
        d.closed = true;
        true
    }

    /// 三色清点：(红未清, 黄未闭环, 绿在册)。
    pub fn tally(&self) -> (u32, u32, u32) {
        let red = self
            .bugs
            .iter()
            .filter(|d| d.color == Bug::Red && !d.closed)
            .count() as u32;
        let yellow = self
            .bugs
            .iter()
            .filter(|d| d.color == Bug::Yellow && !d.closed)
            .count() as u32;
        let green = self
            .bugs
            .iter()
            .filter(|d| d.color == Bug::Green)
            .count() as u32;
        (red, yellow, green)
    }

    /// 收口核：证据齐 + 红清零 + 黄闭环。
    pub fn ready(&self) -> Result<(), u32> {
        if !self.evid_full() {
            return Err(XC_MISSING);
        }
        let (r, y, _) = self.tally();
        if r > 0 {
            return Err(XC_REDBUG);
        }
        if y > 0 {
            return Err(XC_YELLOW);
        }
        Ok(())
    }

    /// 双签。
    pub fn sign(&mut self, a: u32, b: u32) -> Result<(), u32> {
        self.ready()?;
        if a == b {
            return Err(XC_SAMESIG);
        }
        self.sig = Some((a, b));
        Ok(())
    }

    /// 移交 VE-C。
    pub fn handover(&mut self) -> Result<(), u32> {
        if self.sig.is_none() {
            return Err(XC_NOHAND);
        }
        self.handed = true;
        Ok(())
    }

    /// 经验包收录（三条封顶）。
    pub fn learn(&mut self) -> u32 {
        if self.lessons < 3 {
            self.lessons += 1;
        }
        self.lessons
    }

    /// 双签在位。
    pub fn signed(&self) -> bool {
        self.sig.is_some()
    }
}

pub fn run_veb360_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0360");
    // 族零：证据账。
    let mut c = Close8::new();
    s.add("空簿缺十九", c.missing() == 19, "全缺");
    c.put(E_REG);
    s.add("缺十八", c.missing() == 18, "少一件");
    c.put(E_REG);
    s.add("幂等记证", c.missing() == 18, "不翻倍");
    for b in 0..19u32 {
        c.put(b);
    }
    s.add("十九齐", c.evid_full() && c.missing() == 0, "全证据");
    // 族二：缺陷三色。
    c.bug(1, Bug::Red);
    c.bug(2, Bug::Yellow);
    c.bug(3, Bug::Yellow);
    c.bug(4, Bug::Green);
    c.bug(5, Bug::Green);
    s.add("三色账", c.tally() == (1, 2, 2), "红一黄二绿二");
    s.add("红在拒收", c.ready() == Err(XC_REDBUG), "红清零前阻断");
    // 族三：闭环。
    s.add("修红", c.fix(1), "红清");
    s.add("修黄一", c.fix(2), "闭环一");
    s.add("仍黄拒收", c.ready() == Err(XC_YELLOW), "黄未全闭环");
    s.add("修黄二", c.fix(3), "闭环二");
    s.add("收口过", c.ready().is_ok(), "证据齐三色清");
    s.add("重修拒", !c.fix(2), "已闭环");
    s.add("修无此缺陷拒", !c.fix(99), "查无");
    s.add("绿恒在", c.tally() == (0, 0, 2), "绿登记观察");
    // 族四：双签移交。
    s.add("同人拒", c.sign(7, 7) == Err(XC_SAMESIG), "一人两票");
    s.add("双签过", c.sign(7, 8).is_ok(), "复核");
    s.add("移交过", c.handover().is_ok(), "VE-C 接灶");
    s.add("移交在", c.handed, "完成");
    // 未签先移交拒。
    let mut c2 = Close8::new();
    for b in 0..19u32 {
        c2.put(b);
    }
    s.add("未签移交拒", c2.handover() == Err(XC_NOHAND), "先签后交");
    // 族五：证据缺项收口拒。
    let mut c3 = Close8::new();
    for b in 0..18u32 {
        c3.put(b); // 缺 E_DOC(18)
    }
    c3.bug(9, Bug::Red);
    let _ = c3.fix(9);
    s.add("缺件拒收", c3.ready() == Err(XC_MISSING), "十九缺一");
    // 族六：经验包。
    s.add("收一", c.learn() == 1, "fallback 注册语义");
    s.add("收二", c.learn() == 2, "期望管理三清单");
    s.add("收三", c.learn() == 3, "诚实功耗方法论");
    s.add("封顶三", c.learn() == 3, "包满不溢");
    s.add("魔数核", BC_SEG_BASE == 0x32300, "段基址");
    s
}
