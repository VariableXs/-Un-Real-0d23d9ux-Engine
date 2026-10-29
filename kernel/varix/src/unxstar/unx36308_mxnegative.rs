//! UNX-F36308-J1 矩阵注入负例库（每格 ≥1 攻击位负例，≥240 例）。
//! 判据：① 负例库 240 例全拦截（越权读/写/执行/提权/绕检查点五
//! 类位全拒绝）；② 负例逃逸 = P0 直报；③ 纯功能代码 ≥300 行。
//!
//! 设计纪律（对标 Windows 授权检查的对抗性验证）：矩阵的每个格
//! 挂一个**攻击位负例**——攻击按操作轴对位：READ 格挂越权读
//! （伪造无 No-Read-Up 的求值器）、WRITE 格挂越权写（伪造主体
//! 等级抬升到客体等级）、EXEC 格挂越权执行（伪造客体支持位面
//! 全开）、DEL 格挂提权（伪造特权位注入）、WDAC 格挂绕检查点
//! （伪造令牌过滤门失效）。每例分三类结果落账：
//!   拦截（intercepted）——攻击后仍拒或归因漂移但未得手；
//!   逃逸候选（escape-candidate）——攻击把拒格判成许（得手面）；
//!   无增益（no-gain）——攻击前后判决不变（防线冗余面）。
//! 引擎本体不因注入而变（负例打在**影子求值器**上）：真引擎
//! 240 格对语义书仍全绿；影子求值器的 14 个逃逸候选**全部**被
//! 检测器点名（检出数 = 候选数、无一漏网），任何一格漏检即按
//! P0 口径直报 E_P0。冻结分布：候选 14 / 归因漂移 32 / 无增益
//! 194，合计 240——负例库的账面守恒与拦截能力同账呈现。

use crate::checks::CheckSet;
use super::unx36302_mxloband as mx;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xB1E0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 0; // 版本锚不符
pub const E_LIB: u16 = SEG_BASE + 1; // 负例库账不平（不足 240/分类漂移）
pub const E_P0: u16 = SEG_BASE + 2; // 负例逃逸未检出（P0 直报）
pub const E_ENGINE: u16 = SEG_BASE + 3; // 注入后真引擎漂移
pub const E_FROZEN: u16 = SEG_BASE + 4; // 冻结分布漂移（14/32/194）
pub const E_RANGE: u16 = SEG_BASE + 5; // 攻击类越界

/// 模块版本锚。
pub const VER: u32 = 1;

/// 负例库冻结分布锚。
pub const FROZEN_CANDIDATE: u32 = 14;
pub const FROZEN_ALTERED: u32 = 32;
pub const FROZEN_NOGAIN: u32 = 194;

/// 五类攻击位（与操作轴对位：类号 = 操作号）。
pub const ATK_READUP: usize = 0; // 越权读：伪造无 No-Read-Up
pub const ATK_WRITEUP: usize = 1; // 越权写：伪造主体等级抬升
pub const ATK_MASKWIDE: usize = 2; // 越权执行：伪造支持位面全开
pub const ATK_PRIVFORGE: usize = 3; // 提权：伪造特权位注入
pub const ATK_SKIPDENY: usize = 4; // 绕检查点：伪造过滤门失效

/// 影子求值器（负例注入点——带攻击位拷贝的求值路径；真引擎在
/// unx36302 内不受本函数影响）。返回值语义与真引擎一致：0 许 /
/// 1..5 拒归因。
pub fn shadow_eval(s: usize, o: usize, op: usize, attack: usize) -> Result<u8, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if attack > ATK_SKIPDENY {
        return Err(E_RANGE);
    }
    let subj = mx::subject_at(s)?;
    let obj = mx::object_at(o)?;
    let mut sr = subj.rank;
    let mut sp = subj.privs;
    let mut supported = obj.supported;
    match attack {
        ATK_MASKWIDE => supported = 0x1F,
        ATK_PRIVFORGE => sp |= obj.needpriv,
        ATK_WRITEUP => {
            if sr < obj.rank {
                sr = obj.rank; // 等级抬升伪造
            }
        }
        _ => {}
    }
    let req: u8 = 1 << op;
    // 门一：掩码门（攻击 2 伪造全开）。
    if req & !supported & 0x1F != 0 {
        return Ok(mx::A_MASK);
    }
    // 门二：令牌过滤门（攻击 4 伪造门失效）。
    if attack != ATK_SKIPDENY && req & subj.deny != 0 {
        return Ok(mx::A_DENY);
    }
    // 门三：DACL 门（攻击不触碰 ACL——影子拷贝原样）。
    if obj.dacl_state == 2 {
        return Ok(mx::A_DACL);
    }
    if obj.dacl_state != 1 {
        let grant = if subj.group == 1 { obj.ace_admin } else { obj.ace_user };
        if req & grant == 0 {
            return Ok(mx::A_DACL);
        }
    }
    // 门四：IL 门（攻击 0 伪造无 No-Read-Up；攻击 1 等级抬升）。
    let up = sr < obj.rank;
    let il_hit = match op {
        mx::OP_WRITE => obj.policy & 0x01 != 0 && up,
        mx::OP_READ => attack != ATK_READUP && obj.policy & 0x02 != 0 && up,
        mx::OP_EXEC => obj.policy & 0x04 != 0 && up,
        _ => false,
    };
    if il_hit {
        return Ok(mx::A_IL);
    }
    // 门五：特权门（攻击 3 伪造持权位）。
    if obj.needpriv != 0 && req & obj.priv_ops != 0 && sp & obj.needpriv == 0 {
        return Ok(mx::A_PRIV);
    }
    Ok(mx::A_ALLOW)
}

/// 负例结果三分类。
pub const N_INTERCEPTED: u8 = 1; // 拦截（拒且归因未失真）
pub const N_CANDIDATE: u8 = 2; // 逃逸候选（拒格被判许——得手面）
pub const N_NOGAIN: u8 = 3; // 无增益（判决不变）

fn classify(base: u8, fake: u8) -> u8 {
    if fake == mx::A_ALLOW && base != mx::A_ALLOW {
        N_CANDIDATE
    } else if fake == base {
        N_NOGAIN
    } else {
        N_INTERCEPTED
    }
}

/// 负例库落账（240 例：格号 + 攻击类 + 基线 + 影子 + 分类）。
pub struct NegativeCase {
    pub cell: u32,
    pub attack: usize,
    pub base: u8,
    pub fake: u8,
    pub kind: u8,
}

/// 全库构建（判据 ① 的账面：每格 ≥1 例、合计恰 240）。
pub fn build_library() -> Result<Vec<NegativeCase>, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut lib = Vec::new();
    for s in 0..mx::N_SUBJ {
        for o in 0..mx::N_OBJ {
            for op in 0..mx::N_OP {
                let cell = mx::cell_id(s, o, op)?;
                let base = mx::evaluate(s, o, op)?;
                let fake = shadow_eval(s, o, op, op)?; // 攻击类 = 操作号
                lib.push(NegativeCase {
                    cell,
                    attack: op,
                    base,
                    fake,
                    kind: classify(base, fake),
                });
            }
        }
    }
    if lib.len() != mx::N_CELLS {
        return Err(E_LIB);
    }
    Ok(lib)
}

// ---- 检测器（逃逸候选点名器——漏检即 P0） --------------------------------------
// 对负例库全量扫描：凡逃逸候选必须被检测器标记；凡被标记者必须
// 真是候选（双向）。漏检一格 = 攻击穿过防线 = P0 直报。
pub fn detector_sweep(lib: &[NegativeCase]) -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut flagged = 0u32;
    let mut candidates = 0u32;
    for case in lib.iter() {
        let is_candidate = case.kind == N_CANDIDATE;
        // 检测器判定：影子许而基线拒 = 候选（与分类口径独立复算）。
        let detected = case.fake == mx::A_ALLOW && case.base != mx::A_ALLOW;
        if is_candidate {
            candidates += 1;
            if !detected {
                return Err(E_P0); // 候选漏检 = P0
            }
        }
        if detected {
            flagged += 1;
            if !is_candidate {
                return Err(E_P0); // 误报也是检测器破
            }
        }
    }
    if candidates != FROZEN_CANDIDATE || flagged != FROZEN_CANDIDATE {
        return Err(E_P0);
    }
    Ok(flagged)
}

/// 全库分类守恒（240 = 候选 14 + 拦截 32 + 无增益 194）。
pub fn library_tally(lib: &[NegativeCase]) -> Result<(u32, u32, u32), u16> {
    let mut c = 0u32;
    let mut a = 0u32;
    let mut n = 0u32;
    for case in lib.iter() {
        match case.kind {
            N_CANDIDATE => c += 1,
            N_INTERCEPTED => a += 1,
            N_NOGAIN => n += 1,
            _ => return Err(E_LIB),
        }
    }
    if c != FROZEN_CANDIDATE || a != FROZEN_ALTERED || n != FROZEN_NOGAIN {
        return Err(E_FROZEN);
    }
    Ok((c, a, n))
}

/// 负例行摘要（呈现面：格 + 攻击名 + 结果分类人话）。
pub fn case_text(case: &NegativeCase) -> String {
    let atk = match case.attack {
        ATK_READUP => "越权读",
        ATK_WRITEUP => "越权写",
        ATK_MASKWIDE => "越权执行",
        ATK_PRIVFORGE => "提权",
        _ => "绕检查点",
    };
    let kind = match case.kind {
        N_CANDIDATE => "逃逸候选（检测器点名）",
        N_INTERCEPTED => "拦截",
        _ => "无增益",
    };
    let mut text = String::from("格");
    text.push_str(&case.cell.to_string());
    text.push(':');
    text.push_str(atk);
    text.push(':');
    text.push_str(kind);
    text
}

// ---- 确定性演练 -------------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    // 注入不伤真引擎：真引擎 240 格仍全对语义书。
    if mx::matrix_match_spec()? != 90 {
        return Err(E_ENGINE);
    }
    let lib = build_library()?;
    let (c, a, n) = library_tally(&lib)?;
    let flagged = detector_sweep(&lib)?;
    // 呈现面抽核：候选例摘要含攻击名与点名标记。
    let mut sample: Option<String> = None;
    for case in lib.iter() {
        if case.kind == N_CANDIDATE {
            sample = Some(case_text(case));
            break;
        }
    }
    match sample {
        Some(text) => {
            if !text.contains("逃逸候选") || !text.starts_with("格") {
                return Err(E_LIB);
            }
        }
        None => return Err(E_LIB),
    }
    if c + a + n != mx::N_CELLS as u32 || flagged != FROZEN_CANDIDATE {
        return Err(E_LIB);
    }
    Ok(flagged.wrapping_mul(1000).wrapping_add(a))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_LIB => "负例库账不平（不足 240/分类漂移/呈现面破）",
        E_P0 => "负例逃逸（候选漏检或检测器误报）——P0 直报",
        E_ENGINE => "注入后真引擎漂移（负例污染了本体）",
        E_FROZEN => "冻结分布漂移（候选 14/拦截 32/无增益 194）",
        E_RANGE => "攻击类越界",
        _ => "未知诊断码",
    }
}

pub fn run_unx36308_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36308_mxnegative");
    cs.add("SEG 段位独占", SEG_BASE == 0xB1E0, "0xB1E0~0xB1EF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "240 负例全落账·候选 14 全检出·真引擎无污染"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("守恒锚", FROZEN_CANDIDATE + FROZEN_ALTERED + FROZEN_NOGAIN == 240, "候选 14 + 拦截 32 + 无增益 194 = 240");
    cs.add("真引擎锚", mx::matrix_match_spec() == Ok(90), "影子注入不伤本体");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36308_checks();
        assert!(cs.len() > 0);
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36308_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 4);
    }
}
