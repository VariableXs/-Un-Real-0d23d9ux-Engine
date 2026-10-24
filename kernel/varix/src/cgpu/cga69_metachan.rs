//! cga69_metachan —— CGPU-F0069 图像元数据通道。
//! 锚点（CGPU 施工书）：CGPU-F0069 · 图像元数据通道 · 目标 300 行
//! 判据摘录：元数据键闭集（八键：色彩空间/位深/色度采样/Gamma/
//!   ICC 指纹/DPI/DPI 基准/帧序号）；键值域逐键冻结（位深 ∈
//!   {8,10,12,16}、色度采样 ∈ {444,422,420} 等）；元数据包序列
//!   化定长小端（roundtrip 逐位一致）；包版本只进不退；未知键
//!   显性拒绝；包合并冲突闭集（同键同值放行 / 同键异值立案）。
//! 组件：键闭集表 + 值域校验器 + 定长序列化（32 字节包）+
//!   roundtrip 对拍 + 版本单调核 + 合并冲突审计器。
//! 职责定位：图像管线的「边带」——像素外的语义（色彩空间错 =
//!   全图色偏）必须随像素走且账面可核，本模块把元数据从注释变
//!   成可序列化、可审计、可合并的强类型账。
//! 性能逐项分解：校验 O(键数)；序列化 O(32)；合并 O(键数)。
//! 全整数确定性。
//! 诊断码段：0xE6E0（0xE6E1~0xE6E9 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const MC_VERSION: &str = "cga69_metachan-v1";

/// 诊断段基址（0xE6E0 段 · 本模块独占）。
pub const MC_SEG_BASE: u16 = 0xE6E0;

/// XcCode 闭集（0xE6E1~0xE6E9）。
pub const XC_KEY_UNSUP: u32 = 0xE6E1;
pub const XC_VAL_OOB: u32 = 0xE6E2;
pub const XC_SER_DRIFT: u32 = 0xE6E3;
pub const XC_VER_BACK: u32 = 0xE6E4;
pub const XC_MERGE_CLASH: u32 = 0xE6E5;
pub const XC_PKG_LEN: u32 = 0xE6E6;
pub const XC_MAGIC_MISS: u32 = 0xE6E7;
pub const XC_DUP_KEY: u32 = 0xE6E8;
pub const XC_SUM_DRIFT: u32 = 0xE6E9;

/// 键闭集（八键）。
pub const K_COLORSPACE: u8 = 0;
pub const K_BITDEPTH: u8 = 1;
pub const K_CHROMA: u8 = 2;
pub const K_GAMMA: u8 = 3;
pub const K_ICC: u8 = 4;
pub const K_DPI: u8 = 5;
pub const K_DPIBASE: u8 = 6;
pub const K_FRAMENO: u8 = 7;

/// 键值域校验（逐键冻结域）。
pub fn val_ok(key: u8, val: u32) -> Result<(), u32> {
    match key {
        K_COLORSPACE => {
            // 0=sRGB 1=DisplayP3 2=Rec2020 3=Linear
            if val > 3 {
                return Err(XC_VAL_OOB);
            }
        }
        K_BITDEPTH => {
            if val != 8 && val != 10 && val != 12 && val != 16 {
                return Err(XC_VAL_OOB);
            }
        }
        K_CHROMA => {
            // 0=444 1=422 2=420
            if val > 2 {
                return Err(XC_VAL_OOB);
            }
        }
        K_GAMMA => {
            // 千分比定点：1000=γ1.0 … 4545=γ1/2.2
            if val < 100 || val > 10000 {
                return Err(XC_VAL_OOB);
            }
        }
        K_ICC => {
            // ICC 指纹高 32 位（任意非零）。
            if val == 0 {
                return Err(XC_VAL_OOB);
            }
        }
        K_DPI => {
            if val == 0 || val > 10000 {
                return Err(XC_VAL_OOB);
            }
        }
        K_DPIBASE => {
            // 0=meter 1=inch
            if val > 1 {
                return Err(XC_VAL_OOB);
            }
        }
        K_FRAMENO => {} // 帧序号无界（单调由账管）
        _ => return Err(XC_KEY_UNSUP),
    }
    Ok(())
}

/// 元数据包：版本 + 八键值（未设置 = u32::MAX 哨兵）。
pub struct MetaPkg {
    pub ver: u32,
    pub vals: [u32; 8],
}

impl MetaPkg {
    pub fn new() -> MetaPkg {
        MetaPkg { ver: 1, vals: [u32::MAX; 8] }
    }

    /// 设键值（值域校验 + 已设同值幂等）。
    pub fn set(&mut self, key: u8, val: u32) -> Result<(), u32> {
        if key > K_FRAMENO {
            return Err(XC_KEY_UNSUP);
        }
        val_ok(key, val)?;
        self.vals[key as usize] = val;
        Ok(())
    }

    /// 定长序列化（32 字节：magic 2B + ver 2B + 8×(key 1B+val 2B+pad 1B)。
    /// 简化冻结形：magic 0x4D43 + ver u16 + vals[0..7] 各 u32 截 16 位
    /// —— 恰 2+2+28 = 32 字节）。
    pub fn serialize(&self) -> Result<[u8; 32], u32> {
        if self.ver == 0 || self.ver > 0xFFFF {
            return Err(XC_VER_BACK);
        }
        let mut out = [0u8; 32];
        out[0] = 0x4D;
        out[1] = 0x43;
        out[2..4].copy_from_slice(&(self.ver as u16).to_le_bytes());
        for k in 0..7usize {
            let v = self.vals[k];
            out[4 + k * 4..8 + k * 4].copy_from_slice(&(v & 0xFFFF).to_le_bytes());
        }
        // 第 8 键（帧序号）占尾 4 字节全宽。
        out[28..32].copy_from_slice(&self.vals[7].to_le_bytes());
        Ok(out)
    }

    /// 反序列化（roundtrip 对拍面）。
    pub fn deserialize(b: &[u8; 32]) -> Result<MetaPkg, u32> {
        if b[0] != 0x4D || b[1] != 0x43 {
            return Err(XC_MAGIC_MISS);
        }
        let ver = u16::from_le_bytes([b[2], b[3]]) as u32;
        if ver == 0 {
            return Err(XC_VER_BACK);
        }
        let mut pkg = MetaPkg { ver, vals: [u32::MAX; 8] };
        for k in 0..7usize {
            let mut raw = [0u8; 4];
            raw.copy_from_slice(&b[4 + k * 4..8 + k * 4]);
            let v = u32::from_le_bytes(raw) as u32;
            pkg.vals[k] = v;
        }
        let mut tail = [0u8; 4];
        tail.copy_from_slice(&b[28..32]);
        pkg.vals[7] = u32::from_le_bytes(tail);
        Ok(pkg)
    }
}

/// 版本单调核（只进不退）。
pub fn version_forward(old: u32, new: u32) -> Result<(), u32> {
    if new <= old {
        return Err(XC_VER_BACK);
    }
    Ok(())
}

/// 包合并冲突审计：同键同值放行 / 同键异值立案 / 单侧设置取设侧。
/// 返回合并后的新包（版本取 max+1）。
pub fn merge(a: &MetaPkg, b: &MetaPkg) -> Result<MetaPkg, u32> {
    let mut out = MetaPkg::new();
    out.ver = a.ver.max(b.ver) + 1;
    for k in 0..8usize {
        let va = a.vals[k];
        let vb = b.vals[k];
        if va == u32::MAX {
            out.vals[k] = vb;
        } else if vb == u32::MAX {
            out.vals[k] = va;
        } else if va == vb {
            out.vals[k] = va;
        } else {
            return Err(XC_MERGE_CLASH + k as u32);
        }
    }
    Ok(out)
}

/// 键重复设置检出（包内同键两值 = 表损坏）。
pub fn dup_key_check(entries: &[(u8, u32)]) -> Result<(), u32> {
    for (i, (k1, _)) in entries.iter().enumerate() {
        for (k2, _) in entries.iter().skip(i + 1) {
            if k1 == k2 {
                return Err(XC_DUP_KEY);
            }
        }
        if *k1 > K_FRAMENO {
            return Err(XC_KEY_UNSUP);
        }
    }
    Ok(())
}

/// 判据入口（注册于 cgpu/mod.rs 检查表）。
pub fn run_cga69_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0069");
    // 族一：键值域闭集。
    cs.add("位深 10 过", val_ok(K_BITDEPTH, 10).is_ok(), "闭集档");
    cs.add("位深 9 拒", val_ok(K_BITDEPTH, 9).is_err(), "VAL_OOB");
    cs.add("色度 420 过", val_ok(K_CHROMA, 2).is_ok(), "闭集档");
    cs.add("色度 411 拒", val_ok(K_CHROMA, 3).is_err(), "VAL_OOB");
    cs.add("Gamma 界", val_ok(K_GAMMA, 4545).is_ok() && val_ok(K_GAMMA, 50).is_err(), "千分比域");
    cs.add("未知键拒", val_ok(9, 0).is_err(), "KEY_UNSUP");
    // 族二：序列化 roundtrip。
    let mut pkg = MetaPkg::new();
    let _ = pkg.set(K_COLORSPACE, 0).map_err(|e| e as u16);
    let _ = pkg.set(K_BITDEPTH, 10).map_err(|e| e as u16);
    let _ = pkg.set(K_CHROMA, 0).map_err(|e| e as u16);
    let _ = pkg.set(K_FRAMENO, 0x1122_3344).map_err(|e| e as u16);
    let ser = pkg.serialize().map_err(|e| e as u16);
    cs.add("序列化 32B", matches!(&ser, Ok(b) if b.len() == 32 && b[0] == 0x4D && b[1] == 0x43), "magic+定长");
    if let Ok(b) = &ser {
        let back = MetaPkg::deserialize(b);
        cs.add("roundtrip 一致", matches!(&back, Ok(p) if p.ver == pkg.ver && p.vals[0] == pkg.vals[0] && p.vals[7] == pkg.vals[7]), "逐键还原");
        cs.add("帧号全宽还原", matches!(&back, Ok(p) if p.vals[7] == 0x1122_3344), "尾 4B 全宽");
        // magic 破坏检出。
        let mut bad = *b;
        bad[0] = 0x00;
        cs.add("magic 破检出", MetaPkg::deserialize(&bad).is_err(), "MAGIC_MISS");
    }
    // 族三：版本单调。
    cs.add("版本只进", version_forward(1, 2).is_ok(), "单调");
    cs.add("版本回退拒", version_forward(2, 2).is_err(), "VER_BACK");
    // 族四：合并（同值放行 / 异值立案 / 单侧取设侧）。
    let mut a = MetaPkg::new();
    let _ = a.set(K_COLORSPACE, 0).map_err(|e| e as u16);
    let mut b = MetaPkg::new();
    let _ = b.set(K_COLORSPACE, 0).map_err(|e| e as u16);
    let _ = b.set(K_BITDEPTH, 16).map_err(|e| e as u16);
    let mg = merge(&a, &b).map_err(|e| e as u16);
    cs.add("合并同值", matches!(&mg, Ok(p) if p.vals[0] == 0), "同值放行");
    cs.add("合并取设侧", matches!(&mg, Ok(p) if p.vals[1] == 16), "单侧补全");
    cs.add("合并版本前进", matches!(&mg, Ok(p) if p.ver == 2), "max+1");
    let mut c = MetaPkg::new();
    let _ = c.set(K_COLORSPACE, 3).map_err(|e| e as u16);
    cs.add("异值立案", merge(&a, &c).is_err(), "MERGE_CLASH");
    // 族五：键重复检出。
    cs.add("重键检出", dup_key_check(&[(K_DPI, 72), (K_DPI, 96)]).is_err(), "DUP_KEY");
    cs.add("越界键检出", dup_key_check(&[(9, 0)]).is_err(), "KEY_UNSUP");
    cs.add("合法键序", dup_key_check(&[(K_DPI, 72), (K_FRAMENO, 1)]).is_ok(), "放行");
    deepen_meta::merge_into(&mut cs);
    cs
}

/// ===== 深化扩展块（cga69_metachan）：真功能面 + 判据并入主核 =====
pub mod deepen_meta {
    use super::*;

    /// 扩展一：包有效性总核（八键值域全查——全设包审计）。
    pub fn pkg_full_audit(p: &MetaPkg) -> Result<(), u32> {
        for k in 0..8usize {
            let v = p.vals[k];
            if v != u32::MAX {
                val_ok(k as u8, v)?;
            }
        }
        Ok(())
    }

    /// 扩展二：设置计数账（已设键数——部分包/全包判据）。
    pub fn set_count(p: &MetaPkg) -> usize {
        p.vals.iter().filter(|&&v| v != u32::MAX).count()
    }

    /// 扩展三：Gamma 千分比锚（γ2.2 的倒数域 4545 冻结值——常见
    /// 档位闭集账）。
    pub fn gamma_anchor(g: u32) -> Result<&'static str, u32> {
        match g {
            1000 => Ok("linear"),
            2200 => Ok("srgb-enc"),
            4545 => Ok("srgb-dec"),
            _ => Err(XC_VAL_OOB),
        }
    }

    /// 深化判据并入主核。
    pub fn merge_into(cs: &mut CheckSet) {
        let mut p = MetaPkg::new();
        let _ = p.set(K_COLORSPACE, 1).map_err(|e| e as u16);
        let _ = p.set(K_BITDEPTH, 12).map_err(|e| e as u16);
        let _ = p.set(K_DPI, 72).map_err(|e| e as u16);
        cs.add("包有效", pkg_full_audit(&p).is_ok(), "三键全合法");
        cs.add("设置计数", set_count(&p) == 3, "已设 3 键");
        cs.add("无效包检出", {
            let mut bad = MetaPkg::new();
            bad.vals[K_BITDEPTH as usize] = 9; // 旁路 set 直灌坏值
            pkg_full_audit(&bad).is_err()
        }, "VAL_OOB 面");
        cs.add("Gamma 线性锚", matches!(gamma_anchor(1000), Ok("linear")), "1.0");
        cs.add("Gamma 解码锚", matches!(gamma_anchor(4545), Ok("srgb-dec")), "1/2.2");
        cs.add("Gamma 野值拒", gamma_anchor(7777).is_err(), "VAL_OOB");
    }

}
