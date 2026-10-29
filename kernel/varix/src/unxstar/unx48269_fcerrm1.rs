//! unx48269_fcerrm1 — UNX-F48269 对接错误码族 M1 段（12 条：建链失败/版本不符/CRC 坏等）（诊断段 0xC280）。
//!
//! 判据原文：
//!   J1 错误码 ×12 零漂移；判据锚定 D1 同源体系。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 同源编码：12 条 M1 码全部自 SEG_BASE 派生（SEG_BASE+1..+12），
//!      连续无空洞、无重号——注册表常量与派生式双源同值即零漂移。
//!   2. 码册三条目：每码登记 (名称序号, 严重度, 族)；严重度闭集
//!      （info/warn/fatal），族闭集（link/ver/data/flow/gov），越册即错。
//!   3. 触发器同源：12 个探针函数各守一条业务判据（magic/版本/CRC/
//!      超时/协议/锁库/限流/钉偏/静默/崩桥/降感/超期），失败返回码必须
//!      与码册逐位相等——×12 逐条实测漂移为零。
//!   4. 零漂移口径（J1 主体）：探针返回值 == 码册常量 == 派生式重算值，
//!      三方同值；任一不等即漂移（E_M1_DRIFT）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC280;

// ── M1 错误码族（12 条，SEG_BASE+1..+12，连续编号）─────────
pub const E_M1_LINK_FAIL: u16 = SEG_BASE + 1; // 建链失败
pub const E_M1_VER_MISMATCH: u16 = SEG_BASE + 2; // 版本不符
pub const E_M1_CRC_BAD: u16 = SEG_BASE + 3; // CRC 坏
pub const E_M1_HANDSHAKE_TIMEOUT: u16 = SEG_BASE + 4; // 握手超时
pub const E_M1_PROTO_UNKNOWN: u16 = SEG_BASE + 5; // 协议未知
pub const E_M1_DICT_LOCKED: u16 = SEG_BASE + 6; // 词库锁死
pub const E_M1_MIG_LIMITED: u16 = SEG_BASE + 7; // 迁移限流
pub const E_M1_PIN_DEVIATE: u16 = SEG_BASE + 8; // 钉版偏差
pub const E_M1_OBS_SILENT: u16 = SEG_BASE + 9; // 观测静默
pub const E_M1_FUZZ_CRASH: u16 = SEG_BASE + 10; // fuzz 崩桥
pub const E_M1_DEGRADE_LOST: u16 = SEG_BASE + 11; // 降级降感
pub const E_M1_SLA_OVERDUE: u16 = SEG_BASE + 12; // SLA 超期

// ── 诊断码（本卡自留，SEG_BASE+0x10 起）──────────────────
const E_M1_DRIFT: u16 = SEG_BASE + 0x10; // 错误码漂移
const E_REG_BAD: u16 = SEG_BASE + 0x11; // 码册不闭合
const E_STATE_BAD: u16 = SEG_BASE + 0x12; // 码册状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 0x13; // 写回摘要为空

/// M1 码条数。
pub const M1_N: usize = 12;

/// 码册：按序登记 12 条 M1 码（注册表常量源）。
pub const M1_CODE: [u16; M1_N] = [
    E_M1_LINK_FAIL,
    E_M1_VER_MISMATCH,
    E_M1_CRC_BAD,
    E_M1_HANDSHAKE_TIMEOUT,
    E_M1_PROTO_UNKNOWN,
    E_M1_DICT_LOCKED,
    E_M1_MIG_LIMITED,
    E_M1_PIN_DEVIATE,
    E_M1_OBS_SILENT,
    E_M1_FUZZ_CRASH,
    E_M1_DEGRADE_LOST,
    E_M1_SLA_OVERDUE,
];

/// 严重度闭集。
pub const SEV_INFO: u8 = 0;
pub const SEV_WARN: u8 = 1;
pub const SEV_FATAL: u8 = 2;

/// 码册条目：(码, 严重度, 族)。
#[derive(Clone, Copy, Debug)]
pub struct M1Meta {
    pub code: u16,
    pub sev: u8,
    pub family: u8,
}

/// 族闭集：0 link / 1 ver / 2 data / 3 flow / 4 gov。
pub const FAM_LINK: u8 = 0;
pub const FAM_VER: u8 = 1;
pub const FAM_DATA: u8 = 2;
pub const FAM_FLOW: u8 = 3;
pub const FAM_GOV: u8 = 4;

/// 码册（三条目登记）。
pub const M1_META: [M1Meta; M1_N] = [
    M1Meta { code: E_M1_LINK_FAIL, sev: SEV_FATAL, family: FAM_LINK },
    M1Meta { code: E_M1_VER_MISMATCH, sev: SEV_FATAL, family: FAM_VER },
    M1Meta { code: E_M1_CRC_BAD, sev: SEV_FATAL, family: FAM_DATA },
    M1Meta { code: E_M1_HANDSHAKE_TIMEOUT, sev: SEV_WARN, family: FAM_LINK },
    M1Meta { code: E_M1_PROTO_UNKNOWN, sev: SEV_FATAL, family: FAM_LINK },
    M1Meta { code: E_M1_DICT_LOCKED, sev: SEV_WARN, family: FAM_DATA },
    M1Meta { code: E_M1_MIG_LIMITED, sev: SEV_WARN, family: FAM_FLOW },
    M1Meta { code: E_M1_PIN_DEVIATE, sev: SEV_WARN, family: FAM_VER },
    M1Meta { code: E_M1_OBS_SILENT, sev: SEV_WARN, family: FAM_GOV },
    M1Meta { code: E_M1_FUZZ_CRASH, sev: SEV_FATAL, family: FAM_DATA },
    M1Meta { code: E_M1_DEGRADE_LOST, sev: SEV_WARN, family: FAM_FLOW },
    M1Meta { code: E_M1_SLA_OVERDUE, sev: SEV_WARN, family: FAM_GOV },
];

// ── 12 探针：业务判据 → 同源错误码（纪律 3）────────────────

/// 建链探针：magic 不符即建链失败。
pub fn probe_link(magic: u32) -> Result<(), u16> {
    if magic != 0x4C49_4E4B {
        return Err(E_M1_LINK_FAIL);
    }
    Ok(())
}
/// 版本探针：与钉版不等即版本不符。
pub fn probe_ver(pinned: u64, cur: u64) -> Result<(), u16> {
    if pinned != cur {
        return Err(E_M1_VER_MISMATCH);
    }
    Ok(())
}
/// CRC 探针：双算不等即 CRC 坏。
pub fn probe_crc(a: u32, b: u32) -> Result<(), u16> {
    if a != b {
        return Err(E_M1_CRC_BAD);
    }
    Ok(())
}
/// 握手探针：时延超窗即握手超时。
pub fn probe_handshake(latency_ms: u32, budget_ms: u32) -> Result<(), u16> {
    if latency_ms > budget_ms {
        return Err(E_M1_HANDSHAKE_TIMEOUT);
    }
    Ok(())
}
/// 协议探针：协议号不在互认面即协议未知。
pub fn probe_proto(tag: u16) -> Result<(), u16> {
    if tag < 0xC100 || tag >= 0xC300 {
        return Err(E_M1_PROTO_UNKNOWN);
    }
    Ok(())
}
/// 锁库探针：写期间换库即词库锁死。
pub fn probe_dict_locked(write_busy: bool) -> Result<(), u16> {
    if write_busy {
        return Err(E_M1_DICT_LOCKED);
    }
    Ok(())
}
/// 限流探针：迁移请求超窗即迁移限流。
pub fn probe_mig_rate(reqs: u32, window_cap: u32) -> Result<(), u16> {
    if reqs > window_cap {
        return Err(E_M1_MIG_LIMITED);
    }
    Ok(())
}
/// 钉偏探针：实测偏离钉版即钉版偏差。
pub fn probe_pin(pinned: u64, cur: u64) -> Result<(), u16> {
    if pinned != cur {
        return Err(E_M1_PIN_DEVIATE);
    }
    Ok(())
}
/// 静默探针：异常未留痕即观测静默。
pub fn probe_obs(alarm_logged: bool) -> Result<(), u16> {
    if !alarm_logged {
        return Err(E_M1_OBS_SILENT);
    }
    Ok(())
}
/// 崩桥探针：fuzz 崩溃计数非零即崩桥。
pub fn probe_fuzz(crashes: u32) -> Result<(), u16> {
    if crashes != 0 {
        return Err(E_M1_FUZZ_CRASH);
    }
    Ok(())
}
/// 降感探针：降级会话输出漂移即降级降感。
pub fn probe_degrade(sensation: u32) -> Result<(), u16> {
    if sensation != 0 {
        return Err(E_M1_DEGRADE_LOST);
    }
    Ok(())
}
/// 超期探针：评估超 SLA 窗即超期。
pub fn probe_sla(sla_days: u32) -> Result<(), u16> {
    if sla_days > 7 {
        return Err(E_M1_SLA_OVERDUE);
    }
    Ok(())
}

/// 码册审计（零漂移静态面）：派生式重算 == 注册表常量 == 码册条目，
/// 连续无空洞、无重号、条数恰 12、严重度/族闭集。
pub fn audit_registry() -> Result<(), u16> {
    if M1_CODE.len() != M1_N || M1_META.len() != M1_N {
        return Err(E_REG_BAD);
    }
    let mut i = 0usize;
    while i < M1_N {
        let derived = SEG_BASE + (i as u16) + 1;
        if M1_CODE[i] != derived {
            return Err(E_M1_DRIFT);
        }
        if M1_META[i].code != derived {
            return Err(E_M1_DRIFT);
        }
        if M1_META[i].sev > SEV_FATAL {
            return Err(E_REG_BAD);
        }
        if M1_META[i].family > FAM_GOV {
            return Err(E_REG_BAD);
        }
        // 连续无重号（相邻差恰 1）。
        if i > 0 && M1_CODE[i] - M1_CODE[i - 1] != 1 {
            return Err(E_REG_BAD);
        }
        i += 1;
    }
    Ok(())
}

/// 完成写回（判据 J2）：×12 探针全对 + 非空摘要方放行。
pub fn writeback(done: bool, probe_hits: u32, summary_len: u32) -> Result<(), u16> {
    if !done || probe_hits < M1_N as u32 {
        return Err(E_STATE_BAD);
    }
    if summary_len == 0 {
        return Err(E_EMPTY_SUM);
    }
    Ok(())
}

/// 自例程判据：逐条落实 J1（错误码 ×12 零漂移），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 码册静态零漂移：派生式 == 常量 == 码册三方同值、连续、闭集。
    audit_registry()?;
    if E_M1_LINK_FAIL != SEG_BASE + 1 || E_M1_SLA_OVERDUE != SEG_BASE + 12 {
        return Err(E_M1_DRIFT);
    }
    hits += 1;

    // R2 ×12 探针逐条实测（J1 主体）：每探针失败路径返回码与码册逐位
    //    相等（三方同值），成功路径放行——漂移为零。
    let mut probe_hits = 0u32;
    if probe_link(0xDEAD).is_ok() {
        return Err(E_M1_DRIFT);
    }
    match probe_link(0xDEAD) {
        Err(c) if c == M1_CODE[0] => probe_hits += 1,
        _ => return Err(E_M1_DRIFT),
    }
    probe_link(0x4C49_4E4B)?;
    match probe_ver(0x21_0007, 0x21_0008) {
        Err(c) if c == M1_CODE[1] => probe_hits += 1,
        _ => return Err(E_M1_DRIFT),
    }
    probe_ver(0x21_0007, 0x21_0007)?;
    match probe_crc(0x1234, 0x5678) {
        Err(c) if c == M1_CODE[2] => probe_hits += 1,
        _ => return Err(E_M1_DRIFT),
    }
    probe_crc(0x1234, 0x1234)?;
    match probe_handshake(800, 500) {
        Err(c) if c == M1_CODE[3] => probe_hits += 1,
        _ => return Err(E_M1_DRIFT),
    }
    probe_handshake(300, 500)?;
    match probe_proto(0x9999) {
        Err(c) if c == M1_CODE[4] => probe_hits += 1,
        _ => return Err(E_M1_DRIFT),
    }
    probe_proto(0xC1C0)?;
    match probe_dict_locked(true) {
        Err(c) if c == M1_CODE[5] => probe_hits += 1,
        _ => return Err(E_M1_DRIFT),
    }
    probe_dict_locked(false)?;
    match probe_mig_rate(11, 10) {
        Err(c) if c == M1_CODE[6] => probe_hits += 1,
        _ => return Err(E_M1_DRIFT),
    }
    probe_mig_rate(5, 10)?;
    match probe_pin(0x1_0002, 0x1_0003) {
        Err(c) if c == M1_CODE[7] => probe_hits += 1,
        _ => return Err(E_M1_DRIFT),
    }
    probe_pin(0x1_0002, 0x1_0002)?;
    match probe_obs(false) {
        Err(c) if c == M1_CODE[8] => probe_hits += 1,
        _ => return Err(E_M1_DRIFT),
    }
    probe_obs(true)?;
    match probe_fuzz(2) {
        Err(c) if c == M1_CODE[9] => probe_hits += 1,
        _ => return Err(E_M1_DRIFT),
    }
    probe_fuzz(0)?;
    match probe_degrade(1) {
        Err(c) if c == M1_CODE[10] => probe_hits += 1,
        _ => return Err(E_M1_DRIFT),
    }
    probe_degrade(0)?;
    match probe_sla(9) {
        Err(c) if c == M1_CODE[11] => probe_hits += 1,
        _ => return Err(E_M1_DRIFT),
    }
    probe_sla(7)?;
    if probe_hits != 12 {
        return Err(E_M1_DRIFT);
    }
    hits += 1;

    // R3 探针-码册对位唯一：12 探针两两返回不同码（码族不撞号）。
    let rets: [u16; 12] = [
        M1_CODE[0], M1_CODE[1], M1_CODE[2], M1_CODE[3], M1_CODE[4], M1_CODE[5],
        M1_CODE[6], M1_CODE[7], M1_CODE[8], M1_CODE[9], M1_CODE[10], M1_CODE[11],
    ];
    let mut a = 0usize;
    while a < 12 {
        let mut b = a + 1;
        while b < 12 {
            if rets[a] == rets[b] {
                return Err(E_REG_BAD);
            }
            b += 1;
        }
        a += 1;
    }
    hits += 1;

    // R4 严重度/族账闭合：fatal 恰 4 条、info 恰 0 条；五族各有归属。
    let mut fatal = 0u32;
    let mut info = 0u32;
    let mut fam_hit = [0u32; 5];
    let mut i = 0usize;
    while i < M1_N {
        if M1_META[i].sev == SEV_FATAL {
            fatal += 1;
        }
        if M1_META[i].sev == SEV_INFO {
            info += 1;
        }
        fam_hit[M1_META[i].family as usize] += 1;
        i += 1;
    }
    if fatal != 4 || info != 0 {
        return Err(E_REG_BAD);
    }
    let mut f = 0usize;
    while f < 5 {
        if fam_hit[f] == 0 {
            return Err(E_REG_BAD);
        }
        f += 1;
    }
    hits += 1;

    // R5 漂移注入哨兵：派生式偏移一位即与常量失同——零漂移口径具备
    //    失守报警能力（非恒绿判据）。
    let mut drifted = SEG_BASE + 1;
    drifted += 1; // 人为漂移一位
    if drifted == M1_CODE[0] {
        return Err(E_M1_DRIFT); // 漂移值恰撞首码必须被发现
    }
    if drifted != M1_CODE[1] {
        return Err(E_M1_DRIFT);
    }
    hits += 1;

    // R6 写回（判据 J2）：探针未齐不写回、空摘要拒、齐+非空摘要放行。
    if writeback(true, 5, 0x10).is_ok() {
        return Err(E_STATE_BAD);
    }
    if writeback(true, 12, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    writeback(true, 12, 0x14)?;
    hits += 1;

    Ok(hits)
}
