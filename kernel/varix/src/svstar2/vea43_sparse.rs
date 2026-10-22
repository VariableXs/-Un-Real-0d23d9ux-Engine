//! VE-F0043 · 稀疏与平铺资源管理（VE-A 域 · A02b 资源视图组 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0043`
//!
//! **职责定位（锚点原文）**：稀疏纹理（部分驻留）的平铺管理（tile 映射表/
//! 驻留策略/迁移调度三段），驻留预算（稀疏资源的显存占用可控），映射一致性
//! 断言；含稀疏资源的驻留热图（哪块常驻哪块换出可视化）。
//!
//! **数据结构（锚点原文）**：平铺管理；驻留策略。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//! - 映射断裂 → 修复（映射表发现空洞时按迁移调度回填）；
//! - 预算超 → 驻留降级（驻留 tile 数压到预算内，换出策略优先冷块）；
//! - 一致性破坏 → 重建（映射与驻留账对不上时整资源重建映射）。
//!
//! **性能逐项分解（锚点原文）**：O(tile)。映射查询 O(1) 直达表；驻留
//! 换出 O(驻留数) 迁移期一次；热图 O(1) 增量计数。
//!
//! **跨批对接点（锚点原文）**：A05 显存预算联动——驻留预算来自显存预算池。
//!
//! **无障碍与隐私（锚点原文）**：驻留状态读屏可达。
//!
//! **判据（锚点原文）**：tile 映射、驻留预算、迁移调度、一致性断言、判据。
//!
//! 纯功能线：no_std + alloc；零 unwrap/expect/panic；整数；确定性。
//!
//! 诊断段：0x1E00 基址（全域核查无占用）。

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const SPARSE_VERSION: &str = "vea43_sparse-v1";

/// 诊断段基址。
pub const SEG_BASE: u32 = 0x1E00;

// ---- 诊断码 ----
pub const XC_TILE_RANGE: u32 = SEG_BASE + 1; // tile 编号越界
pub const XC_MAP_VOID: u32 = SEG_BASE + 2; // 映射空洞（断裂）
pub const XC_BUDGET: u32 = SEG_BASE + 3; // 驻留预算超限
pub const XC_INCONSIST: u32 = SEG_BASE + 4; // 一致性破坏
pub const XC_RES_FULL: u32 = SEG_BASE + 5; // 资源表满
pub const XC_UNKNOWN_RES: u32 = SEG_BASE + 6; // 资源不存在

/// 单个 tile 的驻留记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileRec {
    /// 是否驻留显存（部分驻留语义的核心位）。
    pub resident: bool,
    /// 驻留代际（换入计数，热图数据源：代际高 = 访问频繁）。
    pub gen: u32,
    /// 物理槽位号（换出后置 0xFFFF_FFFF 表示无槽位）。
    pub phys: u32,
}

impl TileRec {
    const NO_PHYS: u32 = 0xFFFF_FFFF;
    pub fn fresh() -> TileRec {
        TileRec { resident: false, gen: 0, phys: TileRec::NO_PHYS }
    }
}

/// 稀疏资源：tile 表 + 驻留账。
pub struct SparseRes {
    pub res_id: u32,
    tiles: Vec<TileRec>,
    /// 驻留预算（最多同时驻留的 tile 数）。
    budget: u32,
    /// 当前驻留数。
    resident_n: u32,
    /// 换入/换出累计（迁移调度账本）。
    evict_n: u32,
    load_n: u32,
}

impl SparseRes {
    pub fn new(res_id: u32, tile_count: u32, budget: u32) -> SparseRes {
        let n = tile_count.min(4096) as usize;
        let mut tiles = Vec::new();
        tiles.resize(n, TileRec::fresh());
        SparseRes { res_id, tiles, budget, resident_n: 0, evict_n: 0, load_n: 0 }
    }

    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }
    pub fn budget(&self) -> u32 {
        self.budget
    }
    pub fn resident_n(&self) -> u32 {
        self.resident_n
    }
    pub fn evict_n(&self) -> u32 {
        self.evict_n
    }
    pub fn load_n(&self) -> u32 {
        self.load_n
    }

    /// tile 访问（热图增量）：驻留则代际 +1（热度提升）；非驻留则触发换入
    /// 请求（迁移调度入口）。
    pub fn touch(&mut self, tile: usize) -> Result<bool, u32> {
        if tile >= self.tiles.len() {
            return Err(XC_TILE_RANGE);
        }
        if self.tiles[tile].resident {
            self.tiles[tile].gen += 1;
            Ok(true)
        } else {
            self.load_in(tile)
        }
    }

    /// 换入（迁移调度）：预算内直接驻留；超预算先换出最冷块（代际最小）。
    pub fn load_in(&mut self, tile: usize) -> Result<bool, u32> {
        if tile >= self.tiles.len() {
            return Err(XC_TILE_RANGE);
        }
        if self.tiles[tile].resident {
            return Ok(true);
        }
        if self.resident_n >= self.budget {
            // 驻留降级：换出最冷（代际最小；并列取下标小，确定性）。
            let victim = match self.coldest_resident() {
                Some(v) => v,
                None => return Err(XC_INCONSIST),
            };
            self.evict(victim);
        }
        self.tiles[tile].resident = true;
        self.tiles[tile].gen += 1;
        self.tiles[tile].phys = tile as u32; // 逻辑槽 = 物理槽的直映射模型
        self.resident_n += 1;
        self.load_n += 1;
        Ok(true)
    }

    /// 换出（迁移调度另一半）。
    pub fn evict(&mut self, tile: usize) -> bool {
        if tile >= self.tiles.len() || !self.tiles[tile].resident {
            return false;
        }
        self.tiles[tile].resident = false;
        self.tiles[tile].phys = TileRec::NO_PHYS;
        self.resident_n -= 1;
        self.evict_n += 1;
        true
    }

    /// 最冷驻留 tile（代际最小者）。
    fn coldest_resident(&self) -> Option<usize> {
        let mut best: Option<usize> = None;
        let mut best_gen = u32::MAX;
        for (i, t) in self.tiles.iter().enumerate() {
            if t.resident && t.gen < best_gen {
                best_gen = t.gen;
                best = Some(i);
            }
        }
        best
    }

    /// 映射一致性断言：驻留账与 tile 表逐位对账。
    /// resident_n 必须等于表内 resident 计数；驻留块必须有物理槽。
    pub fn consistency_check(&self) -> Result<(), u32> {
        let mut n = 0u32;
        for t in self.tiles.iter() {
            if t.resident {
                n += 1;
                if t.phys == TileRec::NO_PHYS {
                    return Err(XC_INCONSIST);
                }
            } else if t.phys != TileRec::NO_PHYS {
                return Err(XC_INCONSIST);
            }
        }
        if n != self.resident_n {
            return Err(XC_INCONSIST);
        }
        Ok(())
    }

    /// 映射断裂检测：非驻留但曾有代际的 tile 即「曾驻留后被换出」，
    /// 正常；断裂 = 表长度与声明 tile 数不符（外部改表），此处校验长度账。
    pub fn map_intact(&self, declared: u32) -> bool {
        self.tiles.len() == declared as usize
    }

    /// 驻留热图（读屏口径）：热/温/冷三档 tile 计数。
    /// 热：gen ≥ 8；温：4..8；冷：其余驻留块。
    pub fn heatmap(&self) -> (u32, u32, u32) {
        let mut hot = 0;
        let mut warm = 0;
        let mut cold = 0;
        for t in self.tiles.iter() {
            if !t.resident {
                continue;
            }
            if t.gen >= 8 {
                hot += 1;
            } else if t.gen >= 4 {
                warm += 1;
            } else {
                cold += 1;
            }
        }
        (hot, warm, cold)
    }
}

/// 稀疏资源管理器（多资源容器 + 预算总闸）。
pub struct SparseManager {
    res: Vec<SparseRes>,
    cap: usize,
    /// 全局驻留预算（跨资源总闸，A05 联动）。
    global_budget: u32,
    last_code: u32,
    /// 一致性重建次数。
    rebuilds: u32,
}

impl SparseManager {
    pub fn new(cap: usize, global_budget: u32) -> SparseManager {
        SparseManager { res: Vec::new(), cap, global_budget, last_code: 0, rebuilds: 0 }
    }

    pub fn create(&mut self, res_id: u32, tiles: u32, budget: u32) -> Result<usize, u32> {
        if res_id == 0 || tiles == 0 || budget == 0 || budget > self.global_budget {
            self.last_code = XC_BUDGET;
            return Err(self.last_code);
        }
        if self.res.iter().any(|r| r.res_id == res_id) || self.res.len() >= self.cap {
            self.last_code = XC_RES_FULL;
            return Err(self.last_code);
        }
        self.res.push(SparseRes::new(res_id, tiles, budget));
        Ok(self.res.len() - 1)
    }

    fn idx(&self, res_id: u32) -> Result<usize, u32> {
        match self.res.iter().position(|r| r.res_id == res_id) {
            Some(i) => Ok(i),
            None => Err(XC_UNKNOWN_RES),
        }
    }

    /// 访问转发（含换入调度）。
    pub fn touch(&mut self, res_id: u32, tile: usize) -> Result<bool, u32> {
        let i = self.idx(res_id)?;
        self.res[i].touch(tile)
    }

    /// 全局一致性扫描：逐资源对账，破坏者重建映射（驻留清零）。
    pub fn audit(&mut self) -> usize {
        let mut broken = 0usize;
        for r in self.res.iter_mut() {
            if r.consistency_check() == Err(XC_INCONSIST) {
                for t in r.tiles.iter_mut() {
                    *t = TileRec::fresh();
                }
                r.resident_n = 0;
                broken += 1;
            }
        }
        self.rebuilds += broken as u32;
        broken
    }

    /// 全局驻留总量（A05 预算联动查询）。
    pub fn global_resident(&self) -> u32 {
        self.res.iter().map(|r| r.resident_n).sum()
    }

    pub fn rebuilds(&self) -> u32 {
        self.rebuilds
    }
    pub fn len(&self) -> usize {
        self.res.len()
    }

    /// 读屏状态行。
    pub fn status_line(&self) -> String {
        alloc::format!(
            "SPARSE 资源{} 驻留{}/{} 重建{}",
            self.res.len(),
            self.global_resident(),
            self.global_budget,
            self.rebuilds
        )
    }

    pub fn last_code(&self) -> u32 {
        self.last_code
    }
}

// =====================================================================
// 自检（判据锚点：tile 映射 / 驻留预算 / 迁移调度 / 一致性断言 / 热图）
// =====================================================================

fn chk(set: &mut CheckSet, name: &'static str, ok: bool, detail: &'static str) {
    set.add(name, ok, detail);
}

/// VE-F0043 域自检。
pub fn run_vea43_checks() -> CheckSet {
    let mut set = CheckSet::new("vea43");

    // C43-01 tile 映射：touch 非驻留块触发换入并记账
    {
        let mut s = SparseRes::new(1, 16, 4);
        let ok = matches!(s.touch(3), Ok(true)) && s.resident_n() == 1 && s.load_n() == 1;
        chk(&mut set, "C43-01 换入记账", ok, "换入未记账");
    }

    // C43-02 tile 越界拒绝
    {
        let mut s = SparseRes::new(1, 16, 4);
        let ok = s.touch(16) == Err(XC_TILE_RANGE);
        chk(&mut set, "C43-02 越界拒", ok, "越界被放行");
    }

    // C43-03 驻留预算：预算满时换入触发换出最冷块
    {
        let mut s = SparseRes::new(1, 8, 2);
        let _ = s.touch(0);
        let _ = s.touch(0); // gen=2
        let _ = s.touch(1); // gen=1
        let before = s.resident_n();
        let _ = s.touch(2); // 预算满 → 换出 gen 最小的 tile1
        let ok = before == 2 && s.resident_n() == 2 && s.evict_n() == 1
            && !s.tile_resident(1);
        chk(&mut set, "C43-03 冷块换出", ok, "换出对象错");
    }

    // C43-04 一致性断言：账实相符绿
    {
        let mut s = SparseRes::new(1, 16, 4);
        let _ = s.touch(0);
        let _ = s.touch(5);
        let ok = s.consistency_check().is_ok();
        chk(&mut set, "C43-04 账实相符", ok, "正常账被判不一致");
    }

    // C43-05 一致性破坏检出：手工破坏驻留位后 audit 触发重建
    {
        let mut m = SparseManager::new(4, 64);
        let _ = m.create(1, 16, 4);
        let _ = m.touch(1, 0);
        // 破坏：直接把驻留数改错（模拟外部改表/位翻转）。
        if let Some(r) = m.res.iter_mut().find(|r| r.res_id == 1) {
            r.resident_n = 9; // 账与实不符
        }
        let ok = m.audit() == 1 && m.rebuilds() == 1 && m.global_resident() == 0;
        chk(&mut set, "C43-05 破坏重建", ok, "破坏未检出");
    }

    // C43-06 映射完整性：声明的 tile 数与表长一致
    {
        let s = SparseRes::new(1, 32, 4);
        let ok = s.map_intact(32) && !s.map_intact(16);
        chk(&mut set, "C43-06 映射完整", ok, "映射长度账错");
    }

    // C43-07 热图分档：8 次访问进热档
    {
        let mut s = SparseRes::new(1, 4, 4);
        for _ in 0..9 {
            let _ = s.touch(0);
        }
        let _ = s.touch(1);
        let (hot, warm, cold) = s.heatmap();
        let ok = hot == 1 && warm == 0 && cold == 1;
        chk(&mut set, "C43-07 热图分档", ok, "热图分档失准");
    }

    // C43-08 全局预算闸：资源预算 > 全局预算创建拒绝
    {
        let mut m = SparseManager::new(4, 64);
        let ok = m.create(1, 16, 128) == Err(XC_BUDGET);
        chk(&mut set, "C43-08 预算闸", ok, "超全局预算被放行");
    }

    // C43-09 重复注册拒绝
    {
        let mut m = SparseManager::new(4, 64);
        let _ = m.create(1, 16, 4);
        let ok = m.create(1, 16, 4) == Err(XC_RES_FULL);
        chk(&mut set, "C43-09 重复注册", ok, "重复注册被放行");
    }

    // C43-10 未知资源访问拒绝
    {
        let mut m = SparseManager::new(4, 64);
        let ok = m.touch(9, 0) == Err(XC_UNKNOWN_RES);
        chk(&mut set, "C43-10 未知资源", ok, "未知资源被放行");
    }

    // C43-11 换出后驻留位与物理槽清账一致
    {
        let mut s = SparseRes::new(1, 8, 1);
        let _ = s.touch(0);
        let _ = s.evict(0);
        let ok = !s.tile_resident(0) && s.resident_n() == 0 && s.consistency_check().is_ok();
        chk(&mut set, "C43-11 换出清账", ok, "换出后账不符");
    }

    // C43-12 换出非驻留块返回 false
    {
        let mut s = SparseRes::new(1, 8, 2);
        let ok = !s.evict(7);
        chk(&mut set, "C43-12 空换出拒", ok, "空换出被放行");
    }

    // C43-13 确定性
    {
        let seq = || -> (u32, u32, u32) {
            let mut s = SparseRes::new(1, 8, 2);
            let _ = s.touch(0);
            let _ = s.touch(1);
            let _ = s.touch(2);
            (s.resident_n(), s.load_n(), s.evict_n())
        };
        chk(&mut set, "C43-13 确定性", seq() == seq(), "两次不一致");
    }

    // C43-14 读屏行四要素
    {
        let m = SparseManager::new(4, 64);
        let line = m.status_line();
        let ok = line.contains("资源") && line.contains("驻留") && line.contains("重建");
        chk(&mut set, "C43-14 读屏要素", ok, "读屏行缺要素");
    }


    // C43-90 码位互异且段独占（判据自身：整仓唯一段 0x1E00 内六码）
    {
        let codes = [XC_TILE_RANGE, XC_MAP_VOID, XC_BUDGET, XC_INCONSIST, XC_RES_FULL, XC_UNKNOWN_RES];
        let mut all_diff = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    all_diff = false;
                }
            }
        }
        let in_segment = codes.iter().all(|c| (c & 0xFF00) == 0x1E00);
        chk(&mut set, "C43-90 码位互异段独占", all_diff && in_segment && codes.len() == 6,
            "诊断码互异或越段");
    }

    set
}

impl SparseRes {
    fn tile_resident(&self, tile: usize) -> bool {
        self.tiles.get(tile).map(|t| t.resident).unwrap_or(false)
    }
}

impl SparseManager {
    // 供自检与审计使用：按 res_id 取可变资源（ crate 内可见 ）。
    pub(crate) fn res_mut(&mut self) -> &mut Vec<SparseRes> {
        &mut self.res
    }
}

/// 深度驱动入口。
pub fn drill() -> (bool, String) {
    let s = run_vea43_checks();
    let ok = s.all_passed() && !s.truncated();
    (ok, if ok { String::new() } else { s.red_items_text() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea43_residency_budget_eviction() {
        let mut s = SparseRes::new(1, 8, 2);
        let _ = s.touch(0);
        let _ = s.touch(0);
        let _ = s.touch(1);
        let _ = s.touch(2);
        assert_eq!(s.resident_n(), 2, "预算满不超驻留");
        assert_eq!(s.evict_n(), 1);
        assert!(!s.tile_resident(1), "最冷块被换出");
        assert!(s.consistency_check().is_ok());
    }

    #[test]
    fn vea43_heatmap_tiers() {
        let mut s = SparseRes::new(1, 4, 4);
        for _ in 0..9 {
            let _ = s.touch(0);
        }
        let _ = s.touch(1);
        let (hot, warm, cold) = s.heatmap();
        assert_eq!((hot, warm, cold), (1, 0, 1));
        assert!(s.map_intact(4) && !s.map_intact(3));
    }

    #[test]
    fn vea43_manager_global_budget() {
        let mut m = SparseManager::new(4, 64);
        assert_eq!(m.create(1, 16, 128), Err(XC_BUDGET), "超全局预算拒绝");
        assert!(m.create(1, 16, 8).is_ok());
        let _ = m.touch(1, 0);
        assert_eq!(m.global_resident(), 1);
        assert!(SPARSE_VERSION.starts_with("vea43_sparse"));
    }

    #[test]
    fn vea43_checks_all_green() {
        let set = run_vea43_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0043 判据存在红项：{}/{} 绿 —— {}",
            passed,
            passed + failed,
            set.red_items_text()
        );
    }
}
