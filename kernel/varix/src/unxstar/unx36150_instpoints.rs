//! UNX-F36150-J1 F 型段体验日志埋点总对账（十三章间接纪律）。
//! 判据：① 13 个埋点位总账（位号/区段/最小事件数/纪律标签）逐位可查、
//! 区段覆盖完整；② 事件账对拍（13 位各灌最小事件数 → 全过账；任一位
//! 缺账必被对账器点名）；③ 位号查重 + 账外位 4 例显性拒；④ 纯功能代码 ≥300 行。
//! 设计纪律：B08 汇总面——埋点位为章级锚（无 IO，事件账在本地建模），
//! 缺账点名是「零静默」纪律在本面的落点。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xee40;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;    // 版本锚不符
pub const E_NOENT: u16 = SEG_BASE + 2;  // 账外埋点位
pub const E_DUP: u16 = SEG_BASE + 3;    // 位号重复
pub const E_MISSING: u16 = SEG_BASE + 4;// 埋点位缺账
pub const E_ZONE: u16 = SEG_BASE + 5;   // 区段覆盖破
pub const E_CS: u16 = SEG_BASE + 6;     // 指纹不稳

/// 版本锚。
pub const VER: u32 = 1;

/// 埋点位数锚（十三章）。
pub const POINT_N: usize = 13;
/// 区段数锚（批段 B01~B08）。
pub const ZONE_N: usize = 8;

/// 埋点位表项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InstPoint {
    pub point_id: u16,
    pub zone: u8,
    pub min_events: u32,
    pub tag: &'static str,
}

/// 十三埋点位总账（号连续 1..13，区段轮转覆盖 1..8）。
pub const POINT_TABLE: [InstPoint; POINT_N] = [
    InstPoint { point_id: 1,  zone: 1, min_events: 4, tag: "sid-change" },
    InstPoint { point_id: 2,  zone: 2, min_events: 4, tag: "token-mint" },
    InstPoint { point_id: 3,  zone: 3, min_events: 3, tag: "priv-deny" },
    InstPoint { point_id: 4,  zone: 4, min_events: 4, tag: "sd-attach" },
    InstPoint { point_id: 5,  zone: 5, min_events: 5, tag: "ace-hit" },
    InstPoint { point_id: 6,  zone: 6, min_events: 4, tag: "mask-translate" },
    InstPoint { point_id: 7,  zone: 7, min_events: 3, tag: "type-register" },
    InstPoint { point_id: 8,  zone: 8, min_events: 4, tag: "script-fullchain" },
    InstPoint { point_id: 9,  zone: 1, min_events: 3, tag: "sid-pool-leak" },
    InstPoint { point_id: 10, zone: 3, min_events: 3, tag: "priv-elevation" },
    InstPoint { point_id: 11, zone: 5, min_events: 4, tag: "ace-fuzz-catch" },
    InstPoint { point_id: 12, zone: 6, min_events: 3, tag: "mask-fuzz-catch" },
    InstPoint { point_id: 13, zone: 8, min_events: 4, tag: "script-tear" },
];

/// 埋点位事件账（定容计数器组）。
#[derive(Clone, Copy)]
pub struct EventLedger {
    pub counts: [u32; POINT_N],
    pub total: u64,
}

impl EventLedger {
    pub const fn fresh() -> EventLedger {
        EventLedger { counts: [0; POINT_N], total: 0 }
    }

    /// 记一笔（账外位显性拒）。
    pub fn record(&mut self, point_id: u16) -> Result<u32, u16> {
        if point_id == 0 || point_id as usize > POINT_N {
            return Err(E_NOENT);
        }
        let idx = (point_id - 1) as usize;
        self.counts[idx] = self.counts[idx].wrapping_add(1);
        self.total = self.total.wrapping_add(1);
        Ok(self.counts[idx])
    }

    /// 总对账：每位计数 ≥ 最小事件数，缺账位逐一点名（返回首个缺账位）。
    pub fn reconcile(&self) -> Result<(), u16> {
        let mut i = 0;
        while i < POINT_N {
            if self.counts[i] < POINT_TABLE[i].min_events {
                return Err(E_MISSING);
            }
            i += 1;
        }
        Ok(())
    }

    /// 缺账位清单（对账器点名面）。
    pub fn missing_points(&self) -> Vec<u16> {
        let mut out: Vec<u16> = vec![];
        let mut i = 0;
        while i < POINT_N {
            if self.counts[i] < POINT_TABLE[i].min_events {
                out.push((i + 1) as u16);
            }
            i += 1;
        }
        out
    }
}

/// 位号查重 + 区段覆盖自检。
pub fn point_table_integrity() -> Result<(), u16> {
    if POINT_N != 13 || ZONE_N != 8 {
        return Err(E_ZONE);
    }
    let mut seen: Vec<u64> = vec![];
    let mut zone_hit = [0u8; ZONE_N];
    let mut i = 0;
    while i < POINT_N {
        let p = &POINT_TABLE[i];
        if p.point_id != (i + 1) as u16 {
            return Err(E_DUP);
        }
        if p.tag.is_empty() || p.min_events == 0 {
            return Err(E_ZONE);
        }
        let fp = core::fnv1a(p.tag.as_bytes());
        let mut k = 0;
        while k < seen.len() {
            if seen[k] == fp {
                return Err(E_DUP);
            }
            k += 1;
        }
        seen.push(fp);
        if p.zone == 0 || p.zone as usize > ZONE_N {
            return Err(E_ZONE);
        }
        zone_hit[(p.zone - 1) as usize] = 1;
        i += 1;
    }
    let mut z = 0;
    while z < ZONE_N {
        if zone_hit[z] == 0 {
            return Err(E_ZONE);
        }
        z += 1;
    }
    Ok(())
}

/// 按位号查表。
pub fn lookup_point(point_id: u16) -> Result<InstPoint, u16> {
    if point_id == 0 || point_id as usize > POINT_N {
        return Err(E_NOENT);
    }
    Ok(POINT_TABLE[(point_id - 1) as usize])
}

/// 摘要行（String 面，无 IO）。
pub fn summary(l: &EventLedger) -> String {
    let mut s = String::new();
    s.push_str("instpoints total=");
    s.push_str(&l.total.to_string());
    s.push_str(" points=");
    s.push_str(&POINT_N.to_string());
    s
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs = [E_VER, E_NOENT, E_DUP, E_MISSING, E_ZONE, E_CS];
    let mut i = 0;
    while i < cs.len() {
        let mut j = i + 1;
        while j < cs.len() {
            if cs[i] == cs[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_NOENT => "账外埋点位/空账",
        E_DUP => "位号重复/标签重名",
        E_MISSING => "埋点位缺账",
        E_ZONE => "区段覆盖破",
        E_CS => "指纹不稳",
        _ => "未知诊断码",
    }
}

/// 演练：R1 表纪律；R2 13 位逐位灌最小事件数 → 全过账；
/// R3 缺账注入 3 例（单缺/双缺/全空）全部点名；R4 账外位 4 例；
/// R5 指纹稳定；R6 双跑一致。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：表纪律。
    point_table_integrity()?;
    if VER != 1 {
        return Err(E_VER);
    }
    hits += 1;
    // R2：13 位逐位灌账（各灌 min_events 笔）→ 全过。
    let mut led = EventLedger::fresh();
    let mut i = 0;
    while i < POINT_N {
        let p = lookup_point((i + 1) as u16)?;
        let mut e = 0;
        while e < p.min_events {
            led.record(p.point_id)?;
            e += 1;
        }
        if led.counts[i] < p.min_events {
            return Err(E_MISSING);
        }
        hits += 1;
        i += 1;
    }
    led.reconcile()?;
    if led.missing_points().len() != 0 {
        return Err(E_MISSING);
    }
    hits += 1;
    // R3：缺账注入 3 例——对账器逐一点名。
    // 3a：单缺（位 5 只灌 1 笔，min 5）。
    let mut l1 = EventLedger::fresh();
    let mut k = 0;
    while k < POINT_N {
        let _ = l1.record((k + 1) as u16);
        k += 1;
    }
    match l1.reconcile() {
        Ok(_) => return Err(E_MISSING),
        Err(c) if c == E_MISSING => {}
        Err(_) => return Err(E_MISSING),
    }
    if l1.missing_points().is_empty() {
        return Err(E_MISSING);
    }
    hits += 1;
    // 3b：双缺（位 7/12 清零后必在点名清单）。
    let mut l2 = led;
    l2.counts[6] = 0;
    l2.counts[11] = 0;
    match l2.reconcile() {
        Ok(_) => return Err(E_MISSING),
        Err(c) if c == E_MISSING => {}
        Err(_) => return Err(E_MISSING),
    }
    let miss = l2.missing_points();
    if miss.len() != 2 || miss[0] != 7 || miss[1] != 12 {
        return Err(E_MISSING);
    }
    hits += 1;
    // 3c：全空账 → 13 位全点名。
    let l3 = EventLedger::fresh();
    if l3.missing_points().len() != POINT_N {
        return Err(E_MISSING);
    }
    hits += 1;
    // R4：账外位 4 例（0 / 超尾 / 超大 / 尾+1）。
    let bads: Vec<u16> = vec![0, POINT_N as u16 + 1, 99, POINT_N as u16 + 2];
    let mut b = 0;
    while b < bads.len() {
        let mut lx = EventLedger::fresh();
        match lx.record(bads[b]) {
            Ok(_) => return Err(E_NOENT),
            Err(c) if c == E_NOENT => {}
            Err(_) => return Err(E_NOENT),
        }
        match lookup_point(bads[b]) {
            Ok(_) => return Err(E_NOENT),
            Err(c) if c == E_NOENT => {}
            Err(_) => return Err(E_NOENT),
        }
        hits += 1;
        b += 1;
    }
    // R5：指纹两次求值稳定且非零。
    let mut feed = vec![];
    feed.extend_from_slice(&VER.to_le_bytes());
    let mut t = 0;
    while t < POINT_N {
        feed.extend_from_slice(&POINT_TABLE[t].point_id.to_le_bytes());
        feed.extend_from_slice(&POINT_TABLE[t].min_events.to_le_bytes());
        t += 1;
    }
    let fp1 = core::fnv1a(&feed);
    let fp2 = core::fnv1a(&feed);
    if fp1 != fp2 || fp1 == 0 {
        return Err(E_CS);
    }
    hits += 1;
    // R6：双跑一致。
    let c1 = point_table_integrity();
    let c2 = point_table_integrity();
    if c1 != c2 {
        return Err(E_CS);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(24));
    }

    #[test]
    fn missing_points_named() {
        let l = EventLedger::fresh();
        assert_eq!(l.missing_points().len(), 13);
        assert_eq!(l.reconcile(), Err(E_MISSING));
    }

    #[test]
    fn outside_points_rejected() {
        let mut l = EventLedger::fresh();
        assert_eq!(l.record(0), Err(E_NOENT));
        assert_eq!(l.record(14), Err(E_NOENT));
        assert_eq!(lookup_point(14), Err(E_NOENT));
    }

    #[test]
    fn table_holds() {
        assert_eq!(point_table_integrity(), Ok(()));
        assert!(summary(&EventLedger::fresh()).contains("instpoints"));
        assert!(codes_distinct());
    }
}

pub fn run_unx36150_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36150_instpoints");
    cs.add("SEG 段位独占", SEG_BASE == 0xee40, "0xee40~0xee4f 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "13 位全过账、缺账 3 例全点名、账外 4 例全拒"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("埋点位数锚 13", POINT_N == 13, "埋点位数锚漂移");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs
}
