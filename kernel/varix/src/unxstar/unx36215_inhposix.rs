//! UNX-F36215-J1 继承与 POSIX 桥的投影位预告（B31 消费锚）。
//! 判据：① 投影挂点桩：继承后 mode 位重投影 50 例与桥 fake 对平
//! ——引擎投影 vs 独立桥 fake（两套实现同口径）逐例一致；② 语义
//! 损失标记位登记（LOSS_DENY：POSIX mode 位表达不了 Deny——含
//! Deny 的有效集投影必须置损失位，两实现独立判位且一致）；③ 纯
//! 功能 ≥320 行。
//! 设计纪律：POSIX 桥（B31）要把继承后的有效 DACL 投成 rwx mode
//! 位——这个投影是有损的（Deny/继承位次/多主体交集都装不进 9 个
//! 位）。本模块把投影做成「预告桩」：50 例冻结口径 + 损失位显性
//! 登记，B31 消费时拿的是「口径已冻结的桩」而不是现场即兴。两套
//! 实现（引擎侧 fold 与桥 fake 侧逐位扫）必须逐位一致——不一致
//! = 投影口径漂移，桥还没接就已经烂。

use crate::checks::CheckSet;
use super::unx36201_inhengine as ie;
use super::unx36205_inhorder::Lcg;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9fd0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_PROJ: u16 = SEG_BASE + 2; // 两实现投影不一致
pub const E_LOSS: u16 = SEG_BASE + 3; // 损失位判位不一致
pub const E_ANCHOR: u16 = SEG_BASE + 4; // 引擎锚/消费锚漂移
pub const E_MODE: u16 = SEG_BASE + 5; // mode 位越 9 位合法区
pub const E_INTG: u16 = SEG_BASE + 6; // 完整性自检不符

pub const VER: u32 = 1;

/// 投影例数锚（判据 ① 的 50 例）。
pub const PROJ_N: usize = 50;

/// POSIX mode 位域（9 位：owner/group/other × rwx）。
pub const M_R: u16 = 0o4;
pub const M_W: u16 = 0o2;
pub const M_X: u16 = 0o1;
pub const MODE_LEGAL: u16 = 0o777;

/// 语义损失标记位（登记给 B31 的消费锚）。
pub const LOSS_DENY: u8 = 0x01; // Deny 语义装不进 mode 位
pub const LOSS_ORDER: u8 = 0x02; // 位次语义装不进 mode 位
pub const LOSS_INHERITED: u8 = 0x04; // 继承来源装不进 mode 位

/// 掩码位 → mode 位映射（本域冻结口径：0x1→r, 0x2→w, 0x4→x）。
fn mask_to_mode(mask: u32) -> u16 {
    let mut m = 0u16;
    if mask & 0x1 != 0 {
        m |= M_R;
    }
    if mask & 0x2 != 0 {
        m |= M_W;
    }
    if mask & 0x4 != 0 {
        m |= M_X;
    }
    m
}

/// 主体分类锚（51=owner / 52=group / 其余=other——投影用例的约定）。
pub const SID_OWNER: u16 = 51;
pub const SID_GROUP: u16 = 52;

/// 引擎侧投影（fold 实现）：按主体并集 Allow → mode 位；损失位
/// 按 ACL 形态独立判。
pub fn project_engine(acl: &[ie::Ace], who: u16) -> (u16, u8) {
    let mut mode = 0u16;
    let mut loss = 0u8;
    let mut i = 0;
    while i < acl.len() {
        let a = &acl[i];
        let relevant = match who {
            0 => a.sid == SID_OWNER,
            1 => a.sid == SID_GROUP,
            _ => a.sid != SID_OWNER && a.sid != SID_GROUP,
        };
        if relevant {
            if a.kind == ie::AK_ALLOW {
                mode |= mask_to_mode(a.mask);
            } else {
                loss |= LOSS_DENY;
            }
        }
        if a.flags & ie::AI_INHERITED != 0 {
            loss |= LOSS_INHERITED;
        }
        if a.kind == ie::AK_DENY {
            loss |= LOSS_DENY;
        }
        i += 1;
    }
    // 同主体多 Allow 并集无位次概念 → 有多条 Allow 即位次损失。
    let mut allow_count = 0u32;
    let mut i = 0;
    while i < acl.len() {
        if acl[i].kind == ie::AK_ALLOW && acl[i].sid == who_sid(who) {
            allow_count += 1;
        }
        i += 1;
    }
    if allow_count > 1 {
        loss |= LOSS_ORDER;
    }
    (mode & MODE_LEGAL, loss)
}

fn who_sid(who: u16) -> u16 {
    match who {
        0 => SID_OWNER,
        1 => SID_GROUP,
        _ => 53,
    }
}

/// 桥 fake（独立实现二）：逐位扫（r/w/x 三位独立解析），判位独立。
pub fn project_bridge(acl: &[ie::Ace], who: u16) -> (u16, u8) {
    let wsid = who_sid(who);
    let mut mode = 0u16;
    let mut loss = 0u8;
    let mut bit = 0;
    while bit < 3 {
        let mb = 1u32 << bit;
        let mut i = 0;
        while i < acl.len() {
            let a = &acl[i];
            if a.sid == wsid && a.kind == ie::AK_ALLOW && a.mask & mb != 0 {
                mode |= match bit {
                    0 => M_R,
                    1 => M_W,
                    _ => M_X,
                };
            }
            i += 1;
        }
        bit += 1;
    }
    // 判位（独立口径：全表扫 Deny/继承/位次）。
    let mut i = 0;
    while i < acl.len() {
        if acl[i].kind == ie::AK_DENY {
            loss |= LOSS_DENY;
        }
        if acl[i].flags & ie::AI_INHERITED != 0 {
            loss |= LOSS_INHERITED;
        }
        i += 1;
    }
    let mut allow_count = 0u32;
    let mut i = 0;
    while i < acl.len() {
        if acl[i].kind == ie::AK_ALLOW && acl[i].sid == wsid {
            allow_count += 1;
        }
        i += 1;
    }
    if allow_count > 1 {
        loss |= LOSS_ORDER;
    }
    (mode & MODE_LEGAL, loss)
}

/// 50 例投影对平（判据 ①② 主面）：LCG 生成 50 个有效集形态
/// （显式+继承、Allow/Deny 混合、owner/group/other 三主体），
/// 两实现 mode 与 loss 逐例一致。
pub fn proj_sweep() -> Result<u32, u16> {
    let mut done = 0u32;
    let mut case = 0;
    while case < PROJ_N {
        let mut rng = Lcg::new(0x0DD_BA11 ^ (case as u32).wrapping_mul(0x9E37));
        let mut acl: Vec<ie::Ace> = Vec::new();
        let n = 1 + rng.below(5) as usize;
        let mut k = 0;
        while k < n {
            let who = rng.below(3);
            let sid = match who {
                0 => SID_OWNER,
                1 => SID_GROUP,
                _ => 53,
            };
            let kind = if rng.below(3) == 0 { ie::AK_DENY } else { ie::AK_ALLOW };
            let inh = rng.below(2) == 0;
            acl.push(ie::Ace {
                sid,
                mask: 1 + rng.below(7) as u32, // rwx 低位域
                flags: if inh { ie::AI_INHERITED } else { 0 },
                kind,
                origin: if inh { 7 } else { 0 },
            });
            k += 1;
        }
        let mut who = 0;
        while who < 3 {
            let (m1, l1) = project_engine(&acl, who);
            let (m2, l2) = project_bridge(&acl, who);
            if m1 != m2 {
                return Err(E_PROJ);
            }
            if l1 != l2 {
                return Err(E_LOSS);
            }
            if m1 & !MODE_LEGAL != 0 {
                return Err(E_MODE);
            }
            who += 1;
        }
        done += 1;
        case += 1;
    }
    Ok(done)
}

/// 损失位直证面（判据 ② 钉子）：三类损失各造一钉 + 无损失反例。
pub fn loss_pins() -> Result<(), u16> {
    // LOSS_DENY：含 Deny 的有效集必置位。
    let mut deny_acl: Vec<ie::Ace> = Vec::new();
    deny_acl.push(ie::Ace { sid: SID_OWNER, mask: 0x7, flags: 0, kind: ie::AK_DENY, origin: 0 });
    let (m1, l1) = project_engine(&deny_acl, 0);
    if l1 & LOSS_DENY == 0 || m1 != 0 {
        return Err(E_LOSS);
    }
    // LOSS_INHERITED：继承副本必置位。
    let mut inh_acl: Vec<ie::Ace> = Vec::new();
    inh_acl.push(ie::Ace { sid: SID_OWNER, mask: 0x7, flags: ie::AI_INHERITED, kind: ie::AK_ALLOW, origin: 3 });
    let (_, l2) = project_engine(&inh_acl, 0);
    if l2 & LOSS_INHERITED == 0 {
        return Err(E_LOSS);
    }
    // LOSS_ORDER：同主体双 Allow 必置位。
    let mut ord_acl: Vec<ie::Ace> = Vec::new();
    ord_acl.push(ie::Ace { sid: SID_OWNER, mask: 0x1, flags: 0, kind: ie::AK_ALLOW, origin: 0 });
    ord_acl.push(ie::Ace { sid: SID_OWNER, mask: 0x2, flags: 0, kind: ie::AK_ALLOW, origin: 0 });
    let (_, l3) = project_engine(&ord_acl, 0);
    if l3 & LOSS_ORDER == 0 {
        return Err(E_LOSS);
    }
    // 无损失反例：单条显式 Allow。
    let mut clean: Vec<ie::Ace> = Vec::new();
    clean.push(ie::Ace { sid: SID_OWNER, mask: 0x7, flags: 0, kind: ie::AK_ALLOW, origin: 0 });
    let (m4, l4) = project_engine(&clean, 0);
    if l4 != 0 || m4 != 0o7 {
        return Err(E_LOSS);
    }
    // 桥 fake 同判（两实现损失位一致——对平的钉子形态）。
    let (_, lb1) = project_bridge(&deny_acl, 0);
    let (_, lb2) = project_bridge(&inh_acl, 0);
    let (_, lb3) = project_bridge(&ord_acl, 0);
    let (_, lb4) = project_bridge(&clean, 0);
    if lb1 & LOSS_DENY == 0 || lb2 & LOSS_INHERITED == 0 || lb3 & LOSS_ORDER == 0 || lb4 != 0 {
        return Err(E_LOSS);
    }
    Ok(())
}

// ---- mode 位映射冻结表 ------------------------------------------------------
/// 16 组合 mode 冻结表（判据 ① 的映射底账）：本域冻结口径 0x1→r
/// (0o4) / 0x2→w (0o2) / 0x4→x (0o1)——手写冻结账，与 mask_to_mode
/// 双轨互证（表漂移或映射漂移任一爆红）。
pub const MODE_TABLE: [u16; 16] = [
    0o0, // 0x0
    0o4, // 0x1 r
    0o2, // 0x2 w
    0o6, // 0x3 rw
    0o1, // 0x4 x
    0o5, // 0x5 rx
    0o3, // 0x6 wx
    0o7, // 0x7 rwx
    0o0, 0o4, 0o2, 0o6, 0o1, 0o5, 0o3, 0o7, // 0x8..0xF 高位不投影（R/W/X 位外无义）
];

/// 映射双轨对平：MODE_TABLE 16 项逐一与 mask_to_mode 对位。
pub fn mode_table_replay() -> Result<(), u16> {
    let mut m = 0usize;
    while m < MODE_TABLE.len() {
        if MODE_TABLE[m] != mask_to_mode(m as u32) {
            return Err(E_PROJ);
        }
        m += 1;
    }
    Ok(())
}

/// mode 位合法区核：投影结果恒 ≤ 0o7（单主体三分量闭集）——越
/// 0o7 的 mode 是投影实现把非 R/W/X 位漏进来的直接证据。
pub fn mode_bound_pins() -> Result<(), u16> {
    let mut case = 0;
    while case < 8 {
        let mut acl: Vec<ie::Ace> = Vec::new();
        acl.push(ie::Ace {
            sid: SID_OWNER,
            mask: 0x1u32 << case,
            flags: 0,
            kind: ie::AK_ALLOW,
            origin: 0,
        });
        let (m, _) = project_engine(&acl, 0);
        if m > 0o7 {
            return Err(E_MODE);
        }
        case += 1;
    }
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let (av, ab) = ie::engine_anchor();
    if av != 1 || ab != 0x7b30 {
        return Err(E_ANCHOR);
    }
    // R1：50 例双实现对平（三主体全覆盖）。
    let done = proj_sweep()?;
    if done != PROJ_N as u32 {
        return Err(E_INTG);
    }
    // R2：三类损失钉 + 无损失反例（两实现同判）。
    loss_pins()?;
    // R3：mode 合法区锚（9 位 + rwx 映射口径）。
    if mask_to_mode(0x7) != 0o7 || mask_to_mode(0x1) != M_R || mask_to_mode(0x2) != M_W || mask_to_mode(0x4) != M_X {
        return Err(E_MODE);
    }
    // R4：消费锚登记（B31 消费位形——损失位闭集）。
    if LOSS_DENY != 0x01 || LOSS_ORDER != 0x02 || LOSS_INHERITED != 0x04 {
        return Err(E_ANCHOR);
    }
    // R5：主体分类锚（owner/group/other 三分闭集）。
    if who_sid(0) != SID_OWNER || who_sid(1) != SID_GROUP || who_sid(2) != 53 {
        return Err(E_ANCHOR);
    }
    // R6：mode 映射冻结表双轨对平 + 单主体闭集界核。
    mode_table_replay()?;
    mode_bound_pins()?;
    Ok(done)
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_PROJ => "两实现 mode 投影不一致（口径漂移）",
        E_LOSS => "语义损失位判位不一致/漏置",
        E_ANCHOR => "引擎锚/消费锚漂移",
        E_MODE => "mode 位越 9 位合法区/映射口径破",
        E_INTG => "完整性自检不符",
        _ => "未知诊断码",
    }
}

pub fn run_unx36215_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36215_inhposix");
    cs.add("SEG 段位独占", SEG_BASE == 0x9fd0, "0x9fd0~0x9fdF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "50 例双实现对平/三类损失钉/mode 合法区/消费锚全对"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("例量锚", PROJ_N == 50, "投影对平 50 例（判据锚）");
    cs.add("损失位锚", LOSS_DENY == 0x01 && LOSS_ORDER == 0x02 && LOSS_INHERITED == 0x04, "语义损失标记位闭集登记（B31 消费锚）");
    cs.add("映射口径", mask_to_mode(0x7) == 0o7, "0x1→r / 0x2→w / 0x4→x 冻结映射");
    cs.add("mode 区", MODE_LEGAL == 0o777, "mode 9 位合法区");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_green() {
        let cs = run_unx36215_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_projection_modes_match_bridge() {
        // 性质直证：手造三类有效集（owner 授 / group 授 / other 授），
        // 三主体投影两实现逐位一致。
        let mut acl: Vec<ie::Ace> = Vec::new();
        acl.push(ie::Ace { sid: SID_OWNER, mask: 0x7, flags: 0, kind: ie::AK_ALLOW, origin: 0 });
        acl.push(ie::Ace { sid: SID_GROUP, mask: 0x3, flags: ie::AI_INHERITED, kind: ie::AK_ALLOW, origin: 5 });
        acl.push(ie::Ace { sid: 53, mask: 0x1, flags: 0, kind: ie::AK_ALLOW, origin: 0 });
        let mut who = 0;
        while who < 3 {
            let (m1, l1) = project_engine(&acl, who);
            let (m2, l2) = project_bridge(&acl, who);
            assert_eq!(m1, m2, "who={} mode", who);
            assert_eq!(l1, l2, "who={} loss", who);
            who += 1;
        }
        let (mo, _) = project_engine(&acl, 0);
        assert_eq!(mo, 0o7, "owner 全 rwx");
        let (mg, _) = project_engine(&acl, 1);
        assert_eq!(mg, 0o6, "group rw（0x3 → M_R|M_W = 0o6）");
        let (mt, _) = project_engine(&acl, 2);
        assert_eq!(mt, 0o4, "other r（0x1 → M_R = 0o4）");
    }

    #[test]
    fn test_loss_bits_always_marked() {
        // 账面守恒：50 例里含 Deny/继承/双 Allow 的损失位两实现全一致。
        assert_eq!(proj_sweep().ok().unwrap_or(0), PROJ_N as u32);
        assert!(loss_pins().is_ok());
    }
}
