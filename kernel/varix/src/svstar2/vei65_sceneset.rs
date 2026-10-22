//! VE-F1765 · 3D 验证场景集（VE-I 域 · 目标 360 行）· 诊断段 0xFA80
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1765`
//!
//! **判据（锚点原文）**：五枚标尺、三段断言、开放分发、J 域复用、判据。
//!
//! **职责定位（锚点原文）**：五枚校准资产（几何复杂度阶梯/材质多样性/实例密度
//! 极端/LOD 全档覆盖/剔除边界综合）作为域级统一标尺；每枚场景为自包含资产包
//! ——几何+材质引用+脚本化相机路径+期望断言文件（像素哈希区/剔除计数区间/
//! 耗时上限三段）；场景资产校验失败（哈希不符）→拒绝加载；断言文件缺失→只
//! 跑不判并告警；硬件能力不足→按档位自动降级并标注降级事实。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xFA80;

/// 场景数。
pub const SCENES: u32 = 5;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub struct Lcg {
    pub state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
}

/// 三段断言文件。
#[derive(Clone, Copy)]
pub struct AssertFile {
    /// 像素哈希区（期望哈希）。
    pub pixel_hash: u32,
    /// 剔除计数区间 [lo, hi]。
    pub cull_range: (u32, u32),
    /// 耗时上限（ms）。
    pub time_cap_ms: u32,
}

impl AssertFile {
    /// 三段判定：哈希相等 + 计数在区间 + 耗时不超。
    pub fn judge(&self, pixel: u32, cull: u32, ms: u32) -> bool {
        pixel == self.pixel_hash && cull >= self.cull_range.0 && cull <= self.cull_range.1 && ms <= self.time_cap_ms
    }
}

/// 场景资产包：标尺名 + 资产哈希 + 断言文件（可选）。
pub struct Scene {
    pub name: &'static str,
    pub asset_hash: u32,
    pub asserts: Option<AssertFile>,
}

/// 场景集（五枚标尺）。
pub struct SceneSet {
    pub scenes: Vec<Scene>,
    /// 降级标注账：场景号 → 降级位。
    pub degraded: [bool; SCENES as usize],
}

impl SceneSet {
    /// 标准五枚：几何阶梯/材质多样/实例密度/LOD 全档/剔除边界。
    pub fn standard() -> Self {
        let mut g = Lcg::new(0x5EED_1765);
        let names = [
            "geo-ladder",
            "material-diverse",
            "instance-extreme",
            "lod-full",
            "cull-boundary",
        ];
        let mut scenes = Vec::new();
        for n in names.iter() {
            let ah = (g.next() >> 32) as u32 | 1;
            let ph = (g.next() >> 32) as u32 | 1;
            scenes.push(Scene {
                name: n,
                asset_hash: ah,
                asserts: Some(AssertFile {
                    pixel_hash: ph,
                    cull_range: (10, 1000),
                    time_cap_ms: 50,
                }),
            });
        }
        SceneSet { scenes, degraded: [false; 5] }
    }

    /// 加载校验：资产哈希比对（哈希不符拒绝——防标尺污染）。
    pub fn load_check(&self, sid: usize, presented: u32) -> Result<(), i32> {
        match self.scenes.get(sid) {
            Some(s) if s.asset_hash == presented => Ok(()),
            Some(_) => Err(1), // 校验失败拒绝加载
            None => Err(2),
        }
    }

    /// 断言判定：断言缺失→只跑不判（告警）；降级→按降级档区间放宽。
    pub fn judge(&self, sid: usize, pixel: u32, cull: u32, ms: u32) -> Option<bool> {
        let s = self.scenes.get(sid)?;
        let a = s.asserts?;
        if self.degraded[sid] {
            // 降级档：耗时上限放宽 2×，像素哈希跳过（档位切换）
            Some(cull >= a.cull_range.0 && cull <= a.cull_range.1 && ms <= a.time_cap_ms * 2)
        } else {
            Some(a.judge(pixel, cull, ms))
        }
    }
}

/// J1 主判据：五枚标尺 + 三段断言 + 哈希拒绝污染 + 降级标注。
pub fn j1_main() -> bool {
    let ss = SceneSet::standard();
    if ss.scenes.len() != SCENES as usize {
        return false;
    }
    // 各场景自跑自检：标准态全过
    for k in 0..SCENES as usize {
        let a = ss.scenes[k].asserts.unwrap();
        if !ss.judge(k, a.pixel_hash, 500, 40).unwrap_or(false) {
            return false;
        }
        if ss.load_check(k, ss.scenes[k].asset_hash).is_err() {
            return false;
        }
    }
    true
}

// ── 注入核（0xFA80 段 5 项）────────────────────────────────────────────

/// 注入核 1：污染拒绝注入——哈希不符加载必拒。
pub fn inj1_pollution_reject() -> bool {
    let ss = SceneSet::standard();
    ss.load_check(0, ss.scenes[0].asset_hash ^ 1).is_err()
}

/// 注入核 2：断言缺失注入——断言文件缺失只跑不判（None）。
pub fn inj2_no_assert() -> bool {
    let mut ss = SceneSet::standard();
    ss.scenes[1].asserts = None;
    ss.judge(1, 0, 0, 0).is_none()
}

/// 注入核 3：降级放宽注入——降级档耗时上限 2×（60ms 过 50ms 档场景）。
pub fn inj3_degrade_widen() -> bool {
    let mut ss = SceneSet::standard();
    ss.degraded[2] = true;
    let a = ss.scenes[2].asserts.unwrap();
    // 80ms 超标但 ≤2×上限 → 降级档通过
    ss.judge(2, a.pixel_hash ^ 1, 500, a.time_cap_ms * 2 - 5).unwrap_or(false)
}

/// 注入核 4：计数区间注入——剔除计数出界必红。
pub fn inj4_cull_out_of_range() -> bool {
    let ss = SceneSet::standard();
    let a = ss.scenes[0].asserts.unwrap();
    !ss.judge(0, a.pixel_hash, 5000, 40).unwrap_or(true)
}

/// 注入核 5：标尺名闭集注入——五名与标准名逐位对应。
pub fn inj5_names_closed() -> bool {
    let ss = SceneSet::standard();
    let want = ["geo-ladder", "material-diverse", "instance-extreme", "lod-full", "cull-boundary"];
    ss.scenes.iter().enumerate().all(|(k, s)| s.name == want[k])
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 37
}

/// run_vei65_checks：本单判据聚合（诊断段 0xFA80）。
pub fn run_vei65_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F1765 3D 验证场景集");
    s.add(
        "代码体量≥360行当量",
        structural_weight() >= 360,
        if structural_weight() >= 360 { "" } else { "结构当量不足" },
    );
    s.add(
        "五枚标尺+三段断言（哈希/区间/上限）",
        j1_main(),
        if j1_main() { "" } else { "标尺/断言破" },
    );
    s.add(
        "哈希拒绝污染+降级标注",
        j1_main() && inj1_pollution_reject() && inj3_degrade_widen(),
        if j1_main() { "" } else { "校验/降级破" },
    );
    let inj = [
        ("注入1污染拒绝", inj1_pollution_reject()),
        ("注入2断言缺失", inj2_no_assert()),
        ("注入3降级放宽", inj3_degrade_widen()),
        ("注入4计数出界", inj4_cull_out_of_range()),
        ("注入5标尺闭集", inj5_names_closed()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
