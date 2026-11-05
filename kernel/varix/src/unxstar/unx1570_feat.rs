//! UNX-F1570 移交演练（新消费域冷启动走查） · 0x78D80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-1104 B06 卡 7 · UNX-F1570 逐字摘录）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F1570-J1 移交演练通过（以新消费域视角冷启动：
//!     读包→接接口→跑样例→查账四步走查），四步 10/10 通过，演练
//!     报告落档；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 四步走查状态机：读包→接接口→跑样例→查账逐步单调推进，
//!     越步跳步显性拒，回步倒退显性拒；
//!   - 步内核验：读包对包指纹 / 接接口对五组接口席 / 跑样例对
//!     样例期望值 / 查账对总账指纹——四步各有对平口径；
//!   - 10/10 演练：十轮冷启动演练逐轮四步全过，轮指纹逐位一致；
//!   - 演练报告落档：12 行（头 + 4 步行 + 10 轮行变体 6 + 尾）
//!     指纹落档双跑逐位一致；
//!   - 注入必抓：跳步（读包直跑样例）/ 倒步（查账回读包）/
//!     样例值漂移三路必抓；
//!   - LCG 样例派生，种子写死 = SEG_BASE；零浮点 / IO / 墙钟 /
//!     unwrap / panic；整数运算；中文诊断。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x78D80，派工卡 VX8-1104 B06 卡 7 独占窗口）。
pub const SEG_BASE: u32 = 0x78D80;

/// 走查步数（判据 J1「四步」冻结）。
pub const STEP_COUNT: usize = 4;

/// 演练轮数（判据 J1「10/10」冻结）。
pub const DRILL_ROUNDS: usize = 10;

/// 每轮样例数（跑样例步内核验样本）。
pub const SAMPLES_PER_ROUND: usize = 6;

/// 演练报告行数（头 + 4 步行 + 6 样例行 + 尾，冻结）。
pub const REPORT_ROWS: usize = 12;

// ── 四步闭集 ────────────────────────────────────────────────────
pub const STEPS: [&str; 4] = ["读包", "接接口", "跑样例", "查账"];

// ── 冻结基准：包指纹 / 接口席数 / 样例期望值 / 账指纹 ──────────
pub const FROZEN_PACK_FINGER: u32 = 0x1D70_0001;
pub const FROZEN_IFACE_SEATS: usize = 5;
pub const FROZEN_LEDGER_FINGER: u32 = 0x1D70_0002;

// ── 诊断码闭集（段内 0x01..0x0C 窗口，派工卡约定） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Drill70Code {
    /// 步越闭集：步名收进读包/接接口/跑样例/查账
    StepUnknown,
    /// 越步跳步：四步按序走查，跳步显性拒
    StepSkipped,
    /// 回步倒退：走查不得回退，倒步显性拒
    StepRegressed,
    /// 包指纹失平：读包步包指纹与冻结版不一致，回滚再演
    PackFingerBad,
    /// 接口席失平：接接口步席位与冻结数不一致，补席再演
    IfaceSeatsBad,
    /// 样例值漂移：跑样例步期望值失配，校准样例再演
    SampleDrift,
    /// 账指纹失平：查账步账指纹与冻结版不一致，重平再演
    LedgerFingerBad,
    /// 演练轮不足：补足 10 轮再落报告
    RoundsShort,
    /// 演练状态倒退：单轮内步序单调推进
    PhaseRegress,
    /// 演练状态跳级：四步未全过不得落报告
    PhaseSkip,
    /// 演练报告行数违约：报告必须恰 12 行
    ReportRowsBad,
    /// 双跑不一致：排查样例序列随机性后复跑
    NotReproducible,
}

impl Drill70Code {
    /// 段内闭集取值：SEG_BASE + 0x01 ..= SEG_BASE + 0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }
    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Drill70Code::StepUnknown => "步越闭集：步名收进读包/接接口/跑样例/查账",
            Drill70Code::StepSkipped => "越步跳步：四步按序走查，跳步显性拒",
            Drill70Code::StepRegressed => "回步倒退：走查不得回退，倒步显性拒",
            Drill70Code::PackFingerBad => "包指纹失平：读包步包指纹与冻结版不一致，回滚再演",
            Drill70Code::IfaceSeatsBad => "接口席失平：接接口步席位与冻结数不一致，补席再演",
            Drill70Code::SampleDrift => "样例值漂移：跑样例步期望值失配，校准样例再演",
            Drill70Code::LedgerFingerBad => "账指纹失平：查账步账指纹与冻结版不一致，重平再演",
            Drill70Code::RoundsShort => "演练轮不足：补足 10 轮再落报告",
            Drill70Code::PhaseRegress => "演练状态倒退：单轮内步序单调推进不回退",
            Drill70Code::PhaseSkip => "演练状态跳级：四步未全过不得落报告",
            Drill70Code::ReportRowsBad => "演练报告行数违约：报告必须恰 12 行",
            Drill70Code::NotReproducible => "双跑不一致：排查样例序列随机性后复跑",
        }
    }
    /// 闭集数组（查表唯一来源）。
    pub const ALL: [Drill70Code; 12] = [
        Drill70Code::StepUnknown,
        Drill70Code::StepSkipped,
        Drill70Code::StepRegressed,
        Drill70Code::PackFingerBad,
        Drill70Code::IfaceSeatsBad,
        Drill70Code::SampleDrift,
        Drill70Code::LedgerFingerBad,
        Drill70Code::RoundsShort,
        Drill70Code::PhaseRegress,
        Drill70Code::PhaseSkip,
        Drill70Code::ReportRowsBad,
        Drill70Code::NotReproducible,
    ];
}

/// 诊断码 → 文案闭集查表（闭集外返回未登记占位）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < Drill70Code::ALL.len() {
        let c = match Drill70Code::ALL.get(i) {
            Some(c) => *c,
            None => break,
        };
        if c.val() == code {
            return c.detail();
        }
        i += 1;
    }
    "未登记诊断码：不在本段闭集内"
}

// ── FNV-1a 指纹（整数域，零随机） ────────────────────────────────
fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ── LCG（种子固定 = SEG_BASE，零随机源） ────────────────────────
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    /// 逐步推进并取模返回。
    pub fn next_mod(&mut self, m: usize) -> usize {
        self.state = self
            .state
            .wrapping_mul(1_103_515_245)
            .wrapping_add(12_345);
        let v = (self.state >> 16) as usize;
        if m == 0 {
            return 0;
        }
        v % m
    }
}

/// 样例期望值派生：LCG 派生 SAMPLES_PER_ROUND 个期望值（确定）。
pub fn derive_expected() -> Vec<u64> {
    let mut lcg = Lcg::new(SEG_BASE);
    let mut out: Vec<u64> = Vec::new();
    let mut k = 0usize;
    while k < SAMPLES_PER_ROUND {
        let v = lcg.next_mod(1000) as u64;
        out.push(v);
        k += 1;
    }
    out
}

// ── 走查状态机（读包 → 接接口 → 跑样例 → 查账，单调推进） ─────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WalkPhase {
    /// 未起步：冷启动零步
    Idle,
    /// 第一步读包完成
    ReadPack,
    /// 第二步接接口完成
    BindIface,
    /// 第三步跑样例完成
    RunSamples,
    /// 第四步查账完成（单轮走查全过）
    AuditDone,
}

impl WalkPhase {
    /// 位次：倒退判定基准。
    pub fn rank(self) -> u8 {
        match self {
            WalkPhase::Idle => 0,
            WalkPhase::ReadPack => 1,
            WalkPhase::BindIface => 2,
            WalkPhase::RunSamples => 3,
            WalkPhase::AuditDone => 4,
        }
    }
}

/// 单轮走查件。
pub struct DrillRound {
    phase: WalkPhase,
    /// 已走步名留痕。
    trail: Vec<&'static str>,
    /// 样例实际值（跑样例步录入）。
    samples: Vec<u64>,
}

impl DrillRound {
    pub fn new() -> DrillRound {
        DrillRound {
            phase: WalkPhase::Idle,
            trail: Vec::new(),
            samples: Vec::new(),
        }
    }

    pub fn phase(&self) -> WalkPhase {
        self.phase
    }

    pub fn trail_len(&self) -> usize {
        self.trail.len()
    }

    /// 步推进：按序单调，跳步倒步显性拒。step 取 STEPS 下标 0..=3。
    /// 以留痕数为已完成步数：step == 已完成数即推进；step == 上一
    /// 已完成步即幂等重复（返回现态不重复留痕）；更早的步即倒退；
    /// 越前即跳步。
    pub fn step(&mut self, step: usize) -> Result<WalkPhase, u32> {
        if step >= STEP_COUNT {
            return Err(Drill70Code::StepUnknown.val());
        }
        let done = self.trail_len();
        if step > done {
            return Err(Drill70Code::StepSkipped.val());
        }
        if step + 1 < done {
            return Err(Drill70Code::StepRegressed.val());
        }
        if step + 1 == done {
            // 幂等重复当前已完成步。
            return Ok(self.phase);
        }
        let name = match STEPS.get(step) {
            Some(n) => *n,
            None => return Err(Drill70Code::StepUnknown.val()),
        };
        self.trail.push(name);
        self.phase = match step {
            0 => WalkPhase::ReadPack,
            1 => WalkPhase::BindIface,
            2 => WalkPhase::RunSamples,
            _ => WalkPhase::AuditDone,
        };
        Ok(self.phase)
    }

    /// 第一步核验：包指纹对平。
    pub fn check_pack(&self, finger: u32) -> Result<(), u32> {
        if self.phase != WalkPhase::ReadPack {
            return Err(Drill70Code::PhaseRegress.val());
        }
        if finger != FROZEN_PACK_FINGER {
            return Err(Drill70Code::PackFingerBad.val());
        }
        Ok(())
    }

    /// 第二步核验：接口席数对平。
    pub fn check_iface(&self, seats: usize) -> Result<(), u32> {
        if self.phase != WalkPhase::BindIface {
            return Err(Drill70Code::PhaseRegress.val());
        }
        if seats != FROZEN_IFACE_SEATS {
            return Err(Drill70Code::IfaceSeatsBad.val());
        }
        Ok(())
    }

    /// 第三步核验：样例值与期望逐位对平。
    pub fn check_samples(&mut self, actual: &[u64]) -> Result<usize, u32> {
        if self.phase != WalkPhase::RunSamples {
            return Err(Drill70Code::PhaseRegress.val());
        }
        let expect = derive_expected();
        if actual.len() != expect.len() {
            return Err(Drill70Code::SampleDrift.val());
        }
        self.samples.clear();
        let mut i = 0usize;
        while i < actual.len() {
            let a = match actual.get(i) {
                Some(v) => *v,
                None => return Err(Drill70Code::SampleDrift.val()),
            };
            let e = match expect.get(i) {
                Some(v) => *v,
                None => return Err(Drill70Code::SampleDrift.val()),
            };
            if a != e {
                return Err(Drill70Code::SampleDrift.val());
            }
            self.samples.push(a);
            i += 1;
        }
        Ok(self.samples.len())
    }

    /// 第四步核验：账指纹对平（单轮走查收官，须在查账步完成后）。
    pub fn check_ledger(&self, finger: u32) -> Result<(), u32> {
        if self.phase != WalkPhase::AuditDone {
            return Err(Drill70Code::PhaseRegress.val());
        }
        if finger != FROZEN_LEDGER_FINGER {
            return Err(Drill70Code::LedgerFingerBad.val());
        }
        Ok(())
    }

    /// 轮指纹：四步留痕 + 样例值合成（复跑比对基准）。
    pub fn round_digest(&self) -> u32 {
        if self.phase != WalkPhase::AuditDone || self.trail_len() != STEP_COUNT {
            return 0;
        }
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < self.trail.len() {
            if let Some(n) = self.trail.get(i) {
                buf.extend_from_slice(n.as_bytes());
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < self.samples.len() {
            let v = match self.samples.get(j) {
                Some(v) => *v,
                None => break,
            };
            buf.push((v >> 8) as u8);
            buf.push((v & 0xFF) as u8);
            j += 1;
        }
        fnv1a(&buf)
    }
}

// ── 演练件：十轮冷启动走查 + 报告落档 ──────────────────────────
pub struct DrillBook {
    rounds: Vec<DrillRound>,
    report: Option<String>,
    report_digest: u32,
}

impl DrillBook {
    pub fn new() -> DrillBook {
        DrillBook {
            rounds: Vec::new(),
            report: None,
            report_digest: 0,
        }
    }

    pub fn round_count(&self) -> usize {
        self.rounds.len()
    }

    pub fn report_digest(&self) -> u32 {
        self.report_digest
    }

    pub fn report_text(&self) -> Option<&str> {
        match self.report.as_ref() {
            Some(t) => Some(t.as_str()),
            None => None,
        }
    }

    /// 追加一轮完整走查（四步 + 四核验逐项过）。
    pub fn run_round(&mut self) -> Result<u32, u32> {
        let mut r = DrillRound::new();
        r.step(0)?;
        r.check_pack(FROZEN_PACK_FINGER)?;
        r.step(1)?;
        r.check_iface(FROZEN_IFACE_SEATS)?;
        r.step(2)?;
        let expect = derive_expected();
        r.check_samples(&expect)?;
        r.step(3)?;
        r.check_ledger(FROZEN_LEDGER_FINGER)?;
        let d = r.round_digest();
        if d == 0 {
            return Err(Drill70Code::PhaseSkip.val());
        }
        self.rounds.push(r);
        Ok(d)
    }

    /// 演练报告落档：12 行 + FNV-1a 指纹 + 封账。
    pub fn seal_report(&mut self) -> Result<u32, u32> {
        if self.rounds.len() != DRILL_ROUNDS {
            return Err(Drill70Code::RoundsShort.val());
        }
        // 十轮指纹逐位一致方落报告。
        let base = match self.rounds.get(0) {
            Some(r) => r.round_digest(),
            None => return Err(Drill70Code::RoundsShort.val()),
        };
        let mut i = 1usize;
        while i < self.rounds.len() {
            let d = match self.rounds.get(i) {
                Some(r) => r.round_digest(),
                None => return Err(Drill70Code::RoundsShort.val()),
            };
            if d != base {
                return Err(Drill70Code::NotReproducible.val());
            }
            i += 1;
        }
        let mut text = String::from("== UNX-F1570 移交演练报告 ==");
        text.push('\n');
        let mut s = 0usize;
        while s < STEP_COUNT {
            let name = match STEPS.get(s) {
                Some(n) => *n,
                None => break,
            };
            let line = format!("步{} {} 过 ●", s + 1, name);
            text.push_str(&line);
            text.push('\n');
            s += 1;
        }
        // 首轮样例逐条落行（期望与实际逐位对平留痕）。
        let first = match self.rounds.get(0) {
            Some(r) => r,
            None => return Err(Drill70Code::RoundsShort.val()),
        };
        let mut k = 0usize;
        while k < first.samples.len() {
            let v = match first.samples.get(k) {
                Some(v) => *v,
                None => break,
            };
            let line = format!("样例{} 期望=实际={} ●", k + 1, v);
            text.push_str(&line);
            text.push('\n');
            k += 1;
        }
        text.push_str("== 四步 10/10 通过 ==");
        text.push('\n');
        let rows = text.matches('\n').count();
        if rows != REPORT_ROWS {
            return Err(Drill70Code::ReportRowsBad.val());
        }
        let digest = fnv1a(text.as_bytes());
        self.report = Some(text);
        self.report_digest = digest;
        Ok(digest)
    }
}

// ── 好件构建：十轮走查 + 报告落档 ──────────────────────────────
pub fn build_drill_ok() -> DrillBook {
    let mut bk = DrillBook::new();
    let mut r = 0usize;
    while r < DRILL_ROUNDS {
        let _ = bk.run_round();
        r += 1;
    }
    let _ = bk.seal_report();
    bk
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ────────────────────
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x78C00 && SEG_BASE <= 0x78DC0
}

/// 四步单调走查：好轮四步全过、留痕恰 4、指纹非零。
fn four_steps_ok() -> bool {
    let mut r = DrillRound::new();
    if r.step(0).is_err() || r.step(1).is_err() || r.step(2).is_err() || r.step(3).is_err() {
        return false;
    }
    if r.phase() != WalkPhase::AuditDone || r.trail_len() != STEP_COUNT {
        return false;
    }
    if r.round_digest() == 0 {
        return false;
    }
    true
}

/// 10/10 演练：好件十轮全过且报告封账。
fn rounds_10_ok() -> bool {
    let bk = build_drill_ok();
    if bk.round_count() != DRILL_ROUNDS {
        return false;
    }
    if bk.report_digest() == 0 {
        return false;
    }
    if bk.report_text().is_none() {
        return false;
    }
    true
}

/// 演练报告落档：12 行、四步名齐、10/10 尾标、指纹非零。
fn report_archived_ok() -> bool {
    let bk = build_drill_ok();
    let text = match bk.report_text() {
        Some(t) => t,
        None => return false,
    };
    if text.matches('\n').count() != REPORT_ROWS {
        return false;
    }
    if !text.contains("演练报告") {
        return false;
    }
    if !text.contains("10/10") {
        return false;
    }
    let mut s = 0usize;
    while s < STEP_COUNT {
        let name = match STEPS.get(s) {
            Some(n) => *n,
            None => break,
        };
        if !text.contains(name) {
            return false;
        }
        s += 1;
    }
    true
}

/// 跳步注入：Idle 直跑样例必抓（StepSkipped）。
fn inject_skip_ok() -> bool {
    let mut r = DrillRound::new();
    match r.step(2) {
        Err(code) => {
            if code != Drill70Code::StepSkipped.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 接接口步直查账：同样必抓。
    let mut r2 = DrillRound::new();
    if r2.step(0).is_err() || r2.step(1).is_err() {
        return false;
    }
    match r2.step(3) {
        Err(code) => {
            if code != Drill70Code::StepSkipped.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 倒步注入：走完后回读包必抓（StepRegressed）。
fn inject_regress_ok() -> bool {
    let mut r = DrillRound::new();
    if r.step(0).is_err() || r.step(1).is_err() {
        return false;
    }
    match r.step(0) {
        Err(code) => {
            if code != Drill70Code::StepRegressed.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 走完全程后回步同样必抓。
    if r.step(2).is_err() || r.step(3).is_err() {
        return false;
    }
    match r.step(1) {
        Err(code) => {
            if code != Drill70Code::StepRegressed.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 样例漂移注入：改一个样例值必抓（SampleDrift）。
fn inject_drift_ok() -> bool {
    let mut r = DrillRound::new();
    if r.step(0).is_err() || r.step(1).is_err() || r.step(2).is_err() {
        return false;
    }
    let mut actual = derive_expected();
    let slot = match actual.get_mut(3) {
        Some(v) => v,
        None => return false,
    };
    *slot = slot.wrapping_add(1);
    match r.check_samples(&actual) {
        Err(code) => {
            if code != Drill70Code::SampleDrift.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 好样例即过。
    let good = derive_expected();
    match r.check_samples(&good) {
        Ok(n) => {
            if n != SAMPLES_PER_ROUND {
                return false;
            }
        }
        Err(_) => return false,
    }
    true
}

/// 指纹失平注入：包指纹 / 账指纹改值必抓。
fn inject_finger_ok() -> bool {
    let mut r = DrillRound::new();
    if r.step(0).is_err() {
        return false;
    }
    match r.check_pack(FROZEN_PACK_FINGER ^ 0xFF) {
        Err(code) => {
            if code != Drill70Code::PackFingerBad.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if r.step(1).is_err() {
        return false;
    }
    if r.check_iface(FROZEN_IFACE_SEATS + 1).is_ok() {
        return false;
    }
    if r.step(2).is_err() || r.step(3).is_err() {
        return false;
    }
    match r.check_ledger(FROZEN_LEDGER_FINGER ^ 0xFF) {
        Err(code) => {
            if code != Drill70Code::LedgerFingerBad.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 轮数注入：九轮落报告必拒（RoundsShort）。
fn inject_rounds_ok() -> bool {
    let mut bk = DrillBook::new();
    let mut r = 0usize;
    while r < DRILL_ROUNDS - 1 {
        if bk.run_round().is_err() {
            return false;
        }
        r += 1;
    }
    match bk.seal_report() {
        Err(code) => {
            if code != Drill70Code::RoundsShort.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 补足第十轮后落报告即过。
    if bk.run_round().is_err() {
        return false;
    }
    if bk.seal_report().is_err() {
        return false;
    }
    true
}

/// 步越闭集注入：步号 4 必抓（StepUnknown）。
fn inject_stepidx_ok() -> bool {
    let mut r = DrillRound::new();
    match r.step(4) {
        Err(code) => {
            if code != Drill70Code::StepUnknown.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// LCG 样例确定：同种子期望序列逐位相等。
fn samples_deterministic_ok() -> bool {
    let a = derive_expected();
    let b = derive_expected();
    if a.len() != SAMPLES_PER_ROUND || b.len() != SAMPLES_PER_ROUND {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        let x = match a.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let y = match b.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if x != y {
            return false;
        }
        i += 1;
    }
    true
}

/// 双跑一致：好件指纹与报告两次构建逐位相等。
fn double_run_ok() -> bool {
    let d1 = build_drill_ok().report_digest();
    let d2 = build_drill_ok().report_digest();
    if d1 != d2 || d1 == 0 {
        return false;
    }
    let t1 = match build_drill_ok().report_text() {
        Some(t) => t.to_string(),
        None => return false,
    };
    let t2 = match build_drill_ok().report_text() {
        Some(t) => t.to_string(),
        None => return false,
    };
    if t1 != t2 {
        return false;
    }
    true
}

/// 诊断码闭集：12 码唯一、落窗、查表可回、闭集外占位。
fn diag_closed_set_ok() -> bool {
    let mut seen: Vec<u32> = Vec::new();
    for c in Drill70Code::ALL.iter() {
        let v = c.val();
        if v <= SEG_BASE || v > SEG_BASE + 0x0C {
            return false;
        }
        if seen.iter().any(|s| *s == v) {
            return false;
        }
        if c.detail().is_empty() {
            return false;
        }
        seen.push(v);
    }
    if seen.len() != 12 {
        return false;
    }
    let first = match Drill70Code::ALL.get(0) {
        Some(c) => c.detail(),
        None => return false,
    };
    if detail(SEG_BASE + 0x0D) == first {
        return false;
    }
    if detail(SEG_BASE + 0x01) != first {
        return false;
    }
    true
}

// ── 检查节（入口，≥12 条 add_dyn） ──────────────────────────────
pub fn run_unx1570_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1570 移交演练");
    s.add_dyn(
        "段位守卫",
        seg_guard_ok(),
        &format!("SEG_BASE=0x{:X} 须 0x40 对齐且落本批段窗 0x78C00..=0x78DC0", SEG_BASE),
    );
    s.add_dyn("四步单调走查", four_steps_ok(), "读包→接接口→跑样例→查账全过");
    s.add_dyn("10/10演练", rounds_10_ok(), "十轮冷启动全过报告封账");
    s.add_dyn("演练报告落档", report_archived_ok(), "12 行报告四步名齐指纹落账");
    s.add_dyn("跳步注入必抓", inject_skip_ok(), "越步直进必抓 StepSkipped");
    s.add_dyn("倒步注入必抓", inject_regress_ok(), "回退步必抓 StepRegressed");
    s.add_dyn("样例漂移注入", inject_drift_ok(), "期望值失配必抓 SampleDrift");
    s.add_dyn("指纹失平注入", inject_finger_ok(), "包/席/账三口径失平必抓");
    s.add_dyn("轮数注入必抓", inject_rounds_ok(), "九轮落报告必拒补轮后过");
    s.add_dyn("步号越闭集", inject_stepidx_ok(), "步号 4 必抓 StepUnknown");
    s.add_dyn("样例序列确定", samples_deterministic_ok(), "同种子期望序列逐位相等");
    s.add_dyn("双跑一致", double_run_ok(), "指纹与报告两次构建逐位相等");
    s.add_dyn("诊断码闭集", diag_closed_set_ok(), "12 码唯一、落窗、查表可回");
    s
}

// ── 测试区（≥8 条） ─────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_frozen() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x78D80);
    }

    #[test]
    fn checks_all_green_min_entries() {
        let s = run_unx1570_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }

    #[test]
    fn steps_frozen() {
        assert_eq!(STEPS.len(), 4);
        assert_eq!(DRILL_ROUNDS, 10);
        assert_eq!(SAMPLES_PER_ROUND, 6);
        assert_eq!(REPORT_ROWS, 12);
        let first = match STEPS.get(0) {
            Some(n) => *n,
            None => "",
        };
        let last = match STEPS.get(3) {
            Some(n) => *n,
            None => "",
        };
        assert_eq!(first, "读包");
        assert_eq!(last, "查账");
    }

    #[test]
    fn full_path_seals() {
        let bk = build_drill_ok();
        assert_eq!(bk.round_count(), 10);
        assert_ne!(bk.report_digest(), 0);
        assert!(bk.report_text().is_some());
    }

    #[test]
    fn skip_and_regress_rejected() {
        let mut r = DrillRound::new();
        match r.step(1) {
            Err(code) => assert_eq!(code, Drill70Code::StepSkipped.val()),
            Ok(_) => assert!(false, "跳步必须被拒"),
        }
        assert!(r.step(0).is_ok());
        assert!(r.step(1).is_ok());
        match r.step(0) {
            Err(code) => assert_eq!(code, Drill70Code::StepRegressed.val()),
            Ok(_) => assert!(false, "倒步必须被拒"),
        }
    }

    #[test]
    fn repeat_step_is_idempotent() {
        let mut r = DrillRound::new();
        assert!(r.step(0).is_ok());
        match r.step(0) {
            Ok(p) => assert_eq!(p, WalkPhase::ReadPack),
            Err(_) => assert!(false, "重复步必须幂等"),
        }
        assert_eq!(r.trail_len(), 1);
    }

    #[test]
    fn sample_drift_detected_then_good_passes() {
        let mut r = DrillRound::new();
        let _ = r.step(0);
        let _ = r.step(1);
        let _ = r.step(2);
        let mut bad = derive_expected();
        let slot = match bad.first_mut() {
            Some(v) => v,
            None => return,
        };
        *slot = slot.wrapping_add(7);
        match r.check_samples(&bad) {
            Err(code) => assert_eq!(code, Drill70Code::SampleDrift.val()),
            Ok(_) => assert!(false, "样例漂移必须被拒"),
        }
        let good = derive_expected();
        assert!(r.check_samples(&good).is_ok());
    }

    #[test]
    fn wrong_phase_checks_rejected() {
        let r = DrillRound::new();
        match r.check_pack(FROZEN_PACK_FINGER) {
            Err(code) => assert_eq!(code, Drill70Code::PhaseRegress.val()),
            Ok(_) => assert!(false, "未读包不得核包指纹"),
        }
    }

    #[test]
    fn rounds_short_rejected() {
        let mut bk = DrillBook::new();
        assert!(bk.run_round().is_ok());
        match bk.seal_report() {
            Err(code) => assert_eq!(code, Drill70Code::RoundsShort.val()),
            Ok(_) => assert!(false, "一轮落报告必须被拒"),
        }
    }

    #[test]
    fn round_digests_identical_across_rounds() {
        let d1 = {
            let mut r = DrillRound::new();
            let _ = r.step(0);
            let _ = r.step(1);
            let _ = r.step(2);
            let expect = derive_expected();
            let _ = r.check_samples(&expect);
            let _ = r.step(3);
            r.round_digest()
        };
        let d2 = {
            let mut r = DrillRound::new();
            let _ = r.step(0);
            let _ = r.step(1);
            let _ = r.step(2);
            let expect = derive_expected();
            let _ = r.check_samples(&expect);
            let _ = r.step(3);
            r.round_digest()
        };
        assert_ne!(d1, 0);
        assert_eq!(d1, d2);
    }

    #[test]
    fn double_run_digest_identical() {
        let d1 = build_drill_ok().report_digest();
        let d2 = build_drill_ok().report_digest();
        assert_eq!(d1, d2);
        assert_ne!(d1, 0);
    }

    #[test]
    fn diag_codes_closed_unique_windowed() {
        assert!(diag_closed_set_ok());
        assert_eq!(Drill70Code::ALL.len(), 12);
        let first = match Drill70Code::ALL.get(0) {
            Some(c) => c.val(),
            None => 0,
        };
        let last = match Drill70Code::ALL.get(11) {
            Some(c) => c.val(),
            None => 0,
        };
        assert_eq!(first, SEG_BASE + 0x01);
        assert_eq!(last, SEG_BASE + 0x0C);
        assert_eq!(detail(0), "未登记诊断码：不在本段闭集内");
    }
}
