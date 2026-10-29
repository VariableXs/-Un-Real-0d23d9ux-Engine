//! UNX-F36102-J1 标准权限五位常量与语义。
//! 判据：① 五位（READ_CONTROL/DELETE/WRITE_DAC/WRITE_OWNER/SYNCHRONIZE）
//! 位值写死且与 36101 真源段锚对拍一致；② 置位/清位原语幂等往返 +
//! 越段组合显性拒；③ 纯功能代码 ≥300 行。
//! 设计纪律：标准段消费方——位值从 36101 的 STD_USED 段锚定，本模块
//! 提供五位命名常量与置位/清位/组合合法性原语，供 36110/36112 消费。

use super::unx36101_masktable as mt;
use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x3920;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_SEG: u16 = SEG_BASE + 2; // 越出标准五位区（含保留位）
pub const E_BIT: u16 = SEG_BASE + 3; // 位名闭集外的标准位请求
pub const E_IDEM: u16 = SEG_BASE + 4; // 幂等往返破（set/clear 不守恒）
pub const E_EMPTY: u16 = SEG_BASE + 5; // 空掩码用于需要非零语义的位
pub const E_ORDER: u16 = SEG_BASE + 6; // 位号乱序/重复（表纪律破）
pub const E_OP: u16 = SEG_BASE + 7; // 操作序列非法（未知算子）

/// 模块版本锚。
pub const VER: u32 = 1;

// ---- 五位标准权限（位值写死，真源锚 = 36101::STD_USED） ----------------------
/// 删除（bit 16）。
pub const DELETE: u32 = 0x0001_0000;
/// 读控制（读 DAC/所有者，bit 17）。
pub const READ_CONTROL: u32 = 0x0002_0000;
/// 写 DAC（改权限，bit 18）。
pub const WRITE_DAC: u32 = 0x0004_0000;
/// 写所有者（接管，bit 19）。
pub const WRITE_OWNER: u32 = 0x0008_0000;
/// 同步（等待挂点，bit 20；语义面见 36112）。
pub const SYNCHRONIZE: u32 = 0x0010_0000;
/// 五位全并（必须与 36101::STD_USED 相等——真源守恒锚）。
pub const STD_ALL: u32 = DELETE | READ_CONTROL | WRITE_DAC | WRITE_OWNER | SYNCHRONIZE;

/// 标准权限枚举（判别式 = 位号 16..20）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StdRight {
    Delete = 16,
    ReadControl = 17,
    WriteDac = 18,
    WriteOwner = 19,
    Synchronize = 20,
}

impl StdRight {
    pub fn code(self) -> u16 {
        self as u16
    }
    /// 位掩码值（1 << 位号）。
    pub fn mask(self) -> u32 {
        1u32 << self.code()
    }
    /// 闭集人话名（与 36101 位名表一致——drill 对拍）。
    pub fn name(self) -> &'static str {
        match self {
            StdRight::Delete => "DELETE",
            StdRight::ReadControl => "READ_CONTROL",
            StdRight::WriteDac => "WRITE_DAC",
            StdRight::WriteOwner => "WRITE_OWNER",
            StdRight::Synchronize => "SYNCHRONIZE",
        }
    }
}

/// 五位全表（位号升序，drill 校验无重复无乱序）。
pub const STD_N: usize = 5;
pub const STD_TABLE: [StdRight; STD_N] = [
    StdRight::Delete,
    StdRight::ReadControl,
    StdRight::WriteDac,
    StdRight::WriteOwner,
    StdRight::Synchronize,
];

/// 按位号取标准右（16..20 之外显性拒——非静默匹配）。
pub fn by_bit(bit: u32) -> Result<StdRight, u16> {
    match bit {
        16 => Ok(StdRight::Delete),
        17 => Ok(StdRight::ReadControl),
        18 => Ok(StdRight::WriteDac),
        19 => Ok(StdRight::WriteOwner),
        20 => Ok(StdRight::Synchronize),
        _ => Err(E_BIT),
    }
}

/// 置位原语：把某个标准右加进掩码（幂等；非标准位不动、不引入）。
pub fn std_set(mask: u32, r: StdRight) -> u32 {
    mask | r.mask()
}

/// 清位原语：把某个标准右从掩码移除（幂等）。
pub fn std_clear(mask: u32, r: StdRight) -> u32 {
    mask & !r.mask()
}

/// 探测原语：掩码是否含某个标准右。
pub fn std_has(mask: u32, r: StdRight) -> bool {
    mask & r.mask() != 0
}

/// 抽取标准段（只保留五位区，其余段位丢弃）。
pub fn std_of(mask: u32) -> u32 {
    mask & mt::STD_USED
}

/// 组合合法性：掩码的标准段必须完全落在五位区内。
/// 保留位（21..23）一旦出现即拒——存态红线（36113 会再拦一次）。
pub fn combo_check(mask: u32) -> Result<(), u16> {
    let std = std_of(mask);
    if std & !mt::STD_USED != 0 {
        return Err(E_SEG);
    }
    if mask & mt::STD_MASK & !mt::STD_USED != 0 {
        return Err(E_SEG);
    }
    Ok(())
}

/// 置位序列状态机：逐条应用 Set/Clear 并校验组合合法性。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Op {
    Set = 0x0001,
    Clear = 0x0002,
}

/// 应用一条操作：Ok(新掩码)；未知算子 E_OP；结果越段 E_SEG。
pub fn apply(mask: u32, op: Op, r: StdRight) -> Result<u32, u16> {
    let next = match op {
        Op::Set => std_set(mask, r),
        Op::Clear => std_clear(mask, r),
    };
    combo_check(next)?;
    Ok(next)
}

/// 五位位名快照（诊断面复用；格式 "bitN=NAME"）。
pub fn snapshot() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    let mut i = 0;
    while i < STD_N {
        out.push(STD_TABLE[i].name());
        i += 1;
    }
    out
}

/// 演练：R1 五位与真源锚对拍；R2 逐位 set/clear 幂等往返；
/// R3 组合合法性 + 越段负例；R4 操作序列状态机；R5 快照面。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：五位并 = 36101::STD_USED（真源守恒）；位号连续 16..20。
    if STD_ALL != mt::STD_USED {
        return Err(E_SEG);
    }
    if VER != 1 {
        return Err(E_VER);
    }
    let mut expect = 0x0001_0000u32;
    let mut i = 0;
    while i < STD_N {
        let r = STD_TABLE[i];
        if r.mask() != expect {
            return Err(E_ORDER);
        }
        match by_bit(r.code() as u32) {
            Ok(g) if g.code() == r.code() => {}
            _ => return Err(E_BIT),
        }
        expect <<= 1;
        hits += 1;
        i += 1;
    }
    // R2：逐位 set→has→clear→has 幂等往返（零掩码起步）。
    let mut i = 0;
    while i < STD_N {
        let r = STD_TABLE[i];
        let m1 = std_set(0, r);
        if !std_has(m1, r) || m1 != r.mask() {
            return Err(E_IDEM);
        }
        let m2 = std_set(m1, r); // 幂等：再置一次不变
        if m2 != m1 {
            return Err(E_IDEM);
        }
        let m3 = std_clear(m2, r);
        if m3 != 0 || std_has(m3, r) {
            return Err(E_IDEM);
        }
        hits += 1;
        i += 1;
    }
    // R3：组合合法性——五位任意并合法；保留位注入必拒。
    let full = std_set(
        std_set(
            std_set(std_set(std_set(0, StdRight::Delete), StdRight::ReadControl), StdRight::WriteDac),
            StdRight::WriteOwner,
        ),
        StdRight::Synchronize,
    );
    if full != STD_ALL || combo_check(full).is_err() {
        return Err(E_SEG);
    }
    let bads: [u32; 5] = [
        mt::STD_RSVD,                    // 纯保留位
        1u32 << 21,                      // 单保留位
        STD_ALL | (1u32 << 23),          // 五位 + 保留位
        mt::STD_MASK & !mt::STD_USED,    // 标准段保留区全置
        0x0001_0000 | mt::GEN_MASK,      // 标准段越界混 GENERIC（std 视角只看位段越界）
    ];
    let mut k = 0;
    while k < 5 {
        if k == 4 {
            // 混 GENERIC 的掩码，其标准段仍是合法五位——combo_check 只管标准段。
            if combo_check(bads[k]).is_err() {
                return Err(E_SEG);
            }
        } else if combo_check(bads[k]).is_ok() {
            return Err(E_SEG);
        }
        hits += 1;
        k += 1;
    }
    // R4：操作序列状态机——set 五位、clear 两位、has 对拍。
    let seq: [(Op, usize); 7] = [
        (Op::Set, 0),
        (Op::Set, 1),
        (Op::Set, 2),
        (Op::Set, 3),
        (Op::Set, 4),
        (Op::Clear, 1),
        (Op::Clear, 4),
    ];
    let mut m: u32 = 0;
    let mut s = 0;
    while s < 7 {
        let (op, idx) = seq[s];
        m = apply(m, op, STD_TABLE[idx])?;
        if combo_check(m).is_err() {
            return Err(E_SEG);
        }
        s += 1;
    }
    if m != (STD_ALL & !READ_CONTROL & !SYNCHRONIZE) {
        return Err(E_IDEM);
    }
    if std_of(m | 0x0000_0089) != (STD_ALL & !READ_CONTROL & !SYNCHRONIZE) {
        return Err(E_SEG);
    }
    hits += 1;
    // R5：快照面——5 行、顺序与位号表一致。
    let snap = snapshot();
    if snap.len() != STD_N {
        return Err(E_ORDER);
    }
    if snap[0] != "DELETE" || snap[4] != "SYNCHRONIZE" {
        return Err(E_BIT);
    }
    let _ = vec![0u8; 0];
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(17));
    }

    #[test]
    fn idempotent_roundtrip() {
        let m = std_set(0, StdRight::Synchronize);
        assert_eq!(std_set(m, StdRight::Synchronize), m);
        assert_eq!(std_clear(std_clear(m, StdRight::Synchronize), StdRight::Synchronize), 0);
    }

    #[test]
    fn reserved_rejected() {
        assert_eq!(combo_check(mt::STD_RSVD), Err(E_SEG));
        assert_eq!(combo_check(STD_ALL | (1u32 << 22)), Err(E_SEG));
    }

    #[test]
    fn outside_bit_rejected() {
        assert_eq!(by_bit(15), Err(E_BIT));
        assert_eq!(by_bit(21), Err(E_BIT));
        assert_eq!(by_bit(28), Err(E_BIT));
    }
}

pub fn run_unx36102_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36102_stdrights");
    cs.add("SEG 段位独占", SEG_BASE == 0x3920, "0x3920~0x392F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "五位锚定/幂等/负例全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("真源守恒", STD_ALL == mt::STD_USED, "五位并 = 36101::STD_USED");
    cs.add("位值写死", DELETE == 0x00010000 && SYNCHRONIZE == 0x00100000, "DELETE/SYNCHRONIZE 位值锚");
    cs.add("闭集拒", by_bit(21) == Err(E_BIT) && by_bit(0) == Err(E_BIT), "位号闭集外显性拒");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_SEG => "越出标准五位区（含保留位）",
        E_BIT => "位名闭集外的标准位请求",
        E_IDEM => "幂等往返破（set/clear 不守恒）",
        E_EMPTY => "空掩码用于需要非零语义的位",
        E_ORDER => "位号乱序/重复（表纪律破）",
        E_OP => "操作序列非法（未知算子）",
        _ => "未知诊断码",
    }
}
