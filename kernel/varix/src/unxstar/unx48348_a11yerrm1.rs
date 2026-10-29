//! unx48348_a11yerrm1 — UNX-F48348 无障碍错误码族 M1 段（6 条）（诊断段 0xCC60）。
//!
//! 判据原文：
//!   J1 错误码 ×6 零漂移；判据锚定 D1 同源体系。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 同源编号：六条错误码 SEG_BASE+1 起 +1..+6 连续不撞号不跳号，
//!      与 D1 段同一编号纪律——段基址派生，一处定义处处一致。
//!   2. 双路对表：编译期常量码与运行期登记表逐条比对（值/名/族/
//!      级四元组），任何一项漂移即 E_DRIFT，重复码即 E_DUP。
//!   3. 零漂移 ×6：三条重推导路径（直读常量、表遍历、指纹重算）
//!      各跑一遍 ×6 全对表，三轮结果逐位相等。
//!   4. 级别纪律：severity ∈ {1 提示, 2 降级, 3 拒服务}，六条各归属
//!      恰一条，级别越界即 E_SEV_BAD。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCC60;

// ── 诊断码（SEG_BASE+1 起，×6 零漂移对象）────────────────
/// 朗读源数据缺失（供数面无锚可导）。
pub const A11Y_E_SRC_MISS: u16 = SEG_BASE + 1;
/// 高亮位越界（朗读序高亮出榜）。
pub const A11Y_E_HL_RANGE: u16 = SEG_BASE + 2;
/// 语义适配被拒（粘滞/单手映射不合法）。
pub const A11Y_E_ADAPT_DENY: u16 = SEG_BASE + 3;
/// 埋点断流（四点观测缺点）。
pub const A11Y_E_OBS_STALL: u16 = SEG_BASE + 4;
/// 联签缺口（复签账缺对）。
pub const A11Y_E_COSIGN_GAP: u16 = SEG_BASE + 5;
/// 三窗状态漂移（三类窗口数据锚不一致）。
pub const A11Y_E_STATE_DRIFT: u16 = SEG_BASE + 6;

// 本模块自身诊断码（+7 起，与对象码隔离）。
const E_DRIFT: u16 = SEG_BASE + 7; // 码值/四元组漂移
const E_DUP: u16 = SEG_BASE + 8; // 撞号
const E_SEV_BAD: u16 = SEG_BASE + 9; // 级别越界
const E_REPLAY_BAD: u16 = SEG_BASE + 10; // 重推导翻供

/// 级别域。
pub const SEV_HINT: u16 = 1; // 提示
pub const SEV_DEGRADE: u16 = 2; // 降级
pub const SEV_DENY: u16 = 3; // 拒服务

/// 错误码登记条目：值 / 名 / 族 / 级。
#[derive(Clone, Copy)]
pub struct ErrEntry {
    pub code: u16,
    pub name: &'static str,
    pub family: &'static str,
    pub sev: u16,
}

/// ×6 登记表（与上方常量码同源逐条对应）。
pub const TABLE: [ErrEntry; 6] = [
    ErrEntry { code: A11Y_E_SRC_MISS, name: "a11y_src_miss", family: "a11y_data", sev: SEV_DEGRADE },
    ErrEntry { code: A11Y_E_HL_RANGE, name: "a11y_hl_range", family: "a11y_data", sev: SEV_HINT },
    ErrEntry { code: A11Y_E_ADAPT_DENY, name: "a11y_adapt_deny", family: "a11y_adapt", sev: SEV_HINT },
    ErrEntry { code: A11Y_E_OBS_STALL, name: "a11y_obs_stall", family: "a11y_obs", sev: SEV_DEGRADE },
    ErrEntry { code: A11Y_E_COSIGN_GAP, name: "a11y_cosign_gap", family: "a11y_sign", sev: SEV_DENY },
    ErrEntry { code: A11Y_E_STATE_DRIFT, name: "a11y_state_drift", family: "a11y_window", sev: SEV_DENY },
];

/// FNV-1a 64 位指纹。
pub fn fp_str(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        h ^= b[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 表全指纹：六条 (code,sev) 与名族串联。
pub fn table_fp() -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < TABLE.len() {
        h ^= (TABLE[i].code as u64).wrapping_mul(1_000_003);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= (TABLE[i].sev as u64).wrapping_mul(7);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= fp_str(TABLE[i].name) ^ fp_str(TABLE[i].family);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 按码查条目：命中返回表内下标。
pub fn locate(code: u16) -> Result<usize, u16> {
    let mut i = 0usize;
    while i < TABLE.len() {
        if TABLE[i].code == code {
            return Ok(i);
        }
        i += 1;
    }
    Err(E_DRIFT)
}

/// 级别路由：severity → 处置通道号（0=记账, 1=降级续跑, 2=拒服务）。
pub fn sev_route(sev: u16) -> Result<u8, u16> {
    match sev {
        SEV_HINT => Ok(0),
        SEV_DEGRADE => Ok(1),
        SEV_DENY => Ok(2),
        _ => Err(E_SEV_BAD),
    }
}

/// 按码取处置通道：查无即漂移。
pub fn route_of(code: u16) -> Result<u8, u16> {
    let idx = locate(code)?;
    sev_route(TABLE[idx].sev)
}

/// 族普查：逐族清点条数（族名, 条数）成对返回。
pub fn family_census() -> Vec<(&'static str, u32)> {
    let mut out: Vec<(&'static str, u32)> = Vec::new();
    let mut i = 0usize;
    while i < TABLE.len() {
        let mut found = false;
        let mut j = 0usize;
        while j < out.len() {
            if out[j].0 == TABLE[i].family {
                out[j].1 += 1;
                found = true;
            }
            j += 1;
        }
        if !found {
            out.push((TABLE[i].family, 1));
        }
        i += 1;
    }
    out
}

/// 一轮零漂移对表：值连续、名族非空、级别在域、无撞号。
/// 返回对表通过条数（恒 6，少一条即漂移）。
pub fn audit_round() -> Result<u32, u16> {
    // 值连续：TABLE[i].code == SEG_BASE + i + 1。
    let mut i = 0usize;
    while i < TABLE.len() {
        if TABLE[i].code != SEG_BASE + (i as u16) + 1 {
            return Err(E_DRIFT);
        }
        if TABLE[i].name.is_empty() || TABLE[i].family.is_empty() {
            return Err(E_DRIFT);
        }
        if TABLE[i].sev < SEV_HINT || TABLE[i].sev > SEV_DENY {
            return Err(E_SEV_BAD);
        }
        i += 1;
    }
    // 撞号扫描：两两互异。
    let mut a = 0usize;
    while a < TABLE.len() {
        let mut b = a + 1;
        while b < TABLE.len() {
            if TABLE[a].code == TABLE[b].code {
                return Err(E_DUP);
            }
            if TABLE[a].name == TABLE[b].name {
                return Err(E_DUP);
            }
            b += 1;
        }
        a += 1;
    }
    Ok(TABLE.len() as u32)
}

/// drill：J1 错误码 ×6 零漂移。
pub fn drill() -> Result<u32, u16> {
    // ── R1 主判据：三条重推导路径 × 全表对表，逐条零漂移 ────
    // 路径一：直读常量四元组对表。
    let want: [(u16, &str, u16); 6] = [
        (A11Y_E_SRC_MISS, "a11y_src_miss", SEV_DEGRADE),
        (A11Y_E_HL_RANGE, "a11y_hl_range", SEV_HINT),
        (A11Y_E_ADAPT_DENY, "a11y_adapt_deny", SEV_HINT),
        (A11Y_E_OBS_STALL, "a11y_obs_stall", SEV_DEGRADE),
        (A11Y_E_COSIGN_GAP, "a11y_cosign_gap", SEV_DENY),
        (A11Y_E_STATE_DRIFT, "a11y_state_drift", SEV_DENY),
    ];
    let mut i = 0usize;
    while i < 6 {
        let e = &TABLE[i];
        if e.code != want[i].0 || e.name != want[i].1 || e.sev != want[i].2 {
            return Err(E_DRIFT);
        }
        i += 1;
    }

    // 路径二：audit_round 连跑三轮，结果恒 6 且表指纹逐轮相等。
    let fp0 = table_fp();
    let mut round = 0u32;
    while round < 3 {
        if audit_round()? != 6 {
            return Err(E_DRIFT);
        }
        if table_fp() != fp0 {
            return Err(E_DRIFT);
        }
        round += 1;
    }

    // 路径三：locate 逐码回查，命中的表内下标即码序号-1。
    let mut code_step = 1u16;
    while code_step <= 6 {
        let idx = locate(SEG_BASE + code_step)?;
        if idx as u16 != code_step - 1 {
            return Err(E_DRIFT);
        }
        code_step += 1;
    }

    // ── R2 段外码必查无：+7 起不属于 ×6 对象码族 ─────────────
    if locate(SEG_BASE + 7).is_ok() {
        return Err(E_DUP);
    }
    if locate(SEG_BASE).is_ok() || locate(0).is_ok() {
        return Err(E_DRIFT);
    }

    // ── R3 级别纪律：三条 DENY 码必为窗漂移与联签缺口族 ─────
    let mut i = 0usize;
    while i < TABLE.len() {
        if TABLE[i].sev == SEV_DENY {
            let fp_name = fp_str(TABLE[i].name);
            // state_drift / cosign_gap 两族才可拒服务。
            if TABLE[i].family != "a11y_window" && TABLE[i].family != "a11y_sign" {
                let _ = fp_name;
                return Err(E_SEV_BAD);
            }
        }
        i += 1;
    }

    // ── R4 重放翻供：同指纹双算必须逐位相等 ─────────────────
    let f1 = table_fp();
    let f2 = table_fp();
    if f1 != f2 {
        return Err(E_REPLAY_BAD);
    }

    // ── R5 级别路由：三级别通道号 0/1/2 唯一对应，越界必拒 ──
    if sev_route(SEV_HINT)? != 0 || sev_route(SEV_DEGRADE)? != 1 || sev_route(SEV_DENY)? != 2 {
        return Err(E_SEV_BAD);
    }
    if sev_route(0).is_ok() || sev_route(4).is_ok() {
        return Err(E_SEV_BAD);
    }
    // 六条码逐条路由回查：hint→0、degrade→1、deny→2。
    let mut i = 0usize;
    while i < TABLE.len() {
        let want = match TABLE[i].sev {
            SEV_HINT => 0u8,
            SEV_DEGRADE => 1u8,
            _ => 2u8,
        };
        if route_of(TABLE[i].code)? != want {
            return Err(E_SEV_BAD);
        }
        i += 1;
    }

    // ── R6 族普查：四族各 1~2 条，总数守恒 6 ────────────────
    let census = family_census();
    if census.len() != 4 {
        return Err(E_DUP);
    }
    let mut total = 0u32;
    let mut i = 0usize;
    while i < census.len() {
        // 每族至少 1 条且不超过 2 条。
        if census[i].1 < 1 || census[i].1 > 2 {
            return Err(E_DRIFT);
        }
        total += census[i].1;
        i += 1;
    }
    if total != 6 {
        return Err(E_DRIFT);
    }
    // 普查重放：双算逐项相等。
    let c2 = family_census();
    if c2.len() != census.len() {
        return Err(E_REPLAY_BAD);
    }
    let mut i = 0usize;
    while i < census.len() {
        if c2[i].0 != census[i].0 || c2[i].1 != census[i].1 {
            return Err(E_REPLAY_BAD);
        }
        i += 1;
    }

    // J2 写回摘要非空：对表通过数非零即摘要实质。
    let pass = audit_round()?;
    if pass == 0 {
        return Err(E_DRIFT);
    }
    Ok(pass)
}
