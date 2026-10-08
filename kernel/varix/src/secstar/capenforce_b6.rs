//! F177 能力执法可视化 · 批次六深化（secstar · G-G-07）。
//!
//! 批次六功能面（达成率 90%——小批收尾：拦截热力与白名单）：
//! - [`HeatByPoint`]：四执法点热力——拦截计数归一化热度 ‰
//!   （哪里最忙一眼可见——b4 busiest 的量化版）；
//! - [`SilentWhitelist`]：静默白名单——用户明确「不再提醒」的应用
//!   （白名单≠豁免：执法照常只是不通知——与免打扰正交）；
//! - [`whitelist_persist`]：白名单持久帧（跨重启记住选择——撕裂拒）。
//!
//! 零堆纪律：定长表 + 定长帧，无 alloc。

use super::capenforce::POINT_N;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 执法点热力
// ---------------------------------------------------------------------------

/// 热力归一：最大点 = 1000，其余按比例（最大为 0 → 全 0）。
pub fn heat_by_point(intercepts: [u64; POINT_N]) -> [u32; POINT_N] {
    let max = intercepts.iter().copied().max().unwrap_or(0);
    if max == 0 {
        return [0; POINT_N];
    }
    let mut out = [0u32; POINT_N];
    for (o, v) in out.iter_mut().zip(intercepts.iter()) {
        // u128 中间量——u64::MAX 级计数不溢出。
        *o = ((*v as u128 * 1_000) / max as u128) as u32;
    }
    out
}

// ---------------------------------------------------------------------------
// 静默白名单
// ---------------------------------------------------------------------------

/// 白名单容量。
pub const SILENT_CAP: usize = 16;

#[derive(Clone, Copy)]
pub struct SilentWhitelist {
    ids: [Option<u32>; SILENT_CAP],
    pub n: usize,
}

impl SilentWhitelist {
    pub const fn new() -> SilentWhitelist {
        SilentWhitelist { ids: [const { None }; SILENT_CAP], n: 0 }
    }

    /// 加入（去重——重复点「不再提醒」不占双槽）。
    pub fn add(&mut self, app_id: u32) -> bool {
        if self.ids[..self.n].contains(&Some(app_id)) {
            return false;
        }
        if self.n >= SILENT_CAP {
            return false;
        }
        self.ids[self.n] = Some(app_id);
        self.n += 1;
        true
    }

    pub fn contains(&self, app_id: u32) -> bool {
        self.ids[..self.n].contains(&Some(app_id))
    }

    /// 移除（设置页可撤销「不再提醒」——可发现可找回）。
    pub fn remove(&mut self, app_id: u32) -> bool {
        match self.ids[..self.n].iter().position(|x| *x == Some(app_id)) {
            Some(i) => {
                for j in i..self.n - 1 {
                    self.ids[j] = self.ids[j + 1];
                }
                self.ids[self.n - 1] = None;
                self.n -= 1;
                true
            }
            None => false,
        }
    }
}

/// 白名单持久帧：[0..2) "XW" · [2] 条数 · [3..7)×16 id LE 压缩 · 末 2B 校验和。
pub const WHITELIST_FRAME_LEN: usize = 70;

fn fnv16(data: &[u8]) -> u16 {
    let mut h: u32 = 0x811c9dc5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    ((h >> 16) ^ h) as u16
}

pub fn encode_whitelist(ids: &[u32], out: &mut [u8; WHITELIST_FRAME_LEN]) -> bool {
    if ids.len() > SILENT_CAP {
        return false;
    }
    out[0] = b'X';
    out[1] = b'W';
    out[2] = ids.len() as u8;
    for b in out[3..].iter_mut() {
        *b = 0;
    }
    for (i, id) in ids.iter().enumerate() {
        out[3 + i * 4..7 + i * 4].copy_from_slice(&id.to_le_bytes());
    }
    let c = fnv16(&out[..68]);
    out[68] = (c & 0xFF) as u8;
    out[69] = (c >> 8) as u8;
    true
}

pub fn decode_whitelist(frame: &[u8; WHITELIST_FRAME_LEN]) -> Option<([u32; SILENT_CAP], usize)> {
    if frame[0] != b'X' || frame[1] != b'W' || frame[2] as usize > SILENT_CAP {
        return None;
    }
    let want = (frame[69] as u16) << 8 | frame[68] as u16;
    if fnv16(&frame[..68]) != want {
        return None;
    }
    let n = frame[2] as usize;
    let mut ids = [0u32; SILENT_CAP];
    for i in 0..n {
        ids[i] = u32::from_le_bytes(frame[3 + i * 4..7 + i * 4].try_into().ok()?);
    }
    Some((ids, n))
}

// ---------------------------------------------------------------------------
// 批次六自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_capenforce_b6_checks() -> CheckSet {
    let mut cs = CheckSet::new("F177-b6");

    // 1) 热力归一：最大点 1000、其余按比例（量化热力面）。
    let heat = heat_by_point([100, 50, 25, 0]);
    cs.add(
        "heat_normalized",
        heat == [1_000, 500, 250, 0],
        "",
    );

    // 2) 零拦截全零：无拦截无热力（不编造）。
    cs.add("heat_zero", heat_by_point([0; POINT_N]) == [0; POINT_N], "");

    // 3) 白名单去重：重复加入不占双槽（容量诚实）。
    let mut w = SilentWhitelist::new();
    let a = w.add(7);
    let dup = w.add(7);
    cs.add("whitelist_dedup", a && !dup && w.n == 1 && w.contains(7), "");

    // 4) 白名单移除：设置页撤销「不再提醒」（可发现可找回）。
    let removed = w.remove(7);
    let gone = w.remove(7);
    cs.add("whitelist_removable", removed && !gone && !w.contains(7), "");

    // 5) 白名单帧 round-trip：三 id 跨重启保真。
    let mut frame = [0u8; WHITELIST_FRAME_LEN];
    let enc = encode_whitelist(&[3, 9, 42], &mut frame);
    let (ids, n) = decode_whitelist(&frame).unwrap();
    cs.add("whitelist_frame_roundtrip", enc && n == 3 && ids[0] == 3 && ids[1] == 9 && ids[2] == 42, "");

    // 6) 白名单帧撕裂必拒：任一字节翻转 → 校验和关拦。
    let mut torn_all = true;
    for i in 0..WHITELIST_FRAME_LEN {
        let mut t = frame;
        t[i] ^= 0x77;
        torn_all &= decode_whitelist(&t).is_none();
    }
    cs.add("whitelist_frame_tears", torn_all, "");

    // 7) 白名单帧超容拒：17 条不出门（16 上限）。
    let too_many = [0u32; 17];
    cs.add("whitelist_frame_cap", !encode_whitelist(&too_many, &mut frame), "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次六）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b6 {
    use super::*;

    #[test]
    fn heat_extremes_stable() {
        // 极值热力：u64::MAX 与 1 同现 → 1000 与 0（归一不溢出）。
        let heat = heat_by_point([u64::MAX, 1, 0, 0]);
        assert_eq!(heat[0], 1_000);
        assert_eq!(heat[1], 0);
    }

    #[test]
    fn whitelist_cap_boundary() {
        // 16 满容、第 17 拒（容量边界）。
        let mut w = SilentWhitelist::new();
        for i in 0..SILENT_CAP as u32 {
            assert!(w.add(i));
        }
        assert!(!w.add(999));
        assert_eq!(w.n, SILENT_CAP);
    }
}
