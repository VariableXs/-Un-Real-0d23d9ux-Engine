//! UNX-F48399 —— M12 升级路径复验（B01–B20 期间配置/词库格式演进零破坏）
//! 判据原文：UNX-F48399-J1 按卡面验收句执行——
//!   J1-1 纯功能代码 ≥317 行；
//!   J1-2 配置格式版本链 v1..v20 逐级推进，每步 payload 旋转 7 位（可逆无损）；
//!   J1-3 逆迁移逐级回退必须还原原始 payload（零破坏判据）；
//!   J1-4 词库格式 v1..v4 逐级迁移，词表校验和与词条数全程不变（无损）；
//!   J1-5 前向兼容：当前实现必须能直读链上任一旧版本 blob；
//!   J1-6 损坏校验和 → E_CK；越界版本 → E_VER；链顶再升/链底再降 → E_CHAIN。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD2C0; // 卡面分配段

// ── 诊断码闭集（SEG_BASE+1 起连续编号，全部 ≤ SEG_BASE+0x1F）──────
const E_EMPTY: u16 = SEG_BASE + 1;  // 空 blob
const E_VER: u16 = SEG_BASE + 2;    // 版本越界
const E_CK: u16 = SEG_BASE + 3;     // 校验和损坏
const E_CHAIN: u16 = SEG_BASE + 4;  // 断链：链顶再升 / 链底再降
const E_LOSS: u16 = SEG_BASE + 5;   // 迁移有损：往返不一致
const E_LEX: u16 = SEG_BASE + 6;    // 词库迁移破坏（校验和/词条数变化）
const E_TARGET: u16 = SEG_BASE + 7; // 迁移目标版本非法
const E_FINGER: u16 = SEG_BASE + 8; // 指纹不稳定
const E_PAYLOAD: u16 = SEG_BASE + 9;// 终态 payload 与预期不符

pub fn detail(code: u16) -> &'static str {
    match code {
        E_EMPTY => "空 blob：无可迁移数据",
        E_VER => "版本越界：配置 v1..v20 / 词库 v1..v4 之外",
        E_CK => "校验和损坏：blob 完整性破坏",
        E_CHAIN => "断链：链顶再升或链底再降",
        E_LOSS => "迁移有损：逆迁移未还原原始 payload",
        E_LEX => "词库迁移破坏：词表校验和或词条数变化",
        E_TARGET => "迁移目标版本非法：低于当前或高于链顶",
        E_FINGER => "指纹不稳定：确定性被破坏",
        E_PAYLOAD => "终态 payload 与预期不符：链变换偏差",
        _ => "未知诊断码",
    }
}

/// 确定性 LCG。
fn lcg_next(state: &mut u32) -> u32 {
    *state = state
        .wrapping_mul(1_664_525)
        .wrapping_add(1_013_904_223);
    *state
}

/// FNV-1a 64。
fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

// ── 常量 ────────────────────────────────────────────────────
/// 配置格式版本链界（B01–B20 期间 20 次演进）。
pub const CFG_VER_FIRST: u8 = 1;
pub const CFG_VER_LAST: u8 = 20;
/// 词库格式版本链界。
pub const LEX_VER_FIRST: u8 = 1;
pub const LEX_VER_LAST: u8 = 4;
/// 每步 payload 旋转量。
pub const STEP_ROT: u32 = 7;

// ── 配置 blob ───────────────────────────────────────────────
/// 配置格式 blob：版本 + 载荷 + 校验和。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CfgBlob {
    pub ver: u8,
    pub payload: u32,
    pub ck: u64,
}

fn cfg_ck(ver: u8, payload: u32) -> u64 {
    let mut buf: Vec<u8> = vec![b'c', b'f', b'g'];
    buf.push(ver);
    buf.extend_from_slice(&payload.to_le_bytes());
    fnv1a64(&buf)
}

pub fn cfg_make(ver: u8, payload: u32) -> CfgBlob {
    CfgBlob {
        ver,
        payload,
        ck: cfg_ck(ver, payload),
    }
}

/// 直读：当前实现可读链上任一版本（前向兼容判据）。
pub fn cfg_read(b: &CfgBlob) -> Result<u32, u16> {
    if b.ck != cfg_ck(b.ver, b.payload) {
        return Err(E_CK);
    }
    if b.ver < CFG_VER_FIRST || b.ver > CFG_VER_LAST {
        return Err(E_VER);
    }
    Ok(b.payload)
}

/// 升一步：payload 旋转 7 位（可逆）。
pub fn cfg_step_up(b: &CfgBlob) -> Result<CfgBlob, u16> {
    let p = cfg_read(b)?;
    if b.ver >= CFG_VER_LAST {
        return Err(E_CHAIN);
    }
    Ok(cfg_make(b.ver + 1, p.rotate_left(STEP_ROT)))
}

/// 降一步：payload 反旋转（还原）。
pub fn cfg_step_down(b: &CfgBlob) -> Result<CfgBlob, u16> {
    let p = cfg_read(b)?;
    if b.ver <= CFG_VER_FIRST {
        return Err(E_CHAIN);
    }
    Ok(cfg_make(b.ver - 1, p.rotate_right(STEP_ROT)))
}

/// 链式升迁移：逐级推进到目标版本。
pub fn cfg_chain_up(b: &CfgBlob, target: u8) -> Result<CfgBlob, u16> {
    if target < b.ver || target > CFG_VER_LAST {
        return Err(E_TARGET);
    }
    let mut cur = *b;
    while cur.ver < target {
        cur = cfg_step_up(&cur)?;
    }
    Ok(cur)
}

/// 链式降迁移：逐级回退到目标版本。
pub fn cfg_chain_down(b: &CfgBlob, target: u8) -> Result<CfgBlob, u16> {
    if target > b.ver || target < CFG_VER_FIRST {
        return Err(E_TARGET);
    }
    let mut cur = *b;
    while cur.ver > target {
        cur = cfg_step_down(&cur)?;
    }
    Ok(cur)
}

// ── 词库 blob ───────────────────────────────────────────────
/// 词库格式 blob：版本 + 词条数 + 词表校验和 + 完整性校验。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LexBlob {
    pub ver: u8,
    pub count: u32,
    pub words_ck: u64,
    pub ck: u64,
}

fn lex_ck(ver: u8, count: u32, words_ck: u64) -> u64 {
    let mut buf: Vec<u8> = vec![b'l', b'e', b'x'];
    buf.push(ver);
    buf.extend_from_slice(&count.to_le_bytes());
    buf.extend_from_slice(&words_ck.to_le_bytes());
    fnv1a64(&buf)
}

pub fn lex_make(ver: u8, count: u32, words_ck: u64) -> LexBlob {
    LexBlob {
        ver,
        count,
        words_ck,
        ck: lex_ck(ver, count, words_ck),
    }
}

pub fn lex_read(b: &LexBlob) -> Result<(u32, u64), u16> {
    if b.ck != lex_ck(b.ver, b.count, b.words_ck) {
        return Err(E_CK);
    }
    if b.ver < LEX_VER_FIRST || b.ver > LEX_VER_LAST {
        return Err(E_VER);
    }
    Ok((b.count, b.words_ck))
}

/// 词库升一步：词表校验和与词条数不变（无损迁移）。
pub fn lex_step_up(b: &LexBlob) -> Result<LexBlob, u16> {
    let (count, words_ck) = lex_read(b)?;
    if b.ver >= LEX_VER_LAST {
        return Err(E_CHAIN);
    }
    Ok(lex_make(b.ver + 1, count, words_ck))
}

/// 词库链式升迁移 v_first → v_last，断言无损。
pub fn lex_chain_lossless(b: &LexBlob) -> Result<LexBlob, u16> {
    let (count0, ck0) = lex_read(b)?;
    let mut cur = *b;
    while cur.ver < LEX_VER_LAST {
        cur = lex_step_up(&cur)?;
    }
    let (count1, ck1) = lex_read(&cur)?;
    if count0 != count1 || ck0 != ck1 {
        return Err(E_LEX);
    }
    Ok(cur)
}

// ── 主巡检入口 ──────────────────────────────────────────────
/// 链路摘要文本。
pub fn chain_summary(steps_done: u32) -> String {
    let mut s = String::new();
    s.push_str("upgrade steps=");
    s.push_str(&steps_done.to_string());
    s
}

pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let steps = (CFG_VER_LAST - CFG_VER_FIRST) as u32; // 19 步

    // R1：配置版本链 v1 → v20 连续推进（19 步），版本逐级正确。
    let p0: u32 = 0x1234_5678;
    let start = cfg_make(CFG_VER_FIRST, p0);
    let top = cfg_chain_up(&start, CFG_VER_LAST)?;
    if top.ver != CFG_VER_LAST {
        return Err(E_CHAIN);
    }
    let sum = chain_summary(steps);
    if !sum.contains("19") {
        return Err(E_CHAIN);
    }
    hits += 1;

    // R2：终态 payload 与预期一致（rotate_left(7×19 mod 32)）。
    let expect = p0.rotate_left((STEP_ROT * steps) % 32);
    if top.payload != expect {
        return Err(E_PAYLOAD);
    }
    hits += 1;

    // R3：逆迁移无损：v20 → v1 逐级回退还原原始 payload。
    let back = cfg_chain_down(&top, CFG_VER_FIRST)?;
    if back.payload != p0 || back.ver != CFG_VER_FIRST {
        return Err(E_LOSS);
    }
    hits += 1;

    // R4：词库 v1 → v4 迁移无损：词条数与词表校验和全程不变。
    let lex = lex_make(LEX_VER_FIRST, 4096, 0xDEAD_BEEF_CAFE);
    let lex_top = lex_chain_lossless(&lex)?;
    if lex_top.ver != LEX_VER_LAST {
        return Err(E_LEX);
    }
    hits += 1;

    // R5：前向兼容：当前实现直读 v1 / v12 / v20 全部成功。
    let mid = cfg_chain_up(&start, 12)?;
    if cfg_read(&start)? != p0 || cfg_read(&mid)? != mid.payload || cfg_read(&top)? != top.payload {
        return Err(E_VER);
    }
    hits += 1;

    // R6：断链检出：链顶再升 / 链底再降 → E_CHAIN。
    if cfg_step_up(&top) != Err(E_CHAIN) {
        return Err(E_CHAIN);
    }
    if cfg_step_down(&start) != Err(E_CHAIN) {
        return Err(E_CHAIN);
    }
    if lex_step_up(&lex_top) != Err(E_CHAIN) {
        return Err(E_CHAIN);
    }
    hits += 1;

    // R7：损坏检出：篡改校验和 / 篡改 payload → E_CK。
    let mut corrupt = top;
    corrupt.ck = corrupt.ck.wrapping_add(1);
    if cfg_read(&corrupt) != Err(E_CK) {
        return Err(E_CK);
    }
    let mut corrupt2 = top;
    corrupt2.payload = corrupt2.payload.wrapping_add(1);
    if cfg_read(&corrupt2) != Err(E_CK) {
        return Err(E_CK);
    }
    hits += 1;

    // R8：越界版本检出：v0 / v21 直读 → E_VER；非法目标 → E_TARGET。
    let below = cfg_make(0, p0);
    if cfg_read(&below) != Err(E_VER) {
        return Err(E_VER);
    }
    let above = cfg_make(21, p0);
    if cfg_read(&above) != Err(E_VER) {
        return Err(E_VER);
    }
    if cfg_chain_up(&top, CFG_VER_FIRST) != Err(E_TARGET) {
        return Err(E_TARGET);
    }
    hits += 1;

    // R9：LCG 多组载荷全链往返无损：8 组随机 payload 升到顶再降回底。
    let mut state: u32 = 0xD2C0_0001;
    for _ in 0..8 {
        let p = lcg_next(&mut state);
        let s = cfg_make(CFG_VER_FIRST, p);
        let t = cfg_chain_up(&s, CFG_VER_LAST)?;
        let b = cfg_chain_down(&t, CFG_VER_FIRST)?;
        if b.payload != p {
            return Err(E_LOSS);
        }
    }
    hits += 1;

    // R10：指纹：同链两次构建指纹一致；不同载荷指纹不同。
    let fp_a = cfg_chain_up(&cfg_make(CFG_VER_FIRST, 42), CFG_VER_LAST)?.ck;
    let fp_b = cfg_chain_up(&cfg_make(CFG_VER_FIRST, 42), CFG_VER_LAST)?.ck;
    if fp_a != fp_b {
        return Err(E_FINGER);
    }
    let fp_c = cfg_chain_up(&cfg_make(CFG_VER_FIRST, 43), CFG_VER_LAST)?.ck;
    if fp_a == fp_c {
        return Err(E_FINGER);
    }
    // 链步数复核：19 步旋转总量 = 5 位（mod 32）。
    if (STEP_ROT * steps) % 32 != 5 {
        return Err(E_PAYLOAD);
    }
    hits += 1;

    Ok(hits)
}
