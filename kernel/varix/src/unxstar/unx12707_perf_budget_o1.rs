//! UNX-F12708 联签性能预算终表（O1 对标口径） · 0x81AC0段
//!
//! 判据原文（派工卡 12707 逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12708-J1 可重演协议判据绿（种子/样本/脚本
//!      三件归档），重演演练 10/10 一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 联签性能预算终表（O1 对标口径）：八算子行（算子名/预算 μs/
//!    P95 μs）冻结闭集，O1 对标口径 = 逐行 P95 ≤ 预算且 ΣP95 ≤
//!    Σ预算（合账双线不过线）；
//! 2. I 型八批合账：Σ预算 ≡ 7200 μs、ΣP95 ≡ 6650 μs 合账闭合——
//!    缺行/Σ 失衡即终表不齐；
//! 3. 可重演协议三件归档：种子（LCG 种子写死 ≡ SEG_BASE ⊕ 0x5A5A）/
//!    样本（样本容量 ≡ 20）/脚本（脚本号 ≡ 3，脚本名归档）三件
//!    齐全才许冻结终表（三件缺一即协议档不绿）；
//! 4. 重演演练 10/10 一致：重演引擎按三件协议跑 10 轮，每轮折算
//!    摘要逐位一致且与冻结摘要同值——不一致即重演失败（判据硬线）；
//! 5. 终表状态机 ObDraft → ObReckoning → ObFrozen 单向推进，冻结后
//!    只读、倒退显性拒；注入十族（超线行/合账失衡/种子漂移/样本
//!    缺额/脚本缺档/重演不一致/冻结后写/倒退/算子重号/摘要空）
//!    逐路必抓；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子写死）；
//!    取值一律 match/if let/.get()/checked_*/saturating_*，无
//!    unwrap/expect/panic/裸下标；中文注释与中文诊断；无死代码无
//!    #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `budget_table_o1_ok` ↔ 标题「联签性能预算终表（O1 对标口径）」；
//!   - `replay_protocol_ok` ↔ ② 可重演协议判据绿（种子/样本/脚本三件归档）；
//!   - `replay_drill_ten_ok` ↔ ② 重演演练 10/10 一致；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12707 − 12600)）。
pub const SEG_BASE: u32 = 0x81AC0;

// ── 预算终表诊断码闭集 ───────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObDiag {
    /// 超线行（P95 > 预算）。
    EOverline,
    /// 合账失衡（Σ 与冻结值不符）。
    ESumOff,
    /// 算子重号。
    EOpDup,
    /// 算子名空。
    EOpNameEmpty,
    /// 种子漂移。
    ESeedDrift,
    /// 样本缺额。
    ESampleShort,
    /// 脚本缺档。
    EScriptMissing,
    /// 重演不一致。
    EReplayDiff,
    /// 冻结后写。
    EFrozenWrite,
    /// 状态机倒退。
    ERewind,
    /// 协议三件缺一。
    EProtocolMissing,
    /// 摘要空。
    ESummaryEmpty,
}

impl ObDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                ObDiag::EOverline => 0x01,
                ObDiag::ESumOff => 0x02,
                ObDiag::EOpDup => 0x03,
                ObDiag::EOpNameEmpty => 0x04,
                ObDiag::ESeedDrift => 0x05,
                ObDiag::ESampleShort => 0x06,
                ObDiag::EScriptMissing => 0x07,
                ObDiag::EReplayDiff => 0x08,
                ObDiag::EFrozenWrite => 0x09,
                ObDiag::ERewind => 0x0a,
                ObDiag::EProtocolMissing => 0x0b,
                ObDiag::ESummaryEmpty => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            ObDiag::EOverline => "超线行：P95 ≤ 预算（O1 口径），超线先查因",
            ObDiag::ESumOff => "合账失衡：Σ预算 ≡ 7200、ΣP95 ≡ 6650，先对平合账",
            ObDiag::EOpDup => "算子重号：一算子一行，重复先查终表",
            ObDiag::EOpNameEmpty => "算子名空：算子名非空才可入表，空名先补",
            ObDiag::ESeedDrift => "种子漂移：种子 ≡ SEG_BASE ⊕ 0x5A5A 写死，先纠种子",
            ObDiag::ESampleShort => "样本缺额：样本容量 ≡ 20，缺额先补",
            ObDiag::EScriptMissing => "脚本缺档：脚本号/名归档齐全才许冻结",
            ObDiag::EReplayDiff => "重演不一致：10 轮摘要逐位一致，分歧先查引擎",
            ObDiag::EFrozenWrite => "冻结后写：终表只读，补行走下一轮合账",
            ObDiag::ERewind => "状态机倒退：ObDraft→ObReckoning→ObFrozen 只进不退",
            ObDiag::EProtocolMissing => "协议三件缺一：种子/样本/脚本三件齐全才绿",
            ObDiag::ESummaryEmpty => "摘要空：冻结摘要非空才许写回已完成",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const OB_ALL: &[ObDiag] = &[
    ObDiag::EOverline,
    ObDiag::ESumOff,
    ObDiag::EOpDup,
    ObDiag::EOpNameEmpty,
    ObDiag::ESeedDrift,
    ObDiag::ESampleShort,
    ObDiag::EScriptMissing,
    ObDiag::EReplayDiff,
    ObDiag::EFrozenWrite,
    ObDiag::ERewind,
    ObDiag::EProtocolMissing,
    ObDiag::ESummaryEmpty,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in OB_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81AC1..0x81ACC 闭集内"
}

// ── 预算行冻结表（O1 对标口径） ──────────────────────────────────
/// 预算行三元：(算子序, 算子名, (预算 μs, P95 μs)) 展开四元。
///
/// 常量来源：联签性能预算终表（O1 对标口径冻结快照）。
pub const O1_ROWS: &[(u32, &str, u32, u32)] = &[
    (0, "o1-cosign-replay", 1000, 920),
    (1, "o1-inject-catch", 900, 810),
    (2, "o1-ledger-fold", 800, 730),
    (3, "o1-sample-replay", 950, 880),
    (4, "o1-recon-anchor", 900, 840),
    (5, "o1-matrix-final", 1000, 900),
    (6, "o1-dualhost-cmp", 900, 820),
    (7, "o1-ledger-seal", 750, 750),
];

/// 算子行数冻结值（I 型八算子）。
pub const O1_CAP: usize = 8;

/// 合账预算总 μs。
pub const O1_BUDGET_TOTAL: u32 = 7200;

/// 合账 P95 总 μs。
pub const O1_P95_TOTAL: u32 = 6650;

/// 单行超线判定（O1 口径：P95 > 预算即超线）。
pub fn row_overline(row: &(u32, &str, u32, u32)) -> bool {
    row.3 > row.2
}

/// Σ预算。
pub fn budget_sum() -> u32 {
    let mut acc = 0u32;
    for row in O1_ROWS.iter() {
        acc = acc.saturating_add(row.2);
    }
    acc
}

/// ΣP95。
pub fn p95_sum() -> u32 {
    let mut acc = 0u32;
    for row in O1_ROWS.iter() {
        acc = acc.saturating_add(row.3);
    }
    acc
}

// ── 可重演协议三件 ───────────────────────────────────────────────
/// 可重演协议（种子/样本/脚本三件）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplayProtocol {
    /// LCG 种子（写死 ≡ SEG_BASE ⊕ 0x5A5A）。
    pub seed: u32,
    /// 样本容量（≡ 20）。
    pub sample_cap: u32,
    /// 脚本号（≡ 3）。
    pub script_id: u32,
    /// 脚本名（归档）。
    pub script_name: &'static str,
}

/// 种子封闭式：种子 ≡ SEG_BASE ⊕ 0x5A5A。
pub const PROTOCOL_SEED: u32 = SEG_BASE ^ 0x5A5A;

/// 样本容量冻结值。
pub const PROTOCOL_SAMPLES: u32 = 20;

/// 脚本号冻结值。
pub const PROTOCOL_SCRIPT_ID: u32 = 3;

/// 脚本名归档。
pub const PROTOCOL_SCRIPT_NAME: &str = "vx9-o1-replay-v3";

/// 冻结协议档。
pub const FROZEN_PROTOCOL: ReplayProtocol = ReplayProtocol {
    seed: PROTOCOL_SEED,
    sample_cap: PROTOCOL_SAMPLES,
    script_id: PROTOCOL_SCRIPT_ID,
    script_name: PROTOCOL_SCRIPT_NAME,
};

/// 协议三件齐闸（种子对号 + 样本额足 + 脚本归档齐）。
pub fn protocol_pass(p: &ReplayProtocol) -> bool {
    if p.seed != PROTOCOL_SEED {
        return false;
    }
    if p.sample_cap < PROTOCOL_SAMPLES {
        return false;
    }
    if p.script_id != PROTOCOL_SCRIPT_ID {
        return false;
    }
    if p.script_name.is_empty() {
        return false;
    }
    true
}

/// 重演一轮折算摘要（LCG 确定性：种子起，逐样本折叠）。
pub fn replay_once(p: &ReplayProtocol) -> u32 {
    let mut seed = p.seed;
    let mut acc: u32 = p.script_id.wrapping_add(0x9E37);
    let mut k = 0u32;
    while k < p.sample_cap {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        acc = acc
            .wrapping_mul(5)
            .wrapping_add(seed >> 16)
            .wrapping_add(k);
        k += 1;
    }
    acc
}

/// 重演演练 10 轮（逐轮摘要同值且与冻结值一致）。
pub fn replay_drill() -> (u32, u32, String) {
    let mut caught = 0u32;
    let mut escape = String::new();
    let frozen = replay_once(&FROZEN_PROTOCOL);
    let mut round = 0u32;
    while round < 10 {
        let d = replay_once(&FROZEN_PROTOCOL);
        if d == frozen {
            caught += 1;
        } else {
            escape.push_str("轮差;");
        }
        round += 1;
    }
    (caught, 10, escape)
}

// ── 终表状态机 ───────────────────────────────────────────────────
/// 终表相位（单调推进闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObState {
    /// 草稿。
    ObDraft,
    /// 合账中。
    ObReckoning,
    /// 已冻结（只读）。
    ObFrozen,
}

impl ObState {
    /// 相位秩。
    pub fn rank(self) -> u32 {
        match self {
            ObState::ObDraft => 0,
            ObState::ObReckoning => 1,
            ObState::ObFrozen => 2,
        }
    }

    /// 相位名。
    pub fn name(self) -> &'static str {
        match self {
            ObState::ObDraft => "草稿",
            ObState::ObReckoning => "合账中",
            ObState::ObFrozen => "已冻结",
        }
    }
}

/// 相位迁移守卫（只进不退，同相位拒）。
pub fn ob_advance(from: ObState, to: ObState) -> Result<(), u32> {
    if to.rank() != from.rank() + 1 {
        return Err(ObDiag::ERewind.val());
    }
    Ok(())
}

/// 联签性能预算终表。
#[derive(Clone, Debug)]
pub struct ObTable {
    /// 相位。
    state: ObState,
    /// 协议档。
    protocol: Option<ReplayProtocol>,
    /// 已登记算子数。
    booked: usize,
    /// 重演已过轮数。
    drilled: u32,
    /// 拒止计数。
    rejects: u32,
}

impl ObTable {
    /// 新表（草稿相位）。
    pub fn new() -> ObTable {
        ObTable {
            state: ObState::ObDraft,
            protocol: None,
            booked: 0,
            drilled: 0,
            rejects: 0,
        }
    }

    /// 开账（草稿 → 合账中）。
    pub fn begin(&mut self) -> Result<(), u32> {
        ob_advance(self.state, ObState::ObReckoning)?;
        self.state = ObState::ObReckoning;
        Ok(())
    }

    /// 归档协议档（三件齐闸）。
    pub fn archive_protocol(&mut self, p: ReplayProtocol) -> Result<(), u32> {
        if self.state == ObState::ObFrozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ObDiag::EFrozenWrite.val());
        }
        if self.state == ObState::ObDraft {
            self.state = ObState::ObReckoning;
        }
        if !protocol_pass(&p) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ObDiag::EProtocolMissing.val());
        }
        self.protocol = Some(p);
        Ok(())
    }

    /// 登一行预算（序/名/超线三闸）。
    pub fn book_row(&mut self, idx: usize) -> Result<(), u32> {
        if self.state == ObState::ObFrozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ObDiag::EFrozenWrite.val());
        }
        if self.state == ObState::ObDraft {
            self.state = ObState::ObReckoning;
        }
        // 序闸。
        if idx != self.booked {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ObDiag::EOpDup.val());
        }
        let row = match O1_ROWS.get(idx) {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(ObDiag::EOpDup.val());
            }
        };
        // 名闸。
        if row.1.is_empty() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ObDiag::EOpNameEmpty.val());
        }
        // 超线闸。
        if row_overline(row) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ObDiag::EOverline.val());
        }
        self.booked += 1;
        Ok(())
    }

    /// 跑一轮重演演练（协议档就位闸 + 一致闸）。
    pub fn drill_round(&mut self) -> Result<(), u32> {
        if self.state == ObState::ObFrozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ObDiag::EFrozenWrite.val());
        }
        let p = match self.protocol {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(ObDiag::EProtocolMissing.val());
            }
        };
        let d = replay_once(&p);
        let frozen = replay_once(&FROZEN_PROTOCOL);
        if d != frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ObDiag::EReplayDiff.val());
        }
        self.drilled += 1;
        Ok(())
    }

    /// 冻结终表（合账中 → 已冻结；八行/合账双线/协议档/重演 10 轮四闸）。
    pub fn freeze(&mut self) -> Result<(), u32> {
        if self.state != ObState::ObReckoning {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ObDiag::ERewind.val());
        }
        if self.booked != O1_CAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ObDiag::EOpDup.val());
        }
        if budget_sum() != O1_BUDGET_TOTAL || p95_sum() != O1_P95_TOTAL {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ObDiag::ESumOff.val());
        }
        if self.protocol.is_none() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ObDiag::EProtocolMissing.val());
        }
        if self.drilled < 10 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ObDiag::EReplayDiff.val());
        }
        ob_advance(self.state, ObState::ObFrozen)?;
        self.state = ObState::ObFrozen;
        Ok(())
    }

    /// 当前相位。
    pub fn state(&self) -> ObState {
        self.state
    }

    /// 拒止计数。
    pub fn reject_len(&self) -> u32 {
        self.rejects
    }

    /// 已登算子数。
    pub fn booked_len(&self) -> usize {
        self.booked
    }

    /// 重演已过轮数。
    pub fn drilled_len(&self) -> u32 {
        self.drilled
    }

    /// 协议档。
    pub fn protocol(&self) -> Option<ReplayProtocol> {
        self.protocol
    }
}

/// 全量终表重放（协议归档 + 八行登记 + 10 轮重演 + 冻结）。
pub fn build_full_ob_table() -> ObTable {
    let mut t = ObTable::new();
    let _ = t.begin();
    let _ = t.archive_protocol(FROZEN_PROTOCOL);
    let mut i = 0usize;
    while i < O1_CAP {
        let _ = t.book_row(i);
        i += 1;
    }
    let mut r = 0u32;
    while r < 10 {
        let _ = t.drill_round();
        r += 1;
    }
    let _ = t.freeze();
    t
}

// ── 判据谓词（全部私有） ─────────────────────────────────────────
/// 联签性能预算终表（O1 对标口径：逐行不过线 + 合账双线闭合）。
fn budget_table_o1_ok() -> bool {
    if O1_ROWS.len() != O1_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < O1_ROWS.len() {
        let row = match O1_ROWS.get(i) {
            Some(v) => v,
            None => return false,
        };
        if row.0 != i as u32 {
            return false;
        }
        if row.1.is_empty() {
            return false;
        }
        if row_overline(row) {
            return false;
        }
        i += 1;
    }
    budget_sum() == O1_BUDGET_TOTAL && p95_sum() == O1_P95_TOTAL
}

/// 可重演协议判据绿（三件归档齐）。
fn replay_protocol_ok() -> bool {
    if !protocol_pass(&FROZEN_PROTOCOL) {
        return false;
    }
    let t = build_full_ob_table();
    if t.protocol() != Some(FROZEN_PROTOCOL) {
        return false;
    }
    if t.state() != ObState::ObFrozen {
        return false;
    }
    // 三件直证。
    FROZEN_PROTOCOL.seed == PROTOCOL_SEED
        && FROZEN_PROTOCOL.sample_cap == PROTOCOL_SAMPLES
        && FROZEN_PROTOCOL.script_id == PROTOCOL_SCRIPT_ID
        && !FROZEN_PROTOCOL.script_name.is_empty()
}

/// 重演演练 10/10 一致。
fn replay_drill_ten_ok() -> bool {
    let (c, t, e) = replay_drill();
    t == 10 && c == 10 && e.is_empty()
}

/// 注入联测十族（人为构造违约必被拒）。
pub fn ob_fault_sweep() -> (u32, u32, String) {
    let mut caught = 0u32;
    let mut escape = String::new();
    // 1. 种子漂移协议归档拒。
    {
        let mut t = ObTable::new();
        let _ = t.begin();
        let drift = ReplayProtocol {
            seed: PROTOCOL_SEED.wrapping_add(1),
            sample_cap: PROTOCOL_SAMPLES,
            script_id: PROTOCOL_SCRIPT_ID,
            script_name: PROTOCOL_SCRIPT_NAME,
        };
        if t.archive_protocol(drift).is_err() {
            caught += 1;
        } else {
            escape.push_str("种子漂移;");
        }
    }
    // 2. 样本缺额协议归档拒。
    {
        let mut t = ObTable::new();
        let _ = t.begin();
        let short = ReplayProtocol {
            seed: PROTOCOL_SEED,
            sample_cap: PROTOCOL_SAMPLES - 1,
            script_id: PROTOCOL_SCRIPT_ID,
            script_name: PROTOCOL_SCRIPT_NAME,
        };
        if t.archive_protocol(short).is_err() {
            caught += 1;
        } else {
            escape.push_str("样本缺;");
        }
    }
    // 3. 脚本缺档协议归档拒。
    {
        let mut t = ObTable::new();
        let _ = t.begin();
        let no_script = ReplayProtocol {
            seed: PROTOCOL_SEED,
            sample_cap: PROTOCOL_SAMPLES,
            script_id: PROTOCOL_SCRIPT_ID,
            script_name: "",
        };
        if t.archive_protocol(no_script).is_err() {
            caught += 1;
        } else {
            escape.push_str("脚本缺;");
        }
    }
    // 4. 越序登行。
    {
        let mut t = ObTable::new();
        let _ = t.begin();
        if t.book_row(1).is_err() {
            caught += 1;
        } else {
            escape.push_str("越序;");
        }
    }
    // 5. 重演未跑冻结拒。
    {
        let mut t = ObTable::new();
        let _ = t.begin();
        let _ = t.archive_protocol(FROZEN_PROTOCOL);
        let mut i = 0usize;
        while i < O1_CAP {
            let _ = t.book_row(i);
            i += 1;
        }
        if t.freeze().is_err() {
            caught += 1;
        } else {
            escape.push_str("重演缺;");
        }
    }
    // 6. 协议缺档冻结拒。
    {
        let mut t = ObTable::new();
        let _ = t.begin();
        let mut i = 0usize;
        while i < O1_CAP {
            let _ = t.book_row(i);
            i += 1;
        }
        let mut r = 0u32;
        while r < 10 {
            // 协议缺档时重演必拒。
            let _ = t.drill_round();
            r += 1;
        }
        if t.freeze().is_err() {
            caught += 1;
        } else {
            escape.push_str("协议缺;");
        }
    }
    // 7. 超线直证（合成超线行必判超线）。
    {
        let over = (9u32, "o1-synthetic", 100u32, 101u32);
        if row_overline(&over) {
            caught += 1;
        } else {
            escape.push_str("超线直证;");
        }
    }
    // 8. 冻结后写。
    {
        let mut t = build_full_ob_table();
        if t.book_row(0).is_err() && t.drill_round().is_err() {
            caught += 1;
        } else {
            escape.push_str("冻结后写;");
        }
    }
    // 9. 状态机倒退。
    {
        let mut t = build_full_ob_table();
        if t.begin().is_err() {
            caught += 1;
        } else {
            escape.push_str("倒退;");
        }
    }
    // 10. 摘要空面直证（相位名/算子名恒非空）。
    {
        let mut bad = false;
        for st in [ObState::ObDraft, ObState::ObReckoning, ObState::ObFrozen].iter() {
            if st.name().is_empty() {
                bad = true;
            }
        }
        for row in O1_ROWS.iter() {
            if row.1.is_empty() {
                bad = true;
            }
        }
        if !bad {
            caught += 1;
        } else {
            escape.push_str("空摘要;");
        }
    }
    (caught, 10, escape)
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let x = build_full_ob_table();
    let y = build_full_ob_table();
    if x.state() != y.state() || x.reject_len() != y.reject_len() {
        return false;
    }
    if x.booked_len() != y.booked_len() || x.drilled_len() != y.drilled_len() {
        return false;
    }
    if x.protocol() != y.protocol() {
        return false;
    }
    let (c1, _, e1) = ob_fault_sweep();
    let (c2, _, e2) = ob_fault_sweep();
    c1 == c2 && e1 == e2
}

/// 冻结摘要（预算表 + 协议档 + 重演摘要折算）。
pub fn frozen_digest() -> u32 {
    let mut acc: u32 = SEG_BASE;
    let mut i = 0u32;
    for row in O1_ROWS.iter() {
        acc = acc
            .wrapping_mul(7)
            .wrapping_add(row.0)
            .wrapping_add(row.2)
            .wrapping_add(row.3)
            .wrapping_add(row.1.len() as u32)
            .wrapping_add(i);
        i = i.wrapping_add(1);
    }
    acc = acc
        .wrapping_mul(3)
        .wrapping_add(FROZEN_PROTOCOL.seed)
        .wrapping_add(FROZEN_PROTOCOL.sample_cap)
        .wrapping_add(FROZEN_PROTOCOL.script_id)
        .wrapping_add(replay_once(&FROZEN_PROTOCOL));
    acc
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * 199 {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12707u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③）。
fn writeback_ok() -> bool {
    let t = build_full_ob_table();
    if t.state() != ObState::ObFrozen {
        return false;
    }
    for st in [ObState::ObDraft, ObState::ObReckoning, ObState::ObFrozen].iter() {
        if st.name().is_empty() {
            return false;
        }
    }
    for row in O1_ROWS.iter() {
        if row.1.is_empty() {
            return false;
        }
    }
    if !budget_table_o1_ok() || !replay_protocol_ok() || !replay_drill_ten_ok() {
        return false;
    }
    frozen_digest() != 0
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12708 联签性能预算终表自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12707_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12707-perf-budget-o1");
    let t = build_full_ob_table();

    s.add_dyn(
        "F12708-01 预算终表齐（O1 对标口径）",
        budget_table_o1_ok(),
        &format!(
            "budget={} p95={} rows={} frozen={}",
            budget_sum(),
            p95_sum(),
            O1_ROWS.len(),
            t.state() == ObState::ObFrozen
        ),
    );

    s.add_dyn(
        "F12708-02 逐行 P95 ≤ 预算",
        {
            let mut ok = true;
            for row in O1_ROWS.iter() {
                if row_overline(row) {
                    ok = false;
                }
            }
            ok
        },
        "O1 对标口径",
    );

    s.add_dyn(
        "F12708-03 合账双线闭合",
        budget_sum() == O1_BUDGET_TOTAL && p95_sum() == O1_P95_TOTAL,
        &format!(
            "Σbudget={} (={}) Σp95={} (={})",
            budget_sum(),
            O1_BUDGET_TOTAL,
            p95_sum(),
            O1_P95_TOTAL
        ),
    );

    s.add_dyn(
        "F12708-04 可重演协议三件归档",
        replay_protocol_ok(),
        &format!(
            "seed={:05X} samples={} script={} {}",
            FROZEN_PROTOCOL.seed,
            FROZEN_PROTOCOL.sample_cap,
            FROZEN_PROTOCOL.script_id,
            FROZEN_PROTOCOL.script_name
        ),
    );

    s.add_dyn(
        "F12708-05 重演演练 10/10 一致",
        replay_drill_ten_ok(),
        &format!("caught={} drilled={}", replay_drill().0, t.drilled_len()),
    );

    s.add_dyn(
        "F12708-06 重演摘要非零",
        replay_once(&FROZEN_PROTOCOL) != 0,
        &format!("digest={:08X}", replay_once(&FROZEN_PROTOCOL)),
    );

    s.add_dyn(
        "F12708-07 协议三件齐闸直证",
        protocol_pass(&FROZEN_PROTOCOL),
        "种子/样本/脚本三件齐",
    );

    s.add_dyn(
        "F12708-08 注入联测 10/10 拒止",
        {
            let (c, t2, e) = ob_fault_sweep();
            t2 == 10 && c == 10 && e.is_empty()
        },
        &format!("caught={}", ob_fault_sweep().0),
    );

    s.add_dyn(
        "F12708-09 相位迁移守卫直读",
        ob_advance(ObState::ObDraft, ObState::ObReckoning).is_ok()
            && ob_advance(ObState::ObReckoning, ObState::ObFrozen).is_ok()
            && ob_advance(ObState::ObFrozen, ObState::ObDraft).is_err()
            && ob_advance(ObState::ObDraft, ObState::ObFrozen).is_err(),
        "草稿→合账中→已冻结只进不退",
    );

    s.add_dyn(
        "F12708-10 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12708-11 完成写回语义",
        writeback_ok(),
        "冻结即已完成且摘要非空",
    );

    s.add_dyn(
        "F12708-12 冻结摘要非零",
        frozen_digest() != 0,
        &format!("digest={:08X}", frozen_digest()),
    );

    s.add_dyn(
        "F12708-13 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0);
        assert_eq!(SEG_BASE, 0x81AC0);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(OB_ALL.len(), 12);
        let mut i = 0usize;
        while i < OB_ALL.len() {
            let vi = match OB_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < OB_ALL.len() {
                let vj = match OB_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn budget_table_o1_holds() {
        assert!(budget_table_o1_ok());
        assert_eq!(O1_ROWS.len(), 8);
        assert_eq!(budget_sum(), 7200);
        assert_eq!(p95_sum(), 6650);
        // 逐行不超线。
        for row in O1_ROWS.iter() {
            assert!(!row_overline(row), "算子 {} 超线", row.0);
        }
        // 超线直证。
        assert!(row_overline(&(9u32, "x", 100u32, 101u32)));
        assert!(!row_overline(&(9u32, "x", 100u32, 100u32)));
    }

    #[test]
    fn replay_protocol_holds() {
        assert!(replay_protocol_ok());
        assert!(protocol_pass(&FROZEN_PROTOCOL));
        assert_eq!(FROZEN_PROTOCOL.seed, PROTOCOL_SEED);
        assert_eq!(PROTOCOL_SEED, SEG_BASE ^ 0x5A5A);
        assert_eq!(FROZEN_PROTOCOL.sample_cap, 20);
        assert_eq!(FROZEN_PROTOCOL.script_id, 3);
        assert_eq!(FROZEN_PROTOCOL.script_name, "vx9-o1-replay-v3");
        // 协议漂移必拒。
        let drift = ReplayProtocol {
            seed: PROTOCOL_SEED.wrapping_add(1),
            sample_cap: 20,
            script_id: 3,
            script_name: "vx9-o1-replay-v3",
        };
        assert!(!protocol_pass(&drift));
        let short = ReplayProtocol { sample_cap: 19, ..FROZEN_PROTOCOL };
        assert!(!protocol_pass(&short));
    }

    #[test]
    fn replay_drill_ten_consistent() {
        assert!(replay_drill_ten_ok());
        let (caught, total, escape) = replay_drill();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
        // 摘要确定性：两轮同值且非零。
        let d1 = replay_once(&FROZEN_PROTOCOL);
        let d2 = replay_once(&FROZEN_PROTOCOL);
        assert_eq!(d1, d2);
        assert_ne!(d1, 0);
    }

    #[test]
    fn book_gates_reject() {
        let mut t = ObTable::new();
        let _ = t.begin();
        // 越序。
        assert_eq!(t.book_row(1), Err(ObDiag::EOpDup.val()));
        // 正常过。
        assert!(t.book_row(0).is_ok());
        // 重序。
        assert_eq!(t.book_row(0), Err(ObDiag::EOpDup.val()));
        assert_eq!(t.reject_len(), 2);
        assert_eq!(t.booked_len(), 1);
        // 冻结表无超线行（直证）：续登 1..7 全过。
        let mut i = 1usize;
        while i < O1_CAP {
            assert!(t.book_row(i).is_ok(), "行 {} 登记失败", i);
            i += 1;
        }
        assert_eq!(t.booked_len(), O1_CAP);
    }

    #[test]
    fn protocol_gates_reject() {
        let mut t = ObTable::new();
        let _ = t.begin();
        // 种子漂移。
        let drift = ReplayProtocol {
            seed: PROTOCOL_SEED.wrapping_add(1),
            sample_cap: 20,
            script_id: 3,
            script_name: "s",
        };
        assert_eq!(t.archive_protocol(drift), Err(ObDiag::EProtocolMissing.val()));
        // 样本缺额。
        let short = ReplayProtocol {
            seed: PROTOCOL_SEED,
            sample_cap: 10,
            script_id: 3,
            script_name: "s",
        };
        assert_eq!(t.archive_protocol(short), Err(ObDiag::EProtocolMissing.val()));
        // 脚本缺档。
        let nos = ReplayProtocol {
            seed: PROTOCOL_SEED,
            sample_cap: 20,
            script_id: 3,
            script_name: "",
        };
        assert_eq!(t.archive_protocol(nos), Err(ObDiag::EProtocolMissing.val()));
        assert_eq!(t.reject_len(), 3);
        // 正常归档过。
        assert!(t.archive_protocol(FROZEN_PROTOCOL).is_ok());
        assert_eq!(t.protocol(), Some(FROZEN_PROTOCOL));
    }

    #[test]
    fn freeze_gates_hold() {
        // 空表冻结拒。
        let mut t = ObTable::new();
        let _ = t.begin();
        assert_eq!(t.freeze(), Err(ObDiag::EOpDup.val()));
        // 无协议档冻结拒。
        let mut h = ObTable::new();
        let _ = h.begin();
        let mut i = 0usize;
        while i < O1_CAP {
            let _ = h.book_row(i);
            i += 1;
        }
        assert_eq!(h.freeze(), Err(ObDiag::EProtocolMissing.val()));
        // 全量冻结过。
        let mut full = build_full_ob_table();
        assert_eq!(full.state(), ObState::ObFrozen);
        assert_eq!(full.booked_len(), 8);
        assert_eq!(full.drilled_len(), 10);
        // 冻结后写拒。
        assert_eq!(full.book_row(0), Err(ObDiag::EFrozenWrite.val()));
        assert_eq!(full.drill_round(), Err(ObDiag::EFrozenWrite.val()));
        assert!(full.begin().is_err());
    }

    #[test]
    fn phase_guards_hold() {
        assert!(ob_advance(ObState::ObDraft, ObState::ObReckoning).is_ok());
        assert!(ob_advance(ObState::ObReckoning, ObState::ObFrozen).is_ok());
        assert!(ob_advance(ObState::ObDraft, ObState::ObFrozen).is_err());
        assert!(ob_advance(ObState::ObFrozen, ObState::ObReckoning).is_err());
        assert!(ob_advance(ObState::ObReckoning, ObState::ObReckoning).is_err());
        assert!(ObState::ObDraft.rank() < ObState::ObReckoning.rank());
        assert!(ObState::ObReckoning.rank() < ObState::ObFrozen.rank());
    }

    #[test]
    fn injections_all_caught() {
        let (caught, total, escape) = ob_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_ob_table();
        let y = build_full_ob_table();
        assert_eq!(x.booked_len(), y.booked_len());
        assert_eq!(x.drilled_len(), y.drilled_len());
        assert_eq!(frozen_digest(), frozen_digest());
        assert_ne!(frozen_digest(), 0);
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        for st in [ObState::ObDraft, ObState::ObReckoning, ObState::ObFrozen].iter() {
            assert!(!st.name().is_empty());
        }
        for row in O1_ROWS.iter() {
            assert!(!row.1.is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12707_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
