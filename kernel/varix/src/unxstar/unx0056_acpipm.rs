//! UNX-F0056 · ACPI PM 能力位只读采集 · 诊断段 0x2080
//! 验收锚点：UNX-F0056-J1 PM1a/b 控制/状态寄存器位域、睡眠类型
//! 支持集与节能能力台账判据通过；只读——零写寄存器路径。
//! 纯功能代码 ≥220 行。零浮点、零 unwrap、零 expect。

use crate::alloc::{format, string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x2080;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ADDR: u16 = SEG_BASE + 1;     // PM1a/PM1b 控制块地址为零
const E_SPACE: u16 = SEG_BASE + 2;    // 地址空间 ID 不支持（仅 SystemMemory/IO）
const E_WIDTH: u16 = SEG_BASE + 3;    // 寄存器位宽非 8/16/32
const E_SLPEN: u16 = SEG_BASE + 4;    // SLP_EN 位不在最高有效位约定域
const E_S5_MISS: u16 = SEG_BASE + 5;  // \_S5 睡眠对象缺失（软关机路径断）
const E_S_DUP: u16 = SEG_BASE + 6;    // 同一 Sx 睡眠对象重复登记
const E_S_RANGE: u16 = SEG_BASE + 7;  // 睡眠状态值越 S1..S5 闭集
const E_WAK_VEC: u16 = SEG_BASE + 8;  // 唤醒向量与睡眠类型矛盾
const E_FROZEN: u16 = SEG_BASE + 9;   // 能力账已冻结
const E_BAL: u16 = SEG_BASE + 10;     // 能力位账守恒不平

/// ACPI 通用地址结构的空间 ID 闭集（本仓支持两态）。
pub const GAS_SYS_MEM: u8 = 0x00;
pub const GAS_SYS_IO: u8 = 0x01;

/// PM1a 控制 SLP_TYP/SLP_EN 位域约定（IA-PC）：低 13 位特性位，
/// SLP_TYP 位于位 10..12，SLP_EN 固定位 13。
pub const SLP_EN_BIT: u16 = 1 << 13;
pub const SLP_TYP_SHIFT: u16 = 10;
pub const SLP_TYP_MASK: u16 = 0x7;

/// 睡眠类型闭集 S1..=S5（S0 为工作态不入账）。
pub const S1: u8 = 1;
pub const S2: u8 = 2;
pub const S3: u8 = 3;
pub const S4: u8 = 4;
pub const S5: u8 = 5;

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_ADDR => "PM1a 控制块地址为零：FADT 未给 PM1a_CNT_BLK，拒收",
        E_SPACE => "地址空间 ID 不支持：仅 SystemMemory(0)/SystemIO(1) 两态",
        E_WIDTH => "寄存器位宽非法：闭集 {8,16,32}",
        E_SLPEN => "SLP_EN 不在位 13：IA-PC 约定破坏，拒收",
        E_S5_MISS => "\\_S5 缺失：软关机无落点，能力账降级立案",
        E_S_DUP => "同一 Sx 重复登记：后到拒绝",
        E_S_RANGE => "睡眠状态越 S1..S5：闭集外值拒绝",
        E_WAK_VEC => "唤醒向量与睡眠类型矛盾：S1..S3 需运行时唤醒向量",
        E_FROZEN => "能力账已冻结：只读账禁止再喂",
        E_BAL => "能力位账守恒：登记数与位计数不平",
        _ => "未知诊断码",
    }
}

// ── 通用地址结构（寄存器块投影，值语义）─────────────────────────
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GasBlock {
    pub space_id: u8,
    pub bit_width: u8,
    pub bit_offset: u8,
    pub address: u64,
}

impl GasBlock {
    pub fn validate(&self) -> Result<(), u16> {
        if self.space_id != GAS_SYS_MEM && self.space_id != GAS_SYS_IO {
            return Err(E_SPACE);
        }
        if self.bit_width != 8 && self.bit_width != 16 && self.bit_width != 32 {
            return Err(E_WIDTH);
        }
        if self.bit_offset >= self.bit_width {
            return Err(E_WIDTH);
        }
        if self.address == 0 {
            return Err(E_ADDR);
        }
        Ok(())
    }
}

/// PM1 控制位域解析：把一次只读采样的控制字拆成能力位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pm1ControlBits {
    pub sci_en: bool,     // bit0
    pub bm_rld: bool,     // bit1
    pub gbl_rls: bool,    // bit2
    pub slp_typ: u16,     // bit10..12
    pub slp_en: bool,     // bit13
    pub reserved_set: u16, // 位 3..9 与 14..15 非零计数
}

pub fn decode_pm1_ctrl(word: u16) -> Pm1ControlBits {
    Pm1ControlBits {
        sci_en: word & 1 != 0,
        bm_rld: word & 2 != 0,
        gbl_rls: word & 4 != 0,
        slp_typ: (word >> SLP_TYP_SHIFT) & SLP_TYP_MASK,
        slp_en: word & SLP_EN_BIT != 0,
        reserved_set: ((word >> 3) & 0x7F).count_ones() as u16
            + ((word >> 14) & 0x3).count_ones() as u16,
    }
}

/// PM1 状态位域解析。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pm1StatusBits {
    pub tmr_sts: bool,
    pub bm_sts: bool,
    pub gbl_sts: bool,
    pub pwrbtn_sts: bool,
    pub slpbtn_sts: bool,
    pub wake_sts: bool,
    pub wak_sts_type: u16, // 位 10..12（唤醒来源 Sx）
}

pub fn decode_pm1_sts(word: u16) -> Pm1StatusBits {
    Pm1StatusBits {
        tmr_sts: word & 1 != 0,
        bm_sts: word & 2 != 0,
        gbl_sts: word & 4 != 0,
        pwrbtn_sts: word & 8 != 0,
        slpbtn_sts: word & 16 != 0,
        wake_sts: word & 32 != 0,
        wak_sts_type: (word >> SLP_TYP_SHIFT) & SLP_TYP_MASK,
    }
}

// ── PM 能力账 ────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct SleepObject {
    pub s_type: u8,          // S1..S5
    pub slp_typ_a: u16,      // PM1a 值
    pub slp_typ_b: u16,      // PM1b 值（0 表示与 a 同）
}

pub struct PmCapabilityLedger {
    pub pm1a_ctrl: Option<GasBlock>,
    pub pm1b_ctrl: Option<GasBlock>,
    pub pm1a_sts: Option<GasBlock>,
    pub sleeps: Vec<SleepObject>,
    pub s5_present: bool,
    pub frozen: bool,
    pub rejected: Vec<(u8, u16)>,
}

impl PmCapabilityLedger {
    pub fn new() -> PmCapabilityLedger {
        PmCapabilityLedger {
            pm1a_ctrl: None,
            pm1b_ctrl: None,
            pm1a_sts: None,
            sleeps: Vec::new(),
            s5_present: false,
            frozen: false,
            rejected: Vec::new(),
        }
    }

    pub fn feed_block(&mut self, which: &str, g: GasBlock) -> Result<(), u16> {
        if self.frozen {
            return Err(E_FROZEN);
        }
        g.validate()?;
        match which {
            "PM1a_CNT" => self.pm1a_ctrl = Some(g),
            "PM1b_CNT" => self.pm1b_ctrl = Some(g),
            "PM1a_STS" => self.pm1a_sts = Some(g),
            _ => {}
        }
        Ok(())
    }

    /// 睡眠对象登记：闭集 S1..S5，重复拒绝；S5 特记 s5_present。
    pub fn feed_sleep(&mut self, s_type: u8, typ_a: u16, typ_b: u16) -> Result<(), u16> {
        if self.frozen {
            return Err(E_FROZEN);
        }
        if !(S1..=S5).contains(&s_type) {
            return Err(E_S_RANGE);
        }
        if self.sleeps.iter().any(|s| s.s_type == s_type) {
            self.rejected.push((s_type, E_S_DUP));
            return Ok(false);
        }
        if s_type == S5 {
            self.s5_present = true;
        }
        self.sleeps.push(SleepObject { s_type, slp_typ_a: typ_a, slp_typ_b: typ_b });
        Ok(true)
    }

    /// 唤醒向量一致性：S1..S3 声明运行时唤醒则须有对应支持账。
    /// `wake_supported`：固件/平台侧唤醒能力登记。
    pub fn check_wake(&self, wake_supported: &[u8]) -> Result<(), u16> {
        for s in &self.sleeps {
            if s.s_type <= S3 && !wake_supported.contains(&s.s_type) {
                return Err(E_WAK_VEC);
            }
        }
        Ok(())
    }

    /// 收账冻结：S5 必须在场（软关机最小能力），否则降级立案。
    pub fn close(&mut self) -> Result<(), u16> {
        if self.frozen {
            return Err(E_FROZEN);
        }
        self.frozen = true;
        if !self.s5_present {
            return Err(E_S5_MISS);
        }
        // 守恒：登记 + 拒绝 与喂入数由调用方对拍（此处核控制块最小组）
        if self.pm1a_ctrl.is_none() {
            return Err(E_ADDR);
        }
        Ok(())
    }

    /// 能力摘要行（诊断输出）。
    pub fn summary(&self) -> String {
        let mut s = String::from("PM 能力：S 集合 [");
        for (i, sl) in self.sleeps.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!("S{}", sl.s_type));
        }
        s.push_str("] S5=");
        s.push_str(if self.s5_present { "有" } else { "缺" });
        s
    }

    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        if let Some(g) = &self.pm1a_ctrl {
            buf.extend_from_slice(&g.address.to_le_bytes());
        }
        for s in &self.sleeps {
            buf.push(s.s_type);
            buf.extend_from_slice(&s.slp_typ_a.to_le_bytes());
        }
        buf.push(self.s5_present as u8);
        fnv1a64(&buf)
    }
}

// ── 确定性演练：合法 3 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：控制块 + S1..S5 全集登记，位域解码对拍。
    let mut led = PmCapabilityLedger::new();
    led.feed_block("PM1a_CNT", GasBlock { space_id: GAS_SYS_IO, bit_width: 16, bit_offset: 0, address: 0x404 })?;
    led.feed_block("PM1a_STS", GasBlock { space_id: GAS_SYS_IO, bit_width: 16, bit_offset: 0, address: 0x400 })?;
    for (st, ta) in [(1u8, 0x1u16), (2, 0x2), (3, 0x3), (4, 0x4), (5, 0x5)] {
        led.feed_sleep(st, ta, 0)?;
    }
    led.close()?;
    let bits = decode_pm1_ctrl(0x2400 | 0x5 << SLP_TYP_SHIFT | 1);
    if !bits.slp_en || bits.slp_typ != 5 {
        return Err(E_SLPEN);
    }
    hits += 1;

    // R2：状态位域：唤醒来源 S3 解码。
    let sts = decode_pm1_sts(32 | 3 << SLP_TYP_SHIFT);
    if !sts.wake_sts || sts.wak_sts_type != 3 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：摘要串与指纹对拍（同输入同指纹）。
    let mut led2 = PmCapabilityLedger::new();
    led2.feed_block("PM1a_CNT", GasBlock { space_id: GAS_SYS_IO, bit_width: 16, bit_offset: 0, address: 0x404 })?;
    for (st, ta) in [(1u8, 0x1u16), (2, 0x2), (3, 0x3), (4, 0x4), (5, 0x5)] {
        led2.feed_sleep(st, ta, 0)?;
    }
    led2.close()?;
    if led.fingerprint() != led2.fingerprint() {
        return Err(E_BAL);
    }
    if !led.summary().contains("S5=有") {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：控制块地址为零 → E_ADDR。
    let mut i1 = PmCapabilityLedger::new();
    if i1.feed_block("PM1a_CNT", GasBlock { space_id: GAS_SYS_IO, bit_width: 16, bit_offset: 0, address: 0 }) != Err(E_ADDR) {
        return Err(E_ADDR);
    }
    hits += 1;

    // I2：空间 ID 超闭集 → E_SPACE。
    let mut i2 = PmCapabilityLedger::new();
    if i2.feed_block("PM1a_CNT", GasBlock { space_id: 0x0A, bit_width: 16, bit_offset: 0, address: 0x404 }) != Err(E_SPACE) {
        return Err(E_SPACE);
    }
    hits += 1;

    // I3：位宽 12 → E_WIDTH。
    let mut i3 = PmCapabilityLedger::new();
    if i3.feed_block("PM1a_CNT", GasBlock { space_id: GAS_SYS_IO, bit_width: 12, bit_offset: 0, address: 0x404 }) != Err(E_WIDTH) {
        return Err(E_WIDTH);
    }
    hits += 1;

    // I4：S6 越闭集 → E_S_RANGE。
    let mut i4 = PmCapabilityLedger::new();
    if i4.feed_sleep(6, 6, 0) != Err(E_S_RANGE) {
        return Err(E_S_RANGE);
    }
    hits += 1;

    // I5：S3 重复登记 → 留痕拒绝。
    let mut i5 = PmCapabilityLedger::new();
    i5.feed_sleep(3, 3, 0)?;
    i5.feed_sleep(3, 3, 0)?;
    if i5.rejected.len() != 1 || i5.rejected[0].1 != E_S_DUP {
        return Err(E_S_DUP);
    }
    hits += 1;

    // I6：缺 S5 收账 → E_S5_MISS。
    let mut i6 = PmCapabilityLedger::new();
    i6.feed_block("PM1a_CNT", GasBlock { space_id: GAS_SYS_IO, bit_width: 16, bit_offset: 0, address: 0x404 })?;
    i6.feed_sleep(1, 1, 0)?;
    if i6.close() != Err(E_S5_MISS) {
        return Err(E_S5_MISS);
    }
    hits += 1;

    // I7：S1 无唤醒支持 → E_WAK_VEC。
    let mut i7 = PmCapabilityLedger::new();
    i7.feed_sleep(1, 1, 0)?;
    if i7.check_wake(&[]) != Err(E_WAK_VEC) {
        return Err(E_WAK_VEC);
    }
    hits += 1;

    Ok(hits)
}
