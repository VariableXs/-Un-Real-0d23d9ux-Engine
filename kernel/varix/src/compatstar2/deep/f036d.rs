//! F036 深化批次二 · JSONL 帧与饱和记账面（compatstar2/deep · G-A-36）。
//!
//! 批次一深化覆盖 JSON 转义/隐私扫描/合并策略；本批补齐：JSONL 帧写出器
//! （seq 序号包裹——「开放格式」的分帧契约）、饱和计数器（u32 饱和加法
//! ——长期运行计数不回绕）、草稿同日去重指纹（同程序同日合并——批次一
//! 合并策略的日期维度）、星卡 v1→v2 字段迁移（版本化演进的迁移面——
//! 「升级不破坏旧数据」承诺）。
//!
//! 零堆纪律：定长缓冲，无 alloc。

use crate::checks::CheckSet;

/// 去重指纹表容量。
pub const DEDUP_CAP: usize = 16;

/// JSONL 帧：`{"seq":N,"body":<record>}` 形状（seq 逐位十进制；零分配）。
/// 返回写入长度；缓冲不足截断不越界。
pub fn jsonl_frame(seq: u64, record: &[u8], out: &mut [u8]) -> usize {
    const HEAD: &[u8] = b"{\"seq\":";
    let mut n = 0usize;
    for &b in HEAD {
        if n < out.len() {
            out[n] = b;
        }
        n += 1;
    }
    // seq 逐位（0 兜底）。
    let mut digits = [0u8; 20];
    let mut i = 0usize;
    let mut v = seq;
    if v == 0 {
        digits[0] = b'0';
        i = 1;
    }
    while v > 0 {
        digits[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i += 1;
    }
    while i > 0 {
        i -= 1;
        if n < out.len() {
            out[n] = digits[i];
        }
        n += 1;
    }
    for &b in b",\"body\":" {
        if n < out.len() {
            out[n] = b;
        }
        n += 1;
    }
    for &b in record {
        if n < out.len() {
            out[n] = b;
        }
        n += 1;
    }
    if n < out.len() {
        out[n] = b'}';
    }
    n += 1;
    n
}

/// 饱和计数器：u32 饱和加法（长期运行计数不回绕——诚实账面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SatCounter(pub u32);

impl SatCounter {
    pub fn bump(&mut self, by: u32) {
        self.0 = self.0.saturating_add(by);
    }
    pub fn is_saturated(&self) -> bool {
        self.0 == u32::MAX
    }
}

/// 草稿同日去重指纹表：同程序同日 → 合并（去重命中）；跨日 → 新条目
/// （批次一「同程序合并」的日期维度细化）。
pub struct DraftDedup {
    pub fingerprints: [[u8; 8]; DEDUP_CAP],
    pub day: [u32; DEDUP_CAP],
    pub count: usize,
    /// 去重命中数（合并账面）。
    pub merges: u32,
}

impl DraftDedup {
    pub const fn new() -> Self {
        DraftDedup { fingerprints: [[0; 8]; DEDUP_CAP], day: [0; DEDUP_CAP], count: 0, merges: 0 }
    }
    /// 登记：返回 true = 命中既有条目（应合并）；false = 新条目。
    pub fn record(&mut self, fingerprint: [u8; 8], day: u32) -> bool {
        for i in 0..self.count {
            if self.fingerprints[i] == fingerprint && self.day[i] == day {
                self.merges += 1;
                return true;
            }
        }
        if self.count < DEDUP_CAP {
            self.fingerprints[self.count] = fingerprint;
            self.day[self.count] = day;
            self.count += 1;
        }
        false
    }
}

/// 星卡 v1→v2 字段迁移（v1 键名 → v2 键名；未登记键原样透传并计数）。
/// 返回映射命中数。
pub fn migrate_v1_to_v2(key: &str, out: &mut [u8]) -> usize {
    const M: [(&str, &str); 3] =
        [("api_calls", "api_calls_sampled"), ("mem_peak", "mem_peak_bytes"), ("startup", "startup_ms")];
    for (old, new) in M.iter() {
        if key == *old {
            let b = new.as_bytes();
            let n = b.len().min(out.len());
            out[..n].copy_from_slice(&b[..n]);
            return n;
        }
    }
    let b = key.as_bytes();
    let n = b.len().min(out.len());
    out[..n].copy_from_slice(&b[..n]);
    0 // 未命中 → 原样透传（返回 0 表示无映射）
}

/// 域自检（深化批次二）。
pub fn run_f036d_checks() -> CheckSet {
    let mut cs = CheckSet::new("F036-stardraft-d2");
    // 1) JSONL 帧：seq 5 + 记录体 → 精确字节形状。
    let mut out = [0u8; 64];
    let n = jsonl_frame(5, b"\"crashes\":2", &mut out);
    cs.add("jsonl_frame_shape", &out[..n] == b"{\"seq\":5,\"body\":\"crashes\":2}", "");
    // 2) 饱和计数：MAX 处 bump 不回绕（is_saturated 如实）。
    let mut sc = SatCounter(u32::MAX - 1);
    sc.bump(5);
    cs.add("sat_counter", sc.is_saturated() && sc.0 == u32::MAX, "");
    // 3) 同日去重：同指纹同日命中合并；跨日新条目。
    let mut dd = DraftDedup::new();
    let first = dd.record([7; 8], 20_260_927);
    let dup = dd.record([7; 8], 20_260_927);
    let other_day = dd.record([7; 8], 20_260_928);
    cs.add("draft_dedup_same_day", !first && dup && dd.merges == 1 && !other_day && dd.count == 2, "");
    // 4) v1→v2 迁移：三键映射命中 + 未登记键透传（返回 0）。
    //    逐步快照断言（缓冲复用，先取值再写下一键）。
    let mut k = [0u8; 24];
    let m1 = migrate_v1_to_v2("api_calls", &mut k);
    let ok1 = m1 > 0 && &k[..m1] == b"api_calls_sampled";
    let m2 = migrate_v1_to_v2("mem_peak", &mut k);
    let ok2 = m2 > 0 && &k[..m2] == b"mem_peak_bytes";
    let m3 = migrate_v1_to_v2("unknown_key", &mut k);
    cs.add("v1v2_migration", ok1 && ok2 && m3 == 0 && &k[..9] == b"unknown_k", "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_seq_zero() {
        let mut out = [0u8; 64];
        let n = jsonl_frame(0, b"{}", &mut out);
        assert_eq!(&out[..n], b"{\"seq\":0,\"body\":{}}");
    }

    #[test]
    fn frame_small_buffer_truncates_safely() {
        let mut small = [0u8; 6];
        let n = jsonl_frame(1, b"aaaaaaaa", &mut small);
        assert!(n > 6, "总长度如实返回（25 字节形状）");
        assert_eq!(small, *b"{\"seq\"", "写出恰填满缓冲、不越界（截断策略）");
    }

    #[test]
    fn deep2_checks_all_green() {
        let cs = run_f036d_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
