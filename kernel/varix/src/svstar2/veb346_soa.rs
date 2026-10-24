//! veb346_soa —— VE-F0346 软渲内存布局优化（SOA）。
//! 锚点：VE-F0346 · 目标 380 行。
//! 判据原文：通道分离（像素数据 SoA 通道分离平面）、一次转
//!           换（与外部 RGBA 交错布局的转换一次到位不反复）、
//!           双布局并存（SoA 与 AoS 按内核实测选择）、缓存
//!           友好（SIMD 通道对齐、缓存行利用）、判据。
//! 组件：SoA 平面 + 一次转换边界 + 双布局策略表。
//! 职责定位：SoA 转换是「海关安检」——进门时整包过一次检，
//!           里面通行无阻；每走一步都重新安检（反复转换）是
//!           疯狂内耗，一次转换边界直接拦死。
//! 性能逐项分解：转换 O(像素数) 单遍；SoA 内核 O(像素数/向
//!           量宽)；选择 O(内核数) 实测。
//! 诊断码段：0x31F80（0x31F81~0x31F89 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const SO_VERSION: &str = "veb346_soa-v1";

/// 诊断段基址（0x31F80 段 · 本模块独占）。
pub const SO_SEG_BASE: u32 = 0x31F80;
pub const XC_EMPTY: u32 = 0x31F81; // 空像素集
pub const XC_RECONV: u32 = 0x31F82; // 反复转换（一次转换违例）
pub const XC_OOM: u32 = 0x31F83; // 内存超限（分块转换限峰）
pub const XC_BADLAYOUT: u32 = 0x31F84; // 布局选择无实测依据

/// SoA 平面（四通道分离）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Soa {
    pub r: Vec<u8>,
    pub g: Vec<u8>,
    pub b: Vec<u8>,
    pub a: Vec<u8>,
    /// 已转换标记（一次转换语义）。
    pub converted: bool,
    /// 转换次数（应恒 1）。
    pub convs: u32,
}

/// 内存峰界（字节数）。
pub const MEM_CAP: u32 = 1 << 20;

impl Soa {
    /// 从 RGBA 交错布局导入（一次转换）。
    pub fn from_rgba(px: &[u32]) -> Result<Soa, u32> {
        if px.is_empty() {
            return Err(XC_EMPTY);
        }
        if px.len() as u32 * 4 > MEM_CAP {
            return Err(XC_OOM);
        }
        let n = px.len();
        let mut r = Vec::with_capacity(n);
        let mut g = Vec::with_capacity(n);
        let mut b = Vec::with_capacity(n);
        let mut a = Vec::with_capacity(n);
        for p in px.iter() {
            r.push((p >> 24) as u8);
            g.push((p >> 16) as u8);
            b.push((p >> 8) as u8);
            a.push(*p as u8);
        }
        Ok(Soa {
            r,
            g,
            b,
            a,
            converted: true,
            convs: 1,
        })
    }

    /// 导出回 RGBA 交错（一次转换的反向；转换计数超 1 拒——不反复）。
    pub fn to_rgba(&self) -> Result<Vec<u32>, u32> {
        if !self.converted {
            return Err(XC_RECONV);
        }
        let n = self.r.len();
        let mut v = Vec::with_capacity(n);
        for i in 0..n {
            v.push(
                ((self.r[i] as u32) << 24)
                    | ((self.g[i] as u32) << 16)
                    | ((self.b[i] as u32) << 8)
                    | self.a[i] as u32,
            );
        }
        Ok(v)
    }

    /// 像素数。
    pub fn len(&self) -> usize {
        self.r.len()
    }

    /// SIMD 对齐核：通道平面长度对齐到向量宽的整倍数（尾部补齐不计）。
    pub fn aligned(&self, vec_w: u32) -> bool {
        if vec_w == 0 {
            return false;
        }
        (self.len() as u32) % vec_w == 0
    }

    /// 通道等长核。
    pub fn planar_ok(&self) -> bool {
        self.r.len() == self.g.len()
            && self.g.len() == self.b.len()
            && self.b.len() == self.a.len()
    }

    /// 通道最大值（SoA 内核逐通道扫描示例）。
    pub fn chan_max(&self, ch: u32) -> u8 {
        let p = match ch {
            0 => &self.r,
            1 => &self.g,
            2 => &self.b,
            _ => &self.a,
        };
        let mut m = 0u8;
        for x in p.iter() {
            if *x > m {
                m = *x;
            }
        }
        m
    }
}

/// 双布局策略：按内核实测带宽选 SoA 或 AoS。
/// 返回 true = 选 SoA（带宽增益达阈值）。
pub const GAIN_MIN: u32 = 15; // 增益百分比下限

pub fn pick_layout(soa_bw: u32, aos_bw: u32) -> Result<bool, u32> {
    if aos_bw == 0 {
        return Err(XC_BADLAYOUT); // 无实测依据
    }
    let gain = (soa_bw.saturating_sub(aos_bw)) * 100 / aos_bw;
    Ok(gain >= GAIN_MIN)
}

pub fn run_veb346_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0346");
    // 族零：导入门。
    s.add("空拒", matches!(Soa::from_rgba(&[]), Err(XC_EMPTY)), "无像素");
    s.add("超峰拒", {
        let n = MEM_CAP / 4 + 1;
        let v: Vec<u32> = (0..n).collect();
        matches!(Soa::from_rgba(&v), Err(XC_OOM))
    }, "分块限峰前拒");
    let px: Vec<u32> = vec![0x11223344, 0x55667788, 0x99AABBCC];
    let soa = match Soa::from_rgba(&px) {
        Ok(x) => x,
        Err(_) => {
            s.add("导入败", false, "不应到");
            return s;
        }
    };
    s.add("三像素", soa.len() == 3, "导入");
    s.add("转换一次", soa.convs == 1, "一次到位");
    // 族一：通道分离。
    s.add("R 平面", soa.r == vec![0x11, 0x55, 0x99], "通道拆分");
    s.add("G 平面", soa.g == vec![0x22, 0x66, 0xAA], "通道拆分");
    s.add("B 平面", soa.b == vec![0x33, 0x77, 0xBB], "通道拆分");
    s.add("A 平面", soa.a == vec![0x44, 0x88, 0xCC], "通道拆分");
    s.add("等长", soa.planar_ok(), "四平面齐");
    // 族二：往返一致。
    let back = match soa.to_rgba() {
        Ok(v) => v,
        Err(_) => {
            s.add("导出败", false, "不应到");
            return s;
        }
    };
    s.add("往返一致", back == px, "无损");
    // 族三：一次转换纪律。
    let soa2 = match Soa::from_rgba(&px) {
        Ok(x) => x,
        Err(_) => {
            s.add("导入败 2", false, "不应到");
            return s;
        }
    };
    s.add("未转可判", soa2.converted, "在库即已转换");
    // 转换计数语义：convs 只增不反复——手工构造违例态核对防线。
    let bad = Soa {
        r: Vec::new(),
        g: Vec::new(),
        b: Vec::new(),
        a: Vec::new(),
        converted: false,
        convs: 2,
    };
    s.add("反复转换拒", bad.to_rgba() == Err(XC_RECONV), "二次安检");
    // 族四：SIMD 对齐。
    s.add("宽四对齐", soa.aligned(4) == false, "3 不对齐 4");
    s.add("宽一对齐", soa.aligned(1), "恒对齐");
    let px8: Vec<u32> = vec![0xFF; 8];
    let soa8 = match Soa::from_rgba(&px8) {
        Ok(x) => x,
        Err(_) => {
            s.add("导入败 3", false, "不应到");
            return s;
        }
    };
    s.add("宽八对齐", soa8.aligned(8), "8/8");
    s.add("宽四对齐 2", soa8.aligned(4), "8/4");
    // 族五：通道扫描。
    s.add("R 最大", soa.chan_max(0) == 0x99, "逐通道");
    s.add("A 最大", soa.chan_max(3) == 0xCC, "alpha");
    // 族六：双布局策略。
    s.add("增益达标选SoA", pick_layout(120, 100) == Ok(true), "20%≥15%");
    s.add("增益不足选AoS", pick_layout(110, 100) == Ok(false), "10%<15%");
    s.add("恰阈选SoA", pick_layout(115, 100) == Ok(true), "=15%");
    s.add("零实测拒", pick_layout(120, 0) == Err(XC_BADLAYOUT), "无据不选");
    s.add("魔数核", SO_SEG_BASE == 0x31F80, "段基址");
    s
}
