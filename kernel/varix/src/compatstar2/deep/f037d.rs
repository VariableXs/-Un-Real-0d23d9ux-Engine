//! F037 深化批次二 · 布隆前置与规则裁决面（compatstar2/deep · G-A-37）。
//!
//! 批次一深化覆盖三型规则引擎/风险清单/角标状态机；本批补齐：布隆前置
//! 过滤器（8 位小布隆——规则表全扫前的 O(1) 剪枝，无假阴性保证）、
//! deny-first 裁决（任一拦截规则命中即拦——规则冲突的优先级语义）、
//! 审计链头锚点（导出校验的链头指纹十六进制）、信任窗剩余时间（饱和
//! 倒计时——通知卡显示的数据源）。
//!
//! 零堆纪律：u8 位图与定长缓冲，无 alloc。

use crate::checks::CheckSet;

/// 布隆位宽（u8 承载 8 位）。
pub const BLOOM_BITS: u32 = 8;

fn mix64(x: u64) -> u64 {
    x.wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left(29)
}

/// 小布隆（8 位）：插入置位、查询位判。性质：查询为 false ⇒ 一定不在
/// （无假阴性）；为 true 可能假阳性——只作全扫前剪枝，不替代裁决。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Bloom8(pub u8);

impl Bloom8 {
    pub const fn new() -> Self {
        Bloom8(0)
    }
    pub fn insert(&mut self, key: u64) {
        let bit = (mix64(key) % BLOOM_BITS as u64) as u8;
        self.0 |= 1 << bit;
    }
    pub fn maybe_contains(&self, key: u64) -> bool {
        let bit = (mix64(key) % BLOOM_BITS as u64) as u8;
        self.0 >> bit & 1 == 1
    }
}

/// deny-first 裁决：规则串行扫描，任一 Intercept 立即生效——允许规则
/// 不能抵消拦截规则（规则冲突的优先级语义，保守正确）。
pub fn deny_first_verdict(verdicts: &[bool]) -> bool {
    // true = 拦截。任一 true → 拦截（短路语义）。
    verdicts.iter().any(|&v| v)
}

/// 审计链头锚点：链尾指纹 → 大写十六进制（导出校验的链头标识，
/// 接收方重放链后比对锚点即知导出完整性）。
pub fn chain_head_anchor(head_fp: u64, out: &mut [u8]) -> usize {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut n = 0usize;
    let bytes = head_fp.to_be_bytes();
    for &b in bytes.iter() {
        if n + 2 <= out.len() {
            out[n] = HEX[(b >> 4) as usize];
            out[n + 1] = HEX[(b & 0xF) as usize];
        }
        n += 2;
    }
    n.min(out.len())
}

/// 信任窗剩余：now 超窗 → 0（饱和，不回绕）。
pub fn trust_remaining_ms(override_epoch_ms: u64, now_epoch_ms: u64, window_ms: u64) -> u64 {
    override_epoch_ms.saturating_add(window_ms).saturating_sub(now_epoch_ms)
}

/// 域自检（深化批次二）。
pub fn run_f037d_checks() -> CheckSet {
    let mut cs = CheckSet::new("F037-peblockui-d2");
    // 1) 布隆无假阴性：20 键插入后逐键查询必真（剪枝安全性的直接对拍）。
    let mut bl = Bloom8::new();
    for i in 0..20u64 {
        bl.insert(i.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    }
    let mut no_false_negative = true;
    for i in 0..20u64 {
        no_false_negative &= bl.maybe_contains(i.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    }
    cs.add("bloom_no_false_negative", no_false_negative && bl.0 != 0, "");
    // 2) 空布隆查询恒假（未插入即不含——剪枝不误放的前提）。
    cs.add("bloom_empty_negative", !Bloom8::new().maybe_contains(42), "");
    // 3) deny-first：拦截规则混在允许中仍拦；全允许才放行。
    cs.add(
        "deny_first",
        deny_first_verdict(&[false, true, false]) && deny_first_verdict(&[true]) && !deny_first_verdict(&[false, false]),
        "",
    );
    // 4) 链头锚点：8 字节 → 16 大写十六进制（形状 + 已知值）。
    let mut anchor = [0u8; 16];
    let n = chain_head_anchor(0xDEAD_BEEF_CAFE_F00D, &mut anchor);
    cs.add("chain_anchor_hex", n == 16 && &anchor == b"DEADBEEFCAFEF00D", "");
    // 5) 信任窗剩余：窗内递减、窗外饱和归零（通知卡数据源）。
    cs.add(
        "trust_remaining",
        trust_remaining_ms(1000, 1001, 1000) == 999
            && trust_remaining_ms(1000, 2001, 1000) == 0
            && trust_remaining_ms(0, u64::MAX, 1000) == 0,
        "",
    );
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_small_buffer_safe() {
        let mut small = [0u8; 5];
        let n = chain_head_anchor(0xFFFF, &mut small);
        assert!(n <= 5, "缓冲不足不越界（截断如实）");
    }

    #[test]
    fn bloom_inserts_set_bits() {
        let mut bl = Bloom8::new();
        for i in 0..64u64 {
            bl.insert(i);
        }
        assert_eq!(bl.0, 0xFF, "64 键插入 → 8 位全热（鸽笼上界）");
    }

    #[test]
    fn deep2_checks_all_green() {
        let cs = run_f037d_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
