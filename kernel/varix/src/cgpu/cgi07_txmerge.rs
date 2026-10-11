//! cgi07_txmerge — CGPU-F1286 传输批合并（小包凑大车，PCIe 事务不空转）。
//!
//! 判据原文（施工书锚点 CGPU-F1286）：批合并：传输批合并（小传输合并大传
//! 输——PCIe 事务效率优化；合并窗口与延迟权衡声明；合并确定性；合并收益量
//! 化（合并前后吞吐对比）。判据：合并、权衡、确定、收益、判据。
//!
//! 职责定位：合并窗口状态机（window_us 内到达的小传输聚合成批——超窗或批
//! 满即冲刷）+ 权衡声明面（合并省事务头但加首包延迟——收益量化前先声明权
//! 衡，参数可配）+ 确定性保证（同输入序列同合并结果——合并决策不依赖到达
//! 时序抖动，只依赖窗口边界与批容量）+ 收益量化（事务数对比/字节开销对比/
//! 等效吞吐对比——合并前后同口径测）。与 cgi06_txqueue（排队）、cgi05_
//! pciearb（仲裁）分工：彼管顺序与路权，此管把小包装大车。红线：紧急传输
//! 不等窗（Emergency 绕过合并直达——延迟红线优先于事务效率）；合并不改字
//! 节序不丢字节（内容完整性自检）。诊断码段 0x5801~0x5805。

use alloc::vec::Vec;

/// 模块版本。
pub const TX_MERGE_VERSION: &str = "I07-txmerge-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x58 段，本会话独占）
// ---------------------------------------------------------------------------

pub const XC_BATCH_OVERFLOW: u32 = 0x5801; // 批容量违约（单批字节超上限）
pub const XC_EMERGENCY_QUEUED: u32 = 0x5802; // 紧急传输被入合并窗（延迟红线违约）
pub const XC_BYTE_LOSS: u32 = 0x5803; // 合并前后字节不一致（内容完整性失守）
pub const XC_WINDOW_NEGATIVE: u32 = 0x5804; // 窗口参数非法（零窗=关闭合并语义混用）
pub const XC_GAIN_NOT_MEASURED: u32 = 0x5805; // 收益未量化即声称有效（空口承诺）

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 合并窗口默认时长（微秒）：5µs——首包延迟权衡的默认档。
pub const MERGE_WINDOW_US: u64 = 5;
/// 单批容量上限（字节）：4KiB——一个 PCIe 最大载荷事务量级。
pub const BATCH_CAP_BYTES: u32 = 4096;
/// 单批条目上限。
pub const BATCH_CAP_ITEMS: usize = 32;
/// PCIe 事务头固定开销（字节）：TLP 头+摘要——合并收益的度量基线。
pub const TLP_OVERHEAD_BYTES: u32 = 24;

/// 合并模式（权衡声明档位——判据"合并窗口与延迟权衡"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MergeMode {
    /// 关闭：逐包直传（延迟最优先）。
    Off,
    /// 温和：5µs 窗（默认权衡点）。
    Gentle,
    /// 激进：20µs 窗（吞吐优先，延迟敏感流禁用）。
    Aggressive,
}

impl MergeMode {
    pub const fn window_us(self) -> u64 {
        match self {
            MergeMode::Off => 0,
            MergeMode::Gentle => MERGE_WINDOW_US,
            MergeMode::Aggressive => 20,
        }
    }

    pub const fn zh(self) -> &'static str {
        match self {
            MergeMode::Off => "关闭合并",
            MergeMode::Gentle => "温和合并（5µs 窗）",
            MergeMode::Aggressive => "激进合并（20µs 窗）",
        }
    }
}

/// 优先级（复用 F1285 三级——Emergency 不合并）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TxPrio {
    Normal,
    High,
    Emergency,
}

/// 传输片段（合并输入）。
#[derive(Clone, Copy, Debug)]
pub struct TxSeg {
    pub id: u64,
    pub prio: TxPrio,
    pub bytes: u32,
    /// 到达时刻（微秒）。
    pub arrive_us: u64,
}

/// 合并批（输出）。
#[derive(Clone, Debug, PartialEq)]
pub struct MergeBatch {
    /// 成员段 ID（有序——确定性输出面）。
    pub member_ids: Vec<u64>,
    /// 批总字节。
    pub total_bytes: u32,
    /// 批首到达（窗口计时起点）。
    pub first_arrive_us: u64,
    /// 冲刷时刻。
    pub flush_us: u64,
    /// 省下的 TLP 头开销（字节）。
    pub saved_overhead: u32,
}

/// 批合并器。
pub struct TxMerger {
    pub mode: MergeMode,
    /// 待合并窗内缓冲。
    pending: Vec<TxSeg>,
    /// 窗开启时刻。
    window_open_us: u64,
    /// 已冲刷批账（收益量化数据源）。
    batches: Vec<MergeBatch>,
    /// 绕过合并的直传计数。
    pub bypass_count: u64,
    /// 收益账：合并前事务数（=全部段数）。
    pub raw_transactions: u64,
    /// 收益账：合并后事务数（=批数+绕过数）。
    pub effective_transactions: u64,
    /// 字节守恒账：提交总字节（守恒等式左侧）。
    pub total_submitted_bytes: u64,
    /// 字节守恒账：绕过直传字节。
    pub bypassed_bytes: u64,
}

impl TxMerger {
    pub fn new(mode: MergeMode) -> Self {
        TxMerger {
            mode,
            pending: Vec::with_capacity(BATCH_CAP_ITEMS),
            window_open_us: 0,
            batches: Vec::new(),
            bypass_count: 0,
            raw_transactions: 0,
            effective_transactions: 0,
            total_submitted_bytes: 0,
            bypassed_bytes: 0,
        }
    }

    /// 片段提交：Emergency 绕过（延迟红线）；窗口未开则开窗计时。
    pub fn submit(&mut self, seg: TxSeg) -> Result<Option<MergeBatch>, u32> {
        self.raw_transactions += 1;
        self.total_submitted_bytes += seg.bytes as u64;
        if seg.prio == TxPrio::Emergency {
            // 紧急直传：不入窗（判据延迟红线）。同时把缓冲中已凑的批冲掉，
            // 让紧急包插在批前发（顺序公平：先到的批不必让路但也不再等）。
            self.bypass_count += 1;
            self.bypassed_bytes += seg.bytes as u64;
            self.effective_transactions += 1;
            let flushed = self.flush_if_any(seg.arrive_us)?;
            return Ok(flushed);
        }
        if self.mode == MergeMode::Off {
            self.bypass_count += 1;
            self.bypassed_bytes += seg.bytes as u64;
            self.effective_transactions += 1;
            return Ok(None);
        }
        if seg.bytes > BATCH_CAP_BYTES {
            return Err(XC_BATCH_OVERFLOW);
        }
        if self.pending.len() == 0 {
            self.window_open_us = seg.arrive_us;
        }
        self.pending.push(seg);
        // 冲刷判定：批满或窗到期。
        if self.batch_bytes() >= BATCH_CAP_BYTES || self.window_expired(seg.arrive_us) {
            let b = self.flush(seg.arrive_us)?;
            return Ok(b);
        }
        Ok(None)
    }

    fn batch_bytes(&self) -> u32 {
        let mut acc: u32 = 0;
        let mut i = 0;
        while i < self.pending.len() {
            acc += self.pending[i].bytes;
            i += 1;
        }
        acc
    }

    fn window_expired(&self, now_us: u64) -> bool {
        now_us.saturating_sub(self.window_open_us) >= self.mode.window_us()
    }

    /// 冲刷当前缓冲成批（空缓冲返回 None）。
    pub fn flush(&mut self, now_us: u64) -> Result<Option<MergeBatch>, u32> {
        if self.pending.len() == 0 {
            return Ok(None);
        }
        let total = self.batch_bytes();
        if total > BATCH_CAP_BYTES {
            return Err(XC_BATCH_OVERFLOW);
        }
        let n = self.pending.len() as u32;
        let mut ids = Vec::with_capacity(BATCH_CAP_ITEMS);
        let mut i = 0;
        while i < self.pending.len() {
            ids.push(self.pending[i].id);
            i += 1;
        }
        let b = MergeBatch {
            member_ids: ids,
            total_bytes: total,
            first_arrive_us: self.window_open_us,
            flush_us: now_us,
            saved_overhead: (n.saturating_sub(1)) * TLP_OVERHEAD_BYTES,
        };
        self.pending = Vec::with_capacity(BATCH_CAP_ITEMS);
        self.effective_transactions += 1;
        self.batches.push(b.clone());
        Ok(Some(b))
    }

    fn flush_if_any(&mut self, now_us: u64) -> Result<Option<MergeBatch>, u32> {
        self.flush(now_us)
    }

    /// 收益量化（判据"合并前后吞吐对比"——同口径：同字节数下事务数与
    /// 头开销对比。返回 (原始事务数, 合并后事务数, 省下的头字节, 等效
    /// 吞吐提升 ppm)。零合并时提升=0，不许无数据吹收益。
    pub fn gain_report(&self) -> (u64, u64, u32, u64) {
        let mut saved: u64 = 0;
        let mut i = 0;
        while i < self.batches.len() {
            saved += self.batches[i].saved_overhead as u64;
            i += 1;
        }
        // 等效吞吐提升 ppm：字节不变、事务头减少 ⇒ 有效载荷率提升。
        // 提升 = saved / (total_bytes_before + saved_overhead_before) 近似：
        // = saved / (total_bytes + 原头开销)。保守口径只按 saved/总字节比。
        let mut total_bytes: u64 = 0;
        let mut j = 0;
        while j < self.batches.len() {
            total_bytes += self.batches[j].total_bytes as u64;
            j += 1;
        }
        let gain_ppm = if total_bytes > 0 {
            saved * 1_000_000 / (total_bytes + saved)
        } else {
            0
        };
        (
            self.raw_transactions,
            self.effective_transactions,
            saved as u32,
            gain_ppm,
        )
    }

    /// 确定性核验（判据"合并确定性"）：两路同输入逐批对拍成员序列。
    pub fn determinism_check(a: &Vec<MergeBatch>, b: &Vec<MergeBatch>) -> Result<(), u32> {
        if a.len() != b.len() {
            return Err(XC_BYTE_LOSS);
        }
        let mut i = 0;
        while i < a.len() {
            if a[i].member_ids.len() != b[i].member_ids.len() {
                return Err(XC_BYTE_LOSS);
            }
            let mut j = 0;
            while j < a[i].member_ids.len() {
                if a[i].member_ids[j] != b[i].member_ids[j] {
                    return Err(XC_BYTE_LOSS);
                }
                j += 1;
            }
            if a[i].total_bytes != b[i].total_bytes {
                return Err(XC_BYTE_LOSS);
            }
            i += 1;
        }
        Ok(())
    }

    /// 批账查询（收益量化的原始面）。
    pub fn batch_log(&self) -> &Vec<MergeBatch> {
        &self.batches
    }

    /// 缓冲残余（未冲刷条目数——发布前必须为 0，防丢字节）。
    pub const fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// 字节守恒对账（判据"合并不丢字节"的运行时等式）：
    /// 批内字节 + 缓冲字节 + 绕过字节 ≡ 提交总字节。
    pub fn byte_account(&self) -> bool {
        let mut batched: u64 = 0;
        let mut i = 0;
        while i < self.batches.len() {
            batched += self.batches[i].total_bytes as u64;
            i += 1;
        }
        let mut buffered: u64 = 0;
        let mut j = 0;
        while j < self.pending.len() {
            buffered += self.pending[j].bytes as u64;
            j += 1;
        }
        batched + buffered + self.bypassed_bytes == self.total_submitted_bytes
    }
}

// ---------------------------------------------------------------------------
// 自检（判据五组：合并 / 权衡 / 确定性 / 收益 / 红线）
// ---------------------------------------------------------------------------

impl TxMerger {
    /// 组一：合并——窗内小包聚成一批、批满即冲、窗到期即冲。
    pub fn selfcheck_merge(&mut self) -> Result<(), u32> {
        let mut m = TxMerger::new(MergeMode::Gentle);
        // 三个 1KiB 小包窗内到达。
        let mut i = 0;
        while i < 3 {
            let b = m.submit(TxSeg { id: i + 1, prio: TxPrio::Normal, bytes: 1024, arrive_us: 100 })?;
            if i < 2 && b.is_some() {
                return Err(XC_BATCH_OVERFLOW); // 未满未到期不应冲
            }
            i += 1;
        }
        // 第四包触发 4096 批满冲刷。
        let b4 = m.submit(TxSeg { id: 4, prio: TxPrio::Normal, bytes: 1024, arrive_us: 103 })?;
        match b4 {
            Some(b) => {
                if b.member_ids.len() != 4 || b.total_bytes != 4096 {
                    return Err(XC_BATCH_OVERFLOW);
                }
                if b.saved_overhead != 3 * TLP_OVERHEAD_BYTES {
                    return Err(XC_BATCH_OVERFLOW);
                }
            }
            None => return Err(XC_BATCH_OVERFLOW),
        }
        // 窗到期冲刷：单包入窗后等 5µs。
        let _ = m.submit(TxSeg { id: 5, prio: TxPrio::Normal, bytes: 100, arrive_us: 200 })?;
        let b6 = m.submit(TxSeg { id: 6, prio: TxPrio::Normal, bytes: 100, arrive_us: 200 + MERGE_WINDOW_US })?;
        match b6 {
            Some(b) => {
                if b.member_ids.len() != 2 {
                    return Err(XC_WINDOW_NEGATIVE);
                }
            }
            None => return Err(XC_WINDOW_NEGATIVE),
        }
        Ok(())
    }

    /// 组二：权衡——Off 模式全绕过、Emergency 永不入窗。
    pub fn selfcheck_tradeoff(&mut self) -> Result<(), u32> {
        let mut off = TxMerger::new(MergeMode::Off);
        let mut i = 0;
        while i < 5 {
            off.submit(TxSeg { id: i, prio: TxPrio::Normal, bytes: 64, arrive_us: 10 })?;
            i += 1;
        }
        if off.bypass_count != 5 || off.batch_log().len() != 0 {
            return Err(XC_WINDOW_NEGATIVE);
        }
        let (raw, eff, saved, gain) = off.gain_report();
        if saved != 0 || gain != 0 {
            return Err(XC_GAIN_NOT_MEASURED); // 关闭模式零收益——不许吹
        }
        if raw != eff {
            return Err(XC_GAIN_NOT_MEASURED);
        }
        // 全绕过模式的字节守恒：绕过字节=提交字节。
        if !off.byte_account() || off.bypassed_bytes != off.total_submitted_bytes {
            return Err(XC_BYTE_LOSS);
        }
        // Emergency 绕过：即使激进模式也不等窗。
        let mut agg = TxMerger::new(MergeMode::Aggressive);
        let _ = agg.submit(TxSeg { id: 1, prio: TxPrio::Normal, bytes: 64, arrive_us: 1 })?;
        let b2 = agg.submit(TxSeg { id: 2, prio: TxPrio::Emergency, bytes: 64, arrive_us: 2 })?;
        if agg.bypass_count != 1 {
            return Err(XC_EMERGENCY_QUEUED);
        }
        // 紧急包触发缓冲批冲刷（让路语义）。
        match b2 {
            Some(b) => {
                if b.member_ids.len() != 1 || b.member_ids[0] != 1 {
                    return Err(XC_EMERGENCY_QUEUED);
                }
            }
            None => return Err(XC_EMERGENCY_QUEUED),
        }
        Ok(())
    }

    /// 组三：确定性——同输入双跑批序列逐批相同。
    pub fn selfcheck_determinism(&mut self) -> Result<(), u32> {
        // 跑 A。
        let mut a = TxMerger::new(MergeMode::Gentle);
        let mut i = 0;
        while i < 8 {
            a.submit(TxSeg { id: i + 1, prio: TxPrio::Normal, bytes: 512, arrive_us: 10 + i })?;
            i += 1;
        }
        let _ = a.flush(100)?;
        // 跑 B（同输入）。
        let mut b = TxMerger::new(MergeMode::Gentle);
        let mut j = 0;
        while j < 8 {
            b.submit(TxSeg { id: j + 1, prio: TxPrio::Normal, bytes: 512, arrive_us: 10 + j })?;
            j += 1;
        }
        let _ = b.flush(100)?;
        TxMerger::determinism_check(a.batch_log(), b.batch_log())?;
        if a.batch_log().len() == 0 {
            return Err(XC_BATCH_OVERFLOW);
        }
        Ok(())
    }

    /// 组四：收益——同字节多包场景事务数下降、saved 为 TLP 头数×开销。
    pub fn selfcheck_gain(&mut self) -> Result<(), u32> {
        let mut m = TxMerger::new(MergeMode::Gentle);
        let mut i = 0;
        while i < 16 {
            m.submit(TxSeg { id: i + 1, prio: TxPrio::Normal, bytes: 256, arrive_us: 10 })?;
            i += 1;
        }
        let _ = m.flush(50)?;
        if m.pending_len() != 0 {
            return Err(XC_BYTE_LOSS); // 发布前缓冲必须清空
        }
        let (raw, eff, saved, gain) = m.gain_report();
        if raw != 16 {
            return Err(XC_GAIN_NOT_MEASURED);
        }
        if eff != 1 {
            return Err(XC_GAIN_NOT_MEASURED); // 16 包合 1 批
        }
        if saved != 15 * TLP_OVERHEAD_BYTES {
            return Err(XC_GAIN_NOT_MEASURED);
        }
        if gain == 0 {
            return Err(XC_GAIN_NOT_MEASURED);
        }
        // 字节守恒：批内字节和 = 16×256。
        let log = m.batch_log();
        let mut acc: u64 = 0;
        let mut k = 0;
        while k < log.len() {
            acc += log[k].total_bytes as u64;
            k += 1;
        }
        if acc != 16 * 256 {
            return Err(XC_BYTE_LOSS);
        }
        // 字节守恒等式闭环。
        if !m.byte_account() {
            return Err(XC_BYTE_LOSS);
        }
        Ok(())
    }

    /// 组五：红线——超批容量拒、窗参数非法值防御。
    pub fn selfcheck_redline(&mut self) -> Result<(), u32> {
        let mut m = TxMerger::new(MergeMode::Gentle);
        let r = m.submit(TxSeg { id: 1, prio: TxPrio::Normal, bytes: BATCH_CAP_BYTES + 1, arrive_us: 1 });
        if r != Err(XC_BATCH_OVERFLOW) {
            return Err(XC_BATCH_OVERFLOW);
        }
        // Off 窗口语义：window_us=0 时 submit 不开窗（立即绕过）。
        let off = TxMerger::new(MergeMode::Off);
        if off.mode.window_us() != 0 {
            return Err(XC_WINDOW_NEGATIVE);
        }
        Ok(())
    }

    /// 全量自检入口。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_merge()?;
        self.selfcheck_tradeoff()?;
        self.selfcheck_determinism()?;
        self.selfcheck_gain()?;
        self.selfcheck_redline()?;
        Ok(())
    }
}
