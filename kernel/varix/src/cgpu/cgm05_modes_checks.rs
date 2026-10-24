//! CGPU-F1925 域自检 · 显示模式管理（判据四组：API/投影/切换/防护 + 历史
//! 摘要 + 判据承载力）。
//!
//! 判据纪律：投影复用做**双源对拍**（判据侧独立重算过滤与 project 输出
//! 全等 + projection_honest 机检）；黑屏下限做**恰边界双向**（=24000 过、
//! 23999 拒）；序列状态机做**全链 + 四向违约**；码段 0x5419~0x541E 独占
//! 防自判死；零 panic 双面自扫；聚合守恒防自调。

use alloc::vec;
use crate::checks::CheckSet;
use crate::cgpu::cgm01_display::VmCode;
use crate::cgpu::cgm03_edid::{project, projection_honest, EdidCaps, Mode};
use crate::cgpu::cgm05_modes::{
    codes, F1941_PREVIEW, PROJECTION_REUSE, SAFE_REFRESH_FLOOR_MILLIHZ, SWITCH_LOG_CAP,
    TELEMETRY_REUSE,
};
use crate::cgpu::cgm05_modes::{ModeManager, SwitchState};

use alloc::string::String;
use alloc::vec::Vec;

const EXPECT_A_COUNT: usize = 11;
const EXPECT_B_BEFORE: usize = 6;

// ---------------------------------------------------------------------------
// 夹具（判据侧独立逐字段构造）
// ---------------------------------------------------------------------------

/// 基线能力：4K 面板、上限 144000 mHz。
fn caps_base() -> EdidCaps {
    EdidCaps {
        mfg_id: 0x1234u16,
        product: 0x5678u16,
        max_h: 3840u16,
        max_v: 2160u16,
        max_refresh_millihz: 144_000u32,
        rgb444: true,
        ycbcr444: false,
        ycbcr422: false,
        hdr_static: false,
        ext_count: 0u8,
        degraded: false,
    }
}

/// 放大能力：8K 面板（投影源指纹漂移夹具）。
fn caps_big() -> EdidCaps {
    let mut c = caps_base();
    c.max_h = 7680u16;
    c.max_v = 4320u16;
    c
}

fn m(w: u16, h: u16, r: u32) -> Mode {
    Mode {
        h: w,
        v: h,
        refresh_millihz: r,
    }
}

/// 候选请求表（判据侧手写）：
/// m1 1080p60 / m2 4K120 / m3 4K144 可投影（m3 刷新最高）；
/// m4 23999 mHz 可投影但低于黑屏下限；m5 8K 超基线 caps（投影滤掉）；
/// m6 24000 mHz 恰在下限（黑屏闸恰边界过）。
fn want_table() -> Vec<Mode> {
    vec![
        m(1920, 1080, 60_000),
        m(3840, 2160, 120_000),
        m(3840, 2160, 144_000),
        m(640, 480, 23_999),
        m(7680, 4320, 60_000),
        m(640, 480, 24_000),
    ]
}

// ---------------------------------------------------------------------------
// A1 · API 组
// ---------------------------------------------------------------------------

fn chk_api(set: &mut CheckSet) {
    // API-01 建管成功；投影集与判据侧独立重算过滤全等；诚实机检过。
    let mgr = match ModeManager::new(&caps_base(), 0xA1A1u64, &want_table()) {
        Ok(x) => x,
        Err(_) => {
            set.fail("E925-API-01-投影集独立重算全等", "夹具自坏：基线建管失败");
            return;
        }
    };
    let want = want_table();
    let caps = caps_base();
    let mut ref_out: Vec<Mode> = Vec::new();
    let mut i = 0usize;
    while i < want.len() {
        let mo = want[i];
        if mo.h <= caps.max_h && mo.v <= caps.max_v && mo.refresh_millihz <= caps.max_refresh_millihz {
            ref_out.push(mo);
        }
        i += 1usize;
    }
    let same_len = mgr.list().len() == ref_out.len();
    let mut same_all = same_len;
    let mut k = 0usize;
    while same_all && k < ref_out.len() {
        match mgr.list().get(k) {
            Some(a) => {
                if *a != ref_out[k] {
                    same_all = false;
                }
            }
            None => same_all = false,
        }
        k += 1usize;
    }
    let honest = projection_honest(&want, mgr.list());
    if same_all && honest {
        set.ok("E925-API-01-投影集独立重算全等");
    } else {
        set.fail("E925-API-01-投影集独立重算全等", "投影集与判据侧重算不符");
    }

    // API-02 空候选表 → MODE_TABLE_EMPTY（want 空 / caps 全拒 两向）。
    let empty: Vec<Mode> = Vec::new();
    let e1 = ModeManager::new(&caps_base(), 1u64, &empty);
    let mut tiny = caps_base();
    tiny.max_h = 1u16;
    tiny.max_v = 1u16;
    tiny.max_refresh_millihz = 1u32;
    let e2 = ModeManager::new(&tiny, 1u64, &want_table());
    let want_e = codes::MODE_TABLE_EMPTY;
    let both = match (e1, e2) {
        (Err(a), Err(b)) => a == want_e && b == want_e,
        _ => false,
    };
    if both {
        set.ok("E925-API-02-空候选表显性拒");
    } else {
        set.fail("E925-API-02-空候选表显性拒", "空表未被专属码拒");
    }

    // API-03 默认当前 = 投影集最高刷新（手算：144000 的 m3）。
    if mgr.current() == m(3840, 2160, 144_000) {
        set.ok("E925-API-03-默认当前最高刷新");
    } else {
        set.fail("E925-API-03-默认当前最高刷新", "默认当前与手算不符");
    }
}

// ---------------------------------------------------------------------------
// A2 · 投影组
// ---------------------------------------------------------------------------

fn chk_projection(set: &mut CheckSet) {
    // 投影-01 表外请求 → MODE_NOT_PROJECTED；current 不被污染、state 不变。
    let mut mgr = match ModeManager::new(&caps_base(), 0xA1A1u64, &want_table()) {
        Ok(x) => x,
        Err(_) => {
            set.fail("E925-投影-01-表外拒绝不污染", "夹具自坏：基线建管失败");
            return;
        }
    };
    let before = mgr.current();
    let r = mgr.request_switch(m(7680, 4320, 60_000));
    let rejected = match r {
        Err(c) => c == codes::MODE_NOT_PROJECTED,
        Ok(()) => false,
    };
    let clean = mgr.current() == before && mgr.state() == SwitchState::Idle;
    if rejected && clean {
        set.ok("E925-投影-01-表外拒绝不污染");
    } else {
        set.fail("E925-投影-01-表外拒绝不污染", "表外请求漏拦或污染账面");
    }

    // 投影-02 指纹漂移 → 重投影生效（8K caps 下 m5 可切）；同指纹 no-op。
    let mut mgr2 = match ModeManager::new(&caps_base(), 0xA1A1u64, &want_table()) {
        Ok(x) => x,
        Err(_) => {
            set.fail("E925-投影-02-失效重读复用缓存语义", "夹具自坏：基线建管失败");
            return;
        }
    };
    let noop = mgr2.revalidate(&caps_base(), 0xA1A1u64).is_ok();
    let mut drifted_ok = mgr2.revalidate(&caps_big(), 0xB2B2u64).is_ok();
    if drifted_ok {
        drifted_ok = mgr2.request_switch(m(7680, 4320, 60_000)).is_ok();
    }
    if noop && drifted_ok {
        set.ok("E925-投影-02-失效重读复用缓存语义");
    } else {
        set.fail("E925-投影-02-失效重读复用缓存语义", "指纹漂移未重投影");
    }
}

// ---------------------------------------------------------------------------
// A3 · 切换组
// ---------------------------------------------------------------------------

fn chk_switch(set: &mut CheckSet) {
    // 切换-01 合法全链：Idle→Validated→Committed→ack→Idle；current 更新；历史一条。
    let mut mgr = match ModeManager::new(&caps_base(), 0xA1A1u64, &want_table()) {
        Ok(x) => x,
        Err(_) => {
            set.fail("E925-切换-01-合法全链入账", "夹具自坏：基线建管失败");
            return;
        }
    };
    let to = m(1920, 1080, 60_000);
    let from = mgr.current();
    let r1 = mgr.request_switch(to);
    let st1 = mgr.state() == SwitchState::Validated;
    let r2 = mgr.commit_switch();
    let st2 = mgr.state() == SwitchState::Committed && mgr.current() == to;
    let r3 = mgr.ack_commit();
    let st3 = mgr.state() == SwitchState::Idle;
    let hist_ok = match mgr.history().first() {
        Some(rec) => rec.seq == 1u64 && rec.from == from && rec.to == to,
        None => false,
    };
    if r1.is_ok() && st1 && r2.is_ok() && st2 && r3.is_ok() && st3 && hist_ok {
        set.ok("E925-切换-01-合法全链入账");
    } else {
        set.fail("E925-切换-01-合法全链入账", "冷切换序列或历史账不符");
    }

    // 切换-02 序列违约三向 + ack 违约一向。
    let mut a = match ModeManager::new(&caps_base(), 0xA1A1u64, &want_table()) {
        Ok(x) => x,
        Err(_) => {
            set.fail("E925-切换-02-序列违约四向", "夹具自坏：基线建管失败");
            return;
        }
    };
    let _ = a.request_switch(m(1920, 1080, 60_000));
    let v1 = a.request_switch(m(3840, 2160, 120_000));
    let v1_is = match v1 {
        Err(c) => c == codes::SEQUENCE_BROKEN,
        Ok(()) => false,
    };
    let mut b = match ModeManager::new(&caps_base(), 0xA1A1u64, &want_table()) {
        Ok(x) => x,
        Err(_) => {
            set.fail("E925-切换-02-序列违约四向", "夹具自坏：基线建管失败");
            return;
        }
    };
    let v2 = b.commit_switch();
    let v2_is = match v2 {
        Err(c) => c == codes::SEQUENCE_BROKEN,
        Ok(_) => false,
    };
    let mut c3 = match ModeManager::new(&caps_base(), 0xA1A1u64, &want_table()) {
        Ok(x) => x,
        Err(_) => {
            set.fail("E925-切换-02-序列违约四向", "夹具自坏：基线建管失败");
            return;
        }
    };
    let _ = c3.request_switch(m(1920, 1080, 60_000));
    let _ = c3.commit_switch();
    let v3 = c3.request_switch(m(1920, 1080, 60_000));
    let v3_is = match v3 {
        Err(c) => c == codes::SEQUENCE_BROKEN,
        Ok(()) => false,
    };
    let mut d = match ModeManager::new(&caps_base(), 0xA1A1u64, &want_table()) {
        Ok(x) => x,
        Err(_) => {
            set.fail("E925-切换-02-序列违约四向", "夹具自坏：基线建管失败");
            return;
        }
    };
    let v4 = d.ack_commit();
    let v4_is = match v4 {
        Err(c) => c == codes::ACK_INVALID,
        Ok(()) => false,
    };
    if v1_is && v2_is && v3_is && v4_is {
        set.ok("E925-切换-02-序列违约四向");
    } else {
        set.fail("E925-切换-02-序列违约四向", "非法迁移漏拦");
    }
}

// ---------------------------------------------------------------------------
// A4 · 防护组
// ---------------------------------------------------------------------------

fn chk_guard(set: &mut CheckSet) {
    // 防护-01 黑屏下限恰边界双向：23999 拒 BLACKOUT_RISK；24000 过。
    let mut mgr = match ModeManager::new(&caps_base(), 0xA1A1u64, &want_table()) {
        Ok(x) => x,
        Err(_) => {
            set.fail("E925-防护-01-黑屏下限恰边界", "夹具自坏：基线建管失败");
            return;
        }
    };
    let low = mgr.request_switch(m(640, 480, 23_999));
    let low_is = match low {
        Err(c) => c == codes::BLACKOUT_RISK,
        Ok(()) => false,
    };
    let edge = mgr.request_switch(m(640, 480, 24_000));
    let edge_ok = edge.is_ok() && mgr.state() == SwitchState::Validated;
    let mut cleaned = false;
    if edge_ok {
        let _ = mgr.commit_switch();
        let _ = mgr.ack_commit();
        cleaned = mgr.state() == SwitchState::Idle;
    }
    if low_is && edge_ok && cleaned {
        set.ok("E925-防护-01-黑屏下限恰边界");
    } else {
        set.fail("E925-防护-01-黑屏下限恰边界", "下限漏拦或恰边界误拦");
    }

    // 防护-02 三声明逐字（投影复用/F1941 预告/遥测复用）。
    let proj = PROJECTION_REUSE.contains("cgm03_edid::project") && PROJECTION_REUSE.contains("不重建第二套");
    let hot = F1941_PREVIEW.contains("F1941")
        && F1941_PREVIEW.contains("热切换")
        && F1941_PREVIEW.contains("冷切换");
    let tele = TELEMETRY_REUSE.contains("F1930") && TELEMETRY_REUSE.contains("只读");
    if proj && hot && tele {
        set.ok("E925-防护-02-三声明逐字在案");
    } else {
        set.fail("E925-防护-02-三声明逐字在案", "复用/预告声明漂移");
    }
}

// ---------------------------------------------------------------------------
// A5 · 历史组 + 摘要
// ---------------------------------------------------------------------------

fn chk_history(set: &mut CheckSet) {
    // 历史-01 封顶滚动：CAP+3 笔后账长恰 CAP、seq 首尾与手算一致。
    let mut mgr = match ModeManager::new(&caps_base(), 0xA1A1u64, &want_table()) {
        Ok(x) => x,
        Err(_) => {
            set.fail("E925-历史-01-封顶滚动序号单调", "夹具自坏：基线建管失败");
            return;
        }
    };
    let rounds = SWITCH_LOG_CAP + 3usize;
    let mut k = 0usize;
    let mut all_ok = true;
    while k < rounds {
        if mgr.request_switch(m(1920, 1080, 60_000)).is_err() {
            all_ok = false;
            break;
        }
        if mgr.commit_switch().is_err() {
            all_ok = false;
            break;
        }
        if mgr.ack_commit().is_err() {
            all_ok = false;
            break;
        }
        k += 1usize;
    }
    let hist = mgr.history();
    let len_ok = hist.len() == SWITCH_LOG_CAP;
    let mut seq_ok = len_ok;
    if len_ok {
        let first_want = (rounds - SWITCH_LOG_CAP + 1usize) as u64;
        let last_want = rounds as u64;
        let f_ok = match hist.first() {
            Some(r) => r.seq == first_want,
            None => false,
        };
        let l_ok = match hist.last() {
            Some(r) => r.seq == last_want,
            None => false,
        };
        seq_ok = f_ok && l_ok;
    }
    if all_ok && seq_ok {
        set.ok("E925-历史-01-封顶滚动序号单调");
    } else {
        set.fail("E925-历史-01-封顶滚动序号单调", "历史账未按封顶滚动或序号漂移");
    }
}

fn chk_summary(set: &mut CheckSet) {
    // 摘要-01 render_summary 手算对账 + 截断纪律。
    let mgr = match ModeManager::new(&caps_base(), 0xA1A1u64, &want_table()) {
        Ok(x) => x,
        Err(_) => {
            set.fail("E925-摘要-01-读屏摘要手算对账", "夹具自坏：基线建管失败");
            return;
        }
    };
    let mut buf = [0u8; 48usize];
    let n = mgr.render_summary(&mut buf);
    // 期望 "M3840x2160@144000|S0|L5"（手写逐字节）。
    let want: [u8; 23usize] = [
        b'M', b'3', b'8', b'4', b'0', b'x', b'2', b'1', b'6', b'0', b'@', b'1',
        b'4', b'4', b'0', b'0', b'0', b'|', b'S', b'0', b'|', b'L', b'5',
    ];
    let mut eq = n == want.len();
    let mut i = 0usize;
    while eq && i < want.len() {
        match buf.get(i) {
            Some(bt) => {
                if *bt != want[i] {
                    eq = false;
                }
            }
            None => eq = false,
        }
        i += 1usize;
    }
    let mut small = [0u8; 5usize];
    let n2 = mgr.render_summary(&mut small);
    let trunc_ok = n2 == 5usize;
    if eq && trunc_ok {
        set.ok("E925-摘要-01-读屏摘要手算对账");
    } else {
        set.fail("E925-摘要-01-读屏摘要手算对账", "摘要字节与手写期望不符");
    }
}

// ---------------------------------------------------------------------------
// B · 判据承载力
// ---------------------------------------------------------------------------

fn all_codes() -> [VmCode; 6] {
    [
        codes::MODE_TABLE_EMPTY,
        codes::MODE_NOT_PROJECTED,
        codes::BLACKOUT_RISK,
        codes::SEQUENCE_BROKEN,
        codes::PROJECTION_STALE,
        codes::ACK_INVALID,
    ]
}

fn chk_codes(set: &mut CheckSet) {
    let cs = all_codes();
    // 判据-01 码段独占 0x5419..0x541E，不越入 cgm01~04 已占 0x5401..0x5418。
    let mut seg_ok = true;
    let mut i = 0usize;
    while i < cs.len() {
        let v = cs[i].0;
        if !(v >= 0x5419u16 && v <= 0x541Eu16) || v <= 0x5418u16 {
            seg_ok = false;
        }
        i += 1usize;
    }
    if seg_ok {
        set.ok("E925-判据-01-码段独占防自判死");
    } else {
        set.fail("E925-判据-01-码段独占防自判死", "诊断码越出 0x5419~0x541E");
    }

    // 判据-02 六码互异（独立双循环）。
    let mut distinct = true;
    let mut i = 0usize;
    while i < cs.len() {
        let mut j = i + 1usize;
        while j < cs.len() {
            if cs[i] == cs[j] {
                distinct = false;
            }
            j += 1usize;
        }
        i += 1usize;
    }
    if distinct {
        set.ok("E925-判据-02-六码互异");
    } else {
        set.fail("E925-判据-02-六码互异", "诊断码存在重复");
    }
}

fn chk_refs(set: &mut CheckSet) {
    // 判据-03 投影诚实双向：投影输出逐条不超能力（独立否证）+ 诚实机检。
    let mgr = match ModeManager::new(&caps_base(), 0xA1A1u64, &want_table()) {
        Ok(x) => x,
        Err(_) => {
            set.fail("E925-判据-03-投影诚实双向", "夹具自坏：基线建管失败");
            return;
        }
    };
    let caps = caps_base();
    let mut no_oversize = true;
    let mut i = 0usize;
    while i < mgr.list().len() {
        if let Some(mo) = mgr.list().get(i) {
            if mo.h > caps.max_h || mo.v > caps.max_v || mo.refresh_millihz > caps.max_refresh_millihz {
                no_oversize = false;
            }
        }
        i += 1usize;
    }
    let honest = projection_honest(&want_table(), mgr.list());
    let proj_same_len = project(&caps_base(), &want_table()).len() == mgr.list().len();
    if no_oversize && honest && proj_same_len {
        set.ok("E925-判据-03-投影诚实双向");
    } else {
        set.fail("E925-判据-03-投影诚实双向", "投影集含超能力模式");
    }

    // 判据-04 黑屏下限双源：判据侧独立常量 24_000 与生产面常量全等。
    let ref_floor: u32 = 24_000u32;
    if SAFE_REFRESH_FLOOR_MILLIHZ == ref_floor {
        set.ok("E925-判据-04-黑屏下限双源");
    } else {
        set.fail("E925-判据-04-黑屏下限双源", "下限常量漂移");
    }
}

fn strip_lexical_noise(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            let mut depth = 1usize;
            i += 2;
            while i < b.len() && depth > 0 {
                if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < b.len() && b[i] == b'*' && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if b[i] == b'"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        out.push(b[i] as char);
        i += 1;
    }
    out
}

fn scan_panic(src: &str) -> usize {
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    hits
}

fn chk_zero_panic(set: &mut CheckSet) {
    // 判据-05 生产面零 panic。
    let prod = include_str!("cgm05_modes.rs");
    let p_hits = scan_panic(prod);
    if p_hits == 0 {
        set.ok("E925-判据-05-生产面零 panic");
    } else {
        set.fail("E925-判据-05-生产面零 panic", "生产面含 panic 面");
    }

    // 判据-06 判据面零 panic（自扫）。
    let self_src = include_str!("cgm05_modes_checks.rs");
    let s_hits = scan_panic(self_src);
    if s_hits == 0 {
        set.ok("E925-判据-06-判据面零 panic");
    } else {
        set.fail("E925-判据-06-判据面零 panic", "判据面含 panic 面");
    }
}

fn chk_not_truncated(set: &mut CheckSet) {
    // 判据-07 聚合守恒防自调。
    let a = run_cgm05_checks_a_standalone();
    let (ap, af) = a.tally();
    let (sp, sf) = set.tally();
    let a_ok = ap + af == EXPECT_A_COUNT;
    let self_ok = sp + sf == EXPECT_B_BEFORE;
    let no_trunc = !a.truncated() && !set.truncated() && a.dropped() == 0 && set.dropped() == 0;
    if a_ok && self_ok && no_trunc {
        set.ok("E925-判据-07-聚合守恒防自调");
    } else {
        set.fail("E925-判据-07-聚合守恒防自调", "族条数漂移或有截断/丢弃");
    }
}

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

/// 判据族 a：API + 投影 + 切换 + 防护 + 历史 + 摘要。
pub fn run_cgm05_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("cgpu/cgm05/a");
    chk_api(&mut s);
    chk_projection(&mut s);
    chk_switch(&mut s);
    chk_guard(&mut s);
    chk_history(&mut s);
    chk_summary(&mut s);
    s
}

/// 判据族 b：判据承载力。
pub fn run_cgm05_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("cgpu/cgm05/b");
    chk_codes(&mut s);
    chk_refs(&mut s);
    chk_zero_panic(&mut s);
    chk_not_truncated(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_cgm05_checks() -> CheckSet {
    CheckSet::merge(run_cgm05_checks_a_standalone(), run_cgm05_checks_b_standalone())
}
