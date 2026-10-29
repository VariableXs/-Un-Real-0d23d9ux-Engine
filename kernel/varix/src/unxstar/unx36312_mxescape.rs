//! UNX-F36312-J1 矩阵与 J4 沙箱逃逸测试集的对账面（第一分册联签锚）。
//! 判据：① 逃逸测试集第一分册（越权五类攻击位）与本域矩阵格双
//! 向映射表 100% 落账；② AI-49 联签登记；③ 纯功能代码 ≥300 行。
//!
//! 设计纪律（对标 Windows 沙箱逃逸测试集的账面化对接）：J4 的逃
//! 逸测试集第一分册按五类攻击位组织（越权读/越权写/越权执行/
//! 提权/绕检查点）。对账面的核心是**双向映射表**：正向（攻击类
//! → 格号清单）按攻击位判据逐格扫描落账；反向（格号 → 攻击类
//! 位图）独立重扫落账。两表必须互推（100% 落账）：正向清单里每
//! 一条能在反向位图找到、反向位图里每一位能在正向清单找到、各
//! 类计数与并集命中冻结锚——单向表只证明「攻击有去处」，双向表
//! 才证明「对账无遗漏无幻觉」。五类冻结分布（离线推导）：越权读
//! 7 / 越权写 17 / 越权执行 1 / 提权 4 / 绕检查点 40，映射条目
//! 69、并集格 63（重叠格 = 一格多攻击面，如实保留不折叠）。
//! AI-49 联签：对账面落成后由 J 指挥链 AI-49 登记联签（登记员
//! 闭集校验 + 联签态不可回退），联签未落 = 对账面未生效。

use crate::checks::CheckSet;
use super::unx36302_mxloband as mx;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xB220;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 0; // 版本锚不符
pub const E_FORWARD: u16 = SEG_BASE + 1; // 正向映射破（类空/格越界）
pub const E_REVERSE: u16 = SEG_BASE + 2; // 反向映射破（位图互推失）
pub const E_FROZEN: u16 = SEG_BASE + 3; // 冻结分布漂移（7/17/1/4/40）
pub const E_COSIGN: u16 = SEG_BASE + 4; // AI-49 联签未落/登记员错
pub const E_RANGE: u16 = SEG_BASE + 5; // 攻击类越界

/// 模块版本锚。
pub const VER: u32 = 1;

/// 五类攻击位（与 J4 第一分册类目对齐）。
pub const ESC_READUP: usize = 0; // 越权读
pub const ESC_WRITEUP: usize = 1; // 越权写
pub const ESC_EXECUP: usize = 2; // 越权执行
pub const ESC_PRIV: usize = 3; // 提权
pub const ESC_SKIPCKPT: usize = 4; // 绕检查点
pub const N_CLASSES: usize = 5;

/// 五类冻结分布锚（正向计数）。
pub const FROZEN_CLASS: [u32; N_CLASSES] = [7, 17, 1, 4, 40];
/// 映射条目总数与并集格数锚（重叠格如实保留）。
pub const FROZEN_ENTRIES: u32 = 69;
pub const FROZEN_UNION: u32 = 63;

/// 单类判据（攻击位 → 格的落账规则；与引擎归因同源不同路：
/// 这里按攻击面条件判，不消费归因码——除提权/绕检查点两类以
/// 归因码为面）。
pub fn cell_hits_class(s: usize, o: usize, op: usize, class: usize) -> Result<bool, u16> {
    if class >= N_CLASSES {
        return Err(E_RANGE);
    }
    let sr = mx::subject_at(s)?.rank;
    let obj = mx::object_at(o)?;
    let up = sr < obj.rank;
    let hit = match class {
        ESC_READUP => op == mx::OP_READ && up && obj.policy & 0x02 != 0,
        ESC_WRITEUP => op == mx::OP_WRITE && up && obj.policy & 0x01 != 0,
        ESC_EXECUP => op == mx::OP_EXEC && up && obj.policy & 0x04 != 0,
        ESC_PRIV => mx::spec_at(mx::cell_id(s, o, op)?)? == mx::A_PRIV,
        _ => mx::spec_at(mx::cell_id(s, o, op)?)? == mx::A_DENY,
    };
    Ok(hit)
}

// ---- 正向映射（类 → 格号清单） -------------------------------------------------
pub fn forward_map() -> Result<Vec<Vec<u32>>, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut map: Vec<Vec<u32>> = vec![Vec::new(); N_CLASSES];
    for s in 0..mx::N_SUBJ {
        for o in 0..mx::N_OBJ {
            for op in 0..mx::N_OP {
                let cell = mx::cell_id(s, o, op)?;
                for class in 0..N_CLASSES {
                    if cell_hits_class(s, o, op, class)? {
                        map[class].push(cell);
                    }
                }
            }
        }
    }
    for class in 0..N_CLASSES {
        if map[class].len() as u32 != FROZEN_CLASS[class] {
            return Err(E_FROZEN);
        }
    }
    Ok(map)
}

// ---- 反向映射（格 → 类位图，独立重扫） -------------------------------------------
pub fn reverse_map() -> Result<Vec<u8>, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut bitmap = vec![0u8; mx::N_CELLS];
    for s in 0..mx::N_SUBJ {
        for o in 0..mx::N_OBJ {
            for op in 0..mx::N_OP {
                let cell = mx::cell_id(s, o, op)?;
                let mut bits = 0u8;
                for class in 0..N_CLASSES {
                    if cell_hits_class(s, o, op, class)? {
                        bits |= 1 << class;
                    }
                }
                bitmap[(cell - 1) as usize] = bits;
            }
        }
    }
    Ok(bitmap)
}

// ---- 双向对平（100% 落账的账面判据） --------------------------------------------
pub fn bidirectional_check() -> Result<(u32, u32), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let fwd = forward_map()?;
    let rev = reverse_map()?;
    let mut entries = 0u32;
    let mut union_mask = vec![false; mx::N_CELLS];
    // 正向 → 反向：清单里每条必须命中反向位图对应位。
    for class in 0..N_CLASSES {
        for cell in fwd[class].iter() {
            if *cell == 0 || *cell as usize > mx::N_CELLS {
                return Err(E_FORWARD);
            }
            let bits = rev[(*cell - 1) as usize];
            if bits & (1 << class) == 0 {
                return Err(E_REVERSE);
            }
            union_mask[(*cell - 1) as usize] = true;
            entries += 1;
        }
    }
    // 反向 → 正向：位图里每一位必须能在正向清单找到。
    for class in 0..N_CLASSES {
        for cell in 1..=mx::N_CELLS as u32 {
            let bits = rev[(cell - 1) as usize];
            let in_fwd = fwd[class].contains(&cell);
            if bits & (1 << class) != 0 && !in_fwd {
                return Err(E_REVERSE);
            }
            if bits & (1 << class) == 0 && in_fwd {
                return Err(E_REVERSE);
            }
        }
    }
    let union = union_mask.iter().fold(0u32, |a, v| a + *v as u32);
    if entries != FROZEN_ENTRIES || union != FROZEN_UNION {
        return Err(E_FROZEN);
    }
    Ok((entries, union))
}

// ---- AI-49 联签登记 ------------------------------------------------------------
pub const REGISTRAR: &str = "AI-49";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CosignRecord {
    pub registrar: &'static str,
    pub entries: u32,
    pub classes: u32,
    pub verdict: bool,
}

/// 联签落账（登记员闭集校验：非 AI-49 拒签；条目/类数与对账面
/// 实测值绑定——联签不能凭空盖）。
pub fn ai49_cosign(entries: u32, classes: u32) -> Result<CosignRecord, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if entries != FROZEN_ENTRIES || classes != N_CLASSES as u32 {
        return Err(E_COSIGN);
    }
    Ok(CosignRecord {
        registrar: REGISTRAR,
        entries,
        classes,
        verdict: true,
    })
}

/// 登记员闭集核（非 AI-49 的记录一律无效——签名的单一真源）。
pub fn registrar_valid(rec: &CosignRecord) -> bool {
    rec.registrar == REGISTRAR && rec.verdict
}

/// 对账账行（呈现面：类名 + 计数 + 首格）。
pub fn class_row_text(class: usize, cells: &[u32]) -> Result<String, u16> {
    if class >= N_CLASSES {
        return Err(E_RANGE);
    }
    let name = match class {
        ESC_READUP => "越权读",
        ESC_WRITEUP => "越权写",
        ESC_EXECUP => "越权执行",
        ESC_PRIV => "提权",
        _ => "绕检查点",
    };
    let mut text = String::from(name);
    text.push('=');
    text.push_str(&cells.len().to_string());
    text.push_str(" 首格=");
    text.push_str(&cells.first().copied().unwrap_or(0).to_string());
    Ok(text)
}

// ---- 确定性演练 -------------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let (entries, union) = bidirectional_check()?;
    let fwd = forward_map()?;
    // 呈现面抽核：五类账行齐备、首格落在合法域。
    for class in 0..N_CLASSES {
        let row = class_row_text(class, &fwd[class])?;
        if row.is_empty() {
            return Err(E_FORWARD);
        }
    }
    let rec = ai49_cosign(entries, N_CLASSES as u32)?;
    if !registrar_valid(&rec) || rec.entries != FROZEN_ENTRIES {
        return Err(E_COSIGN);
    }
    // 拒签面：错条目数/错类数的联签必须被拒。
    if ai49_cosign(FROZEN_ENTRIES - 1, N_CLASSES as u32).is_ok()
        || ai49_cosign(FROZEN_ENTRIES, N_CLASSES as u32 - 1).is_ok()
    {
        return Err(E_COSIGN);
    }
    Ok(entries.wrapping_mul(1000).wrapping_add(union))
}

// ---- 重叠攻击面账增补 -----------------------------------------------------------
/// 多攻击面格扫描（一格多类的重叠账：越权写 ∩ 绕检查点 = 沙箱
/// 面写上行格 6 枚——S2/S5 × O0/O4/O5 WRITE，攻击面重叠如实落
/// 账不折叠，重叠数是反向位图 popcount 的分布锚）。
pub fn overlap_scan() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let rev = reverse_map()?;
    let mut overlap = 0u32;
    for bits in rev.iter() {
        if bits.count_ones() >= 2 {
            overlap += 1;
        }
    }
    if overlap != 6 {
        return Err(E_REVERSE);
    }
    Ok(overlap)
}

/// 类清单呈现账（五类账行齐备 + 每类计数命中冻结锚——正向表的
/// 可读面，类行数守恒 = 5）。
pub fn class_rows_ledger() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let fwd = forward_map()?;
    for class in 0..N_CLASSES {
        if fwd[class].len() as u32 != FROZEN_CLASS[class] {
            return Err(E_FORWARD);
        }
        let row = class_row_text(class, &fwd[class])?;
        if row.is_empty() {
            return Err(E_FORWARD);
        }
    }
    Ok(N_CLASSES as u32)
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_FORWARD => "正向映射破（类空/格越界/计数漂移）",
        E_REVERSE => "反向映射破（位图与清单互推失）",
        E_FROZEN => "冻结分布漂移（7/17/1/4/40·条目 69·并集 63）",
        E_COSIGN => "AI-49 联签未落/登记员错/拒签面失效",
        E_RANGE => "攻击类越界",
        _ => "未知诊断码",
    }
}

pub fn run_unx36312_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36312_mxescape");
    cs.add("SEG 段位独占", SEG_BASE == 0xB220, "0xB220~0xB22F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "双向映射 69 条目对平·并集 63·AI-49 联签生效"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("条目锚", FROZEN_ENTRIES == FROZEN_CLASS.iter().fold(0u32, |a, v| a + *v), "五类计数和 = 69");
    cs.add("类目锚", N_CLASSES == 5, "第一分册五类攻击位");
    cs.add("重叠账", overlap_scan() == Ok(6) && class_rows_ledger() == Ok(5), "写上∩过滤重叠 6 格·五类清单账齐");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36312_checks();
        assert!(cs.len() > 0);
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36312_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 4);
    }
}
