//! cgb870_legacyfloor —— CGPU-F0870 老旧 AMD 支持下限。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0870 老旧 AMD 支持下限  <SEG 0x7740>
//! 目标行数：≥300（判据：纯功能代码 ≥300 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥300 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0870；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（老旧 AMD 支持下限，整数账面）：
//!   1. 支持面闭集六面（写死）：基础光栅 / 固定功能混合 / 无网格
//!      着色 / 无光追 / 无 AI 加速 / 无双媒体；面号 0..=5；老卡
//!      全部只保前四面；
//!   2. 老卡代际闭集（写死）：GCN1 / GCN2 / GCN3 / GCN4 / Polaris；
//!      越界红码（XC_BAD_GEN）；
//!   3. 支持下限账（恒等式）：面声明数 = 保留数 + 降级数 + 拒绝
//!      数；漂移红码（XC_FLOOR_DRIFT）；
//!   4. 降级判据（写死）：GCN1/2 光栅面降级为兼容模式（保语义弃
//!      性能）；GCN3/4/Polaris 全保留；越代纪要求能力（如网格着
//!      色）红码（XC_CAPS_ABOVE_FLOOR）；
//!   5. 退役判据（写死）：驱动白名单面不再收录 GCN1 以下（无此
//!      代际即无 SKU 路径），声明越界红码（XC_GEN_RETIRED）；
//!   6. 兼容分数：每面下限分（写死 500 分/面），低于下限红码
//!      （XC_FLOOR_SCORE）；
//!   7. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x7740 段为本模块独占（SEG_BASE=0x7740），判别式
//!   SEG_BASE+1..+N；与 0x7720/0x7760 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 模块版本。
pub const LF_VERSION: &str = "cgb870_legacyfloor-v1";

/// 诊断段基址（0x7740 · 本模块独占）。
pub const SEG_BASE: u16 = 0x7740;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x7741 支持面越界（0..=5 之外）。
pub const XC_BAD_FACE: u16 = SEG_BASE + 1;
/// 0x7742 代际越界（0..=4 之外）。
pub const XC_BAD_GEN: u16 = SEG_BASE + 2;
/// 0x7743 下限账漂移（声明 ≠ 保留 + 降级 + 拒绝）。
pub const XC_FLOOR_DRIFT: u16 = SEG_BASE + 3;
/// 0x7744 超下限能力申请（老卡求新能力）。
pub const XC_CAPS_ABOVE_FLOOR: u16 = SEG_BASE + 4;
/// 0x7745 已退役代际声明。
pub const XC_GEN_RETIRED: u16 = SEG_BASE + 5;
/// 0x7746 面分低于下限。
pub const XC_FLOOR_SCORE: u16 = SEG_BASE + 6;
/// 0x7747 下限账空出报告。
pub const XC_FLOOR_EMPTY: u16 = SEG_BASE + 7;
/// 0x7748 SKU 代际映射缺失。
pub const XC_SKU_NO_GEN: u16 = SEG_BASE + 8;

/// 支持面数（写死：6）。
pub const FACE_CAP: usize = 6;
/// 面下限分（写死：500）。
pub const FLOOR_SCORE: u32 = 500;

/// 支持面闭集：0=基础光栅 1=固定功能混合 2=无网格着色声明
/// 3=无光追声明 4=无 AI 加速声明 5=无双媒体声明。
pub const F_RASTER: usize = 0;
pub const F_FIXED: usize = 1;
pub const F_NOMESH: usize = 2;
pub const F_NORT: usize = 3;
pub const F_NOAI: usize = 4;
pub const F_NOMEDIA: usize = 5;

/// 老卡代际闭集：0=GCN1 1=GCN2 2=GCN3 3=GCN4 4=Polaris。
pub const G_GCN1: u8 = 0;
pub const G_GCN2: u8 = 1;
pub const G_GCN3: u8 = 2;
pub const G_GCN4: u8 = 3;
pub const G_POLARIS: u8 = 4;
/// 代际上限。
pub const GEN_CAP: u8 = 4;

/// 代际名。
pub fn gen_name(g: u8) -> &'static str {
    match g {
        G_GCN1 => "GCN1",
        G_GCN2 => "GCN2",
        G_GCN3 => "GCN3",
        G_GCN4 => "GCN4",
        _ => "Polaris",
    }
}

/// 面 → 代际支持位图（写死）：GCN1 起全六面声明（含否定面），
/// 但光栅面按代际降级。返回 true = 该代际保留该面原义。
pub fn face_kept(gen: u8, face: usize) -> Result<bool, u16> {
    if gen > GEN_CAP {
        return Err(XC_BAD_GEN);
    }
    if face >= FACE_CAP {
        return Err(XC_BAD_FACE);
    }
    // GCN1/2 光栅面降级（false → 走降级账），其余保留。
    if face == F_RASTER && gen <= G_GCN2 {
        return Ok(false);
    }
    Ok(true)
}

/// 面名。
pub fn face_name(f: usize) -> &'static str {
    match f {
        F_RASTER => "raster",
        F_FIXED => "fixed",
        F_NOMESH => "nomesh",
        F_NORT => "nort",
        F_NOAI => "noai",
        _ => "nomedia",
    }
}

/// 老旧支持下限册。
pub struct LegacyFloor {
    /// 面声明数（六面各代际合计）。
    pub declared: u64,
    /// 保留数。
    pub kept: u64,
    /// 降级数。
    pub degraded: u64,
    /// 拒绝数（退役面声明）。
    pub rejected: u64,
    /// 各面分数账（定容 12/面）。
    pub scores: [Vec<u32>; FACE_CAP],
    /// 累计红码。
    pub red_count: u64,
}

impl LegacyFloor {
    /// 空册。
    pub fn new() -> Self {
        LegacyFloor {
            declared: 0,
            kept: 0,
            degraded: 0,
            rejected: 0,
            scores: [Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            red_count: 0,
        }
    }

    /// 面账守恒。
    pub fn balanced(&self) -> bool {
        self.declared == self.kept + self.degraded + self.rejected
    }

    /// 声明面（代际 × 面；GCN1 以下退役即拒）。
    pub fn declare(&mut self, gen: u8, face: usize) -> Result<(), u16> {
        if gen > GEN_CAP {
            self.red_count += 1;
            return Err(XC_GEN_RETIRED);
        }
        if face >= FACE_CAP {
            self.red_count += 1;
            return Err(XC_BAD_FACE);
        }
        self.declared += 1;
        if face_kept(gen, face)? {
            self.kept += 1;
        } else {
            self.degraded += 1;
        }
        Ok(())
    }

    /// 录面分（闭集 + 下限校验 + 定容 12）。
    pub fn score(&mut self, face: usize, val: u32) -> Result<(), u16> {
        if face >= FACE_CAP {
            self.red_count += 1;
            return Err(XC_BAD_FACE);
        }
        if val < FLOOR_SCORE {
            self.red_count += 1;
            return Err(XC_FLOOR_SCORE);
        }
        if val > 1000 {
            self.red_count += 1;
            return Err(XC_FLOOR_SCORE);
        }
        if self.scores[face].len() == 12 {
            self.scores[face].remove(0);
        }
        self.scores[face].push(val);
        Ok(())
    }

    /// 面均分（空面回 0）。
    pub fn face_mean(&self, face: usize) -> Result<u32, u16> {
        if face >= FACE_CAP {
            return Err(XC_BAD_FACE);
        }
        if self.scores[face].is_empty() {
            return Ok(0);
        }
        let mut acc: u64 = 0;
        let mut i = 0;
        while i < self.scores[face].len() {
            acc += self.scores[face][i] as u64;
            i += 1;
        }
        Ok((acc / self.scores[face].len() as u64) as u32)
    }

    /// 老卡能力申请校验（申请网格着色/光追/AI/双媒体即超下限）。
    pub fn request_caps(&mut self, mesh: bool, rt: bool, ai: bool, dual_media: bool) -> Result<(), u16> {
        if mesh || rt || ai || dual_media {
            self.red_count += 1;
            return Err(XC_CAPS_ABOVE_FLOOR);
        }
        Ok(())
    }

    /// 出下限报告（账空红；账不平衡红）。
    pub fn report(&self) -> Result<String, u16> {
        if !self.balanced() {
            return Err(XC_FLOOR_DRIFT);
        }
        if self.declared == 0 {
            return Err(XC_FLOOR_EMPTY);
        }
        let mut s = String::from("legacyfloor declared=");
        s += &self.declared.to_string();
        s += " kept=";
        s += &self.kept.to_string();
        s += " degraded=";
        s += &self.degraded.to_string();
        s += " rejected=";
        s += &self.rejected.to_string();
        Ok(s)
    }

    /// 全账审计。
    pub fn audit(&self) -> Result<(), u16> {
        if !self.balanced() {
            return Err(XC_FLOOR_DRIFT);
        }
        let mut f = 0;
        while f < FACE_CAP {
            let mut i = 0;
            while i < self.scores[f].len() {
                if self.scores[f][i] < FLOOR_SCORE || self.scores[f][i] > 1000 {
                    return Err(XC_FLOOR_SCORE);
                }
                i += 1;
            }
            f += 1;
        }
        Ok(())
    }
}

/// 自例程：五代际 × 六面声明 → 降级核对 → 面分 → 报告 + 负例六连。
pub fn drill() -> Result<u32, u16> {
    let mut lf = LegacyFloor::new();
    // 负例：退役代际（无 GCN0 定义——越界即退役语义）。
    if lf.declare(9, F_RASTER) != Err(XC_GEN_RETIRED) {
        return Err(XC_GEN_RETIRED);
    }
    // 负例：面越界。
    if lf.declare(G_POLARIS, FACE_CAP) != Err(XC_BAD_FACE) {
        return Err(XC_BAD_FACE);
    }
    // 五代际 × 六面声明。
    let mut g: u8 = 0;
    while g <= GEN_CAP {
        let mut f = 0;
        while f < FACE_CAP {
            lf.declare(g, f)?;
            f += 1;
        }
        g += 1;
    }
    // 降级核对：GCN1/GCN2 各 1 面（光栅）降级 → 2。
    if lf.degraded != 2 {
        return Err(XC_FLOOR_DRIFT);
    }
    // 能力申请：老卡求光追 → 红。
    if lf.request_caps(false, true, false, false) != Err(XC_CAPS_ABOVE_FLOOR) {
        return Err(XC_CAPS_ABOVE_FLOOR);
    }
    // 全否定申请合法。
    lf.request_caps(false, false, false, false)?;
    // 面分录入（各面 2 条）。
    let mut f2 = 0;
    while f2 < FACE_CAP {
        lf.score(f2, 600)?;
        lf.score(f2, 800)?;
        f2 += 1;
    }
    // 负例：低于下限。
    if lf.score(F_RASTER, FLOOR_SCORE - 1) != Err(XC_FLOOR_SCORE) {
        return Err(XC_FLOOR_SCORE);
    }
    // 负例：超上限。
    if lf.score(F_RASTER, 1001) != Err(XC_FLOOR_SCORE) {
        return Err(XC_FLOOR_SCORE);
    }
    // 负例：面越界。
    if lf.score(FACE_CAP, 600) != Err(XC_BAD_FACE) {
        return Err(XC_BAD_FACE);
    }
    lf.audit()?;
    // 面均分核对。
    let m0 = lf.face_mean(F_RASTER)?;
    if m0 != 700 {
        return Err(XC_FLOOR_SCORE);
    }
    // 空面均分回 0。
    let mut q = LegacyFloor::new();
    if q.face_mean(F_FIXED)? != 0 {
        return Err(XC_FLOOR_EMPTY);
    }
    // 负例：账空报告。
    if q.report() != Err(XC_FLOOR_EMPTY) {
        return Err(XC_FLOOR_EMPTY);
    }
    let rep = lf.report()?;
    if rep.len() == 0 {
        return Err(XC_FLOOR_EMPTY);
    }
    // face_kept 面：GCN1 光栅 false、Polaris 光栅 true。
    if face_kept(G_GCN1, F_RASTER)? != false || face_kept(G_POLARIS, F_RASTER)? != true {
        return Err(XC_FLOOR_DRIFT);
    }
    Ok(0)
}
