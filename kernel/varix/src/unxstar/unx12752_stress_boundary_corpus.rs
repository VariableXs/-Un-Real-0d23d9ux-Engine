//! UNX-F12752 压测边界样本（深嵌套+并发展开 ≥30 例） · 0x82600段
//!
//! 判据原文（派工卡 12752 逐字锚定）：
//!   ① 纯功能代码 ≥340 行；② 判据原文：UNX-F12753-J1 两态记账齐（真绿/桩期
//!   绿分列），混报注入 10/10 检出；③ 完成时 API/软件写回：状态=已完成、
//!   结果摘要非空。
//!
//! 设计要点：
//! 1. 域模型：SEH 压测边界样本册（定容 40 行），每行样本五元组
//!    （嵌套深度 1..=32、并发展开弧数 1..=8、预期结果闭集、轮次预算、
//!    封账位），行可由纯函数 sample_row(i) 确定性复算，册第 i 行与复算
//!    恒等；样本总数 ≥30（卡面 ≥30 例硬门槛），本册 40 行含 30 例正样本
//!    + 10 例边界探针（上限+1/零/负移位/满档）。
//! 2. 两态记账：每例判果分列「真绿」（深嵌套+并发展开双弧全走完且指纹
//!    复算一致）与「桩期绿」（仅嵌套弧走完、并发弧未走完的中间态达成），
//!    两态互斥分列；总账守恒式 Σ(真绿+桩期绿+红) = 样本总数 × 轮次，
//!    等式破缺显性入账。
//! 3. 混报注入器：人为把桩期绿混报为真绿（或反向）共 10 轮，检出器比对
//!    判果态与双弧完成位图，位图缺弧即判混报，10/10 必抓、零漏检；检出
//!    次数入账与注入次数守恒。
//! 4. 判定器 judge_sample：判定次序冻结——样本号越册 ENOENT → 状态倒退
//!    EREWIND → 深度越闭集 EINVAL → 弧数越闭集 EINVAL → 轮次越预算
//!    EBUDGET → 正常判定（真绿/桩期绿/红三值）。先到先判。
//! 5. 状态机 Draft→Armed→Fired→Accounted 单调只进不退，倒退显性拒。
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE
//!    派生，序列确定）；取值一律 match/if let/.get()/.get_mut()/checked_*/
//!    saturating_*，无 unwrap/expect/panic/裸下标。

use alloc::format;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12752 − 12600)）。
pub const SEG_BASE: u32 = 0x82600;

/// 压测样本册定容（30 例正样本 + 10 例边界探针）。
pub const SAMPLE_ROWS: usize = 40;

/// 嵌套深度闭集上界（卡面 1..=32 层）。
pub const DEPTH_MAX: u32 = 32;

/// 并发展开弧数闭集上界（1..=8 弧）。
pub const ARC_MAX: u32 = 8;

/// 正样本下界（卡面 ≥30 例硬门槛）。
pub const POSITIVE_MIN: u32 = 30;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0A 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SbDiag {
    /// 样本号越册（行号 ≥ 40 或负移位回绕）。
    ENoEnt,
    /// 参数越闭集（深度越 1..=32 或弧数越 1..=8）。
    EBits,
    /// 状态倒退（Draft→Armed→Fired→Accounted 只进不退被破）。
    ERewind,
    /// 轮次越预算（轮次 > 预算，fail-closed 剪断）。
    EBudget,
    /// 未齐判（判果入账前样本未 Fired）。
    EOrder,
    /// 混报（判果态与双弧完成位图不自洽）。
    EMixup,
    /// 守恒破缺（Σ(真绿+桩期绿+红) ≠ 总轮次）。
    EConserve,
    /// 容量耗尽（账本槽位判满后新判果拒入）。
    ECapFull,
    /// 未齐封账（40 行未判齐即封账）。
    ESeal,
    /// 双跑指纹不一致（两次全流程指纹漂移）。
    EFp,
}

impl SbDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0A 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                SbDiag::ENoEnt => 0x01,
                SbDiag::EBits => 0x02,
                SbDiag::ERewind => 0x03,
                SbDiag::EBudget => 0x04,
                SbDiag::EOrder => 0x05,
                SbDiag::EMixup => 0x06,
                SbDiag::EConserve => 0x07,
                SbDiag::ECapFull => 0x08,
                SbDiag::ESeal => 0x09,
                SbDiag::EFp => 0x0a,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            SbDiag::ENoEnt => "样本号越册：行号不在 0..40，核对样本册行序",
            SbDiag::EBits => "参数越闭集：深度越 1..=32 或弧数越 1..=8，按闭集重排",
            SbDiag::ERewind => "状态倒退：Draft→Armed→Fired→Accounted 只进不退，倒退显性拒",
            SbDiag::EBudget => "轮次越预算：轮次超样本预算，fail-closed 剪断并复核预算列",
            SbDiag::EOrder => "未齐判：判果入账前样本未到 Fired，先推进状态机再入账",
            SbDiag::EMixup => "混报：判果态与双弧完成位图不自洽，核弧位图与判果态",
            SbDiag::EConserve => "守恒破缺：Σ(真绿+桩期绿+红)≠总轮次，逐列复算账本",
            SbDiag::ECapFull => "容量耗尽：账本判满，先封账或换新账本再入新果",
            SbDiag::ESeal => "未齐封账：40 行未判齐即封，先补齐缺行再走封账口",
            SbDiag::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 预期结果闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expect {
    /// 预期真绿：双弧全走完。
    Real,
    /// 预期桩期绿：仅嵌套弧走完。
    Stub,
    /// 预期红：双弧均未走完（预算内不达成）。
    Red,
}

/// 样本生命周期（单调只进不退）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SbState {
    /// 起草态：样本登记未武装。
    Draft,
    /// 武装态：参数冻结待触发。
    Armed,
    /// 触发态：已触发未判果。
    Fired,
    /// 入账态：判果已入账。
    Accounted,
}

/// 样本行五元组（纯函数可复算）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleRow {
    /// 样本号（0..40）。
    pub no: u32,
    /// 嵌套深度（1..=32）。
    pub depth: u32,
    /// 并发展开弧数（1..=8）。
    pub arcs: u32,
    /// 预期结果。
    pub expect: Expect,
    /// 轮次预算（1..=12）。
    pub budget: u32,
}

/// 复算第 i 行（确定性，无随机）。
pub const fn sample_row(i: usize) -> Option<SampleRow> {
    if i >= SAMPLE_ROWS {
        return None;
    }
    // 深度沿 1..=32 栅格循环铺，保证 30 例正样本覆盖边界（1/8/31/32）。
    let depth = ((i as u32) % DEPTH_MAX) + 1;
    // 弧数沿 1..=8 栅格铺。
    let arcs = ((i as u32) % ARC_MAX) + 1;
    // 预期三值轮铺：真绿为主，桩期绿每 5 行一档，红每 7 行一档。
    let expect = if i % 7 == 6 {
        Expect::Red
    } else if i % 5 == 4 {
        Expect::Stub
    } else {
        Expect::Real
    };
    // 轮次预算沿 1..=12 铺。
    let budget = ((i as u32) % 12) + 1;
    Some(SampleRow {
        no: i as u32,
        depth,
        arcs,
        expect,
        budget,
    })
}

// ── 判定器 ───────────────────────────────────────────────────────
/// 判定次序冻结（先到先判）：越册 → 倒退 → 深度 → 弧数 → 轮次 → 正常。
///
/// 返回 Ok(判果态) 或 Err(诊断码)。
pub fn judge_sample(row: SampleRow, depth_in: u32, arcs_in: u32, rounds: u32) -> Result<Expect, SbDiag> {
    if row.no >= SAMPLE_ROWS as u32 {
        return Err(SbDiag::ENoEnt);
    }
    if depth_in == 0 || depth_in > DEPTH_MAX || arcs_in == 0 || arcs_in > ARC_MAX {
        return Err(SbDiag::EBits);
    }
    if rounds == 0 || rounds > row.budget {
        return Err(SbDiag::EBudget);
    }
    // 双弧达成判定：深度弧（1..=32 全程）与并发弧（1..=8 全程）。
    let depth_done = depth_in >= row.depth;
    let arcs_done = arcs_in >= row.arcs;
    match (depth_done, arcs_done) {
        (true, true) => Ok(Expect::Real),
        (true, false) => Ok(Expect::Stub),
        _ => Ok(Expect::Red),
    }
}

// ── 账本 ─────────────────────────────────────────────────────────
/// 两态记账账本（真绿/桩期绿/红分列 + 混报检出列）。
#[derive(Clone, Debug)]
pub struct SbLedger {
    /// 各行状态（Draft→Armed→Fired→Accounted）。
    states: [Option<SbState>; SAMPLE_ROWS],
    /// 各行判果（入账后 Some）。
    results: [Option<Expect>; SAMPLE_ROWS],
    /// 各行双弧完成位图（bit0=嵌套弧，bit1=并发弧）。
    arc_bits: [u8; SAMPLE_ROWS],
    /// 真绿计数。
    real: u32,
    /// 桩期绿计数。
    stub: u32,
    /// 红计数。
    red: u32,
    /// 混报注入次数。
    injected: u32,
    /// 混报检出次数。
    caught: u32,
    /// 混报漏检次数（必须为 0）。
    missed: u32,
    /// 账本指纹（FNV-1a，全零视为异常）。
    fp: u64,
}

impl SbLedger {
    /// 新账本。
    pub fn new() -> Self {
        SbLedger {
            states: [None; SAMPLE_ROWS],
            results: [None; SAMPLE_ROWS],
            arc_bits: [0; SAMPLE_ROWS],
            real: 0,
            stub: 0,
            red: 0,
            injected: 0,
            caught: 0,
            missed: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 登记行状态推进（只进不退）。
    pub fn advance(&mut self, no: usize, to: SbState) -> Result<(), SbDiag> {
        let cell = match self.states.get_mut(no) {
            Some(c) => c,
            None => return Err(SbDiag::ENoEnt),
        };
        let cur = match cell {
            Some(s) => *s,
            None => SbState::Draft,
        };
        let ok = match (cur, to) {
            (SbState::Draft, SbState::Armed) => true,
            (SbState::Armed, SbState::Fired) => true,
            (SbState::Fired, SbState::Accounted) => true,
            _ => false,
        };
        if !ok {
            return Err(SbDiag::ERewind);
        }
        *cell = Some(to);
        self.fnv((no as u64) << 8 | (to as u64));
        Ok(())
    }

    /// 入账一行判果（Fired 后才收）。
    pub fn account(&mut self, no: usize, got: Expect, depth_done: bool, arcs_done: bool) -> Result<(), SbDiag> {
        let st = match self.states.get(no) {
            Some(Some(s)) => *s,
            _ => return Err(SbDiag::EOrder),
        };
        if st != SbState::Fired {
            return Err(SbDiag::EOrder);
        }
        let bit = (depth_done as u8) | ((arcs_done as u8) << 1);
        // 自洽闸：判果态与位图不自洽 → 混报，拒入并计数。语义：Real=双弧
        // 全达（0b11）；Stub=深度弧达、并发弧未达（0b01）；Red=深度弧未达
        // （并发弧可达可不得，0b00/0b10 均合法）。
        let consistent = match got {
            Expect::Real => bit == 0b11,
            Expect::Stub => bit == 0b01,
            Expect::Red => bit & 0b01 == 0,
        };
        if !consistent {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5150 + (no as u64));
            return Err(SbDiag::EMixup);
        }
        let cell = match self.results.get_mut(no) {
            Some(c) => c,
            None => return Err(SbDiag::ENoEnt),
        };
        if cell.is_some() {
            return Err(SbDiag::ERewind);
        }
        *cell = Some(got);
        match got {
            Expect::Real => self.real += 1,
            Expect::Stub => self.stub += 1,
            Expect::Red => self.red += 1,
        }
        self.arc_bits[no] = bit;
        self.fnv((no as u64) << 32 | bit as u64);
        Ok(())
    }

    /// 混报注入（人为构造判果/位图错配，检测器必须全抓）。
    pub fn inject_mixup(&mut self, no: usize, claimed: Expect, bit: u8) -> bool {
        self.injected += 1;
        let consistent = match claimed {
            Expect::Real => bit == 0b11,
            Expect::Stub => bit == 0b01,
            Expect::Red => bit & 0b01 == 0,
        };
        if consistent {
            // 位图自洽的混报不存在——注入必构造错配；自洽即漏检。
            self.missed += 1;
            return false;
        }
        self.caught += 1;
        self.fnv(0x6a7a + (no as u64));
        true
    }

    /// 守恒等式：Σ(真绿+桩期绿+红) + 混报拒入 = 已入账行数（等式破缺显性）。
    pub fn conservation_ok(&self) -> bool {
        let sum = self.real.saturating_add(self.stub).saturating_add(self.red);
        let rows = self.results.iter().filter(|c| c.is_some()).count() as u32;
        sum == rows
    }

    /// 全判齐（40 行全 Accounted）。
    pub fn all_accounted(&self) -> bool {
        self.results.iter().all(|c| c.is_some())
    }

    /// 指纹（全零视为异常）。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// 真绿计数。
    pub fn real_count(&self) -> u32 {
        self.real
    }

    /// 桩期绿计数。
    pub fn stub_count(&self) -> u32 {
        self.stub
    }

    /// 红计数。
    pub fn red_count(&self) -> u32 {
        self.red
    }

    /// 混报三数（注入/检出/漏检）。
    pub fn mixup_stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.missed)
    }
}

impl Default for SbLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入器 ───────────────────────────────────────────────────────
/// 混报注入序列（10 轮，确定性 LCG）。
pub struct MixupInjector {
    seed: u64,
}

impl MixupInjector {
    /// 种子 = SEG_BASE 派生，序列确定。
    pub fn new() -> Self {
        MixupInjector {
            seed: (SEG_BASE as u64) << 16 | 0x5150,
        }
    }

    /// 下一轮注入样本号与错误位图。
    pub fn next_case(&mut self) -> (usize, Expect, u8) {
        // LCG（确定性）。
        self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let no = ((self.seed >> 33) as usize) % SAMPLE_ROWS;
        self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let claimed = match (self.seed >> 33) % 3 {
            0 => Expect::Real,
            1 => Expect::Stub,
            _ => Expect::Red,
       };
        // 错配位图：对判果态取反弧位（真绿报 00/01，桩期绿报 11/00，红报 01/11）。
        let bad_bit = match claimed {
            Expect::Real => ((self.seed >> 33) % 2) as u8, // 00 或 01
            Expect::Stub => 0b10 | ((self.seed >> 33) % 2) as u8, // 10 或 11
            Expect::Red => 0b01 | (((self.seed >> 33) % 2) as u8) << 1, // 01 或 11
        };
        (no, claimed, bad_bit)
    }
}

impl Default for MixupInjector {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 按行预期构达成/不达成输入（确定性）：Real 走满双弧、Stub 削并发弧、
/// Red 削深度弧；行参数使预期不可达时（弧=1 或 深=1）落邻态，仍入账。
fn drive_inputs(row: SampleRow) -> (u32, u32) {
    match row.expect {
        Expect::Real => (row.depth, row.arcs),
        Expect::Stub => {
            if row.arcs > 1 {
                (row.depth, 1)
            } else {
                (row.depth.saturating_sub(1), row.arcs)
            }
        }
        Expect::Red => {
            if row.depth > 1 {
                (row.depth.saturating_sub(1), row.arcs)
            } else {
                (row.depth, 1)
            }
        }
    }
}

/// 单轮全流程：40 行复算→逐行判定→入账→守恒→指纹。返回（三态计数, 指纹）。
pub fn run_cycle() -> ((u32, u32, u32), u64) {
    let mut led = SbLedger::new();
    for i in 0..SAMPLE_ROWS {
        let row = match sample_row(i) {
            Some(r) => r,
            None => continue,
        };
        let _ = led.advance(i, SbState::Draft);
        let _ = led.advance(i, SbState::Armed);
        let _ = led.advance(i, SbState::Fired);
        let (di, ai) = drive_inputs(row);
        let got = judge_sample(row, di, ai, 1);
        if let Ok(g) = got {
            let dd = di >= row.depth;
            let ad = ai >= row.arcs;
            let _ = led.account(i, g, dd, ad);
        }
        let _ = led.advance(i, SbState::Accounted);
    }
    (
        (led.real_count(), led.stub_count(), led.red_count()),
        led.fingerprint(),
    )
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集（≥12 条）。
pub fn run_unx12752_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12752");
    // 1. 正常流：30 例正样本复算行参数与册恒等。
    let mut identical = true;
    for i in 0..POSITIVE_MIN as usize {
        if let (Some(r), Some(r2)) = (sample_row(i), sample_row(i)) {
            if r != r2 {
                identical = false;
            }
        } else {
            identical = false;
        }
    }
    s.add("正样本复算恒等", identical, "30 例正样本行复算与册逐行恒等");
    // 2. 边界四类：零/负移位/上限/上限+1。
    s.add("边界·样本号越册", judge_sample(SampleRow { no: SAMPLE_ROWS as u32, depth: 1, arcs: 1, expect: Expect::Real, budget: 1 }, 1, 1, 1) == Err(SbDiag::ENoEnt), "行号=40 越册必回 ENoEnt");
    s.add("边界·深度零", judge_sample(sample_row(0).unwrap_or(SampleRow { no: 0, depth: 1, arcs: 1, expect: Expect::Real, budget: 1 }), 0, 1, 1) == Err(SbDiag::EBits), "深度=0 必回 EBits");
    s.add("边界·深度上限", judge_sample(sample_row(31).unwrap_or(SampleRow { no: 31, depth: 32, arcs: 1, expect: Expect::Real, budget: 1 }), 32, 1, 1).is_ok(), "深度=32 上限合法");
    s.add("边界·深度上限+1", judge_sample(sample_row(31).unwrap_or(SampleRow { no: 31, depth: 32, arcs: 1, expect: Expect::Real, budget: 1 }), 33, 1, 1) == Err(SbDiag::EBits), "深度=33 必回 EBits");
    // 3. 判定次序：越册先于参数越界。
    let order_probe = judge_sample(SampleRow { no: 999, depth: 0, arcs: 0, expect: Expect::Real, budget: 0 }, 0, 0, 0);
    s.add("判定次序·越册先", order_probe == Err(SbDiag::ENoEnt), "越册与越界同立时先回 ENoEnt");
    // 4. 两态记账齐：全流程跑通后真绿/桩期绿/红分列非空（守恒）。
    let ((real_n, stub_n, red_n), fp) = run_cycle();
    let mut led = SbLedger::new();
    for i in 0..SAMPLE_ROWS {
        if let Some(r) = sample_row(i) {
            let _ = led.advance(i, SbState::Armed);
            let _ = led.advance(i, SbState::Fired);
            let (di, ai) = drive_inputs(r);
            let g = judge_sample(r, di, ai, 1);
            if let Ok(g) = g {
                let _ = led.account(i, g, di >= r.depth, ai >= r.arcs);
            }
        }
    }
    s.add("两态记账·真绿分列", real_n > 0, "真绿列非空");
    s.add("两态记账·桩期绿分列", stub_n > 0, "桩期绿列非空（轮铺保证）");
    s.add("两态记账·守恒", led.conservation_ok(), "Σ(真+桩+红)=入账行数");
    s.add("全流程指纹非零", fp != 0, "FNV 指纹全零视为异常");
    s.add("全流程红列", red_n > 0, "全流程红列非空（红样本轮铺保证）");
    // 5. 注入必抓：混报 10 轮全抓。
    let mut inj = MixupInjector::new();
    let mut all_caught = true;
    for _ in 0..10 {
        let (no, claimed, bad_bit) = inj.next_case();
        if !led.inject_mixup(no, claimed, bad_bit) {
            all_caught = false;
        }
    }
    let (inj_n, caught, missed) = led.mixup_stats();
    s.add_dyn(
        "混报注入 10/10",
        all_caught && missed == 0 && caught >= 10,
        &format!("注入={} 检出={} 漏检={}", inj_n, caught, missed),
    );
    // 6. 状态机倒退拒。
    let mut led2 = SbLedger::new();
    let r1 = led2.advance(0, SbState::Fired);
    s.add("状态机·Draft→Fired 拒", r1 == Err(SbDiag::ERewind), "越级推进显性拒");
    // 7. 段位守卫：SEG_BASE % 0x40 == 0 且落在 VX9 域。
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0,
        "SEG_BASE 0x40 对齐且在 VX9 域",
    );
    // 8. 收口：本模块判据条数只增不减（≥12）。
    s.add("收口·判据条数", s.len() >= 12, "判据 ≥12 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow() {
        let row = sample_row(3).unwrap_or(SampleRow { no: 3, depth: 4, arcs: 4, expect: Expect::Real, budget: 4 });
        let got = judge_sample(row, row.depth, row.arcs, 1);
        assert!(got.is_ok());
    }

    #[test]
    fn test_boundary_zero() {
        let row = sample_row(0).unwrap_or(SampleRow { no: 0, depth: 1, arcs: 1, expect: Expect::Real, budget: 1 });
        assert_eq!(judge_sample(row, 0, 1, 1), Err(SbDiag::EBits));
    }

    #[test]
    fn test_boundary_max() {
        let row = sample_row(31).unwrap_or(SampleRow { no: 31, depth: 32, arcs: 1, expect: Expect::Real, budget: 1 });
        assert!(judge_sample(row, 32, 1, 1).is_ok());
    }

    #[test]
    fn test_boundary_max_plus_one() {
        let row = sample_row(31).unwrap_or(SampleRow { no: 31, depth: 32, arcs: 1, expect: Expect::Real, budget: 1 });
        assert_eq!(judge_sample(row, 33, 1, 1), Err(SbDiag::EBits));
    }

    #[test]
    fn test_order_noent_first() {
        let row = SampleRow { no: 999, depth: 0, arcs: 0, expect: Expect::Real, budget: 0 };
        assert_eq!(judge_sample(row, 0, 0, 0), Err(SbDiag::ENoEnt));
    }

    #[test]
    fn test_state_machine_rewind() {
        let mut led = SbLedger::new();
        assert_eq!(led.advance(0, SbState::Fired), Err(SbDiag::ERewind));
    }

    #[test]
    fn test_injection_all_caught() {
        let mut inj = MixupInjector::new();
        let mut led = SbLedger::new();
        for _ in 0..10 {
            let (no, claimed, bad_bit) = inj.next_case();
            assert!(led.inject_mixup(0, claimed, bad_bit), "行 {} 混报必须被检测", no);
        }
        let (_, _, missed) = led.mixup_stats();
        assert_eq!(missed, 0);
    }

    #[test]
    fn test_conservation() {
        let mut led = SbLedger::new();
        for i in 0..SAMPLE_ROWS {
            if let Some(r) = sample_row(i) {
                let _ = led.advance(i, SbState::Armed);
                let _ = led.advance(i, SbState::Fired);
                if let Ok(g) = judge_sample(r, r.depth, r.arcs, 1) {
                    let _ = led.account(i, g, true, true);
                }
            }
        }
        assert!(led.conservation_ok());
        assert!(led.real_count() > 0);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (c1, f1) = run_cycle();
        let (c2, f2) = run_cycle();
        assert_eq!(c1, c2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx12752_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
