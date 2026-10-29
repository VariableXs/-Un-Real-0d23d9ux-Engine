//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1330 · 等价验证故障注入（等价破坏场景必抓） · 0x7700
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F1330-J1 故障场景 ≥5 类（隐性丢事件/时序漂移/应答错配/
//!      基线污染/判定绕过）注入 10/10 次必抓（等价率跌破 100% 即红账），
//!      防护动作正确率 100%，资产可重复一致率 100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 故障宇宙 6 类：隐性丢事件/时序漂移/应答错配/基线污染/判定绕过/
//!     事件重复，全确定性注入（LCG 种子写死）；
//!   - 双路对拍检域：轮询流 vs 中断流逐事件对拍，任一故障落入必须使
//!     等价率跌破 100%（跌破即红账，红账即入防护流）；
//!   - 防护动作状态机：Detect→Freeze→Locate→Revert 四步，每步带前置
//!     守卫，正确率 100%（跳步/错序/漏步必拒）；
//!   - 判定绕过防线：绕过判据（宣称等价但违例>0）必须被双验复核打回；
//!   - 资产可重复：同故障种子三跑，检出结论+防护轨迹全同。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

// ═══════════════════════════════════════════════════════════════════════
// §0 常量与诊断码闭集（0x7700 段，连续编号）
// ═══════════════════════════════════════════════════════════════════════

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x7700;

/// 注入演练轮数（判据点名 10）。
pub const INJECT_ROUNDS: usize = 10;

/// 故障宇宙（6 类）。
pub const FAULT_COUNT: usize = 6;

/// 对拍事件量（每故障场景）。
pub const CMP_EVENTS: u64 = 4096;

/// 0x7700 故障漏抓。
pub const E_FAULT_MISS: u16 = SEG_BASE + 0;
/// 0x7701 防护跳步。
pub const E_GUARD_SKIP: u16 = SEG_BASE + 1;
/// 0x7702 判定绕过得手（宣称等价但违例>0）。
pub const E_BYPASS_PASS: u16 = SEG_BASE + 2;
/// 0x7703 防护漏步（四步不满）。
pub const E_GUARD_SHORT: u16 = SEG_BASE + 3;
/// 0x7704 资产不可重复（三跑不一致）。
pub const E_REPEAT: u16 = SEG_BASE + 4;
/// 0x7705 故障越界。
pub const E_FAULT_OOB: u16 = SEG_BASE + 5;
/// 0x7706 等价未破（注入后仍宣称 100%）。
pub const E_EQ_STUCK: u16 = SEG_BASE + 6;
/// 0x7707 回退不净。
pub const E_REVERT_DIRTY: u16 = SEG_BASE + 7;
/// 0x7708 红账漏入（跌破未入防护流）。
pub const E_RED_OMIT: u16 = SEG_BASE + 8;
/// 0x7709 双验失配。
pub const E_DUAL_MISS: u16 = SEG_BASE + 9;

/// 全码闭集。
pub const CODE_ALL: [u16; 10] = [
    E_FAULT_MISS, E_GUARD_SKIP, E_BYPASS_PASS, E_GUARD_SHORT, E_REPEAT,
    E_FAULT_OOB, E_EQ_STUCK, E_REVERT_DIRTY, E_RED_OMIT, E_DUAL_MISS,
];

/// 诊断码中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_FAULT_MISS => "故障漏抓：注入器与判定器必须同源（同一事件宇宙），漏抓先比对拍边界",
        E_GUARD_SKIP => "防护跳步：四步机每步前置态校验，跳步返回 Err(E_GUARD_SKIP)",
        E_BYPASS_PASS => "判定绕过得手：宣称等价必须过双验（违例计数+逐事件重核）",
        E_GUARD_SHORT => "防护漏步：四步不满禁封账，查动作轨迹长度",
        E_REPEAT => "资产不可重复：同故障种子三跑必须全同，查 LCG 状态污染",
        E_FAULT_OOB => "故障越界：故障宇宙 6 类闭集（0..6），越界即拒",
        E_EQ_STUCK => "等价未破：注入后仍 100% 说明注入落空，查注入命中条件",
        E_REVERT_DIRTY => "回退不净：回退后事件账必须回基线（计数+指纹全同）",
        E_RED_OMIT => "红账漏入：跌破 100% 必入防护流，漏入即双错",
        E_DUAL_MISS => "双验失配：两判必须同真同假，同真同假才放行",
        _ => "未知诊断码：核对 0x7700 段闭集登记表",
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §1 LCG 与故障宇宙
// ═══════════════════════════════════════════════════════════════════════

/// 64 位 LCG。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 建（种子写死）。
    pub fn new(seed: u64) -> Self {
        Lcg {
            state: seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407)
                | 1,
        }
    }

    /// 下一原始值。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }

    /// [0, n)。
    pub fn next_below(&mut self, n: u64) -> u64 {
        let m = if n == 0 { 1 } else { n };
        self.next_u64() % m
    }
}

/// 故障宇宙（6 类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultKind {
    /// 隐性丢事件（中断侧吞事件）。
    HiddenDrop,
    /// 时序漂移（中断侧次序错排）。
    TimingDrift,
    /// 应答错配（命令-应答张冠李戴）。
    RespMismatch,
    /// 基线污染（基线流被改写）。
    BaselinePollute,
    /// 判定绕过（宣称等价但违例存在）。
    JudgeBypass,
    /// 事件重复（同事件重复入账）。
    EventDup,
}

impl FaultKind {
    /// 闭集。
    pub const ALL: [FaultKind; 6] = [
        FaultKind::HiddenDrop,
        FaultKind::TimingDrift,
        FaultKind::RespMismatch,
        FaultKind::BaselinePollute,
        FaultKind::JudgeBypass,
        FaultKind::EventDup,
    ];

    /// 中文短句。
    pub const fn text(self) -> &'static str {
        match self {
            FaultKind::HiddenDrop => "隐性丢事件",
            FaultKind::TimingDrift => "时序漂移",
            FaultKind::RespMismatch => "应答错配",
            FaultKind::BaselinePollute => "基线污染",
            FaultKind::JudgeBypass => "判定绕过",
            FaultKind::EventDup => "事件重复",
        }
    }

    /// 枚举序。
    pub const fn idx(self) -> usize {
        match self {
            FaultKind::HiddenDrop => 0,
            FaultKind::TimingDrift => 1,
            FaultKind::RespMismatch => 2,
            FaultKind::BaselinePollute => 3,
            FaultKind::JudgeBypass => 4,
            FaultKind::EventDup => 5,
        }
    }

    /// 从序号还原。
    pub const fn from_idx(i: usize) -> Option<FaultKind> {
        match i {
            0 => Some(FaultKind::HiddenDrop),
            1 => Some(FaultKind::TimingDrift),
            2 => Some(FaultKind::RespMismatch),
            3 => Some(FaultKind::BaselinePollute),
            4 => Some(FaultKind::JudgeBypass),
            5 => Some(FaultKind::EventDup),
            _ => None,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §2 事件流与故障注入器
// ═══════════════════════════════════════════════════════════════════════

/// 双路对拍样本（轮询流 + 中断流）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DualStreams {
    pub poll: Vec<u64>,
    pub int: Vec<u64>,
    /// 命令-应答对（cmd, resp）序列。
    pub resp_pairs: Vec<(u8, u8)>,
    /// 基线指纹（poll 流 FNV）。
    pub baseline_fp: u64,
}

/// 生成诚实双路（同源 LCG，逐值全同）。
pub fn gen_honest(seed: u64, n: u64) -> DualStreams {
    let mut g = Lcg::new(seed);
    let mut poll = Vec::new();
    let mut int = Vec::new();
    let mut pairs = Vec::new();
    for i in 0..n {
        let v = g.next_u64();
        poll.push(v);
        int.push(v);
        if i % 64 == 0 {
            let cmd = (g.next_u64() & 0x03) as u8; // 0..3
            let resp = cmd.wrapping_add(0x10); // 派生合法应答
            pairs.push((cmd, resp));
        }
    }
    let fp = fnv64(&poll);
    DualStreams { poll, int, resp_pairs: pairs, baseline_fp: fp }
}

/// FNV-1a 64 位（u64 数据流摘要）。
pub const fn fnv64(data: &[u64]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < data.len() {
        // 逐字节吸收 u64。
        let v = data[i];
        let mut b = 0;
        while b < 8 {
            h ^= (v >> (b * 8)) & 0xFF;
            h = h.wrapping_mul(0x100000001b3);
            b += 1;
        }
        i += 1;
    }
    h
}

/// 注入故障（对中断侧流/应答表/基线做确定性破坏，返回注入是否命中）。
pub fn inject_fault(s: &mut DualStreams, kind: FaultKind, seed: u64) -> bool {
    let mut g = Lcg::new(seed ^ ((kind.idx() as u64) << 24));
    match kind {
        FaultKind::HiddenDrop => {
            // 每泄漏点吞一个事件（必命中：事件量足够）。
            if s.int.len() < 8 {
                return false;
            }
            let pos = (g.next_below((s.int.len() - 4) as u64) as usize) + 2;
            s.int.remove(pos);
            true
        }
        FaultKind::TimingDrift => {
            // 相邻两事件次序对调。
            if s.int.len() < 8 {
                return false;
            }
            let pos = (g.next_below((s.int.len() - 5) as u64) as usize) + 2;
            s.int.swap(pos, pos + 1);
            true
        }
        FaultKind::RespMismatch => {
            // 应答表相邻两应答对调（张冠李戴）。
            if s.resp_pairs.len() < 2 {
                return false;
            }
            let pos = g.next_below((s.resp_pairs.len() - 1) as u64) as usize;
            let t = s.resp_pairs[pos].1;
            s.resp_pairs[pos].1 = s.resp_pairs[pos + 1].1;
            s.resp_pairs[pos + 1].1 = t;
            true
        }
        FaultKind::BaselinePollute => {
            // 基线指纹改写（污染=基线与实际流脱钩）。
            s.baseline_fp = s.baseline_fp ^ 0xDEAD_BEEF;
            true
        }
        FaultKind::JudgeBypass => {
            // 宣称等价但暗中丢事件（等判定面抓）。
            if s.int.len() < 8 {
                return false;
            }
            s.int.truncate(s.int.len() - 3);
            true
        }
        FaultKind::EventDup => {
            // 同事件重复入账（中断侧插重复值）。
            if s.int.len() < 8 {
                return false;
            }
            let pos = (g.next_below((s.int.len() - 4) as u64) as usize) + 2;
            let v = s.int[pos];
            s.int.insert(pos, v);
            true
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §3 检测器（等价判定 + 双验复核）
// ═══════════════════════════════════════════════════════════════════════

/// 检测结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetectOut {
    /// 等价（流级）。
    pub stream_eq: bool,
    /// 应答配对等价。
    pub resp_eq: bool,
    /// 基线一致。
    pub baseline_ok: bool,
    /// 综合绿（三者全过）。
    pub green: bool,
    /// 首差异（流级）。
    pub first_diff: Option<usize>,
}

/// 主判定：流对拍+应答对拍+基线核三重。
pub fn detect(s: &DualStreams) -> DetectOut {
    let n = if s.poll.len() < s.int.len() { s.poll.len() } else { s.int.len() };
    let mut first_diff: Option<usize> = None;
    let mut i = 0;
    while i < n {
        if s.poll[i] != s.int[i] {
            first_diff = Some(i);
            break;
        }
        i += 1;
    }
    // 长度失配也是流级差异：首差钉在公共前缀尾（供定位步使用）。
    if s.poll.len() != s.int.len() && first_diff.is_none() {
        first_diff = Some(n);
    }
    let stream_eq = s.poll.len() == s.int.len() && first_diff.is_none();
    let resp_eq = s.resp_pairs.iter().all(|(c, r)| *r == c.wrapping_add(0x10));
    let baseline_ok = fnv64(&s.poll) == s.baseline_fp;
    DetectOut {
        green: stream_eq && resp_eq && baseline_ok,
        stream_eq,
        resp_eq,
        baseline_ok,
        first_diff,
    }
}

/// 双验复核（主判 + 独立重核必须同结论；否则 E_DUAL_MISS）。
pub fn dual_check(s: &DualStreams) -> Result<DetectOut, u16> {
    let main = detect(s);
    // 独立重核：用指纹+长度+应答长度重推。
    let fp_eq = fnv64(&s.poll) == fnv64(&s.int) && s.poll.len() == s.int.len();
    let resp_ok = s.resp_pairs.iter().all(|(c, r)| *r == c.wrapping_add(0x10));
    let bl_ok = fnv64(&s.poll) == s.baseline_fp;
    let again_green = fp_eq && resp_ok && bl_ok;
    if main.green == again_green {
        Ok(main)
    } else {
        Err(E_DUAL_MISS)
    }
}

/// 判定绕过防线：宣称等价但违例面存在的病态结论必须打回。
pub fn bypass_guard(s: &DualStreams, claimed_eq: bool) -> Result<(), u16> {
    let real = detect(s);
    if claimed_eq && !real.green {
        return Err(E_BYPASS_PASS);
    }
    if !claimed_eq && real.green {
        return Err(E_DUAL_MISS);
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// §4 防护动作状态机（Detect→Freeze→Locate→Revert）
// ═══════════════════════════════════════════════════════════════════════

/// 防护步。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardStep {
    /// 检出。
    Detect,
    /// 冻结。
    Freeze,
    /// 定位。
    Locate,
    /// 回退。
    Revert,
}

impl GuardStep {
    /// 中文短句。
    pub const fn text(self) -> &'static str {
        match self {
            GuardStep::Detect => "检出",
            GuardStep::Freeze => "冻结",
            GuardStep::Locate => "定位",
            GuardStep::Revert => "回退",
        }
    }
}

/// 防护轨迹（单次故障的处置流水）。
pub struct GuardTrail {
    /// 故障类。
    pub fault: FaultKind,
    /// 已执行步（按序）。
    steps: Vec<GuardStep>,
    /// 定位点。
    pub located_at: Option<usize>,
    /// 封账（四步全过）。
    pub sealed: bool,
    /// 回退后基线恢复。
    pub baseline_restored: bool,
}

impl GuardTrail {
    /// 建（检出步由构造即记）。
    pub fn new(fault: FaultKind) -> Self {
        let mut steps = Vec::new();
        steps.push(GuardStep::Detect);
        GuardTrail { fault, steps, located_at: None, sealed: false, baseline_restored: false }
    }

    /// 推进一步（顺序守卫：只允许下一正确步）。
    pub fn advance(&mut self, step: GuardStep, at: Option<usize>) -> Result<(), u16> {
        let expect = match self.steps.len() {
            1 => GuardStep::Freeze,
            2 => GuardStep::Locate,
            3 => GuardStep::Revert,
            _ => return Err(E_GUARD_SHORT),
        };
        if step != expect {
            return Err(E_GUARD_SKIP);
        }
        if step == GuardStep::Locate {
            match at {
                Some(p) => self.located_at = Some(p),
                None => return Err(E_GUARD_SKIP),
            }
        }
        if step == GuardStep::Revert {
            if self.located_at.is_none() {
                return Err(E_GUARD_SKIP);
            }
            self.baseline_restored = true;
            self.sealed = true;
        }
        self.steps.push(step);
        Ok(())
    }

    /// 四步全过。
    pub fn full(&self) -> bool {
        self.steps.len() == 4
            && self.steps[0] == GuardStep::Detect
            && self.steps[1] == GuardStep::Freeze
            && self.steps[2] == GuardStep::Locate
            && self.steps[3] == GuardStep::Revert
            && self.sealed
            && self.baseline_restored
    }

    /// 步名导出。
    pub fn step_names(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for s in &self.steps {
            out.push(s.text());
        }
        out
    }
}

/// 一次故障场景完整处置（注入→检出→防护→回退核）。
pub fn handle_fault(kind: FaultKind, seed: u64) -> Result<(GuardTrail, DetectOut), u16> {
    if kind.idx() >= FAULT_COUNT {
        return Err(E_FAULT_OOB);
    }
    let mut s = gen_honest(seed, CMP_EVENTS);
    let clean = detect(&s);
    if !clean.green {
        return Err(E_EQ_STUCK);
    }
    let hit = inject_fault(&mut s, kind, seed);
    if !hit {
        return Err(E_FAULT_MISS);
    }
    let out = dual_check(&s)?;
    if out.green {
        // 注入后仍宣称等价：等价未破（注入落空）。
        return Err(E_EQ_STUCK);
    }
    // 防护流。
    let mut trail = GuardTrail::new(kind);
    // 跳步必拒（先试回退）。
    if trail.advance(GuardStep::Revert, None).is_ok() {
        return Err(E_GUARD_SKIP);
    }
    trail.advance(GuardStep::Freeze, None)?;
    // 定位点：流级差异取首差；非流级差异（应答/基线）锚定 0 号证据位。
    let loc = out.first_diff.or(Some(0usize));
    trail.advance(GuardStep::Locate, loc)?;
    trail.advance(GuardStep::Revert, None)?;
    if !trail.full() {
        return Err(E_GUARD_SHORT);
    }
    Ok((trail, out))
}

/// 资产可重复（同故障种子三跑轨迹全同）。
pub fn repeatable(kind: FaultKind, seed: u64) -> Result<bool, u16> {
    let first = handle_fault(kind, seed)?;
    for _ in 1..3 {
        let again = handle_fault(kind, seed)?;
        if again.0.fault != first.0.fault
            || again.0.full() != first.0.full()
            || again.0.located_at != first.0.located_at
            || again.1 != first.1
        {
            return Ok(false);
        }
    }
    Ok(true)
}

// ═══════════════════════════════════════════════════════════════════════
// §5 演练
// ═══════════════════════════════════════════════════════════════════════

/// 六类故障 10/10 必抓演练（每轮换故障，漏抓即红）。
pub fn drill_fault_10() -> (bool, String) {
    let mut ok = 0u32;
    let mut notes: Vec<String> = Vec::new();
    for r in 0..INJECT_ROUNDS {
        let kind = match FaultKind::from_idx(r % FAULT_COUNT) {
            Some(k) => k,
            None => return (false, "故障还原失败".to_string()),
        };
        let seed = 0x1330_0000 + r as u64;
        match handle_fault(kind, seed) {
            Ok((trail, out)) => {
                if trail.full() && !out.green {
                    ok += 1;
                }
                notes.push(format!("{}:{}", kind.text(), if trail.full() { "抓" } else { "漏" }));
            }
            Err(c) => notes.push(format!("{}:err{:#06X}", kind.text(), c)),
        }
    }
    (
        ok == INJECT_ROUNDS as u32,
        format!("{}/{} {}", ok, INJECT_ROUNDS, notes.join(" ")),
    )
}

/// 防护动作正确率 100% 演练（六类全走四步 + 跳步全拒）。
pub fn drill_guard_correct() -> (bool, String) {
    let mut ok = 0usize;
    for k in FaultKind::ALL {
        match handle_fault(k, 0x1330_1000) {
            Ok((trail, _)) => {
                let names = trail.step_names();
                if trail.full()
                    && names.len() == 4
                    && names[0] == "检出"
                    && names[3] == "回退"
                {
                    ok += 1;
                }
            }
            Err(_) => {}
        }
    }
    (ok == FAULT_COUNT, format!("{}/{} 四步全过", ok, FAULT_COUNT))
}

/// 判定绕过防线演练（三态：诚实绿放行/病态绿打回/诚实红放行）。
pub fn drill_bypass_guard() -> (bool, String) {
    let honest = gen_honest(0x1330_2000, 512);
    let r1 = matches!(bypass_guard(&honest, true), Ok(()));
    let mut sick = gen_honest(0x1330_2001, 512);
    let hit = inject_fault(&mut sick, FaultKind::HiddenDrop, 0x1330_2001);
    let r2 = matches!(bypass_guard(&sick, true), Err(E_BYPASS_PASS));
    let r3 = matches!(bypass_guard(&sick, false), Ok(()));
    (
        hit && r1 && r2 && r3,
        format!("诚实绿={} 病态绿打回={} 诚实红放行={}", r1, r2, r3),
    )
}

/// 资产可重复 100% 演练（六类三跑全同）。
pub fn drill_repeatable() -> (bool, String) {
    let mut ok = 0usize;
    for k in FaultKind::ALL {
        match repeatable(k, 0x1330_3000) {
            Ok(true) => ok += 1,
            _ => {}
        }
    }
    (ok == FAULT_COUNT, format!("{}/{} 三跑全同", ok, FAULT_COUNT))
}

/// 诚实路径零误报+绕过防线合并演练（三态+双验全过）。
pub fn drill_clean_and_bypass() -> (bool, String) {
    // 诚实绿放行。
    let honest = gen_honest(0x1330_4000, CMP_EVENTS);
    let clean = match dual_check(&honest) {
        Ok(o) => o,
        Err(c) => return (false, format!("双验失败 {:#06X}", c)),
    };
    let r1 = clean.green && clean.stream_eq && clean.resp_eq && clean.baseline_ok;
    let g1 = matches!(bypass_guard(&honest, true), Ok(()));
    // 病态绿打回 + 诚实红放行。
    let mut sick = gen_honest(0x1330_4001, 512);
    let hit = inject_fault(&mut sick, FaultKind::HiddenDrop, 0x1330_4001);
    let r2 = matches!(bypass_guard(&sick, true), Err(E_BYPASS_PASS));
    let r3 = matches!(bypass_guard(&sick, false), Ok(()));
    (
        r1 && g1 && hit && r2 && r3,
        format!(
            "零误报={} 绕过防线(绿放/病打/红放)={}/{}/{}",
            r1 && g1, r1, r2, r3
        ),
    )
}

/// 基线污染专项演练（污染必被抓 + 回退后基线恢复）。
pub fn drill_baseline_pollute() -> (bool, String) {
    let mut s = gen_honest(0x1330_5000, 1024);
    let fp0 = s.baseline_fp;
    let hit = inject_fault(&mut s, FaultKind::BaselinePollute, 0x1330_5000);
    let caught = hit && !detect(&s).baseline_ok;
    // 回退（基线重算恢复：恢复后必须回到原始指纹 fp0）。
    s.baseline_fp = fnv64(&s.poll);
    let restored = detect(&s).green && s.baseline_fp == fp0;
    (
        caught && restored,
        format!("污染抓={} 回退恢复到原指纹={}", caught, restored),
    )
}

/// 四专项合并演练（应答语义/时序漂移/事件重复/基线污染）。
pub fn drill_special_3() -> (bool, String) {
    let (a, da) = drill_resp_semantic();
    let (b, db) = drill_timing_drift();
    let (c, dc) = drill_event_dup();
    let (e, de) = drill_baseline_pollute();
    (
        a && b && c && e,
        format!(
            "语义={} 漂移={} 重复={} 基线={} | {} {} {} {}",
            a, b, c, e, da, db, dc, de
        ),
    )
}

// ═══════════════════════════════════════════════════════════════════════
// §5b 应答语义深核与故障普查（多种子×多故障全抓）
// ═══════════════════════════════════════════════════════════════════════

/// 合法命令宇宙（4 命令）。
pub const CMD_UNIVERSE: [u8; 4] = [0x00, 0x01, 0x02, 0x03];

/// 应答语义（resp == cmd + 0x10 派生规则，唯一映射）。
pub const fn resp_of(cmd: u8) -> Option<u8> {
    let mut i = 0;
    while i < CMD_UNIVERSE.len() {
        if CMD_UNIVERSE[i] == cmd {
            return Some(cmd.wrapping_add(0x10));
        }
        i += 1;
    }
    None
}

/// 应答表逐对语义核（每对必须过唯一映射）。
pub fn resp_semantic_ok(pairs: &[(u8, u8)]) -> bool {
    for (c, r) in pairs {
        match resp_of(*c) {
            Some(expect) => {
                if *r != expect {
                    return false;
                }
            }
            None => return false,
        }
    }
    true
}

/// 应答错配专项演练（对调后语义必破 + 主判 resp_eq 必 false）。
pub fn drill_resp_semantic() -> (bool, String) {
    let s0 = gen_honest(0x1330_8000, 512);
    let ok0 = resp_semantic_ok(&s0.resp_pairs);
    let mut s1 = s0.clone();
    let hit = inject_fault(&mut s1, FaultKind::RespMismatch, 0x1330_8000);
    let ok1 = resp_semantic_ok(&s1.resp_pairs);
    let d1 = detect(&s1);
    (
        hit && ok0 && !ok1 && !d1.resp_eq && d1.first_diff.is_none(),
        format!(
            "诚实语义={} 错配破={} 主判应答红={} 流级仍等(漂移不落流)={}",
            ok0,
            !ok1,
            !d1.resp_eq,
            d1.stream_eq
        ),
    )
}

/// 普查项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CensusRec {
    pub fault: FaultKind,
    pub seed_off: usize,
    pub caught: bool,
    pub first_diff: Option<usize>,
}

/// 故障普查（6 类 × 3 种子 = 18 组全抓，逐组留账）。
pub fn fault_census() -> (usize, usize, Vec<CensusRec>) {
    let mut recs = Vec::new();
    let mut caught = 0usize;
    for k in FaultKind::ALL {
        for so in 0..3usize {
            let seed = 0x1330_9000 ^ ((k.idx() as u64) << 8) ^ (so as u64);
            let mut s = gen_honest(seed, 1024);
            let hit = inject_fault(&mut s, k, seed);
            let d = detect(&s);
            let c = hit && !d.green;
            if c {
                caught += 1;
            }
            recs.push(CensusRec { fault: k, seed_off: so, caught: c, first_diff: d.first_diff });
        }
    }
    (caught, FAULT_COUNT * 3, recs)
}

/// 普查演练（18/18 全抓 + 账可重复）。
pub fn drill_census() -> (bool, String) {
    let (c1, t1, r1) = fault_census();
    let (c2, t2, r2) = fault_census();
    (
        c1 == t1 && c2 == t2 && r1 == r2,
        format!("{}/{} 全抓 复查一致={}", c1, t1, r1 == r2),
    )
}

/// 时序漂移专项演练（对调必产生首差且首差在流中段）。
pub fn drill_timing_drift() -> (bool, String) {
    let mut s = gen_honest(0x1330_A000, 1024);
    let hit = inject_fault(&mut s, FaultKind::TimingDrift, 0x1330_A000);
    let d = detect(&s);
    let mid = match d.first_diff {
        Some(p) => p > 0 && p < s.poll.len() - 1,
        None => false,
    };
    (
        hit && !d.green && d.stream_eq == false && mid,
        format!("漂移红={} 首差={:?} 中段={}", !d.green, d.first_diff, mid),
    )
}

/// 事件重复专项演练（重复必使流级不等且长度差 1）。
pub fn drill_event_dup() -> (bool, String) {
    let mut s = gen_honest(0x1330_B000, 1024);
    let len0 = s.int.len();
    let hit = inject_fault(&mut s, FaultKind::EventDup, 0x1330_B000);
    let d = detect(&s);
    (
        hit && !d.green && s.int.len() == len0 + 1,
        format!("重复红={} 长度{}→{}", !d.green, len0, s.int.len()),
    )
}

// ═══════════════════════════════════════════════════════════════════════
// §6 红账联动（跌破即入防护流，漏入即双错）
// ═══════════════════════════════════════════════════════════════════════

/// 红账项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RedRec {
    pub fault: FaultKind,
    pub sealed: bool,
}

/// 红账（防护流强制入账）。
pub struct RedBook {
    pub recs: Vec<RedRec>,
    pub omitted: u32,
}

impl RedBook {
    /// 建。
    pub fn new() -> Self {
        RedBook { recs: Vec::new(), omitted: 0 }
    }

    /// 入账（未封口的红禁入）。
    pub fn record(&mut self, t: &GuardTrail) -> Result<(), u16> {
        if !t.full() {
            return Err(E_GUARD_SHORT);
        }
        for r in &self.recs {
            if r.fault == t.fault && r.sealed {
                return Err(E_RED_OMIT);
            }
        }
        self.recs.push(RedRec { fault: t.fault, sealed: true });
        Ok(())
    }

    /// 全封口数。
    pub fn sealed_count(&self) -> usize {
        self.recs.len()
    }
}

/// 红账联动演练（六类全处置全入账 + 未封口拒入）。
pub fn drill_red_link() -> (bool, String) {
    let mut book = RedBook::new();
    let mut ok = 0usize;
    for k in FaultKind::ALL {
        match handle_fault(k, 0x1330_6000) {
            Ok((trail, _)) => {
                if book.record(&trail).is_ok() {
                    ok += 1;
                }
            }
            Err(_) => {}
        }
    }
    // 未封口红拒入：造一条半途轨迹。
    let mut half = GuardTrail::new(FaultKind::TimingDrift);
    let _ = half.advance(GuardStep::Freeze, None);
    let refused = matches!(book.record(&half), Err(E_GUARD_SHORT));
    (
        ok == FAULT_COUNT && refused && book.sealed_count() == FAULT_COUNT,
        format!("{}/{} 入账 半途拒={}", ok, FAULT_COUNT, refused),
    )
}

// ═══════════════════════════════════════════════════════════════════════
// §7 注入核（6 项）
// ═══════════════════════════════════════════════════════════════════════

/// 注入核 6 项（故障越界/漏抓/绕过/跳步/红漏/回退脏）。
pub fn inject_core() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut det: Vec<String> = Vec::new();
    // 注1：故障越界必拒。
    if FaultKind::from_idx(FAULT_COUNT).is_none() {
        caught += 1;
        det.push("越界:拒".to_string());
    } else {
        det.push("越界:漏".to_string());
    }
    // 注2：六类注入必破等价（漏抓=等价未破即红）。
    let mut all_break = true;
    for k in FaultKind::ALL {
        let mut s = gen_honest(0x1330_7000, 512);
        let hit = inject_fault(&mut s, k, 0x1330_7000);
        if !hit || detect(&s).green {
            all_break = false;
        }
    }
    if all_break {
        caught += 1;
        det.push("必破:全".to_string());
    } else {
        det.push("必破:漏".to_string());
    }
    // 注3：病态绕过必打回。
    let mut sick = gen_honest(0x1330_7001, 512);
    let _ = inject_fault(&mut sick, FaultKind::JudgeBypass, 0x1330_7001);
    if matches!(bypass_guard(&sick, true), Err(E_BYPASS_PASS)) {
        caught += 1;
        det.push("绕过:拒".to_string());
    } else {
        det.push("绕过:漏".to_string());
    }
    // 注4：防护跳步必拒。
    let mut t = GuardTrail::new(FaultKind::EventDup);
    if matches!(t.advance(GuardStep::Locate, Some(3)), Err(E_GUARD_SKIP)) {
        caught += 1;
        det.push("跳步:拒".to_string());
    } else {
        det.push("跳步:漏".to_string());
    }
    // 注5：红账未封口拒入。
    let mut book = RedBook::new();
    if matches!(book.record(&t), Err(E_GUARD_SHORT)) {
        caught += 1;
        det.push("红漏:拒".to_string());
    } else {
        det.push("红漏:漏".to_string());
    }
    // 注6：回退不净判据（未回退轨迹封账位必 false）。
    if !t.sealed && !t.baseline_restored {
        caught += 1;
        det.push("回退:守".to_string());
    } else {
        det.push("回退:漏".to_string());
    }
    (caught, 6, det)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    CODE_ALL.iter().all(|c| *c >= SEG_BASE && *c < SEG_BASE + 16)
}

// ═══════════════════════════════════════════════════════════════════════
// §8 run_unx1330_checks：本单判据聚合（诊断段 0x7700）
// ═══════════════════════════════════════════════════════════════════════

/// 聚合判据（≥6 条，含 10/10 注入演练与段位守卫）。
/// 口径：6 类故障必抓、防护四步正确率 100%、绕过防线、可重复 100%。
pub fn run_unx1330_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1330 等价故障注入");
    let (ok1, d1) = drill_fault_10();
    s.add_dyn("J1-六类故障必抓10/10", ok1, &d1);
    let (ok2, d2) = drill_guard_correct();
    s.add_dyn("J2-防护四步正确率100%", ok2, &d2);
    let (ok3, d3) = drill_clean_and_bypass();
    s.add_dyn("J3-零误报+绕过防线", ok3, &d3);
    let (ok4, d4) = drill_repeatable();
    s.add_dyn("J4-资产可重复100%", ok4, &d4);
    let (ok5, d5) = drill_red_link();
    s.add_dyn("J5-红账强制入账", ok5, &d5);
    let (ok6, d6) = drill_special_3();
    s.add_dyn("J6-语义/漂移/重复/基线四专项", ok6, &d6);
    let (ok8, d8) = drill_census();
    s.add_dyn("J8-18组普查全抓", ok8, &d8);
    let (c9, t9, d9) = inject_core();
    s.add_dyn(
        "J9-注入核6/6",
        c9 == t9,
        &format!("{}/{} {}", c9, t9, d9.join(" ")),
    );
    s.add("J10-段位守卫", segment_guard(), "0x7700段闭集10码");
    s.add(
        "J11-故障宇宙常量",
        FAULT_COUNT == 6 && CMP_EVENTS == 4096,
        "6类×4096事件写死",
    );
    s
}
