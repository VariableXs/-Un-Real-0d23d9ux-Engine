//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18454 · 双份 manifest 冲突仲裁（内嵌+外部） · 0x7ED40 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18454-J1 内嵌优先仲裁断言全过，冲突告警入账可导出
//! 本模块结构清单：
//!   1. 冻结常量层：源类别闭集 2（内嵌=0/外部=1）/ 键三元组域 /
//!      容量 / 判据轮数（推导注释写死）；
//!   2. 诊断码闭集 ArCode（code()=SEG_BASE+n，段内 0x40 窗口）；
//!   3. 数据结构一（仲裁表）：ArbTable——键三元组行账（进程/API/维度/
//!      值/源），每键一胜者（与 F18421 表同键域口径）；
//!   4. 数据结构二（裁决表）：AlertLedger——冲突告警账追加只读
//!      （序号/键/内嵌值/外部值/胜者源），可导出为回放文本；
//!   5. 仲裁规则（冻结 v1）：同键异值异源 → 冲突告警 + 内嵌优先
//!      （内嵌在场必胜，外部只告警不生效）；同键同值 → 去重记账
//!      不告警；同源异值 → 更新记账不告警；
//!   6. 守恒账：告警数 == 冲突对数；导出行数 == 告警数；
//!   7. 注入核 4 类×10 轮：外部抢写 / 告警丢账 / 序号重复 /
//!      胜者错标，逐类断言被显式拒绝；
//!   8. 黄金全链 golden_run：双源混合注册→仲裁全过→导出回放逐字
//!      一致→注入全抓→双跑一致；
//!   9. run_unx18454_checks：15 条判据（断言性质）。
//! 为什么内嵌优先：manifest 双份并存时 Windows ActCtx 语义以内嵌
//! 资源为准——外部文件只是旁证；若外部值能盖过内嵌值，谎报面即
//! 被旁路，故仲裁方向写死单向（内嵌 → 胜）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7ED40，AI-VX11-1105-G1·A06 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7ED40;

// ── 冻结常量（推导注记） ─────────────────────────────────────────
/// 源类别闭集（冻结 2：内嵌=0、外部=1）。
pub const SRC_EMBEDDED: u32 = 0;
pub const SRC_EXTERNAL: u32 = 1;
/// 仲裁表容量（冻结 16：双份 manifest 场景键规模上限）。
pub const ARB_CAP: usize = 16;
/// 维度闭集（与 F18421 键域对齐）。
pub const DIM_CLOSEN: u32 = 4;
/// API 族闭集（与 F18424 族数对齐）。
pub const API_CLOSEN: u32 = 6;
/// 正例混合注册轮数（冻结 8：4 键 × 双源）。
pub const MIX_ROUNDS: usize = 8;
/// 注入判定轮数（冻结：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArCode {
    /// 源类别越界（闭集 {0,1} 外）
    SrcIllegal,
    /// 键域非法（进程键 0 / API 或维度越界）
    KeyIllegal,
    /// 仲裁表满
    TableFull,
    /// 外部抢写（外部值试图盖过内嵌生效值）
    ExternalOverride,
    /// 告警丢账（冲突对数 ≠ 告警数）
    AlertLost,
    /// 告警序号重复/断链
    AlertSeqBroken,
    /// 胜者错标（内嵌在场但胜者标成外部）
    WinnerMislabel,
    /// 导出回放不一致
    ExportDrift,
    /// 去重账破坏（同值异源必须记去重不告警）
    DedupBroken,
    /// 双跑不一致
    NotReproducible,
}

impl ArCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 诊断码别名：code() = SEG_BASE + n（与 val() 同值，契约骨架口径）。
    pub fn code(self) -> u32 {
        self.val()
    }
    pub fn detail(self) -> &'static str {
        match self {
            ArCode::SrcIllegal => "源类别越界：闭集 {内嵌,外部} 外",
            ArCode::KeyIllegal => "键域非法：进程键 0 或 API/维度越界",
            ArCode::TableFull => "仲裁表满：16 键上限",
            ArCode::ExternalOverride => "外部抢写：内嵌生效值不许被外部盖过",
            ArCode::AlertLost => "告警丢账：冲突对数与告警数不等",
            ArCode::AlertSeqBroken => "告警序号重复或断链",
            ArCode::WinnerMislabel => "胜者错标：内嵌在场胜者必为内嵌",
            ArCode::ExportDrift => "导出回放不一致：导出面漂移",
            ArCode::DedupBroken => "去重账破坏：同值异源必须记去重不告警",
            ArCode::NotReproducible => "双跑不一致：确定性红线",
        }
    }
    pub const ALL: [ArCode; 10] = [
        ArCode::SrcIllegal,
        ArCode::KeyIllegal,
        ArCode::TableFull,
        ArCode::ExternalOverride,
        ArCode::AlertLost,
        ArCode::AlertSeqBroken,
        ArCode::WinnerMislabel,
        ArCode::ExportDrift,
        ArCode::DedupBroken,
        ArCode::NotReproducible,
    ];
}

/// 按数值取中文文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ArCode::ALL.len() {
        if ArCode::ALL[i].val() == code {
            return ArCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7ED40 段闭集内"
}

// ── 数据结构一：仲裁表行 ────────────────────────────────────────
/// 仲裁表行：键三元组 + 生效值 + 胜者源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArbRow {
    pub proc: u32,
    pub api: u32,
    pub dim: u32,
    pub value: u32,
    pub src: u32,
}

impl ArbRow {
    pub fn same_key(&self, proc: u32, api: u32, dim: u32) -> bool {
        self.proc == proc && self.api == api && self.dim == dim
    }
}

// ── 数据结构二：告警账行 ────────────────────────────────────────
/// 冲突告警账行：序号 + 键 + 内嵌值 + 外部值 + 胜者源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlertRow {
    pub seq: u32,
    pub proc: u32,
    pub api: u32,
    pub dim: u32,
    pub emb_val: u32,
    pub ext_val: u32,
    pub winner: u32,
}

/// 告警账 + 仲裁表 + 计数账。
pub struct ArbState {
    pub table: Vec<ArbRow>,
    pub alerts: Vec<AlertRow>,
    pub dedup_count: u32,
    pub update_count: u32,
    pub conflict_pairs: u32,
    pub seq_next: u32,
}

impl ArbState {
    pub fn new() -> ArbState {
        ArbState {
            table: Vec::new(),
            alerts: Vec::new(),
            dedup_count: 0,
            update_count: 0,
            conflict_pairs: 0,
            seq_next: 1,
        }
    }

    fn find_key(&self, proc: u32, api: u32, dim: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.table.len() {
            if let Some(r) = self.table.get(i) {
                if r.same_key(proc, api, dim) {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 注册一路 manifest 声明：三分支仲裁（冻结 v1 规则）。
    pub fn register(&mut self, proc: u32, api: u32, dim: u32, value: u32, src: u32)
        -> Result<u32, u32>
    {
        // 键域守卫。
        if proc == 0 || api >= API_CLOSEN || dim >= DIM_CLOSEN {
            return Err(ArCode::KeyIllegal.val());
        }
        // 源闭集守卫。
        if src != SRC_EMBEDDED && src != SRC_EXTERNAL {
            return Err(ArCode::SrcIllegal.val());
        }
        match self.find_key(proc, api, dim) {
            None => {
                // 首次声明：直接入表（容量守卫）。
                if self.table.len() >= ARB_CAP {
                    return Err(ArCode::TableFull.val());
                }
                self.table.push(ArbRow { proc, api, dim, value, src });
                Ok(0)
            }
            Some(idx) => {
                let cur = match self.table.get(idx) {
                    Some(r) => *r,
                    None => return Err(ArCode::KeyIllegal.val()),
                };
                if cur.value == value {
                    // 同值异源：去重记账，不告警。
                    self.dedup_count = self.dedup_count.saturating_add(1);
                    return Ok(1);
                }
                if cur.src == src {
                    // 同源异值：更新记账，不告警（后写为准，同源无冲突）。
                    if let Some(r) = self.table.get_mut(idx) {
                        r.value = value;
                    }
                    self.update_count = self.update_count.saturating_add(1);
                    return Ok(2);
                }
                // 异源异值：冲突。内嵌优先——内嵌值必生效。
                let seq = self.seq_next;
                let (emb_val, ext_val) = if cur.src == SRC_EMBEDDED {
                    (cur.value, value)
                } else {
                    (value, cur.value)
                };
                self.alerts.push(AlertRow {
                    seq,
                    proc,
                    api,
                    dim,
                    emb_val,
                    ext_val,
                    winner: SRC_EMBEDDED,
                });
                self.seq_next = self.seq_next.saturating_add(1);
                self.conflict_pairs = self.conflict_pairs.saturating_add(1);
                // 内嵌在场必胜：生效值切到内嵌值。
                if let Some(r) = self.table.get_mut(idx) {
                    r.value = emb_val;
                    r.src = SRC_EMBEDDED;
                }
                Ok(3)
            }
        }
    }

    /// 查生效值（仲裁后的读出口）。
    pub fn query(&self, proc: u32, api: u32, dim: u32) -> Option<u32> {
        let idx = self.find_key(proc, api, dim)?;
        match self.table.get(idx) {
            Some(r) => Some(r.value),
            None => None,
        }
    }

    /// 守恒：告警数 == 冲突对数。
    pub fn conserve(&self) -> Result<(), u32> {
        if self.alerts.len() as u32 != self.conflict_pairs {
            return Err(ArCode::AlertLost.val());
        }
        Ok(())
    }

    /// 裁决面审计：序号连续 + 胜者必内嵌 + 生效值与内嵌值一致。
    pub fn audit(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.alerts.len() {
            let a = match self.alerts.get(i) {
                Some(x) => *x,
                None => return Err(ArCode::AlertLost.val()),
            };
            if a.seq != (i as u32) + 1 {
                return Err(ArCode::AlertSeqBroken.val());
            }
            if a.winner != SRC_EMBEDDED {
                return Err(ArCode::WinnerMislabel.val());
            }
            match self.query(a.proc, a.api, a.dim) {
                Some(v) if v == a.emb_val => {}
                _ => return Err(ArCode::ExternalOverride.val()),
            }
            i += 1;
        }
        Ok(())
    }

    /// 导出：表头 + 告警数据行（可导出回放文本）。
    pub fn export_text(&self) -> String {
        let mut out = String::new();
        out.push_str("ARB-ALERTS-V1|seq|proc|api|dim|emb|ext|winner\n");
        let mut i = 0;
        while i < self.alerts.len() {
            let a = match self.alerts.get(i) {
                Some(x) => *x,
                None => break,
            };
            let line = format!(
                "A|{}|{}|{}|{}|{}|{}|{}\n",
                a.seq, a.proc, a.api, a.dim, a.emb_val, a.ext_val, a.winner
            );
            out.push_str(&line);
            i += 1;
        }
        out
    }
}

/// 回放核：导出文本行数（数据行）== 告警数，且二次导出逐字节一致。
pub fn replay_check(st: &ArbState) -> Result<(), u32> {
    let text = st.export_text();
    // 数据行 = 总行数 - 表头行。
    let mut lines = 0u32;
    let mut start = 0usize;
    let mut k = 0;
    while k <= text.len() {
        let is_end = k == text.len();
        let is_nl = !is_end && {
            let b = text.as_bytes();
            b[k] == b'\n'
        };
        if is_end || is_nl {
            if k > start {
                lines = lines.saturating_add(1);
            }
            start = k + 1;
        }
        k += 1;
    }
    // 表头占 1 行。
    if lines.saturating_sub(1) != st.alerts.len() as u32 {
        return Err(ArCode::ExportDrift.val());
    }
    let again = st.export_text();
    if again != text {
        return Err(ArCode::ExportDrift.val());
    }
    Ok(())
}

// ── 混合注册脚本（确定性） ──────────────────────────────────────
/// 双源混合注册 8 轮：每两轮共用一键（键 = (1+轮/2, 组%3, 组%4)），
/// 组内先内嵌后外部异值 → 恰 4 组冲突；另配 2 例同值去重。
pub fn mixed_register(st: &mut ArbState) -> Result<(), u32> {
    let mut i = 0;
    while i < MIX_ROUNDS {
        let pair = (i / 2) as u32;
        let proc = 1 + pair;
        let api = pair % 3;
        let dim = pair % DIM_CLOSEN;
        let src = if i % 2 == 0 { SRC_EMBEDDED } else { SRC_EXTERNAL };
        let value = 100 + (i as u32) * 3;
        st.register(proc, api, dim, value, src)?;
        i += 1;
    }
    // 同值异源去重 2 例（不告警）。
    st.register(1, 0, 0, 100, SRC_EXTERNAL)?;
    st.register(2, 1, 1, 106, SRC_EMBEDDED)?;
    Ok(())
}

// ── 注入核（4 类 ×10 轮） ───────────────────────────────────────
/// 注入检出账：四槽 tries/caught 成对。
#[derive(Debug)]
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn missed(&self) -> u32 {
        let mut m = 0;
        let mut i = 0;
        while i < 4 {
            m += self.tries[i].saturating_sub(self.caught[i]);
            i += 1;
        }
        m
    }
}

/// 10 轮注入：槽 0 外部抢写 / 槽 1 告警丢账 / 槽 2 序号重复 /
/// 槽 3 胜者错标。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽 0：外部抢写——内嵌 100 在场，外部 999 注册后查询面
        // 仍必须是 100（外部值永不生效）。
        let mut st = ArbState::new();
        st.register(1, 0, 0, 100, SRC_EMBEDDED).ok();
        let arb = st.register(1, 0, 0, 999, SRC_EXTERNAL);
        let hit0 = arb == Ok(3)
            && st.query(1, 0, 0) == Some(100)
            && st.audit().is_ok();
        led.record(0, hit0);
        // 槽 1：告警丢账——副本里删一条告警，守恒必破。
        let mut st1 = ArbState::new();
        st1.register(1, 1, 1, 50, SRC_EMBEDDED).ok();
        st1.register(1, 1, 1, 60, SRC_EXTERNAL).ok();
        let clean1 = st1.conserve().is_ok();
        st1.alerts.pop();
        let hit1 = clean1 && st1.conserve().err() == Some(ArCode::AlertLost.val());
        led.record(1, hit1);
        // 槽 2：序号重复——副本两条告警同序号，审计必红。
        let mut st2 = ArbState::new();
        st2.register(2, 0, 1, 10, SRC_EMBEDDED).ok();
        st2.register(2, 0, 1, 11, SRC_EXTERNAL).ok();
        st2.register(3, 2, 2, 20, SRC_EXTERNAL).ok();
        st2.register(3, 2, 2, 21, SRC_EMBEDDED).ok();
        let seq0 = st2.alerts.get(0).map(|x| x.seq).unwrap_or(0);
        if let Some(a) = st2.alerts.get_mut(1) {
            a.seq = seq0;
        }
        let hit2 = st2.audit().err() == Some(ArCode::AlertSeqBroken.val());
        led.record(2, hit2);
        // 槽 3：胜者错标——副本告警 winner 改成外部，审计必红。
        let mut st3 = ArbState::new();
        st3.register(4, 1, 2, 30, SRC_EMBEDDED).ok();
        st3.register(4, 1, 2, 31, SRC_EXTERNAL).ok();
        if let Some(a) = st3.alerts.get_mut(0) {
            a.winner = SRC_EXTERNAL;
        }
        let hit3 = st3.audit().err() == Some(ArCode::WinnerMislabel.val());
        led.record(3, hit3);
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：混合注册→仲裁全过→守恒→审计→导出回放→注入全抓→双跑。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut st = ArbState::new();
    mixed_register(&mut st)?;
    // 冲突组数推导：MIX_ROUNDS=8 → 4 组双源异值键（proc 1..4）。
    if st.conflict_pairs != (MIX_ROUNDS / 2) as u32 {
        return Err(ArCode::AlertLost.val());
    }
    st.conserve()?;
    st.audit()?;
    // 去重账：2 例同值异源。
    if st.dedup_count != 2 {
        return Err(ArCode::DedupBroken.val());
    }
    // 导出回放逐字节一致。
    replay_check(&st)?;
    let inj = inject_rounds(JUDGE_ROUNDS)?;
    if !inj.all_caught() || inj.missed() != 0 {
        return Err(ArCode::NotReproducible.val());
    }
    Ok((st.conflict_pairs, st.alerts.len() as u32, st.dedup_count))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ArCode::NotReproducible.val());
    }
    Ok(())
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 正探针：内嵌先声明、外部异值后注册——内嵌保持生效。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut st = ArbState::new();
    st.register(2, 1, 0, 7, SRC_EMBEDDED)?;
    st.register(2, 1, 0, 8, SRC_EXTERNAL)?;
    match st.query(2, 1, 0) {
        Some(v) if v == 7 => Ok(()),
        _ => Err(ArCode::ExternalOverride.val()),
    }
}

/// 负探针 1：源越界拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut st = ArbState::new();
    match st.register(2, 1, 0, 7, 9) {
        Err(c) if c == ArCode::SrcIllegal.val() => Ok(()),
        _ => Err(ArCode::SrcIllegal.val()),
    }
}

/// 负探针 2：键域拒（进程键 0 优先于源越界——键域先行）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut st = ArbState::new();
    match st.register(0, 1, 0, 7, 9) {
        Err(c) if c == ArCode::KeyIllegal.val() => Ok(()),
        _ => Err(ArCode::KeyIllegal.val()),
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ArCode::ALL.len() {
        let v = ArCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

// ── 判据收口 ────────────────────────────────────────────────────
pub fn run_unx18454_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18454 双份 manifest 冲突仲裁");
    // ① 黄金全链。
    let g = golden_run();
    s.add_dyn(
        "J1-黄金全链",
        g.is_ok(),
        &format!(
            "冲突{:?} 告警{:?} 去重{:?}",
            g.as_ref().map(|r| r.0).unwrap_or(0),
            g.as_ref().map(|r| r.1).unwrap_or(0),
            g.as_ref().map(|r| r.2).unwrap_or(0)
        ),
    );
    // ② 内嵌优先（J1 前半句）：外部异值注册后内嵌值仍生效。
    let mut st = ArbState::new();
    st.register(1, 0, 0, 100, SRC_EMBEDDED).ok();
    st.register(1, 0, 0, 999, SRC_EXTERNAL).ok();
    s.add("内嵌优先仲裁", st.query(1, 0, 0) == Some(100), "外部值 999 不生效");
    // ③ 内嵌后到也必胜：外部先声明、内嵌后注册，生效值切内嵌。
    let mut st2 = ArbState::new();
    st2.register(3, 2, 1, 40, SRC_EXTERNAL).ok();
    st2.register(3, 2, 1, 41, SRC_EMBEDDED).ok();
    s.add("内嵌后到必胜", st2.query(3, 2, 1) == Some(41), "生效值切换到内嵌");
    // ④ 冲突告警入账（J1 后半句）：告警数==冲突对数。
    let mut st3 = ArbState::new();
    mixed_register(&mut st3).ok();
    s.add("冲突告警入账", st3.conserve().is_ok() && st3.alerts.len() == 4,
        "4 组冲突 4 条告警，账不丢");
    // ⑤ 裁决面审计：序号连续+胜者内嵌+生效值一致。
    s.add("裁决面审计", st3.audit().is_ok(), "序号/胜者/生效值三查全过");
    // ⑥ 导出可回放：数据行数==告警数，二次导出逐字节一致。
    s.add("告警可导出回放", replay_check(&st3).is_ok(), "导出文本确定性");
    // ⑦ 去重记账：同值异源不告警。
    s.add("同值去重不告警", st3.dedup_count == 2 && st3.alerts.len() == 4,
        "去重 2 例照记，告警不虚发");
    // ⑧ 探针三序。
    s.add("探针三序",
        probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok(),
        "正仲裁/源越界拒/键域先行");
    // ⑨ 注入四类 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn(
        "注入四类10/10",
        inj_ok,
        &format!(
            "{}/{}/{}/{}",
            inj.as_ref().map(|l| l.caught[0]).unwrap_or(0),
            inj.as_ref().map(|l| l.caught[1]).unwrap_or(0),
            inj.as_ref().map(|l| l.caught[2]).unwrap_or(0),
            inj.as_ref().map(|l| l.caught[3]).unwrap_or(0)
        ),
    );
    // ⑩ 漏检为零。
    s.add("注入漏检为零", match &inj {
        Ok(l) => l.missed() == 0,
        Err(_) => false,
    }, "注入=检出+漏检，漏检=0");
    // ⑪ 表满边界：16 键填满后第 17 键拒。
    let mut stf = ArbState::new();
    let mut filled = 0u32;
    let mut k = 0;
    while k < ARB_CAP + 2 {
        let proc = 1 + (k as u32 / DIM_CLOSEN);
        let api = (k as u32) % API_CLOSEN;
        let dim = (k as u32) % DIM_CLOSEN;
        if stf.register(proc, api, dim, 1 + (k as u32), SRC_EMBEDDED).is_ok() {
            filled += 1;
        }
        k += 1;
    }
    s.add("表满边界", filled == ARB_CAP as u32, "16 键填满，超额键拒入");
    // ⑫ 判定次序：键域守卫先于源闭集。
    let mut sto = ArbState::new();
    s.add("判定次序键域先行",
        sto.register(0, 9, 9, 1, 9).err() == Some(ArCode::KeyIllegal.val()),
        "键域错先报（api/维度越界并入键域）");
    // ⑬ 双跑一致。
    s.add("双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // ⑭ 文案闭集。
    let mut closed = true;
    let mut ci = 0;
    while ci < ArCode::ALL.len() {
        if ArCode::ALL[ci].detail().is_empty() {
            closed = false;
        }
        ci += 1;
    }
    s.add("诊断文案闭集非空", closed, "10 码全有中文建议");
    // ⑮ 段位守卫。
    s.add("段位守卫 0x7ED40", segment_guard(), "SEG_BASE 0x40 对齐窗口");
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_always_wins() {
        let mut st = ArbState::new();
        st.register(1, 0, 0, 100, SRC_EMBEDDED).ok();
        st.register(1, 0, 0, 999, SRC_EXTERNAL).ok();
        assert_eq!(st.query(1, 0, 0), Some(100), "内嵌在场外部永不生效");
        let mut st2 = ArbState::new();
        st2.register(1, 0, 0, 999, SRC_EXTERNAL).ok();
        st2.register(1, 0, 0, 100, SRC_EMBEDDED).ok();
        assert_eq!(st2.query(1, 0, 0), Some(100), "内嵌后到仍必胜");
    }

    #[test]
    fn alerts_match_conflicts() {
        let mut st = ArbState::new();
        mixed_register(&mut st).unwrap_or(());
        assert_eq!(st.conflict_pairs, 4, "4 组冲突");
        assert_eq!(st.alerts.len() as u32, st.conflict_pairs, "告警守恒");
        assert!(st.conserve().is_ok());
    }

    #[test]
    fn dedup_no_alert() {
        let mut st = ArbState::new();
        st.register(1, 2, 3, 55, SRC_EMBEDDED).ok();
        st.register(1, 2, 3, 55, SRC_EXTERNAL).ok();
        assert_eq!(st.dedup_count, 1, "同值异源记去重");
        assert_eq!(st.alerts.len(), 0, "不告警");
    }

    #[test]
    fn same_source_update_no_alert() {
        let mut st = ArbState::new();
        st.register(1, 2, 3, 55, SRC_EXTERNAL).ok();
        st.register(1, 2, 3, 66, SRC_EXTERNAL).ok();
        assert_eq!(st.update_count, 1, "同源异值记更新");
        assert_eq!(st.alerts.len(), 0, "不告警");
        assert_eq!(st.query(1, 2, 3), Some(66), "同源后写生效");
    }

    #[test]
    fn export_replay_byte_identical() {
        let mut st = ArbState::new();
        mixed_register(&mut st).unwrap_or(());
        let t1 = st.export_text();
        let t2 = st.export_text();
        assert_eq!(t1, t2, "导出逐字节一致");
        assert!(t1.starts_with("ARB-ALERTS-V1|"), "表头冻结");
        assert!(replay_check(&st).is_ok());
    }

    #[test]
    fn audit_catches_seq_dup_and_mislabel() {
        let mut st = ArbState::new();
        st.register(2, 0, 1, 10, SRC_EMBEDDED).ok();
        st.register(2, 0, 1, 11, SRC_EXTERNAL).ok();
        st.register(3, 2, 2, 20, SRC_EXTERNAL).ok();
        st.register(3, 2, 2, 21, SRC_EMBEDDED).ok();
        let mut stc = ArbState::new();
        stc.table = st.table.clone();
        stc.alerts = st.alerts.clone();
        stc.seq_next = st.seq_next;
        stc.conflict_pairs = st.conflict_pairs;
        if let Some(a) = stc.alerts.get_mut(1) {
            a.seq = 1;
        }
        assert_eq!(stc.audit().err(), Some(ArCode::AlertSeqBroken.val()));
        let mut std2 = ArbState::new();
        std2.table = st.table.clone();
        std2.alerts = st.alerts.clone();
        std2.seq_next = st.seq_next;
        std2.conflict_pairs = st.conflict_pairs;
        if let Some(a) = std2.alerts.get_mut(0) {
            a.winner = SRC_EXTERNAL;
        }
        assert_eq!(std2.audit().err(), Some(ArCode::WinnerMislabel.val()));
    }

    #[test]
    fn external_override_injection_ten_rounds() {
        let mut r = 0;
        while r < JUDGE_ROUNDS {
            let mut st = ArbState::new();
            st.register(1, 1, 1, 100, SRC_EMBEDDED).ok();
            let arb = st.register(1, 1, 1, 777, SRC_EXTERNAL);
            assert_eq!(arb.unwrap_or(9), 3, "第 {} 轮冲突分支命中", r);
            assert_eq!(st.query(1, 1, 1), Some(100), "第 {} 轮外部值不生效", r);
            r += 1;
        }
    }

    #[test]
    fn table_full_boundary() {
        let mut st = ArbState::new();
        let mut k = 0;
        while k < ARB_CAP {
            let proc = 1 + (k as u32 / DIM_CLOSEN);
            let api = (k as u32) % API_CLOSEN;
            let dim = (k as u32) % DIM_CLOSEN;
            assert!(st.register(proc, api, dim, 1 + k as u32, SRC_EMBEDDED).is_ok(), "键 {} 入表", k);
            k += 1;
        }
        assert_eq!(
            st.register(99, 0, 0, 1, SRC_EMBEDDED).err(),
            Some(ArCode::TableFull.val())
        );
    }

    #[test]
    fn guard_order_key_before_src() {
        let mut st = ArbState::new();
        assert_eq!(
            st.register(0, 1, 0, 7, 9).err(),
            Some(ArCode::KeyIllegal.val()),
            "键域先行"
        );
        assert_eq!(
            st.register(2, 1, 0, 7, 9).err(),
            Some(ArCode::SrcIllegal.val()),
            "源闭集后判"
        );
    }

    #[test]
    fn injection_ledger_all_caught_no_miss() {
        let inj = inject_rounds(10).unwrap_or_else(|_| InjectLedger::new());
        assert!(inj.all_caught());
        assert_eq!(inj.missed(), 0);
    }

    #[test]
    fn probes_and_double_run() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert_eq!(golden_run(), golden_run(), "双跑一致");
    }

    #[test]
    fn checks_set_all_green() {
        let s = run_unx18454_checks();
        assert!(s.all_passed(), "本卡判据必须全绿: {}", s.red_items_text());
        assert!(s.len() >= 8 && s.len() <= 20, "判据条数 {} 在窗内", s.len());
    }
}
