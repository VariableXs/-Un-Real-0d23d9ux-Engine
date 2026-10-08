//! CGPU-F1923 判据层：显示能力查询 EDID（锚点判据逐条映射：解析/容错/缓存/投影/覆盖）
//!
//! 判据侧独立构造 EDID 夹具（逐字节含手算校验和）+ 手算 60000mHz 刷新对账
//! ——同源恒绿的弱门禁比没有门禁更坏。聚合防自调：A/B 两族 + tally 守恒。

use crate::checks::CheckSet;

use super::cgm03_edid as api;
use super::cgm03_edid::*;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// A 族条数（判据侧写死）。
const EXPECT_A_COUNT: usize = 11;
/// B 族防自调 tally 时刻已登记条数（判据侧写死；B 全族 6 条）。
const EXPECT_B_BEFORE: usize = 5;

// ---------------------------------------------------------------------------
// EDID 夹具（判据侧独立构造——逐字节手算）
// ---------------------------------------------------------------------------

/// 128 字节块校验和补齐（和 ≡ 0 mod 256）。
fn with_checksum(mut v: Vec<u8>) -> Vec<u8> {
    let n = v.len();
    let mut sum = 0u32;
    let mut i = n - 128;
    while i < n - 1 {
        sum += v[i] as u32;
        i += 1;
    }
    v[n - 1] = ((256 - sum % 256) % 256) as u8;
    v
}

/// 1080p60 基块（无扩展）：DTD@54 pxclk=14850(10kHz) → 刷新手算 60000mHz。
fn valid_edid_1080p60() -> Vec<u8> {
    let mut v = vec![0u8; 128];
    v[0] = 0x00;
    let mut i = 1usize;
    while i < 7 {
        v[i] = 0xFF;
        i += 1;
    }
    v[7] = 0x00;
    v[8] = 0x04; // mfg 0x0469 hi
    v[9] = 0x69; // mfg lo
    v[10] = 0x34; // product 0x1234 lo
    v[11] = 0x12; // product hi
    // DTD@54：pxclk=14850(=0x3A02)；hact 1920/blank 280；vact 1080/blank 45。
    v[54] = 0x02; // pxclk lo
    v[55] = 0x3A; // pxclk hi
    v[56] = 0x80; // hact lo
    v[57] = 0x18; // hblank lo
    v[58] = 0x71; // hact hi=7 / hblank hi=1
    v[59] = 0x38; // vact lo
    v[60] = 0x2D; // vblank lo
    v[61] = 0x40; // vact hi=4 / vblank hi=0
    v[126] = 0; // 无扩展
    with_checksum(v)
}

/// 1080p60 + CTA 扩展（Y444|Y422 flags=0x30，HDR 静态元数据块 0xE6）。
fn valid_edid_hdr() -> Vec<u8> {
    let mut v = valid_edid_1080p60();
    v[126] = 1;
    // ext_count 入账后基块校验和必须重算（126 字节在求和范围内）。
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < 127 {
        sum += v[i] as u32;
        i += 1;
    }
    v[127] = ((256 - sum % 256) % 256) as u8;
    let mut ext = vec![0u8; 128];
    ext[0] = 0x02; // CTA tag
    ext[1] = 0x00; // rev
    ext[2] = 0x08; // DTB 区到 byte8
    ext[3] = 0x30; // Y444(0x20) | Y422(0x10)
    ext[4] = 0xE3; // 扩展块 tag7 len3
    ext[5] = 0xE6; // HDR 静态元数据
    // ext 校验和
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < 127 {
        sum += ext[i] as u32;
        i += 1;
    }
    ext[127] = ((256 - sum % 256) % 256) as u8;
    let mut j = 0usize;
    while j < 128 {
        v.push(ext[j]);
        j += 1;
    }
    v
}

/// 说谎面板夹具：EDID 原生时序只有 30Hz（pxclk=7425），mfg/product 在白名单。
fn lying_edid_1080p30() -> Vec<u8> {
    let mut v = valid_edid_1080p60();
    // pxclk 7425 = 0x1D01
    v[54] = 0x01;
    v[55] = 0x1D;
    with_checksum(v)
}

// ---------------------------------------------------------------------------
// A 族：解析 + 容错 + 缓存 + 投影 + 覆盖 + 全链
// ---------------------------------------------------------------------------

fn chk_parse(set: &mut CheckSet) {
    // 解析-01 基块字段手算对账：mfg/product/1920x1080/60000mHz。
    let caps = match parse_edid(&valid_edid_1080p60()) {
        Ok(c) => c,
        Err(_) => {
            set.fail("E923-解析-01-基块手算对账", "合法 EDID 被拒");
            return;
        }
    };
    let ok = caps.mfg_id == 0x0469
        && caps.product == 0x1234
        && caps.max_h == 1920
        && caps.max_v == 1080
        // 手算：14850×10^7/(2200×1125) = 60000 mHz
        && caps.max_refresh_millihz == 60_000
        && caps.rgb444
        && !caps.hdr_static
        && !caps.degraded;
    if ok {
        set.ok("E923-解析-01-基块手算对账");
    } else {
        set.fail("E923-解析-01-基块手算对账", "字段值或 60000mHz 手算漂移");
    }
    // 解析-02 CTA 扩展：Y444/Y422 位与 HDR 0xE6 块扫描。
    let caps = match parse_edid(&valid_edid_hdr()) {
        Ok(c) => c,
        Err(_) => {
            set.fail("E923-解析-02-CTA 扩展", "带扩展 EDID 被拒");
            return;
        }
    };
    if caps.ycbcr444 && caps.ycbcr422 && caps.hdr_static && caps.ext_count == 1 && !caps.degraded {
        set.ok("E923-解析-02-CTA 扩展");
    } else {
        set.fail("E923-解析-02-CTA 扩展", "色彩位或 HDR 块扫描漂移");
    }
}

fn chk_tolerance(set: &mut CheckSet) {
    // 容错-01 结构损坏三向：短块/魔数错/校验和错各显性码拒。
    use api::codes::*;
    let short = vec![0u8; 64];
    let mut bad_magic = valid_edid_1080p60();
    bad_magic[2] = 0x12;
    let mut bad_sum = valid_edid_1080p60();
    bad_sum[100] = bad_sum[100] ^ 0xFF;
    let ok = parse_edid(&short) == Err(EDID_MALFORMED)
        && parse_edid(&bad_magic) == Err(EDID_HEADER)
        && parse_edid(&bad_sum) == Err(EDID_CHECKSUM);
    if ok {
        set.ok("E923-容错-01-结构损坏三向");
    } else {
        set.fail("E923-容错-01-结构损坏三向", "损坏形态未被显性拒绝");
    }
    // 容错-02 截断降级读出：声明扩展但区截断 → Ok + degraded 标注，基块不虚构。
    let mut trunc = valid_edid_hdr();
    trunc.truncate(130);
    match parse_edid(&trunc) {
        Ok(c) => {
            if c.degraded && c.max_h == 1920 && c.max_v == 1080 && !c.hdr_static {
                set.ok("E923-容错-02-截断降级读出");
            } else {
                set.fail("E923-容错-02-截断降级读出", "降级读出未标注或基块字段漂移");
            }
        }
        Err(_) => set.fail("E923-容错-02-截断降级读出", "截断被硬拒而非降级读出"),
    }
}

fn chk_cache(set: &mut CheckSet) {
    // 缓存-01 同屏同指纹命中：第二次不再调 loader。
    let mut cache = EdidCache::new();
    let mut loads = 0u32;
    let fp = 0xAABB_CCDD;
    let (c1, hit1) = match cache.get_or_load(7, fp, || {
        loads += 1;
        parse_edid(&valid_edid_1080p60())
    }) {
        Ok(x) => x,
        Err(_) => {
            set.fail("E923-缓存-01-指纹命中", "首次加载失败");
            return;
        }
    };
    let (c2, hit2) = match cache.get_or_load(7, fp, || {
        loads += 1;
        parse_edid(&valid_edid_1080p60())
    }) {
        Ok(x) => x,
        Err(_) => {
            set.fail("E923-缓存-01-指纹命中", "二次读取失败");
            return;
        }
    };
    if loads == 1 && !hit1 && hit2 && c1 == c2 && c2.max_h == 1920 {
        set.ok("E923-缓存-01-指纹命中");
    } else {
        set.fail("E923-缓存-01-指纹命中", "命中语义或 loader 计账漂移");
    }
    // 缓存-02 指纹失效重读：换屏后 fp 变 → 重读并更新能力（绝不回旧账）。
    let (c3, hit3) = match cache.get_or_load(7, 0x1122_3344, || {
        loads += 1;
        parse_edid(&valid_edid_hdr())
    }) {
        Ok(x) => x,
        Err(_) => {
            set.fail("E923-缓存-02-失效重读", "重读失败");
            return;
        }
    };
    if loads == 2 && !hit3 && c3.hdr_static {
        set.ok("E923-缓存-02-失效重读");
    } else {
        set.fail("E923-缓存-02-失效重读", "指纹变更未触发重读或仍回旧能力");
    }
}

fn chk_projection(set: &mut CheckSet) {
    // 投影-01 纯裁剪：越分辨率/越刷新被裁、恰等边界保留、顺序保序。
    let caps = match parse_edid(&valid_edid_1080p60()) {
        Ok(c) => c,
        Err(_) => {
            set.fail("E923-投影-01-纯裁剪", "夹具解析失败");
            return;
        }
    };
    let want = vec![
        Mode { h: 1280, v: 720, refresh_millihz: 60_000 },
        Mode { h: 1920, v: 1080, refresh_millihz: 60_000 },
        Mode { h: 2560, v: 1440, refresh_millihz: 60_000 },
        Mode { h: 1920, v: 1080, refresh_millihz: 144_000 },
    ];
    let out = project(&caps, &want);
    let expect_ok = out.len() == 2
        && out[0] == want[0]
        && out[1] == want[1]
        && projection_honest(&want, &out);
    if expect_ok {
        set.ok("E923-投影-01-纯裁剪");
    } else {
        set.fail("E923-投影-01-纯裁剪", "裁剪越界/放行或顺序漂移");
    }
    // 投影-02 诚实红线：虚构模式被 projection_honest 抓出；PROJECTION_LIE 码可观测。
    let mut alien = out.clone();
    alien.push(Mode { h: 999, v: 999, refresh_millihz: 99_999 });
    if projection_honest(&want, &out) && !projection_honest(&want, &alien) {
        set.ok("E923-投影-02-诚实红线");
    } else {
        set.fail("E923-投影-02-诚实红线", "虚构模式未被诚实机检抓出");
    }
}

fn chk_override(set: &mut CheckSet) {
    // 覆盖-01 白名单托底：白名单内托到实测下限、表外拒绝且不改账。
    use api::codes::*;
    let mut caps = match parse_edid(&lying_edid_1080p30()) {
        Ok(c) => c,
        Err(_) => {
            set.fail("E923-覆盖-01-白名单台账", "说谎面板夹具解析失败");
            return;
        }
    };
    if caps.max_refresh_millihz != 30_000 {
        set.fail("E923-覆盖-01-白名单台账", "30Hz 夹具手算漂移");
        return;
    }
    let applied = apply_override(&mut caps, 0x0469, 0x1234);
    let mut untouched = caps;
    untouched.max_refresh_millihz = 30_000;
    let unlisted = apply_override(&mut untouched, 0x9999, 0x9999);
    if applied == Ok(true)
        && caps.max_refresh_millihz == 60_000
        && unlisted == Err(OVERRIDE_UNLISTED)
        && untouched.max_refresh_millihz == 30_000
    {
        set.ok("E923-覆盖-01-白名单台账");
    } else {
        set.fail("E923-覆盖-01-白名单台账", "托底值/表外拒绝/不改账语义漂移");
    }
    // 全链-01 端到端：说谎面板 解析→覆盖托底→投影 1080p60 放行。
    let mut caps = match parse_edid(&lying_edid_1080p30()) {
        Ok(c) => c,
        Err(_) => {
            set.fail("E923-全链-01-说谎面板端到端", "夹具解析失败");
            return;
        }
    };
    let _ = apply_override(&mut caps, 0x0469, 0x1234);
    let want = vec![Mode { h: 1920, v: 1080, refresh_millihz: 60_000 }];
    let out = project(&caps, &want);
    if out.len() == 1 && projection_honest(&want, &out) {
        set.ok("E923-全链-01-说谎面板端到端");
    } else {
        set.fail("E923-全链-01-说谎面板端到端", "覆盖后投影未按实测能力放行");
    }
    // 摘要-01 摘要承载。
    let line = screen_line();
    if line.contains(CGM03_VERSION) && line.contains("能力不虚报") {
        set.ok("E923-摘要-01-摘要承载");
    } else {
        set.fail("E923-摘要-01-摘要承载", "版本或诚实声明漂移");
    }
}

// ---------------------------------------------------------------------------
// B 族：判据承载力
// ---------------------------------------------------------------------------

fn chk_codes(set: &mut CheckSet) {
    // 判据-01 六码续占 0x540D..0x5412、互异、不越入 cgm02 段（<=0x540C）。
    use api::codes::*;
    let vals = [
        EDID_MALFORMED,
        EDID_HEADER,
        EDID_CHECKSUM,
        CACHE_STALE,
        PROJECTION_LIE,
        OVERRIDE_UNLISTED,
    ];
    let mut distinct = true;
    let mut i = 0usize;
    while i < vals.len() {
        let mut j = i + 1;
        while j < vals.len() {
            if vals[i] == vals[j] {
                distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    let seg_ok = vals.iter().all(|c| c.0 >= 0x540D && c.0 <= 0x5412);
    if distinct && seg_ok {
        set.ok("E923-判据-01-码段续占");
    } else {
        set.fail("E923-判据-01-码段续占", "码漂移/撞号/越段");
    }
}

/// 判据侧手写刷新率公式（双源同构——不复用生产代码路径；u64 防溢出）。
fn ref_refresh(pxclk_10k: u32, htotal: u32, vtotal: u32) -> u32 {
    if htotal == 0 || vtotal == 0 {
        return 0;
    }
    let num = (pxclk_10k as u64) * 10_000_000u64;
    let den = (htotal as u64) * (vtotal as u64);
    (num / den) as u32
}

fn chk_fingerprint(set: &mut CheckSet) {
    // 判据-02 刷新率公式双源对账：判据侧手算与解析结果同值。
    let caps = match parse_edid(&valid_edid_1080p60()) {
        Ok(c) => c,
        Err(_) => {
            set.fail("E923-判据-02-刷新率双源", "夹具解析失败");
            return;
        }
    };
    let expect = ref_refresh(14850, 2200, 1125);
    if expect == 60_000 && caps.max_refresh_millihz == expect {
        set.ok("E923-判据-02-刷新率双源");
    } else {
        set.fail("E923-判据-02-刷新率双源", "公式或手算值漂移");
    }
    // 白名单台账：两条目键对互异、说明互异非空。
    let mut wl_ok = OVERRIDE_WHITELIST.len() == 2;
    let a = &OVERRIDE_WHITELIST[0];
    let b = &OVERRIDE_WHITELIST[1];
    if (a.mfg_id, a.product) == (b.mfg_id, b.product)
        || a.note.is_empty()
        || b.note.is_empty()
        || a.note == b.note
    {
        wl_ok = false;
    }
    if wl_ok {
        set.ok("E923-判据-02-白名单台账");
    } else {
        set.fail("E923-判据-02-白名单台账", "台账键冲突或说明雷同");
    }
}

/// 单遍词法剥除（字符串/注释不误伤）。
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
                let closed = b[i] == b'"';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if b[i] == b'\'' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'\'';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if let Some(c) = src.get(i..i + 1) {
            out.push_str(c);
        }
        i += 1;
    }
    out
}

fn chk_zero_panic(set: &mut CheckSet) {
    // 判据-03 生产面零 panic。
    let src = include_str!("cgm03_edid.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!", "unwrap_or_else(||"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("E923-判据-03-生产面零 panic");
    } else {
        set.fail("E923-判据-03-生产面零 panic", "生产面含 panic 面");
    }
    // 判据-04 判据面零 panic（自扫）。
    let src = include_str!("cgm03_edid_checks.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("E923-判据-04-判据面零 panic");
    } else {
        set.fail("E923-判据-04-判据面零 panic", "判据面含 panic 面");
    }
}

fn chk_not_truncated(set: &mut CheckSet) {
    // 判据-05 聚合守恒防自调。
    let a = run_cgm03_checks_a_standalone();
    let (ap, af) = a.tally();
    let (sp, sf) = set.tally();
    let a_ok = ap + af == EXPECT_A_COUNT;
    let self_ok = sp + sf == EXPECT_B_BEFORE;
    let no_trunc = !a.truncated() && !set.truncated() && a.dropped() == 0 && set.dropped() == 0;
    if a_ok && self_ok && no_trunc {
        set.ok("E923-判据-05-聚合守恒防自调");
    } else {
        set.fail("E923-判据-05-聚合守恒防自调", "族条数漂移或有截断/丢弃");
    }
}

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

/// 判据族 a：解析 + 容错 + 缓存 + 投影 + 覆盖 + 全链 + 摘要。
pub fn run_cgm03_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("cgpu/cgm03/a");
    chk_parse(&mut s);
    chk_tolerance(&mut s);
    chk_cache(&mut s);
    chk_projection(&mut s);
    chk_override(&mut s);
    s
}

/// 判据族 b：判据承载力。
pub fn run_cgm03_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("cgpu/cgm03/b");
    chk_codes(&mut s);
    chk_fingerprint(&mut s);
    chk_zero_panic(&mut s);
    chk_not_truncated(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_cgm03_checks() -> CheckSet {
    CheckSet::merge(run_cgm03_checks_a_standalone(), run_cgm03_checks_b_standalone())
}
