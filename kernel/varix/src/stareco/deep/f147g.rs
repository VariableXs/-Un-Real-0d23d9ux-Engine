//! 深化层四 · F147 跨设备主题同步（2026-09-27 深化批次四 · g 层）。
//!
//! 冲突解决策略表（字段类型×策略）、传输完整性通道（逐块 MAC 链）、
//! 多设备星型拓扑校验、档案配额清理（最旧淘汰）、同步事件日志回放
//! （幂等复跑）。

use crate::checks::CheckSet;
use crate::stareco::ebase;

// ---------------------------------------------------------------------------
// 冲突策略表：字段类型 → 策略（last-write/two-phase/manual）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Strategy {
    LastWrite,
    TwoPhase,
    Manual,
}

/// 策略表：数值/布尔可自动收敛；自由文本必须人工（机器不裁决审美）。
pub fn strategy_for(field_kind: &str) -> Strategy {
    match field_kind {
        "accent_color" | "font_size" | "dock_pos" | "dark_mode" => Strategy::LastWrite,
        "layout_custom" => Strategy::TwoPhase,
        "desktop_icon_layout" | "notes" => Strategy::Manual,
        _ => Strategy::Manual, // 未知字段保守走人工
    }
}

// ---------------------------------------------------------------------------
// 传输完整性通道：逐块 MAC = fnv(prev_mac ‖ block_fp)，首块锚定
// ---------------------------------------------------------------------------

pub const MAC_SEED: u64 = 0xC0FF_EE00_1234_5678;

pub fn block_mac(prev: u64, block_fp: u64) -> u64 {
    let mut buf = [0u8; 16];
    buf[0..8].copy_from_slice(&prev.to_be_bytes());
    buf[8..16].copy_from_slice(&block_fp.to_be_bytes());
    ebase::fnv1a64(&buf)
}

/// 通道校验：重放全部块 MAC 与随行链对照，首个偏离即坏块序号。
pub fn verify_channel(block_fps: &[u64], macs: &[u64]) -> Result<(), usize> {
    if block_fps.len() != macs.len() {
        return Err(block_fps.len().min(macs.len()));
    }
    let mut mac = MAC_SEED;
    for (i, fp) in block_fps.iter().enumerate() {
        mac = block_mac(mac, *fp);
        if mac != macs[i] {
            return Err(i);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 星型拓扑：每设备恰好连到一个 hub；hub 恰一个
// ---------------------------------------------------------------------------

/// links: (设备, 对端)；hub 即对端重复最多的那台。
pub fn star_topology_ok(devices: &[&'static str], links: &[(&'static str, &'static str)]) -> Result<(), &'static str> {
    if devices.len() < 2 {
        return Err("单设备无拓扑");
    }
    // hub = 出现次数最多的对端。
    let mut counts: alloc::vec::Vec<(&'static str, usize)> = alloc::vec::Vec::new();
    for (_, peer) in links {
        match counts.iter_mut().find(|(n, _)| n == peer) {
            Some((_, c)) => *c += 1,
            None => counts.push((peer, 1)),
        }
    }
    let hub = counts.iter().max_by_key(|(_, c)| *c).map(|(n, _)| *n).ok_or("零链路")?;
    let hub_count = counts.iter().find(|(n, _)| *n == hub).map(|(_, c)| *c).expect("hub");
    // 每设备恰好一条链，且全部指向 hub。
    for d in devices {
        if *d == hub {
            continue; // hub 是汇聚点，不要求自身有出链
        }
        let l: alloc::vec::Vec<_> = links.iter().filter(|(f, _)| f == d).collect();
        if l.len() != 1 {
            return Err("设备链路数不为 1");
        }
        if l[0].1 != hub {
            return Err("存在非 hub 链路：不是星型");
        }
    }
    if hub_count != devices.len() - 1 {
        return Err("hub 链路数与设备数不匹配");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 档案配额清理：超上限按 last_used_day 最旧淘汰（正在使用的豁免）
// ---------------------------------------------------------------------------

pub struct Archive {
    pub device: &'static str,
    pub last_used_day: u32,
    pub in_use: bool,
}

/// 返回待淘汰名单（最旧优先），使保留数 ≤ cap；使用中档案豁免。
pub fn cleanup_plan(archives: &mut alloc::vec::Vec<Archive>, cap: usize) -> alloc::vec::Vec<&'static str> {
    // 冒泡排序：键 (in_use, last_used_day) 升序——使用中沉底、最旧在前。
    for i in 0..archives.len() {
        for j in 1..archives.len() - i {
            let a = (archives[j - 1].in_use, archives[j - 1].last_used_day);
            let b = (archives[j].in_use, archives[j].last_used_day);
            if a > b {
                archives.swap(j - 1, j);
            }
        }
    }
    let mut out: alloc::vec::Vec<&'static str> = alloc::vec::Vec::new();
    let mut keep = archives.len();
    for a in archives.iter() {
        if keep <= cap {
            break;
        }
        if a.in_use {
            continue; // 使用中豁免
        }
        out.push(a.device);
        keep -= 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 同步事件日志回放：事件流 → 终态；幂等复跑结果一致
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub enum SyncEvent {
    Set(&'static str, u32),
    Del(&'static str),
}

/// 回放：Set 覆盖 / Del 删除（不存在则忽略——幂等）。
pub fn replay(events: &[SyncEvent]) -> alloc::vec::Vec<(&'static str, u32)> {
    let mut state: alloc::vec::Vec<(&'static str, u32)> = alloc::vec::Vec::new();
    for e in events {
        match e {
            SyncEvent::Set(k, v) => match state.iter_mut().find(|(sk, _)| sk == k) {
                Some(entry) => entry.1 = *v,
                None => state.push((k, *v)),
            },
            SyncEvent::Del(k) => state.retain(|(sk, _)| sk != k),
        }
    }
    state
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F147G_TAG: &str = "stareco-F147-deep4";

pub fn run_f147_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new(F147G_TAG);

    // 策略表
    set.add(
        "f147g strategy",
        strategy_for("accent_color") == Strategy::LastWrite
            && strategy_for("layout_custom") == Strategy::TwoPhase
            && strategy_for("notes") == Strategy::Manual,
        "三类策略",
    );
    set.add("f147g unknown manual", strategy_for("whatever") == Strategy::Manual, "未知字段保守人工");

    // 传输通道
    let fps = [1u64, 2, 3];
    let mut macs: alloc::vec::Vec<u64> = alloc::vec::Vec::new();
    let mut mac = MAC_SEED;
    for fp in fps {
        mac = block_mac(mac, fp);
        macs.push(mac);
    }
    set.add("f147g channel ok", verify_channel(&fps, &macs).is_ok(), "通道完整");
    let mut bad = fps;
    bad[1] = 99;
    set.add("f147g channel localize", verify_channel(&bad, &macs) == Err(1), "坏块定位 1");
    set.add("f147g channel len", verify_channel(&[1], &macs) == Err(1), "块数矛盾");

    // 星型拓扑
    let devs = ["phone", "pad", "pc"];
    set.add(
        "f147g star ok",
        star_topology_ok(&devs, &[("phone", "pc"), ("pad", "pc")]).is_ok(),
        "pc 为 hub 的星型",
    );
    set.add(
        "f147g star mesh",
        star_topology_ok(&devs, &[("phone", "pc"), ("pad", "phone")]).is_err(),
        "混链非星型拒绝",
    );
    set.add(
        "f147g star dup",
        star_topology_ok(&devs, &[("phone", "pc"), ("phone", "pc")]).is_err(),
        "重复链路拒绝",
    );

    // 配额清理
    let mut arcs = alloc::vec![
        Archive { device: "old-pad", last_used_day: 10, in_use: false },
        Archive { device: "busy-phone", last_used_day: 5, in_use: true },
        Archive { device: "mid-pc", last_used_day: 30, in_use: false },
    ];
    let plan = cleanup_plan(&mut arcs, 1);
    set.add(
        "f147g cleanup",
        plan == alloc::vec!["old-pad", "mid-pc"] && arcs[2].device == "busy-phone",
        "cap=1 淘汰两台（使用中豁免）且使用中沉底",
    );

    // 回放幂等
    let events = [
        SyncEvent::Set("accent", 3),
        SyncEvent::Set("font", 14),
        SyncEvent::Del("accent"),
        SyncEvent::Set("font", 16),
    ];
    let s1 = replay(&events);
    let s2 = replay(&events);
    set.add("f147g replay", s1 == alloc::vec![("font", 16)], "回放终态");
    set.add("f147g idempotent", s1 == s2, "复跑一致");

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn channel_empty_ok() {
        assert!(verify_channel(&[], &[]).is_ok());
    }

    #[test]
    fn replay_del_missing_is_fine() {
        let s = replay(&[SyncEvent::Del("ghost")]);
        assert!(s.is_empty());
    }
}
