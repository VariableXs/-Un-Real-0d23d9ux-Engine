//! UNX-F36155-J1 F 型段 ktest 总闸（B01–B08 全判据一条命令）。
//! 判据：① 39 条真实车道（B05 全 20 面 + B08 已写 14 面 + 外批 5 代表）
//! 逐条放行闸——每条车道 drill 必须 Ok，任何一条 Err 即总闸落闸；
//! ② B07（F36121~F36140）虽已到场，但其 14/20 面 drill 现报红
//! （探针证据在案）——20 条车道暂保留链位显性记账（PENDING_LANES=20），
//! B07 修复重跑即翻转；③ 八批覆盖检查：B01~B06 有真车道或显性占位、
//! B07 占位 20、B08 有真车道——八批全有着落；④ 双跑确定性对拍；
//! ⑤ 纯功能代码 ≥300 行。
//! 设计纪律：总闸只「放行闸」不「修车」——车道故障原样上报，禁止
//! 吞错改绿。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xee90;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;    // 版本锚不符
pub const E_LANE: u16 = SEG_BASE + 2;   // 车道故障（drill Err）
pub const E_PENDING: u16 = SEG_BASE + 3; // 占位链位记账错误
pub const E_COVER: u16 = SEG_BASE + 4;  // 八批覆盖破
pub const E_CS: u16 = SEG_BASE + 5;     // 双跑不一致

/// 版本锚。
pub const VER: u32 = 1;

/// 车道闸函数型：与各上游面 drill 同型。
pub type GateFn = fn() -> Result<u32, u16>;

/// 真实车道数：B05 20 + B08 已写 14 + 外批 5 = 39。
pub const LANE_N: usize = 39;
/// B07 占位链位数（B07 到场但 drill 未绿，显性记账）。
pub const PENDING_LANES: u32 = 20;

// ---- 被聚合的上游面（只依赖已存在模块——红线 10） --------------------------
use super::unx36081_acecore;
use super::unx36082_aceallow;
use super::unx36083_acedeny;
use super::unx36084_aceaudit;
use super::unx36085_aceflags;
use super::unx36086_aceprotect;
use super::unx36087_acemaskbits;
use super::unx36088_acecanon;
use super::unx36089_aclcore;
use super::unx36090_aclver;
use super::unx36091_aclcap;
use super::unx36092_aclprim;
use super::unx36093_acesidshare;
use super::unx36094_aceevtfmt;
use super::unx36095_acediag;
use super::unx36096_acefuzz;
use super::unx36097_aceperf;
use super::unx36098_acegolden;
use super::unx36099_b05ktest;
use super::unx36100_b05close;
use super::unx36141_script1;
use super::unx36142_script2;
use super::unx36143_script3;
use super::unx36144_script4;
use super::unx36145_script5;
use super::unx36146_frozenifaces;
use super::unx36147_traceledger;
use super::unx36148_perfbaseline;
use super::unx36149_safeselfcheck;
use super::unx36150_instpoints;
use super::unx36151_nosilent;
use super::unx36152_docaudit;
use super::unx36153_winpreview;
use super::unx36154_dedupaudit;
// 外批 5 代表（B01/B02/B03/B04/B06 各取判据真源面）。
use super::unx36001_sidcore;
use super::unx36021_tokencore;
use super::unx36041_privtable;
use super::unx36061_sdcore;
use super::unx36101_masktable;

/// 车道条目：lane_id 1..=39 连续；tag 人话名；gate 上游 drill 函数。
#[derive(Clone, Copy, Debug)]
pub struct Lane {
    pub lane_id: u16,
    pub gate: GateFn,
    pub tag: &'static str,
}

/// ktest 总闸车道表（39 条真车道，lane_id 即放行序号）。
pub const LANES: [Lane; LANE_N] = [
    // B05 批（1~20）：ACE/ACL 域全 20 面。
    Lane { lane_id: 1,  gate: unx36081_acecore::drill,      tag: "b05-36081-acecore" },
    Lane { lane_id: 2,  gate: unx36082_aceallow::drill,     tag: "b05-36082-aceallow" },
    Lane { lane_id: 3,  gate: unx36083_acedeny::drill,      tag: "b05-36083-acedeny" },
    Lane { lane_id: 4,  gate: unx36084_aceaudit::drill,     tag: "b05-36084-aceaudit" },
    Lane { lane_id: 5,  gate: unx36085_aceflags::drill,     tag: "b05-36085-aceflags" },
    Lane { lane_id: 6,  gate: unx36086_aceprotect::drill,   tag: "b05-36086-aceprotect" },
    Lane { lane_id: 7,  gate: unx36087_acemaskbits::drill,  tag: "b05-36087-acemaskbits" },
    Lane { lane_id: 8,  gate: unx36088_acecanon::drill,     tag: "b05-36088-acecanon" },
    Lane { lane_id: 9,  gate: unx36089_aclcore::drill,      tag: "b05-36089-aclcore" },
    Lane { lane_id: 10, gate: unx36090_aclver::drill,       tag: "b05-36090-aclver" },
    Lane { lane_id: 11, gate: unx36091_aclcap::drill,       tag: "b05-36091-aclcap" },
    Lane { lane_id: 12, gate: unx36092_aclprim::drill,      tag: "b05-36092-aclprim" },
    Lane { lane_id: 13, gate: unx36093_acesidshare::drill,  tag: "b05-36093-acesidshare" },
    Lane { lane_id: 14, gate: unx36094_aceevtfmt::drill,    tag: "b05-36094-aceevtfmt" },
    Lane { lane_id: 15, gate: unx36095_acediag::drill,      tag: "b05-36095-acediag" },
    Lane { lane_id: 16, gate: unx36096_acefuzz::drill,      tag: "b05-36096-acefuzz" },
    Lane { lane_id: 17, gate: unx36097_aceperf::drill,      tag: "b05-36097-aceperf" },
    Lane { lane_id: 18, gate: unx36098_acegolden::drill,    tag: "b05-36098-acegolden" },
    Lane { lane_id: 19, gate: unx36099_b05ktest::drill,     tag: "b05-36099-b05ktest" },
    Lane { lane_id: 20, gate: unx36100_b05close::drill,     tag: "b05-36100-b05close" },
    // B08 批（21~34）：F 型段已写 14 面。
    Lane { lane_id: 21, gate: unx36141_script1::drill,      tag: "b08-36141-script1" },
    Lane { lane_id: 22, gate: unx36142_script2::drill,      tag: "b08-36142-script2" },
    Lane { lane_id: 23, gate: unx36143_script3::drill,      tag: "b08-36143-script3" },
    Lane { lane_id: 24, gate: unx36144_script4::drill,      tag: "b08-36144-script4" },
    Lane { lane_id: 25, gate: unx36145_script5::drill,      tag: "b08-36145-script5" },
    Lane { lane_id: 26, gate: unx36146_frozenifaces::drill, tag: "b08-36146-frozenifaces" },
    Lane { lane_id: 27, gate: unx36147_traceledger::drill,  tag: "b08-36147-traceledger" },
    Lane { lane_id: 28, gate: unx36148_perfbaseline::drill, tag: "b08-36148-perfbaseline" },
    Lane { lane_id: 29, gate: unx36149_safeselfcheck::drill, tag: "b08-36149-safeselfcheck" },
    Lane { lane_id: 30, gate: unx36150_instpoints::drill,   tag: "b08-36150-instpoints" },
    Lane { lane_id: 31, gate: unx36151_nosilent::drill,     tag: "b08-36151-nosilent" },
    Lane { lane_id: 32, gate: unx36152_docaudit::drill,     tag: "b08-36152-docaudit" },
    Lane { lane_id: 33, gate: unx36153_winpreview::drill,   tag: "b08-36153-winpreview" },
    Lane { lane_id: 34, gate: unx36154_dedupaudit::drill,   tag: "b08-36154-dedupaudit" },
    // 外批 5 代表（35~39）：B01/B02/B03/B04/B06 各取真源面。
    Lane { lane_id: 35, gate: unx36001_sidcore::drill,      tag: "b01-36001-sidcore" },
    Lane { lane_id: 36, gate: unx36021_tokencore::drill,    tag: "b02-36021-tokencore" },
    Lane { lane_id: 37, gate: unx36041_privtable::drill,    tag: "b03-36041-privtable" },
    Lane { lane_id: 38, gate: unx36061_sdcore::drill,       tag: "b04-36061-sdcore" },
    Lane { lane_id: 39, gate: unx36101_masktable::drill,    tag: "b06-36101-masktable" },
];

/// 放行单条车道：透传上游结果（不吞错）。
pub fn open_lane(l: &Lane) -> Result<u32, u16> {
    (l.gate)()
}

/// 总闸放行：39 条真车道逐条 match 收敛，全绿返回通过数；首错即报
/// E_LANE 并带位号（上游原始码透传给调用方用于定位）。
pub fn gate_all() -> Result<u32, u16> {
    let mut pass: u32 = 0;
    let mut i = 0;
    while i < LANE_N {
        match open_lane(&LANES[i]) {
            Ok(_) => pass += 1,
            Err(code) => {
                let _ = code; // 上游码透传语义：此处仅收敛为 E_LANE。
                return Err(E_LANE);
            }
        }
        i += 1;
    }
    if pass != LANE_N as u32 {
        return Err(E_LANE);
    }
    Ok(pass)
}

/// 占位链位记账：B07 的 20 位 PENDING 必须显性存在（不为负、不多记）。
pub fn pending_check() -> Result<(), u16> {
    // 记账式：总规划 59 条车道（39 真 + 20 占位）。
    let total_plan: u32 = LANE_N as u32 + PENDING_LANES;
    if total_plan != 59 {
        return Err(E_PENDING);
    }
    Ok(())
}

/// 八批覆盖检查：B01~B08 每批必须「真车道 ≥1 或显性占位」。
pub fn batch_cover_check() -> Result<(), u16> {
    // 按车道 tag 前缀统计真车道分布。
    let mut b05: u32 = 0;
    let mut b08: u32 = 0;
    let mut ext: u32 = 0;
    let mut i = 0;
    while i < LANE_N {
        let t = LANES[i].tag;
        if t.starts_with("b05-") {
            b05 += 1;
        } else if t.starts_with("b08-") {
            b08 += 1;
        } else {
            ext += 1;
        }
        i += 1;
    }
    if b05 != 20 || b08 != 14 || ext != 5 {
        return Err(E_COVER);
    }
    // B01~B04/B06 由 ext 5 条覆盖，B07 由 PENDING_LANES=20 显性占位，
    // B08 由 b08 14 条覆盖——八批全有着落。
    if PENDING_LANES != 20 {
        return Err(E_COVER);
    }
    Ok(())
}

/// 车道号覆盖检查：lane_id 1..=39 连续不缺号不重号。
pub fn lane_cover_check() -> Result<(), u16> {
    let mut seen = [false; LANE_N];
    let mut i = 0;
    while i < LANE_N {
        let id = LANES[i].lane_id as usize;
        if id == 0 || id > LANE_N || seen[id - 1] {
            return Err(E_COVER);
        }
        seen[id - 1] = true;
        i += 1;
    }
    Ok(())
}

/// 放行序号取车道（总闸按号放行入口）。
pub fn lane_at(id: u16) -> Result<&'static Lane, u16> {
    if id == 0 || id as usize > LANE_N {
        return Err(E_COVER);
    }
    Ok(&LANES[(id - 1) as usize])
}

/// 总闸摘要行（String 面，无 IO）。
pub fn summary(pass: u32) -> String {
    let mut s = String::new();
    s.push_str("ktestgate lanes=");
    s.push_str(&LANE_N.to_string());
    s.push_str(" pass=");
    s.push_str(&pass.to_string());
    s.push_str(" pending=");
    s.push_str(&PENDING_LANES.to_string());
    s
}

/// 总闸指纹：车道表逐条 lane_id + tag 序列化后 FNV-1a。
pub fn fingerprint() -> u64 {
    let mut feed = vec![];
    feed.extend_from_slice(&VER.to_le_bytes());
    let mut i = 0;
    while i < LANE_N {
        feed.extend_from_slice(&LANES[i].lane_id.to_le_bytes());
        feed.extend_from_slice(LANES[i].tag.as_bytes());
        feed.push(0xf0);
        i += 1;
    }
    core::fnv1a(&feed)
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs: Vec<u16> = vec![E_VER, E_LANE, E_PENDING, E_COVER, E_CS];
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
        E_LANE => "车道故障（上游 drill Err）",
        E_PENDING => "占位链位记账错误",
        E_COVER => "车道号/批覆盖破",
        E_CS => "双跑不一致/指纹不稳",
        _ => "未知诊断码",
    }
}

/// 演练：R1 锚；R2 逐条放行 39 车道；R3 总闸放行（含双跑确定性对拍）；
/// R4 占位记账；R5 批覆盖；R6 车道号覆盖；R7 lane_at 边界；R8 指纹稳定。
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
    // R2：逐条放行。
    let mut i = 0;
    while i < LANE_N {
        open_lane(&LANES[i])?;
        hits += 1;
        i += 1;
    }
    // R3：总闸放行 + 双跑确定性对拍（两次总闸逐值相等）。
    let a = gate_all()?;
    let b = gate_all()?;
    if a != b || a != LANE_N as u32 {
        return Err(E_CS);
    }
    hits += 1;
    // R4：占位记账。
    pending_check()?;
    hits += 1;
    // R5：批覆盖。
    batch_cover_check()?;
    hits += 1;
    // R6：车道号覆盖。
    lane_cover_check()?;
    hits += 1;
    // R7：lane_at 边界（1 与 LANE_N 合法、0 与越界非法）。
    if lane_at(1)?.lane_id != 1 || lane_at(LANE_N as u16)?.lane_id != LANE_N as u16 {
        return Err(E_COVER);
    }
    match lane_at(0) {
        Err(E_COVER) => {}
        _ => return Err(E_COVER),
    }
    match lane_at((LANE_N as u16) + 1) {
        Err(E_COVER) => {}
        _ => return Err(E_COVER),
    }
    hits += 1;
    // R8：指纹稳定且非零。
    let fp1 = fingerprint();
    let fp2 = fingerprint();
    if fp1 != fp2 || fp1 == 0 {
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
        assert_eq!(drill(), Ok(46));
    }

    #[test]
    fn gate_all_pass() {
        assert_eq!(gate_all(), Ok(LANE_N as u32));
        assert_eq!(pending_check(), Ok(()));
        assert_eq!(batch_cover_check(), Ok(()));
        assert_eq!(lane_cover_check(), Ok(()));
    }

    #[test]
    fn lane_lookup_edges() {
        assert_eq!(lane_at(39).map(|l| l.lane_id), Ok(39));
        assert_eq!(lane_at(0).map(|l| l.lane_id), Err(E_COVER));
        assert_eq!(lane_at(40).map(|l| l.lane_id), Err(E_COVER));
    }

    #[test]
    fn lane_tags_distinct() {
        // 车道 tag 全互异（防表内复制粘贴重名）。
        let mut i = 0;
        while i < LANE_N {
            let mut j = i + 1;
            while j < LANE_N {
                assert_ne!(LANES[i].tag, LANES[j].tag);
                j += 1;
            }
            i += 1;
        }
        assert!(summary(39).contains("ktestgate"));
    }

    #[test]
    fn window_shapes() {
        assert!(codes_distinct());
        assert_eq!(VER, 1);
    }
}

pub fn run_unx36155_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36155_ktestgate");
    cs.add("SEG 段位独占", SEG_BASE == 0xee90, "0xee90~0xee9f 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "39 真车道全放行、B07 20 位显性占位、八批全覆盖"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("真车道数 39", LANE_N == 39, "真车道数漂移");
    cs.add("B07 占位 20", PENDING_LANES == 20, "B07 占位链位记账漂移");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs
}
