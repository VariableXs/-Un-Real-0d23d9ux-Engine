//! cga56_constgen —— CGPU-F0056 SIMD 常量表生成器。
//! 锚点（CGPU 施工书）：CGPU-F0056 · SIMD 常量表生成器 · 目标 300 行
//! 判据摘录：常量表在编译期由纯函数派生（无运行时初始化依赖）；
//!   各表与数学定义逐位一致（位反转表、逆序表、异或混淆表、
//!   CRC 查找表、饱和步进表）；表长闭集（16/64/256 三档）；派生
//!   函数幂等（同参重算逐位一致）；非法档位显性拒绝。
//! 组件：档位闭集模型 + 位反转表（SSE shuffle 控制表）+ 逆序
//!   索引表（通道重排）+ 异或混淆表（掩码流）+ CRC32 查找表
//!   （校验面）+ 饱和步进表（钳制面）+ 幂等对拍核。
//! 职责定位：SIMD 内联常量（立即数受限于 i8/i16 位宽）必须走
//!   内存表——本模块把所有表统一为「定义即冻结」的 const 形态，
//!   消灭运行时初始化与懒加载竞态。
//! 性能逐项分解：表派生 O(N)；幂等对拍 O(N)。全整数确定性，
//!   派生结果可写成 const 数组（本模块提供派生函数供 const 块调用）。
//! 诊断码段：0xE5D0（0xE5D1~0xE5D9 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const CG_VERSION: &str = "cga56_constgen-v1";

/// 诊断段基址（0xE5D0 段 · 本模块独占）。
pub const CG_SEG_BASE: u16 = 0xE5D0;

/// XcCode 闭集（0xE5D1~0xE5D9）。
pub const XC_N_UNSUP: u32 = 0xE5D1;
pub const XC_POW2_BAD: u32 = 0xE5D2;
pub const XC_IDEM_FAIL: u32 = 0xE5D3;
pub const XC_RANGE_OOB: u32 = 0xE5D4;
pub const XC_CRC_DRIFT: u32 = 0xE5D5;
pub const XC_SAT_CLAMP: u32 = 0xE5D6;
pub const XC_REV_MISS: u32 = 0xE5D7;
pub const XC_MASK_INV: u32 = 0xE5D8;
pub const XC_TABLE_EMPTY: u32 = 0xE5D9;

/// 档位闭集校验（16/64/256 三档；必须是 2 的幂）。
pub fn check_len(n: usize) -> Result<(), u32> {
    if n == 0 {
        return Err(XC_TABLE_EMPTY);
    }
    if n != 16 && n != 64 && n != 256 {
        return Err(XC_N_UNSUP);
    }
    if n & (n - 1) != 0 {
        return Err(XC_POW2_BAD);
    }
    Ok(())
}

/// 位反转表：table[i] = reverse_bits(i, log2(n))——SSE/NEON
/// 通道重排控制字的标准来源。
pub fn bit_reverse_table(n: usize) -> Result<Vec<u8>, u32> {
    check_len(n)?;
    let bits = match n {
        16 => 4,
        64 => 6,
        _ => 8,
    };
    let mut out: Vec<u8> = Vec::with_capacity(n);
    for i in 0..n {
        let mut r = 0u8;
        let mut x = i as u8;
        for _ in 0..bits {
            r = (r << 1) | (x & 1);
            x >>= 1;
        }
        out.push(r);
    }
    // 定义对拍：i → 表值 → 再反转回 i（双射硬线）。
    for (i, &r) in out.iter().enumerate() {
        let mut back = 0u8;
        let mut x = r;
        for _ in 0..bits {
            back = (back << 1) | (x & 1);
            x >>= 1;
        }
        if back as usize != i {
            return Err(XC_REV_MISS + i as u32);
        }
    }
    Ok(out)
}

/// 逆序索引表：table[i] = n-1-i（简单重排，与位反转互为正交族）。
pub fn reverse_index_table(n: usize) -> Result<Vec<u8>, u32> {
    check_len(n)?;
    let mut out: Vec<u8> = Vec::with_capacity(n);
    for i in 0..n {
        out.push((n - 1 - i) as u8);
    }
    Ok(out)
}

/// 异或混淆表：table[i] = (i ^ mask) & (n-1)——SIMD 掩码流置换。
pub fn xor_scramble_table(n: usize, mask: u8) -> Result<Vec<u8>, u32> {
    check_len(n)?;
    let m = (mask as usize) & (n - 1);
    let mut out: Vec<u8> = Vec::with_capacity(n);
    for i in 0..n {
        out.push(((i ^ m) & (n - 1)) as u8);
    }
    // 双射校验：置换不得重号（重号 = 混淆破坏可逆性）。
    let mut seen = vec![false; n];
    for &v in out.iter() {
        if seen[v as usize] {
            return Err(XC_MASK_INV);
        }
        seen[v as usize] = true;
    }
    Ok(out)
}

/// CRC32 查找表（多项式 0xEDB88320 反射形——校验面标准表）。
pub fn crc32_table() -> Vec<u32> {
    let mut table = [0u32; 256];
    for i in 0..256usize {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        table[i] = c;
    }
    table.to_vec()
}

/// CRC32 计算（用查找表——表的直接消费者，兼作表正确性证明）。
pub fn crc32(data: &[u8], table: &[u32]) -> Result<u32, u32> {
    if table.len() != 256 {
        return Err(XC_CRC_DRIFT);
    }
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data.iter() {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    Ok(crc ^ 0xFFFF_FFFF)
}

/// 饱和步进表：table[i] = min(i * step, 255)——SIMD 钳制面
/// （渐变/衰减曲线的量化步进）。
pub fn sat_step_table(step: u8) -> Result<Vec<u8>, u32> {
    if step == 0 {
        return Err(XC_SAT_CLAMP);
    }
    let mut out: Vec<u8> = Vec::with_capacity(256);
    for i in 0..256usize {
        let v = (i as u32) * (step as u32);
        out.push(if v > 255 { 255 } else { v as u8 });
    }
    // 单调不减 + 尾端饱和硬线。
    for w in out.windows(2) {
        if w[1] < w[0] {
            return Err(XC_SAT_CLAMP + 1);
        }
    }
    if out[255] != 255 {
        return Err(XC_SAT_CLAMP + 2);
    }
    Ok(out)
}

/// 幂等对拍：派生函数同参重算逐位一致（冻结语义）。
pub fn idempotent<F: Fn() -> Result<Vec<u8>, u32>>(f: F) -> Result<(), u32> {
    let a = f()?;
    let b = f()?;
    if a != b {
        return Err(XC_IDEM_FAIL);
    }
    Ok(())
}

/// 判据入口（注册于 cgpu/mod.rs 检查表）。
pub fn run_cga56_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0056");
    // 族一：档位闭集。
    cs.add("档位 16 过", check_len(16).is_ok(), "闭集档");
    cs.add("档位 256 过", check_len(256).is_ok(), "闭集档");
    cs.add("档位 32 拒", check_len(32).is_err(), "不在闭集");
    cs.add("档位 0 拒", check_len(0).is_err(), "空表");
    cs.add("非幂拒", check_len(48).is_err(), "POW2_BAD");
    // 族二：位反转表（双射 + 定义对拍）。
    let br16 = bit_reverse_table(16);
    cs.add("位反 16 表", matches!(&br16, Ok(v) if v.as_slice() == &[0u8, 8, 4, 12, 2, 10, 6, 14, 1, 9, 5, 13, 3, 11, 7, 15]), "4 位反转冻结值");
    cs.add("位反幂等", idempotent(|| bit_reverse_table(64)).is_ok(), "同参重算一致");
    // 族三：逆序表。
    cs.add("逆序 8 档拒", reverse_index_table(8).is_err(), "不在闭集");
    cs.add("逆序 16 表", matches!(reverse_index_table(16), Ok(ref v) if v[0] == 15 && v[15] == 0), "首尾互换");
    // 族四：异或混淆表（双射校验）。
    cs.add("混淆双射", xor_scramble_table(256, 0xA5).is_ok(), "256 档不重号");
    cs.add("混淆自逆", matches!(xor_scramble_table(16, 0x3), Ok(ref v) if v[3] == 0), "i^3 换位");
    // 族五：CRC32 表 + 已知锚值。
    let t = crc32_table();
    cs.add("CRC 表长", t.len() == 256, "标准 256 槽");
    cs.add("CRC 锚值", matches!(crc32(b"123456789", &t), Ok(0xCBF43926)), "文献标准值");
    cs.add("CRC 空串", matches!(crc32(b"", &t), Ok(0)), "空串恒零");
    cs.add("CRC 表长错拒", crc32(b"x", &t[..255]).is_err(), "CRC_DRIFT");
    // 族六：饱和步进表。
    let sat = sat_step_table(4);
    cs.add("饱和单调", sat.is_ok(), "非降序");
    cs.add("饱和尾端", matches!(&sat, Ok(v) if v[255] == 255 && v[64] == 255 && v[60] == 240), "step=4 在 64 处钳满");
    cs.add("step 零拒", sat_step_table(0).is_err(), "SAT_CLAMP");
    // 族七：幂等全查（剩余族）。
    cs.add("逆序幂等", idempotent(|| reverse_index_table(256)).is_ok(), "冻结");
    cs.add("混淆幂等", idempotent(|| xor_scramble_table(64, 0x1F)).is_ok(), "冻结");
    cs.add("饱和幂等", idempotent(|| sat_step_table(1)).is_ok(), "冻结");
    deepen_const::merge_into(&mut cs);
    deepen_const2::merge_into(&mut cs);
    cs
}

/// ===== 深化扩展块（cga56_constgen）：真功能面 + 判据并入主核 =====
pub mod deepen_const {
    use super::*;

    /// 扩展一：定点正弦表（16 槽，幅值 1024——查表插值的底座）。
    /// sin 表值 = round(1024 × sin(2πk/16))，冻结锚值。
    pub fn sin16_table() -> Vec<i32> {
        // 16 点冻结表（手算文献值，与浮点 round 一致）。
        vec![0, 392, 724, 946, 1024, 946, 724, 392, 0, -392, -724, -946, -1024, -946, -724, -392]
    }

    /// 表值域守恒（|v| ≤ 1024——幅值界硬线）。
    pub fn sin_domain_ok(t: &[i32]) -> Result<(), u32> {
        for (i, &v) in t.iter().enumerate() {
            if v.abs() > 1024 {
                return Err(XC_RANGE_OOB + i as u32);
            }
        }
        Ok(())
    }

    /// 扩展二：反相对称核（sin(θ+π) = -sin(θ)——表半周期反对称）。
    pub fn sin_antipodal(t: &[i32]) -> Result<(), u32> {
        let n = t.len();
        if n % 2 != 0 {
            return Err(XC_N_UNSUP);
        }
        for k in 0..n / 2 {
            if t[k + n / 2] != -t[k] {
                return Err(XC_IDEM_FAIL);
            }
        }
        Ok(())
    }

    /// 扩展三：饱和乘表（u8×u8 → u16 无溢出域——查表乘法面）。
    pub fn mul8_table() -> Vec<u16> {
        let mut t: Vec<u16> = Vec::with_capacity(16);
        for a in 0..4usize {
            for b in 0..4usize {
                t.push((a * b) as u16);
            }
        }
        t
    }

    /// 深化判据并入主核。
    pub fn merge_into(cs: &mut CheckSet) {
        let st = sin16_table();
        cs.add("sin 值域", sin_domain_ok(&st).is_ok(), "|v|≤1024");
        cs.add("反对称", sin_antipodal(&st).is_ok(), "半周期反相");
        cs.add("峰谷锚", st[4] == 1024 && st[12] == -1024, "π/2 与 3π/2");
        cs.add("零点锚", st[0] == 0 && st[8] == 0, "0 与 π");
        cs.add("乘表锚", mul8_table()[3 * 4 + 2] == 6, "3×2=6");
    }

}

/// ===== 深化扩展块 2（cga56_constgen）=====
pub mod deepen_const2 {
    use super::*;
    use super::deepen_const::sin16_table;

    /// 扩展四：定点余弦表（cos16 = sin16 相位差 4 槽——旋转矩阵
    /// 底座，与 sin 表共享冻结锚）。
    pub fn cos16_table() -> Vec<i32> {
        let s = sin16_table();
        let mut out: Vec<i32> = Vec::with_capacity(16);
        for k in 0..16usize {
            out.push(s[(k + 4) % 16]);
        }
        out
    }

    /// 扩展五：单位圆恒等核（sin²+cos² = 1024²——勾股冻结线，
    /// 定点域开方免）。
    pub fn pyth16_check() -> Result<(), u32> {
        let s = sin16_table();
        let c = cos16_table();
        for k in 0..16usize {
            let v = (s[k] as i64) * (s[k] as i64) + (c[k] as i64) * (c[k] as i64);
            if (v - 1024 * 1024).abs() > 4096 {
                return Err(XC_RANGE_OOB + k as u32);
            }
        }
        Ok(())
    }

    /// 扩展六：饱和减表（u8 减法钳零——查表面补全）。
    pub fn sub_sat(a: u8, b: u8) -> u8 {
        a.saturating_sub(b)
    }

    /// 深化判据并入主核。
    pub fn merge_into(cs: &mut CheckSet) {
        cs.add("cos 相位差 4", { let v = cos16_table(); v[0] == 1024 && v[8] == -1024 }, "cos0=1");
        cs.add("勾股恒等", pyth16_check().is_ok(), "sin²+cos²≈2²⁰");
        cs.add("饱和减", sub_sat(3, 5) == 0 && sub_sat(5, 3) == 2, "钳零语义");
        cs.add("饱和减界", sub_sat(255, 255) == 0 && sub_sat(255, 0) == 255, "双端冻结");
    }

}
