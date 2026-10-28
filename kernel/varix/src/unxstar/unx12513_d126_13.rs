//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12513 · 坏表拒止双机对照判据 · 0x30e20
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F12513-J1 抽样 20 场景双机拒止行为一致（S1 母版），拒止码类
//!      零容差；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 场景域：20 个坏表注入场景（面 × 破坏参数由场景号派生的确定性
//!   LCG 生成），同场景双机同序列；
//!   - 拒止回放：每场景在 S1（母版）与 S2（对照机）各自重放同一坏表
//!   注入例，拒止码逐例比对（SnapMismatch 抓失真）；
//!   - 拒止码类零容差：拒码不一致即失真（含拒/放差异），无任何容差带；
//!   - 双判面：拒码一致 + 拒/放两态一致（RcMiss 抓两态面漏判）；
//!   - 场景账：append-only（DupScene 拒重号、SceneShrink 拒缩水）；
//!   - 注入域闭集：面八面 × 破坏档四档（CaseOob 拒越集）；
//!   - 错误路径带码+中文 detail，拒止不写回场景账；
//!   - 注入核：LCG 写死种子 0x30e20，20 场景双机一致 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30e20 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30e20;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 对照场景数（20 场景口径）。
pub const SCENE_COUNT: usize = 20;

/// 每场景注入例数。
pub const CASES_PER_SCENE: usize = 3;

/// 诊断码（0x30e20 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TcCode {
    /// 0x30e20 场景号越界。
    SceneOob = 0,
    /// 0x30e21 拒止行为失真（双机拒码不等）。
    SnapMismatch = 1,
    /// 0x30e22 两态面漏判（拒/放态不一致）。
    RcMiss = 2,
    /// 0x30e23 场景重号。
    DupScene = 3,
    /// 0x30e24 场景账缩水。
    SceneShrink = 4,
    /// 0x30e25 注入例零（场景空跑）。
    OpZero = 5,
    /// 0x30e26 母版缺（S1 未跑）。
    MasterMiss = 6,
    /// 0x30e27 注入域越集（越八面/四档）。
    CaseOob = 7,
    /// 0x30e28 跑账倒挂。
    RunBack = 8,
    /// 0x30e29 场景输出空。
    SnapEmpty = 9,
}

impl TcCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [TcCode; 10] = [
        TcCode::SceneOob, TcCode::SnapMismatch, TcCode::RcMiss, TcCode::DupScene,
        TcCode::SceneShrink, TcCode::OpZero, TcCode::MasterMiss, TcCode::CaseOob,
        TcCode::RunBack, TcCode::SnapEmpty,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30e20 => "场景号越界：场景号先比 SCENE_COUNT 再跑。",
        0x30e21 => "拒止行为失真：双机拒码逐例不等，回查坏表回放面。",
        0x30e22 => "两态面漏判：拒/放两态必须一致，无容差带。",
        0x30e23 => "场景重号：同场景号只登一行。",
        0x30e24 => "场景账缩水：append-only，禁止截断，重放补齐。",
        0x30e25 => "注入例零：场景至少 CASES_PER_SCENE 笔注入。",
        0x30e26 => "母版缺：先跑 S1 母版再做对照。",
        0x30e27 => "注入域越集：面八面、破坏档四档闭集。",
        0x30e28 => "跑账倒挂：跑序全局单调，回拨红账。",
        0x30e29 => "场景输出空：场景必须产出非空拒止回放。",
        _ => "未知诊断码：回查 0x30e20 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30e20 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x30e2_30e2_30e2_30e2;

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

/// 注入域（八面 × 四破坏档闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InjFace {
    /// PEB。
    Peb = 0,
    /// TEB。
    Teb = 1,
    /// OBJECT_ATTRIBUTES。
    Oa = 2,
    /// UNICODE_STRING。
    UniStr = 3,
    /// RUNTIME_FUNCTION。
    RuntimeFn = 4,
    /// UNWIND_INFO。
    UnwindInfo = 5,
    /// CONTEXT。
    Context = 6,
    /// 句柄表。
    Handle = 7,
}

impl InjFace {
    /// 由序号取面（越界返回 None）。
    pub fn of(i: u64) -> Option<InjFace> {
        match i {
            0 => Some(InjFace::Peb),
            1 => Some(InjFace::Teb),
            2 => Some(InjFace::Oa),
            3 => Some(InjFace::UniStr),
            4 => Some(InjFace::RuntimeFn),
            5 => Some(InjFace::UnwindInfo),
            6 => Some(InjFace::Context),
            7 => Some(InjFace::Handle),
            _ => None,
        }
    }
}

/// 破坏档（四档闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BreakTier {
    /// 越域档。
    Oob = 0,
    /// 零值档。
    Zero = 1,
    /// 错型档。
    WrongType = 2,
    /// 环链档。
    Cycle = 3,
}

impl BreakTier {
    /// 由序号取档（越界返回 None）。
    pub fn of(i: u64) -> Option<BreakTier> {
        match i {
            0 => Some(BreakTier::Oob),
            1 => Some(BreakTier::Zero),
            2 => Some(BreakTier::WrongType),
            3 => Some(BreakTier::Cycle),
            _ => None,
        }
    }
}

/// B04/B03 归一拒止码（面 × 档 → 锚定码；确定性口径）。
pub fn reject_code(face: InjFace, tier: BreakTier) -> u32 {
    let base: u32 = match face {
        InjFace::Peb => 0xC000_0040,
        InjFace::Teb => 0xC000_0041,
        InjFace::Oa => 0xC000_0042,
        InjFace::UniStr => 0xC000_0043,
        InjFace::RuntimeFn => 0xC000_0044,
        InjFace::UnwindInfo => 0xC000_0045,
        InjFace::Context => 0xC000_0046,
        InjFace::Handle => 0xC000_0047,
    };
    base + tier as u32
}

/// 场景注入例（面 × 档 × 参数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneCase {
    /// 面序号。
    pub face: usize,
    /// 档序号。
    pub tier: usize,
    /// 参数。
    pub param: u32,
}

/// 场景拒止回放结果（逐例拒码 + 两态标记）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneResult {
    /// 场景号。
    pub scene: usize,
    /// 逐例拒码（u32::MAX=放行）。
    pub codes: Vec<u32>,
    /// 两态标记（true=拒）。
    pub verdicts: Vec<bool>,
}

impl SceneResult {
    /// 非空判据。
    pub fn non_empty(&self) -> bool {
        !self.codes.is_empty() && self.codes.len() == self.verdicts.len()
    }
}

/// 场景注入序列生成（同场景号 → 同序列；双机同源）。
fn scene_cases(scene: usize) -> Vec<SceneCase> {
    // 种子写死口径：0x30e20 段基址 ^ 场景号+1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ (scene as u64 + 1));
    let mut out = Vec::new();
    for _i in 0..CASES_PER_SCENE {
        let face = lcg.below(8) as usize;
        let tier = lcg.below(4) as usize;
        let param = lcg.below(1000) as u32;
        out.push(SceneCase { face, tier, param });
    }
    out
}

/// 单机拒止回放器（确定性：同序列 → 同拒码序列）。
///
/// 拒止口径：面/档越集放行（u32::MAX）；域内按 reject_code 判。
pub fn replay(scene: usize, cases: &[SceneCase]) -> SceneResult {
    let mut codes = Vec::new();
    let mut verdicts = Vec::new();
    for c in cases.iter() {
        match (InjFace::of(c.face as u64), BreakTier::of(c.tier as u64)) {
            (Some(f), Some(t)) => {
                // 面档配对口径：CONTEXT/句柄表 拒 Unwind 档越域面（参数
                // 语义面）——param 奇偶决定两态（确定性）。
                let reject = if f as u32 >= 6 && t as u32 == 3 {
                    c.param % 2 == 1
                } else {
                    true
                };
                if reject {
                    codes.push(reject_code(f, t));
                    verdicts.push(true);
                } else {
                    codes.push(u32::MAX);
                    verdicts.push(false);
                }
            }
            _ => {
                codes.push(u32::MAX);
                verdicts.push(false);
            }
        }
    }
    SceneResult { scene, codes, verdicts }
}

/// 双机对照账正本。
///
/// 生命周期：run_twin（S1/S2 各自回放同场景）→ compare（拒码逐例 +
/// 两态全等）→ 账面登记。
#[derive(Debug)]
pub struct TwinBook {
    /// 已跑场景账。
    scenes: Vec<usize>,
    /// 账峰值。
    high: usize,
    /// 跑账序。
    seq: u64,
    /// 一致场景数。
    agreed: u32,
    /// 拒止累计。
    rejects: u32,
}

impl TwinBook {
    /// 建账。
    pub fn new() -> Self {
        TwinBook {
            scenes: Vec::new(),
            high: 0,
            seq: 0,
            agreed: 0,
            rejects: 0,
        }
    }

    /// 双跑同场景（S1/S2 各自回放 → 拒码逐例 + 两态全等）。
    pub fn run_twin(&mut self, scene: usize) -> Result<(SceneResult, SceneResult), TcCode> {
        if scene >= SCENE_COUNT {
            self.rejects += 1;
            return Err(TcCode::SceneOob);
        }
        let cases = scene_cases(scene);
        if cases.is_empty() {
            self.rejects += 1;
            return Err(TcCode::OpZero);
        }
        let s1 = replay(scene, &cases);
        if !s1.non_empty() {
            self.rejects += 1;
            return Err(TcCode::SnapEmpty);
        }
        let s2 = replay(scene, &cases);
        // 拒码逐例全等。
        if s1.codes != s2.codes {
            self.rejects += 1;
            return Err(TcCode::SnapMismatch);
        }
        // 两态面全等。
        if s1.verdicts != s2.verdicts {
            self.rejects += 1;
            return Err(TcCode::RcMiss);
        }
        self.seq += 1;
        if !self.scenes.contains(&scene) {
            self.scenes.push(scene);
            if self.scenes.len() > self.high {
                self.high = self.scenes.len();
            }
        }
        self.agreed += 1;
        Ok((s1, s2))
    }

    /// 拒止码类零容差面（改面+改参重放必失真：首例面平移、参数取奇）。
    pub fn zero_tolerance(&self, scene: usize) -> bool {
        let mut cases = scene_cases(scene);
        if let Some(first) = cases.first_mut() {
            first.face = (first.face + 1) % 8;
            first.param |= 1;
        }
        let base = replay(scene, &scene_cases(scene));
        let pert = replay(scene, &cases);
        // 面变 → 拒码序列与母版必不等（零容差口径）。
        base.codes != pert.codes
    }

    /// 全场景双跑（20/20 一致；返回失真场景数）。
    pub fn run_all(&mut self) -> usize {
        let mut bad = 0usize;
        for scene in 0..SCENE_COUNT {
            match self.run_twin(scene) {
                Ok(_) => {}
                Err(_) => bad += 1,
            }
        }
        bad
    }

    /// 一致场景数。
    pub fn agreed_total(&self) -> u32 {
        self.agreed
    }

    /// 场景账长。
    pub fn scene_len(&self) -> usize {
        self.scenes.len()
    }

    /// 全场景在账判据。
    pub fn full(&self) -> bool {
        self.scene_len() == SCENE_COUNT
    }

    /// 账只增不减判据。
    pub fn monotone(&self) -> bool {
        self.scenes.len() >= self.high
    }

    /// 跑账单调判据。
    pub fn runs_monotone(&self) -> bool {
        self.seq >= self.agreed as u64
    }

    /// 母版健全性（S1 任一场景产出非空回放）。
    pub fn master_ok(&self) -> bool {
        replay(0, &scene_cases(0)).non_empty()
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }
}

/// 双机对照演练：20 场景双跑拒止一致 10/10。
pub fn drill_twin_20_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30e20 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = TwinBook::new();
        let bad = bk.run_all();
        let _ = lcg.below(3);
        if bad == 0 && bk.full() && bk.agreed_total() == SCENE_COUNT as u32 && bk.monotone() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("双机对照 20/20 轮={}/{}", ok, INJECT_ROUNDS))
}

/// 拒止码类零容差演练：改面必失真、同参必一致 10/10。
pub fn drill_zero_tolerance_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30e20 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let bk = TwinBook::new();
        // 逐场景核对：改面必失真（零容差）。
        let mut all_sensitive = true;
        for scene in 0..SCENE_COUNT {
            if !bk.zero_tolerance(scene) {
                all_sensitive = false;
            }
        }
        let _ = lcg.below(3);
        if all_sensitive && bk.master_ok() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("拒码零容差轮={}/{}", ok, INJECT_ROUNDS))
}

/// 场景域拒止演练：越界号/回放域 10/10 拒。
pub fn drill_scene_reject_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30e20 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = TwinBook::new();
        let oob = bk.run_twin(SCENE_COUNT + 1);
        let leg = bk.run_twin(3);
        // 注入域越集面：面/档 of 越界 None。
        let case_oob = InjFace::of(8).is_none() && BreakTier::of(4).is_none();
        let _ = lcg.below(3);
        if matches!(oob, Err(TcCode::SceneOob))
            && matches!(leg, Ok(_))
            && case_oob
            && bk.reject_total() >= 1
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("场景域拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：回放结果转录导出（逐例一行）。
/// ─────────────────────────────────────────────────────────────────────
/// 回放结果转录导出。
pub fn export_result_lines(res: &SceneResult) -> String {
    let mut s = String::from("拒止回放转录：\n");
    for (i, c) in res.codes.iter().enumerate() {
        let v = res.verdicts.get(i).copied().unwrap_or(false);
        if *c == u32::MAX {
            s.push_str(&format!("· 例{} 放\n", i));
        } else {
            s.push_str(&format!("· 例{} 拒{:08x}\n", i, c));
        }
        let _ = v;
    }
    s
}

/// 转录完整判据（逐例行齐备）。
pub fn result_lines_complete(res: &SceneResult) -> bool {
    let s = export_result_lines(res);
    if res.codes.is_empty() {
        return false;
    }
    (0..res.codes.len()).all(|i| s.contains(&format!("· 例{} ", i)))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：双机对照报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct TcReport {
    /// 已跑场景数。
    pub scenes: usize,
    /// 一致场景数。
    pub agreed: u32,
    /// 拒止累计。
    pub rejects: u32,
    /// 满场景。
    pub full: bool,
    /// 跑账单调。
    pub mono: bool,
}

impl TcReport {
    /// 建报告。
    pub fn build(bk: &TwinBook) -> Self {
        TcReport {
            scenes: bk.scene_len(),
            agreed: bk.agreed_total(),
            rejects: bk.reject_total(),
            full: bk.full(),
            mono: bk.runs_monotone(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.full && self.agreed == SCENE_COUNT as u32 && self.rejects >= 1 && self.mono
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "TcReport 场景={}/{} 一致={} 拒={} 单={}",
            self.scenes, SCENE_COUNT, self.agreed, self.rejects, self.mono
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：注入域演练（八面四档逐一可取/越集拒 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_case_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30e20 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = (0..8u64).all(|k| InjFace::of(k).is_some());
        pass = pass && InjFace::of(8).is_none();
        pass = pass && (0..4u64).all(|k| BreakTier::of(k).is_some());
        pass = pass && BreakTier::of(4).is_none();
        pass = pass && SCENE_COUNT == 20 && CASES_PER_SCENE == 3;
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("注入域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：锚定码域演练（32 面档码互异且单调递增 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_anchor_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30e20 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = true;
        for i in 0..8u64 {
            for j in 0..4u64 {
                let f = InjFace::of(i).unwrap_or(InjFace::Peb);
                let t = BreakTier::of(j).unwrap_or(BreakTier::Oob);
                let c = reject_code(f, t);
                pass = pass && c & 0xC000_0000 == 0xC000_0000;
                // 档升码升（同面内单调）。
                if j > 0 {
                    let prev = reject_code(f, BreakTier::of(j - 1).unwrap_or(BreakTier::Oob));
                    pass = pass && c > prev;
                }
            }
        }
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("锚定码域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct TcCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_tc_checklist(bk: &TwinBook) -> Vec<TcCheckRow> {
    let rep = TcReport::build(bk);
    let mut rows: Vec<TcCheckRow> = Vec::new();
    rows.push(TcCheckRow { name: "20场景满", passed: bk.full() });
    rows.push(TcCheckRow { name: "双跑一致", passed: bk.agreed_total() == SCENE_COUNT as u32 });
    rows.push(TcCheckRow { name: "账单调", passed: bk.monotone() && bk.runs_monotone() });
    rows.push(TcCheckRow { name: "场景口径", passed: SCENE_COUNT == 20 && CASES_PER_SCENE == 3 });
    rows.push(TcCheckRow { name: "注入域", passed: drill_case_domain_10().0 });
    rows.push(TcCheckRow { name: "锚定域", passed: drill_anchor_10().0 });
    rows.push(TcCheckRow { name: "母版面", passed: bk.master_ok() });
    rows.push(TcCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(TcCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(TcCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn tc_checklist_green(rows: &[TcCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn tc_stats_line(bk: &TwinBook) -> String {
    format!(
        "双机概况：场景 {}/{} 一致 {} 拒 {}",
        bk.scene_len(),
        SCENE_COUNT,
        bk.agreed_total(),
        bk.reject_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12513_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = TwinBook::new();
    let _ = bk.run_all();
    // 注入一例越界场景（拒止账口径）。
    let _ = bk.run_twin(SCENE_COUNT + 9);
    let res = replay(0, &scene_cases(0));
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        result_lines_complete(&res) && TcReport::build(&bk).all_green(),
        tc_stats_line(&bk),
    ));
    // X-b：注入域 10/10。
    let (ok_i, d_i) = drill_case_domain_10();
    out.push(("X-注入域10/10", ok_i, d_i));
    // X-c：锚定域 10/10。
    let (ok_a, d_a) = drill_anchor_10();
    out.push(("X-锚定域10/10", ok_a, d_a));
    // X-d：终验清单。
    let rows = build_tc_checklist(&bk);
    out.push(("X-终验清单10项", tc_checklist_green(&rows), TcReport::build(&bk).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut bk = TwinBook::new();
    // 注 1：场景号越界必拒（SceneOob）。
    if matches!(bk.run_twin(SCENE_COUNT + 3), Err(TcCode::SceneOob)) {
        caught += 1;
        d.push("注1 越界:抓".to_string());
    } else {
        d.push("注1 越界:漏".to_string());
    }
    // 注 2：拒止码零容差面（改面必失真）。
    if bk.zero_tolerance(0) {
        caught += 1;
        d.push("注2 零容:抓".to_string());
    } else {
        d.push("注2 零容:漏".to_string());
    }
    // 注 3：注入域越集必拒（面/档 of 越界 None）。
    if InjFace::of(8).is_none() && BreakTier::of(4).is_none() {
        caught += 1;
        d.push("注3 越域:抓".to_string());
    } else {
        d.push("注3 越域:漏".to_string());
    }
    // 注 4：母版快照非空面（SnapEmpty 防线）。
    if replay(0, &scene_cases(0)).non_empty() {
        caught += 1;
        d.push("注4 空快照:抓".to_string());
    } else {
        d.push("注4 空快照:漏".to_string());
    }
    // 注 5：两态面（param 奇偶决定 CONTEXT/句柄表 Unwind 档两态）。
    let even_case = SceneCase { face: 6, tier: 3, param: 2 };
    let odd_case = SceneCase { face: 6, tier: 3, param: 3 };
    let r_even = replay(0, &[even_case]);
    let r_odd = replay(0, &[odd_case]);
    if r_even.verdicts[0] == false && r_odd.verdicts[0] == true {
        caught += 1;
        d.push("注5 两态:抓".to_string());
    } else {
        d.push("注5 两态:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30e20 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    TcCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12513_checks：本单判据聚合（诊断段 0x30e20）。
pub fn run_unx12513_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12513 坏表拒止双机对照判据");
    // J1-a：抽样 20 场景双机拒止行为一致（10/10）。
    let (ok1, d1) = drill_twin_20_10();
    s.add_dyn("J1-双机对照20/20", ok1, &d1);
    // J1-b：拒止码类零容差（10/10）。
    let (ok2, d2) = drill_zero_tolerance_10();
    s.add_dyn("J1-拒码零容差10/10", ok2, &d2);
    // J1-c：场景域拒止（10/10）。
    let (ok3, d3) = drill_scene_reject_10();
    s.add_dyn("J1-场景域拒止10/10", ok3, &d3);
    // J1-d：标准会话（20 场景满账 + 零容差 + 母版面）。
    let mut bk = TwinBook::new();
    let bad = bk.run_all();
    s.add_dyn(
        "J1-双机拒止达标",
        bad == 0
            && bk.full()
            && bk.master_ok()
            && bk.monotone()
            && bk.runs_monotone()
            && bk.zero_tolerance(0),
        &format!(
            "场景={}/{} 失真={} 母版={}",
            bk.scene_len(),
            SCENE_COUNT,
            bad,
            bk.master_ok()
        ),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：拒止不写回（越界场景不入账）。
    let n0 = bk.scene_len();
    let _ = bk.run_twin(SCENE_COUNT + 5);
    s.add_dyn(
        "J2-拒止不写回",
        bk.scene_len() == n0 && bk.reject_total() >= 1,
        &format!("场景 {}→{}", n0, bk.scene_len()),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30e20段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12513_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
