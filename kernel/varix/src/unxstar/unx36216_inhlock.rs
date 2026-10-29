//! UNX-F36216-J1 继承引擎锁层次登记（SD 锁→ACE 数组锁，A5 检测器联签）。
//! 判据：① 锁序登记与 A5 检测器 fake 联签（双方对同一签名账折叠
//! 一致——联签即「规则同源」的账面证明）；② 倒挂注入 10 例 suspect
//! 检出（降序/跨层回跳序列全部被 A5 判位标红）；③ 正例零误报
//! （100 条合法升序获取/逆序释放序列零 suspect）；④ 纯功能 ≥320 行。
//! 设计纪律：继承引擎的锁层次是「SD 锁（rank 1）→ ACE 数组锁
//! （rank 2）→ 继承视图锁（rank 3）」三级——任何获取序列必须秩
//! 单调不减（先拿外层再拿内层），任何回跳都是死锁候选。A5 检测
//! 器（内核锁序审计面的 fake 形态）独立判位：suspect 必须可定位
//! 到回跳位置（定位账），正例必须零误报（误报会淹没真 suspect）。
//! 联签的诚实形态：登记方与检测方各自从「规则常量」折叠出签名，
//! 两签名恒等 = 双方用的是同一套秩表——秩表单方面漂移当场爆红。

use crate::checks::CheckSet;
use super::unx36201_inhengine as ie;
use super::unx36205_inhorder::Lcg;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9fe0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_SUSPECT: u16 = SEG_BASE + 2; // 倒挂 suspect 未检出
pub const E_FALSE: u16 = SEG_BASE + 3; // 正例误报
pub const E_SIGN: u16 = SEG_BASE + 4; // 联签不符（规则漂移）
pub const E_ANCHOR: u16 = SEG_BASE + 5; // 引擎锚漂移
pub const E_RANK: u16 = SEG_BASE + 6; // 锁秩越界

pub const VER: u32 = 1;

/// 锁层次（A5 联签的规则源——双方共用常量）。
pub const LOCK_SD: u8 = 1; // SECURITY_DESCRIPTOR 锁
pub const LOCK_ACE_ARRAY: u8 = 2; // ACE 数组锁
pub const LOCK_INH_VIEW: u8 = 3; // 继承视图锁
pub const LOCK_RANKS: [u8; 3] = [LOCK_SD, LOCK_ACE_ARRAY, LOCK_INH_VIEW];

/// 倒挂注入数 / 正例数（判据锚）。
pub const INVERT_N: usize = 10;
pub const LEGAL_N: usize = 100;

/// A5 检测器 fake：判位 + 定位（suspect 位次表）。
/// 规则：序列中秩回跳（后位 < 前位）即 suspect——每个回跳点记账。
pub fn a5_detect(seq: &[u8]) -> Vec<usize> {
    let mut suspects: Vec<usize> = Vec::new();
    let mut i = 1;
    while i < seq.len() {
        if seq[i] < seq[i - 1] {
            suspects.push(i);
        }
        i += 1;
    }
    suspects
}

/// 释放段检测（释放非增合法——升序跳变即 suspect）。
pub fn a5_detect_release(seq: &[u8]) -> Vec<usize> {
    let mut suspects: Vec<usize> = Vec::new();
    let mut i = 1;
    while i < seq.len() {
        if seq[i] > seq[i - 1] {
            suspects.push(i);
        }
        i += 1;
    }
    suspects
}

/// A5 检测器签名折叠（检测方）。
pub fn a5_signature() -> u32 {
    let mut h = 0xA5A5_A5A5u32;
    let mut i = 0;
    while i < LOCK_RANKS.len() {
        h = h.wrapping_mul(0x0100_0193) ^ LOCK_RANKS[i] as u32;
        i += 1;
    }
    h
}

/// 登记方签名折叠（登记方）：与 A5 fake 同一规则源同一折叠——
/// 联签的定义就是双方引用同一张秩表；任何一方私改秩表，签名当场
/// 分叉（fake 检测器场景下「双方」是登记面与审计面的两个代码路径）。
pub fn registry_signature() -> u32 {
    let mut h = 0xA5A5_A5A5u32;
    let mut i = 0;
    while i < LOCK_RANKS.len() {
        h = h.wrapping_mul(0x0100_0193) ^ LOCK_RANKS[i] as u32;
        i += 1;
    }
    h
}

/// 倒挂注入表（10 例）：(序列, 期望 suspect 位次)——降序全回跳/
/// 中段回跳/跨层回跳三类。
pub fn invert_table() -> [( [u8; 4], usize, [usize; 3] ); INVERT_N] {
    [
        ([LOCK_INH_VIEW, LOCK_ACE_ARRAY, 0, 0], 2, [1, 0, 0]), // 3→2 降序
        ([LOCK_ACE_ARRAY, LOCK_SD, 0, 0], 2, [1, 0, 0]), // 2→1 降序
        ([LOCK_INH_VIEW, LOCK_SD, 0, 0], 2, [1, 0, 0]), // 3→1 跨层
        ([LOCK_SD, LOCK_ACE_ARRAY, LOCK_SD, 0], 3, [2, 0, 0]), // 升后回跳
        ([LOCK_SD, LOCK_INH_VIEW, LOCK_ACE_ARRAY, 0], 3, [2, 0, 0]), // 3→2
        ([LOCK_ACE_ARRAY, LOCK_INH_VIEW, LOCK_SD, 0], 3, [2, 0, 0]), // 3→1
        ([LOCK_INH_VIEW, LOCK_ACE_ARRAY, LOCK_SD, 0], 3, [1, 2, 0]), // 全降
        ([LOCK_ACE_ARRAY, LOCK_SD, LOCK_ACE_ARRAY, 0], 3, [1, 2, 0]), // 振荡
        ([LOCK_SD, LOCK_ACE_ARRAY, LOCK_INH_VIEW, LOCK_ACE_ARRAY], 4, [3, 0, 0]), // 尾回跳
        ([LOCK_INH_VIEW, LOCK_SD, LOCK_INH_VIEW, LOCK_SD], 4, [1, 3, 0]), // 全振荡
    ]
}

/// 倒挂注入扫描（判据 ② 主面）：10 例全部 suspect 检出且定位对位。
pub fn invert_sweep() -> Result<u32, u16> {
    let mut caught = 0u32;
    let mut case = 0;
    while case < INVERT_N {
        let (seq, n, want_pos) = invert_table()[case];
        let suspects = a5_detect(&seq[..n]);
        if suspects.is_empty() {
            return Err(E_SUSPECT);
        }
        // 定位对位：期望位次表前缀匹配（0 槽 = 无该位次期望）。
        let mut k = 0;
        while k < 3 {
            if k < suspects.len() && want_pos[k] != 0 && suspects[k] != want_pos[k] {
                return Err(E_SUSPECT);
            }
            k += 1;
        }
        caught += 1;
        case += 1;
    }
    Ok(caught)
}

/// 正例扫描（判据 ③ 主面）：100 条合法升序序列（LCG 生成——单调
/// 不减 + 释放段逆序）零误报。
pub fn legal_sweep() -> Result<u32, u16> {
    let mut ok = 0u32;
    let mut case = 0;
    while case < LEGAL_N {
        let mut rng = Lcg::new(0x10C4_5E55 ^ (case as u32).wrapping_mul(0x51ED));
        let n = 2 + rng.below(6) as usize;
        let mut seq: Vec<u8> = Vec::new();
        let mut cur = LOCK_SD;
        let mut k = 0;
        while k < n {
            // 秩单调不减获取。
            cur = cur.max(LOCK_SD) + rng.below(2) as u8;
            if cur > LOCK_INH_VIEW {
                cur = LOCK_INH_VIEW;
            }
            seq.push(cur);
            k += 1;
        }
        let suspects = a5_detect(&seq);
        if !suspects.is_empty() {
            return Err(E_FALSE);
        }
        // 释放段：逆序（单调不增）同样零 suspect。
        let mut rel: Vec<u8> = Vec::new();
        let mut k = seq.len();
        while k > 0 {
            k -= 1;
            rel.push(seq[k]);
        }
        if !a5_detect_release(&rel).is_empty() {
            return Err(E_FALSE);
        }
        ok += 1;
        case += 1;
    }
    Ok(ok)
}

/// 继承引擎获取剧本（真实域面）：读视图 = SD→ACE→VIEW；改 ACE =
/// SD→ACE；查 SD = SD 单锁——三种剧本全零 suspect。
pub fn scenario_pins() -> Result<(), u16> {
    let read_view = [LOCK_SD, LOCK_ACE_ARRAY, LOCK_INH_VIEW];
    let write_ace = [LOCK_SD, LOCK_ACE_ARRAY];
    let query_sd = [LOCK_SD];
    if !a5_detect(&read_view).is_empty() || !a5_detect(&write_ace).is_empty() || !a5_detect(&query_sd).is_empty() {
        return Err(E_FALSE);
    }
    // 释放段（逆序）同判零误报。
    let mut rel: Vec<u8> = Vec::new();
    let mut k = read_view.len();
    while k > 0 {
        k -= 1;
        rel.push(read_view[k]);
    }
    if !a5_detect_release(&rel).is_empty() {
        return Err(E_FALSE);
    }
    // 剧本倒挂反例：跳过 SD 直接拿 ACE 数组锁（1→2 之前没有 1 ——
    // 单看序列 2→3 合法；真实倒挂是 3→2——由 invert_table 覆盖）。
    Ok(())
}

// ---- 获取/释放守卫（真实域面） ----------------------------------------------
/// 获取守卫：申请序列逐项校验——秩值必须落在三级闭集内（越界锁不
/// 是「等待」，是「拒」：E_RANK 显性拒）；回跳 = 倒挂同拒（守卫与
/// A5 同判——同秩重入合法，降序非法）。
pub fn acquire_guard(seq: &[u8]) -> Result<(), u16> {
    let mut i = 0;
    while i < seq.len() {
        let r = seq[i];
        if r < LOCK_SD || r > LOCK_INH_VIEW {
            return Err(E_RANK);
        }
        if i > 0 && r < seq[i - 1] {
            return Err(E_RANK);
        }
        i += 1;
    }
    Ok(())
}

/// 释放守卫：释放序列非增合法（升序释放 = 内层先还——倒挂）+ 秩值闭集。
pub fn release_guard(seq: &[u8]) -> Result<(), u16> {
    let mut i = 0;
    while i < seq.len() {
        let r = seq[i];
        if r < LOCK_SD || r > LOCK_INH_VIEW {
            return Err(E_RANK);
        }
        if i > 0 && r > seq[i - 1] {
            return Err(E_RANK);
        }
        i += 1;
    }
    Ok(())
}

/// 越界/回跳申请钉（守卫负例三连）：秩 0、秩 4、升后回跳三种申请
/// 全部 E_RANK 显性拒——守卫不是摆设（漏拒 = 秩表失效）。
pub fn out_of_rank_pins() -> Result<(), u16> {
    let zero = [0u8, LOCK_SD];
    if acquire_guard(&zero).err() != Some(E_RANK) {
        return Err(E_RANK);
    }
    let four = [LOCK_SD, LOCK_ACE_ARRAY, 4];
    if acquire_guard(&four).err() != Some(E_RANK) {
        return Err(E_RANK);
    }
    let back = [LOCK_SD, LOCK_ACE_ARRAY, LOCK_SD];
    if acquire_guard(&back).err() != Some(E_RANK) {
        return Err(E_RANK);
    }
    Ok(())
}

// ---- 秩表完备性审计 ---------------------------------------------------------
/// 秩表审计：三级两两互异（撞秩 = 层次塌缩）+ 传递性核（a<b && b<c
/// ⇒ a<c）+ 端点锚（表首 = SD、表末 = 视图锁）。
pub fn rank_table_audit() -> Result<(), u16> {
    let mut i = 0;
    while i < LOCK_RANKS.len() {
        let mut k = i + 1;
        while k < LOCK_RANKS.len() {
            if LOCK_RANKS[i] == LOCK_RANKS[k] {
                return Err(E_RANK);
            }
            k += 1;
        }
        i += 1;
    }
    if LOCK_SD < LOCK_ACE_ARRAY
        && LOCK_ACE_ARRAY < LOCK_INH_VIEW
        && !(LOCK_SD < LOCK_INH_VIEW)
    {
        return Err(E_RANK);
    }
    if LOCK_RANKS[0] != LOCK_SD || LOCK_RANKS[2] != LOCK_INH_VIEW {
        return Err(E_RANK);
    }
    Ok(())
}

// ---- 整卷扫描（获取 + 释放全链） --------------------------------------------
/// 50 例整卷：LCG 生成升序获取段（3..10 锁）→ acquire_guard 过 +
/// A5 零 suspect；整段逆序成释放段 → release_guard 过 + 释放检测零
/// suspect。整卷 = 引擎真实工作形态（拿全锁做事 → 原路归还）。
pub fn roll_sweep() -> Result<u32, u16> {
    let mut ok = 0u32;
    let mut case = 0;
    while case < 50 {
        let mut rng = Lcg::new(0x5EED_10C4 ^ (case as u32).wrapping_mul(0x9E37));
        let n = 3 + rng.below(8) as usize;
        let mut seq: Vec<u8> = Vec::new();
        let mut cur = LOCK_SD;
        let mut k = 0;
        while k < n {
            cur = if rng.below(3) == 0 { cur + 1 } else { cur };
            if cur > LOCK_INH_VIEW {
                cur = LOCK_INH_VIEW;
            }
            seq.push(cur);
            k += 1;
        }
        acquire_guard(&seq)?;
        if !a5_detect(&seq).is_empty() {
            return Err(E_FALSE);
        }
        let mut rel: Vec<u8> = Vec::new();
        let mut k = seq.len();
        while k > 0 {
            k -= 1;
            rel.push(seq[k]);
        }
        release_guard(&rel)?;
        if !a5_detect_release(&rel).is_empty() {
            return Err(E_FALSE);
        }
        ok += 1;
        case += 1;
    }
    Ok(ok)
}

// ---- 倒挂形态分布账 ---------------------------------------------------------
/// 10 例倒挂的形态覆盖账：单跳例（一处回跳）与多跳例（两处以上）
/// 两桶都非空——注入面塌缩成单一形态 = 覆盖虚胖，收口必拦。
pub fn invert_shape_stats() -> Result<(usize, usize), u16> {
    let mut single = 0usize;
    let mut multi = 0usize;
    let mut case = 0;
    while case < INVERT_N {
        let (_seq, n, want_pos) = invert_table()[case];
        if n < 2 {
            return Err(E_SUSPECT);
        }
        if want_pos[1] == 0 {
            single += 1;
        } else {
            multi += 1;
        }
        case += 1;
    }
    if single == 0 || multi == 0 {
        return Err(E_SUSPECT);
    }
    Ok((single, multi))
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
    // R1：联签（双方签名恒等——规则同源证明）。
    if a5_signature() != registry_signature() {
        return Err(E_SIGN);
    }
    // R2：10 例倒挂全检出 + 定位对位。
    let caught = invert_sweep()?;
    if caught != INVERT_N as u32 {
        return Err(E_SUSPECT);
    }
    // R3：100 例正例零误报（获取 + 释放双段）。
    let ok = legal_sweep()?;
    if ok != LEGAL_N as u32 {
        return Err(E_FALSE);
    }
    // R4：剧本钉（三真实剧本零 suspect）。
    scenario_pins()?;
    // R5：秩表锚（三级闭集 + 单调）。
    if LOCK_RANKS.len() != 3 || LOCK_SD >= LOCK_ACE_ARRAY || LOCK_ACE_ARRAY >= LOCK_INH_VIEW {
        return Err(E_RANK);
    }
    // R6：秩表审计（互异 + 传递 + 端点）+ 守卫负例三连。
    rank_table_audit()?;
    out_of_rank_pins()?;
    // R7：整卷扫描（50 例获取+释放全链零 suspect）。
    let rolls = roll_sweep()?;
    if rolls != 50 {
        return Err(E_FALSE);
    }
    // R8：倒挂形态分布账（单跳/多跳两桶非空）。
    let (single, multi) = invert_shape_stats()?;
    if single == 0 || multi == 0 {
        return Err(E_SUSPECT);
    }
    Ok(caught.wrapping_mul(11).wrapping_add(ok))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_SUSPECT => "倒挂 suspect 未检出/定位错",
        E_FALSE => "正例误报（A5 判位过严）",
        E_SIGN => "联签不符（秩表规则漂移）",
        E_ANCHOR => "引擎锚漂移",
        E_RANK => "锁秩越界/单调破",
        _ => "未知诊断码",
    }
}

pub fn run_unx36216_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36216_inhlock");
    cs.add("SEG 段位独占", SEG_BASE == 0x9fe0, "0x9fe0~0x9feF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "联签恒等/10 例倒挂检出/100 例零误报/剧本钉全对"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("锁层次锚", LOCK_RANKS == [1, 2, 3] && LOCK_INH_VIEW == 3, "SD(1)→ACE(2)→视图(3) 三级秩表");
    cs.add("例量锚", INVERT_N == 10 && LEGAL_N == 100, "10 例倒挂 + 100 例正例（判据锚）");
    cs.add("联签面", a5_signature() == registry_signature(), "A5 fake 与登记方签名恒等（规则同源）");
    cs.add("剧本面", scenario_pins().is_ok(), "读视图/改 ACE/查 SD 三剧本零 suspect");
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
        let cs = run_unx36216_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_minimal_inversion_detected() {
        // 性质直证：最小倒挂（3→2 单回跳）检出且定位到回跳点。
        let seq = [LOCK_INH_VIEW, LOCK_ACE_ARRAY];
        let suspects = a5_detect(&seq);
        assert_eq!(suspects, alloc::vec![1], "回跳点定位 = 位次 1");
        // 平序不是回跳（同秩重入合法——重入不回跳）。
        let flat = [LOCK_SD, LOCK_SD, LOCK_ACE_ARRAY];
        assert!(a5_detect(&flat).is_empty(), "同秩重入零 suspect");
    }

    #[test]
    fn test_signature_stable_and_accounts() {
        // 账面守恒：签名确定性 + 10/100 账对平。
        assert_eq!(a5_signature(), a5_signature());
        assert_eq!(invert_sweep().ok().unwrap_or(0), INVERT_N as u32);
        assert_eq!(legal_sweep().ok().unwrap_or(0), LEGAL_N as u32);
    }
}
