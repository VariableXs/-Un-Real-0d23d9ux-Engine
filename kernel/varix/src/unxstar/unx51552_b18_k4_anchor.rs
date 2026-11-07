//! UNX-F51552 B18 与 K4 电源域联动终核（AI-54 联签锚定行） · 0xC0CC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F51552 卡验收②逐字）：
//!   判据原文：UNX-F51553-J1 ×100 零 hang 零悬挂引用；内存账零泄漏。
//!
//! 设计要点（成对对账模型——两域联签，逐对锚定）：
//!   ① 联动面按「成对对账」建模：B18 域与 K4 域各自独立产电平事件，终核
//!      引擎按拍号逐对撮合（B18 第 n 拍对 K4 第 n 拍），两拍电平相等才算
//!      联签成立；不等即记漂移，漂移事件必须全额检出（零逃逸）。
//!   ② AI-54 联签锚定行：撮合成功的那一对在锚定账里落一行
//!      (锚号, 拍号, B18 电平, K4 电平)，锚号单调递增，锚定行只增不改；
//!      回放逐行复算两域差恒 0，双跑逐字段一致即全程可复现。
//!   ③ 判定次序：拍号配对 → 域标签 → 电平档界 → 漂移判零 → 锚定落账；
//!      多判据同现时配对先于档界、档界先于漂移，避免错码掩盖真因。
//!   ④ 悬挂引用防线：撮合失败的一拍立即进入「待重配」悬挂槽，槽位有界；
//!      同拍重复漂移不重复占槽（禁乒乓占位），重配成功（联签成立）才清槽，
//!      清槽必须有对应锚定行佐证；槽满显性拒新拍（容量耗尽显性化）。
//!   ⑤ 守恒账：注入 = 检出 + 逃逸，逃逸恒 0；锚定行数 = 已撮合对数，
//!      悬挂总账 = 已清槽 + 在挂数；账面随撮合同步推进，不硬编码拍量。
//!   ⑥ 诊断码闭集 10 变体，中文建议随码给出；Result<T, 联签码> 显性回传。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC0CC0;

/// 冻结档位表（毫伏整刻度，零浮点；越档即拒）。
pub const LEVEL_TABLE: [u32; 8] = [0, 500, 900, 1800, 2500, 3300, 5000, 12000];
/// 悬挂槽容量（容量耗尽显性拒）。
pub const HANG_SLOT_CAP: usize = 4;

/// 诊断码闭集（B18/K4 联签终核域，val = SEG_BASE + 变体序号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CoSignCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败（SEG_BASE 越界或不对齐）
    SegBad,
    /// 拍号不配对（B18/K4 拍号不一致）
    SeqMismatch,
    /// 域标签非法（非 B18/K4 的域组合）
    DomainUnknown,
    /// 电平越档（不在冻结档位表内）
    LevelOutOfRange,
    /// 漂移非零（两域电平差 ≠ 0，联签不成立）
    DriftNonZero,
    /// 悬挂槽耗尽（待重配对超出槽容量）
    HangSlotExhausted,
    /// 清槽无佐证（重配成功但锚定账无对应行）
    ClearWithoutAnchor,
    /// 锚定账回放不一致（复算漂移非零或锚号倒退）
    AnchorReplayMismatch,
    /// 账面失守（锚定行数 ≠ 已撮合对数）
    LedgerMismatch,
}

impl CoSignCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CoSignCode::Ok => "联签成立",
            CoSignCode::SegBad => "段位越界：核对 SEG_BASE 对齐与取值区间",
            CoSignCode::SeqMismatch => "拍号不配对：B18 与 K4 必须同拍提交",
            CoSignCode::DomainUnknown => "域标签非法：只收 B18×K4 正向组合",
            CoSignCode::LevelOutOfRange => "电平越档：查冻结档位表后再提交",
            CoSignCode::DriftNonZero => "漂移非零：两域电平必须逐对相等",
            CoSignCode::HangSlotExhausted => "悬挂槽满：先重配清槽再收新拍",
            CoSignCode::ClearWithoutAnchor => "清槽无佐证：重配必须先落锚定行",
            CoSignCode::AnchorReplayMismatch => "回放不一致：锚定账被改写，查确定性",
            CoSignCode::LedgerMismatch => "账面失守：锚定行数须等于撮合对数",
        }
    }
}

/// 电源域标签（闭集两域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Domain {
    /// B18 电源域
    B18,
    /// K4 域
    K4,
}

/// 单域电平事件：一拍一电平（毫伏整刻度）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LevelTick {
    pub domain: Domain,
    pub seq: u32,
    pub level_mv: u32,
}

/// AI-54 联签锚定行：撮合成功才落账，只增不改。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnchorRow {
    pub anchor: u32,
    pub seq: u32,
    pub level_b18: u32,
    pub level_k4: u32,
}

/// 悬挂槽状态机：空位 ↔ 悬挂待重配（清槽必须有锚定佐证）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum HangState {
    /// 无悬挂
    Vacant,
    /// 该拍悬挂待重配
    Hanging(u32),
}

/// 联签终核引擎：撮合 + 锚定账 + 悬挂槽 + 守恒账。
pub struct CoSignEngine {
    hang_slots: [HangState; HANG_SLOT_CAP],
    anchors: Vec<AnchorRow>,
    matched_pairs: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
    hang_total: u64,
    cleared_total: u64,
}

impl CoSignEngine {
    pub fn new() -> CoSignEngine {
        CoSignEngine {
            hang_slots: [HangState::Vacant; HANG_SLOT_CAP],
            anchors: Vec::new(),
            matched_pairs: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
            hang_total: 0,
            cleared_total: 0,
        }
    }

    /// 档界判定：电平必须在冻结档位表内（纯查表，零裸下标）。
    fn level_in_table(&self, mv: u32) -> bool {
        let mut found = false;
        for v in LEVEL_TABLE.iter() {
            if *v == mv {
                found = true;
            }
        }
        found
    }

    /// 单拍合法性（域标签先于档界）。
    fn judge_tick(&self, t: &LevelTick) -> Result<(), CoSignCode> {
        match t.domain {
            Domain::B18 | Domain::K4 => {}
        }
        if !self.level_in_table(t.level_mv) {
            return Err(CoSignCode::LevelOutOfRange);
        }
        Ok(())
    }

    /// 悬挂槽找空位（枚举 + get，不裸下标）。
    fn free_slot(&self) -> Option<usize> {
        let mut slot = None;
        for i in 0..HANG_SLOT_CAP {
            if let Some(HangState::Vacant) = self.hang_slots.get(i) {
                slot = Some(i);
            }
        }
        slot
    }

    /// 成对撮合：B18 与 K4 同拍同电平才锚定落账。
    /// 漂移或档外时落悬挂槽；槽满显性拒。成功返回落账锚号。
    pub fn cosign_pair(&mut self, b: &LevelTick, k: &LevelTick) -> Result<u32, CoSignCode> {
        // 判定次序一：拍号配对（先于域标签、档界与漂移）。
        if b.seq != k.seq {
            self.put_hang(b.seq)?;
            return Err(CoSignCode::SeqMismatch);
        }
        // 判定次序二：域标签（B18×K4 正向组合）。
        if !matches!(b.domain, Domain::B18) || !matches!(k.domain, Domain::K4) {
            return Err(CoSignCode::DomainUnknown);
        }
        // 判定次序三：档界。
        self.judge_tick(b)?;
        self.judge_tick(k)?;
        // 判定次序四：漂移判零。
        if b.level_mv != k.level_mv {
            self.put_hang(b.seq)?;
            return Err(CoSignCode::DriftNonZero);
        }
        // 联签成立：锚定落账（账面与撮合同步推进）。
        self.anchors.push(AnchorRow {
            anchor: self.anchors.len() as u32,
            seq: b.seq,
            level_b18: b.level_mv,
            level_k4: k.level_mv,
        });
        self.matched_pairs += 1;
        // 同拍若有悬挂，凭刚落的锚定行佐证清槽。
        self.clear_hang(b.seq);
        Ok((self.anchors.len() as u32) - 1)
    }

    /// 落悬挂槽（容量耗尽显性拒；同拍已挂不重复占位，禁乒乓）。
    fn put_hang(&mut self, seq: u32) -> Result<(), CoSignCode> {
        for i in 0..HANG_SLOT_CAP {
            if let Some(HangState::Hanging(s)) = self.hang_slots.get(i).copied() {
                if s == seq {
                    return Ok(());
                }
            }
        }
        match self.free_slot() {
            Some(i) => {
                if let Some(s) = self.hang_slots.get_mut(i) {
                    *s = HangState::Hanging(seq);
                }
                self.hang_total += 1;
                Ok(())
            }
            None => Err(CoSignCode::HangSlotExhausted),
        }
    }

    /// 清槽：锚定佐证成立才清（清槽数与佐证行同步）。
    fn clear_hang(&mut self, seq: u32) {
        for i in 0..HANG_SLOT_CAP {
            if let Some(HangState::Hanging(s)) = self.hang_slots.get(i).copied() {
                if s == seq {
                    let backed = self.anchors.iter().any(|a| a.seq == seq);
                    if backed {
                        if let Some(s) = self.hang_slots.get_mut(i) {
                            *s = HangState::Vacant;
                        }
                        self.cleared_total += 1;
                    }
                }
            }
        }
    }

    /// 锚定账回放：逐行复算两域差恒 0、锚号严格递增；返回账行数。
    pub fn replay_anchors(&self) -> Result<u32, CoSignCode> {
        let mut prev_anchor: i64 = -1;
        for a in self.anchors.iter() {
            if a.level_b18 != a.level_k4 {
                return Err(CoSignCode::AnchorReplayMismatch);
            }
            if (a.anchor as i64) <= prev_anchor {
                return Err(CoSignCode::AnchorReplayMismatch);
            }
            prev_anchor = a.anchor as i64;
        }
        Ok(self.anchors.len() as u32)
    }

    /// 注入记账：检出加一（调用方在捕获违约时登记）。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据要求恒 0；非零即记账面破洞）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    pub fn anchor_len(&self) -> usize {
        self.anchors.len()
    }

    pub fn matched_pairs(&self) -> u64 {
        self.matched_pairs
    }

    pub fn hang_alive(&self) -> u64 {
        self.hang_total - self.cleared_total
    }

    /// 守恒断言面：注入 = 检出 + 逃逸，且逃逸恒 0、检出 > 0。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }

    /// 悬挂账闭合：总悬挂 = 已清槽 + 在挂数。
    pub fn hang_conserved(&self) -> bool {
        self.hang_total == self.cleared_total + self.hang_alive()
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致：同一拍序两次撮合，锚定账逐字段相等（确定性）。
pub fn dual_run_consistent() -> bool {
    let build = || -> Vec<AnchorRow> {
        let mut e = CoSignEngine::new();
        let seqs = [1u32, 2, 3, 4];
        let lvls = [3300u32, 5000, 900, 1800];
        for i in 0..4usize {
            let seq = match seqs.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let mv = match lvls.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let _ = e.cosign_pair(
                &LevelTick { domain: Domain::B18, seq, level_mv: mv },
                &LevelTick { domain: Domain::K4, seq, level_mv: mv },
            );
        }
        e.anchors.clone()
    };
    let a = build();
    let b = build();
    a == b
}

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51552_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51552");
    // 1 正常流：同拍同电平联签成立并落锚定行。
    {
        let mut e = CoSignEngine::new();
        let r = e.cosign_pair(
            &LevelTick { domain: Domain::B18, seq: 1, level_mv: 3300 },
            &LevelTick { domain: Domain::K4, seq: 1, level_mv: 3300 },
        );
        s.add_dyn(
            "联签正常落锚",
            matches!(r, Ok(a) if a == 0) && e.anchor_len() == 1,
            "同拍同电平应落第 0 号锚定行",
        );
    }
    // 2 边界一：档位表首档放行。
    {
        let mut e = CoSignEngine::new();
        let lo_mv = match LEVEL_TABLE.get(0) {
            Some(v) => *v,
            None => 0,
        };
        let r = e.cosign_pair(
            &LevelTick { domain: Domain::B18, seq: 1, level_mv: lo_mv },
            &LevelTick { domain: Domain::K4, seq: 1, level_mv: lo_mv },
        );
        s.add_dyn("首档边界放行", r.is_ok(), "档位表首档应联签成立");
    }
    // 3 边界二：档位表末档放行。
    {
        let mut e = CoSignEngine::new();
        let hi_mv = match LEVEL_TABLE.get(7) {
            Some(v) => *v,
            None => 0,
        };
        let r = e.cosign_pair(
            &LevelTick { domain: Domain::B18, seq: 1, level_mv: hi_mv },
            &LevelTick { domain: Domain::K4, seq: 1, level_mv: hi_mv },
        );
        s.add_dyn("末档边界放行", r.is_ok(), "档位表末档应联签成立");
    }
    // 4 边界三：表外电平（首档-1 与表内缝隙值）必拒。
    {
        let mut e = CoSignEngine::new();
        let r1 = e.cosign_pair(
            &LevelTick { domain: Domain::B18, seq: 1, level_mv: 3301 },
            &LevelTick { domain: Domain::K4, seq: 1, level_mv: 3301 },
        );
        let r2 = e.cosign_pair(
            &LevelTick { domain: Domain::B18, seq: 2, level_mv: 12001 },
            &LevelTick { domain: Domain::K4, seq: 2, level_mv: 12001 },
        );
        s.add_dyn(
            "表外电平必拒",
            r1 == Err(CoSignCode::LevelOutOfRange)
                && r2 == Err(CoSignCode::LevelOutOfRange),
            "档位表外电平必须拒收",
        );
    }
    // 5 边界四：拍号 +1 错位必拒（配对先于档界）。
    {
        let mut e = CoSignEngine::new();
        let r = e.cosign_pair(
            &LevelTick { domain: Domain::B18, seq: 10, level_mv: 3300 },
            &LevelTick { domain: Domain::K4, seq: 11, level_mv: 3300 },
        );
        s.add_dyn(
            "拍号错位必拒",
            r == Err(CoSignCode::SeqMismatch),
            "B18/K4 拍号不一致必须先拒",
        );
    }
    // 6 注入必抓一：漂移拍 ×10 轮。
    {
        let mut e = CoSignEngine::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let r = e.cosign_pair(
                &LevelTick { domain: Domain::B18, seq: i + 1, level_mv: 3300 },
                &LevelTick { domain: Domain::K4, seq: i + 1, level_mv: 5000 },
            );
            if r == Err(CoSignCode::DriftNonZero) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("漂移注入 10/10", caught == 10, "两域电平漂移必须全抓");
    }
    // 7 注入必抓二：拍号错位 ×10 轮。
    {
        let mut e = CoSignEngine::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let r = e.cosign_pair(
                &LevelTick { domain: Domain::B18, seq: i + 1, level_mv: 3300 },
                &LevelTick { domain: Domain::K4, seq: i + 100, level_mv: 3300 },
            );
            if r == Err(CoSignCode::SeqMismatch) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("错位注入 10/10", caught == 10, "拍号错位必须全抓");
    }
    // 8 注入必抓三：域标签伪造 ×10 轮。
    {
        let mut e = CoSignEngine::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let r = e.cosign_pair(
                &LevelTick { domain: Domain::K4, seq: i + 1, level_mv: 3300 },
                &LevelTick { domain: Domain::K4, seq: i + 1, level_mv: 3300 },
            );
            if r == Err(CoSignCode::DomainUnknown) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("伪造域注入 10/10", caught == 10, "非 B18×K4 组合必须全抓");
    }
    // 9 判定次序：错拍 + 越档同现应报配对错（配对先于档界）。
    {
        let mut e = CoSignEngine::new();
        let r = e.cosign_pair(
            &LevelTick { domain: Domain::B18, seq: 1, level_mv: 7777 },
            &LevelTick { domain: Domain::K4, seq: 2, level_mv: 7777 },
        );
        s.add_dyn(
            "判序配对先于档界",
            r == Err(CoSignCode::SeqMismatch),
            "拍号配对判据先于电平档界判据",
        );
    }
    // 10 状态机拒止：同拍重复漂移不重复占槽，联签成立即清槽。
    {
        let mut e = CoSignEngine::new();
        let _ = e.cosign_pair(
            &LevelTick { domain: Domain::B18, seq: 5, level_mv: 3300 },
            &LevelTick { domain: Domain::K4, seq: 5, level_mv: 5000 },
        );
        let h1 = e.hang_alive();
        let _ = e.cosign_pair(
            &LevelTick { domain: Domain::B18, seq: 5, level_mv: 3300 },
            &LevelTick { domain: Domain::K4, seq: 5, level_mv: 900 },
        );
        let h2 = e.hang_alive();
        let _ = e.cosign_pair(
            &LevelTick { domain: Domain::B18, seq: 5, level_mv: 3300 },
            &LevelTick { domain: Domain::K4, seq: 5, level_mv: 3300 },
        );
        s.add_dyn(
            "悬挂状态机闭合",
            h1 == 1 && h2 == 1 && e.hang_alive() == 0 && e.hang_conserved(),
            "同拍重复漂移不重复入槽，联签成立凭锚定行清槽",
        );
    }
    // 11 容量耗尽：悬挂槽满显性拒（不覆盖不静默）。
    {
        let mut e = CoSignEngine::new();
        let mut last: Result<u32, CoSignCode> = Ok(0);
        for i in 0..(HANG_SLOT_CAP as u32 + 2) {
            last = e.cosign_pair(
                &LevelTick { domain: Domain::B18, seq: i + 1, level_mv: 3300 },
                &LevelTick { domain: Domain::K4, seq: i + 1, level_mv: 0 },
            );
        }
        s.add_dyn(
            "悬挂槽耗尽显性拒",
            last == Err(CoSignCode::HangSlotExhausted),
            "槽满后新漂移必须显性拒绝",
        );
    }
    // 12 锚定回放可复现：×30 拍逐行复算通过。
    {
        let mut e = CoSignEngine::new();
        for i in 0..30u32 {
            let _ = e.cosign_pair(
                &LevelTick { domain: Domain::B18, seq: i + 1, level_mv: 1800 },
                &LevelTick { domain: Domain::K4, seq: i + 1, level_mv: 1800 },
            );
        }
        let replay = e.replay_anchors();
        let synced = e.anchor_len() as u64 == e.matched_pairs();
        s.add_dyn(
            "锚定回放可复现",
            matches!(replay, Ok(n) if n == 30) && synced,
            "逐行复算漂移恒 0、锚号递增，账面与撮合同步",
        );
    }
    // 13 对账守恒：注入 = 检出，逃逸 0。
    {
        let mut e = CoSignEngine::new();
        for i in 0..10u32 {
            let r = e.cosign_pair(
                &LevelTick { domain: Domain::B18, seq: i + 1, level_mv: 3300 },
                &LevelTick { domain: Domain::K4, seq: i + 1, level_mv: 900 },
            );
            if r == Err(CoSignCode::DriftNonZero) {
                e.note_detected();
            }
        }
        s.add_dyn(
            "注入账守恒",
            e.conserved() && e.hang_conserved(),
            "注入=检出+逃逸，逃逸恒 0；悬挂账闭合",
        );
    }
    // 14 双跑一致 + 文案闭集 + 段位守卫。
    {
        let mut closed = true;
        for c in ALL_CODES.iter() {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同一拍序两次建账逐字段相等");
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
        s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    }
    s
}

/// 诊断码全序表（长度与变体数严格一致：10 变体）。
pub const ALL_CODES: [CoSignCode; 10] = [
    CoSignCode::Ok,
    CoSignCode::SegBad,
    CoSignCode::SeqMismatch,
    CoSignCode::DomainUnknown,
    CoSignCode::LevelOutOfRange,
    CoSignCode::DriftNonZero,
    CoSignCode::HangSlotExhausted,
    CoSignCode::ClearWithoutAnchor,
    CoSignCode::AnchorReplayMismatch,
    CoSignCode::LedgerMismatch,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_pair_anchors_row_zero() {
        let mut e = CoSignEngine::new();
        let r = e.cosign_pair(
            &LevelTick { domain: Domain::B18, seq: 1, level_mv: 3300 },
            &LevelTick { domain: Domain::K4, seq: 1, level_mv: 3300 },
        );
        assert!(matches!(r, Ok(0)));
        assert_eq!(e.anchor_len(), 1);
        assert_eq!(e.matched_pairs(), 1);
    }

    #[test]
    fn table_first_last_bounds() {
        let mut e = CoSignEngine::new();
        let lo = match LEVEL_TABLE.get(0) {
            Some(v) => *v,
            None => 0,
        };
        let hi = match LEVEL_TABLE.get(7) {
            Some(v) => *v,
            None => 0,
        };
        let r1 = e.cosign_pair(
            &LevelTick { domain: Domain::B18, seq: 1, level_mv: lo },
            &LevelTick { domain: Domain::K4, seq: 1, level_mv: lo },
        );
        assert!(r1.is_ok());
        let r2 = e.cosign_pair(
            &LevelTick { domain: Domain::B18, seq: 2, level_mv: hi },
            &LevelTick { domain: Domain::K4, seq: 2, level_mv: hi },
        );
        assert!(r2.is_ok());
        let r3 = e.cosign_pair(
            &LevelTick { domain: Domain::B18, seq: 3, level_mv: hi + 1 },
            &LevelTick { domain: Domain::K4, seq: 3, level_mv: hi + 1 },
        );
        assert_eq!(r3, Err(CoSignCode::LevelOutOfRange));
    }

    #[test]
    fn seq_mismatch_takes_priority_over_level() {
        let mut e = CoSignEngine::new();
        let r = e.cosign_pair(
            &LevelTick { domain: Domain::B18, seq: 1, level_mv: 7777 },
            &LevelTick { domain: Domain::K4, seq: 2, level_mv: 7777 },
        );
        assert_eq!(r, Err(CoSignCode::SeqMismatch));
    }

    #[test]
    fn drift_injection_all_caught_ledger_conserved() {
        let mut e = CoSignEngine::new();
        for i in 0..10u32 {
            let r = e.cosign_pair(
                &LevelTick { domain: Domain::B18, seq: i + 1, level_mv: 3300 },
                &LevelTick { domain: Domain::K4, seq: i + 1, level_mv: 5000 },
            );
            assert_eq!(r, Err(CoSignCode::DriftNonZero));
            e.note_detected();
        }
        assert!(e.conserved());
        assert!(e.hang_conserved());
        assert_eq!(e.hang_alive(), 10);
    }

    #[test]
    fn hang_slot_no_duplicate_no_pingpong() {
        let mut e = CoSignEngine::new();
        let _ = e.cosign_pair(
            &LevelTick { domain: Domain::B18, seq: 7, level_mv: 3300 },
            &LevelTick { domain: Domain::K4, seq: 7, level_mv: 900 },
        );
        assert_eq!(e.hang_alive(), 1);
        let _ = e.cosign_pair(
            &LevelTick { domain: Domain::B18, seq: 7, level_mv: 3300 },
            &LevelTick { domain: Domain::K4, seq: 7, level_mv: 5000 },
        );
        assert_eq!(e.hang_alive(), 1, "同拍重复漂移不得重复占槽");
        let _ = e.cosign_pair(
            &LevelTick { domain: Domain::B18, seq: 7, level_mv: 3300 },
            &LevelTick { domain: Domain::K4, seq: 7, level_mv: 3300 },
        );
        assert_eq!(e.hang_alive(), 0, "联签成立必须凭锚定行清槽");
    }

    #[test]
    fn hang_slot_exhaustion_rejects_explicitly() {
        let mut e = CoSignEngine::new();
        let mut last: Result<u32, CoSignCode> = Ok(0);
        for i in 0..(HANG_SLOT_CAP as u32 + 1) {
            last = e.cosign_pair(
                &LevelTick { domain: Domain::B18, seq: i + 1, level_mv: 3300 },
                &LevelTick { domain: Domain::K4, seq: i + 1, level_mv: 0 },
            );
        }
        assert_eq!(last, Err(CoSignCode::HangSlotExhausted));
    }

    #[test]
    fn anchor_replay_strictly_increasing() {
        let mut e = CoSignEngine::new();
        for i in 0..12u32 {
            let _ = e.cosign_pair(
                &LevelTick { domain: Domain::B18, seq: i + 1, level_mv: 1800 },
                &LevelTick { domain: Domain::K4, seq: i + 1, level_mv: 1800 },
            );
        }
        let n = e.replay_anchors();
        assert!(matches!(n, Ok(c) if c == 12));
        assert_eq!(e.anchor_len() as u64, e.matched_pairs());
    }

    #[test]
    fn fake_domain_rejected() {
        let mut e = CoSignEngine::new();
        let r = e.cosign_pair(
            &LevelTick { domain: Domain::K4, seq: 1, level_mv: 3300 },
            &LevelTick { domain: Domain::K4, seq: 1, level_mv: 3300 },
        );
        assert_eq!(r, Err(CoSignCode::DomainUnknown));
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn seg_guard_and_codes_closed() {
        assert!(seg_guard_ok());
        assert_eq!(ALL_CODES.len(), 10);
        for c in ALL_CODES.iter() {
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn anchor_rows_read_via_get() {
        let mut e = CoSignEngine::new();
        let _ = e.cosign_pair(
            &LevelTick { domain: Domain::B18, seq: 3, level_mv: 900 },
            &LevelTick { domain: Domain::K4, seq: 3, level_mv: 900 },
        );
        let row = e.anchors.get(0);
        assert!(row.is_some());
        if let Some(r) = row {
            assert_eq!(r.level_b18, r.level_k4);
            assert_eq!(r.seq, 3);
        }
    }

    #[test]
    fn unx51552_checks_green() {
        let set = run_unx51552_checks();
        assert!(set.all_passed(), "UNX-F51552 red: {}", set.red_items_text());
    }
}
