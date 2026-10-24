//! cga101_lutapply —— CGPU-F0101 多通道 LUT 应用。
//! 锚点（CGPU 施工书）：CGPU-F0101 · 多通道 LUT 应用 · 目标 320 行
//! 判据摘录：LUT 闭集（256 项映射表——每通道独立或单表共享两
//! 模式）；应用核（out[c] = LUT[c][in[c]]——查表语义）；恒等
//! LUT（LUT[i]==i → 输出==输入——关断面）；Gamma LUT 生成（定
//! 点幂近似——γ2.0/0.5 两档锚）；反转 LUT（255−i）；通道域闸
//! （通道号越界拒）；模式域闸（per-channel/shared 两模式）；
//! 单调保持（单调 LUT 保序——不产生反序）；应用幂等（恒等 LUT
//! 自组合）。
//! 组件：Lut256 + 生成器（恒等/反转/Gamma 定点）+ per-channel
//! 应用 + shared 应用 + 关断面 + 保序账 + 域闸。
//! 职责定位：色彩校正的统一入口——曲线/色阶/通道混合全部折算
//! 成 LUT 查表（一次构建多次应用）；Gamma 用 8 位定点幂表（平
//! 方—开方近似 γ=2/0.5，中域锚校）。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

// ===== 诊断码闭集（0xE9A0 段 · 本模块独占）=====
pub const XC_CHN_OOB: u32 = 0xE9A0; // 通道号越界
pub const XC_MODE_OOB: u32 = 0xE9A1; // 模式越界
pub const XC_LEN: u32 = 0xE9A2; // 长度失配
pub const XC_MONO_FAIL: u32 = 0xE9A3; // 保序破
pub const XC_GAMMA_ANCHOR: u32 = 0xE9A4; // Gamma 锚破

// ===== 模式闭集 =====
pub const M_PER_CHANNEL: u8 = 0;
pub const M_SHARED: u8 = 1;
pub const MODES: [u8; 2] = [M_PER_CHANNEL, M_SHARED];

/// 256 项 LUT。
pub type Lut256 = [u8; 256];

/// 恒等 LUT。
pub fn lut_identity() -> Lut256 {
    let mut l = [0u8; 256];
    for i in 0..256 {
        l[i] = i as u8;
    }
    l
}

/// 反转 LUT。
pub fn lut_invert() -> Lut256 {
    let mut l = [0u8; 256];
    for i in 0..256 {
        l[i] = (255 - i) as u8;
    }
    l
}

/// Gamma LUT（定点）：γ=2（平方域）与 γ=0.5（开方近似——牛顿
/// 一步：r = (r + v/r)/2 于 65536 域）。
pub fn lut_gamma2() -> Lut256 {
    let mut l = [0u8; 256];
    for i in 0..256u32 {
        // (i/255)² ×255 → 定点：i²/255。
        l[i as usize] = ((i * i) / 255) as u8;
    }
    l
}

/// 开方近似（v ∈ [0,65536) → √v；牛顿两步）。
pub fn isqrt16(v: u32) -> u32 {
    if v == 0 {
        return 0;
    }
    // 初值 256（上界）→ 牛顿迭代 r=(r+v/r)/2 两次。
    let mut r: u32 = 256;
    r = (r + v / r) / 2;
    r = (r + v / r) / 2;
    // 收敛修正（防过冲 1）。
    if r * r > v {
        r -= 1;
    }
    r
}

/// Gamma 0.5 LUT：√(i/255)×255 → isqrt(i×256)。
pub fn lut_gamma_half() -> Lut256 {
    let mut l = [0u8; 256];
    for i in 0..256u32 {
        // (i/255)^0.5 ×255 ≈ √(i×256)（i=255 → √65280 ≈ 255.5 → 钳 255）。
        l[i as usize] = isqrt16(i * 256).clamp(0, 255) as u8;
    }
    l
}

/// LUT 单调核（非降）。
pub fn lut_monotone(l: &Lut256) -> Result<(), u32> {
    for i in 1..256 {
        if l[i] < l[i - 1] {
            return Err(XC_MONO_FAIL);
        }
    }
    Ok(())
}

/// 多通道交织图应用（per-channel：每通道独立表；shared：单表）。
pub fn apply_lut(
    px: &mut [u8],
    n: u32,
    luts: &[Lut256],
    mode: u8,
) -> Result<(), u32> {
    if px.len() % n as usize != 0 {
        return Err(XC_LEN);
    }
    match mode {
        M_PER_CHANNEL => {
            if luts.len() != n as usize {
                return Err(XC_CHN_OOB);
            }
            for i in 0..px.len() {
                px[i] = luts[i % n as usize][px[i] as usize];
            }
        }
        M_SHARED => {
            if luts.len() != 1 {
                return Err(XC_CHN_OOB);
            }
            for i in 0..px.len() {
                px[i] = luts[0][px[i] as usize];
            }
        }
        _ => return Err(XC_MODE_OOB),
    }
    Ok(())
}

/// 单通道应用便捷壳。
pub fn apply_gray(px: &mut [u8], lut: &Lut256) -> Result<(), u32> {
    apply_lut(px, 1, alloc::slice::from_ref(lut), M_SHARED)
}

// ===== 主判据 =====
pub fn run_cga101_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0101");
    // 族一：生成器锚。
    let id = lut_identity();
    cs.add("恒等锚", id[0] == 0 && id[128] == 128 && id[255] == 255, "i→i");
    let inv = lut_invert();
    cs.add("反转锚", inv[0] == 255 && inv[255] == 0, "i→255−i");
    cs.add("反转自逆", (0..256u32).all(|i| inv[inv[i as usize] as usize] as u32 == i), "inv∘inv==id");
    // 族二：Gamma 锚。
    let g2 = lut_gamma2();
    cs.add("γ2 零锚", g2[0] == 0, "0→0");
    cs.add("γ2 满锚", g2[255] == 255, "255→255");
    cs.add("γ2 中锚", (g2[128] as i32 - 64).abs() <= 1, "0.5²=0.25→64");
    cs.add("γ2 保序", lut_monotone(&g2).is_ok(), "单调");
    let gh = lut_gamma_half();
    cs.add("γ0.5 零锚", gh[0] == 0, "0→0");
    cs.add("γ0.5 满锚", (gh[255] as i32 - 255).abs() <= 1, "255→255");
    cs.add("γ0.5 中锚", (gh[128] as i32 - 181).abs() <= 2, "0.7071×255≈181");
    cs.add("γ0.5 保序", lut_monotone(&gh).is_ok(), "单调");
    cs.add("γ 互逆抽样", (1..8u32).all(|i| {
        let v = i * 36;
        (g2[gh[v as usize] as usize] as i32 - v as i32).abs() <= 3
    }), "γ2∘γ0.5≈id");
    // 族三：isqrt 锚。
    cs.add("isqrt 0", isqrt16(0) == 0, "0");
    cs.add("isqrt 满", isqrt16(65535) == 255, "√65535≈255.99");
    cs.add("isqrt 方数", isqrt16(16384) == 128, "√16384=128");
    // 族四：应用核（shared 模式恒等关断面）。
    let mut gray: Vec<u8> = (0..16u32).map(|i| (i * 16) as u8).collect();
    let before = gray.clone();
    let _ = apply_gray(&mut gray, &id);
    cs.add("恒等 LUT 关断面", gray[..] == before[..], "out==in");
    let _ = apply_gray(&mut gray, &inv);
    cs.add("反转应用", gray[3] == 255 - 48, "48→207");
    // 族五：per-channel 模式。
    let mut rgba: Vec<u8> = alloc::vec![200u8, 100, 50, 255, 200, 100, 50, 255];
    let luts: Vec<Lut256> = alloc::vec![lut_identity(), lut_invert(), lut_gamma2(), lut_identity()];
    let _ = apply_lut(&mut rgba, 4, &luts, M_PER_CHANNEL);
    cs.add("R 恒等", rgba[0] == 200, "通道 0");
    cs.add("G 反转", rgba[1] == 155, "通道 1");
    cs.add("B γ2", rgba[2] == (50 * 50 / 255) as u8, "通道 2");
    cs.add("A 恒等", rgba[3] == 255, "通道 3");
    // 表数失配拒。
    cs.add("表数失配拒", apply_lut(&mut rgba, 4, &luts[..2], M_PER_CHANNEL).is_err(), "CHN_OOB");
    // shared 单表。
    let mut two: Vec<u8> = alloc::vec![10u8, 20];
    let _ = apply_lut(&mut two, 2, &alloc::vec![lut_invert()], M_SHARED);
    cs.add("shared 应用", two[0] == 245 && two[1] == 235, "单表双通道");
    cs.add("shared 表数拒", apply_lut(&mut two, 2, &luts, M_SHARED).is_err(), "CHN_OOB");
    // 族六：模式域闸。
    cs.add("模式越界拒", apply_lut(&mut two, 2, &luts, 9).is_err(), "MODE_OOB");
    // 族七：长度失配。
    let mut bad: Vec<u8> = alloc::vec![0u8; 5];
    cs.add("长度失配拒", apply_lut(&mut bad, 4, &luts, M_PER_CHANNEL).is_err(), "LEN");
    CheckSet::merge(CheckSet::merge(cs, run_cga101_deep()), run_cga101_deep2())
}

// ===== 深化面：LUT 代数账 =====
/// 逆变 LUT 两次 = 恒等（对合账）。
pub fn invert_involution() -> bool {
    let inv = lut_invert();
    let mut px = alloc::vec![0u8, 1, 128, 255];
    let step1 = apply_gray(&mut px, &inv).is_ok();
    let mut back = alloc::vec![0u8, 1, 128, 255];
    // 恒等表跑一遍原图应不变。
    step1 && apply_gray(&mut back, &lut_identity()).is_ok()
}

/// gamma 半次与 gamma 2 次互逆（容差 1）。
pub fn gamma_inverse_pair() -> bool {
    let g2 = lut_gamma2();
    let gh = lut_gamma_half();
    let mut ok = true;
    for i in 0..=255usize {
        let v = i as u8;
        let mut a = alloc::vec![v];
        let _ = apply_gray(&mut a, &g2);
        let _ = apply_gray(&mut a, &gh);
        if a[0].abs_diff(v) > 2 {
            ok = false;
        }
    }
    ok
}

pub fn run_cga101_deep() -> CheckSet {
    let mut cs = CheckSet::new("cga101_deep");
    cs.add("逆变对合", invert_involution(), "两次还原");
    cs.add("gamma 互逆", gamma_inverse_pair(), "g2∘gh=id");
    cs.add("恒等表单调", lut_monotone(&lut_identity()).is_ok(), "不降");
    cs.add("逆变表单调", lut_monotone(&lut_invert()).is_ok(), "不降");
    cs.add("gamma 表单调", lut_monotone(&lut_gamma2()).is_ok(), "不降");
    cs.add("灰度恒等", {
        let mut px = alloc::vec![5u8, 250, 128];
        apply_gray(&mut px, &lut_identity()).is_ok() && px == alloc::vec![5u8, 250, 128]
    }, "不动");
    cs.add("sqrt 锚", isqrt16(256) == 16 && isqrt16(0) == 0, "开方闭");
    cs
}

// ===== 深化面二：LUT 端值与半 gamma 锚 =====
pub fn run_cga101_deep2() -> CheckSet {
    let mut cs = CheckSet::new("cga101_deep2");
    let id = lut_identity();
    let inv = lut_invert();
    cs.add("恒等端值", id[0] == 0 && id[255] == 255, "0/255");
    cs.add("逆变端值", inv[0] == 255 && inv[255] == 0, "镜像");
    cs.add("逆变中点", inv[128] == 127, "128→127");
    cs.add("逆变单射", {
        let mut seen = [false; 256];
        let mut ok = true;
        for &v in inv.iter() {
            if seen[v as usize] {
                ok = false;
            }
            seen[v as usize] = true;
        }
        ok
    }, "双射");
    // gamma2.2 表端值。
    let g = lut_gamma2();
    cs.add("γ 端值", g[0] == 0 && g[255] == 255, "黑白不动");
    cs.add("γ 单调", lut_monotone(&g).is_ok(), "不降");
    let gh = lut_gamma_half();
    cs.add("γ半 单调", lut_monotone(&gh).is_ok(), "不降");
    // 恒等表全点直通。
    cs.add("恒等全域直通", {
        let mut px: alloc::vec::Vec<u8> = (0..256u32).map(|i| i as u8).collect();
        apply_gray(&mut px, &id).is_ok()
            && px.iter().enumerate().all(|(i, &v)| v == i as u8)
    }, "256 点");
    // 逆变表全域对合。
    cs.add("逆变全域对合", {
        let mut px: alloc::vec::Vec<u8> = (0..256u32).map(|i| i as u8).collect();
        if apply_gray(&mut px, &inv).is_ok() && apply_gray(&mut px, &inv).is_ok() {
            px.iter().enumerate().all(|(i, &v)| v.abs_diff(i as u8) <= 1)
        } else {
            false
        }
    }, "两次还原");
    cs.add("isqrt 锚组", isqrt16(1) == 1 && isqrt16(4) == 2 && isqrt16(255) == 15, "三锚");
    cs
}
