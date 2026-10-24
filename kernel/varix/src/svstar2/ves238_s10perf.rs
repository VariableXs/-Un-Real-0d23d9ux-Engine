//! VE-F3984 · S 域性能总基准
//!
//! 终审：S 域性能总基准合成（八组基准并入 + 五档阈值复核（渲染增量/
//! 色标切换/快照生成/朗读排队/工具开销）+ 与家族基准对拍）；预算基线
//! 冻结前最后一核。
//! 数据结构：总基准册；阈值复核表；对拍记录。
//! 错误路径：阈值越界→重议或降级声明；对拍分歧→仲裁；基线未冻→阻断
//! F3990。
//! 性能：合成 O(组)；复核 O(档)；对拍 O(基线)。
//! 跨批对接：F3968 上游；F3990 冻结前置。
//! 无障碍与隐私：基准含无障碍专项开销档并设独立阈值（域本色）；无隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x3EC0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x3EC0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T4PCode(pub u16);

impl T4PCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const THRESHOLD_OVER: Self = Self(SEG_BASE + 1);
    pub const PEER_SPLIT: Self = Self(SEG_BASE + 2);
    pub const BASELINE_BLOCK: Self = Self(SEG_BASE + 3);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::THRESHOLD_OVER => "阈值越界→重议或降级声明",
            Self::PEER_SPLIT => "对拍分歧→仲裁",
            Self::BASELINE_BLOCK => "基线未冻→阻断 F3990",
            _ => "未知",
        }
    }
}

// ── 五档阈值 ─────────────────────────────────────────────────────────

/// 基准五档（含无障碍专项档——域本色）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BenchSlot {
    /// 渲染增量。
    RenderDelta,
    /// 色标切换。
    ColorSwitch,
    /// 快照生成。
    SnapshotGen,
    /// 朗读排队。
    SpeechQueue,
    /// 工具开销。
    ToolOverhead,
}

pub const BENCH_SLOTS: [BenchSlot; 5] = [
    BenchSlot::RenderDelta, BenchSlot::ColorSwitch, BenchSlot::SnapshotGen,
    BenchSlot::SpeechQueue, BenchSlot::ToolOverhead,
];

impl BenchSlot {
    pub fn name(self) -> &'static str {
        match self {
            BenchSlot::RenderDelta => "渲染增量",
            BenchSlot::ColorSwitch => "色标切换",
            BenchSlot::SnapshotGen => "快照生成",
            BenchSlot::SpeechQueue => "朗读排队",
            BenchSlot::ToolOverhead => "工具开销",
        }
    }

    pub fn slot(self) -> usize {
        match self {
            BenchSlot::RenderDelta => 0,
            BenchSlot::ColorSwitch => 1,
            BenchSlot::SnapshotGen => 2,
            BenchSlot::SpeechQueue => 3,
            BenchSlot::ToolOverhead => 4,
        }
    }
}

/// 单档基准：实测值 + 阈值 + 降级声明。
#[derive(Debug, Clone)]
pub struct BenchRow {
    /// 实测（微秒或计数）。
    pub measured: u64,
    /// 阈值上限。
    pub threshold: u64,
    /// 降级声明（越界时须给出，空 = 无声明）。
    pub downgrade_note: String,
}

/// 总基准册。
#[derive(Debug, Clone)]
pub struct BenchMaster {
    pub rows: [Option<BenchRow>; 5],
    /// 家族对拍值（同档）。
    pub peer: [u64; 5],
    /// 仲裁记录（对拍分歧仲裁后采用值）。
    pub arbitrated: [u64; 5],
}

impl BenchMaster {
    pub fn new() -> Self {
        Self {
            rows: [None, None, None, None, None],
            peer: [0; 5],
            arbitrated: [0; 5],
        }
    }

    /// 录入一档。
    pub fn put(&mut self, slot: BenchSlot, measured: u64, threshold: u64) {
        self.rows[slot.slot()] = Some(BenchRow {
            measured,
            threshold,
            downgrade_note: String::new(),
        });
    }

    /// 阈值复核：实测 ≤ 阈值 → 过；越界但带降级声明 → 降级通过；
    /// 越界无声明 → THRESHOLD_OVER（重议）。
    pub fn review(&mut self, slot: BenchSlot) -> T4PCode {
        match self.rows[slot.slot()].as_mut() {
            Some(r) => {
                if r.measured <= r.threshold {
                    T4PCode::OK
                } else if !r.downgrade_note.is_empty() {
                    T4PCode::OK
                } else {
                    T4PCode::THRESHOLD_OVER
                }
            }
            None => T4PCode::BASELINE_BLOCK,
        }
    }

    /// 五档全复核。
    pub fn review_all(&mut self) -> T4PCode {
        let mut worst = T4PCode::OK;
        for s in BENCH_SLOTS.iter() {
            let c = self.review(*s);
            if c != T4PCode::OK {
                worst = c;
            }
        }
        worst
    }

    /// 五档齐备。
    pub fn complete(&self) -> bool {
        self.rows.iter().all(|r| r.is_some())
    }

    /// 家族对拍：分歧检出（实测与对拍值偏差 > 20% 即分歧）。
    pub fn peer_compare(&mut self, slot: BenchSlot, peer_val: u64) -> T4PCode {
        self.peer[slot.slot()] = peer_val;
        if let Some(r) = self.rows[slot.slot()].as_ref() {
            let a = r.measured.min(peer_val) as u128;
            let b = r.measured.max(peer_val) as u128;
            if a == 0 || b * 100 > a * 120 {
                return T4PCode::PEER_SPLIT;
            }
        }
        T4PCode::OK
    }

    /// 仲裁：登记仲裁后采用值。
    pub fn arbitrate(&mut self, slot: BenchSlot, value: u64) {
        self.arbitrated[slot.slot()] = value;
    }
}

// ── 冻结前置核（F3990 闸）────────────────────────────────────────────

/// 冻结前置核：五档齐备 + 全复核过 + 对拍无未仲裁分歧。
pub struct FreezePrecheck;

impl FreezePrecheck {
    pub fn gate(bm: &BenchMaster) -> T4PCode {
        if !bm.complete() {
            return T4PCode::BASELINE_BLOCK;
        }
        let mut bm2 = BenchMaster::new();
        // 只读复核（不写降级，先拷贝）。
        for (i, r) in bm.rows.iter().enumerate() {
            if let Some(row) = r {
                bm2.rows[i] = Some(BenchRow {
                    measured: row.measured,
                    threshold: row.threshold,
                    downgrade_note: row.downgrade_note.clone(),
                });
            }
        }
        let code = bm2.review_all();
        if code != T4PCode::OK {
            return code;
        }
        // 对拍分歧且未仲裁 → 阻断。
        for s in BENCH_SLOTS.iter() {
            let i = s.slot();
            if bm.peer[i] > 0 && bm.arbitrated[i] == 0 {
                if let Some(r) = bm.rows[i].as_ref() {
                    let a = r.measured.min(bm.peer[i]) as u128;
                    let b = r.measured.max(bm.peer[i]) as u128;
                    if a == 0 || b * 100 > a * 120 {
                        return T4PCode::PEER_SPLIT;
                    }
                }
            }
        }
        T4PCode::OK
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves238_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves238_s10perf");

    // C3984-01 诊断码段不越窗。
    chk(&mut set, "C3984-01",
        T4PCode::BASELINE_BLOCK.0 < SEG_BASE + 16 && SEG_BASE == 0x3EC0, "err");

    // C3984-02 五档齐备含专项档。
    chk(&mut set, "C3984-02",
        BENCH_SLOTS.len() == 5
            && BENCH_SLOTS[0].name() == "渲染增量"
            && BENCH_SLOTS[3].name() == "朗读排队", "err");

    // C3984-03 录入 + 阈值内复核过。
    let mut bm = BenchMaster::new();
    bm.put(BenchSlot::RenderDelta, 800, 1000);
    chk(&mut set, "C3984-03", bm.review(BenchSlot::RenderDelta) == T4PCode::OK, "err");

    // C3984-04 越界无声明 → THRESHOLD_OVER（重议）。
    bm.put(BenchSlot::ColorSwitch, 1500, 1000);
    chk(&mut set, "C3984-04", bm.review(BenchSlot::ColorSwitch) == T4PCode::THRESHOLD_OVER, "err");

    // C3984-05 越界带降级声明 → 降级通过。
    if let Some(r) = bm.rows[BenchSlot::ColorSwitch.slot()].as_mut() {
        r.downgrade_note = String::from("降级声明：色标切换走简化路径");
    }
    chk(&mut set, "C3984-05", bm.review(BenchSlot::ColorSwitch) == T4PCode::OK, "err");

    // C3984-06 缺档复核 → BASELINE_BLOCK。
    chk(&mut set, "C3984-06", bm.review(BenchSlot::SnapshotGen) == T4PCode::BASELINE_BLOCK, "err");

    // C3984-07 五档齐 + 全过 → review_all OK。
    bm.put(BenchSlot::SnapshotGen, 300, 400);
    bm.put(BenchSlot::SpeechQueue, 90, 100);
    bm.put(BenchSlot::ToolOverhead, 45, 50);
    chk(&mut set, "C3984-07", bm.complete() && bm.review_all() == T4PCode::OK, "err");

    // C3984-08 家族对拍：偏差 20% 内 → 无分歧。
    let mut bm2 = BenchMaster::new();
    bm2.put(BenchSlot::RenderDelta, 900, 1000);
    chk(&mut set, "C3984-08", bm2.peer_compare(BenchSlot::RenderDelta, 850) == T4PCode::OK, "err");

    // C3984-09 家族对拍：偏差超 20% → 分歧。
    chk(&mut set, "C3984-09", bm2.peer_compare(BenchSlot::RenderDelta, 200) == T4PCode::PEER_SPLIT, "err");

    // C3984-10 仲裁登记后冻结闸放行。
    bm2.arbitrate(BenchSlot::RenderDelta, 550);
    chk(&mut set, "C3984-10", FreezePrecheck::gate(&bm2) == T4PCode::OK, "err");

    // C3984-11 分歧未仲裁 → 冻结闸阻断。
    let mut bm3 = BenchMaster::new();
    bm3.put(BenchSlot::RenderDelta, 900, 1000);
    let _ = bm3.peer_compare(BenchSlot::RenderDelta, 200);
    chk(&mut set, "C3984-11", FreezePrecheck::gate(&bm3) == T4PCode::PEER_SPLIT, "err");

    // C3984-12 缺档 → 冻结闸阻断（基线未冻）。
    let bm4 = BenchMaster::new();
    chk(&mut set, "C3984-12", FreezePrecheck::gate(&bm4) == T4PCode::BASELINE_BLOCK, "err");

    // C3984-13 档位槽双射。
    let slots: Vec<usize> = BENCH_SLOTS.iter().map(|s| s.slot()).collect();
    let mut uniq = true;
    for i in 0..5 {
        for j in (i + 1)..5 {
            if slots[i] == slots[j] { uniq = false; }
        }
    }
    chk(&mut set, "C3984-13", uniq, "err");

    // C3984-14 无障碍专项档独立阈值（朗读排队档）。
    let mut bm5 = BenchMaster::new();
    bm5.put(BenchSlot::SpeechQueue, 120, 100);
    chk(&mut set, "C3984-14",
        bm5.review(BenchSlot::SpeechQueue) == T4PCode::THRESHOLD_OVER, "err");

    // C3984-15 对拍零值防除零（实测 0 vs 对拍值）。
    let mut bm6 = BenchMaster::new();
    bm6.put(BenchSlot::ToolOverhead, 0, 10);
    chk(&mut set, "C3984-15", bm6.peer_compare(BenchSlot::ToolOverhead, 50) == T4PCode::PEER_SPLIT, "err");

    // C3984-16 复核只读性：review 不改实测。
    let before = bm.rows[0].as_ref().map(|r| r.measured).unwrap_or(0);
    let _ = bm.review(BenchSlot::RenderDelta);
    chk(&mut set, "C3984-16", bm.rows[0].as_ref().map(|r| r.measured).unwrap_or(0) == before, "err");

    // C3984-17 阈值复核表逐档语义。
    chk(&mut set, "C3984-17",
        T4PCode::THRESHOLD_OVER.explain().contains("重议")
            && T4PCode::PEER_SPLIT.explain().contains("仲裁"), "err");

    // C3984-18 仲裁值覆盖后闸再核。
    bm3.arbitrate(BenchSlot::RenderDelta, 900);
    chk(&mut set, "C3984-18", FreezePrecheck::gate(&bm3) == T4PCode::OK, "err");

    // C3984-19 错误码互异。
    let codes = [T4PCode::OK.0, T4PCode::THRESHOLD_OVER.0, T4PCode::PEER_SPLIT.0,
        T4PCode::BASELINE_BLOCK.0];
    chk(&mut set, "C3984-19",
        codes[0] != codes[1] && codes[1] != codes[2] && codes[2] != codes[3], "err");

    // C3984-20 五档齐备判定与 rows 同步。
    let mut bm7 = BenchMaster::new();
    for s in BENCH_SLOTS.iter() {
        bm7.put(*s, 1, 2);
    }
    chk(&mut set, "C3984-20", bm7.complete(), "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn over_threshold_needs_downgrade() {
        let mut bm = BenchMaster::new();
        bm.put(BenchSlot::RenderDelta, 2000, 1000);
        assert_eq!(bm.review(BenchSlot::RenderDelta), T4PCode::THRESHOLD_OVER);
    }

    #[test]
    fn freeze_gate_blocks_split() {
        let mut bm = BenchMaster::new();
        bm.put(BenchSlot::RenderDelta, 900, 1000);
        let _ = bm.peer_compare(BenchSlot::RenderDelta, 100);
        assert_eq!(FreezePrecheck::gate(&bm), T4PCode::PEER_SPLIT);
    }

    #[test]
    fn full_pass_gate_ok() {
        let mut bm = BenchMaster::new();
        for s in BENCH_SLOTS.iter() {
            bm.put(*s, 10, 20);
        }
        assert_eq!(FreezePrecheck::gate(&bm), T4PCode::OK);
    }
}
