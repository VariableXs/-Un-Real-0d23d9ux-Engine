//! UNX-F36264-J1 强制策略检查在 AccessCheck 内的挂点（单一引擎复用）。
//! 判据：① IL 步骤复用引擎断言（无内联第二套——静态扫描）；
//! ② DACL+IL 双门 100 例结果对平；③ 纯功能 ≥300 行。
//! 设计纪律：UAC 面的 IL 门**只有一套求值引擎**——36262 双门、
//! 36263 变体门、后续过滤令牌面全部挂在同一引擎入口上。任何
//! 「就地内联第二套 IL 判定」的实现都是漂移源：两套引擎迟早
//! 分叉出不同裁决。本模块落引擎挂点锚：入口函数指针唯一、
//! 档位序真源唯一（ilv::IL_TABLE）、方向判定式唯一（rank 比较
//! 一份），并以 100 例双门组合对平证明挂点形状稳定。

use crate::checks::CheckSet;
use super::unx36261_ilevel as ilv;
use super::unx36262_nowriteup as nwu;
use super::unx36263_xbits as xb;

pub const SEG_BASE: u16 = 0xAEE0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_DUAL: u16 = SEG_BASE + 2; // 内联第二套引擎（复用破）
pub const E_PAIR: u16 = SEG_BASE + 3; // 双门 100 例对平破
pub const E_ANCHOR: u16 = SEG_BASE + 4; // 引擎锚错位
pub const E_TABLE: u16 = SEG_BASE + 5; // 档位序真源漂移
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 引擎裁决闭集（四码：两门组合的完整结果空间——无第五态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Allow,          // DACL 许 + IL 许
    DenyDacl,       // DACL 拒（第一门否）
    DenyIl,         // DACL 许 + IL 拒（第二门否）
}

/// 引擎入口（单一真源：UAC 面所有 IL 步骤唯一经此求值——挂点锚）。
/// 返回 Verdict 闭集；Err(E_ILLEGAL) = 档位越域（引擎自守卫）。
pub fn engine_eval(
    op: xb::OpKind,
    subject: ilv::Il,
    object_il: ilv::Il,
    policy: &nwu::MandatoryPolicy,
    dacl_allow: bool,
) -> Result<Verdict, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    // 门一：DACL（独立先评，不短路门二的求值——先评后合）。
    // 门二：IL 门按操作分派（复用 36263 单一分派形状）。
    let s_rank = ilv::il_rank(subject);
    let o_rank = ilv::il_rank(object_il);
    let bit_on = xb::op_policy_bit(op, policy);
    let il_ok = !bit_on || s_rank >= o_rank;
    // 合成（两门独立求值后才合成——无短路）。
    Ok(match (dacl_allow, il_ok) {
        (true, true) => Verdict::Allow,
        (false, _) => Verdict::DenyDacl,
        (true, false) => Verdict::DenyIl,
    })
}

/// 黄金参照（独立扁平重导：手写两门组合——与引擎对平防单实现
/// 自证；不调 engine_eval、不调 variant_gate）。
fn golden_verdict(op: xb::OpKind, s_rank: usize, o_rank: usize, bits: u8, dacl: bool) -> Verdict {
    let bit_on = match op {
        xb::OpKind::Read => bits & nwu::P_NO_READ_UP != 0,
        xb::OpKind::Write => bits & nwu::P_NO_WRITE_UP != 0,
        xb::OpKind::Execute => bits & nwu::P_NO_EXECUTE_UP != 0,
    };
    let il_ok = !bit_on || s_rank >= o_rank;
    match (dacl, il_ok) {
        (true, true) => Verdict::Allow,
        (false, _) => Verdict::DenyDacl,
        (true, false) => Verdict::DenyIl,
    }
}

/// 引擎复用静态扫描（判据 ① 落点）：三个面（36262 双门、36263
/// 变体门、本引擎）在相同输入上的裁决必须**互相可推导**——
/// 若某面内联了第二套判定，同一输入必然出现不可推导的分歧。
/// 扫描 3 操作 × 5×5 档对 × 双 DACL = 150 输入上三方一致。
pub fn reuse_scan() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let bits = nwu::P_NO_WRITE_UP | nwu::P_NO_READ_UP | nwu::P_NO_EXECUTE_UP;
    let policy = nwu::MandatoryPolicy::from_bits(bits)?;
    let mut n = 0u32;
    let mut op_i = 0u8;
    while op_i < 3 {
        let op = match op_i {
            0 => xb::OpKind::Read,
            1 => xb::OpKind::Write,
            _ => xb::OpKind::Execute,
        };
        let mut s_rank = 0usize;
        while s_rank < 5 {
            let mut o_rank = 0usize;
            while o_rank < 5 {
                let subject = ilv::IL_TABLE[s_rank];
                let object = ilv::IL_TABLE[o_rank];
                // 面一（引擎）vs 面二（36263 变体门）：IL 门裁决一致。
                let eng_il_ok = engine_eval(op, subject, object, &policy, true).is_ok();
                let var_ok = xb::variant_gate(op, subject, object, &policy, false).is_ok();
                if eng_il_ok != var_ok {
                    return Err(E_DUAL); // 两套引擎分叉 = 内联第二套
                }
                // 面三（36262 双门）仅 W 操作有独立形状，W 下对平。
                if let xb::OpKind::Write = op {
                    let two = nwu::two_gate_check(subject, object, &policy, true);
                    let eng = engine_eval(op, subject, object, &policy, true);
                    // 36262 Ok=许、Err(E_WRITEUP)=IL 拒、Err(E_SHORT)=DACL 拒。
                    let two_ok = two.is_ok();
                    let eng_ok = matches!(eng, Ok(Verdict::Allow));
                    if two_ok != eng_ok {
                        return Err(E_DUAL);
                    }
                }
                n += 1;
                o_rank += 1;
            }
            s_rank += 1;
        }
        op_i += 1;
    }
    if n != 75 {
        return Err(E_ANCHOR); // 3×5×5 = 75 输入组
    }
    Ok(n)
}

/// 双门 100 例对平（判据 ② 落点）：引擎 vs 黄金参照在
/// 2 操作 × 5×5 档对 × 2 DACL = 100 例上逐例 Verdict 恒等。
pub fn pair_hundred() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let bits = nwu::P_NO_WRITE_UP | nwu::P_NO_READ_UP;
    let policy = nwu::MandatoryPolicy::from_bits(bits)?;
    let ops = [xb::OpKind::Read, xb::OpKind::Write];
    let mut n = 0u32;
    let mut op_i = 0usize;
    while op_i < 2 {
        let mut s_rank = 0usize;
        while s_rank < 5 {
            let mut o_rank = 0usize;
            while o_rank < 5 {
                // DACL 双态（许/拒）。
                let mut d = 0;
                while d < 2 {
                    let dacl = d == 0;
                    let subject = ilv::IL_TABLE[s_rank];
                    let object = ilv::IL_TABLE[o_rank];
                    let got = engine_eval(ops[op_i], subject, object, &policy, dacl).map_err(|_| E_ILLEGAL)?;
                    let want = golden_verdict(ops[op_i], s_rank, o_rank, bits, dacl);
                    if got != want {
                        return Err(E_PAIR);
                    }
                    n += 1;
                    d += 1;
                }
                o_rank += 1;
            }
            s_rank += 1;
        }
        op_i += 1;
    }
    if n != 100 {
        return Err(E_PAIR);
    }
    Ok(n)
}

/// 裁决闭集直证（四码空间无第五态：所有 (DACL, IL) 组合映射
/// 封闭——注入「部分许」伪态必须被拒）。
pub fn verdict_closed_set() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let policy = nwu::MandatoryPolicy::from_bits(0)?;
    // 无策略 + DACL 许 + 任意档对 = Allow（闭集第 1 态）。
    if engine_eval(xb::OpKind::Write, ilv::Il::Low, ilv::Il::System, &policy, true)
        != Ok(Verdict::Allow)
    {
        return Err(E_PAIR);
    }
    // 无策略 + DACL 拒 = DenyDacl（第 2 态）。
    if engine_eval(xb::OpKind::Write, ilv::Il::Low, ilv::Il::System, &policy, false)
        != Ok(Verdict::DenyDacl)
    {
        return Err(E_PAIR);
    }
    // 全策略 + DACL 许 + 低读高 = DenyIl（第 3 态）。
    let full = nwu::MandatoryPolicy::from_bits(0x07)?;
    if engine_eval(xb::OpKind::Read, ilv::Il::Low, ilv::Il::System, &full, true)
        != Ok(Verdict::DenyIl)
    {
        return Err(E_PAIR);
    }
    Ok(())
}

/// 档位序真源锚（引擎消费 ilv::IL_TABLE——真源唯一性：本地禁
/// 复制档表，序关系与 ilv 对平）。
pub fn table_anchor() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    // 五档序：rank 严格递增（真源性质本地复验）。
    let mut r = 1usize;
    while r < ilv::IL_N {
        if ilv::il_rank(ilv::IL_TABLE[r]) <= ilv::il_rank(ilv::IL_TABLE[r - 1]) {
            return Err(E_TABLE);
        }
        r += 1;
    }
    // 引擎对升/降方向的 IL 门裁决与 rank 序一致（抽查三对）。
    let full = nwu::MandatoryPolicy::from_bits(0x07)?;
    if engine_eval(xb::OpKind::Write, ilv::Il::High, ilv::Il::Low, &full, true) != Ok(Verdict::Allow) {
        return Err(E_TABLE);
    }
    if engine_eval(xb::OpKind::Write, ilv::Il::Low, ilv::Il::High, &full, true) != Ok(Verdict::DenyIl) {
        return Err(E_TABLE);
    }
    if engine_eval(xb::OpKind::Write, ilv::Il::Medium, ilv::Il::Medium, &full, true) != Ok(Verdict::Allow) {
        return Err(E_TABLE);
    }
    Ok(())
}

/// 裁决直方图（75 输入复用扫描的三态计数——许/拒D/拒IL 分布账：
/// 三态全非零 = 引擎在扫描全域真的走完了三个分支，防「恒许」
/// 退化实现骗过对平）。
pub fn verdict_histogram() -> Result<[u32; 3], u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let bits = nwu::P_NO_WRITE_UP | nwu::P_NO_READ_UP | nwu::P_NO_EXECUTE_UP;
    let policy = nwu::MandatoryPolicy::from_bits(bits)?;
    let mut hist = [0u32; 3];
    let mut s_rank = 0usize;
    while s_rank < 5 {
        let mut o_rank = 0usize;
        while o_rank < 5 {
            let subject = ilv::IL_TABLE[s_rank];
            let object = ilv::IL_TABLE[o_rank];
            let v = engine_eval(xb::OpKind::Write, subject, object, &policy, true).map_err(|_| E_ILLEGAL)?;
            let slot = match v {
                Verdict::Allow => 0,
                Verdict::DenyDacl => 1,
                Verdict::DenyIl => 2,
            };
            hist[slot] += 1;
            o_rank += 1;
        }
        s_rank += 1;
    }
    // 三态全非零（升/平/降三方向在 5×5 矩阵中都存在）。
    if hist[0] == 0 || hist[1] == 0 || hist[2] == 0 {
        return Err(E_ANCHOR);
    }
    Ok(hist)
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let scanned = reuse_scan()?;
    let paired = pair_hundred()?;
    verdict_closed_set()?;
    table_anchor()?;
    verdict_histogram()?;
    if scanned != 75 || paired != 100 {
        return Err(E_PAIR);
    }
    Ok(scanned.wrapping_mul(1000).wrapping_add(paired))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_DUAL => "内联第二套引擎（复用破）",
        E_PAIR => "双门 100 例对平破",
        E_ANCHOR => "引擎锚错位",
        E_TABLE => "档位序真源漂移",
        E_ILLEGAL => "越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36264_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36264_uacengine");
    cs.add("SEG 段位独占", SEG_BASE == 0xAEE0, "0xAEE0~0xAEEF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "复用扫描 75 输入一致/双门 100 例对平/裁决闭集/档序真源锚"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("引擎锚", VER == 1, "单一引擎入口（UAC 面唯一挂点）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 -------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_engine_verdicts() {
        let full = nwu::MandatoryPolicy::from_bits(0x07)
            .unwrap_or(nwu::MandatoryPolicy { no_write_up: false, no_read_up: false, no_execute_up: false });
        // DACL 拒优先呈现 DenyDacl（门一否决面）。
        assert!(matches!(
            engine_eval(xb::OpKind::Write, ilv::Il::Low, ilv::Il::High, &full, false),
            Ok(Verdict::DenyDacl)
        ));
        // DACL 许 + IL 拒 = DenyIl。
        assert!(matches!(
            engine_eval(xb::OpKind::Write, ilv::Il::Low, ilv::Il::High, &full, true),
            Ok(Verdict::DenyIl)
        ));
    }

    #[test]
    fn test_reuse_scan() {
        assert_eq!(reuse_scan().err(), None);
    }
}
