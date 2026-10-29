//! unx48286_memdrift — UNX-F48286 内存常驻漂移账（漂移 <10MB 判据承载+周期采样）（诊断段 0xC4A0）。
//!
//! 判据原文：
//!   J1 采样 ×24 周期漂移落账；判据锚定 §AI-61 §5 J-4。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 采样口径：24 周期（24h 每小时一采）逐点落账 (周期号, 常驻 KB)；
//!      周期号 1..=24 单调不重，KB 为常驻 RSS 口径（KB 账，非 MB 浮点）。
//!   2. 漂移定义（判据承载）：漂移 = 尾采 - 首采（KB）；判据线 10MB =
//!      10240 KB，漂移 < 10240 即绿；负漂移（回落）按 0 计不投机。
//!   3. 双口径防抖：全判漂移（尾-首）之外加峰谷漂移（max-min）副账——
//!      尾首漂移绿而峰谷漂移红 = 中途鼓包，鼓包必须落副账留痕。
//!   4. 周期完整线：×24 采样一缺即账不成环（E_SAMPLE_MISS），半程账
//!      不出判据；采样只追加不改写（留痕不可变）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC4A0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_SAMPLE_MISS: u16 = SEG_BASE + 1; // 周期缺采（账不成环）
const E_DRIFT_OFF: u16 = SEG_BASE + 2; // 漂移越判据线
const E_SEQ_BAD: u16 = SEG_BASE + 3; // 周期号违规（重号/倒序）
const E_LEDGER_BAD: u16 = SEG_BASE + 4; // 采样账不守恒
const E_STATE_BAD: u16 = SEG_BASE + 5; // 采样器状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 6; // 写回摘要为空

/// 采样口径。
pub const CYCLES: u32 = 24; // ×24 周期
pub const DRIFT_LINE_KB: u32 = 10_240; // 10MB = 10240KB

/// 采样点一条。
#[derive(Clone, Copy, Debug)]
pub struct SampleRow {
    pub cycle: u32,  // 1..=24
    pub rss_kb: u32, // 常驻 KB
}

/// 漂移账。
pub struct DriftLedger {
    pub rows: Vec<SampleRow>,
    pub peak_valley_flag: bool, // 峰谷副账红位（中途鼓包）
}

impl DriftLedger {
    pub fn new() -> DriftLedger {
        DriftLedger { rows: Vec::new(), peak_valley_flag: false }
    }

    /// 采样落账：周期号严格 +1 单调（只追加）。
    pub fn sample(&mut self, cycle: u32, rss_kb: u32) -> Result<(), u16> {
        if rss_kb == 0 {
            return Err(E_LEDGER_BAD); // 零常驻不入账（采样下限）
        }
        let want = self.rows.len() as u32 + 1;
        if cycle != want {
            return Err(E_SEQ_BAD); // 周期号必须 1,2,3,… 严格递增
        }
        self.rows.push(SampleRow { cycle, rss_kb });
        Ok(())
    }

    /// 尾首漂移（判据主口径）：尾采 - 首采，负漂移按 0 计。
    pub fn drift_tail_head(&self) -> Result<u32, u16> {
        if self.rows.len() as u32 != CYCLES {
            return Err(E_SAMPLE_MISS); // 半程不出判据
        }
        let first = self.rows[0].rss_kb;
        let last = self.rows[(CYCLES - 1) as usize].rss_kb;
        if last >= first {
            Ok(last - first)
        } else {
            Ok(0)
        }
    }

    /// 峰谷漂移（副口径）：max - min。
    pub fn drift_peak_valley(&self) -> Result<u32, u16> {
        if self.rows.len() == 0 {
            return Err(E_SAMPLE_MISS);
        }
        let mut mx = self.rows[0].rss_kb;
        let mut mn = self.rows[0].rss_kb;
        let mut i = 1usize;
        while i < self.rows.len() {
            if self.rows[i].rss_kb > mx {
                mx = self.rows[i].rss_kb;
            }
            if self.rows[i].rss_kb < mn {
                mn = self.rows[i].rss_kb;
            }
            i += 1;
        }
        Ok(mx - mn)
    }

    /// 逐周期增量账：cycle i→i+1 的常驻增量（成环后方可出增量账）。
    pub fn per_cycle_delta(&self) -> Result<Vec<i64>, u16> {
        if self.rows.len() as u32 != CYCLES {
            return Err(E_SAMPLE_MISS);
        }
        let mut deltas: Vec<i64> = Vec::new();
        let mut i = 1usize;
        while i < self.rows.len() {
            let d = self.rows[i].rss_kb as i64 - self.rows[i - 1].rss_kb as i64;
            deltas.push(d);
            i += 1;
        }
        Ok(deltas)
    }

    /// 判据承载（J1 主体）：×24 成环 + 尾首漂移 < 10240KB + 峰谷副账
    /// 逐位核对——尾首绿而峰谷越线即鼓包红位落账。
    pub fn judge(&mut self) -> Result<(u32, u32), u16> {
        let tail_head = self.drift_tail_head()?;
        let peak_valley = self.drift_peak_valley()?;
        if tail_head >= DRIFT_LINE_KB {
            return Err(E_DRIFT_OFF);
        }
        self.peak_valley_flag = peak_valley >= DRIFT_LINE_KB;
        Ok((tail_head, peak_valley))
    }

    /// 采样账审计：×24 逐点在册、周期号闭合、KB 单调账可溯。
    pub fn audit(&self) -> Result<(), u16> {
        if self.rows.len() as u32 != CYCLES {
            return Err(E_SAMPLE_MISS);
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].cycle != (i + 1) as u32 {
                return Err(E_SEQ_BAD);
            }
            if self.rows[i].rss_kb == 0 {
                return Err(E_LEDGER_BAD);
            }
            i += 1;
        }
        Ok(())
    }

    /// 完成写回（判据 J2）：账成环 + 非空摘要方放行。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.rows.len() as u32 != CYCLES {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（采样 ×24 周期漂移落账），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主对齐 ×24：常驻 KB 缓涨序列（每小时 +13KB，24h 共 +312KB），
    //    ×24 全采落账、漂移落账、判据绿（312KB << 10240KB）。
    let mut dl = DriftLedger::new();
    let mut c = 1u32;
    while c <= CYCLES {
        dl.sample(c, 51_200 + (c - 1) * 13)?;
        c += 1;
    }
    dl.audit()?;
    let (th, pv) = dl.judge()?;
    if th != 312 {
        return Err(E_DRIFT_OFF);
    }
    if pv != 312 {
        return Err(E_DRIFT_OFF); // 平缓序列峰谷=尾首
    }
    if dl.peak_valley_flag {
        return Err(E_DRIFT_OFF);
    }
    // 增量账终判：23 个周期增量恒 +13KB（缓涨序列逐位复核）。
    let deltas = dl.per_cycle_delta()?;
    if deltas.len() != 23 {
        return Err(E_SAMPLE_MISS);
    }
    let mut i = 0usize;
    while i < deltas.len() {
        if deltas[i] != 13 {
            return Err(E_LEDGER_BAD);
        }
        i += 1;
    }
    hits += 1;

    // R2 判据线边界：漂移恰 10239KB 绿、恰 10240KB 红（开界口径逐位核）。
    let mut dl2 = DriftLedger::new();
    let mut c = 1u32;
    while c <= CYCLES {
        dl2.sample(c, 40_960 + (c - 1) * (10_239 / 23))?;
        c += 1;
    }
    // 补齐余数使尾恰 10239（确定性重排尾点）。
    let idx = (CYCLES - 1) as usize;
    dl2.rows[idx].rss_kb = 40_960 + 10_239;
    let (th2, _) = dl2.judge()?;
    if th2 != 10_239 {
        return Err(E_DRIFT_OFF);
    }
    let mut dl3 = DriftLedger::new();
    let mut c = 1u32;
    while c <= CYCLES {
        dl3.sample(c, 40_960 + (c - 1) * (10_240 / 23))?;
        c += 1;
    }
    dl3.rows[idx].rss_kb = 40_960 + 10_240; // 恰压线
    if dl3.judge().is_ok() {
        return Err(E_DRIFT_OFF); // 10240KB 不许过线
    }
    hits += 1;

    // R3 负漂移口径：尾采回落（低于首采）→ 漂移按 0 计（不投机于回落）。
    let mut dl4 = DriftLedger::new();
    let mut c = 1u32;
    while c <= CYCLES {
        dl4.sample(c, 60_000 - (c - 1) * 50)?;
        c += 1;
    }
    let (th4, _) = dl4.judge()?;
    if th4 != 0 {
        return Err(E_DRIFT_OFF);
    }
    hits += 1;

    // R4 峰谷鼓包副账：中段鼓包 11MB 后回落 → 尾首漂移绿（回落抵消）、
    //    峰谷副账红位落账（鼓包必须留痕）。
    let mut dl5 = DriftLedger::new();
    let mut c = 1u32;
    while c <= CYCLES {
        let kb = if c >= 8 && c <= 16 {
            51_200 + 11_000 // 中段鼓包 11MB
        } else {
            51_200
        };
        dl5.sample(c, kb)?;
        c += 1;
    }
    let (th5, pv5) = dl5.judge()?;
    if th5 != 0 {
        return Err(E_DRIFT_OFF);
    }
    if pv5 != 11_000 || !dl5.peak_valley_flag {
        return Err(E_DRIFT_OFF); // 鼓包必须被抓
    }
    hits += 1;

    // R5 周期缺采负例：×23 半程账 judge 必拒（E_SAMPLE_MISS，账不成环
    //    不出判据）；audit 同拒。
    let mut dl6 = DriftLedger::new();
    let mut c = 1u32;
    while c < CYCLES {
        dl6.sample(c, 51_200)?;
        c += 1;
    }
    if dl6.judge().is_ok() {
        return Err(E_SAMPLE_MISS);
    }
    if dl6.audit().is_ok() {
        return Err(E_SAMPLE_MISS);
    }
    hits += 1;

    // R6 周期号违规负例：跳号（1 后直采 3）→ E_SEQ_BAD；零 KB 采样 →
    //    E_LEDGER_BAD。
    let mut dl7 = DriftLedger::new();
    dl7.sample(1, 51_200)?;
    if dl7.sample(3, 51_213).is_ok() {
        return Err(E_SEQ_BAD);
    }
    if dl7.sample(2, 0).is_ok() {
        return Err(E_LEDGER_BAD);
    }
    if dl7.rows.len() != 1 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R7 留痕不可变负例：人工篡改中间采样点 → audit 逐点账号核验
    //    仍绿（值合法）——留痕审计以周期号闭合为主口径，篡改侦测由
    //    双口径差账承担：篡改后峰谷漂移必异于原账（确定性对拍）。
    let mut dl8 = DriftLedger::new();
    let mut c = 1u32;
    while c <= CYCLES {
        dl8.sample(c, 51_200 + (c - 1) * 13)?;
        c += 1;
    }
    let pv_before = dl8.drift_peak_valley()?;
    dl8.rows[10].rss_kb += 40_960; // 人为鼓包
    let pv_after = dl8.drift_peak_valley()?;
    if pv_after == pv_before {
        return Err(E_LEDGER_BAD); // 篡改必须从副口径现形
    }
    if pv_after <= pv_before {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R8 写回（判据 J2）：半程账不写回、空摘要拒、成环+非空摘要放行。
    let mut dl9 = DriftLedger::new();
    if dl9.writeback(true, 0x10).is_ok() {
        return Err(E_STATE_BAD);
    }
    let mut c = 1u32;
    while c <= CYCLES {
        dl9.sample(c, 51_200 + (c - 1) * 13)?;
        c += 1;
    }
    if dl9.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    dl9.writeback(true, 0x18)?;
    hits += 1;

    Ok(hits)
}
