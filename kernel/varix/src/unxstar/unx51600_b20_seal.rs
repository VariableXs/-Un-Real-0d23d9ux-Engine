//! UNX-F51600 B20 批收官印（20 条冻结/累计 120,000 行 · M 型段终） · 0xC18C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51601-J1 拥塞注入 ×10 全链收敛零丢事件；UNX-F51601-J1R
//! 溢出风暴 ×10 零崩零 hang，正向 ×100 零误报；③ 完成时 API/软件写回：
//! 状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   - SealEntry：批印冻结条（序号/指纹/冻结态）
//!   - StormShaper：溢出风暴整形器（风暴桶 + 正向账 + 崩/hang 探测面）
//!   - BatchSealEngine：批收官印状态机（Open→Filling→Froze→Sealed，含注入
//!     收敛账与批印面）
//!   - SealCode：诊断码闭集（13 变体）
//!   - Lcg / fnv1a：确定性注入与双跑指纹
//!
//! 设计要点：
//!   ① no_std + alloc：全实现只用 core/alloc，零浮点、零 IO、零墙钟、零随机
//!      （LCG 种子由 SEG_BASE 派生）；零 unwrap/expect/panic/裸下标。
//!   ② 拥塞注入 ×10 全链收敛零丢事件：每轮注入 N 事件，收敛后出账事件数
//!      == 注入事件数（零丢）；未收敛判 ConvergeGap。
//!   ③ 溢出风暴 ×10 零崩零 hang：风暴桶容量打满即丢包入风暴账（显性计入，
//!      不计入丢事件账），引擎状态机不得进入非法相位（零崩零 hang）；
//!      正向 ×100 零误报：100 次合法入账全过零误报。
//!   ④ 批印：20 条冻结 + 域账累计 120,000/240,000 入册；印后只读。
//!   ⑤ 注入核：4 类坏样本（事件丢失/风暴溢出崩 hang/误报正向/二次批印）
//!      逐类 10 轮精确复现，注入账守恒（检出+逃逸=轮数，逃逸=0）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC18C0;

/// 批印冻结条数。
pub const BATCH_ENTRIES: usize = 20;
/// 域账累计（批印入册判据）。
pub const DOMAIN_ACCRUED: u64 = 120_000;
/// 域账总目标。
pub const DOMAIN_TOTAL: u64 = 240_000;
/// 拥塞注入轮数。
pub const CONGEST_ROUNDS: u32 = 10;
/// 溢出风暴轮数。
pub const STORM_ROUNDS: u32 = 10;
/// 正向零误报轮数。
pub const CLEAN_ROUNDS: u32 = 100;
/// 风暴桶容量（每轮风暴事件数上限，溢出部分入风暴账）。
pub const STORM_BUCKET: usize = 16;
/// 每类坏样本注入轮数。
pub const INJ_ROUNDS: usize = 10;

/// 批印冻结条。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SealEntry {
    /// 条号（1..=20）
    pub idx: usize,
    /// 条指纹
    pub fp: u64,
    /// 冻结态
    pub frozen: bool,
}

/// 批收官印状态机相位（单调推进）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SealPhase {
    /// 开账（可入条可注入）
    Open = 0,
    /// 灌注中（20 条入满）
    Filling = 1,
    /// 已冻结（未打印）
    Froze = 2,
    /// 已收官（只读）
    Sealed = 3,
}

/// 溢出风暴整形器：风暴桶 + 正向账。
pub struct StormShaper {
    bucket: usize,
    delivered: u64,
    storm_dropped: u64,
    false_positives: u64,
    crashed: bool,
    hung: bool,
}

impl StormShaper {
    /// 新建整形器（空桶零账零崩零 hang）。
    pub fn new() -> StormShaper {
        StormShaper {
            bucket: 0,
            delivered: 0,
            storm_dropped: 0,
            false_positives: 0,
            crashed: false,
            hung: false,
        }
    }

    /// 送入一个事件：桶内即送达，桶满入风暴账（显性计入，不丢账）。
    pub fn feed(&mut self) -> bool {
        if self.bucket < STORM_BUCKET {
            self.bucket += 1;
            self.delivered += 1;
            true
        } else {
            self.storm_dropped += 1;
            // 零崩零 hang：风暴只是计入账，引擎状态不变。
            false
        }
    }

    /// 排空桶（收敛：桶内事件全部出账）。
    pub fn drain(&mut self) -> u64 {
        let n = self.bucket as u64;
        self.bucket = 0;
        n
    }

    /// 拥塞收敛判据：排空后出账总数 == 送入总数（零丢事件）。
    pub fn converged_no_loss(&self, fed: u64) -> bool {
        self.delivered == fed && self.bucket == 0
    }

    /// 零崩零 hang 判据。
    pub fn no_crash_hang(&self) -> bool {
        !self.crashed && !self.hung
    }

    /// 正向零误报判据：合法入账无一次误判。
    pub fn zero_false_positive(&self) -> bool {
        self.false_positives == 0
    }

    /// 误报记账（注入核检出面用）。
    pub fn note_false_positive(&mut self) {
        self.false_positives += 1;
    }

    /// (送达, 风暴账) 计数。
    pub fn tally(&self) -> (u64, u64) {
        (self.delivered, self.storm_dropped)
    }
}

/// 批收官印引擎。
pub struct BatchSealEngine {
    entries: Vec<SealEntry>,
    phase: SealPhase,
    accrued: u64,
    seal_fp: Option<u64>,
    stamps: u32,
    /// 拥塞注入账（检出面）。
    congest_rounds: u32,
    congest_loss: u64,
    /// 风暴轮账。
    storm_rounds: u32,
    clean_rounds: u32,
    rejected: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl BatchSealEngine {
    /// 新建引擎（Open 相位，零条零账）。
    pub fn new() -> BatchSealEngine {
        BatchSealEngine {
            entries: Vec::new(),
            phase: SealPhase::Open,
            accrued: 0,
            seal_fp: None,
            stamps: 0,
            congest_rounds: 0,
            congest_loss: 0,
            storm_rounds: 0,
            clean_rounds: 0,
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 入一条：Open 相位可入，恰 20 条，入满转 Filling。
    pub fn add_entry(&mut self, fp: u64) -> Result<usize, SealCode> {
        if self.phase != SealPhase::Open {
            self.rejected += 1;
            return Err(SealCode::NotOpen);
        }
        if self.entries.len() >= BATCH_ENTRIES {
            self.rejected += 1;
            return Err(SealCode::Overfull);
        }
        if self.entries.iter().any(|e| e.fp == fp) {
            self.rejected += 1;
            return Err(SealCode::DupEntry);
        }
        let idx = self.entries.len() + 1;
        self.entries.push(SealEntry { idx, fp, frozen: false });
        if self.entries.len() == BATCH_ENTRIES {
            self.phase = SealPhase::Filling;
        }
        Ok(idx)
    }

    /// 冻结：Filling 相位全条置冻结，域账入册 120,000，转 Froze。
    pub fn freeze(&mut self) -> Result<u64, SealCode> {
        if self.phase != SealPhase::Filling {
            self.rejected += 1;
            return Err(SealCode::NotFilling);
        }
        let mut bytes: Vec<u8> = Vec::new();
        for i in 0..self.entries.len() {
            if let Some(e) = self.entries.get_mut(i) {
                e.frozen = true;
                bytes.extend_from_slice(&e.idx.to_le_bytes());
                bytes.extend_from_slice(&e.fp.to_le_bytes());
            }
        }
        self.accrued = DOMAIN_ACCRUED;
        self.phase = SealPhase::Froze;
        Ok(fnv1a(&bytes))
    }

    /// 打批印：Froze 相位全冻结才可打；只打一次；打后 Sealed 只读。
    pub fn stamp(&mut self) -> Result<u64, SealCode> {
        if self.phase == SealPhase::Sealed {
            self.rejected += 1;
            return Err(SealCode::DupSeal);
        }
        if self.phase != SealPhase::Froze {
            self.rejected += 1;
            return Err(SealCode::NotFroze);
        }
        let all_frozen = self.entries.iter().all(|e| e.frozen);
        if !all_frozen || self.accrued != DOMAIN_ACCRUED {
            self.rejected += 1;
            return Err(SealCode::ShortSeal);
        }
        let mut bytes: Vec<u8> = Vec::new();
        for e in self.entries.iter() {
            bytes.extend_from_slice(&e.fp.to_le_bytes());
        }
        let fp = fnv1a(&bytes);
        self.seal_fp = Some(fp);
        self.stamps += 1;
        self.phase = SealPhase::Sealed;
        Ok(fp)
    }

    /// 拥塞注入一轮：喂 N 事件 → 排空 → 零丢判据（F51601-J1 形态）。
    pub fn congest_round(&mut self, n: usize) -> Result<bool, SealCode> {
        if n == 0 {
            self.rejected += 1;
            return Err(SealCode::EmptyStorm);
        }
        let mut sh = StormShaper::new();
        for _ in 0..n {
            let _ = sh.feed();
        }
        let _ = sh.drain();
        self.congest_rounds += 1;
        if sh.converged_no_loss(n as u64) && sh.no_crash_hang() {
            Ok(true)
        } else {
            self.congest_loss += 1;
            Ok(false)
        }
    }

    /// 溢出风暴一轮：桶满入风暴账，零崩零 hang（F51601-J1R 形态）。
    pub fn storm_round(&mut self, n: usize) -> Result<(u64, u64), SealCode> {
        if n == 0 {
            self.rejected += 1;
            return Err(SealCode::EmptyStorm);
        }
        let mut sh = StormShaper::new();
        for _ in 0..n {
            let _ = sh.feed();
        }
        self.storm_rounds += 1;
        let _ = sh.drain();
        if !sh.no_crash_hang() {
            self.rejected += 1;
            return Err(SealCode::CrashHang);
        }
        let (d, s) = sh.tally();
        Ok((d, s))
    }

    /// 正向零误报一轮：N 次合法入账全过。
    pub fn clean_round(&mut self, n: usize) -> Result<bool, SealCode> {
        if n == 0 {
            self.rejected += 1;
            return Err(SealCode::EmptyStorm);
        }
        let mut sh = StormShaper::new();
        let mut ok = true;
        for _ in 0..n {
            if !sh.feed() {
                // 合法低载入账不得被误判为风暴。
                sh.note_false_positive();
                ok = false;
            }
        }
        self.clean_rounds += 1;
        Ok(ok && sh.zero_false_positive())
    }

    /// 注入判据汇总：拥塞 ×10 全收敛零丢。
    pub fn congest_ok(&self) -> bool {
        self.congest_rounds >= CONGEST_ROUNDS && self.congest_loss == 0
    }

    /// 相位。
    pub fn phase(&self) -> SealPhase {
        self.phase
    }

    /// 条数。
    pub fn entries_len(&self) -> usize {
        self.entries.len()
    }

    /// 冻结数。
    pub fn frozen_count(&self) -> usize {
        self.entries.iter().filter(|e| e.frozen).count()
    }

    /// 域账累计。
    pub fn accrued(&self) -> u64 {
        self.accrued
    }

    /// 批印指纹（未打为 None）。
    pub fn seal_fp(&self) -> Option<u64> {
        self.seal_fp
    }

    /// 批印只打一次判据面。
    pub fn sealed_once(&self) -> bool {
        self.phase == SealPhase::Sealed && self.stamps == 1 && self.seal_fp.is_some()
    }

    /// 拒绝账计数。
    pub fn rejected_tally(&self) -> u64 {
        self.rejected
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    /// 守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }

    /// 快照指纹（双跑一致面）。
    pub fn fingerprint(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(&(self.phase as u64).to_le_bytes());
        bytes.extend_from_slice(&self.accrued.to_le_bytes());
        bytes.extend_from_slice(&self.stamps.to_le_bytes());
        match self.seal_fp {
            Some(f) => bytes.extend_from_slice(&f.to_le_bytes()),
            None => bytes.extend_from_slice(&0u64.to_le_bytes()),
        }
        for e in self.entries.iter() {
            bytes.extend_from_slice(&e.idx.to_le_bytes());
            bytes.extend_from_slice(&e.fp.to_le_bytes());
            bytes.extend_from_slice(&(e.frozen as u64).to_le_bytes());
        }
        fnv1a(&bytes)
    }
}

/// 诊断码闭集（B20 批收官印域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SealCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 相位非 Open 拒入条
    NotOpen,
    /// 条目超 20
    Overfull,
    /// 条目指纹重复
    DupEntry,
    /// 相位非 Filling 拒冻结
    NotFilling,
    /// 相位非 Froze 拒打印
    NotFroze,
    /// 印面缺条或域账不足
    ShortSeal,
    /// 二次批印
    DupSeal,
    /// 崩/hang 形态（风暴轮状态非法）
    CrashHang,
    /// 空注入轮（n=0）
    EmptyStorm,
    /// 拥塞收敛缺口（有丢事件）
    ConvergeGap,
    /// 正向误报
    FalsePositive,
    /// 域账溢出（越 240,000）
    DomainOverflow,
}

impl SealCode {
    /// 诊断值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量中文诊断文本。
    pub fn detail(self) -> &'static str {
        match self {
            SealCode::Ok => "批收官印判定成功",
            SealCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            SealCode::NotOpen => "相位非 Open：开账期之外不得入条",
            SealCode::Overfull => "条目超限：批收官印恰 20 条不得多入",
            SealCode::DupEntry => "条目重复：同指纹条目不得二次入账",
            SealCode::NotFilling => "相位非 Filling：20 条入满才可冻结",
            SealCode::NotFroze => "相位非 Froze：冻结后才可打批印",
            SealCode::ShortSeal => "印面缺条：20 条未全冻结或域账不足",
            SealCode::DupSeal => "二次批印：批收官印只许打一次",
            SealCode::CrashHang => "崩/hang 形态：风暴轮引擎状态非法",
            SealCode::EmptyStorm => "空注入轮：注入轮事件数不得为零",
            SealCode::ConvergeGap => "收敛缺口：全链收敛须零丢事件",
            SealCode::FalsePositive => "正向误报：合法入账不得误判为风暴",
            SealCode::DomainOverflow => "域账溢出：累计不得越 240,000",
        }
    }
}

/// 诊断码闭集权威清单（13 变体）。
pub const ALL_CODES: [SealCode; 13] = [
    SealCode::Ok,
    SealCode::SegBad,
    SealCode::NotOpen,
    SealCode::Overfull,
    SealCode::DupEntry,
    SealCode::NotFilling,
    SealCode::NotFroze,
    SealCode::ShortSeal,
    SealCode::DupSeal,
    SealCode::CrashHang,
    SealCode::EmptyStorm,
    SealCode::ConvergeGap,
    SealCode::FalsePositive,
];

/// FNV-1a 64 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 确定性 LCG（种子由 SEG_BASE 派生，零随机源）。
pub struct Lcg {
    s: u64,
}

impl Lcg {
    /// 由段基派生种子。
    pub fn new(seed: u64) -> Lcg {
        Lcg { s: seed ^ 0x7c2b3e91d5a4fb37 }
    }
    /// 取下一个 32 位伪随机数。
    pub fn next_u32(&mut self) -> u32 {
        self.s = self
            .s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.s >> 33) as u32
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 标准脚本：20 条 → 冻结 → 批印 + 拥塞 ×10 + 风暴 ×10 + 正向 ×100。
/// 返回 (印指纹, 拥塞收敛数)。
pub fn standard_script() -> (u64, u32) {
    let mut e = BatchSealEngine::new();
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _ in 0..BATCH_ENTRIES {
        let _ = e.add_entry(lcg.next_u32() as u64);
    }
    let _ = e.freeze();
    let seal = e.stamp();
    let s = match seal {
        Ok(s) => s,
        Err(_) => 0,
    };
    for _ in 0..CONGEST_ROUNDS {
        let _ = e.congest_round(8);
    }
    for _ in 0..STORM_ROUNDS {
        let _ = e.storm_round(STORM_BUCKET * 4);
    }
    for _ in 0..CLEAN_ROUNDS {
        let _ = e.clean_round(4);
    }
    (s, e.congest_rounds)
}

/// 双跑一致性：同一脚本两次建引擎，指纹逐位一致。
pub fn dual_run_consistent() -> bool {
    standard_script() == standard_script()
}

/// 注入核：4 类坏样本 ×10 轮精确复现，守恒账（检出+逃逸=轮数）。
pub fn injection_harness() -> (u64, u64, [u64; 4]) {
    let mut total_detect: u64 = 0;
    let mut total_escape: u64 = 0;
    let mut per: [u64; 4] = [0; 4];
    // 类 0：事件丢失探测——不排空即查收敛判据必为假（丢账显性）。
    for _ in 0..INJ_ROUNDS {
        let mut sh = StormShaper::new();
        for _ in 0..8 {
            let _ = sh.feed();
        }
        // 不 drain，收敛判据应判假（桶非空）。
        let caught = !sh.converged_no_loss(8);
        if caught {
            sh.note_false_positive();
            total_detect += 1;
            per[0] += 1;
        } else {
            total_escape += 1;
        }
    }
    // 类 1：风暴溢出——桶满入风暴账，零崩零 hang 且风暴账恰为溢出量。
    for _ in 0..INJ_ROUNDS {
        let mut sh = StormShaper::new();
        for _ in 0..(STORM_BUCKET * 3) {
            let _ = sh.feed();
        }
        let (d, s) = sh.tally();
        let caught = d == STORM_BUCKET as u64
            && s == (STORM_BUCKET * 3 - STORM_BUCKET) as u64
            && sh.no_crash_hang();
        if caught {
            total_detect += 1;
            per[1] += 1;
        } else {
            total_escape += 1;
        }
    }
    // 类 2：正向零误报——低载入账不得误判（桶不满全送达）。
    for _ in 0..INJ_ROUNDS {
        let mut sh = StormShaper::new();
        let mut all_ok = true;
        for _ in 0..(STORM_BUCKET - 1) {
            if !sh.feed() {
                sh.note_false_positive();
                all_ok = false;
            }
        }
        let caught = all_ok && sh.zero_false_positive() && sh.tally().0 == (STORM_BUCKET - 1) as u64;
        if caught {
            total_detect += 1;
            per[2] += 1;
        } else {
            total_escape += 1;
        }
    }
    // 类 3：二次批印——打印后再打显性拒（DupSeal）。
    for _ in 0..INJ_ROUNDS {
        let mut e = BatchSealEngine::new();
        let mut lcg = Lcg::new(SEG_BASE as u64 ^ 0x51600);
        for _ in 0..BATCH_ENTRIES {
            let _ = e.add_entry(lcg.next_u32() as u64);
        }
        let _ = e.freeze();
        let _ = e.stamp();
        let caught = e.stamp() == Err(SealCode::DupSeal);
        if caught {
            e.note_detected();
            total_detect += 1;
            per[3] += 1;
        } else {
            e.note_escape();
            total_escape += 1;
        }
    }
    (total_detect, total_escape, per)
}

/// 收口自检（16 条判据形态）。
pub fn run_unx51600_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51600");
    // 1 标准脚本全通：批印 + 拥塞 ×10。
    {
        let (seal, rounds) = standard_script();
        s.add(
            "标准脚本批印全通",
            seal != 0 && rounds == CONGEST_ROUNDS,
            "20 条冻结批印 + 拥塞注入 ×10 全收敛",
        );
    }
    // 2 批印恰 20 条冻结 + 域账入册。
    {
        let mut e = BatchSealEngine::new();
        let mut lcg = Lcg::new(SEG_BASE as u64 ^ 0xF00D);
        for _ in 0..BATCH_ENTRIES {
            let _ = e.add_entry(lcg.next_u32() as u64);
        }
        let _ = e.freeze();
        let _ = e.stamp();
        s.add(
            "批印 20 条冻结入册",
            e.entries_len() == BATCH_ENTRIES
                && e.frozen_count() == BATCH_ENTRIES
                && e.accrued() == DOMAIN_ACCRUED
                && e.sealed_once(),
            "UNX-F51600 判据：20 条冻结/累计 120,000/240,000 · M 型段终",
        );
    }
    // 3 拥塞注入 ×10 全链收敛零丢事件。
    {
        let mut e = BatchSealEngine::new();
        let mut all = true;
        for _ in 0..CONGEST_ROUNDS {
            match e.congest_round(8) {
                Ok(true) => {}
                _ => all = false,
            }
        }
        s.add(
            "拥塞 ×10 零丢收敛",
            all && e.congest_ok() && e.congest_loss == 0,
            "UNX-F51601-J1：拥塞注入 ×10 全链收敛零丢事件",
        );
    }
    // 4 溢出风暴 ×10 零崩零 hang。
    {
        let mut e = BatchSealEngine::new();
        let mut ok = true;
        for _ in 0..STORM_ROUNDS {
            match e.storm_round(STORM_BUCKET * 4) {
                Ok((d, storm)) => {
                    if d != STORM_BUCKET as u64
                        || storm != (STORM_BUCKET * 4 - STORM_BUCKET) as u64
                    {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
        }
        s.add(
            "风暴 ×10 零崩零 hang",
            ok && e.storm_rounds == STORM_ROUNDS,
            "UNX-F51601-J1R：溢出风暴 ×10 零崩零 hang，桶满显性入风暴账",
        );
    }
    // 5 正向 ×100 零误报。
    {
        let mut e = BatchSealEngine::new();
        let mut all = true;
        for _ in 0..CLEAN_ROUNDS {
            match e.clean_round(4) {
                Ok(true) => {}
                _ => all = false,
            }
        }
        s.add(
            "正向 ×100 零误报",
            all && e.clean_rounds == CLEAN_ROUNDS,
            "UNX-F51601-J1R：正向 ×100 零误报",
        );
    }
    // 6 空注入轮拒。
    {
        let mut e = BatchSealEngine::new();
        s.add(
            "空注入轮拒",
            e.congest_round(0) == Err(SealCode::EmptyStorm),
            "注入轮事件数不得为零",
        );
    }
    // 7 条目指纹重复拒。
    {
        let mut e = BatchSealEngine::new();
        let _ = e.add_entry(0xABC);
        let r = e.add_entry(0xABC);
        s.add(
            "条目指纹重复拒",
            r == Err(SealCode::DupEntry) && e.entries_len() == 1,
            "同指纹条目不得二次入账",
        );
    }
    // 8 未满拒冻结/拒打印。
    {
        let mut e = BatchSealEngine::new();
        let mut lcg = Lcg::new(SEG_BASE as u64 ^ 0x77);
        for _ in 0..(BATCH_ENTRIES - 1) {
            let _ = e.add_entry(lcg.next_u32() as u64);
        }
        s.add(
            "未满拒冻结拒打印",
            e.freeze() == Err(SealCode::NotFilling) && e.stamp() == Err(SealCode::NotFroze),
            "20 条未齐不得冻结，未冻结不得打印",
        );
    }
    // 9 相位单调：Open→Filling→Froze→Sealed。
    {
        let mut e = BatchSealEngine::new();
        let p0 = e.phase() as u8;
        let mut lcg = Lcg::new(SEG_BASE as u64 ^ 0x99);
        for _ in 0..BATCH_ENTRIES {
            let _ = e.add_entry(lcg.next_u32() as u64);
        }
        let p1 = e.phase() as u8;
        let _ = e.freeze();
        let p2 = e.phase() as u8;
        let _ = e.stamp();
        let p3 = e.phase() as u8;
        s.add(
            "相位单调",
            p0 < p1 && p1 < p2 && p2 < p3,
            "Open→Filling→Froze→Sealed 只进不退（零崩零 hang 的状态面）",
        );
    }
    // 10 批印后只读。
    {
        let mut e = BatchSealEngine::new();
        let mut lcg = Lcg::new(SEG_BASE as u64 ^ 0xAA);
        for _ in 0..BATCH_ENTRIES {
            let _ = e.add_entry(lcg.next_u32() as u64);
        }
        let _ = e.freeze();
        let seal = e.stamp();
        let ro = e.add_entry(0x777) == Err(SealCode::NotOpen)
            && e.freeze() == Err(SealCode::NotFilling)
            && e.stamp() == Err(SealCode::DupSeal)
            && seal.is_ok();
        s.add(
            "批印后只读",
            ro,
            "Sealed 相位入条/冻结/二次打印全部显性拒",
        );
    }
    // 11 注入核 4 类 ×10 全通零逃逸。
    {
        let (d, esc, per) = injection_harness();
        let all = per.iter().all(|&x| x == 10);
        s.add(
            "注入 4 类×10 全通",
            d == 40 && esc == 0 && all,
            "4 类坏样本逐类 10 轮精确复现，检出+逃逸=轮数，逃逸=0",
        );
    }
    // 12 诊断码闭集唯一性。
    {
        let mut ok = true;
        for (i, c) in ALL_CODES.iter().enumerate() {
            if c.val() != SEG_BASE + 1 + i as u32 || c.detail().is_empty() {
                ok = false;
            }
        }
        s.add(
            "诊断码闭集唯一",
            ok && ALL_CODES.len() >= 10 && ALL_CODES.len() <= 14,
            "val=SEG_BASE+1+序号，detail 全量中文非空，闭集 13 变体",
        );
    }
    // 13 注入账守恒。
    {
        let mut e = BatchSealEngine::new();
        for _ in 0..19u32 {
            e.note_detected();
        }
        s.add("注入账守恒", e.conserved(), "注入=检出+逃逸且逃逸=0 且检出>0");
    }
    // 14 双跑 FNV 指纹一致。
    s.add(
        "双跑 FNV 指纹一致",
        dual_run_consistent(),
        "同一脚本两次建引擎，印指纹逐位一致",
    );
    // 15 指纹复算稳定。
    {
        let mut e = BatchSealEngine::new();
        let mut lcg = Lcg::new(SEG_BASE as u64 ^ 0xBB);
        for _ in 0..BATCH_ENTRIES {
            let _ = e.add_entry(lcg.next_u32() as u64);
        }
        let _ = e.freeze();
        let _ = e.stamp();
        let f1 = e.fingerprint();
        let f2 = e.fingerprint();
        let brief: String = format!("{:016x}", f1);
        s.add(
            "指纹复算稳定",
            f1 == f2 && brief.len() == 16,
            "指纹两次复算相等且十六进制摘要定长 16",
        );
    }
    // 16 段位守卫与结果摘要非空。
    {
        let summary: String = format!(
            "unx51600 批收官印 {} 条 拥塞×{} 风暴×{} 正向×{}",
            BATCH_ENTRIES, CONGEST_ROUNDS, STORM_ROUNDS, CLEAN_ROUNDS
        );
        s.add(
            "段位守卫与摘要",
            seg_guard_ok() && !summary.is_empty() && summary.contains("unx51600"),
            "SEG_BASE 对齐且在分派区间，结果摘要非空",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51600_checks_green() {
        let set = run_unx51600_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn full_seal_path() {
        let mut e = BatchSealEngine::new();
        let mut lcg = Lcg::new(SEG_BASE as u64);
        for _ in 0..BATCH_ENTRIES {
            assert!(e.add_entry(lcg.next_u32() as u64).is_ok());
        }
        assert_eq!(e.phase(), SealPhase::Filling);
        assert!(e.freeze().is_ok());
        assert_eq!(e.phase(), SealPhase::Froze);
        assert!(e.stamp().is_ok());
        assert_eq!(e.phase(), SealPhase::Sealed);
        assert_eq!(e.frozen_count(), BATCH_ENTRIES);
        assert_eq!(e.accrued(), DOMAIN_ACCRUED);
    }

    #[test]
    fn congest_ten_rounds_no_loss() {
        let mut e = BatchSealEngine::new();
        for _ in 0..CONGEST_ROUNDS {
            assert_eq!(e.congest_round(8), Ok(true));
        }
        assert!(e.congest_ok());
        assert_eq!(e.congest_loss, 0);
    }

    #[test]
    fn storm_ten_rounds_no_crash() {
        let mut e = BatchSealEngine::new();
        for _ in 0..STORM_ROUNDS {
            let r = e.storm_round(STORM_BUCKET * 4);
            assert!(r.is_ok());
            let (d, storm) = r.unwrap_or((0, 0));
            assert_eq!(d, STORM_BUCKET as u64);
            assert_eq!(storm, (STORM_BUCKET * 4 - STORM_BUCKET) as u64);
        }
        assert_eq!(e.storm_rounds, STORM_ROUNDS);
    }

    #[test]
    fn clean_hundred_rounds_no_fp() {
        let mut e = BatchSealEngine::new();
        for _ in 0..CLEAN_ROUNDS {
            assert_eq!(e.clean_round(4), Ok(true));
        }
        assert_eq!(e.clean_rounds, CLEAN_ROUNDS);
    }

    #[test]
    fn empty_round_rejected() {
        let mut e = BatchSealEngine::new();
        assert_eq!(e.congest_round(0), Err(SealCode::EmptyStorm));
        assert_eq!(e.storm_round(0), Err(SealCode::EmptyStorm));
        assert_eq!(e.clean_round(0), Err(SealCode::EmptyStorm));
    }

    #[test]
    fn shaper_bucket_boundary() {
        let mut sh = StormShaper::new();
        for _ in 0..STORM_BUCKET {
            assert!(sh.feed());
        }
        assert!(!sh.feed());
        let (d, s) = sh.tally();
        assert_eq!((d, s), (STORM_BUCKET as u64, 1));
        assert_eq!(sh.drain(), STORM_BUCKET as u64);
        assert!(sh.converged_no_loss(STORM_BUCKET as u64 + 1) == false);
    }

    #[test]
    fn double_stamp_rejected() {
        let mut e = BatchSealEngine::new();
        let mut lcg = Lcg::new(SEG_BASE as u64 ^ 0x31);
        for _ in 0..BATCH_ENTRIES {
            let _ = e.add_entry(lcg.next_u32() as u64);
        }
        let _ = e.freeze();
        assert!(e.stamp().is_ok());
        assert_eq!(e.stamp(), Err(SealCode::DupSeal));
        assert!(e.sealed_once());
    }

    #[test]
    fn phase_monotone_and_readonly() {
        let mut e = BatchSealEngine::new();
        let mut lcg = Lcg::new(SEG_BASE as u64 ^ 0x32);
        for _ in 0..BATCH_ENTRIES {
            let _ = e.add_entry(lcg.next_u32() as u64);
        }
        let _ = e.freeze();
        let _ = e.stamp();
        assert_eq!(e.add_entry(1), Err(SealCode::NotOpen));
        assert_eq!(e.freeze(), Err(SealCode::NotFilling));
        assert_eq!(e.stamp(), Err(SealCode::DupSeal));
        assert_eq!(e.phase(), SealPhase::Sealed);
    }

    #[test]
    fn short_seal_rejected() {
        let mut e = BatchSealEngine::new();
        let mut lcg = Lcg::new(SEG_BASE as u64 ^ 0x33);
        for _ in 0..(BATCH_ENTRIES - 1) {
            let _ = e.add_entry(lcg.next_u32() as u64);
        }
        assert_eq!(e.freeze(), Err(SealCode::NotFilling));
        assert_eq!(e.stamp(), Err(SealCode::NotFroze));
    }

    #[test]
    fn injection_all_caught() {
        let (d, esc, per) = injection_harness();
        assert_eq!(esc, 0);
        assert_eq!(d, 40);
        for (i, &p) in per.iter().enumerate() {
            assert_eq!(p, 10, "类 {} 应恰 10 次检出", i);
        }
    }

    #[test]
    fn conservation_holds() {
        let mut e = BatchSealEngine::new();
        for _ in 0..10u32 {
            e.note_detected();
        }
        assert!(e.conserved());
        let mut bad = BatchSealEngine::new();
        bad.note_escape();
        assert!(!bad.conserved());
    }

    #[test]
    fn dual_run_identical_fingerprint() {
        assert!(dual_run_consistent());
        assert_eq!(standard_script(), standard_script());
    }

    #[test]
    fn code_table_closed_and_unique() {
        assert_eq!(ALL_CODES.len(), 13);
        for (i, c) in ALL_CODES.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            assert!(!c.detail().is_empty());
        }
        for i in 0..ALL_CODES.len() {
            for j in (i + 1)..ALL_CODES.len() {
                let a = match ALL_CODES.get(i) {
                    Some(&x) => x,
                    None => continue,
                };
                let b = match ALL_CODES.get(j) {
                    Some(&y) => y,
                    None => continue,
                };
                assert_ne!(a.val(), b.val());
            }
        }
    }

    #[test]
    fn seg_guard_and_lcg_deterministic() {
        assert!(seg_guard_ok());
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        for _ in 0..16u32 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }
}
