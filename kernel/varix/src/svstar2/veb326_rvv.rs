//! veb326_rvv —— VE-F0326 RVV 向量扩展 CPU 加速。
//! 锚点（CoRun Unxreal 主册）：VE-F0326 · RVV 向量扩展 CPU 加速 · 目标 320 行
//! 判据原文：VE-F0326-J1 RVV 加速账：向量长档（VLEN 128/256/
//!           512 三档）；向量核（逐元素加/乘纯函数，VL 段切
//!           分）；段切分核（n 元素 = ceil(n / (VLEN/ewidth))
//!           段）；RVV 加速比（标量周期 vs 向量周期：加速
//!           = 标量/向量，封顶 8）；不可用回退（RVV 旗缺 →
//!           走标量路径，加速比 1）；段尾残组核（尾段不满
//!           组也计一段）；加速封顶核（理论加速超 8 按 8 记）。
//! 组件：向量长档 + 段切分器 + 加速比账。
//! 职责定位：RVV 是「RISC-V 的group 心算术」——VLEN 是心算
//!           的位数：一次记 8 个数总比记 8 次快；段切分是心
//!           算的分组：31 个数按 8 个一组要 4 组，尾组不满
//!           也算一组；加速封顶是心算的诚实：纸上算得再快
//!           也封个顶。
//! 性能逐项分解：切分 O(1)；向量核 O(VL)。
//! 诊断码段：0x31A80（0x31A81~0x31A89 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const VV_VERSION: &str = "veb326_rvv-v1";

/// 诊断段基址（0x31A80 段 · 本模块独占）。
pub const VV_SEG_BASE: u32 = 0x31A80;
pub const XC_MAGIC: u32 = 0x31A81; // 账魔数破
pub const XC_VLEN: u32 = 0x31A82; // VLEN 不合法
pub const XC_EW: u32 = 0x31A83; // 元素宽不合法
pub const XC_ZERO: u32 = 0x31A84; // 零元素

/// 账魔数。
const MAGIC: u32 = 0x31A8_1B2D;

/// 合法 VLEN。
pub const VLENS: [u32; 3] = [128, 256, 512];

/// 合法元素宽（位）。
pub const EWS: [u32; 3] = [8, 16, 32];

/// 加速封顶。
pub const SPEED_CAP: u32 = 8;

/// VLEN 合法？
pub fn vlen_ok(v: u32) -> bool {
    for x in VLENS.iter() {
        if *x == v {
            return true;
        }
    }
    false
}

/// 元素宽合法？
pub fn ew_ok(e: u32) -> bool {
    for x in EWS.iter() {
        if *x == e {
            return true;
        }
    }
    false
}

/// 段切分：组宽 = VLEN / ewidth；段数 = ceil(n / 组宽)。
pub fn segments(n: u32, vlen: u32, ew: u32) -> Result<u32, u32> {
    if !vlen_ok(vlen) {
        return Err(XC_VLEN);
    }
    if !ew_ok(ew) {
        return Err(XC_EW);
    }
    if n == 0 {
        return Err(XC_ZERO);
    }
    let grp = vlen / ew;
    Ok((n + grp - 1) / grp)
}

/// 向量加核：逐元素 a+b（段切分模拟，返回校验和）。
pub fn vadd_checksum(a: &[u32], b: &[u32], vlen: u32, ew: u32) -> Result<u32, u32> {
    if !vlen_ok(vlen) {
        return Err(XC_VLEN);
    }
    if !ew_ok(ew) {
        return Err(XC_EW);
    }
    if a.len() != b.len() || a.is_empty() {
        return Err(XC_ZERO);
    }
    let segs = segments(a.len() as u32, vlen, ew)?;
    let mut sum = segs * 7919; // 段开销模型
    for i in 0..a.len() {
        sum = sum.wrapping_add(a[i].wrapping_add(b[i]));
    }
    Ok(sum)
}

/// 标量核：同长度标量校验和（无段开销，每元素 +13 周期模型）。
pub fn scalar_checksum(a: &[u32], b: &[u32]) -> Result<u32, u32> {
    if a.len() != b.len() || a.is_empty() {
        return Err(XC_ZERO);
    }
    let mut sum = (a.len() as u32) * 13;
    for i in 0..a.len() {
        sum = sum.wrapping_add(a[i].wrapping_add(b[i]));
    }
    Ok(sum)
}

/// 加速比：标量周期 / 向量周期，封顶 8。向量周期模型：理想
/// 发射每元素 1 周期 + 每段 3 周期段开销。
pub fn speedup(n: u32, vlen: u32, ew: u32) -> Result<u32, u32> {
    if !vlen_ok(vlen) {
        return Err(XC_VLEN);
    }
    if !ew_ok(ew) {
        return Err(XC_EW);
    }
    if n == 0 {
        return Err(XC_ZERO);
    }
    let scalar_cyc = n * 13;
    let grp = vlen / ew;
    let segs = (n + grp - 1) / grp;
    let vec_cyc = n + segs * 3;
    if vec_cyc == 0 {
        return Ok(SPEED_CAP);
    }
    let sp = scalar_cyc / vec_cyc;
    Ok(if sp > SPEED_CAP { SPEED_CAP } else { sp })
}

pub fn run_veb326_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0326");
    // 族零：合法档。
    s.add("VLEN 128 合", vlen_ok(128), "档 1");
    s.add("VLEN 256 合", vlen_ok(256), "档 2");
    s.add("VLEN 512 合", vlen_ok(512), "档 3");
    s.add("VLEN 64 拒", vlen_ok(64) == false, "非法");
    s.add("EW 8 合", ew_ok(8), "窄");
    s.add("EW 32 合", ew_ok(32), "宽");
    s.add("EW 64 拒", ew_ok(64) == false, "非法");
    // 族一：段切分。
    // 32 位元素 128 位档：组宽 4 → 16 元素 4 段。
    s.add("16 元 4 段", segments(16, 128, 32) == Ok(4), "恰组");
    // 17 元素 → 5 段（尾残组）。
    s.add("17 元 5 段", segments(17, 128, 32) == Ok(5), "尾残");
    // 8 位元素 512 位档：组宽 64 → 128 元素 2 段。
    s.add("128 元 2 段", segments(128, 512, 8) == Ok(2), "宽档");
    // 129 → 3 段。
    s.add("129 元 3 段", segments(129, 512, 8) == Ok(3), "越一");
    // 单元素 1 段。
    s.add("1 元 1 段", segments(1, 128, 32) == Ok(1), "最小");
    s.add("零元拒", segments(0, 128, 32) == Err(XC_ZERO), "无活干");
    s.add("VLEN 100 拒", segments(10, 100, 32) == Err(XC_VLEN), "非法档");
    s.add("EW 64 拒", segments(10, 128, 64) == Err(XC_EW), "非法宽");
    // 族二：向量核。
    let a = [1u32, 2, 3, 4];
    let b = [10u32, 20, 30, 40];
    // 标量和 = 110，4 元素 ×13 = 52 开销 → 162。
    s.add("标量 162", scalar_checksum(&a, &b) == Ok(162), "周期模型");
    // 向量：4 元素 128/32 → 1 段，段开销 7919，和 110 → 8029。
    s.add("向量 8029", vadd_checksum(&a, &b, 128, 32) == Ok(8029), "段开销");
    // 长不齐拒。
    let c = [1u32, 2, 3];
    s.add("长不齐拒", vadd_checksum(&a, &c, 128, 32) == Err(XC_ZERO), "配对");
    s.add("空拒", vadd_checksum(&[], &[], 128, 32) == Err(XC_ZERO), "无元素");
    // 族三：加速比。
    // n=16，128/32：标量 208；向量 16 + 4×3 = 28；208/28 = 7。
    s.add("加速 7", speedup(16, 128, 32) == Ok(7), "模型比");
    // 封顶：n=4096，512/8 组宽 64：向量 4096+192 = 4288；标量 53248；12→封 8。
    s.add("封顶 8", speedup(4096, 512, 8) == Ok(8), "cap");
    // 窄档 n=4096，128/32 组宽 4：向量 4096+3072 = 7168；比 7.4 → 7。
    s.add("窄档 7", speedup(4096, 128, 32) == Ok(7), "未封");
    s.add("零元拒速", speedup(0, 128, 32) == Err(XC_ZERO), "无活");
    s.add("档拒速", speedup(10, 100, 32) == Err(XC_VLEN), "非法");
    // 族四：宽档加速更高（组宽大段数少）。
    let sp128 = speedup(64, 128, 32);
    let sp512 = speedup(64, 512, 32);
    s.add("宽档更快", sp512 >= sp128, "段少");
    // 族五：窄元素段多。
    s.add("窄元段多", segments(64, 128, 8) == Ok(8) && segments(64, 128, 32) == Ok(2), "组宽差");
    s.add("魔数核", VV_SEG_BASE == 0x31A80, "段基址");
    s
}
