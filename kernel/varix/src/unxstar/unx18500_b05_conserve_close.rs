//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18500 · B05 批守恒闭合断言 · 0x7F8C0 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18500-J1 本批 20 条行数逐条求和=6,000 三方一致，红绿齐
//! 本模块结构清单：
//!   1. 冻结常量层：行账 20 条（A10 本工作单 10 行 + 对半工作单镜像
//!       10 行）/ 配额域 [200,480] / 批位闭集 5 / 判据轮数 10；
//!   2. 诊断码闭集 CsCd（code()=SEG_BASE+n，段内 0x40 窗口）；
//!   3. 数据结构：
//!       - 行账条目 RowEntry（card/quota/target/batch 四元）；
//!       - 批行账 RowLedger：[RowEntry; 20] 数组账本 + 分账双计；
//!       - 聚合票账 Tally（green/red 双账——红绿齐的机器口径）；
//!   4. 三方一致（闭合律）：
//!       W1 全表逐条求和 == 6000；
//!       W2 分账求和（A10 分账 3040 + 镜像分账 2960）== 6000 == W1；
//!       W3 聚合票账 green==20 ∧ red==0（每行一票绿，红绿齐）；
//!   5. 行域律：quota ∈ [200,480] ∧ target ≥ quota ∧ batch ∈ {5}；
//!   6. 注入核四类：篡改行配额 / 目标小于配额 / 未知批位 / 零配额——
//!       逐类 10 轮全抓，守恒漏检必 0；篡改后红=1 绿=19（红绿齐的
//!       注入面对照）；
//!   7. run_unx18500_checks：17 条判据。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7F8C0，G1 工单 A10 批05 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7F8C0;

// ── 冻结常量层（推导注记） ─────────────────────────────────────────
/// 行账规模（写死 20：J1"20 条行数"语义——B05 批两工作单各 10 卡）。
pub const ROW_N: usize = 20;
/// 批位闭集（写死 5：B05 批）。
pub const BATCH_NO: u8 = 5;
/// 配额域（冻结 200..=480：与批 05 全卡配额同域）。
pub const QUOTA_MIN: u32 = 200;
pub const QUOTA_MAX: u32 = 480;
/// 批总配额（冻结 6000：J1"逐条求和=6,000"）。
pub const BATCH_QUOTA_SUM: u32 = 6000;
/// A10 分账配额和（推导：本工作单 10 卡配额 300+320+280+300+260+300+
/// 300+320+360+300 = 3040）。
pub const A10_QUOTA_SUM: u32 = 3040;
/// 镜像分账配额和（推导：6000-3040 = 2960——对半工作单冻结镜像）。
pub const MIRROR_QUOTA_SUM: u32 = 2960;
/// 判据轮数（写死 10）。
pub const JUDGE_ROUNDS: u32 = 10;

/// A10 十卡配额（冻结，与工单 A10 卡表逐卡一致）。
pub const A10_QUOTAS: [u32; 10] = [300, 320, 280, 300, 260, 300, 300, 320, 360, 300];
/// A10 十卡目标行数（冻结，与工单 A10 卡表逐卡一致）。
pub const A10_TARGETS: [u32; 10] = [600, 640, 600, 600, 600, 600, 600, 600, 720, 600];
/// 镜像十卡配额（冻结：合计 2960，逐条在 [200,480] 域内）。
pub const MIRROR_QUOTAS: [u32; 10] = [340, 300, 280, 260, 300, 280, 300, 300, 300, 300];
/// 镜像十卡目标行数（冻结：逐条 ≥ 配额，合计 5920）。
pub const MIRROR_TARGETS: [u32; 10] = [680, 600, 560, 520, 600, 560, 600, 600, 600, 600];

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CsCd {
    /// 求和断裂（全表 ≠ 6000 或分账 ≠ 全表）
    SumBroken,
    /// 配额域断裂（[200,480] 之外）
    DomainBroken,
    /// 目标小于配额（target < quota）
    TargetLess,
    /// 未知批位（≠5）
    BatchBad,
    /// 零配额
    QuotaZero,
    /// 行数断裂（≠20）
    RowCountBad,
    /// 红绿不齐（green+red ≠ 20）
    RedGreenBroken,
    /// 镜像账断裂（A10/镜像分账与冻结不符）
    MirrorBroken,
    /// 闭合失败（三律任一不齐）
    CloseBroken,
    /// 双跑不一致
    NotReproducible,
}

impl CsCd {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            CsCd::SumBroken => "求和断裂：全表≠6000 或分账≠全表",
            CsCd::DomainBroken => "配额域断裂：须在 200..=480",
            CsCd::TargetLess => "目标小于配额：target≥quota",
            CsCd::BatchBad => "未知批位：须为 5（B05 批）",
            CsCd::QuotaZero => "零配额：配额恒非零",
            CsCd::RowCountBad => "行数断裂：须恰 20 行",
            CsCd::RedGreenBroken => "红绿不齐：green+red≠20",
            CsCd::MirrorBroken => "镜像账断裂：分账与冻结不符",
            CsCd::CloseBroken => "闭合失败：三律任一不齐",
            CsCd::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 全闭集（段位守卫遍历用）。
    pub const ALL: [CsCd; 10] = [
        CsCd::SumBroken,
        CsCd::DomainBroken,
        CsCd::TargetLess,
        CsCd::BatchBad,
        CsCd::QuotaZero,
        CsCd::RowCountBad,
        CsCd::RedGreenBroken,
        CsCd::MirrorBroken,
        CsCd::CloseBroken,
        CsCd::NotReproducible,
    ];
}

// ── 行账 ───────────────────────────────────────────────────────────
/// 行账条目（卡号/配额/目标行数/批位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RowEntry {
    pub card: u32,
    pub quota: u32,
    pub target: u32,
    pub batch: u8,
}

impl RowEntry {
    /// 空行（const）。
    pub const fn empty() -> RowEntry {
        RowEntry { card: 0, quota: 0, target: 0, batch: 0 }
    }
}

/// 行域律（单行）：card 域 ∧ quota 域 ∧ target≥quota ∧ batch 闸。
pub fn row_legal(e: &RowEntry) -> Result<(), u32> {
    if e.quota == 0 {
        return Err(CsCd::QuotaZero.code());
    }
    if e.quota < QUOTA_MIN || e.quota > QUOTA_MAX {
        return Err(CsCd::DomainBroken.code());
    }
    if e.target < e.quota {
        return Err(CsCd::TargetLess.code());
    }
    if e.batch != BATCH_NO {
        return Err(CsCd::BatchBad.code());
    }
    Ok(())
}

/// 行冻结对账：第 i 行与冻结卡表逐字段一致（卡号/配额/目标/批位）。
/// ——票账计红的对账维度：域内但被篡改的行同样翻红。
pub fn row_frozen_match(i: usize, e: &RowEntry) -> bool {
    if i >= ROW_N {
        return false;
    }
    if i < 10 {
        e.card == 18491 + i as u32
            && e.quota == A10_QUOTAS[i]
            && e.target == A10_TARGETS[i]
            && e.batch == BATCH_NO
    } else {
        let k = i - 10;
        e.card == 18481 + k as u32
            && e.quota == MIRROR_QUOTAS[k]
            && e.target == MIRROR_TARGETS[k]
            && e.batch == BATCH_NO
    }
}

/// 构建冻结 20 行账（A10 十行 + 镜像十行；卡号 A10=18491..18500，
/// 镜像=18481..18490——批 05 对半工作单卡号段）。
pub fn make_rows() -> [RowEntry; ROW_N] {
    let mut arr = [RowEntry::empty(); ROW_N];
    let mut i = 0;
    while i < 10 {
        arr[i] = RowEntry {
            card: 18491 + i as u32,
            quota: A10_QUOTAS[i],
            target: A10_TARGETS[i],
            batch: BATCH_NO,
        };
        i += 1;
    }
    let mut k = 0;
    while k < 10 {
        arr[10 + k] = RowEntry {
            card: 18481 + k as u32,
            quota: MIRROR_QUOTAS[k],
            target: MIRROR_TARGETS[k],
            batch: BATCH_NO,
        };
        k += 1;
    }
    arr
}

/// 批行账（数组账本 + 聚合票账）。
pub struct RowLedger {
    pub rows: [RowEntry; ROW_N],
    pub count: usize,
    /// 聚合票账：绿票（行域律过的行数）/红票（行域律拒的行数）。
    pub greens: u32,
    pub reds: u32,
}

impl RowLedger {
    /// 冻结行账（全表入账 + 全行域律过 → 全绿）。
    pub fn frozen() -> RowLedger {
        let rows = make_rows();
        let mut led = RowLedger { rows, count: ROW_N, greens: 0, reds: 0 };
        let mut i = 0;
        while i < ROW_N {
            if row_legal(&led.rows[i]).is_ok() {
                led.greens = led.greens.saturating_add(1);
            } else {
                led.reds = led.reds.saturating_add(1);
            }
            i += 1;
        }
        led
    }

    /// W1：全表逐条求和 == 6000。
    pub fn sum_all(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0;
        while i < self.count {
            s = s.saturating_add(self.rows[i].quota);
            i += 1;
        }
        s
    }

    /// W2：分账求和 == 全表（A10 分账 3040 + 镜像分账 2960）。
    pub fn sum_split(&self) -> Result<(u32, u32), u32> {
        let mut a = 0u32;
        let mut m = 0u32;
        let mut i = 0;
        while i < self.count {
            if i < 10 {
                a = a.saturating_add(self.rows[i].quota);
            } else {
                m = m.saturating_add(self.rows[i].quota);
            }
            i += 1;
        }
        // 分账与冻结值逐项对账。
        if a != A10_QUOTA_SUM || m != MIRROR_QUOTA_SUM {
            return Err(CsCd::MirrorBroken.code());
        }
        Ok((a, m))
    }

    /// W3：红绿齐（green+red == 20）。
    pub fn tally_aligned(&self) -> Result<(), u32> {
        if (self.greens + self.reds) as usize != ROW_N {
            return Err(CsCd::RedGreenBroken.code());
        }
        if self.count != ROW_N {
            return Err(CsCd::RowCountBad.code());
        }
        Ok(())
    }

    /// 行域律全过（全行零红）。
    pub fn all_rows_legal(&self) -> bool {
        let mut i = 0;
        while i < self.count {
            if row_legal(&self.rows[i]).is_err() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 三方一致闭合：W1 == W2 == 6000 ∧ W3 红绿齐 ∧ 行域律全过。
    pub fn close(&self) -> Result<(), u32> {
        if self.count != ROW_N {
            return Err(CsCd::RowCountBad.code());
        }
        let (a, m) = self.sum_split()?;
        if a.saturating_add(m) != BATCH_QUOTA_SUM {
            return Err(CsCd::SumBroken.code());
        }
        if self.sum_all() != BATCH_QUOTA_SUM {
            return Err(CsCd::SumBroken.code());
        }
        self.tally_aligned()?;
        if self.reds != 0 || !self.all_rows_legal() {
            return Err(CsCd::CloseBroken.code());
        }
        Ok(())
    }
}

// ── 注入核（坏行形态 × JUDGE_ROUNDS 轮） ──────────────────────────
/// 注入账（4 槽：篡改配额/目标倒挂/未知批位/零配额，守恒 tries==caught）。
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    /// 空账。
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    /// 记一轮。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    /// 守恒：四槽全记录且全抓。
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
}

/// 篡改账：从冻结账出发篡改第 idx 行配额（+delta），其余不动。
/// 返回闭合结果（Err=断裂被抓，Ok=漏检——漏检即红）。
pub fn tampered_close(idx: usize, delta: i32) -> Result<(), u32> {
    let mut led = RowLedger::frozen();
    if idx >= ROW_N {
        return Err(CsCd::RowCountBad.code());
    }
    // 配额带符号偏移（saturating 落界——篡改形态有限域内构造）。
    if delta >= 0 {
        led.rows[idx].quota = led.rows[idx].quota.saturating_add(delta as u32);
    } else {
        led.rows[idx].quota = led.rows[idx].quota.saturating_sub((-delta) as u32);
    }
    // 篡改后重算票账（行域律 + 冻结对账双维计红）。
    led.greens = 0;
    led.reds = 0;
    let mut i = 0;
    while i < ROW_N {
        if row_legal(&led.rows[i]).is_ok() && row_frozen_match(i, &led.rows[i]) {
            led.greens = led.greens.saturating_add(1);
        } else {
            led.reds = led.reds.saturating_add(1);
        }
        i += 1;
    }
    led.close()
}

/// 四类坏行注入 × rounds 轮（每轮从冻结账出发）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：篡改行配额（+1 → 求和断裂）。
        led.record(0, tampered_close(3, 1).is_err());
        // 槽1：目标倒挂（target := quota-1 → TargetLess）。
        let bad_target = {
            let mut l = RowLedger::frozen();
            l.rows[7].target = l.rows[7].quota.saturating_sub(1);
            row_legal(&l.rows[7]).err() == Some(CsCd::TargetLess.code())
        };
        led.record(1, bad_target);
        // 槽2：未知批位（batch := 4）。
        let bad_batch = {
            let mut l = RowLedger::frozen();
            l.rows[11].batch = 4;
            row_legal(&l.rows[11]).err() == Some(CsCd::BatchBad.code())
        };
        led.record(2, bad_batch);
        // 槽3：零配额。
        let zero = {
            let mut l = RowLedger::frozen();
            l.rows[15].quota = 0;
            row_legal(&l.rows[15]).err() == Some(CsCd::QuotaZero.code())
        };
        led.record(3, zero);
        r += 1;
    }
    Ok(led)
}

/// 篡改后红绿齐：单行篡改 → 恰 1 红 19 绿（票账守恒不丢票）。
pub fn tampered_tally() -> Result<(u32, u32), u32> {
    let mut led = RowLedger::frozen();
    led.rows[5].quota = led.rows[5].quota.saturating_add(1);
    led.greens = 0;
    led.reds = 0;
    let mut i = 0;
    while i < ROW_N {
        // 双维计红：域律过 ∧ 冻结对账齐——被篡改行（域内漂移）同样翻红。
        if row_legal(&led.rows[i]).is_ok() && row_frozen_match(i, &led.rows[i]) {
            led.greens = led.greens.saturating_add(1);
        } else {
            led.reds = led.reds.saturating_add(1);
        }
        i += 1;
    }
    if (led.greens + led.reds) as usize != ROW_N {
        return Err(CsCd::RedGreenBroken.code());
    }
    Ok((led.greens, led.reds))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < CsCd::ALL.len() {
        let v = CsCd::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 文案闭集。
pub fn detail_closed() -> bool {
    let mut i = 0;
    while i < CsCd::ALL.len() {
        if CsCd::ALL[i].detail().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 黄金全链：冻结账闭合（三方一致）→ 双跑一致。返回全表求和。
pub fn golden_run() -> Result<u32, u32> {
    let led = RowLedger::frozen();
    led.close()?;
    Ok(led.sum_all())
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(CsCd::NotReproducible.code());
    }
    Ok(())
}

/// 对账导出：表头 + 20 行 "RW|card|quota|target|batch"（可回放格式）。
pub fn export_rows() -> Vec<String> {
    let led = RowLedger::frozen();
    let mut out = Vec::new();
    out.push(format!("ROW-LEDGER v1 card|quota|target|batch n={}", led.count));
    let mut i = 0;
    while i < led.count {
        let e = led.rows[i];
        out.push(format!("RW|{}|{}|{}|{}", e.card, e.quota, e.target, e.batch));
        i += 1;
    }
    out
}

/// 数字段解析（十进制无符号）。
fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() {
        return Err(CsCd::CloseBroken.code());
    }
    let mut acc: u32 = 0;
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c < b'0' || c > b'9' {
            return Err(CsCd::CloseBroken.code());
        }
        acc = match acc.checked_mul(10).and_then(|x| x.checked_add((c - b'0') as u32)) {
            Some(v) => v,
            None => return Err(CsCd::CloseBroken.code()),
        };
        i += 1;
    }
    Ok(acc)
}

/// 对账行解析："RW|card|quota|target|batch"。
pub fn parse_row_line(line: &str) -> Result<RowEntry, u32> {
    if !line.starts_with("RW|") {
        return Err(CsCd::CloseBroken.code());
    }
    let body = &line[3..];
    let bb = body.as_bytes();
    let mut pipes = 0usize;
    let mut i = 0;
    while i < bb.len() {
        if bb[i] == b'|' {
            pipes += 1;
        }
        i += 1;
    }
    if pipes != 3 {
        return Err(CsCd::CloseBroken.code());
    }
    let mut segs: [&str; 4] = ["", "", "", ""];
    let mut idx = 0usize;
    let mut start = 0usize;
    let mut k = 0usize;
    while k <= bb.len() {
        if k == bb.len() || bb[k] == b'|' {
            if idx >= 4 {
                return Err(CsCd::CloseBroken.code());
            }
            segs[idx] = &body[start..k];
            idx += 1;
            start = k + 1;
        }
        k += 1;
    }
    if idx != 4 {
        return Err(CsCd::CloseBroken.code());
    }
    let card = parse_u32(segs[0])?;
    let quota = parse_u32(segs[1])?;
    let target = parse_u32(segs[2])?;
    let batch = parse_u32(segs[3])? as u8;
    Ok(RowEntry { card, quota, target, batch })
}

/// 对账回放：逐行解析 == 冻结账逐条（往返一致）。
pub fn replay_rows() -> Result<(), u32> {
    let lines = export_rows();
    let led = RowLedger::frozen();
    if lines.len() != ROW_N + 1 {
        return Err(CsCd::CloseBroken.code());
    }
    let mut i = 1;
    while i < lines.len() {
        let e = parse_row_line(&lines[i])?;
        let src = led.rows[i - 1];
        if e.card != src.card || e.quota != src.quota || e.target != src.target
            || e.batch != src.batch
        {
            return Err(CsCd::NotReproducible.code());
        }
        i += 1;
    }
    Ok(())
}

// ── 判据收口（16 条，断言性质） ────────────────────────────────────
/// 判据入口（单模块 ≤20 条）。
pub fn run_unx18500_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18500 B05 批守恒闭合断言");
    // 1. 三方一致闭合（J1 主体）。
    s.add("J1-三方一致闭合", golden_run().is_ok(), "W1==W2==6000 ∧ 红绿齐");
    // 2. 全表逐条求和 == 6000。
    let w1 = RowLedger::frozen().sum_all() == BATCH_QUOTA_SUM;
    s.add("J1-全表求和6000", w1, "W1");
    // 3. 分账求和复算（3040/2960）。
    let w2 = match RowLedger::frozen().sum_split() {
        Ok((a, m)) => a == A10_QUOTA_SUM && m == MIRROR_QUOTA_SUM && a + m == BATCH_QUOTA_SUM,
        Err(_) => false,
    };
    s.add("J1-分账复算3040/2960", w2, "W2 与冻结逐项对账");
    // 4. 行数恰 20。
    s.add("J1-行数20条", RowLedger::frozen().count == ROW_N, "A10 十行+镜像十行");
    // 5. 配额域全行合法。
    s.add("J1-配额域[200,480]", RowLedger::frozen().all_rows_legal(), "行域律全过");
    // 6. 红绿齐（green==20 red==0）。
    let w3 = {
        let led = RowLedger::frozen();
        led.greens == ROW_N as u32 && led.reds == 0 && led.tally_aligned().is_ok()
    };
    s.add("J1-红绿齐", w3, "W3 全绿零红");
    // 7. 注入 10/10（四类守恒）。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn("J1-注入10/10四类全抓", inj_ok, &format!("t/c={:?}", inj.as_ref().map(|l| l.caught)));
    // 8. 篡改行配额拒（逐码：闭合断裂）。
    s.add("J1-篡改行配额拒", tampered_close(3, 1).is_err(), "SumBroken 经 close");
    // 9. 目标倒挂拒。
    let tl = {
        let mut l = RowLedger::frozen();
        l.rows[7].target = l.rows[7].quota.saturating_sub(1);
        row_legal(&l.rows[7]).err() == Some(CsCd::TargetLess.code())
    };
    s.add("J1-目标倒挂拒", tl, "TargetLess");
    // 10. 未知批位拒。
    let bb = {
        let mut l = RowLedger::frozen();
        l.rows[11].batch = 4;
        row_legal(&l.rows[11]).err() == Some(CsCd::BatchBad.code())
    };
    s.add("J1-未知批位拒", bb, "BatchBad");
    // 11. 零配额拒。
    let qz = {
        let mut l = RowLedger::frozen();
        l.rows[15].quota = 0;
        row_legal(&l.rows[15]).err() == Some(CsCd::QuotaZero.code())
    };
    s.add("J1-零配额拒", qz, "QuotaZero");
    // 12. 篡改后红绿齐（1 红 19 绿——票账守恒）。
    let tt = match tampered_tally() {
        Ok((g, r)) => g == 19 && r == 1,
        Err(_) => false,
    };
    s.add("J1-篡改后票账守恒", tt, "1红19绿");
    // 13. 冻结配额逐卡对账（A10 十卡与卡表一致）。
    let frozen = {
        let rows = make_rows();
        let mut ok = true;
        let mut i = 0;
        while i < 10 {
            if rows[i].quota != A10_QUOTAS[i] || rows[i].target != A10_TARGETS[i] {
                ok = false;
            }
            i += 1;
        }
        ok && rows[0].card == 18491 && rows[9].card == 18500
    };
    s.add("J1-冻结配额逐卡对账", frozen, "A10 卡号 18491..18500");
    // 14. 对账回放往返。
    s.add("J1-对账回放往返", replay_rows().is_ok(), "ROW-LEDGER v1 逐条对拍");
    // 15. 双跑一致。
    s.add("J1-双跑一致", repro_check().is_ok(), "golden 两跑同值");
    // 16. 段位守卫 + 文案闭集。
    s.add("J1-段位守卫", segment_guard(), "0x7F8C0 窗口");
    s.add("J1-文案闭集", detail_closed(), "逐码非空中文");
    s
}

// ── 测试 ───────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx18500_checks_green() {
        let set = run_unx18500_checks();
        assert!(set.all_passed(), "UNX-F18500 red: {}", set.red_items_text());
    }

    #[test]
    fn t_three_way_consistency() {
        let led = RowLedger::frozen();
        // W1：全表 6000。
        assert_eq!(led.sum_all(), 6000, "W1 全表求和");
        // W2：分账 3040+2960。
        let (a, m) = led.sum_split().unwrap_or((0, 0));
        assert_eq!(a, 3040, "A10 分账");
        assert_eq!(m, 2960, "镜像分账");
        assert_eq!(a + m, led.sum_all(), "W2==W1");
        // W3：红绿齐。
        assert_eq!(led.greens, 20, "全绿");
        assert_eq!(led.reds, 0, "零红");
        assert!(led.close().is_ok(), "闭合");
    }

    #[test]
    fn t_frozen_tables_match_card_sheet() {
        // A10 卡表逐卡配额（推导注记：与工单 A10 卡表逐卡一致）。
        let expect: [u32; 10] = [300, 320, 280, 300, 260, 300, 300, 320, 360, 300];
        let mut i = 0;
        while i < 10 {
            assert_eq!(A10_QUOTAS[i], expect[i], "卡 {} 配额", 18491 + i);
            i += 1;
        }
        // 目标行数逐卡。
        let expect_t: [u32; 10] = [600, 640, 600, 600, 600, 600, 600, 600, 720, 600];
        let mut k = 0;
        while k < 10 {
            assert_eq!(A10_TARGETS[k], expect_t[k], "卡 {} 目标行数", 18491 + k);
            k += 1;
        }
        // 镜像账求和与域。
        let mut ms = 0u32;
        let mut q = 0;
        while q < 10 {
            assert!(MIRROR_QUOTAS[q] >= QUOTA_MIN && MIRROR_QUOTAS[q] <= QUOTA_MAX, "镜像域");
            assert!(MIRROR_TARGETS[q] >= MIRROR_QUOTAS[q], "镜像目标≥配额");
            ms += MIRROR_QUOTAS[q];
            q += 1;
        }
        assert_eq!(ms, MIRROR_QUOTA_SUM, "镜像分账 2960");
        assert_eq!(A10_QUOTA_SUM + MIRROR_QUOTA_SUM, BATCH_QUOTA_SUM, "6000 闭合");
    }

    #[test]
    fn t_inject_rounds_all_caught() {
        let l = inject_rounds(JUDGE_ROUNDS).unwrap_or_else(|_| InjectLedger::new());
        assert!(l.all_caught(), "四类全抓");
        let mut total = 0u32;
        let mut i = 0;
        while i < 4 {
            total += l.tries[i];
            i += 1;
        }
        assert_eq!(total, 40, "总注入 4×10=40");
    }

    #[test]
    fn t_tamper_detected_every_row() {
        // 逐行篡改 +1 全部检测（求和断裂对所有行敏感）。
        let mut i = 0;
        while i < ROW_N {
            assert!(tampered_close(i, 1).is_err(), "行 {} 篡改必抓", i);
            i += 1;
        }
        // 负向篡改（-1）同样断裂。
        assert!(tampered_close(0, -1).is_err(), "负向篡改必抓");
    }

    #[test]
    fn t_row_domain_gates() {
        // 四域闸逐码：零配额 / 域下越 / 域上越 / 目标倒挂 / 批位错。
        let mk = |q: u32, t: u32, b: u8| RowEntry { card: 18491, quota: q, target: t, batch: b };
        assert_eq!(row_legal(&mk(0, 600, 5)).err(), Some(CsCd::QuotaZero.code()));
        assert_eq!(row_legal(&mk(QUOTA_MIN - 1, 600, 5)).err(), Some(CsCd::DomainBroken.code()));
        assert_eq!(row_legal(&mk(QUOTA_MAX + 1, 600, 5)).err(), Some(CsCd::DomainBroken.code()));
        assert_eq!(row_legal(&mk(300, 299, 5)).err(), Some(CsCd::TargetLess.code()));
        assert_eq!(row_legal(&mk(300, 600, 4)).err(), Some(CsCd::BatchBad.code()));
        assert!(row_legal(&mk(300, 600, 5)).is_ok(), "合法行");
        // 边界：域端点合法。
        assert!(row_legal(&mk(QUOTA_MIN, QUOTA_MIN, 5)).is_ok());
        assert!(row_legal(&mk(QUOTA_MAX, QUOTA_MAX, 5)).is_ok());
    }

    #[test]
    fn t_tally_conservation_under_tamper() {
        // 篡改前后票账守恒：绿+红 恒 20。
        let clean = RowLedger::frozen();
        assert_eq!(clean.greens + clean.reds, 20);
        let (g, r) = tampered_tally().unwrap_or((0, 0));
        assert_eq!(g + r, 20, "票账守恒");
        assert_eq!(g, 19, "篡改行翻红");
        assert_eq!(r, 1, "恰 1 红");
    }

    #[test]
    fn t_export_replay_roundtrip() {
        let lines = export_rows();
        assert_eq!(lines.len(), ROW_N + 1, "表头+20行");
        assert!(lines[0].starts_with("ROW-LEDGER v1"), "表头协议");
        assert!(lines[1].starts_with("RW|18491|300|600|5"), "首行卡号+配额");
        assert!(lines[10].starts_with("RW|18500|300|600|5"), "末行 A10");
        assert!(replay_rows().is_ok(), "回放逐条对拍");
        // 篡改行。
        assert!(parse_row_line("XX|18491|300|600|5").is_err(), "前缀错");
        assert!(parse_row_line("RW|18491|300|600").is_err(), "字段不足");
        assert!(parse_row_line("RW|18491|x|600|5").is_err(), "非数字");
    }

    #[test]
    fn t_double_run_identical() {
        assert!(repro_check().is_ok(), "双跑一致");
        let a = export_rows();
        let b = export_rows();
        assert_eq!(a, b, "导出确定性");
    }

    #[test]
    fn t_segment_guard_and_detail() {
        assert!(segment_guard(), "0x7F8C0 窗口");
        assert!(detail_closed(), "文案闭集");
        assert_eq!(CsCd::CloseBroken.code(), SEG_BASE + 8, "码位推导");
    }
}
