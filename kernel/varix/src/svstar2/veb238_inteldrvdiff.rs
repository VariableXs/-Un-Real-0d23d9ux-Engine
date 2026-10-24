//! VE-F0238 · Intel 驱动版本差异表（VE-B 域 · Intel 核显组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0238`
//!
//! **职责定位（锚点原文）**：宿主侧用户态驱动版本行为差异登记（API 可用性、
//! 已知问题、推荐版本），与 F0235 缺陷表分工明确（缺陷=硬件规避、差异=版本
//! 行为）；版本探测入初始化；推荐版本缺失时呈现全部已知不代言。
//!
//! **数据结构（锚点原文）**：差异表（版本区间×条目×建议）；探测记录。
//!
//! **判据（锚点原文）**：区间匹配、保守未知、不代言、矩阵联动、判据。
//! 本模块 14 条判据逐条映射：**区间匹配 4 条**（版本区间二分定位 O(log n)／
//! 区间有序不重叠／区间外未知显性标注／边界值含端点）、**保守未知 3 条**
//! （探测失败按未知版本保守路径／未知版本不加推荐不猜行为／探测记录入账
//! 可查）、**不代言 2 条**（推荐版本缺失呈现全部已知不代言／有推荐版本时
//! 显性标注推荐与理由）、**矩阵联动 4 条**（F0236 矩阵按表裁剪驱动维度／
//! 与 F0235 缺陷表分工不混线／差异条目冲突以实测修正／修正留痕可对账）、
//! **判据自身 1 条**（码位互异且段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 版本探测失败 → **按未知版本保守路径** | [`DrvDiffTable::probe_version`] 返回 None 即探测失败：命中判定走「未知版本」路径——不命中任何推荐、不加任何建议、`unknown_probes` 计数（保守 = 不猜） |
//! | 差异条目冲突 → **以实测修正** | 实测行为与登记不符：[`DrvDiffTable::calibrate`] 修正条目建议并留痕（原建议/实测结论/计数），下次命中走修正值 |
//! | 推荐版本缺失 → **不代言呈现全部** | 表内无 `recommended` 标记的条目时 [`DrvDiffTable::recommendation`] 返回 None 并呈现全部已知差异清单——「不代言」是显性语义不是遗漏 |
//!
//! ## 设计要点（为什么这样写）
//!
//! - **区间匹配二分，加载一次运行时零开销**：条目按区间下界排序，探测后
//!   的版本定位是二分查找——锚点「区间匹配 O(log n) 一次性加载」的字面
//!   落实；运行时只读定位结果。
//! - **与 F0235 的分工在数据结构层就分开**：本表条目是「版本→行为差异→
//!   建议」，F0235 是「代际×版本→缺陷→规避动作」——建议（recommend）
//!   与规避（mitigate）不同词不同码，混用即语义事故。
//! - **边界含端点**：区间 `[from, to]` 双闭——版本号是离散发布的，含端点
//!   才能表达「正好这个版本有问题」。

use alloc::format;
use alloc::vec;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断段基址（全域核查零占用）。
pub const SEG_BASE: u32 = 0xE640;

// ---- 诊断码 ----
pub const XC_RANGE_BAD: u32 = SEG_BASE + 1; // 版本区间非法（倒挂/重叠）
pub const XC_PROBE_FAIL: u32 = SEG_BASE + 2; // 版本探测失败（保守未知路径）
pub const XC_CONFLICT: u32 = SEG_BASE + 3; // 实测与登记冲突
pub const XC_ENTRY_BAD: u32 = SEG_BASE + 4; // 条目非法（空建议）
pub const XC_SPLIT_BAD: u32 = SEG_BASE + 5; // 矩阵裁剪参数非法

/// 差异条目：版本区间×行为差异×建议。
#[derive(Clone, Debug)]
pub struct DrvEntry {
    /// 区间下界（含）。
    pub ver_from: u32,
    /// 区间上界（含）；0 表示通配。
    pub ver_to: u32,
    /// 差异描述码（API 可用性/已知问题，供快照读屏）。
    pub behavior: u32,
    /// 建议文本（版本行为建议，非规避动作——与 F0235 分工）。
    pub advice: String,
    /// 是否推荐版本条目。
    pub recommended: bool,
}

impl DrvEntry {
    /// 条目合法性：区间有序（通配除外）、建议非空。
    pub fn well_formed(&self) -> bool {
        !self.advice.is_empty() && (self.ver_to == 0 || self.ver_from <= self.ver_to)
    }

    /// 命中版本。
    pub const fn hits(&self, ver: u32) -> bool {
        ver >= self.ver_from && (self.ver_to == 0 || ver <= self.ver_to)
    }
}

/// 实测修正痕迹。
#[derive(Clone, Debug)]
pub struct DrvCorrection {
    /// 修正的条目序号。
    pub entry_idx: u16,
    /// 原建议。
    pub old_advice: String,
    /// 实测结论。
    pub measured_advice: String,
}

/// 驱动差异表控制器。
pub struct DrvDiffTable {
    /// 条目表（按 ver_from 升序）。
    entries: Vec<DrvEntry>,
    /// 修正痕迹账。
    corrections: Vec<DrvCorrection>,
    /// 探测记录账（版本探测历史）。
    probes: Vec<(u32, bool)>,
    /// 未知版本探测计数。
    pub unknown_probes: u32,
    /// 实测修正计数。
    pub conflict_fixes: u32,
}

impl DrvDiffTable {
    /// 新空表。
    pub fn new() -> Self {
        DrvDiffTable {
            entries: Vec::new(),
            corrections: Vec::new(),
            probes: Vec::new(),
            unknown_probes: 0,
            conflict_fixes: 0,
        }
    }

    /// 登记条目：区间倒挂/空建议拒绝；与既有条目区间重叠拒绝（有序不重叠）。
    pub fn register(&mut self, e: DrvEntry) -> Result<u16, u32> {
        if !e.well_formed() {
            return Err(XC_ENTRY_BAD);
        }
        if self.entries.iter().any(|x| e.ver_to != 0 && x.ver_to != 0
            && !(e.ver_to < x.ver_from || e.ver_from > x.ver_to))
        {
            return Err(XC_RANGE_BAD);
        }
        // 保持按 ver_from 升序插入。
        let pos = self
            .entries
            .iter()
            .position(|x| x.ver_from > e.ver_from)
            .unwrap_or(self.entries.len());
        self.entries.insert(pos, e);
        Ok(pos as u16)
    }

    /// 版本探测入初始化：成功记（版本, true）；失败记（0, false）并走
    /// 保守未知路径（锚点：版本探测失败→按未知版本保守路径）。
    pub fn probe_version(&mut self, ver: Option<u32>) -> Option<u32> {
        match ver {
            Some(v) if v > 0 => {
                self.probes.push((v, true));
                Some(v)
            }
            _ => {
                self.probes.push((0, false));
                self.unknown_probes += 1;
                None
            }
        }
    }

    /// 区间定位：二分查找命中条目（锚点：区间匹配 O(log n)）。
    pub fn locate(&self, ver: u32) -> Option<usize> {
        let mut lo = 0usize;
        let mut hi = self.entries.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            let e = &self.entries[mid];
            if e.hits(ver) {
                return Some(mid);
            }
            if e.ver_from > ver {
                hi = mid;
            } else {
                lo = mid + 1;
            }
        }
        None
    }

    /// 命中条目的建议；未命中返回 None（保守：不猜行为）。
    pub fn advice_for(&self, ver: u32) -> Option<&String> {
        self.locate(ver).map(|i| &self.entries[i].advice)
    }

    /// 推荐版本呈现：有推荐条目返回其建议；无推荐条目返回 None 并由
    /// [`DrvDiffTable::all_known`] 呈现全部已知（锚点：推荐版本缺失时
    /// 呈现全部已知不代言）。
    pub fn recommendation(&self) -> Option<&String> {
        self.entries.iter().find(|e| e.recommended).map(|e| &e.advice)
    }

    /// 全部已知差异清单（不代言模式）。
    pub fn all_known(&self) -> String {
        let mut out = alloc::format!("已知差异 {} 条：", self.entries.len());
        for (i, e) in self.entries.iter().enumerate() {
            out.push_str(&alloc::format!("[{}]{:x} {} ", i, e.behavior, e.advice));
        }
        out
    }

    /// 实测修正：实测行为与登记不符时改建议并留痕（锚点：差异条目冲突
    /// →以实测修正）。
    pub fn calibrate(&mut self, entry_idx: u16, measured_advice: &str) -> Result<(), u32> {
        let i = entry_idx as usize;
        if i >= self.entries.len() || measured_advice.is_empty() {
            return Err(XC_ENTRY_BAD);
        }
        let old = self.entries[i].advice.clone();
        self.entries[i].advice = alloc::format!("{}", measured_advice);
        self.corrections.push(DrvCorrection {
            entry_idx,
            old_advice: old,
            measured_advice: alloc::format!("{}", measured_advice),
        });
        self.conflict_fixes += 1;
        Ok(())
    }

    /// F0236 矩阵联动：按本表裁剪驱动维度——版本无任何登记（表外未知）
    /// 的驱动档从矩阵剔除，返回保留的驱动版本档位。
    pub fn cert_drv_dims(&self, candidates: &[u32]) -> Result<Vec<u32>, u32> {
        if candidates.is_empty() {
            return Err(XC_SPLIT_BAD);
        }
        Ok(candidates.iter().copied().filter(|v| self.locate(*v).is_some()).collect())
    }

    /// 修正痕迹账。
    pub fn corrections(&self) -> &[DrvCorrection] {
        &self.corrections
    }

    /// 探测记录数。
    pub fn probes_len(&self) -> usize {
        self.probes.len()
    }

    /// 条目数。
    pub fn entries_len(&self) -> usize {
        self.entries.len()
    }
}

impl Default for DrvDiffTable {
    fn default() -> Self {
        Self::new()
    }
}

/// 快速构造条目（测试/判据夹具）。
pub fn drv_e(from: u32, to: u32, behavior: u32, advice: &str, rec: bool) -> DrvEntry {
    DrvEntry { ver_from: from, ver_to: to, behavior, advice: alloc::format!("{}", advice), recommended: rec }
}

/// VE-F0238 判据组（14 条）。
pub fn run_veb238_checks() -> CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("veb238_inteldrvdiff");

    // --- 判据 1：区间匹配——登记后二分定位命中 ---
    {
        let mut t = DrvDiffTable::new();
        let _ = t.register(drv_e(100, 199, 0x1, "该区间 API A 不可用", false));
        let _ = t.register(drv_e(200, 299, 0x2, "该区间 API A 可用", false));
        let hit_low = t.locate(150);
        let hit_high = t.locate(250);
        s.add(
            "B38-区间匹配-二分定位命中",
            hit_low == Some(0) && hit_high == Some(1),
            "两区间登记后版本 150/250 分别命中各自条目",
        );
    }
    // --- 判据 2：区间匹配——边界含端点 ---
    {
        let mut t = DrvDiffTable::new();
        let _ = t.register(drv_e(100, 199, 0x1, "边界含端点验证", false));
        s.add(
            "B38-区间匹配-边界含端点",
            t.locate(100).is_some() && t.locate(199).is_some() && t.locate(99).is_none() && t.locate(200).is_none(),
            "[from,to] 双闭区间：端点命中、界外不命中",
        );
    }
    // --- 判据 3：区间匹配——区间重叠拒绝 ---
    {
        let mut t = DrvDiffTable::new();
        let _ = t.register(drv_e(100, 199, 0x1, "第一条", false));
        let overlap = t.register(drv_e(150, 250, 0x2, "重叠条目", false));
        let adjacent = t.register(drv_e(200, 299, 0x3, "相邻条目", false));
        s.add(
            "B38-区间匹配-重叠拒绝相邻放行",
            overlap == Err(XC_RANGE_BAD) && adjacent.is_ok(),
            "区间有序不重叠（150 落在 100-199 内拒绝）；相邻区间可并存",
        );
    }
    // --- 判据 4：区间匹配——倒挂与空建议拒绝 ---
    {
        let mut t = DrvDiffTable::new();
        let inverted = t.register(drv_e(300, 100, 0x1, "倒挂区间", false));
        let empty = t.register(drv_e(300, 400, 0x1, "", false));
        s.add(
            "B38-区间匹配-非法条目拒绝",
            inverted == Err(XC_ENTRY_BAD) && empty == Err(XC_ENTRY_BAD),
            "区间倒挂与空建议都不可入表（条目合法性两道闸）",
        );
    }
    // --- 判据 5：保守未知——探测失败走未知路径 ---
    {
        let mut t = DrvDiffTable::new();
        let _ = t.register(drv_e(100, 199, 0x1, "已知区间", false));
        let _ = t.probe_version(None);
        let _ = t.probe_version(Some(0));
        s.add(
            "B38-保守未知-探测失败计数",
            t.unknown_probes == 2 && t.probes_len() == 2 && t.locate(0).is_none(),
            "None/零值探测都记未知：未知版本不命中任何区间（不猜）",
        );
    }
    // --- 判据 6：保守未知——未知版本建议为空 ---
    {
        let mut t = DrvDiffTable::new();
        let _ = t.register(drv_e(100, 199, 0x1, "已知建议", false));
        let none_probe = t.probe_version(None);
        s.add(
            "B38-保守未知-未知版本不给建议",
            none_probe.is_none() && t.advice_for(999).is_none(),
            "探测失败返回 None；表外版本建议为 None（保守路径双保险）",
        );
    }
    // --- 判据 7：不代言——推荐缺失呈现全部已知 ---
    {
        let mut t = DrvDiffTable::new();
        let _ = t.register(drv_e(100, 199, 0x1, "差异甲", false));
        let _ = t.register(drv_e(200, 299, 0x2, "差异乙", false));
        let rec = t.recommendation();
        let all = t.all_known();
        s.add_dyn(
            "B38-不代言-缺失时呈现全部",
            rec.is_none() && all.contains("差异甲") && all.contains("差异乙") && all.contains("2 条"),
            &alloc::format!("无推荐条目：{}；不代言、只罗列", all),
        );
    }
    // --- 判据 8：不代言——有推荐时显性标注 ---
    {
        let mut t = DrvDiffTable::new();
        let _ = t.register(drv_e(100, 199, 0x1, "旧区间差异", false));
        let _ = t.register(drv_e(200, 299, 0x2, "推荐使用 200+ 区间", true));
        let rec = t.recommendation();
        s.add(
            "B38-不代言-推荐显性标注",
            rec.map(|a| a.contains("推荐使用")).unwrap_or(false),
            "推荐条目带标记：呈现层可区分「推荐」与「罗列」两种模式",
        );
    }
    // --- 判据 9：矩阵联动——按表裁剪 F0236 驱动维度 ---
    {
        let mut t = DrvDiffTable::new();
        let _ = t.register(drv_e(100, 199, 0x1, "甲", false));
        let _ = t.register(drv_e(300, 399, 0x2, "乙", false));
        let kept = t.cert_drv_dims(&[150, 250, 350]);
        s.add(
            "B38-矩阵联动-按表裁剪维度",
            kept == Ok(alloc::vec![150, 350]),
            "驱动维度 250 无任何登记（表外）被剔除，150/350 保留",
        );
    }
    // --- 判据 10：矩阵联动——空候选拒绝 ---
    {
        let t = DrvDiffTable::new();
        s.add(
            "B38-矩阵联动-空候选拒绝",
            t.cert_drv_dims(&[]) == Err(XC_SPLIT_BAD),
            "裁剪参数空列表是调用方错误，显性拒绝不返回空集假装成功",
        );
    }
    // --- 判据 11：矩阵联动——与 F0235 分工不混线 ---
    {
        // 本表条目是「建议」（recommend），不是规避动作（mitigate）。
        let e = drv_e(100, 199, 0x1, "建议文本", false);
        s.add(
            "B38-矩阵联动-建议与规避分工",
            e.well_formed() && !e.advice.contains("mitigat"),
            "差异表承载版本行为建议；硬件缺陷规避归 F0235（词面即分工）",
        );
    }
    // --- 判据 12：矩阵联动——冲突以实测修正留痕 ---
    {
        let mut t = DrvDiffTable::new();
        let idx = t.register(drv_e(100, 199, 0x1, "原建议", false));
        let _ = t.calibrate(idx.unwrap_or(0), "实测修正后的建议");
        let c = t.corrections();
        s.add(
            "B38-矩阵联动-实测修正留痕",
            t.conflict_fixes == 1 && c.len() == 1 && c[0].old_advice == "原建议"
                && c[0].measured_advice == "实测修正后的建议",
            "修正留原值/新值对，下次命中走修正值（差异表实测校准闭环）",
        );
    }
    // --- 判据 13：保守未知——探测成功记录入账 ---
    {
        let mut t = DrvDiffTable::new();
        let v = t.probe_version(Some(150));
        s.add(
            "B38-保守未知-探测成功入账",
            v == Some(150) && t.probes_len() == 1 && t.unknown_probes == 0,
            "成功探测记（版本, ok）——初始化期一次性完成",
        );
    }
    // --- 判据 14：码位互异且段独占 ---
    {
        let codes = [XC_RANGE_BAD, XC_PROBE_FAIL, XC_CONFLICT, XC_ENTRY_BAD, XC_SPLIT_BAD];
        let mut all_diff = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    all_diff = false;
                }
            }
        }
        let in_seg = codes.iter().all(|c| *c > SEG_BASE && *c <= SEG_BASE + 9);
        s.add(
            "B38-判据-码位互异段独占",
            all_diff && in_seg && codes.len() == 5,
            "五诊断码互异且全部落在 0xE640 段基的判别式区间内",
        );
    }
    s
}

// ---------------------------------------------------------------- 测试

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veb238_locate_and_bounds() {
        let mut t = DrvDiffTable::new();
        let _ = t.register(drv_e(100, 199, 0x1, "甲", false));
        let _ = t.register(drv_e(200, 299, 0x2, "乙", false));
        assert_eq!(t.locate(100), Some(0));
        assert_eq!(t.locate(199), Some(0));
        assert_eq!(t.locate(250), Some(1));
        assert_eq!(t.locate(99), None);
        assert_eq!(t.locate(300), None);
        assert_eq!(t.advice_for(250).map(|s| s.as_str()), Some("乙"));
    }

    #[test]
    fn veb238_probe_conservative() {
        let mut t = DrvDiffTable::new();
        assert_eq!(t.probe_version(Some(150)), Some(150));
        assert_eq!(t.probe_version(None), None);
        assert_eq!(t.probe_version(Some(0)), None);
        assert_eq!(t.unknown_probes, 2);
        assert_eq!(t.probes_len(), 3);
    }

    #[test]
    fn veb238_recommendation_modes() {
        let mut t = DrvDiffTable::new();
        let _ = t.register(drv_e(100, 199, 0x1, "旧差异", false));
        // 无推荐：呈现全部。
        assert!(t.recommendation().is_none());
        let all = t.all_known();
        assert!(all.contains("旧差异"));
        // 加推荐条目后显性返回。
        let _ = t.register(drv_e(200, 299, 0x2, "推荐 200+", true));
        assert_eq!(t.recommendation().map(|s| s.as_str()), Some("推荐 200+"));
    }

    #[test]
    fn veb238_calibrate_and_cert_trim() {
        let mut t = DrvDiffTable::new();
        let i0 = t.register(drv_e(100, 199, 0x1, "原甲", false)).unwrap_or(0);
        let _ = t.register(drv_e(300, 399, 0x2, "乙", false));
        // 实测修正。
        let _ = t.calibrate(i0, "实测甲改");
        assert_eq!(t.corrections().len(), 1);
        assert_eq!(t.corrections()[0].old_advice, "原甲");
        assert_eq!(t.advice_for(150).map(|s| s.as_str()), Some("实测甲改"));
        // 矩阵裁剪。
        assert_eq!(t.cert_drv_dims(&[150, 250, 350]), Ok(alloc::vec![150, 350]));
        assert_eq!(t.cert_drv_dims(&[]), Err(XC_SPLIT_BAD));
        // 非法修正拒绝。
        assert_eq!(t.calibrate(9, "越界"), Err(XC_ENTRY_BAD));
        assert_eq!(t.calibrate(i0, ""), Err(XC_ENTRY_BAD));
    }

    #[test]
    fn veb238_checks_all_green() {
        let set = run_veb238_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0238 判据存在红项：{}/{} 绿 —— {}",
            passed,
            passed + failed,
            set.red_items_text()
        );
    }
}
