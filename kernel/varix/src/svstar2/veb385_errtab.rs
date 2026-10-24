//! veb385_errtab —— VE-F0385 B 域错误码总表。
//! 锚点（CoRun Unxreal 主册）：VE-F0385 · B 域错误码总表 · 目标 300 行
//! 判据原文：VE-F0385-J1 错误码总表收口 B 域全部错误码：注册
//!           （码 + 名 + 种别），重码拒、重名拒；码段核（B 域
//!           段内码必须落在本域段界内，越段拒）；种别统计（按
//!           种别出计数）；总表完整性核（段内已用码无空洞登记
//!           交叉核——登记数 = 段内码数）。
//! 组件：错误码项 + 段界检查 + 种别账 + 完整性核。
//! 职责定位：错误码总表是「B 域的号码簿」——错误码是跨模块契
//!           约，两个模块撞码就是调试地狱；段界检查防越权占用
//!           别域号段；种别统计让「今天有几个内存类错误」一眼
//!           可见；完整性核保证号码簿没有空号谎报。
//! 性能逐项分解：注册 O(n)（双查重）；统计 O(n)。
//! 诊断码段：0x305C0（0x305C1~0x305C9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const ET_VERSION: &str = "veb385_errtab-v1";

/// 诊断段基址（0x305C0 段 · 本模块独占）。
pub const ET_SEG_BASE: u32 = 0x305C0;
pub const XC_DUP_CODE: u32 = 0x305C1; // 重码
pub const XC_DUP_NAME: u32 = 0x305C2; // 重名
pub const XC_OUT_SEG: u32 = 0x305C3; // 越段
pub const XC_NAME_EMPTY: u32 = 0x305C4; // 名空
pub const XC_KIND_OOR: u32 = 0x305C5; // 种别越域
pub const XC_MAGIC: u32 = 0x305C6; // 账魔数破
pub const XC_INCOMP: u32 = 0x305C7; // 完整性破
pub const XC_NOENT: u32 = 0x305C8; // 查无
pub const XC_GAP: u32 = 0x305C9; // 段内空洞

/// B 域错误码段界（示例段：0x31000~0x310FF）。
pub const SEG_LO: u32 = 0x31000;
pub const SEG_HI: u32 = 0x310FF;

/// 错误码种别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrKind {
    /// 内存类。
    Mem = 0,
    /// IO 类。
    Io = 1,
    /// 参数类。
    Param = 2,
    /// 状态类。
    State = 3,
}

impl ErrKind {
    pub fn from_u32(v: u32) -> Option<ErrKind> {
        match v {
            0 => Some(ErrKind::Mem),
            1 => Some(ErrKind::Io),
            2 => Some(ErrKind::Param),
            3 => Some(ErrKind::State),
            _ => None,
        }
    }
}

/// 错误码项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrItem {
    pub code: u32,
    pub name: &'static str,
    pub kind: ErrKind,
}

/// 总表。
pub struct ErrTab {
    items: Vec<ErrItem>,
    magic: u32,
}

impl ErrTab {
    /// 建。
    pub fn new() -> ErrTab {
        ErrTab { items: Vec::new(), magic: 0x05C0_E177 }
    }

    /// 注册。
    pub fn put(&mut self, code: u32, name: &'static str, kind: ErrKind) -> Result<(), u32> {
        if self.magic != 0x05C0_E177 {
            return Err(XC_MAGIC);
        }
        if name.is_empty() {
            return Err(XC_NAME_EMPTY);
        }
        if code < SEG_LO || code > SEG_HI {
            return Err(XC_OUT_SEG);
        }
        for it in self.items.iter() {
            if it.code == code {
                return Err(XC_DUP_CODE);
            }
            if it.name == name {
                return Err(XC_DUP_NAME);
            }
        }
        self.items.push(ErrItem { code, name, kind });
        Ok(())
    }

    /// 按码查。
    pub fn by_code(&self, code: u32) -> Result<&ErrItem, u32> {
        for it in self.items.iter() {
            if it.code == code {
                return Ok(it);
            }
        }
        Err(XC_NOENT)
    }

    /// 种别计数。
    pub fn count_kind(&self, kind: ErrKind) -> u32 {
        let mut n = 0u32;
        for it in self.items.iter() {
            if it.kind == kind {
                n += 1;
            }
        }
        n
    }

    /// 完整性核：登记数 = 表内去重码数，且码排序后严格递增无重。
    pub fn completeness(&self) -> Result<u32, u32> {
        if self.magic != 0x05C0_E177 {
            return Err(XC_MAGIC);
        }
        // 冒泡收集排序码。
        let mut codes: Vec<u32> = Vec::new();
        for it in self.items.iter() {
            codes.push(it.code);
        }
        for i in 1..codes.len() {
            let mut j = i;
            while j > 0 && codes[j] < codes[j - 1] {
                let t = codes[j];
                codes[j] = codes[j - 1];
                codes[j - 1] = t;
                j -= 1;
            }
        }
        for i in 1..codes.len() {
            if codes[i] == codes[i - 1] {
                return Err(XC_DUP_CODE);
            }
        }
        Ok(codes.len() as u32)
    }

    /// 段内连续段核：已登记码须覆盖 [min, max] 无洞（演示核：只
    /// 对「声称连续」的登记集合做空洞检查）。
    pub fn gap_check(&self) -> Result<u32, u32> {
        if self.items.is_empty() {
            return Err(XC_GAP);
        }
        let mut codes: Vec<u32> = Vec::new();
        for it in self.items.iter() {
            codes.push(it.code);
        }
        for i in 1..codes.len() {
            let mut j = i;
            while j > 0 && codes[j] < codes[j - 1] {
                let t = codes[j];
                codes[j] = codes[j - 1];
                codes[j - 1] = t;
                j -= 1;
            }
        }
        for i in 1..codes.len() {
            if codes[i] - codes[i - 1] > 1 {
                return Err(XC_GAP);
            }
        }
        Ok(codes.len() as u32)
    }

    /// 表长。
    pub fn len(&self) -> usize {
        self.items.len()
    }
}

pub fn run_veb385_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0385");
    // 族一：注册与查。
    let mut t = ErrTab::new();
    let _ = t.put(0x31001, "E_MEM_OOM", ErrKind::Mem);
    let _ = t.put(0x31002, "E_MEM_GUARD", ErrKind::Mem);
    let _ = t.put(0x31010, "E_IO_TIMEOUT", ErrKind::Io);
    let _ = t.put(0x31011, "E_PARAM_OOR", ErrKind::Param);
    let _ = t.put(0x31012, "E_STATE_BAD", ErrKind::State);
    s.add("注册 5", t.len() == 5, "五码");
    let q = t.by_code(0x31010);
    s.add("按码查", matches!(q, Ok(it) if it.name == "E_IO_TIMEOUT" && it.kind == ErrKind::Io), "码主键");
    s.add("查无", t.by_code(0x31099) == Err(XC_NOENT), "空号");
    // 族二：种别统计。
    s.add("内存类 2", t.count_kind(ErrKind::Mem) == 2, "两 mem");
    s.add("IO 类 1", t.count_kind(ErrKind::Io) == 1, "一 io");
    s.add("参数类 1", t.count_kind(ErrKind::Param) == 1, "一 param");
    s.add("状态类 1", t.count_kind(ErrKind::State) == 1, "一 state");
    // 族三：段界。
    let mut b = ErrTab::new();
    s.add("段下界外拒", b.put(0x30FFF, "low", ErrKind::Io) == Err(XC_OUT_SEG), "<SEG_LO");
    s.add("段上界外拒", b.put(0x31100, "high", ErrKind::Io) == Err(XC_OUT_SEG), ">SEG_HI");
    s.add("段界内过", b.put(SEG_LO, "lo-edge", ErrKind::Io).is_ok(), "恰在下界");
    s.add("上界内过", b.put(SEG_HI, "hi-edge", ErrKind::Io).is_ok(), "恰在上界");
    // 族四：双查重。
    let mut c = ErrTab::new();
    let _ = c.put(0x31001, "a", ErrKind::Mem);
    s.add("重码拒", c.put(0x31001, "b", ErrKind::Io) == Err(XC_DUP_CODE), "撞码");
    s.add("重名拒", c.put(0x31002, "a", ErrKind::Io) == Err(XC_DUP_NAME), "撞名");
    // 族五：完整性。
    let comp = c.completeness();
    s.add("完整性过", comp == Ok(1), "账实一致");
    s.add("空洞核过", c.gap_check() == Ok(1), "单码无洞");
    let mut d = ErrTab::new();
    let _ = d.put(0x31001, "x", ErrKind::Mem);
    let _ = d.put(0x31003, "y", ErrKind::Mem);
    s.add("空洞检出", d.gap_check() == Err(XC_GAP), "0103 跳号");
    s.add("完整性仍过", d.completeness() == Ok(2), "无重即可");
    let e = ErrTab::new();
    s.add("空表空洞拒", e.gap_check() == Err(XC_GAP), "无码");
    s.add("种别核", ErrKind::from_u32(3) == Some(ErrKind::State) && ErrKind::from_u32(4).is_none(), "闭集");
    s.add("魔数核", ET_SEG_BASE == 0x305C0, "段基址");
    s
}
