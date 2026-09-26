//! F068 渲染资产按需装载 · 完整设计（STAR I 主册 G-B-28）。
//!
//! **判据（主册）**：单主题驻留内存 ≤80MB（4K 全套实测）；主题切换全程
//! 无白屏闪烁（录屏帧检）。
//!
//! **设计要点（主册）**：
//! - 壁纸/图标集/音效按当前主题（E1）惰性装载：**未选中的主题资产不驻
//!   内存**；内存占用差值入账本；
//! - 10 套官方主题只用一套时，内存里只有那套的资产；切换主题时旧资产
//!   卸载新资产流入（300ms 交叉淡入完成换装）；
//! - 引用计数管理资产生命周期（最后一个引用卸载）；
//! - **解码在后缓冲完成后原子换入**（无中间态上屏——无白屏的结构保证：
//!   换装期先建新驻留再释放旧驻留，峰值 = 旧 + 新，全程驻留不归零）；
//! - 切换时新资产解码失败 → 保持旧主题（不白屏）+ 诊断报备；
//! - 资产文件损坏 → 回退默认主题 + 诊断报备；
//! - 音效文件小（<200KB 全量驻留不按需）；图标集按需粒度 = 单图标
//!   （常用 100 个预驻——declare 顺序即常用序）；
//! - 装载器复用图像解码面（F054——解码成败由调用方注入）；
//! - 诊断面板资产页显示当前驻留清单（本模块驻留账直读）。
//!
//! 无外部依赖。一切时间注入式（分钟戳），宿主测试确定复现。

use crate::checks::CheckSet;
use crate::star::sbase::MinuteBook;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数进旋钮清单）
// ---------------------------------------------------------------------------

/// 单主题驻留内存判线（4K 全套 ≤80MB）。
pub const THEME_RESIDENT_LIMIT_BYTES: u64 = 80 * 1024 * 1024;

/// 音效全量驻留上限（<200KB 不按需）。
pub const SOUND_FULL_RESIDENT_MAX: u64 = 200 * 1024;

/// 常用图标预驻数。
pub const ICON_PRELOAD_COUNT: usize = 100;

/// 主题切换交叉淡入时长（ms）。
pub const CROSSFADE_MS: u32 = 300;

/// 主题注册容量（官方 10 套 + 余量；LRU 逐出最早注册者）。
const THEME_CAP: usize = 10;

/// 诊断报备日志容量。
const NOTE_CAP: usize = 64;

/// 内存差值账本保留窗（分钟）。
const DIFF_BOOK_MIN: u64 = 1440;

// ---------------------------------------------------------------------------
// 资产模型
// ---------------------------------------------------------------------------

/// 资产类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetKind {
    /// 壁纸（按需装载，单件大）。
    Wallpaper,
    /// 图标（按需粒度 = 单图标；常用 100 预驻）。
    Icon,
    /// 音效（<200KB 全量驻留）。
    Sound,
}

/// 资产声明条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssetDef {
    pub name: &'static str,
    pub kind: AssetKind,
    pub bytes: u64,
}

/// 驻留条目（引用计数生命周期）。
#[derive(Clone, Copy, Debug)]
pub struct ResidentEntry {
    pub name: &'static str,
    pub kind: AssetKind,
    pub bytes: u64,
    /// 引用计数（>0 驻留）。
    pub refs: u32,
    /// 主题激活期钉住（音效全量驻留 / 常用图标预驻）。
    pub pinned: bool,
}

/// 主题驻留状态。
#[derive(Clone, Debug)]
struct ThemeState {
    name: &'static str,
    defs: Vec<AssetDef>,
    resident: Vec<ResidentEntry>,
}

/// 主题切换结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchOutcome {
    /// 换装完成（交叉淡入）。
    Swapped {
        /// 释放的内存（旧驻留 − 新驻留；负增长如实给 0——差值口径唯一）。
        freed_bytes: u64,
        /// 双驻留峰值（无白屏证明：峰值 ≥ max(旧, 新)——全程不归零）。
        peak_bytes: u64,
    },
    /// 保持旧主题（不白屏——诚实降级）。
    KeptOld(&'static str),
}

// ---------------------------------------------------------------------------
// 装载器
// ---------------------------------------------------------------------------

/// 渲染资产按需装载器。
pub struct AssetLoader {
    themes: Vec<ThemeState>,
    active: Option<&'static str>,
    default_theme: &'static str,
    /// 内存差值账本（单列：分钟 → 释放字节累计）。
    diff_book: MinuteBook,
    /// 诊断报备（主题, 原因）——损坏回退 / 解码失败直读面。
    notes: Vec<(&'static str, &'static str)>,
    /// 最近一次换装的双驻留峰值。
    last_peak_bytes: u64,
}

impl AssetLoader {
    pub fn new(default_theme: &'static str) -> AssetLoader {
        AssetLoader {
            themes: Vec::new(),
            active: None,
            default_theme,
            diff_book: MinuteBook::new(1, DIFF_BOOK_MIN),
            notes: Vec::new(),
            last_peak_bytes: 0,
        }
    }

    /// 注册主题资产清单（declare 顺序 = 图标常用序）。重复注册拒绝；
    /// 容量满时 LRU 逐出最早注册的**非活动**主题（活动主题豁免）。
    pub fn declare_theme(&mut self, name: &'static str, defs: &[AssetDef]) -> bool {
        if self.themes.iter().any(|t| t.name == name) {
            return false;
        }
        if self.themes.len() >= THEME_CAP {
            let drop_idx =
                match self.themes.iter().position(|t| Some(t.name) != self.active) {
                    Some(i) => i,
                    None => return false, // 理论不可达（活动主题唯一）。
                };
            self.themes.remove(drop_idx);
        }
        self.themes.push(ThemeState { name, defs: defs.to_vec(), resident: Vec::new() });
        true
    }

    fn theme_idx(&self, theme: &str) -> Option<usize> {
        self.themes.iter().position(|t| t.name == theme)
    }

    /// 主题激活（首次/回退通用）：建立 pinned 驻留（音效全量 + 常用图标
    /// 前 100）；壁纸按需。双驻留期由调用方保证（switch 内部已按序执行）。
    fn build_pinned(&mut self, idx: usize) {
        let mut icon_seq = 0usize;
        let defs = self.themes[idx].defs.clone();
        for d in &defs {
            let pinned = match d.kind {
                AssetKind::Sound => d.bytes <= SOUND_FULL_RESIDENT_MAX,
                AssetKind::Icon => {
                    icon_seq += 1;
                    icon_seq <= ICON_PRELOAD_COUNT
                }
                AssetKind::Wallpaper => false,
            };
            if pinned {
                self.themes[idx].resident.push(ResidentEntry {
                    name: d.name,
                    kind: d.kind,
                    bytes: d.bytes,
                    refs: 1,
                    pinned: true,
                });
            }
        }
    }

    /// 初次激活（无切换语义；活动主题已存在时拒绝）。
    pub fn activate(&mut self, theme: &'static str) -> bool {
        if self.active.is_some() {
            return false;
        }
        let idx = match self.theme_idx(theme) {
            Some(i) => i,
            None => return false,
        };
        self.build_pinned(idx);
        self.active = Some(theme);
        true
    }

    /// 按需装载（壁纸/单图标）：refs++。
    pub fn acquire(&mut self, theme: &str, asset: &str) -> bool {
        let idx = match self.theme_idx(theme) {
            Some(i) => i,
            None => return false,
        };
        let def = match self.themes[idx].defs.iter().find(|d| d.name == asset) {
            Some(d) => *d,
            None => return false,
        };
        match self.themes[idx].resident.iter_mut().find(|r| r.name == asset) {
            Some(r) => {
                r.refs += 1;
                true
            }
            None => {
                self.themes[idx].resident.push(ResidentEntry {
                    name: def.name,
                    kind: def.kind,
                    bytes: def.bytes,
                    refs: 1,
                    pinned: false,
                });
                true
            }
        }
    }

    /// 释放：refs-- → 归零且非 pinned → 卸载。返回是否发生卸载。
    pub fn release(&mut self, theme: &str, asset: &str) -> bool {
        let idx = match self.theme_idx(theme) {
            Some(i) => i,
            None => return false,
        };
        let t = &mut self.themes[idx];
        let r = match t.resident.iter_mut().find(|r| r.name == asset) {
            Some(r) => r,
            None => return false,
        };
        if r.refs == 0 {
            return false;
        }
        r.refs -= 1;
        if r.refs == 0 && !r.pinned {
            t.resident.retain(|e| e.name != asset);
            return true;
        }
        false
    }

    /// 主题驻留字节（诊断面板驻留清单总量）。
    pub fn resident_bytes(&self, theme: &str) -> Option<u64> {
        self.theme_idx(theme)
            .map(|i| self.themes[i].resident.iter().map(|r| r.bytes).sum())
    }

    /// 驻留清单直读（诊断面板资产页）。
    pub fn resident_list(&self, theme: &str) -> Vec<ResidentEntry> {
        match self.theme_idx(theme) {
            Some(i) => self.themes[i].resident.clone(),
            None => Vec::new(),
        }
    }

    /// 单主题驻留预算判线（≤80MB）。
    pub fn budget_ok(&self, theme: &str) -> bool {
        matches!(self.resident_bytes(theme), Some(b) if b <= THEME_RESIDENT_LIMIT_BYTES)
    }

    /// 主题切换（原子换入时序）：
    /// 1. 解码（成败由调用方注入）失败 → 保持旧主题 + 报备（不白屏）；
    /// 2. 成功 → 先建新主题 pinned 驻留（峰值 = 旧 + 新，全程不归零），
    ///    再释放旧主题全部驻留（音效 pinned 也随主题卸载——驻留归属主题）；
    /// 3. 内存差值入账本。
    pub fn switch_theme(&mut self, new: &'static str, decode_ok: bool, minute: u64) -> SwitchOutcome {
        let old = match self.active {
            Some(a) => a,
            None => return SwitchOutcome::KeptOld("no active theme"),
        };
        if old == new {
            return SwitchOutcome::KeptOld("same theme");
        }
        let new_idx = match self.theme_idx(new) {
            Some(i) => i,
            None => return SwitchOutcome::KeptOld("unknown theme"),
        };
        if !decode_ok {
            self.note(new, "decode failed: kept old theme (no blank screen)");
            return SwitchOutcome::KeptOld("decode failed");
        }
        let old_bytes = self.resident_bytes(old).unwrap_or(0);
        self.build_pinned(new_idx);
        let peak = old_bytes + self.resident_bytes(new).unwrap_or(0);
        self.last_peak_bytes = peak;

        // 原子换入完成：旧主题驻留整体释放。
        let old_idx = self.theme_idx(old).unwrap();
        self.themes[old_idx].resident.clear();
        self.active = Some(new);

        let new_bytes = self.resident_bytes(new).unwrap_or(0);
        let freed = old_bytes.saturating_sub(new_bytes);
        self.diff_book.record_minute(minute, &[freed]);
        SwitchOutcome::Swapped { freed_bytes: freed, peak_bytes: peak }
    }

    /// 资产损坏 → 回退默认主题 + 诊断报备（theme 入报备面，须 'static——
    /// 与 declare_theme 的主题名同源）。
    pub fn report_corrupt(&mut self, theme: &'static str, minute: u64) -> SwitchOutcome {
        self.note(theme, "asset corrupt: fallback to default theme");
        let d = self.default_theme;
        self.switch_theme(d, true, minute)
    }

    /// 最近一次换装峰值（无白屏证明直读）。
    pub fn last_peak(&self) -> u64 {
        self.last_peak_bytes
    }

    /// 内存差值账本窗口查询。
    pub fn freed_in(&self, now_minute: u64, span_min: u64) -> u64 {
        self.diff_book.range_sum(now_minute.saturating_sub(span_min) + 1, now_minute).iter().sum()
    }

    /// 诊断报备直读。
    pub fn note_log(&self) -> &[(&'static str, &'static str)] {
        &self.notes
    }

    /// 当前活动主题（诊断直读）。
    pub fn active_theme(&self) -> Option<&'static str> {
        self.active
    }

    fn note(&mut self, theme: &'static str, reason: &'static str) {
        if self.notes.len() >= NOTE_CAP {
            self.notes.remove(0);
        }
        self.notes.push((theme, reason));
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// 150 个唯一图标名（真实清单中每个图标名字唯一——按需粒度=单图标）。
const ICON_NAMES: [&str; 150] = [
    "i000", "i001", "i002", "i003", "i004", "i005", "i006", "i007", "i008", "i009",
    "i010", "i011", "i012", "i013", "i014", "i015", "i016", "i017", "i018", "i019",
    "i020", "i021", "i022", "i023", "i024", "i025", "i026", "i027", "i028", "i029",
    "i030", "i031", "i032", "i033", "i034", "i035", "i036", "i037", "i038", "i039",
    "i040", "i041", "i042", "i043", "i044", "i045", "i046", "i047", "i048", "i049",
    "i050", "i051", "i052", "i053", "i054", "i055", "i056", "i057", "i058", "i059",
    "i060", "i061", "i062", "i063", "i064", "i065", "i066", "i067", "i068", "i069",
    "i070", "i071", "i072", "i073", "i074", "i075", "i076", "i077", "i078", "i079",
    "i080", "i081", "i082", "i083", "i084", "i085", "i086", "i087", "i088", "i089",
    "i090", "i091", "i092", "i093", "i094", "i095", "i096", "i097", "i098", "i099",
    "i100", "i101", "i102", "i103", "i104", "i105", "i106", "i107", "i108", "i109",
    "i110", "i111", "i112", "i113", "i114", "i115", "i116", "i117", "i118", "i119",
    "i120", "i121", "i122", "i123", "i124", "i125", "i126", "i127", "i128", "i129",
    "i130", "i131", "i132", "i133", "i134", "i135", "i136", "i137", "i138", "i139",
    "i140", "i141", "i142", "i143", "i144", "i145", "i146", "i147", "i148", "i149",
];

/// 8 个唯一音效名。
const SFX_NAMES: [&str; 8] = ["s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7"];

/// 4K 主题标准资产集（壁纸 60MB + 150 图标 ×96KB + 8 音效 ×64KB）。
fn theme4k_defs() -> Vec<AssetDef> {
    let mut v = Vec::new();
    v.push(AssetDef { name: "wall-4k", kind: AssetKind::Wallpaper, bytes: 60 * 1024 * 1024 });
    for n in ICON_NAMES.iter() {
        v.push(AssetDef { name: n, kind: AssetKind::Icon, bytes: 96 * 1024 });
    }
    for n in SFX_NAMES.iter() {
        v.push(AssetDef { name: n, kind: AssetKind::Sound, bytes: 64 * 1024 });
    }
    v
}

/// 轻量主题（壁纸 30MB + 同款图标/音效——换装差值有真实区分度）。
fn theme_lite_defs() -> Vec<AssetDef> {
    let mut v = Vec::new();
    v.push(AssetDef { name: "wall-lite", kind: AssetKind::Wallpaper, bytes: 30 * 1024 * 1024 });
    for n in ICON_NAMES.iter() {
        v.push(AssetDef { name: n, kind: AssetKind::Icon, bytes: 96 * 1024 });
    }
    for n in SFX_NAMES.iter() {
        v.push(AssetDef { name: n, kind: AssetKind::Sound, bytes: 64 * 1024 });
    }
    v
}

/// F068 自检（判据：单主题 ≤80MB；切换无白屏）。
pub fn run_assetldr_checks() -> CheckSet {
    let mut set = CheckSet::new("F068-assetldr");

    // 1. 激活后 pinned 驻留 = 音效 8×64KB + 常用图标 100×96KB（壁纸按需
    //    不预驻），精确口径 + 在 80MB 判线内。
    let mut ld = AssetLoader::new("base");
    assert!(ld.declare_theme("aurora", &theme4k_defs()));
    assert!(ld.activate("aurora"));
    let pinned = ld.resident_bytes("aurora").unwrap_or(0);
    set.add(
        "pinned resident = sfx + top-100 icons",
        pinned == 8 * 64 * 1024 + 100 * 96 * 1024 && ld.budget_ok("aurora"),
        "",
    );

    // 2. 未选中主题不驻内存：注册第二主题（轻量壁纸）驻留为 0。
    assert!(ld.declare_theme("mist", &theme_lite_defs()));
    set.add("unselected theme not resident", ld.resident_bytes("mist") == Some(0), "");

    // 3. 引用计数生命周期：acquire ×2 → release ×1 仍驻留 → 再 release 卸载。
    assert!(ld.acquire("aurora", "wall-4k"));
    assert!(ld.acquire("aurora", "wall-4k"));
    let with_wall = ld.resident_bytes("aurora").unwrap_or(0);
    assert!(!ld.release("aurora", "wall-4k"));
    assert_eq!(ld.resident_bytes("aurora"), Some(with_wall), "还有 1 个引用");
    assert!(ld.release("aurora", "wall-4k"));
    set.add(
        "refcount unload on last release",
        ld.resident_bytes("aurora") == Some(pinned),
        "",
    );

    // 4. 常用图标前 100 预驻：150 图标注册 → 驻留清单里图标恰 100。
    let icons = ld.resident_list("aurora").iter().filter(|r| r.kind == AssetKind::Icon).count();
    set.add("top-100 icons preloaded", icons == ICON_PRELOAD_COUNT, "");

    // 5. 音效全量驻留且随主题归属（切换后旧音效卸载——见 7）。
    let sfx = ld.resident_list("aurora").iter().filter(|r| r.kind == AssetKind::Sound).count();
    set.add("sounds fully resident", sfx == 8, "");

    // 6. 解码失败 → 保持旧主题（不白屏）+ 报备。
    let before = ld.resident_bytes("aurora").unwrap_or(0);
    let out = ld.switch_theme("mist", false, 1);
    set.add(
        "decode failure keeps old theme",
        out == SwitchOutcome::KeptOld("decode failed")
            && ld.resident_bytes("aurora") == Some(before)
            && ld.resident_bytes("mist") == Some(0)
            && ld.note_log().iter().any(|(t, _)| *t == "mist"),
        "",
    );

    // 7. 换装原子时序：成功 → 峰值 = 旧 + 新（全程驻留不归零——无白屏
    //    证明），旧主题驻留清零，新主题 pinned 建立，差值入账本。
    let out = ld.switch_theme("mist", true, 2);
    let mist = ld.resident_bytes("mist").unwrap_or(0);
    let sfx_after = ld.resident_list("mist").iter().filter(|r| r.kind == AssetKind::Sound).count();
    set.add(
        "atomic swap: peak covers, old cleared, diff booked",
        matches!(out, SwitchOutcome::Swapped { peak_bytes, .. } if peak_bytes == before + mist)
            && ld.resident_bytes("aurora") == Some(0)
            && mist > 0
            && sfx_after == 8
            && ld.last_peak() == before + mist
            && ld.freed_in(2, 1) == before.saturating_sub(mist),
        "",
    );

    // 8. 差值口径唯一：freed = 旧 − 新（saturating——负增长给 0）。
    if let SwitchOutcome::Swapped { freed_bytes, .. } = out {
        set.add("freed diff single source", freed_bytes == before.saturating_sub(mist), "");
    } else {
        set.add("freed diff single source", false, "unexpected outcome");
    }

    // 9. 资产损坏 → 回退默认主题 + 报备。
    let mut ld2 = AssetLoader::new("base");
    assert!(ld2.declare_theme("base", &theme4k_defs()));
    assert!(ld2.declare_theme("aurora", &theme4k_defs()));
    assert!(ld2.activate("aurora"));
    let out2 = ld2.report_corrupt("aurora", 3);
    set.add(
        "corrupt asset falls back to default",
        matches!(out2, SwitchOutcome::Swapped { .. })
            && ld2.resident_bytes("aurora") == Some(0)
            && ld2.resident_bytes("base") > Some(0)
            && ld2.note_log().iter().any(|(t, r)| *t == "aurora" && r.contains("fallback")),
        "",
    );

    // 10. 同主题切换与未知主题 → 拒绝（不产生假换装）。
    let mut ld3 = AssetLoader::new("base");
    assert!(ld3.declare_theme("base", &theme4k_defs()));
    assert!(ld3.activate("base"));
    set.add(
        "same/unknown theme switch rejected",
        ld3.switch_theme("base", true, 1) == SwitchOutcome::KeptOld("same theme")
            && ld3.switch_theme("ghost", true, 1) == SwitchOutcome::KeptOld("unknown theme"),
        "",
    );

    // 11. 主题容量 LRU：第 11 套逐出最早注册者，活动主题豁免。
    let mut ld4 = AssetLoader::new("base");
    for i in 0..10usize {
        let name = match i {
            0 => "t0",
            1 => "t1",
            2 => "t2",
            3 => "t3",
            4 => "t4",
            5 => "t5",
            6 => "t6",
            7 => "t7",
            8 => "t8",
            _ => "t9",
        };
        assert!(ld4.declare_theme(name, &theme4k_defs()));
    }
    assert!(ld4.activate("t0"));
    assert!(ld4.declare_theme("t10", &theme4k_defs()), "第 11 套可注册（t0 活动豁免，逐出最早的 t1）");
    set.add(
        "theme lru with active exemption",
        ld4.resident_bytes("t1").is_none() && ld4.resident_bytes("t0").is_some() && ld4.resident_bytes("t10").is_some(),
        "",
    );

    // 12. 4K 全套实测预算（主册判据）：壁纸 + 150 图标 + 8 音效全驻 =
    //     60MB + 150×96KB + 8×64KB ≈ 74.6MB ≤ 80MB。
    let mut ld5 = AssetLoader::new("base");
    assert!(ld5.declare_theme("aurora", &theme4k_defs()));
    assert!(ld5.activate("aurora"));
    assert!(ld5.acquire("aurora", "wall-4k"), "壁纸按需装载");
    for n in ICON_NAMES.iter() {
        assert!(ld5.acquire("aurora", n));
    }
    let full = ld5.resident_bytes("aurora").unwrap_or(0);
    set.add(
        "4k full set within 80MB",
        full == 60 * 1024 * 1024 + 150 * 96 * 1024 + 8 * 64 * 1024 && ld5.budget_ok("aurora"),
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use alloc::vec;
    use super::*;

    fn tiny_defs() -> Vec<AssetDef> {
        let mut v = Vec::new();
        v.push(AssetDef { name: "wp", kind: AssetKind::Wallpaper, bytes: 1024 });
        for n in ICON_NAMES.iter().take(120) {
            v.push(AssetDef { name: n, kind: AssetKind::Icon, bytes: 64 });
        }
        v.push(AssetDef { name: "sfx", kind: AssetKind::Sound, bytes: 128 });
        v
    }

    #[test]
    fn icon_preload_boundary() {
        let mut ld = AssetLoader::new("d");
        assert!(ld.declare_theme("t", &tiny_defs()));
        assert!(ld.activate("t"));
        let icons = ld.resident_list("t").iter().filter(|r| r.kind == AssetKind::Icon).count();
        assert_eq!(icons, ICON_PRELOAD_COUNT, "第 101 个图标不预驻");
        // 第 101 个（i100——declare 序即 i000 起）按需可装载。
        assert!(ld.acquire("t", "i100"));
        let icons2 = ld.resident_list("t").iter().filter(|r| r.kind == AssetKind::Icon).count();
        assert_eq!(icons2, ICON_PRELOAD_COUNT + 1);
    }

    #[test]
    fn sound_over_200kb_not_pinned() {
        let mut ld = AssetLoader::new("d");
        let defs = vec![AssetDef { name: "big-sfx", kind: AssetKind::Sound, bytes: 300 * 1024 }];
        assert!(ld.declare_theme("t", &defs));
        assert!(ld.activate("t"));
        assert_eq!(ld.resident_bytes("t"), Some(0), "超 200KB 音效不满足全量驻留条件");
        assert!(ld.acquire("t", "big-sfx"));
        assert_eq!(ld.resident_bytes("t"), Some(300 * 1024));
        assert!(ld.release("t", "big-sfx"), "非 pinned → 最后引用卸载");
        assert_eq!(ld.resident_bytes("t"), Some(0));
    }

    #[test]
    fn duplicate_theme_decline() {
        let mut ld = AssetLoader::new("d");
        assert!(ld.declare_theme("t", &tiny_defs()));
        assert!(!ld.declare_theme("t", &tiny_defs()));
    }

    #[test]
    fn activate_twice_declined() {
        let mut ld = AssetLoader::new("d");
        assert!(ld.declare_theme("t", &tiny_defs()));
        assert!(ld.activate("t"));
        assert!(!ld.activate("t"), "已有活动主题");
        assert!(!ld.activate("ghost"));
    }

    #[test]
    fn switch_to_same_or_unknown_is_kept() {
        let mut ld = AssetLoader::new("d");
        assert!(ld.declare_theme("a", &tiny_defs()));
        assert!(ld.declare_theme("b", &tiny_defs()));
        assert!(ld.activate("a"));
        assert_eq!(ld.switch_theme("a", true, 1), SwitchOutcome::KeptOld("same theme"));
        assert_eq!(ld.switch_theme("zz", true, 1), SwitchOutcome::KeptOld("unknown theme"));
        assert_eq!(ld.last_peak(), 0, "未发生换装无峰值");
    }

    #[test]
    fn peak_never_zero_across_swap() {
        let mut ld = AssetLoader::new("d");
        assert!(ld.declare_theme("a", &tiny_defs()));
        assert!(ld.declare_theme("b", &tiny_defs()));
        assert!(ld.activate("a"));
        let ra = ld.resident_bytes("a").unwrap_or(0);
        assert!(ra > 0);
        let out = ld.switch_theme("b", true, 1);
        if let SwitchOutcome::Swapped { peak_bytes, .. } = out {
            assert!(peak_bytes >= ra, "双驻留峰值覆盖旧驻留——全程不归零");
        } else {
            panic!("应成功换装");
        }
        assert!(ld.resident_bytes("b").unwrap_or(0) > 0);
    }

    #[test]
    fn freed_book_window() {
        let mut ld = AssetLoader::new("d");
        assert!(ld.declare_theme("a", &tiny_defs()));
        assert!(ld.declare_theme("b", &tiny_defs()));
        assert!(ld.activate("a"));
        assert!(ld.acquire("a", "wp"), "壁纸按需驻留——换装释放差值的来源");
        let _ = ld.switch_theme("b", true, 10);
        assert!(ld.freed_in(10, 1) > 0);
        assert_eq!(ld.freed_in(10, 0), 0, "空窗");
    }

    #[test]
    fn corrupt_fallback_when_no_default_declared() {
        // 默认主题未注册 → 回退换装失败 → KeptOld（诚实：退无可退则保持）。
        let mut ld = AssetLoader::new("ghost");
        assert!(ld.declare_theme("a", &tiny_defs()));
        assert!(ld.activate("a"));
        let out = ld.report_corrupt("a", 1);
        assert_eq!(out, SwitchOutcome::KeptOld("unknown theme"));
        assert_eq!(ld.active_theme(), Some("a"), "退无可退保持当前主题");
    }
}

