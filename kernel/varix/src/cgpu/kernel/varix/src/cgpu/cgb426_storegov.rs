//! CGPU-F0426 · 统计存储治理（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 存储治理：统计存储配额（长期统计的磁盘上限声明——超限策略可配：
//!   压缩/过期）；压缩分级（冷数据高压缩比）；过期策略（默认保留
//!   12 个月——可配，过期前告知）；治理透明（存储占用可查——12 章）。
//! - 判据：配额、压缩、过期告知、透明、判据。
//!
//! 设计纪律：
//! - 配额：桶条数上限；超限策略可配（Compress 先压 / Expire 先过期）
//!   ——策略是枚举不是硬编码。
//! - 压缩分级：热桶（近期）不压、冷桶（远期）高压缩（行程编码 RLE——
//!   确定性可逆）；压缩后字节数计入水位。
//! - 过期策略：默认保留 12 个月可配；过期前 N 桶进「告知名单」
//!   （过期前告知的形式化——不静默删）。
//! - 透明：水位/配额/压缩比/告知名单全可查。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0DD0 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0DD0，全仓独占）。
pub const SEG_BASE: u16 = 0x0DD0;

pub const E_GOV_QUOTA: u16 = SEG_BASE + 1; // 超配额且策略未能解
pub const E_GOV_POLICY: u16 = SEG_BASE + 2; // 策略非法
pub const E_GOV_RLE: u16 = SEG_BASE + 3; // RLE 压缩不可逆
pub const E_GOV_NOTICE: u16 = SEG_BASE + 4; // 过期未告知就删
pub const E_GOV_OPAQUE: u16 = SEG_BASE + 5; // 治理数据不可查
pub const E_GOV_KEEP: u16 = SEG_BASE + 6; // 保留期非法（0）

/// 超限策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverPolicy {
    /// 先压缩冷桶。
    CompressFirst,
    /// 先过期最旧桶。
    ExpireFirst,
}

/// 默认保留 12 个月。
pub const DEFAULT_KEEP_MONTHS: u32 = 12;
/// 过期前告知桶数。
pub const NOTICE_AHEAD: usize = 3;

/// 一个统计桶（水位按字节计）。
#[derive(Clone, Debug)]
pub struct GovBucket {
    pub period: u64,
    /// 原始字节。
    pub raw_bytes: u64,
    /// 压缩后字节（未压 = 同 raw）。
    pub stored_bytes: u64,
    pub compressed: bool,
}

/// 存储治理器。
#[derive(Clone, Debug)]
pub struct StoreGovernor {
    pub quota_bytes: u64,
    pub policy: OverPolicy,
    pub keep_months: u32,
    pub buckets: Vec<GovBucket>,
    /// 过期告知名单（期号）。
    pub notices: Vec<u64>,
    /// 已过期删除数。
    pub expired: u64,
}

impl StoreGovernor {
    pub fn new(quota_bytes: u64, policy: OverPolicy, keep_months: u32) -> Self {
        StoreGovernor {
            quota_bytes,
            policy,
            keep_months: if keep_months == 0 { DEFAULT_KEEP_MONTHS } else { keep_months },
            buckets: Vec::new(),
            notices: Vec::new(),
            expired: 0,
        }
    }

    pub fn used_bytes(&self) -> u64 {
        self.buckets.iter().map(|b| b.stored_bytes).sum()
    }

    pub fn watermark_pm(&self) -> u64 {
        if self.quota_bytes == 0 {
            return 0;
        }
        self.used_bytes() * 1000 / self.quota_bytes
    }

    /// 入桶：超配额按策略自救（压冷桶或过期最旧），救不动即拒。
    pub fn admit(&mut self, period: u64, raw_bytes: u64) -> Result<(), u16> {
        // 超限自救循环（最多两轮：一轮压缩 + 一轮过期）。
        let mut guard = 0;
        while self.used_bytes() + raw_bytes > self.quota_bytes {
            guard += 1;
            if guard > 2 {
                return Err(E_GOV_QUOTA);
            }
            match self.policy {
                OverPolicy::CompressFirst => {
                    if !self.compress_coldest() {
                        if !self.expire_oldest() {
                            return Err(E_GOV_QUOTA);
                        }
                    }
                }
                OverPolicy::ExpireFirst => {
                    if !self.expire_oldest() {
                        if !self.compress_coldest() {
                            return Err(E_GOV_QUOTA);
                        }
                    }
                }
            }
        }
        self.buckets.push(GovBucket {
            period,
            raw_bytes,
            stored_bytes: raw_bytes,
            compressed: false,
        });
        self.buckets.sort_by_key(|b| b.period);
        Ok(())
    }

    /// 压缩最旧未压桶（RLE 高压缩——冷数据）。返回是否压成。
    fn compress_coldest(&mut self) -> bool {
        let mut target: Option<usize> = None;
        for (i, b) in self.buckets.iter().enumerate() {
            if !b.compressed {
                target = Some(i);
                break;
            }
        }
        let i = match target {
            Some(i) => i,
            None => return false,
        };
        // RLE 模拟：压后字节 = 原始 / 8（冷数据高压缩比——保守模型）。
        let b = &mut self.buckets[i];
        let packed = (b.raw_bytes / 8).max(1);
        // 压缩后必须更小（否则不压）。
        if packed >= b.stored_bytes {
            return false;
        }
        b.stored_bytes = packed;
        b.compressed = true;
        true
    }

    /// 过期最旧桶：必须先在告知名单中（不静默删）。
    fn expire_oldest(&mut self) -> bool {
        if self.buckets.is_empty() {
            return false;
        }
        let oldest = self.buckets[0].period;
        if !self.notices.contains(&oldest) {
            return false; // 未告知不许删
        }
        self.buckets.remove(0);
        self.notices.retain(|p| *p != oldest);
        self.expired += 1;
        true
    }

    /// 过期前告知：距保留期还剩 ≤ NOTICE_AHEAD 个月的最旧桶进名单。
    pub fn sweep_notices(&mut self, current_month: u64) -> usize {
        let keep = self.keep_months as u64;
        self.notices.clear();
        for b in &self.buckets {
            let age = current_month.saturating_sub(b.period);
            let remain = keep.saturating_sub(age);
            if remain <= NOTICE_AHEAD as u64 && !self.notices.contains(&b.period) {
                self.notices.push(b.period);
            }
        }
        self.notices.len()
    }

    /// 治理透明快照（12 章资源透明——全可查）。
    pub fn transparent(&self) -> Result<(u64, u64, u32, usize), u16> {
        if self.quota_bytes == 0 {
            return Err(E_GOV_OPAQUE);
        }
        let compressed_count = self.buckets.iter().filter(|b| b.compressed).count();
        let ratio_pm = if self.used_bytes() == 0 {
            1000
        } else {
            self.used_bytes() * 1000
                / self.buckets.iter().map(|b| b.raw_bytes).sum::<u64>().max(1)
        };
        Ok((self.used_bytes(), self.quota_bytes, ratio_pm as u32, compressed_count))
    }
}

/// RLE 行程编码（确定性可逆——压缩正确性自证用）。
pub fn rle_encode(data: &[u8]) -> Vec<(u8, u32)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let mut run = 1u32;
        while i + 1 < data.len() && data[i + 1] == data[i] {
            run += 1;
            i += 1;
        }
        out.push((data[i], run));
        i += 1;
    }
    out
}

/// RLE 解码（可逆自证）。
pub fn rle_decode(enc: &[(u8, u32)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (b, n) in enc {
        for _ in 0..*n {
            out.push(*b);
        }
    }
    out
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a01_配额_超限按策略自救() {
        // 配额 1000：灌 700 后再灌 500 → 超限 → 压缩冷桶自救。
        let mut g = StoreGovernor::new(1_000, OverPolicy::CompressFirst, DEFAULT_KEEP_MONTHS);
        g.admit(1, 700).ok();
        g.admit(2, 200).ok();
        // 第三桶触发压缩（桶 1 → 88 字节）后放行。
        assert_eq!(g.admit(3, 500), Ok(()));
        assert_eq!(g.buckets.iter().filter(|b| b.compressed).count(), 1);
        // 压不动也过期不动 → 拒。
        let mut g2 = StoreGovernor::new(500, OverPolicy::CompressFirst, DEFAULT_KEEP_MONTHS);
        g2.admit(1, 300).ok();
        g2.notices.push(1); // 可过期
        assert_eq!(g2.admit(2, 800), Err(E_GOV_QUOTA));
    }

    #[test]
    fn a02_压缩分级_冷桶高压缩比可逆() {
        // RLE 可逆自证。
        let data = vec![7u8; 100];
        let enc = rle_encode(&data);
        assert_eq!(enc, vec![(7, 100)]);
        assert_eq!(rle_decode(&enc), data);
        // 压缩比 100/3（编码体）远小于原始。
        assert!(enc.len() < data.len());
        // 混合数据往返。
        let mixed = vec![1u8, 1, 1, 2, 2, 3, 3, 3, 3, 4];
        let e2 = rle_encode(&mixed);
        assert_eq!(rle_decode(&e2), mixed);
    }

    #[test]
    fn a03_过期策略_默认12个月可配告知先行() {
        // 零保留期回退默认。
        let g = StoreGovernor::new(1_000_000, OverPolicy::ExpireFirst, 0);
        assert_eq!(g.keep_months, DEFAULT_KEEP_MONTHS);
        // 告知名单：距到期 ≤ 3 个月的桶。
        let mut g2 = StoreGovernor::new(1_000_000, OverPolicy::ExpireFirst, 12);
        g2.admit(0, 100).ok(); // 第 0 月桶
        g2.admit(2, 100).ok();
        g2.admit(3, 100).ok();
        g2.admit(5, 100).ok();
        assert_eq!(g2.sweep_notices(12), 3);
        // remain = keep - age = 12 - (12 - period) = period。
        // 桶 0 remain 0、桶 2 remain 2、桶 3 remain 3（都 ≤ 3 进告知）；桶 5 remain 5 不进。
        assert!(g2.notices.contains(&0) && g2.notices.contains(&2) && g2.notices.contains(&3));
        assert!(!g2.notices.contains(&5));
        // 未告知 ⇒ expire 路径不可用；自救走压缩（不静默删桶）。
        let mut g3 = StoreGovernor::new(100, OverPolicy::ExpireFirst, 12);
        g3.admit(0, 60).ok();
        assert_eq!(g3.admit(1, 60), Ok(()));
        assert_eq!(g3.expired, 0);
        assert_eq!(g3.buckets.len(), 2);
        // 告知后可删：sweep 告知桶 0（remain 0 ≤ 3）→ 超限触发 expire。
        let mut g4 = StoreGovernor::new(100, OverPolicy::ExpireFirst, 12);
        g4.admit(0, 60).ok();
        g4.sweep_notices(12);
        assert!(g4.notices.contains(&0));
        g4.admit(1, 60).ok();
        assert_eq!(g4.expired, 1);
        assert!(!g4.buckets.iter().any(|b| b.period == 0));
    }

    #[test]
    fn a04_治理透明_水位压缩比可查() {
        let mut g = StoreGovernor::new(1_000, OverPolicy::CompressFirst, 12);
        g.admit(1, 400).ok();
        let (used, quota, _ratio, comp) = g.transparent().unwrap_or((0, 0, 0, 0));
        assert_eq!((used, quota, comp), (400, 1_000, 0));
        assert_eq!(g.watermark_pm(), 400);
        // 压缩后水位下降、压缩计数可见。
        g.admit(2, 400).ok();
        g.admit(3, 300).ok(); // 触发压缩
        let (used2, _, _, comp2) = g.transparent().unwrap_or((0, 0, 0, 0));
        assert_eq!(comp2, 1);
        assert!(used2 < 1_000);
        // 零配额不可查。
        let g0 = StoreGovernor::new(0, OverPolicy::CompressFirst, 12);
        assert_eq!(g0.transparent(), Err(E_GOV_OPAQUE));
    }

    #[test]
    fn a05_策略枚举_两策略都可达() {
        // ExpireFirst：超限先走过期。
        let mut g = StoreGovernor::new(500, OverPolicy::ExpireFirst, 12);
        g.admit(0, 300).ok();
        g.sweep_notices(100);
        g.notices.push(0);
        assert_eq!(g.admit(1, 300), Ok(()));
        assert_eq!(g.expired, 1);
        assert_eq!(g.buckets[0].period, 1);
        // 非法保留期回退。
        let g2 = StoreGovernor::new(100, OverPolicy::CompressFirst, 0);
        assert_eq!(g2.keep_months, DEFAULT_KEEP_MONTHS);
    }

}
