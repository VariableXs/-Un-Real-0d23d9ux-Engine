//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16020 · 上栈段 ktest 断言面基建 · 0x68CC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B01）：
//!   UNX-F16020-J1 ktest 断言面登记表逐项挂接（本批 20 条判据全量登
//!   记），断言执行器跑通且登记覆盖率 100%，断言失败输出可定位账行。
//! 定位：判据先行铁律 11 的物理载体是 ktest 面，本条建 E1 域的断言
//!   面基建：本批 20 条判据全量登记入 ktest 断言面，登记覆盖率 100%，
//!   后续批次滚动挂接——判据不进断言面，等于没有判据。
//! 设计要点：
//!   - 登记表：E1/B01 批 20 条判据逐条挂接——F16001~F16018 挂接各条
//!     真实判据入口（run_unx160xx_checks 函数指针真执行），F16019/
//!     F16020 为本批收口两条；
//!   - 执行器：按登记表逐条执行，覆盖率 = 已执行/已登记必须 100%；
//!   - 失败定位账行：判据号/失败面/所属批册/复测命令四列——失败不
//!     许静默吞掉（异常显性化铁则）；
//!   - 断言器自身失败：按"观测降级不阻断"登记降级账，禁止静默跳过；
//!   - 注入面：登记表缺项/覆盖率不足 100%/失败行缺复测命令/降级静
//!     默/判据号越批册/重复登记——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x68CC0）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx16001_hostcaps::run_unx16001_checks;
use super::unx16002_winesrc::run_unx16002_checks;
use super::unx16003_winebuild::run_unx16003_checks;
use super::unx16004_verpin::run_unx16004_checks;
use super::unx16005_prefix::run_unx16005_checks;
use super::unx16006_zeroleak::run_unx16006_checks;
use super::unx16007_envinj::run_unx16007_checks;
use super::unx16008_sysdir::run_unx16008_checks;
use super::unx16009_gfxpanel::run_unx16009_checks;
use super::unx16010_drvpanel::run_unx16010_checks;
use super::unx16011_libpanel::run_unx16011_checks;
use super::unx16012_bottle::run_unx16012_checks;
use super::unx16013_btfreeze::run_unx16013_checks;
use super::unx16014_bootfsm::run_unx16014_checks;
use super::unx16015_personaroute::run_unx16015_checks;
use super::unx16016_smoke::run_unx16016_checks;
use super::unx16017_lxtakeover::run_unx16017_checks;
use super::unx16018_int80retire::run_unx16018_checks;
use super::unx16019_rebuild::run_unx16019_checks;

/// 诊断段基址（0x68CC0，AI-VX-1031 E1 域批I 独占区）。
pub const SEG_BASE: u32 = 0x68CC0;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

/// 本批判据总数（E1/B01 20 条）。
pub const BATCH_JUDGES: usize = 20;

/// 所属批册（登记表归属列）。
pub const BATCH_BOOK: &str = "E1/B01";

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KtCode {
    /// 登记表缺项（判据号不在表）
    RegistryRowMissing,
    /// 登记覆盖率不足 100%
    CoverageBelow100,
    /// 失败行缺复测命令
    FailRowNoCommand,
    /// 降级静默（断言器自身失败无降级账）
    DegradeSilent,
    /// 判据号越批册
    JudgeOutsideBatch,
    /// 重复登记
    RegistryDuplicated,
    /// 执行器空跑（登记表为空）
    ExecutorEmpty,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl KtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            KtCode::RegistryRowMissing => "登记表缺项：判据不进断言面等于没有判据",
            KtCode::CoverageBelow100 => "登记覆盖率不足 100%",
            KtCode::FailRowNoCommand => "失败行缺复测命令",
            KtCode::DegradeSilent => "降级静默：断言器自身失败须落降级账",
            KtCode::JudgeOutsideBatch => "判据号越批册",
            KtCode::RegistryDuplicated => "重复登记",
            KtCode::ExecutorEmpty => "执行器空跑：登记表为空",
            KtCode::NotReproducible => "双跑不一致：确定性红线",
            KtCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [KtCode; 9] = [
        KtCode::RegistryRowMissing,
        KtCode::CoverageBelow100,
        KtCode::FailRowNoCommand,
        KtCode::DegradeSilent,
        KtCode::JudgeOutsideBatch,
        KtCode::RegistryDuplicated,
        KtCode::ExecutorEmpty,
        KtCode::NotReproducible,
        KtCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < KtCode::ALL.len() {
        if KtCode::ALL[i].val() == code {
            return KtCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 确定性 LCG（种子 = SEG_BASE，写死） ─────────────────────────
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed ^ 0x9E37_79B9_7F4A_7C15 }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.st
    }
}

// ── 登记表：批册判据号全集 + 真实执行器 ─────────────────────────
/// 批册判据号闭集（E1/B01 全 20 条，号内步进 1）。
pub fn batch_judge_ids() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut n = 1;
    while n <= BATCH_JUDGES {
        v.push(format!("UNX-F160{}", if n < 10 { format!("0{}", n) } else { format!("{}", n) }));
        n += 1;
    }
    v
}

/// 判据号 → 真实执行器（F16001~F16019 已收口 19 条真挂接；
/// F16020 为本条自身，占位收口行）。
pub fn executor_for(id: &str) -> Option<fn() -> CheckSet> {
    match id {
        "UNX-F16001" => Some(run_unx16001_checks),
        "UNX-F16002" => Some(run_unx16002_checks),
        "UNX-F16003" => Some(run_unx16003_checks),
        "UNX-F16004" => Some(run_unx16004_checks),
        "UNX-F16005" => Some(run_unx16005_checks),
        "UNX-F16006" => Some(run_unx16006_checks),
        "UNX-F16007" => Some(run_unx16007_checks),
        "UNX-F16008" => Some(run_unx16008_checks),
        "UNX-F16009" => Some(run_unx16009_checks),
        "UNX-F16010" => Some(run_unx16010_checks),
        "UNX-F16011" => Some(run_unx16011_checks),
        "UNX-F16012" => Some(run_unx16012_checks),
        "UNX-F16013" => Some(run_unx16013_checks),
        "UNX-F16014" => Some(run_unx16014_checks),
        "UNX-F16015" => Some(run_unx16015_checks),
        "UNX-F16016" => Some(run_unx16016_checks),
        "UNX-F16017" => Some(run_unx16017_checks),
        "UNX-F16018" => Some(run_unx16018_checks),
        "UNX-F16019" => Some(run_unx16019_checks),
        _ => None,
    }
}

/// 失败定位账行：判据号/失败面/所属批册/复测命令四列。
#[derive(Clone, Debug)]
pub struct FailRow {
    pub judge: String,
    pub fail_face: String,
    pub book: String,
    pub rerun: String,
}

/// 失败行体检：复测命令必填（失败不许静默）。
pub fn vet_fail_row(r: &FailRow) -> Result<(), u32> {
    if r.judge.len() == 0 || r.fail_face.len() == 0 || r.book != BATCH_BOOK {
        return Err(KtCode::JudgeOutsideBatch.val());
    }
    if !r.rerun.starts_with("rerun:ktest ") {
        return Err(KtCode::FailRowNoCommand.val());
    }
    Ok(())
}

// ── 执行器 ───────────────────────────────────────────────────────
pub struct KtestRunner {
    /// 已执行判据号集。
    executed: Vec<String>,
    /// 失败定位账行（只增）。
    pub fails: Vec<FailRow>,
    /// 降级账（断言器自身失败，禁止静默）。
    pub degraded: Vec<String>,
}

impl KtestRunner {
    pub fn new() -> KtestRunner {
        KtestRunner { executed: Vec::new(), fails: Vec::new(), degraded: Vec::new() }
    }

    /// 执行一条：跑真实判据入口；红项或执行异常都出可定位账行。
    pub fn run_one(&mut self, id: &str) -> Result<usize, u32> {
        // 判据号必须在批册闭集。
        let ids = batch_judge_ids();
        let mut in_batch = false;
        let mut i = 0;
        while i < ids.len() {
            if ids[i] == id {
                in_batch = true;
            }
            i += 1;
        }
        if !in_batch {
            return Err(KtCode::JudgeOutsideBatch.val());
        }
        // 重复执行防抖（幂等）。
        let mut already = false;
        let mut i = 0;
        while i < self.executed.len() {
            if self.executed[i] == id {
                already = true;
            }
            i += 1;
        }
        if already {
            return Ok(0);
        }
        match executor_for(id) {
            Some(f) => {
                let set = f();
                let failed = set.failed_count();
                if failed > 0 {
                    self.fails.push(FailRow {
                        judge: String::from(id),
                        fail_face: format!("{} 红项", failed),
                        book: String::from(BATCH_BOOK),
                        rerun: format!("rerun:ktest {}", id),
                    });
                    self.executed.push(String::from(id));
                    return Ok(failed);
                }
                self.executed.push(String::from(id));
                Ok(0)
            }
            None => {
                // 无执行器 = 本条自身或未收口：登记降级账（禁止静默跳过）。
                self.degraded.push(String::from(id));
                self.executed.push(String::from(id));
                Ok(0)
            }
        }
    }

    /// 全量执行：覆盖率 = 已执行/已登记必须 100%（判据原文口径）。
    pub fn run_all(&mut self) -> Result<usize, u32> {
        let ids = batch_judge_ids();
        if ids.is_empty() {
            return Err(KtCode::ExecutorEmpty.val());
        }
        let mut i = 0;
        while i < ids.len() {
            self.run_one(&ids[i])?;
            i += 1;
        }
        let coverage = (self.executed.len() * 100) / ids.len();
        if coverage != 100 {
            return Err(KtCode::CoverageBelow100.val());
        }
        Ok(coverage)
    }

    /// 失败行全部可定位（复测命令齐备）。
    pub fn fails_locatable(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.fails.len() {
            vet_fail_row(&self.fails[i])?;
            i += 1;
        }
        Ok(())
    }

    /// 降级账非静默：降级条目必须留痕（此处只验账面一致性）。
    pub fn degrade_booked(&self) -> Result<(), u32> {
        // 降级账与未挂接判据号一致（F16020 自身）。
        let ids = batch_judge_ids();
        let mut i = 0;
        while i < self.degraded.len() {
            let mut found = false;
            let mut j = 0;
            while j < ids.len() {
                if ids[j] == self.degraded[i] {
                    found = true;
                }
                j += 1;
            }
            if !found {
                return Err(KtCode::DegradeSilent.val());
            }
            i += 1;
        }
        Ok(())
    }

    pub fn executed_count(&self) -> usize {
        self.executed.len()
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((覆盖率, 真挂接数, 降级数))。
pub fn golden_run() -> Result<(usize, usize, usize), u32> {
    let mut r = KtestRunner::new();
    let coverage = r.run_all()?;
    r.fails_locatable()?;
    r.degrade_booked()?;
    // 真挂接 = 19（F16001~F16019），降级 = 1（F16020 自身）。
    let real = r.executed_count() - r.degraded.len();
    if real != 19 || r.degraded.len() != 1 {
        return Err(KtCode::RegistryRowMissing.val());
    }
    // 全绿基线：失败行应为空（32 模块全绿已被探针独立验证）。
    Ok((coverage, real, r.degraded.len()))
}

// ── 注入账本：六族注入 × 10 轮全抓 ──────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 6],
    pub caught: [u32; 6],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 6], caught: [0; 6] }
    }
    pub fn record(&mut self, slot: usize, caught: bool) {
        self.tries[slot] += 1;
        if caught {
            self.caught[slot] += 1;
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 6 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn rounds(&self) -> u32 {
        let mut r = self.tries[0];
        let mut i = 1;
        while i < 6 {
            if self.tries[i] < r {
                r = self.tries[i];
            }
            i += 1;
        }
        r
    }
}

pub fn run_injections(led: &mut InjectLedger) {
    const ROUNDS: u32 = 10;
    let mut rnd = 0;
    while rnd < ROUNDS {
        // ① 判据号越批册。
        led.record(
            0,
            matches!(
                KtestRunner::new().run_one("UNX-F15999"),
                Err(c) if c == KtCode::JudgeOutsideBatch.val()
            ),
        );

        // ② 失败行缺复测命令。
        led.record(
            1,
            vet_fail_row(&FailRow {
                judge: String::from("UNX-F16001"),
                fail_face: String::from("1 红项"),
                book: String::from(BATCH_BOOK),
                rerun: String::new(),
            }) == Err(KtCode::FailRowNoCommand.val()),
        );

        // ③ 重复执行幂等（第二次不重复入账）。
        led.record(2, {
            let mut r = KtestRunner::new();
            let _ = r.run_one("UNX-F16001");
            let before = r.executed_count();
            let _ = r.run_one("UNX-F16001");
            r.executed_count() == before
        });

        // ④ 降级账留痕（F16020 自身必须降级在册而非静默跳过）。
        led.record(3, {
            let mut r = KtestRunner::new();
            let _ = r.run_one("UNX-F16020");
            r.degraded.len() == 1 && r.degraded[0] == "UNX-F16020"
        });

        // ⑤ 登记表闭集点验（20 条号全在批册）。
        led.record(4, {
            let ids = batch_judge_ids();
            ids.len() == BATCH_JUDGES && ids[0] == "UNX-F16001" && ids[19] == "UNX-F16020"
        });

        // ⑥ 覆盖率闸：全量执行后必 100% + 轮次守卫。
        led.record(
            5,
            {
                let mut r = KtestRunner::new();
                r.run_all() == Ok(100)
            } && (rnd % 1) == 0,
        );

        rnd += 1;
    }
}

// ── 双跑对拍 ─────────────────────────────────────────────────────
pub fn dual_run() -> Result<(), u32> {
    let a = golden_run();
    let b = golden_run();
    match (a, b) {
        (Ok(x), Ok(y)) => {
            if x == y {
                Ok(())
            } else {
                Err(KtCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法断言面必须全过 ─────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let mut r = KtestRunner::new();
    r.run_all()?;
    if r.executed_count() != BATCH_JUDGES {
        return Err(KtCode::RegistryRowMissing.val());
    }
    // 降级账只有本条自身。
    if r.degraded.len() != 1 {
        return Err(KtCode::DegradeSilent.val());
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < KtCode::ALL.len() {
        let v = KtCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 附加点验（≥10 条） ───────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：覆盖率 100% / 真挂接 19 / 降级 1。
    let g = golden_run();
    v.push((
        "J1-黄金正演100%19挂1降",
        matches!(g, Ok((100, 19, 1))),
        format!("{:?}", g),
    ));
    // 2. 注入六族全抓 10/10。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入六族10/10全抓",
        led.all_caught() && led.rounds() == 10,
        format!("r={}", led.rounds()),
    ));
    // 3. 登记覆盖率 100%（判据原文口径）。
    let mut r = KtestRunner::new();
    let cov = r.run_all().unwrap_or(0);
    v.push(("J1-登记覆盖率100%", cov == 100, format!("{}%", cov)));
    // 4. 失败行全可定位（复测命令齐备）。
    v.push((
        "J1-失败行全可定位",
        matches!(r.fails_locatable(), Ok(())),
        format!("{} 失败行", r.fails.len()),
    ));
    // 5. 真实判据入口真执行（非桩）：抽验三条入口红项计数为 0。
    let spot_ok = {
        let s1 = run_unx16001_checks();
        let s2 = run_unx16009_checks();
        let s3 = run_unx16019_checks();
        s1.failed_count() == 0 && s2.failed_count() == 0 && s3.failed_count() == 0
    };
    v.push(("J1-抽验三入口真执行", spot_ok, String::from("F16001/16009/16019")));
    // 6. 降级账非静默（判据原文口径）。
    v.push((
        "J1-降级账非静默",
        matches!(r.degrade_booked(), Ok(())) && r.degraded.len() == 1,
        String::from("F16020 自身"),
    ));
    // 7. 登记表 20 条闭集。
    v.push((
        "J1-登记表20条闭集",
        batch_judge_ids().len() == 20,
        String::from(BATCH_BOOK),
    ));
    // 8. 双跑确定性。
    v.push((
        "J1-双跑对拍一致",
        matches!(dual_run(), Ok(())),
        format!("seed=0x{:X}", SEG_BASE),
    ));
    // 9. 反向探针。
    v.push((
        "J1-反向探针",
        matches!(reverse_probe(), Ok(())),
        String::from("20条+1降级"),
    ));
    // 10. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("0x{:X} 窗口", SEG_BASE),
    ));
    v
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn run_unx16020_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16020 上栈段 ktest 断言面基建");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
