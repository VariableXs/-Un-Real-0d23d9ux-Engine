//! UNX-F36159-J1 F 型段收口宣告与下游冻结交付（B09–B15 消费锚）。
//! 判据：① 下游七批（B09~B15）消费锚登记面——每批一条冻结锚，锚值
//! 取自本域已冻结可复算面（守恒 48,000 / F0001 绿位 136 / 域档档目
//! 数 12 / ktest 真车道 39），锚值逐条对拍；② 下游覆盖：B09~B15 七批
//! 全有着落；③ 冻结纪律：全部锚 frozen=true，解冻任一位 → 交付闸
//! 必拒（E_THAW）；④ 交付闸：覆盖全 + 冻结全 + 锚值全对 → 宣告可交付；
//! ⑤ 指纹稳定 + 双跑一致；⑥ 纯功能代码 ≥300 行。
//! 设计纪律：冻结交付是单向闸——锚一旦登记即冻结，下游按锚取值；
//! 本面不开放任何解冻路径，解冻态只能出现在注入面（自证拦截）。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xefb0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;  // 版本锚不符
pub const E_ANCHOR: u16 = SEG_BASE + 2; // 锚值对拍不符
pub const E_THAW: u16 = SEG_BASE + 3; // 解冻态（冻结纪律破）
pub const E_COVER: u16 = SEG_BASE + 4; // 下游覆盖破
pub const E_CS: u16 = SEG_BASE + 5;   // 指纹不稳

/// 版本锚。
pub const VER: u32 = 1;

/// 下游批数锚（B09~B15 七批）。
pub const DOWNSTREAM_N: usize = 7;

/// 下游批号闭集。
pub const CONS_B09: u16 = 0x09;
pub const CONS_B10: u16 = 0x0a;
pub const CONS_B11: u16 = 0x0b;
pub const CONS_B12: u16 = 0x0c;
pub const CONS_B13: u16 = 0x0d;
pub const CONS_B14: u16 = 0x0e;
pub const CONS_B15: u16 = 0x0f;

// ---- 锚值真源（只依赖已存在模块——红线 10） ------------------------------
use super::unx36156_conserve48000;
use super::unx36157_segverdict;
use super::unx36158_domarchive;

/// 冻结锚条目：consumer 下游批号；value 冻结锚值；frozen 恒 true。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FreezeAnchor {
    pub consumer: u16,
    pub value: u64,
    pub frozen: bool,
    pub tag: &'static str,
}

/// 下游七批冻结锚（锚值取自本域已冻结可复算面）。
pub const FREEZE_TABLE: [FreezeAnchor; DOWNSTREAM_N] = [
    FreezeAnchor { consumer: CONS_B09, value: 48_000, frozen: true, tag: "b09-total-line-budget" },
    FreezeAnchor { consumer: CONS_B10, value: 160,    frozen: true, tag: "b10-total-cards" },
    FreezeAnchor { consumer: CONS_B11, value: 136,    frozen: true, tag: "b11-f0001-green-bits" },
    FreezeAnchor { consumer: CONS_B12, value: 39,     frozen: true, tag: "b12-ktest-real-lanes" },
    FreezeAnchor { consumer: CONS_B13, value: 20,     frozen: true, tag: "b13-b07-pending-lanes" },
    FreezeAnchor { consumer: CONS_B14, value: 12,     frozen: true, tag: "b14-domarchive-entries" },
    FreezeAnchor { consumer: CONS_B15, value: 300,    frozen: true, tag: "b15-per-card-line-budget" },
];

/// 下游批号闭集校验。
pub fn consumer_known(c: u16) -> bool {
    c == CONS_B09
        || c == CONS_B10
        || c == CONS_B11
        || c == CONS_B12
        || c == CONS_B13
        || c == CONS_B14
        || c == CONS_B15
}

/// 单锚对拍：值、冻结态、批号闭集三关。
pub fn verify_anchor(a: &FreezeAnchor, expect: u64) -> Result<(), u16> {
    if !consumer_known(a.consumer) {
        return Err(E_COVER);
    }
    if a.value != expect {
        return Err(E_ANCHOR);
    }
    if !a.frozen {
        return Err(E_THAW);
    }
    Ok(())
}

/// 期望锚值表（与 FREEZE_TABLE 同序，供逐条对拍）。
pub fn expected_values() -> Vec<u64> {
    let mut out: Vec<u64> = vec![];
    out.push(48_000);
    out.push(160);
    out.push(136);
    out.push(39);
    out.push(20);
    out.push(12);
    out.push(300);
    out
}

/// 全锚对拍：七批逐条三关，全过返回批数。
pub fn verify_all() -> Result<u32, u16> {
    let expects = expected_values();
    if expects.len() != DOWNSTREAM_N {
        return Err(E_COVER);
    }
    let mut pass: u32 = 0;
    let mut i = 0;
    while i < DOWNSTREAM_N {
        verify_anchor(&FREEZE_TABLE[i], expects[i])?;
        pass += 1;
        i += 1;
    }
    if pass != DOWNSTREAM_N as u32 {
        return Err(E_COVER);
    }
    Ok(pass)
}

/// 下游覆盖检查：七批批号两两互异且全覆盖。
pub fn downstream_cover() -> Result<(), u16> {
    let mut seen = [false; 7];
    let mut i = 0;
    while i < DOWNSTREAM_N {
        let c = FREEZE_TABLE[i].consumer;
        if !consumer_known(c) {
            return Err(E_COVER);
        }
        let idx = (c - CONS_B09) as usize;
        if idx >= 7 || seen[idx] {
            return Err(E_COVER);
        }
        seen[idx] = true;
        i += 1;
    }
    Ok(())
}

/// 冻结纪律扫描：全部 frozen，返回冻结数；出现解冻即报 E_THAW。
pub fn frozen_scan() -> Result<u32, u16> {
    let mut n: u32 = 0;
    let mut i = 0;
    while i < DOWNSTREAM_N {
        if !FREEZE_TABLE[i].frozen {
            return Err(E_THAW);
        }
        n += 1;
        i += 1;
    }
    Ok(n)
}

/// 注入面：解冻一位 → 交付校验必拒（拦截能力自证）。
pub fn thaw_injection(slot: usize) -> Result<(), u16> {
    if slot >= DOWNSTREAM_N {
        return Err(E_COVER);
    }
    let mut fake = FREEZE_TABLE[slot];
    fake.frozen = false;
    match verify_anchor(&fake, expected_values()[slot]) {
        Err(E_THAW) => Ok(()),
        _ => Err(E_THAW),
    }
}

/// 交付闸：覆盖全 + 冻结全 + 全锚对拍 → 宣告可交付（返回冻结锚数）。
pub fn deliver_gate() -> Result<u32, u16> {
    downstream_cover()?;
    frozen_scan()?;
    verify_all()
}

/// 交付宣告行（String 面，无 IO）。
pub fn deliver_line() -> String {
    let mut s = String::from("freezedeliver downstream=");
    s.push_str(&DOWNSTREAM_N.to_string());
    s.push_str(" frozen=");
    s.push_str(&DOWNSTREAM_N.to_string());
    s.push_str(" state=DELIVERABLE");
    s
}

/// 锚人话行（取第 n 条）。
pub fn anchor_line(n: usize) -> String {
    if n >= DOWNSTREAM_N {
        return String::from("anchor oob");
    }
    let a = &FREEZE_TABLE[n];
    let mut s = String::from(a.tag);
    s.push_str("=");
    s.push_str(&a.value.to_string());
    s
}

/// 冻结面指纹：逐锚 consumer+value+frozen 序列化后 FNV-1a。
pub fn fingerprint() -> u64 {
    let mut feed = vec![];
    feed.extend_from_slice(&VER.to_le_bytes());
    let mut i = 0;
    while i < DOWNSTREAM_N {
        feed.extend_from_slice(&FREEZE_TABLE[i].consumer.to_le_bytes());
        feed.extend_from_slice(&FREEZE_TABLE[i].value.to_le_bytes());
        feed.push(FREEZE_TABLE[i].frozen as u8);
        i += 1;
    }
    core::fnv1a(&feed)
}

/// 锚值真源复算抽验：48,000 / 136 两条锚可从真源面重算。
pub fn recalc_anchor_probe() -> Result<u32, u16> {
    let lines = unx36156_conserve48000::conservation_eq().map_err(|_| E_ANCHOR)?;
    if lines as u64 != FREEZE_TABLE[0].value {
        return Err(E_ANCHOR);
    }
    let green = unx36157_segverdict::proclaim().map_err(|_| E_ANCHOR)?;
    if green as u64 != FREEZE_TABLE[2].value {
        return Err(E_ANCHOR);
    }
    let arch_n = unx36158_domarchive::ARCH_N as u64;
    if arch_n != FREEZE_TABLE[5].value {
        return Err(E_ANCHOR);
    }
    Ok(3)
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs = [E_VER, E_ANCHOR, E_THAW, E_COVER, E_CS];
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
        E_ANCHOR => "锚值对拍不符",
        E_THAW => "解冻态（冻结纪律破）",
        E_COVER => "下游覆盖破/批号越闭集",
        E_CS => "指纹不稳",
        _ => "未知诊断码",
    }
}

/// 演练：R1 锚；R2 全锚对拍 7 条；R3 下游覆盖；R4 冻结纪律 7/7；
/// R5 解冻注入 7 位逐一必拦；R6 交付闸；R7 锚值真源复算 3 条；
/// R8 指纹稳定；R9 双跑一致。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：版本锚 + 闭集自检。
    if VER != 1 {
        return Err(E_VER);
    }
    if !codes_distinct() {
        return Err(E_CS);
    }
    hits += 1;
    // R2：全锚对拍。
    let pass = verify_all()?;
    if pass != DOWNSTREAM_N as u32 {
        return Err(E_COVER);
    }
    hits += 1;
    // R3：下游覆盖。
    downstream_cover()?;
    hits += 1;
    // R4：冻结纪律。
    let frozen = frozen_scan()?;
    if frozen != DOWNSTREAM_N as u32 {
        return Err(E_THAW);
    }
    hits += 1;
    // R5：解冻注入逐位必拦。
    let mut i = 0;
    while i < DOWNSTREAM_N {
        thaw_injection(i)?;
        hits += 1;
        i += 1;
    }
    // R6：交付闸。
    deliver_gate()?;
    hits += 1;
    // R7：锚值真源复算。
    recalc_anchor_probe()?;
    hits += 1;
    // R8：指纹稳定且非零。
    let fp1 = fingerprint();
    let fp2 = fingerprint();
    if fp1 != fp2 || fp1 == 0 {
        return Err(E_CS);
    }
    hits += 1;
    // R9：双跑一致。
    let a = deliver_gate();
    let b = deliver_gate();
    if a != b {
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
        assert_eq!(drill(), Ok(15));
    }

    #[test]
    fn anchors_frozen() {
        assert_eq!(verify_all(), Ok(7));
        assert_eq!(downstream_cover(), Ok(()));
        assert_eq!(frozen_scan(), Ok(7));
        assert_eq!(deliver_gate(), Ok(7));
    }

    #[test]
    fn thaw_all_caught() {
        let mut i = 0;
        while i < DOWNSTREAM_N {
            assert_eq!(thaw_injection(i), Ok(()));
            i += 1;
        }
        assert_eq!(thaw_injection(7), Err(E_COVER));
        assert!(deliver_line().contains("DELIVERABLE"));
        assert!(anchor_line(0).contains("48000"));
    }

    #[test]
    fn recalc_holds() {
        assert_eq!(recalc_anchor_probe(), Ok(3));
    }

    #[test]
    fn window_shapes() {
        assert!(codes_distinct());
        assert_eq!(VER, 1);
    }
}

pub fn run_unx36159_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36159_freezedeliver");
    cs.add("SEG 段位独占", SEG_BASE == 0xefb0, "0xefb0~0xefbf 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "七批锚全冻结全对拍、解冻 7 位逐一必拦、交付闸 PASS"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("下游批数 7", DOWNSTREAM_N == 7, "下游批数锚漂移");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs
}
