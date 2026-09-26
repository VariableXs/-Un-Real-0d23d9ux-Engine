//! F298 快速设置磁贴编辑 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：编辑三动作（重排/移除/添加）用例；默认 8 磁贴
//! 清单；候选池完整性；恢复默认；持久化重启验证。
//!
//! **设计要点（主册）**：快速设置面板磁贴可编辑：长按进入编辑态（磁贴
//! 抖动提示、可拖重排、点 × 移除、面板底部「添加」列出候选磁贴），布局
//! 持久化；默认 8 磁贴（Wi-Fi/蓝牙/飞行/夜间模式/亮度/音量/投影/省电），
//! 候选池 16 个（含专注模式、就近共享等）；编辑态与使用态切换顺滑；
//! 恢复默认一键。
//!
//! 实装：磁贴布局（默认 8 清单唯一源 + 候选池 16 完整性）；编辑三动作
//! （重排/移除/添加）；候选池与现役互斥（在用的不出现在候选里）；恢复
//! 默认一键；持久化快照可逆。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 默认 8 磁贴清单（唯一源——判据原文顺序）。
pub const DEFAULT_TILES: [&str; 8] = [
    "Wi-Fi",
    "蓝牙",
    "飞行模式",
    "夜间模式",
    "亮度",
    "音量",
    "投影",
    "省电",
];

/// 候选池 16 个（含默认 8 + 备选 8——完整性判据的对照表）。
pub const CANDIDATE_POOL: [&str; 16] = [
    "Wi-Fi",
    "蓝牙",
    "飞行模式",
    "夜间模式",
    "亮度",
    "音量",
    "投影",
    "省电",
    "专注模式",
    "就近共享",
    "辅助功能",
    "截图",
    "录屏",
    "热点",
    "无线显示",
    "深浅主题",
];

/// 快速设置磁贴面板。
pub struct QuickTiles {
    /// 现役磁贴（有序——重排直接改序）。
    pub active: Vec<String>,
}

impl QuickTiles {
    pub fn defaults() -> QuickTiles {
        QuickTiles {
            active: DEFAULT_TILES.iter().map(|t| String::from(*t)).collect(),
        }
    }

    /// 重排：from → to。
    pub fn reorder(&mut self, from: usize, to: usize) -> bool {
        if from >= self.active.len() || to >= self.active.len() {
            return false;
        }
        let t = self.active.remove(from);
        self.active.insert(to, t);
        true
    }

    /// 移除。
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.active.len();
        self.active.retain(|t| t != name);
        self.active.len() != before
    }

    /// 添加（从候选池；现役重复添加拒绝；不在候选池的拒绝）。
    pub fn add(&mut self, name: &str) -> Result<(), &'static str> {
        if self.active.iter().any(|t| t == name) {
            return Err("磁贴已在面板中");
        }
        if !CANDIDATE_POOL.contains(&name) {
            return Err("不在候选池中");
        }
        self.active.push(String::from(name));
        Ok(())
    }

    /// 候选列表：候选池 − 现役（在用的不出现）。
    pub fn candidates(&self) -> Vec<&'static str> {
        CANDIDATE_POOL
            .iter()
            .copied()
            .filter(|c| !self.active.iter().any(|t| t == c))
            .collect()
    }

    /// 恢复默认一键。
    pub fn reset(&mut self) {
        self.active = DEFAULT_TILES.iter().map(|t| String::from(*t)).collect();
    }

    /// 持久化快照（重启验证——可逆）。
    pub fn snapshot(&self) -> Vec<String> {
        self.active.clone()
    }

    pub fn restore(&mut self, snap: Vec<String>) {
        self.active = snap;
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_tileedit_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F298");
    let mut qt = QuickTiles::defaults();
    // 默认 8 磁贴清单。
    set.add(
        "F298 default 8",
        qt.active.len() == 8 && qt.active[0] == "Wi-Fi" && qt.active[7] == "省电",
        "canonical list",
    );
    // 候选池完整性：16 个、含专注模式与就近共享。
    set.add(
        "F298 pool complete",
        CANDIDATE_POOL.len() == 16
            && CANDIDATE_POOL.contains(&"专注模式")
            && CANDIDATE_POOL.contains(&"就近共享"),
        "16 entries",
    );
    // 编辑三动作。
    set.add("F298 reorder", qt.reorder(0, 2) && qt.active[2] == "Wi-Fi", "drag");
    set.add(
        "F298 remove",
        qt.remove("夜间模式") && qt.active.len() == 7,
        "× remove",
    );
    let added = qt.add("专注模式");
    set.add(
        "F298 add",
        added.is_ok() && qt.active.last().map(|t| t.as_str()) == Some("专注模式"),
        "from pool",
    );
    // 候选列表与现役互斥。
    let cands = qt.candidates();
    set.add(
        "F298 candidates exclusive",
        cands.len() == 8
            && cands.iter().all(|c| !qt.active.iter().any(|a| a == c))
            && qt.add("Wi-Fi").is_err(),
        "no dup, no leak",
    );
    // 持久化 + 恢复默认。
    let snap = qt.snapshot();
    let mut qt2 = QuickTiles::defaults();
    qt2.restore(snap);
    let same = qt2.active == qt.active;
    qt2.reset();
    set.add(
        "F298 persist+reset",
        same && qt2.active == QuickTiles::defaults().active,
        "round-trip + one-click",
    );
    // --- 深化批次二：编辑态状态机（抖动走 h2curve 弹性档——接线对账）。 ---
    set.add(
        "F298 wobble spec wired",
        crate::h2star::h2curve::motion_of("F298.wobble").map(|s| s.duration_ms == 300).unwrap_or(false),
        "h2curve elastic 300ms",
    );
    // 排布引擎接线：8 默认磁贴在标准面板宽内单行放下（h2geo 流式）。
    let spans8 = [crate::h2star::h2geo::TileSpan::Small; 8];
    let panel = 8 * crate::h2star::h2geo::TILE_CELL_PX + 7 * 8;
    let laid = crate::h2star::h2geo::flow_tiles(&spans8, panel, 8);
    set.add(
        "F298 flow layout wired",
        laid.len() == 8 && laid.iter().all(|r| r.y == 0),
        "h2geo one-row panel",
    );
    // 持久化接线：磁贴布局经 h2snap 编解码 round-trip（id = 候选池下标）。
    let id_of = |t: &str| CANDIDATE_POOL.iter().position(|c| *c == t).unwrap() as u32;
    let enc = crate::h2star::h2snap::encode_tiles(&[
        (id_of("音量"), 0),
        (id_of("Wi-Fi"), 0),
        (id_of("专注模式"), 2),
    ]);
    let dec = crate::h2star::h2snap::decode_tiles(&enc);
    set.add(
        "F298 snapshot roundtrip",
        dec.as_ref().map(|d| d.len() == 3 && d[0].0 == id_of("音量")).unwrap_or(false),
        "h2snap tile encode/decode",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f298_tile_edit() {
        let set = run_tileedit_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F298 自检红 {f}/{p}");
    }

    #[test]
    fn non_pool_add_rejected() {
        let mut qt = QuickTiles::defaults();
        assert!(qt.add("不存在磁贴").is_err(), "候选池外进不来——白名单纪律");
    }
}
