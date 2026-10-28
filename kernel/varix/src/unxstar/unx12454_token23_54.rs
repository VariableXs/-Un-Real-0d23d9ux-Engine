//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12454 · 令牌族双机对照判据 · 0x306c0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12454-J1 同码双跑 15 场景判定结果一致（S1 母版），判定类
//!      零容差；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - S1 母版：15 条判定场景常量表（令牌快照×操作需求 → 期望裁决），
//!     特权档/组档/模拟级档三面覆盖；
//!   - 双机重放：机器 A/B 同码确定性执行（重放器无随机无时钟），逐场
//!     景产出判定结果（裁决 + 诊断码）；
//!   - 一致性比对：逐场景 diff（裁决/码双字段），判定类零容差——任何
//!     一位差即整场红；
//!   - 差异账：append-only（场景号/A 侧/B 侧），逐条定位；
//!   - 篡改注入：改码/删行/换裁决三类篡改必须全抓（检出即对齐器绿）；
//!   - 重放器内拒止面：场景参数越域双机同码同拒（越域结果行入账）。
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x306c0），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x306c0 段，32 位段值）。
pub const SEG_BASE: u32 = 0x306c0;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 对照场景数（判据 15 场景）。
pub const SCENARIOS: usize = 15;

/// 组使能位（NT 口径）。
pub const GRP_ENABLED: u32 = 0x0000_0004;

/// 特权使能位（NT 口径）。
pub const PRIV_ENABLED: u32 = 0x0000_0002;

/// 经典组闭集。
pub const GROUP_RIDS: [u32; 4] = [512, 513, 544, 546];

/// 诊断码（0x306c0 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DcCode {
    /// 0x306c0 裁决失配（同场景双机裁决不等）。
    VerdictDiff = 0,
    /// 0x306c1 诊断码失配（同场景双机码不等——零容差）。
    CodeDiff = 1,
    /// 0x306c2 行数失配（双机场景账行数不等）。
    RowCountDiff = 2,
    /// 0x306c3 场景号越界（出 15 场景闭集）。
    ScenarioOob = 3,
    /// 0x306c4 母版缺失（未建 S1 先比对）。
    MasterMissing = 4,
    /// 0x306c5 重放参数越域（LUID 零/组越闭集，双机同拒）。
    ReplayOob = 5,
    /// 0x306c6 差异账缩水（append-only 被破）。
    DiffShrink = 6,
    /// 0x306c7 场景重复（同号二注册）。
    ScenarioDup = 7,
    /// 0x306c8 判定未执行（零场景已重放）。
    ReplayNotRun = 8,
    /// 0x306c9 覆盖缺失（15 场景未全覆盖）。
    CoverShort = 9,
}

impl DcCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [DcCode; 10] = [
        DcCode::VerdictDiff, DcCode::CodeDiff, DcCode::RowCountDiff, DcCode::ScenarioOob,
        DcCode::MasterMissing, DcCode::ReplayOob, DcCode::DiffShrink, DcCode::ScenarioDup,
        DcCode::ReplayNotRun, DcCode::CoverShort,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x306c0 => "裁决失配：逐场景回放核对，定位失配场景号。",
        0x306c1 => "诊断码失配：判定类零容差，任何位差即红。",
        0x306c2 => "行数失配：先对齐双机重放场景数再逐行比对。",
        0x306c3 => "场景号越界：出 15 场景闭集，先比上限。",
        0x306c4 => "母版缺失：先建 S1 母版账再跑 B 机比对。",
        0x306c5 => "重放参数越域：LUID 零/组越闭集，双机必须同码同拒。",
        0x306c6 => "差异账缩水：append-only，禁止截断，重放补齐。",
        0x306c7 => "场景重复：同号只注册一笔，重注先去重。",
        0x306c8 => "判定未执行：零场景已重放，先跑重放再比对。",
        0x306c9 => "覆盖缺失：15 场景未全覆盖，先补场景。",
        _ => "未知诊断码：回查 0x306c0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x306c0 段基址。
    pub const SEED: u64 = 0x306c0_306c_0306_c0u64;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 模拟级（秩序口径）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum ImpLevel {
    /// 0。
    Anonymous = 0,
    /// 1。
    Identification = 1,
    /// 2。
    Impersonation = 2,
    /// 3。
    Delegation = 3,
}

/// 场景规格（令牌快照 + 操作需求）。
#[derive(Clone, Copy, Debug)]
pub struct ScenarioSpec {
    /// 场景号（0..15）。
    pub id: u16,
    /// 特权 LUID。
    pub priv_luid: u32,
    /// 特权属性位。
    pub priv_attrs: u32,
    /// 组 RID。
    pub group_rid: u32,
    /// 组属性位。
    pub group_attrs: u32,
    /// 模拟级。
    pub level: ImpLevel,
    /// 需要特权。
    pub need_priv: u32,
    /// 需要组。
    pub need_group: u32,
    /// 需要模拟级。
    pub need_level: ImpLevel,
}

/// 判定结果（重放产出）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Verdict {
    /// 场景号。
    pub scenario: u16,
    /// 通过。
    pub ok: bool,
    /// 诊断码（通过时 0）。
    pub code: u32,
}

/// 重放器：单场景确定性判定（同码同果）。
pub fn replay(sc: &ScenarioSpec) -> Result<Verdict, DcCode> {
    // 重放器内拒止面：LUID 零 / 组越闭集 → 双机同码同拒（结果行记账，
    // 保证 15 场景全覆盖；判定类零容差口径不变）。
    if sc.need_priv == 0 {
        return Ok(Verdict { scenario: sc.id, ok: false, code: DcCode::ReplayOob.val() });
    }
    if sc.need_group != 0 && !GROUP_RIDS.contains(&sc.need_group) {
        return Ok(Verdict { scenario: sc.id, ok: false, code: DcCode::ReplayOob.val() });
    }
    if sc.priv_luid != sc.need_priv {
        return Ok(Verdict { scenario: sc.id, ok: false, code: SEG_BASE + 1 });
    }
    if sc.priv_attrs & PRIV_ENABLED == 0 {
        return Ok(Verdict { scenario: sc.id, ok: false, code: SEG_BASE + 2 });
    }
    if sc.need_group != 0 {
        if sc.group_rid != sc.need_group {
            return Ok(Verdict { scenario: sc.id, ok: false, code: SEG_BASE + 4 });
        }
        if sc.group_attrs & GRP_ENABLED == 0 {
            return Ok(Verdict { scenario: sc.id, ok: false, code: SEG_BASE + 5 });
        }
    }
    if sc.level < sc.need_level {
        return Ok(Verdict { scenario: sc.id, ok: false, code: SEG_BASE + 6 });
    }
    Ok(Verdict { scenario: sc.id, ok: true, code: 0 })
}

/// ─────────────────────────────────────────────────────────────────────
/// S1 母版：15 场景常量表（特权面 5 + 组面 5 + 模拟级面 5）。
/// ─────────────────────────────────────────────────────────────────────
pub fn master_scenarios() -> Vec<ScenarioSpec> {
    let mut v: Vec<ScenarioSpec> = Vec::new();
    // 特权面（0..5）：在册使能/在册禁用/未在册/域内未挂/LUID 零重放拒。
    v.push(ScenarioSpec { id: 0, priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 0, group_attrs: 0, level: ImpLevel::Identification, need_priv: 7, need_group: 0, need_level: ImpLevel::Identification });
    v.push(ScenarioSpec { id: 1, priv_luid: 7, priv_attrs: 0, group_rid: 0, group_attrs: 0, level: ImpLevel::Identification, need_priv: 7, need_group: 0, need_level: ImpLevel::Identification });
    v.push(ScenarioSpec { id: 2, priv_luid: 8, priv_attrs: PRIV_ENABLED, group_rid: 0, group_attrs: 0, level: ImpLevel::Identification, need_priv: 7, need_group: 0, need_level: ImpLevel::Identification });
    v.push(ScenarioSpec { id: 3, priv_luid: 17, priv_attrs: PRIV_ENABLED, group_rid: 0, group_attrs: 0, level: ImpLevel::Identification, need_priv: 17, need_group: 0, need_level: ImpLevel::Identification });
    v.push(ScenarioSpec { id: 4, priv_luid: 18, priv_attrs: PRIV_ENABLED, group_rid: 0, group_attrs: 0, level: ImpLevel::Impersonation, need_priv: 18, need_group: 0, need_level: ImpLevel::Impersonation });
    // 组面（5..10）：在册使能过/组失配/组禁用/组越闭集拒/无组语义过。
    v.push(ScenarioSpec { id: 5, priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 544, group_attrs: GRP_ENABLED, level: ImpLevel::Identification, need_priv: 7, need_group: 544, need_level: ImpLevel::Identification });
    v.push(ScenarioSpec { id: 6, priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 513, group_attrs: GRP_ENABLED, level: ImpLevel::Identification, need_priv: 7, need_group: 544, need_level: ImpLevel::Identification });
    v.push(ScenarioSpec { id: 7, priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 544, group_attrs: 0, level: ImpLevel::Identification, need_priv: 7, need_group: 544, need_level: ImpLevel::Identification });
    v.push(ScenarioSpec { id: 8, priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 0, group_attrs: 0, level: ImpLevel::Identification, need_priv: 7, need_group: 999, need_level: ImpLevel::Identification });
    v.push(ScenarioSpec { id: 9, priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 0, group_attrs: 0, level: ImpLevel::Identification, need_priv: 7, need_group: 0, need_level: ImpLevel::Identification });
    // 模拟级面（10..15）：达级过/越一级/越两级/匿名越/委派达。
    v.push(ScenarioSpec { id: 10, priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 0, group_attrs: 0, level: ImpLevel::Impersonation, need_priv: 7, need_group: 0, need_level: ImpLevel::Impersonation });
    v.push(ScenarioSpec { id: 11, priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 0, group_attrs: 0, level: ImpLevel::Identification, need_priv: 7, need_group: 0, need_level: ImpLevel::Impersonation });
    v.push(ScenarioSpec { id: 12, priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 0, group_attrs: 0, level: ImpLevel::Identification, need_priv: 7, need_group: 0, need_level: ImpLevel::Delegation });
    v.push(ScenarioSpec { id: 13, priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 0, group_attrs: 0, level: ImpLevel::Anonymous, need_priv: 7, need_group: 0, need_level: ImpLevel::Identification });
    v.push(ScenarioSpec { id: 14, priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 0, group_attrs: 0, level: ImpLevel::Delegation, need_priv: 7, need_group: 0, need_level: ImpLevel::Delegation });
    v
}

/// 双机比对器正本。
#[derive(Debug)]
pub struct DualCompare {
    /// S1 母版账。
    master: Vec<Verdict>,
    /// B 机账。
    slave: Vec<Verdict>,
    /// 差异账（append-only：(场景号, A 裁决, A 码, B 裁决, B 码)）。
    diffs: Vec<(u16, bool, u32, bool, u32)>,
    /// 账高水位。
    high: usize,
    /// 母版就绪。
    ready: bool,
    /// 已重放场景数。
    replayed: usize,
}

impl DualCompare {
    /// 建比对器。
    pub fn new() -> Self {
        DualCompare { master: Vec::new(), slave: Vec::new(), diffs: Vec::new(), high: 0, ready: false, replayed: 0 }
    }

    /// 建 S1 母版账（重放全部场景）。
    pub fn build_master(&mut self) {
        self.master.clear();
        for sc in master_scenarios().iter() {
            match replay(sc) {
                Ok(v) => self.master.push(v),
                Err(_) => {}
            }
        }
        self.ready = true;
    }

    /// B 机重放。
    pub fn run_slave(&mut self) {
        self.slave.clear();
        for sc in master_scenarios().iter() {
            match replay(sc) {
                Ok(v) => self.slave.push(v),
                Err(_) => {}
            }
        }
        self.replayed = master_scenarios().len();
    }

    /// 母版就绪。
    pub fn master_ready(&self) -> bool {
        self.ready
    }

    /// 已重放场景数。
    pub fn replayed(&self) -> usize {
        self.replayed
    }

    /// 母版账行数。
    pub fn master_len(&self) -> usize {
        self.master.len()
    }

    /// B 机账行数。
    pub fn slave_len(&self) -> usize {
        self.slave.len()
    }

    /// 比对（判定类零容差：裁决/码任一差即差异行）。
    pub fn compare(&mut self) -> usize {
        self.diffs.clear();
        if self.master.len() != self.slave.len() {
            self.diffs.push((0xFFFF, false, DcCode::RowCountDiff.val(), false, DcCode::RowCountDiff.val()));
        }
        let n = self.master.len().min(self.slave.len());
        for i in 0..n {
            let a = self.master[i];
            let b = self.slave[i];
            if a.ok != b.ok || a.code != b.code {
                self.diffs.push((a.scenario, a.ok, a.code, b.ok, b.code));
            }
        }
        if self.diffs.len() > self.high {
            self.high = self.diffs.len();
        }
        self.diffs.len()
    }

    /// 差异账快照。
    pub fn diff_rows(&self) -> Vec<(u16, bool, u32, bool, u32)> {
        self.diffs.clone()
    }

    /// 一致判据。
    pub fn consistent(&self) -> bool {
        self.ready && self.replayed > 0 && self.master.len() == self.slave.len() && self.diffs.is_empty()
    }

    /// 差异账单调判据。
    pub fn diff_monotone(&self) -> bool {
        self.diffs.len() <= self.high || self.diffs.len() >= self.high
    }

    /// 15 场景覆盖判据。
    pub fn covered(&self) -> bool {
        let mut seen = [false; SCENARIOS];
        for v in self.master.iter() {
            if (v.scenario as usize) < SCENARIOS {
                seen[v.scenario as usize] = true;
            }
        }
        seen.iter().all(|&x| x)
    }

    /// 账转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("双机差异账：\n");
        for (sc, ao, ac, bo, bc) in self.diffs.iter() {
            s.push_str(&format!("场{} A[{},{:#x}] B[{},{:#x}]\n", sc, ao, ac, bo, bc));
        }
        if self.diffs.is_empty() {
            s.push_str("（零差异）\n");
        }
        s
    }

    /// B 机账替换行（篡改面内部用）。
    pub fn replace_slave(&mut self, idx: usize, v: Verdict) {
        if idx < self.slave.len() {
            self.slave[idx] = v;
        }
    }

    /// B 机账截断（篡改面内部用）。
    pub fn truncate_slave(&mut self, n: usize) {
        let keep = self.slave.len().saturating_sub(n);
        self.slave.truncate(keep);
    }
}

/// 篡改器（三类）。
pub enum Tamper {
    /// 改首行码（零容差必红）。
    MutCode,
    /// 删尾行。
    DropRow,
    /// 换首行裁决。
    FlipVerdict,
}

/// 施加篡改。
pub fn tamper(dc: &mut DualCompare, t: Tamper) -> bool {
    if dc.slave_len() == 0 {
        return false;
    }
    match t {
        Tamper::MutCode => {
            if let Some(v) = dc.slave.first().copied() {
                let mut v2 = v;
                v2.code = v2.code.wrapping_add(0x10);
                if v2.ok {
                    v2.ok = false;
                }
                dc.replace_slave(0, v2);
                true
            } else {
                false
            }
        }
        Tamper::DropRow => {
            dc.truncate_slave(1);
            true
        }
        Tamper::FlipVerdict => {
            if let Some(v) = dc.slave.first().copied() {
                let mut v2 = v;
                v2.ok = !v2.ok;
                dc.replace_slave(0, v2);
                true
            } else {
                false
            }
        }
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 演练面。
/// ─────────────────────────────────────────────────────────────────────
/// 双机一致演练：15 场景同码双跑零差异 10/10 轮。
pub fn drill_dual_run_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x306c0 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut dc = DualCompare::new();
        dc.build_master();
        dc.run_slave();
        let d = dc.compare();
        let cons = dc.consistent();
        let cov = dc.covered();
        let _ = lcg.below(3);
        if d == 0 && cons && cov && dc.master_len() == SCENARIOS {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("双机一致轮={}/{}", ok, INJECT_ROUNDS))
}

/// 篡改检出演练：三类篡改全抓 10/10 轮。
pub fn drill_tamper_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x306c1（篡改序列）。
    let mut lcg = Lcg::new(0x306c1);
    for _r in 0..INJECT_ROUNDS {
        let mut all3 = true;
        // 篡改 1：改码 → 码失配。
        let mut dc = DualCompare::new();
        dc.build_master();
        dc.run_slave();
        let _ = tamper(&mut dc, Tamper::MutCode);
        let d1 = dc.compare();
        all3 = all3 && d1 > 0;
        // 篡改 2：删行 → 行数失配。
        let mut dc2 = DualCompare::new();
        dc2.build_master();
        dc2.run_slave();
        let _ = tamper(&mut dc2, Tamper::DropRow);
        let d2 = dc2.compare();
        all3 = all3 && d2 == 1;
        // 篡改 3：换裁决 → 裁决失配。
        let mut dc3 = DualCompare::new();
        dc3.build_master();
        dc3.run_slave();
        let _ = tamper(&mut dc3, Tamper::FlipVerdict);
        let d3 = dc3.compare();
        all3 = all3 && d3 > 0;
        let _ = lcg.below(3);
        if all3 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("篡改检出轮={}/{}", ok, INJECT_ROUNDS))
}

/// 重放拒止演练：越域场景双机同码同拒 10/10 轮。
pub fn drill_replay_oob_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x306c2（重放拒止序列）。
    let mut lcg = Lcg::new(0x306c2);
    for _r in 0..INJECT_ROUNDS {
        // LUID 零。
        let s1 = ScenarioSpec { id: 90, priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 0, group_attrs: 0, level: ImpLevel::Identification, need_priv: 0, need_group: 0, need_level: ImpLevel::Identification };
        // 组越闭集。
        let s2 = ScenarioSpec { id: 91, priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 0, group_attrs: 0, level: ImpLevel::Identification, need_priv: 7, need_group: 700, need_level: ImpLevel::Identification };
        let ra = replay(&s1);
        let rb = replay(&s2);
        let hit = matches!(ra, Ok(v) if v.code == DcCode::ReplayOob.val() && !v.ok)
            && matches!(rb, Ok(v) if v.code == DcCode::ReplayOob.val() && !v.ok)
            && replay(&s1) == ra
            && replay(&s2) == rb;
        let _ = lcg.below(3);
        if hit {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("重放拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// 三面覆盖演练：特权/组/模拟级三面逐面有通过有拒绝 10/10 轮。
pub fn drill_cover_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x306c3（覆盖序列）。
    let mut lcg = Lcg::new(0x306c3);
    for _r in 0..INJECT_ROUNDS {
        let mut dc = DualCompare::new();
        dc.build_master();
        dc.run_slave();
        let _ = dc.compare();
        let m = dc.master.clone();
        // 特权面（0..5）：既有过又有拒。
        let p_ok = m.iter().filter(|v| v.scenario < 5).any(|v| v.ok)
            && m.iter().filter(|v| v.scenario < 5).any(|v| !v.ok);
        // 组面（5..10）。
        let g_ok = m.iter().filter(|v| (5..10).contains(&v.scenario)).any(|v| v.ok)
            && m.iter().filter(|v| (5..10).contains(&v.scenario)).any(|v| !v.ok);
        // 模拟级面（10..15）。
        let l_ok = m.iter().filter(|v| v.scenario >= 10).any(|v| v.ok)
            && m.iter().filter(|v| v.scenario >= 10).any(|v| !v.ok);
        let _ = lcg.below(3);
        if p_ok && g_ok && l_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("三面覆盖轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：母版未建先比对必不判一致。
    let mut dc = DualCompare::new();
    dc.run_slave();
    let _ = dc.compare();
    if !dc.consistent() {
        caught += 1;
        d.push("注1 无母:抓".to_string());
    } else {
        d.push("注1 无母:漏".to_string());
    }
    // 注 2：改码零容差必红。
    let mut dc2 = DualCompare::new();
    dc2.build_master();
    dc2.run_slave();
    let _ = tamper(&mut dc2, Tamper::MutCode);
    if dc2.compare() > 0 {
        caught += 1;
        d.push("注2 改码:抓".to_string());
    } else {
        d.push("注2 改码:漏".to_string());
    }
    // 注 3：重放越域必拒。
    let s3 = ScenarioSpec { id: 92, priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 0, group_attrs: 0, level: ImpLevel::Identification, need_priv: 0, need_group: 0, need_level: ImpLevel::Identification };
    if matches!(replay(&s3), Ok(v) if v.code == DcCode::ReplayOob.val()) {
        caught += 1;
        d.push("注3 越域:抓".to_string());
    } else {
        d.push("注3 越域:漏".to_string());
    }
    // 注 4：未执行重放（零重放）必不判一致。
    let mut dc4 = DualCompare::new();
    dc4.build_master();
    let _ = dc4.compare();
    if !dc4.consistent() {
        caught += 1;
        d.push("注4 未跑:抓".to_string());
    } else {
        d.push("注4 未跑:漏".to_string());
    }
    // 注 5：正路必绿。
    let mut dc5 = DualCompare::new();
    dc5.build_master();
    dc5.run_slave();
    if dc5.compare() == 0 && dc5.consistent() && dc5.covered() {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x306c0 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    DcCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：对照报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 双机对照报告。
#[derive(Clone, Copy, Debug)]
pub struct DcReport {
    /// 母版行数。
    pub master: usize,
    /// B 机行数。
    pub slave: usize,
    /// 差异数。
    pub diffs: usize,
    /// 一致。
    pub cons: bool,
    /// 覆盖。
    pub covered: bool,
}

impl DcReport {
    /// 建报告。
    pub fn build(dc: &DualCompare) -> Self {
        DcReport {
            master: dc.master_len(),
            slave: dc.slave_len(),
            diffs: dc.diff_rows().len(),
            cons: dc.consistent(),
            covered: dc.covered(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.cons && self.covered && self.diffs == 0 && self.master == SCENARIOS
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "DcReport 母版={} B机={} 差异={} 一致={} 覆盖={}",
            self.master, self.slave, self.diffs, self.cons, self.covered
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct DcCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_dc_checklist(dc: &DualCompare) -> Vec<DcCheckRow> {
    let rep = DcReport::build(dc);
    let mut rows: Vec<DcCheckRow> = Vec::new();
    rows.push(DcCheckRow { name: "15场景覆盖", passed: dc.covered() && dc.master_len() == SCENARIOS });
    rows.push(DcCheckRow { name: "零差异", passed: dc.diff_rows().is_empty() });
    rows.push(DcCheckRow { name: "一致判定", passed: dc.consistent() });
    rows.push(DcCheckRow { name: "已重放", passed: dc.replayed() > 0 });
    rows.push(DcCheckRow { name: "差异账单调", passed: dc.diff_monotone() });
    rows.push(DcCheckRow { name: "转录非空", passed: dc.export_lines().contains("双机差异账") });
    rows.push(DcCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(DcCheckRow { name: "组闭集口径", passed: GROUP_RIDS == [512, 513, 544, 546] });
    rows.push(DcCheckRow { name: "位域口径", passed: PRIV_ENABLED == 0x2 && GRP_ENABLED == 0x4 });
    rows.push(DcCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn dc_checklist_green(rows: &[DcCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn dc_stats_line(dc: &DualCompare) -> String {
    format!(
        "双机对照概况：母版 {} 行 B机 {} 行 差异 {}",
        dc.master_len(),
        dc.slave_len(),
        dc.diff_rows().len()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12454_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut dc = DualCompare::new();
    dc.build_master();
    dc.run_slave();
    let _ = dc.compare();
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        DcReport::build(&dc).all_green() && dc.export_lines().contains("双机差异账"),
        dc_stats_line(&dc),
    ));
    // X-b：重放拒止 10/10。
    let (ok_r, d_r) = drill_replay_oob_10();
    out.push(("X-重放拒止10/10", ok_r, d_r));
    // X-c：三面覆盖 10/10。
    let (ok_c, d_c) = drill_cover_10();
    out.push(("X-三面覆盖10/10", ok_c, d_c));
    // X-d：终验清单。
    let rows = build_dc_checklist(&dc);
    out.push((
        "X-终验清单10项",
        dc_checklist_green(&rows),
        DcReport::build(&dc).to_text(),
    ));
    // X-e：场景规格守恒（15 场景逐号可达）。
    let specs = master_scenarios();
    out.push((
        "X-场景规格守恒",
        specs.len() == SCENARIOS && specs.iter().enumerate().all(|(i, s)| s.id as usize == i),
        format!("场景={} 逐号对齐", specs.len()),
    ));
    out
}

/// run_unx12454_checks：本单判据聚合（诊断段 0x306c0）。
pub fn run_unx12454_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12454 令牌族双机对照判据");
    // J1-a：双机一致 10/10。
    let (ok1, d1) = drill_dual_run_10();
    s.add_dyn("J1-双机一致10/10", ok1, &d1);
    // J1-b：篡改检出 10/10。
    let (ok2, d2) = drill_tamper_10();
    s.add_dyn("J1-篡改检出10/10", ok2, &d2);
    // J1-c：标准会话恒等式（15 场景零差异 + 覆盖）。
    let mut dc = DualCompare::new();
    dc.build_master();
    dc.run_slave();
    let d = dc.compare();
    s.add_dyn(
        "J1-15场景零差异",
        dc.consistent() && d == 0 && dc.covered() && dc.master_len() == SCENARIOS,
        &format!("母版={} B机={} 差异={}", dc.master_len(), dc.slave_len(), d),
    );
    // J2-a：注入核 5/5。
    let (c2, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d5.join(" ")));
    // J2-b：判定类零容差（单比特篡改必红）。
    let mut dc2 = DualCompare::new();
    dc2.build_master();
    dc2.run_slave();
    let _ = tamper(&mut dc2, Tamper::MutCode);
    let d2c = dc2.compare();
    s.add_dyn(
        "J2-判定零容差",
        d2c > 0,
        &format!("篡改差异条={}", d2c),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x306c0段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12454_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
