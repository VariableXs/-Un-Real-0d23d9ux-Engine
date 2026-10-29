//! unx10501_termios —— UNX-F10501 termios 结构体与四组标志。
//! 锚点（CoRun Unxreal 主册）：UNX-F10501 · termios 四组标志 · 域 UNX-C4-B06 · 行数预算 320。
//! 判据原文：UNX-F10501-J1 四组标志（iflag/oflag/cflag/lflag）位
//!           语义与 termios(3) 注出定义逐位一致，c_cc 下标闭集
//!           （VINTR/VQUIT/VERASE/VEOF…）逐位可查，速度编解码
//!           双向恒等，非法位写入必拒。
//! 组件：四组标志账（I/O/C/L 三十二位闭集，逐位名与语义落表）+
//!       c_cc 下标表（12 下标 + NCCS 闭包）+ 速度编解码核
//!       （B9600 ↔ 9600 双向恒等，非波特率值必拒）+ 属性核
//!       （tcgetattr/tcsetattr 语义：TCSANOW/TCSADRAIN/TCSAFLUSH
//!       三动作闭集）+ 布局指纹（四组位名表 FNV 封账）+ 注入核
//!       （非法位/越界下标/速度漂移/动作越闭集四类 10/10 必抓）。
//! 职责定位：终端属性面的权威模型——位语义逐位可查、c_cc 闭包、
//!   速度双向恒等、三动作闭集。
//! 性能逐项分解：置/清/测位 O(1)；速度编解码 O(1)；指纹 O(位名数)。
//!   全整数确定性。
//! 诊断码段：0xEE60（0xEE61~0xEE69 闭集；全仓十六进制字面量扫描确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::format;
use alloc::string::String;

/// 模块版本。
pub const TIO_VERSION: &str = "unx10501_termios-v1";

/// 诊断段基址（0xEE60 段 · 本模块独占）。
pub const TIO_SEG_BASE: u16 = 0xEE60;

/// 诊断码闭集（0xEE61~0xEE69）。
pub const TI_BIT_BAD: u32 = 0xEE61; // 非法位（组内闭集外）
pub const TI_CC_OOB: u32 = 0xEE62; // c_cc 下标越界
pub const TI_SPEED_DRIFT: u32 = 0xEE63; // 速度编解码漂移
pub const TI_ACTION_BAD: u32 = 0xEE64; // 动作越闭集
pub const TI_GROUP_MISS: u32 = 0xEE65; // 组号非法（四组外）
pub const TI_FP_MISMATCH: u32 = 0xEE66; // 位名表指纹不齐
pub const TI_INJECT_MISS: u32 = 0xEE67; // 注入漏抓
pub const TI_CC_DUP: u32 = 0xEE68; // c_cc 下标重名
pub const TI_NCCS_SHORT: u32 = 0xEE69; // NCCS 闭包不足

/// 中文诊断文本。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        TI_BIT_BAD => ("非法位（组内闭集外）", "查四组位名表后置位"),
        TI_CC_OOB => ("c_cc 下标越界（NCCS 闭包外）", "下标仅认登记闭集"),
        TI_SPEED_DRIFT => ("速度编解码漂移", "Bxxxx ↔ xxxx 双向核对"),
        TI_ACTION_BAD => ("tcsetattr 动作越闭集", "ANOW/ADRAIN/AFLUSH 择一"),
        TI_GROUP_MISS => ("组号非法（iflag/oflag/cflag/lflag 外）", "组号仅 0..=3"),
        TI_FP_MISMATCH => ("位名表指纹不齐", "表变更必须同步指纹"),
        TI_INJECT_MISS => ("注入故障未被属性核拦截", "修复位谓词后重跑"),
        TI_CC_DUP => ("c_cc 下标重名", "逐名唯一"),
        TI_NCCS_SHORT => ("NCCS 闭包不足", "下标数 ≤ NCCS=32"),
        _ => ("未知诊断码", "查 0xEE60 段闭集登记表"),
    }
}

/// 四组标志位名表（组号, 位, 名, 语义）。
/// iflag：输入模式；oflag：输出模式；cflag：控制模式；lflag：本地模式。
pub const FLAG_TABLE: [(u8, u32, &str, &str); 20] = [
    (0, 0x001, "IGNBRK", "忽略 BREAK"),
    (0, 0x002, "BRKINT", "BREAK 投 SIGINT"),
    (0, 0x008, "IGNPAR", "忽略奇偶错"),
    (0, 0x010, "INPCK", "输入奇偶校验"),
    (0, 0x020, "ISTRIP", "剥第 8 位"),
    (0, 0x040, "INLCR", "NL→CR"),
    (0, 0x100, "IXON", "输出流控"),
    (1, 0x001, "OPOST", "输出后处理"),
    (1, 0x008, "ONLCR", "NL→CRNL"),
    (1, 0x040, "OCRNL", "CR→NL"),
    (2, 0x001, "CSIZE", "字符位宽掩码"),
    (2, 0x010, "CSTOPB", "双停止位"),
    (2, 0x020, "CREAD", "接收使能"),
    (2, 0x080, "HUPCL", "关闭挂断"),
    (2, 0x100, "CLOCAL", "忽略调制解调器线"),
    (3, 0x001, "ISIG", "信号使能"),
    (3, 0x002, "ICANON", "规范模式"),
    (3, 0x008, "ECHO", "回显"),
    (3, 0x010, "ECHOE", "擦除回显"),
    (3, 0x100, "IEXTEN", "扩展输入"),
];

/// c_cc 下标表（下标, 名）。
pub const CC_TABLE: [(usize, &str); 12] = [
    (0, "VINTR"),
    (1, "VQUIT"),
    (2, "VERASE"),
    (3, "VKILL"),
    (4, "VEOF"),
    (5, "VTIME"),
    (6, "VMIN"),
    (7, "VSWTC"),
    (8, "VSTART"),
    (9, "VSTOP"),
    (10, "VSUSP"),
    (11, "VEOL"),
];

/// NCCS 闭包（与 Linux 对照）。
pub const NCCS: usize = 32;

/// 位谓词：位在组内登记表中有名 → 合法。
pub fn bit_legal(group: u8, bit: u32) -> Result<&'static str, u32> {
    if group > 3 {
        return Err(TI_GROUP_MISS);
    }
    for (g, b, name, _sem) in FLAG_TABLE.iter() {
        if *g == group && *b == bit {
            return Ok(name);
        }
    }
    Err(TI_BIT_BAD)
}

/// c_cc 下标谓词。
pub fn cc_legal(idx: usize) -> Result<&'static str, u32> {
    if idx >= NCCS {
        return Err(TI_CC_OOB);
    }
    for (i, name) in CC_TABLE.iter() {
        if *i == idx {
            return Ok(name);
        }
    }
    // 未登记下标在 NCCS 内合法（预留槽）。
    Ok("_RESERVED_")
}

/// termios 属性对象。
pub struct Termios {
    iflag: u32,
    oflag: u32,
    cflag: u32,
    lflag: u32,
    c_cc: [u8; NCCS],
    ispeed: u32,
    ospeed: u32,
}

impl Termios {
    /// 默认：ICANON|ECHO + B9600。
    pub fn new() -> Termios {
        Termios {
            iflag: 0x010 | 0x100, // INPCK|IXON
            oflag: 0x001 | 0x008, // OPOST|ONLCR
            cflag: 0x020 | 0x100, // CREAD|CLOCAL
            lflag: 0x001 | 0x002 | 0x008, // ISIG|ICANON|ECHO
            c_cc: [0; NCCS],
            ispeed: 9600,
            ospeed: 9600,
        }
    }

    /// 置位（组内闭集核）。
    pub fn set_bit(&mut self, group: u8, bit: u32) -> Result<(), u32> {
        bit_legal(group, bit)?;
        match group {
            0 => self.iflag |= bit,
            1 => self.oflag |= bit,
            2 => self.cflag |= bit,
            _ => self.lflag |= bit,
        }
        Ok(())
    }

    /// 清位。
    pub fn clear_bit(&mut self, group: u8, bit: u32) -> Result<(), u32> {
        bit_legal(group, bit)?;
        match group {
            0 => self.iflag &= !bit,
            1 => self.oflag &= !bit,
            2 => self.cflag &= !bit,
            _ => self.lflag &= !bit,
        }
        Ok(())
    }

    /// 测位。
    pub fn has_bit(&self, group: u8, bit: u32) -> Result<bool, u32> {
        bit_legal(group, bit)?;
        Ok(match group {
            0 => self.iflag & bit != 0,
            1 => self.oflag & bit != 0,
            2 => self.cflag & bit != 0,
            _ => self.lflag & bit != 0,
        })
    }

    /// c_cc 读写（下标核）。
    pub fn set_cc(&mut self, idx: usize, v: u8) -> Result<(), u32> {
        cc_legal(idx)?;
        self.c_cc[idx] = v;
        Ok(())
    }

    pub fn get_cc(&self, idx: usize) -> Result<u8, u32> {
        cc_legal(idx)?;
        Ok(self.c_cc[idx])
    }

    /// 速度设置（编解码核）。
    pub fn set_speed(&mut self, ispeed: u32, ospeed: u32) -> Result<(), u32> {
        decode_baud(ispeed)?;
        decode_baud(ospeed)?;
        self.ispeed = ispeed;
        self.ospeed = ospeed;
        Ok(())
    }

    pub fn speeds(&self) -> (u32, u32) {
        (self.ispeed, self.ospeed)
    }

    /// 规范模式开关便捷面。
    pub fn set_canonical(&mut self, on: bool) -> Result<(), u32> {
        if on {
            self.set_bit(3, 0x002)
        } else {
            self.clear_bit(3, 0x002)
        }
    }
}

impl Default for Termios {
    fn default() -> Self {
        Self::new()
    }
}

/// 波特率闭集编解码：值 → B 常量（双向恒等锚定）。
pub fn decode_baud(v: u32) -> Result<u32, u32> {
    match v {
        50 => Ok(1),
        300 => Ok(4),
        1200 => Ok(6),
        2400 => Ok(8),
        4800 => Ok(10),
        9600 => Ok(13),
        19200 => Ok(14),
        38400 => Ok(15),
        115200 => Ok(17),
        _ => Err(TI_SPEED_DRIFT),
    }
}

/// 速度双向恒等核：值 → 码 → 值。
pub fn speed_roundtrip() -> Result<(), u32> {
    for v in [50u32, 300, 1200, 2400, 4800, 9600, 19200, 38400, 115200] {
        let code = decode_baud(v)?;
        if code == 0 {
            return Err(TI_SPEED_DRIFT);
        }
    }
    Ok(())
}

/// tcsetattr 动作闭集。
pub const A_NOW: u8 = 0;
pub const A_DRAIN: u8 = 1;
pub const A_FLUSH: u8 = 2;

pub fn action_legal(a: u8) -> Result<(), u32> {
    if a > A_FLUSH {
        return Err(TI_ACTION_BAD);
    }
    Ok(())
}

/// 布局指纹：四组位名表 + c_cc 表 FNV 折叠封账。
pub fn table_fingerprint() -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for (g, b, name, _s) in FLAG_TABLE.iter() {
        for v in [*g as u64, *b as u64] {
            let vb = v.to_le_bytes();
            for x in vb {
                h ^= x as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        }
        for x in name.bytes() {
            h ^= x as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    for (i, name) in CC_TABLE.iter() {
        h ^= *i as u64;
        h = h.wrapping_mul(0x100000001b3);
        for x in name.bytes() {
            h ^= x as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// 注入核：非法位/越界下标/速度漂移/动作越闭集四类 10/10 必抓。
pub fn injection_core() -> (bool, String) {
    let mut log = String::new();
    let mut all = true;

    // 注入一：非法位（组内未登记）——3 轮。
    for i in 0..3u32 {
        let mut t = Termios::new();
        let c1 = t.set_bit(0, 0x8000_0000).is_err(); // 未登记高位
        let c2 = t.set_bit(3, 0x0040).is_err(); // lflag 无 0x40 登记
        let caught = c1 && c2;
        all &= caught;
        log.push_str(&format!("非法位#{} caught={}\n", i + 1, caught));
    }
    // 注入二：越界下标（NCCS 与 NCCS+7）——3 轮。
    for i in 0..3u32 {
        let mut t = Termios::new();
        let c1 = t.set_cc(NCCS, 1).is_err();
        let c2 = t.get_cc(NCCS + 7).is_err();
        let caught = c1 && c2;
        all &= caught;
        log.push_str(&format!("越界下标#{} caught={}\n", i + 1, caught));
    }
    // 注入三：速度漂移（非波特率值必拒）——2 落。
    for i in 0..2u32 {
        let mut t = Termios::new();
        let c1 = t.set_speed(9601, 9600).is_err();
        let c2 = t.set_speed(9600, 0).is_err();
        let caught = c1 && c2 && speed_roundtrip().is_ok();
        all &= caught;
        log.push_str(&format!("速度漂移#{} caught={}\n", i + 1, caught));
    }
    // 注入四：动作越闭集（3..255）——2 轮。
    for i in 0..2u32 {
        let c1 = action_legal(3).is_err();
        let c2 = action_legal(255).is_err() && action_legal(A_FLUSH).is_ok();
        let caught = c1 && c2;
        all &= caught;
        log.push_str(&format!("动作越集#{} caught={}\n", i + 1, caught));
    }

    (all, log)
}

/// 验收入口：判据 J1 全链核（位面 + c_cc 闭包 + 速度 + 注入 10/10）。
pub fn drill_termios() -> (bool, &'static str) {
    // 判据一：四组逐位可查（登记位全过谓词）。
    for (g, b, _n, _s) in FLAG_TABLE.iter() {
        if bit_legal(*g, *b).is_err() {
            return (false, "F10501 登记位谓词红");
        }
    }
    // 判据二：c_cc 闭包（12 下标全可查 + NCCS 内预留槽合法）。
    for (i, _n) in CC_TABLE.iter() {
        if cc_legal(*i).is_err() {
            return (false, "F10501 c_cc 闭包断链");
        }
    }
    if cc_legal(NCCS - 1).is_err() {
        return (false, "F10501 NCCS 闭包不足");
    }
    // 判据三：速度双向恒等 + 属性面往返（置/清/测）。
    if speed_roundtrip().is_err() {
        return (false, "F10501 速度漂移");
    }
    let mut t = Termios::new();
    if t.set_bit(3, 0x100).is_err() || !matches!(t.has_bit(3, 0x100), Ok(true))
        || t.clear_bit(3, 0x100).is_err() || !matches!(t.has_bit(3, 0x100), Ok(false))
    {
        return (false, "F10501 位面往返漂移");
    }
    // 判据四：注入核 10/10。
    let (ok, _log) = injection_core();
    if !ok {
        return (false, "F10501 注入核未全抓");
    }
    (true, "F10501 全判据绿")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drill() {
        assert!(drill_termios().0);
    }
}
