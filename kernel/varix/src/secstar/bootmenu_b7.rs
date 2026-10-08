//! F171 引导选单 · 批次七深化（v7）——默认项策略账、选择记忆持久帧、
//! 无障碍高对比模式、按键重复（长按加速）模型。零堆、no_std。

use crate::checks::CheckSet;

/// 选单项上限（与主层对齐）。
pub const MENU_ITEMS: usize = 8;
/// 记忆帧长（8B）。
pub const MEMORY_FRAME_LEN: usize = 8;
/// 长按触发时延（ms）。
pub const REPEAT_DELAY_MS: u32 = 400;
/// 长按重复间隔（ms）。
pub const REPEAT_INTERVAL_MS: u32 = 80;
/// 高对比模式色对（前景/背景 ‰ 亮度差下限）。
pub const CONTRAST_MIN_PERMILLE: u32 = 700;

/// 默认项策略：三条规则按序裁决（记忆项 → 有效 → 0 号兜底）。
/// 记忆项越界/被移除时诚实回落 0 号（不猜测、不悬挂）。
pub fn default_item(memorized: Option<usize>, items: usize) -> usize {
    match memorized {
        Some(i) if i < items => i,
        _ => 0,
    }
}

/// 选择记忆持久帧（8B）：
/// [0..2) 魔数 "BM" · [2..4) 记忆项 LE · [4..6) 菜单项数 LE ·
/// [6..8) 校验和（前 6B FNV-16）。记忆项 ≥ 项数仍可入帧（读方负责回落）。
pub fn fnv16(data: &[u8]) -> u16 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h = (h ^ b as u32).wrapping_mul(0x0100_0193);
    }
    (h & 0xFFFF) as u16
}

pub fn encode_memory(memorized: u16, items: u16, out: &mut [u8; MEMORY_FRAME_LEN]) -> bool {
    if items == 0 || (memorized as usize) >= items as usize && memorized != 0xFFFF {
        return false; // 零菜单不记忆；越界记忆只允许「无记忆」哨兵
    }
    out[0] = b'B';
    out[1] = b'M';
    out[2..4].copy_from_slice(&memorized.to_le_bytes());
    out[4..6].copy_from_slice(&items.to_le_bytes());
    let c = fnv16(&out[..6]);
    out[6] = (c & 0xFF) as u8;
    out[7] = (c >> 8) as u8;
    true
}

pub fn decode_memory(frame: &[u8; MEMORY_FRAME_LEN]) -> Option<(u16, u16)> {
    if frame[0] != b'B' || frame[1] != b'M' {
        return None;
    }
    let want = (frame[7] as u16) << 8 | frame[6] as u16;
    if fnv16(&frame[..6]) != want {
        return None;
    }
    Some((
        u16::from_le_bytes(frame[2..4].try_into().ok()?),
        u16::from_le_bytes(frame[4..6].try_into().ok()?),
    ))
}

/// 帧读回 → 策略消费：解码 + 回落判定一条龙（哨兵 0xFFFF = 无记忆 → 0 号）。
pub fn memory_to_default(frame: &[u8; MEMORY_FRAME_LEN]) -> Option<usize> {
    let (mem, items) = decode_memory(frame)?;
    let m = if mem == 0xFFFF { None } else { Some(mem as usize) };
    Some(default_item(m, items as usize))
}

/// 长按重复模型：按下起 REPEAT_DELAY 后开始重复，每 REPEAT_INTERVAL 一步。
/// 事件序列推进（t 单调），返回本次推进移动了几步。
#[derive(Clone, Copy)]
pub struct KeyRepeater {
    held_since: Option<u32>,
    last_step_ms: u32,
    pub steps: u32,
}

impl KeyRepeater {
    pub const fn new() -> KeyRepeater {
        KeyRepeater { held_since: None, last_step_ms: 0, steps: 0 }
    }

    pub fn press(&mut self, now_ms: u32) {
        self.held_since = Some(now_ms);
        self.last_step_ms = now_ms;
        self.steps += 1; // 首按即一步（按下就有反馈——100ms 反馈铁律）
    }

    /// 推进：长按期间按间隔补步（首个重复步从 delay 点起算——
    /// delay 是「按键反馈」与「重复加速」的分界，不是可追帧的债务）。
    pub fn advance(&mut self, now_ms: u32) -> u32 {
        let held = match self.held_since {
            Some(s) => s,
            None => return 0,
        };
        let since = now_ms.saturating_sub(held);
        if since < REPEAT_DELAY_MS {
            return 0;
        }
        let mut moved = 0u32;
        if self.last_step_ms == held {
            // 首个重复步：锚定在 delay 点（400ms 那一拍只走一步）。
            self.last_step_ms = held + REPEAT_DELAY_MS;
            moved = 1;
            self.steps += 1;
        }
        while moved < MENU_ITEMS as u32 && now_ms.saturating_sub(self.last_step_ms) >= REPEAT_INTERVAL_MS {
            self.last_step_ms += REPEAT_INTERVAL_MS;
            moved += 1;
            self.steps += 1;
        }
        moved
    }

    pub fn release(&mut self) {
        self.held_since = None;
    }
}

/// 高对比判定：亮度差 ≥ CONTRAST_MIN_PERMILLE 才可读（亮度 ‰ 0=黑 1000=白）。
pub fn contrast_ok(fg_luma_permille: u32, bg_luma_permille: u32) -> bool {
    fg_luma_permille.abs_diff(bg_luma_permille) >= CONTRAST_MIN_PERMILLE
}

/// 高对比主题对：白字黑底 / 黑字白底两套（双向——亮暗环境都成立）。
pub const HC_THEMES: [(u32, u32); 2] = [(950, 30), (30, 950)];

#[inline(never)]
pub fn run_bootmenu_b7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F171-b7");

    // 1) 默认策略三路：记忆有效用记忆、记忆越界落 0、无记忆落 0。
    cs.add(
        "default_policy_three_paths",
        default_item(Some(2), 5) == 2 && default_item(Some(9), 5) == 0 && default_item(None, 5) == 0,
        "",
    );

    // 2) 空菜单兜底：0 项菜单 → 0（不 panic 不负数——菜单永远有项可选）。
    cs.add("default_empty_menu_safe", default_item(Some(0), 0) == 0, "");

    // 3) 记忆帧 round-trip：有效记忆往返一致。
    let mut f = [0u8; MEMORY_FRAME_LEN];
    assert!(encode_memory(3, 8, &mut f));
    cs.add("memory_frame_roundtrip", decode_memory(&f) == Some((3, 8)) && f[0] == b'B', "");

    // 4) 帧守门：零菜单拒、越界记忆拒（0xFFFF 哨兵除外）。
    let mut f2 = [0u8; MEMORY_FRAME_LEN];
    let bad0 = !encode_memory(0, 0, &mut f2);
    let bad_oob = !encode_memory(9, 8, &mut f2);
    let sentinel_ok = encode_memory(0xFFFF, 8, &mut f2);
    cs.add("memory_frame_guards", bad0 && bad_oob && sentinel_ok, "");

    // 5) 哨兵消费：0xFFFF → 无记忆 → 默认 0 号（一条龙不悬挂）。
    let mut f3 = [0u8; MEMORY_FRAME_LEN];
    encode_memory(0xFFFF, 8, &mut f3);
    cs.add("memory_sentinel_to_default", memory_to_default(&f3) == Some(0), "");

    // 6) 撕裂拒：8 字节逐一翻转全拦。
    let mut tear_ok = true;
    for i in 0..MEMORY_FRAME_LEN {
        let mut t = f;
        t[i] ^= 0x66;
        if decode_memory(&t).is_some() {
            tear_ok = false;
        }
    }
    cs.add("memory_frame_tear_proof", tear_ok, "");

    // 7) 长按重复：400ms 前不动、400ms 起每 80ms 一步（时序直核）。
    let mut k = KeyRepeater::new();
    k.press(1_000);
    let early = k.advance(1_300); // 300ms < 400 → 0
    let first = k.advance(1_400); // 恰 400 → 1
    let burst = k.advance(1_640); // +240ms = 3 步
    cs.add(
        "repeat_timing",
        early == 0 && first == 1 && burst == 3 && k.steps == 5,
        "",
    );

    // 8) 释放停步：release 后 advance 归零（手松了就停——不滑步）。
    k.release();
    cs.add("repeat_stops_on_release", k.advance(5_000) == 0, "");

    // 9) 单次推进钳制：极长缺帧一次最多补 8 步（防长按穿越菜单）。
    let mut k2 = KeyRepeater::new();
    k2.press(0);
    let huge = k2.advance(0 + REPEAT_DELAY_MS + 100 * REPEAT_INTERVAL_MS);
    cs.add("repeat_clamp_per_advance", huge == MENU_ITEMS as u32, "");

    // 10) 高对比：黑白 920 差过线、灰灰 50 差不过线（可读性量化）。
    cs.add(
        "contrast_quantified",
        contrast_ok(950, 30) && !contrast_ok(500, 450) && contrast_ok(30, 950),
        "",
    );

    // 11) 高对比主题表：两套主题全过线（表内即合法——配置面复核）。
    let all_ok = HC_THEMES.iter().all(|&(fg, bg)| contrast_ok(fg, bg));
    cs.add("hc_themes_valid", all_ok && HC_THEMES.len() == 2, "");

    // 12) 常量自洽：时延 400、间隔 80、对比线 700（手感三支点）。
    cs.add(
        "b7_constants",
        REPEAT_DELAY_MS == 400 && REPEAT_INTERVAL_MS == 80 && CONTRAST_MIN_PERMILLE == 700,
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeat_sustained_rate() {
        // 持续长按 1.6s：首步 + (1600-400)/80 = 16 步（节奏恒定复核）。
        let mut k = KeyRepeater::new();
        k.press(0);
        let mut total = 0;
        let mut t = 0u32;
        while t < 1_600 {
            t += 80;
            total += k.advance(t);
        }
        assert_eq!(total, 16);
    }

    #[test]
    fn memory_frame_survives_items_growth() {
        // 菜单扩容：旧帧记忆 3、新菜单 5 项 → 记忆仍有效（升级不丢用户选择）。
        let mut f = [0u8; MEMORY_FRAME_LEN];
        assert!(encode_memory(3, 4, &mut f));
        // 模拟读回时菜单已扩到 5：decode 得 (3,4)，策略层用新 items 裁决。
        let (mem, _) = decode_memory(&f).unwrap();
        assert_eq!(default_item(Some(mem as usize), 5), 3);
    }

    #[test]
    fn memory_frame_items_shrunk_falls_back() {
        // 菜单缩容：记忆 6、新菜单 4 → 回落 0（缩容不悬挂）。
        let mut f = [0u8; MEMORY_FRAME_LEN];
        assert!(encode_memory(6, 8, &mut f));
        let (mem, _) = decode_memory(&f).unwrap();
        assert_eq!(default_item(Some(mem as usize), 4), 0);
    }
}
