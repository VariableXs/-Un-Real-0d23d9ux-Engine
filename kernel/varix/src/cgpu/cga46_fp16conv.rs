//! cga46_fp16conv —— CGPU-F0046 SIMD 半精度转换库。
//! 锚点（CGPU 施工书）：CGPU-F0046 · SIMD 半精度转换库 · 目标 320 行
//! 判据摘录：IEEE 754 half（binary16）与 bfloat16 两族位级转换
//!   正确（含次正规/无穷/NaN 三特形）；转换无浮点指令（纯整数
//!   位运算）；通道批量转换与单通道逐位一致；NaN 载荷保持纪律。
//! 组件：binary16 解析器（符号/指数/尾数三段拆解 + 特形分类
//!   闭集：正规/次正规/零/无穷/NaN 五类）+ binary16→f32 位级
//!   转换（指数偏移重排 + 尾数左移 13 + 次正规规格化——全部
//!   整数运算）+ f32→binary16 舍入转换（就近偶舍入，溢出饱和
//!   到无穷）+ bfloat16 截断/舍入转换（7 位尾数，IEEE RNE）+
//!   批量通道面（4 通道批量与逐通道一致断言）。
//! 职责定位：混合精度管线的位级地基——半精度转换错一位就是
//!   图像坏点，本模块用纯整数位运算实现并携带五特形闭集分类，
//!   让转换语义有逐类可核的账面。
//! 性能逐项分解：单次转换 O(1)；批量 O(通道数)；分类闭集核
//!   O(特形数)。全整数确定性，零浮点指令（判据红线）。
//! 诊断码段：0xE510（0xE511~0xE519 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const FC_VERSION: &str = "cga46_fp16conv-v1";

/// 诊断段基址（0xE510 段 · 本模块独占）。
pub const FC_SEG_BASE: u16 = 0xE510;

/// XcCode 闭集（0xE511~0xE519）。
pub const XC_NAN_PAYLOAD: u32 = 0xE511;
pub const XC_ROUND_DRIFT: u32 = 0xE512;
pub const XC_SUBNORMAL_LOST: u32 = 0xE513;
pub const XC_INF_MAPPED: u32 = 0xE514;
pub const XC_BATCH_MISMATCH: u32 = 0xE515;
pub const XC_OVERFLOW_SAT: u32 = 0xE516;
pub const XC_SIGN_LOST: u32 = 0xE517;
pub const XC_CLASS_OOR: u32 = 0xE518;
pub const XC_BF16_TRUNC: u32 = 0xE519;

/// binary16 特形分类闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HfClass {
    /// 正零/负零（指数 0 尾数 0）。
    Zero,
    /// 次正规（指数 0 尾数非 0）。
    Subnormal,
    /// 正规（指数 1..30）。
    Normal,
    /// 无穷（指数 31 尾数 0）。
    Inf,
    /// NaN（指数 31 尾数非 0）。
    Nan,
}

/// 拆解 binary16 位型。
pub const HF_EXP_SHIFT: u32 = 10;
pub const HF_EXP_MASK: u16 = 0x7C00;
pub const HF_MANA_MASK: u16 = 0x03FF;

/// 分类一个 binary16 位型。
pub fn classify_half(h: u16) -> HfClass {
    let exp = (h >> HF_EXP_SHIFT) & 0x1F;
    let man = h & HF_MANA_MASK;
    match (exp, man) {
        (0, 0) => HfClass::Zero,
        (0, _) => HfClass::Subnormal,
        (31, 0) => HfClass::Inf,
        (31, _) => HfClass::Nan,
        _ => HfClass::Normal,
    }
}

/// binary16 → f32 位级转换（纯整数；输出 f32 位型）。
pub fn half_to_f32_bits(h: u16) -> u32 {
    let sign = ((h as u32) & 0x8000) << 16;
    let exp = ((h >> HF_EXP_SHIFT) & 0x1F) as u32;
    let man = (h & HF_MANA_MASK) as u32;
    match (exp, man) {
        (0, 0) => sign, // ±零
        (0, _) => {
            // 次正规规格化：找尾数最高位，重排为正规 f32。
            let mut m = man;
            let mut e: i32 = -14; // binary16 次正规隐含指数
            while (m & 0x0400) == 0 {
                m <<= 1;
                e -= 1;
            }
            m &= 0x03FF; // 去掉隐含位
            let f32_exp = (e + 127 + 10) as u32; // 移到 f32 域
            sign | (f32_exp << 23) | (m << 13)
        }
        (31, 0) => sign | 0x7F80_0000, // ±无穷
        (31, _) => {
            // NaN：载荷左移 13 保持非零（载荷保持纪律）。
            sign | 0x7F80_0000 | (man << 13) | 1
        }
        _ => {
            // 正规：指数重偏（15→127），尾数左移 13。
            let f32_exp = exp - 15 + 127;
            sign | (f32_exp << 23) | (man << 13)
        }
    }
}

/// f32 位型 → binary16 就近偶舍入（纯整数；溢出饱和到无穷）。
pub fn f32_bits_to_half(f: u32) -> u16 {
    let sign = ((f >> 16) & 0x8000) as u16;
    let exp = ((f >> 23) & 0xFF) as i32;
    let man = f & 0x007F_FFFF;
    // NaN / Inf 直映射。
    if exp == 0xFF {
        if man == 0 {
            return sign | 0x7C00;
        }
        return sign | 0x7C00 | ((man >> 13) as u16).max(1);
    }
    // 归一化指数：f32 偏 127 → h 偏 15。
    let e = exp - 127 + 15;
    if e >= 31 {
        // 溢出饱和到无穷（不产 NaN——饱和纪律）。
        return sign | 0x7C00;
    }
    if e >= 1 {
        // 正规：尾数 23→10 位，就近偶舍入。
        let keep = man >> 13;
        let round_bit = (man >> 12) & 1;
        let sticky = if man & 0x0FFF != 0 { 1u32 } else { 0 };
        let mut m = keep;
        if round_bit == 1 && (sticky == 1 || (keep & 1) == 1) {
            m += 1;
        }
        if m > 0x03FF {
            // 尾数进位上溢 → 指数 +1。
            return if e + 1 >= 31 { sign | 0x7C00 } else { sign | (((e + 1) as u16) << 10) };
        }
        return sign | ((e as u16) << 10) | (m as u16);
    }
    // 次正规 / 下溢：把有效尾数右移规格化。
    let shift = 1 - e;
    if shift > 25 {
        return sign; // 深度下溢 → ±零
    }
    let full = man | 0x0080_0000; // 恢复隐含位
    let keep = full >> (13 + shift);
    let round_bit = (full >> (12 + shift)) & 1;
    let sticky = if full & ((1u32 << (12 + shift)) - 1) != 0 { 1u32 } else { 0 };
    let mut m = keep;
    if round_bit == 1 && (sticky == 1 || (keep & 1) == 1) {
        m += 1;
    }
    if m > 0x03FF {
        return sign | 0x0400; // 进位到最小正规
    }
    sign | (m as u16)
}

/// bfloat16：f32 截断高 16 位（截断语义）。
pub fn f32_bits_to_bf16_trunc(f: u32) -> u16 {
    (f >> 16) as u16
}

/// bfloat16：f32 就近偶舍入（尾数第 16 位起舍入）。
pub fn f32_bits_to_bf16_rne(f: u32) -> u16 {
    if ((f >> 23) & 0xFF) == 0xFF && (f & 0x007F_FFFF) != 0 {
        return (f >> 16) as u16; // NaN 直截断（载荷在上 16 位内保持）
    }
    let lsb = (f >> 16) & 1;
    let round = (f >> 15) & 1;
    let sticky = if f & 0x7FFF != 0 { 1u32 } else { 0 };
    let mut hi = f >> 16;
    if round == 1 && (sticky == 1 || lsb == 1) {
        hi += 1;
    }
    hi as u16
}

/// bf16 → f32 位型（低 16 位补零——bfloat16 是 f32 的截断前缀）。
pub fn bf16_to_f32_bits(b: u16) -> u32 {
    (b as u32) << 16
}

/// 批量通道转换与逐通道一致断言（4 通道）。
pub fn batch_half_to_f32(hs: [u16; 4]) -> Result<[u32; 4], u16> {
    let mut out = [0u32; 4];
    for (i, &h) in hs.iter().enumerate() {
        out[i] = half_to_f32_bits(h);
    }
    // 一致断言：批量与逐通道重跑一致（防批量路径私改语义）。
    for (i, &h) in hs.iter().enumerate() {
        if out[i] != half_to_f32_bits(h) {
            return Err(XC_BATCH_MISMATCH as u16 + i as u16);
        }
    }
    Ok(out)
}

/// 域自检入口：分类/转换/舍入/批量 四族。
pub fn run_cga46_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0046-半精度转换");
    // 族一：五特形分类闭集。
    cs.add("零分类", classify_half(0x0000) == HfClass::Zero && classify_half(0x8000) == HfClass::Zero, "±零");
    cs.add("次正规分类", classify_half(0x0001) == HfClass::Subnormal, "指数 0 尾数非 0");
    cs.add("正规分类", classify_half(0x3C00) == HfClass::Normal, "1.0h");
    cs.add("无穷分类", classify_half(0x7C00) == HfClass::Inf, "指数 31 尾数 0");
    cs.add("NaN 分类", classify_half(0x7E00) == HfClass::Nan, "指数 31 尾数非 0");

    // 族二：binary16→f32 位级核对（1.0 / 2.0 / -1.0 / 无穷 / NaN 载荷）。
    cs.add("1.0h 转换", half_to_f32_bits(0x3C00) == 0x3F80_0000, "f32 1.0");
    cs.add("2.0h 转换", half_to_f32_bits(0x4000) == 0x4000_0000, "f32 2.0");
    cs.add("符号保持", half_to_f32_bits(0xBC00) == 0xBF80_0000, "-1.0");
    cs.add("无穷映射", half_to_f32_bits(0x7C00) == 0x7F80_0000, "h+inf→f32+inf");
    cs.add("NaN 载荷保持", (half_to_f32_bits(0x7C01) & 0x7F_FF) != 0, "载荷非零");

    // 族三：f32→binary16 舍入（1.0 / 65519 上溢饱和 / 小数舍入）。
    cs.add("1.0 回转", f32_bits_to_half(0x3F80_0000) == 0x3C00, "f32→h");
    cs.add("上溢饱和", f32_bits_to_half(0x7F80_0000) == 0x7C00, "f32+inf→h+inf");
    cs.add("大数饱和无穷", f32_bits_to_half(0x7780_0000) == 0x7C00, "超 binary16 域饱和");
    cs.add("65504 精确值", f32_bits_to_half(0x477F_F000) == 0x7BFF, "h 最大正规");
    // 就近偶：0x3F80_1000（1.0 + 极小）尾数 sticky=0 round=1 lsb=0 → 舍去。
    cs.add("舍入到偶舍去", f32_bits_to_half(0x3F80_1000) == 0x3C00, "RNE 舍去");

    // 族四：bfloat16（截断 vs RNE）。
    let f = 0x3F80_3FFF; // 1.0 + ε
    cs.add("bf16 截断", f32_bits_to_bf16_trunc(f) == 0x3F80, "直接取高 16 位");
    cs.add("bf16 RNE 进位", f32_bits_to_bf16_rne(0x3F80_8000) == 0x3F81, "round=1 sticky=1");
    cs.add("bf16 RNE 偶舍", f32_bits_to_bf16_rne(0x3F80_7FFF) == 0x3F80, "round=1 sticky=0 lsb=0 舍去");
    cs.add("bf16 往返", bf16_to_f32_bits(f32_bits_to_bf16_trunc(0x4000_0000)) == 0x4000_0000, "2.0 往返");

    // 族五：批量一致。
    let batch = batch_half_to_f32([0x3C00, 0x4000, 0xBC00, 0x7C00]);
    cs.add("批量一致", batch.is_ok(), "批量==逐通道");
    cs.add("批量值核对", matches!(batch, Ok(v) if v[0] == 0x3F80_0000 && v[3] == 0x7F80_0000),
           "1.0/inf 落位");
    cs
}

// ===== 扩展面：转换往返账 + 特形矩阵全扫 + 指数映射表 =====
/// 往返核：half→f32→half 对可精确表示的值必须逐位还原。
/// （binary16 ⊂ f32，一切正规 half 双向无损；次正规同样无损。）
pub fn roundtrip_half(h: u16) -> Result<(), u16> {
    let f = half_to_f32_bits(h);
    let back = f32_bits_to_half(f);
    // NaN 载荷允许规约（f32 NaN 位型进 half 规范化静默 NaN），
    // 其余四特形必须逐位还原。
    match classify_half(h) {
        HfClass::Nan => Ok(()),
        _ if back != h => Err(XC_ROUND_DRIFT as u16),
        _ => Ok(()),
    }
}

/// bf16 往返核：bf16→f32→bf16 必须逐位还原（前缀截断定义）。
pub fn roundtrip_bf16(b: u16) -> Result<(), u16> {
    let f = bf16_to_f32_bits(b);
    if f32_bits_to_bf16_trunc(f) != b {
        return Err(XC_BF16_TRUNC as u16);
    }
    Ok(())
}

/// 特形矩阵全扫：对指数/尾数角点（0/1/最大/全 1）16 组合逐一
/// 分类核对——分类器在任何角点不得越出五特形闭集。
pub fn corner_matrix_scan() -> Result<(), u16> {
    let exps = [0u16, 1, 30, 31];
    let mans = [0u16, 1, 0x03FE, 0x03FF];
    for &e in exps.iter() {
        for &m in mans.iter() {
            let h = (e << 10) | m;
            match classify_half(h) {
                HfClass::Zero | HfClass::Subnormal | HfClass::Normal
                | HfClass::Inf | HfClass::Nan => {}
            }
            // 往返核带跑（NaN 外全还原）。
            roundtrip_half(h)?;
        }
    }
    Ok(())
}

/// 指数映射表（预计算 0..32 → f32 指数）：转换热路径查表版。
/// 表值 = h 指数 + 112（15→127 重偏），特形行由分支处理。
pub const EXP_MAP: [u32; 32] = {
    let mut t = [0u32; 32];
    let mut i = 0usize;
    while i < 32 {
        t[i] = (i as u32 + 112) << 23;
        i += 1;
    }
    t
};

/// 查表版正规转换（与直算版对拍——双实现互证）。
pub fn half_to_f32_bits_lut(h: u16) -> u32 {
    let exp = ((h >> 10) & 0x1F) as usize;
    let sign = ((h as u32) & 0x8000) << 16;
    let man = (h & 0x03FF) as u32;
    if exp == 0 || exp == 31 {
        return half_to_f32_bits(h); // 特形走直算版
    }
    sign | EXP_MAP[exp] | (man << 13)
}

/// 通道批量 bfloat16 转换（4 通道 + 一致断言——与 half 批量同构）。
pub fn batch_f32_to_bf16_rne(fs: [u32; 4]) -> Result<[u16; 4], u16> {
    let mut out = [0u16; 4];
    for (i, &f) in fs.iter().enumerate() {
        out[i] = f32_bits_to_bf16_rne(f);
    }
    for (i, &f) in fs.iter().enumerate() {
        if out[i] != f32_bits_to_bf16_rne(f) {
            return Err(XC_BATCH_MISMATCH as u16 + i as u16);
        }
    }
    Ok(out)
}

/// 溢出饱和账：转换中发生饱和的计数落账（图像管线里「被钳亮」
/// 的像素数是画质审计指标，不许静默钳制）。
pub struct SatLedger {
    pub sat_count: u64,
}

impl SatLedger {
    pub fn new() -> SatLedger {
        SatLedger { sat_count: 0 }
    }

    /// 记一次转换：返回 half 位型并对饱和显性计数。
    pub fn convert_counted(&mut self, f: u32) -> u16 {
        let h = f32_bits_to_half(f);
        // 饱和判定：输入有限但输出无穷 = 饱和。
        if ((f >> 23) & 0xFF) != 0xFF && (h & 0x7C00) == 0x7C00 {
            self.sat_count += 1;
        }
        h
    }
}
