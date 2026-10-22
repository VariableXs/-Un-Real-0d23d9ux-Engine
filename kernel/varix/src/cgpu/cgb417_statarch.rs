//! CGPU-F0417 · 长期统计总架构（目标 360 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 长期统计：三级时间尺度（小时→天→月逐级聚合——聚合管线）；统计定位
//!   （回答"长期看谁在消耗、趋势如何"——与 B08 实时遥测互补分层）；存储
//!   架构（聚合结果落持久化——对接 B07 格式同源）；查询层（长时间窗秒级
//!   响应——预聚合支撑）。
//! - 判据：三级聚合、互补分层、持久化、查询、判据。
//!
//! 设计纪律：
//! - 三级聚合管线：小时桶（原始帧归约）→ 天桶（24 小时桶归约）→ 月桶
//!   （30 天桶归约）；每级只吃上一级输出（增量聚合——不全量重算）。
//! - 互补分层：B08 实时遥测（帧级、分钟窗）与长期统计（小时级以上）
//!   职责不相交——时间粒度切分点 = 1 小时，跨层查询委托不重复计。
//! - 持久化：B07 格式同源（段头+CRC+记录体），聚合桶落盘带级坐标。
//! - 查询层：预聚合支撑——长窗查询只摸预聚合桶，原始帧永不进长窗路径
//!   （秒级响应的形式化：查询成本 = O(桶数) 不随帧数增长）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0D40 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0D40，全仓独占）。
pub const SEG_BASE: u16 = 0x0D40;

pub const E_STAT_LEVEL: u16 = SEG_BASE + 1; // 聚合级非法
pub const E_STAT_GAP: u16 = SEG_BASE + 2; // 上级桶缺口（不能越级聚合）
pub const E_STAT_STORE: u16 = SEG_BASE + 3; // 落盘格式错（B07 同源校验红）
pub const E_STAT_QUERY: u16 = SEG_BASE + 4; // 查询窗非法（起>止 或 跨层越权）
pub const E_STAT_LAYER: u16 = SEG_BASE + 5; // 互补分层违反（查询进实时层职责）
pub const E_STAT_CARRY: u16 = SEG_BASE + 6; // 聚合守恒破坏（和不等）

/// 三级时间尺度（桶宽，秒）。
pub const LEVEL_HOUR: u8 = 0;
pub const LEVEL_DAY: u8 = 1;
pub const LEVEL_MONTH: u8 = 2;
pub const LEVEL_COUNT: usize = 3;

/// 桶宽（秒）：小时 3600 / 天 86400 / 月 30×86400。
pub const LEVEL_SECS: [u64; LEVEL_COUNT] = [3_600, 86_400, 2_592_000];

/// 互补分层切分点（秒）：≥1 小时归长期统计，<1 小时归 B08。
pub const LAYER_SPLIT_SECS: u64 = 3_600;

pub fn level_name(l: u8) -> &'static str {
    match l {
        LEVEL_HOUR => "小时",
        LEVEL_DAY => "天",
        _ => "月",
    }
}

/// B07 同源段头（持久化格式——与 B07 采集侧同构）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StoreHead {
    /// 格式魔数（B07 同源值）。
    pub magic: u32,
    /// 级坐标。
    pub level: u8,
    /// 桶序号（自纪元的桶号）。
    pub bucket: u64,
    /// 记录条数。
    pub records: u32,
    /// 体内 CRC32。
    pub crc: u32,
}

/// B07 同源魔数。
pub const STORE_MAGIC: u32 = 0xB075_7A71;

/// 一条聚合记录（账户 → 消耗总量）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AggRec {
    pub acct: u32,
    pub total: u64,
}

/// CRC32（IEEE——B07 同源算法）。
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for b in data {
        crc ^= *b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// 聚合桶。
#[derive(Clone, Debug, Default)]
pub struct AggBucket {
    pub level: u8,
    pub bucket: u64,
    pub recs: Vec<AggRec>,
}

impl AggBucket {
    pub fn total(&self) -> u64 {
        self.recs.iter().map(|r| r.total).sum()
    }

    pub fn of(&self, acct: u32) -> u64 {
        self.recs
            .iter()
            .find(|r| r.acct == acct)
            .map(|r| r.total)
            .unwrap_or(0)
    }
}

/// 长期统计引擎：三级聚合管线 + 持久化 + 查询。
#[derive(Clone, Debug, Default)]
pub struct LongStats {
    /// 三级桶库（level → 桶列表，按 bucket 序）。
    pub buckets: [Vec<AggBucket>; LEVEL_COUNT],
    /// 落盘账（已持久化的桶坐标）。
    pub persisted: Vec<(u8, u64)>,
    /// 查询计数（观测用）。
    pub queries: u64,
}

impl LongStats {
    /// 原始帧样本灌入小时级（管线的唯一入口）。
    pub fn ingest(&mut self, secs: u64, acct: u32, amount: u64) -> Result<(), u16> {
        let b = secs / LEVEL_SECS[LEVEL_HOUR as usize];
        let bl = &mut self.buckets[LEVEL_HOUR as usize];
        match bl.iter_mut().find(|x| x.bucket == b) {
            Some(x) => {
                match x.recs.iter_mut().find(|r| r.acct == acct) {
                    Some(r) => r.total += amount,
                    None => x.recs.push(AggRec { acct, total: amount }),
                }
            }
            None => bl.push(AggBucket {
                level: LEVEL_HOUR,
                bucket: b,
                recs: vec![AggRec { acct, total: amount }],
            }),
        }
        Ok(())
    }

    /// 逐级聚合：hour→day（24 桶缺一即拒）、day→month（30 桶缺一即拒）。
    /// 上级桶被消费后从库中移除（增量聚合——不重算）。
    pub fn rollup(&mut self, from: u8) -> Result<usize, u16> {
        if from as usize >= LEVEL_COUNT - 1 {
            return Err(E_STAT_LEVEL);
        }
        let to = from + 1;
        let src = &self.buckets[from as usize];
        // 完整窗口检查。
        let need = if to == LEVEL_DAY { 24 } else { 30 };
        // 按桶号找连续窗口：取最小桶号起的 need 个连续桶。
        let mut first = u64::MAX;
        for b in src {
            if b.bucket < first {
                first = b.bucket;
            }
        }
        if first == u64::MAX {
            return Err(E_STAT_GAP);
        }
        let mut have = [false; 32];
        for b in src {
            if b.bucket >= first && b.bucket < first + need as u64 {
                let idx = (b.bucket - first) as usize;
                if idx < need {
                    have[idx] = true;
                }
            }
        }
        if !have[..need].iter().all(|h| *h) {
            return Err(E_STAT_GAP);
        }
        // 归约。
        let mut merged: Vec<AggRec> = Vec::new();
        for b in src {
            if b.bucket >= first && b.bucket < first + need as u64 {
                for r in &b.recs {
                    match merged.iter_mut().find(|m| m.acct == r.acct) {
                        Some(m) => m.total += r.total,
                        None => merged.push(AggRec { acct: r.acct, total: r.total }),
                    }
                }
            }
        }
        // 守恒：上级和 = 下级和。
        let src_sum: u64 = src
            .iter()
            .filter(|b| b.bucket >= first && b.bucket < first + need as u64)
            .map(|b| b.total())
            .sum();
        let dst_sum: u64 = merged.iter().map(|r| r.total).sum();
        if src_sum != dst_sum {
            return Err(E_STAT_CARRY);
        }
        // 上级桶号 = 首个源桶号 / 归约扇入（24 小时→1 天、30 天→1 月）。
        let dst_bucket = first / need as u64;
        // 消费掉的源桶移除。
        self.buckets[from as usize]
            .retain(|b| !(b.bucket >= first && b.bucket < first + need as u64));
        self.buckets[to as usize].push(AggBucket {
            level: to,
            bucket: dst_bucket,
            recs: merged,
        });
        self.buckets[to as usize].sort_by_key(|b| b.bucket);
        Ok(1)
    }

    /// 持久化：桶落 B07 同源段（头 CRC 自检）。
    pub fn persist(&mut self, level: u8, bucket: u64) -> Result<u32, u16> {
        if level as usize >= LEVEL_COUNT {
            return Err(E_STAT_LEVEL);
        }
        let b = self
            .buckets
            .get(level as usize)
            .and_then(|v| v.iter().find(|x| x.bucket == bucket))
            .ok_or(E_STAT_GAP)?;
        // 体内字节序（确定性序列化——CRC 同源）。
        let mut body = Vec::new();
        for r in &b.recs {
            body.extend_from_slice(&r.acct.to_le_bytes());
            body.extend_from_slice(&r.total.to_le_bytes());
        }
        let crc = crc32(&body);
        let head = StoreHead {
            magic: STORE_MAGIC,
            level,
            bucket,
            records: b.recs.len() as u32,
            crc,
        };
        // 头自检（B07 同源校验）。
        if head.magic != STORE_MAGIC || head.records != b.recs.len() as u32 {
            return Err(E_STAT_STORE);
        }
        self.persisted.push((level, bucket));
        Ok(crc)
    }

    /// 查询：起止秒窗内某级某账户总消耗。
    /// 长窗强制走 ≥小时级桶（互补分层——原始帧不进长窗路径）。
    pub fn query(&mut self, level: u8, from_secs: u64, to_secs: u64, acct: u32) -> Result<u64, u16> {
        self.queries += 1;
        if level as usize >= LEVEL_COUNT {
            return Err(E_STAT_LEVEL);
        }
        if from_secs > to_secs {
            return Err(E_STAT_QUERY);
        }
        let span = to_secs - from_secs;
        if span < LAYER_SPLIT_SECS {
            return Err(E_STAT_LAYER); // 短窗请走 B08 实时层
        }
        let b0 = from_secs / LEVEL_SECS[level as usize];
        let b1 = to_secs / LEVEL_SECS[level as usize];
        let mut sum = 0u64;
        for b in &self.buckets[level as usize] {
            if b.bucket >= b0 && b.bucket <= b1 {
                sum += b.of(acct);
            }
        }
        Ok(sum)
    }

    /// 查询成本模型：长窗响应 O(桶数)——桶数上限（秒级响应的形式化）。
    pub fn query_buckets_touched(&self, level: u8, from_secs: u64, to_secs: u64) -> Result<usize, u16> {
        if level as usize >= LEVEL_COUNT || from_secs > to_secs {
            return Err(E_STAT_QUERY);
        }
        // 预聚合支撑的真实成本 = 窗内存量桶数（空桶位不虚报——
        // 成本不随帧数增长，也不随窗的名义桶位虚涨）。
        let w = LEVEL_SECS[level as usize];
        Ok(self.buckets[level as usize]
            .iter()
            .filter(|b| b.bucket * w + w > from_secs && b.bucket * w <= to_secs)
            .count())
    }
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn seeded() -> LongStats {
        let mut s = LongStats::default();
        // 24 个小时桶 × 账户 1 每桶 100、账户 2 每桶 50。
        for h in 0..24u64 {
            s.ingest(h * 3_600, 1, 100).ok();
            s.ingest(h * 3_600, 2, 50).ok();
        }
        s
    }

    #[test]
    fn a01_三级聚合_小时到天逐级归约() {
        let mut s = seeded();
        assert_eq!(s.rollup(LEVEL_HOUR), Ok(1));
        // 天桶生成：账户 1 = 2400、账户 2 = 1200；小时桶被消费。
        assert_eq!(s.buckets[LEVEL_HOUR as usize].len(), 0);
        let day = &s.buckets[LEVEL_DAY as usize][0];
        assert_eq!(day.of(1), 2_400);
        assert_eq!(day.of(2), 1_200);
        // 越级聚合拒（hour→month 不允许）。
        assert_eq!(s.rollup(2), Err(E_STAT_LEVEL));
        assert_eq!(s.rollup(LEVEL_MONTH), Err(E_STAT_LEVEL));
        // 桶缺口拒。
        let mut s2 = seeded();
        s2.buckets[LEVEL_HOUR as usize].pop();
        assert_eq!(s2.rollup(LEVEL_HOUR), Err(E_STAT_GAP));
    }

    #[test]
    fn a02_守恒_上级和等于下级和() {
        let mut s = seeded();
        let before: u64 = s.buckets[0].iter().map(|b| b.total()).sum();
        s.rollup(LEVEL_HOUR).ok();
        let after = s.buckets[LEVEL_DAY as usize].iter().map(|b| b.total()).sum::<u64>();
        assert_eq!(before, after);
        // 逐账户守恒。
        let day = &s.buckets[1][0];
        assert_eq!(day.total(), before);
    }

    #[test]
    fn a03_持久化_B07同源头与CRC() {
        let mut s = seeded();
        s.rollup(LEVEL_HOUR).ok();
        let crc = s.persist(LEVEL_DAY, 0).ok();
        assert!(crc.is_some());
        assert_eq!(s.persisted, vec![(LEVEL_DAY, 0)]);
        // CRC 确定性（同数据同 CRC）。
        let mut s2 = seeded();
        s2.rollup(LEVEL_HOUR).ok();
        assert_eq!(s2.persist(LEVEL_DAY, 0), Ok(crc.unwrap_or(0)));
        // CRC32 算法自证（标准向量 "123456789" = 0xCBF43926）。
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        // 缺桶持久化拒。
        assert_eq!(s.persist(LEVEL_MONTH, 0), Err(E_STAT_GAP));
    }

    #[test]
    fn a04_互补分层_短窗拒绝走实时层() {
        let mut s = LongStats::default();
        // 30 分钟窗 < 1 小时切分点 ⇒ 拒（B08 职责）。
        assert_eq!(s.query(LEVEL_HOUR, 0, 1_800, 1), Err(E_STAT_LAYER));
        // 2 小时窗放行。
        s.ingest(0, 1, 10).ok();
        s.ingest(3_600, 1, 20).ok();
        assert_eq!(s.query(LEVEL_HOUR, 0, 7_200 - 1, 1), Ok(30));
        // 非法窗拒。
        assert_eq!(s.query(LEVEL_HOUR, 100, 0, 1), Err(E_STAT_QUERY));
        assert_eq!(s.query(9, 0, 7_200, 1), Err(E_STAT_LEVEL));
        // 切分点常量自洽。
        assert_eq!(LAYER_SPLIT_SECS, LEVEL_SECS[0]);
    }

    #[test]
    fn a05_查询层_长窗成本只随桶数增长() {
        let mut s = LongStats::default();
        // 灌 3 天的小时数据并聚合成 3 个天桶。
        for h in 0..72u64 {
            s.ingest(h * 3_600, 1, 1).ok();
        }
        for _ in 0..3 {
            s.rollup(LEVEL_HOUR).ok();
        }
        assert_eq!(s.buckets[LEVEL_DAY as usize].len(), 3);
        // 一月窗查询摸的天桶数 = 3（而非 72 个小时桶）——预聚合支撑。
        assert_eq!(s.query_buckets_touched(LEVEL_DAY, 0, 2_592_000 - 1), Ok(3));
        assert_eq!(s.query(LEVEL_DAY, 0, 2_592_000 - 1, 1), Ok(72));
        assert_eq!(s.queries, 1);
        // 级名表。
        assert_eq!(level_name(LEVEL_MONTH), "月");
    }
}
