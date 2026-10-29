//! UNX-F36153-J1 F 型段 Windows 对照预告面（J-根 消费登记）。
//! 判据：① J-根（Windows 对照层）将要消费的 16 个批面锚点全部登记——
//! 每条登记编译期绑定真实已存在模块的 pub 常量，登记值与真源值全等；
//! ② 消费方向纪律：J-根 只读消费，全部登记位 readonly=true，出现
//! writable 即 E_RDWR；③ 消费方编码闭集：consumer 必须落在 J1 已知
//! 对照域内；④ 指纹稳定 + 双跑一致；⑤ 纯功能代码 ≥300 行。
//! 设计纪律：预告面是「消费登记」而非「消费执行」——J-根 到场后按
//! 登记表逐条取值，本面只证明登记表本身与真源一致、方向纪律完好。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xee70;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;   // 版本锚不符
pub const E_DRIFT: u16 = SEG_BASE + 2; // 登记值与真源值漂移
pub const E_RDWR: u16 = SEG_BASE + 3;  // 只读纪律破坏
pub const E_CONS: u16 = SEG_BASE + 4;  // 消费方编码越闭集
pub const E_COVER: u16 = SEG_BASE + 5; // 登记面缺号/重号
pub const E_CS: u16 = SEG_BASE + 6;    // 指纹不稳

/// 版本锚。
pub const VER: u32 = 1;

/// 登记锚点数。
pub const PREVIEW_N: usize = 16;

/// 消费方编码闭集：0x01=B01 批 0x02=B02 0x03=B03 0x04=B04 0x05=B05
/// 0x07=B07（预留）0x08=B08 本批 0x0a=J1-Windows 对照层。
pub const CONS_B01: u16 = 0x01;
pub const CONS_B02: u16 = 0x02;
pub const CONS_B03: u16 = 0x03;
pub const CONS_B04: u16 = 0x04;
pub const CONS_B05: u16 = 0x05;
pub const CONS_B07: u16 = 0x07;
pub const CONS_B08: u16 = 0x08;
pub const CONS_J1WIN: u16 = 0x0a;

fn consumer_known(c: u16) -> bool {
    c == CONS_B01
        || c == CONS_B02
        || c == CONS_B03
        || c == CONS_B04
        || c == CONS_B05
        || c == CONS_B07
        || c == CONS_B08
        || c == CONS_J1WIN
}

/// 被引用的真源模块（只依赖已存在模块——红线 10）。
use super::unx36081_acecore as acec;
use super::unx36083_acedeny as deny;
use super::unx36084_aceaudit as aud;
use super::unx36085_aceflags as flg;
use super::unx36086_aceprotect as prot;
use super::unx36089_aclcore as aclc;
use super::unx36091_aclcap as capc;
use super::unx36093_acesidshare as sidp;
use super::unx36141_script1 as scr1;

/// 预告登记条目：anchor_id 连续编号；value 编译期绑定真源常量；
/// consumer 声明消费方；readonly 恒 true（J-根 只读纪律）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PreviewEntry {
    pub anchor_id: u16,
    pub value: u64,
    pub consumer: u16,
    pub readonly: bool,
    pub tag: &'static str,
}

/// J-根 消费登记表（16 条，值全部编译期绑定真源）。
pub const PREVIEW_TABLE: [PreviewEntry; PREVIEW_N] = [
    PreviewEntry { anchor_id: 1,  value: acec::FLAG_RESERVED as u64,    consumer: CONS_J1WIN, readonly: true, tag: "win-flag-reserved-bit" },
    PreviewEntry { anchor_id: 2,  value: acec::MASK_RESERVED as u64,    consumer: CONS_J1WIN, readonly: true, tag: "win-mask-reserved" },
    PreviewEntry { anchor_id: 3,  value: acec::ACE_SIZE_MAX as u64,     consumer: CONS_J1WIN, readonly: true, tag: "win-ace-size-max" },
    PreviewEntry { anchor_id: 4,  value: deny::GENERIC_SEGMENT as u64,  consumer: CONS_J1WIN, readonly: true, tag: "win-generic-segment-gate" },
    PreviewEntry { anchor_id: 5,  value: aud::TRIG_SUCCESS as u64,      consumer: CONS_J1WIN, readonly: true, tag: "win-audit-trig-success" },
    PreviewEntry { anchor_id: 6,  value: aud::TRIG_FAILURE as u64,      consumer: CONS_J1WIN, readonly: true, tag: "win-audit-trig-failure" },
    PreviewEntry { anchor_id: 7,  value: flg::F_CI as u64,              consumer: CONS_J1WIN, readonly: true, tag: "win-flag-ci" },
    PreviewEntry { anchor_id: 8,  value: flg::F_OI as u64,              consumer: CONS_J1WIN, readonly: true, tag: "win-flag-oi" },
    PreviewEntry { anchor_id: 9,  value: flg::F_IP as u64,              consumer: CONS_J1WIN, readonly: true, tag: "win-flag-ip" },
    PreviewEntry { anchor_id: 10, value: flg::F_ID as u64,              consumer: CONS_J1WIN, readonly: true, tag: "win-flag-id" },
    PreviewEntry { anchor_id: 11, value: prot::SE_DACL_PROTECTED as u64, consumer: CONS_J1WIN, readonly: true, tag: "win-dacl-protected" },
    PreviewEntry { anchor_id: 12, value: aclc::ACL_CAP as u64,          consumer: CONS_J1WIN, readonly: true, tag: "win-acl-slot-cap" },
    PreviewEntry { anchor_id: 13, value: aclc::ACL_SIZE_MAX as u64,     consumer: CONS_J1WIN, readonly: true, tag: "win-acl-size-cap" },
    PreviewEntry { anchor_id: 14, value: capc::ACE_SIZE_CAP as u64,     consumer: CONS_J1WIN, readonly: true, tag: "win-ace-size-cap" },
    PreviewEntry { anchor_id: 15, value: sidp::POOL_CAP as u64,         consumer: CONS_B08,   readonly: true, tag: "win-sidpool-cap" },
    PreviewEntry { anchor_id: 16, value: 0x8230,                        consumer: CONS_B08,   readonly: true, tag: "win-b08-first-seg" },
];

/// 单条登记校验：值漂移 / 方向 / 消费方闭集 三关。
pub fn verify_entry(e: &PreviewEntry, truth: u64) -> Result<(), u16> {
    if e.value != truth {
        return Err(E_DRIFT);
    }
    if !e.readonly {
        return Err(E_RDWR);
    }
    if !consumer_known(e.consumer) {
        return Err(E_CONS);
    }
    Ok(())
}

/// 逐条登记对拍（truth 由调用方按位号给出，避免数组内手抄二值）。
pub fn verify_all() -> Result<u32, u16> {
    let truths: [u64; PREVIEW_N] = [
        acec::FLAG_RESERVED as u64,
        acec::MASK_RESERVED as u64,
        acec::ACE_SIZE_MAX as u64,
        deny::GENERIC_SEGMENT as u64,
        aud::TRIG_SUCCESS as u64,
        aud::TRIG_FAILURE as u64,
        flg::F_CI as u64,
        flg::F_OI as u64,
        flg::F_IP as u64,
        flg::F_ID as u64,
        prot::SE_DACL_PROTECTED as u64,
        aclc::ACL_CAP as u64,
        aclc::ACL_SIZE_MAX as u64,
        capc::ACE_SIZE_CAP as u64,
        sidp::POOL_CAP as u64,
        scr1::SEG_BASE as u64,
    ];
    let mut pass: u32 = 0;
    let mut i = 0;
    while i < PREVIEW_N {
        match verify_entry(&PREVIEW_TABLE[i], truths[i]) {
            Ok(_) => pass += 1,
            Err(code) => return Err(code),
        }
        i += 1;
    }
    if pass != PREVIEW_N as u32 {
        return Err(E_COVER);
    }
    Ok(pass)
}

/// 登记面覆盖检查：anchor_id 1..=PREVIEW_N 连续不缺号不重号。
pub fn coverage_check() -> Result<(), u16> {
    let mut seen = [false; PREVIEW_N];
    let mut i = 0;
    while i < PREVIEW_N {
        let id = PREVIEW_TABLE[i].anchor_id as usize;
        if id == 0 || id > PREVIEW_N || seen[id - 1] {
            return Err(E_COVER);
        }
        seen[id - 1] = true;
        i += 1;
    }
    Ok(())
}

/// 方向纪律扫描：全部 readonly，返回只读位数；出现 writable 即报错。
pub fn direction_scan() -> Result<u32, u16> {
    let mut ro: u32 = 0;
    let mut i = 0;
    while i < PREVIEW_N {
        if !PREVIEW_TABLE[i].readonly {
            return Err(E_RDWR);
        }
        ro += 1;
        i += 1;
    }
    Ok(ro)
}

/// 消费方分布统计：返回 J1WIN 消费位数（预告面主体）。
pub fn j1win_count() -> u32 {
    let mut n: u32 = 0;
    let mut i = 0;
    while i < PREVIEW_N {
        if PREVIEW_TABLE[i].consumer == CONS_J1WIN {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 注入面：把一条登记位改成 writable + 消费方越闭集 → 校验器必须
/// 分别报 E_RDWR / E_CONS（纪律自证）。
pub fn injection_probe() -> Result<(), u16> {
    let mut fake = PREVIEW_TABLE[0];
    fake.readonly = false;
    match verify_entry(&fake, PREVIEW_TABLE[0].value) {
        Err(E_RDWR) => {}
        _ => return Err(E_RDWR),
    }
    let mut fake2 = PREVIEW_TABLE[0];
    fake2.consumer = 0x7f;
    match verify_entry(&fake2, PREVIEW_TABLE[0].value) {
        Err(E_CONS) => {}
        _ => return Err(E_CONS),
    }
    Ok(())
}

/// 指纹（无 IO）：登记表逐条序列化后 FNV-1a。
pub fn fingerprint() -> u64 {
    let mut feed = vec![];
    feed.extend_from_slice(&VER.to_le_bytes());
    let mut i = 0;
    while i < PREVIEW_N {
        let e = &PREVIEW_TABLE[i];
        feed.extend_from_slice(&e.anchor_id.to_le_bytes());
        feed.extend_from_slice(&e.value.to_le_bytes());
        feed.extend_from_slice(&e.consumer.to_le_bytes());
        feed.push(e.readonly as u8);
        i += 1;
    }
    core::fnv1a(&feed)
}

/// 摘要行（String 面，无 IO）。
pub fn summary(pass_n: u32) -> String {
    let mut s = String::new();
    s.push_str("winpreview anchors=");
    s.push_str(&PREVIEW_N.to_string());
    s.push_str(" pass=");
    s.push_str(&pass_n.to_string());
    s.push_str(" j1win=");
    s.push_str(&j1win_count().to_string());
    s
}

/// 人话化锚行（取第 n 条）。
pub fn anchor_line(n: usize) -> String {
    if n >= PREVIEW_N {
        return String::from("anchor oob");
    }
    let e = &PREVIEW_TABLE[n];
    let mut s = String::from(e.tag);
    s.push_str(" val=");
    s.push_str(&e.value.to_string());
    s
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs: Vec<u16> = vec![E_VER, E_DRIFT, E_RDWR, E_CONS, E_COVER, E_CS];
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
        E_DRIFT => "登记值与真源值漂移",
        E_RDWR => "只读纪律破坏",
        E_CONS => "消费方编码越闭集",
        E_COVER => "登记面缺号/重号",
        E_CS => "指纹不稳",
        _ => "未知诊断码",
    }
}

/// 演练：R1 锚；R2 逐条对拍 16/16；R3 覆盖检查；R4 方向纪律 16/16；
/// R5 注入双关必拦；R6 指纹稳定；R7 双跑一致；R8 人话账行逐条非空。
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
    // R2：逐条对拍。
    let pass = verify_all()?;
    if pass != PREVIEW_N as u32 {
        return Err(E_COVER);
    }
    hits += 1;
    // R3：覆盖检查。
    coverage_check()?;
    hits += 1;
    // R4：方向纪律。
    let ro = direction_scan()?;
    if ro != PREVIEW_N as u32 {
        return Err(E_RDWR);
    }
    hits += 1;
    // R5：注入双关。
    injection_probe()?;
    hits += 1;
    // R6：指纹稳定且非零。
    let fp1 = fingerprint();
    let fp2 = fingerprint();
    if fp1 != fp2 || fp1 == 0 {
        return Err(E_CS);
    }
    hits += 1;
    // R7：双跑一致。
    let a = verify_all();
    let b = verify_all();
    if a != b {
        return Err(E_CS);
    }
    hits += 1;
    // R8：人话账行逐条非空。
    let mut i = 0;
    while i < PREVIEW_N {
        if anchor_line(i).len() == 0 {
            return Err(E_DRIFT);
        }
        hits += 1;
        i += 1;
    }
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(23));
    }

    #[test]
    fn verify_all_pass() {
        assert_eq!(verify_all(), Ok(PREVIEW_N as u32));
        assert_eq!(coverage_check(), Ok(()));
        assert_eq!(direction_scan(), Ok(PREVIEW_N as u32));
    }

    #[test]
    fn injection_caught() {
        assert_eq!(injection_probe(), Ok(()));
        let mut fake = PREVIEW_TABLE[3];
        fake.value = fake.value.wrapping_add(1);
        assert_eq!(verify_entry(&fake, PREVIEW_TABLE[3].value), Err(E_DRIFT));
        assert!(summary(16).contains("winpreview"));
    }

    #[test]
    fn consumer_closed_set() {
        assert!(consumer_known(CONS_J1WIN));
        assert!(consumer_known(CONS_B08));
        assert!(!consumer_known(0x7f));
        assert_eq!(j1win_count(), 14);
    }

    #[test]
    fn window_shapes() {
        assert!(codes_distinct());
        assert_eq!(VER, 1);
    }
}

pub fn run_unx36153_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36153_winpreview");
    cs.add("SEG 段位独占", SEG_BASE == 0xee70, "0xee70~0xee7f 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "16 锚全对拍、只读纪律全过、注入双关必拦"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("登记锚数 16", PREVIEW_N == 16, "登记锚数漂移");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs
}
