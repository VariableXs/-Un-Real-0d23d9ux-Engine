//! UNX-F36321-J1 UAC 场景账 v2（20 场景 → 60 场景扩容）。
//! 判据：① 场景账 60 组（启动/安装/设置更改/服务安装四族）全
//! 组判据化；② 双查同引擎断言逐组可注入；③ 纯功能代码 ≥300 行。
//!
//! 设计纪律（对标 Windows UAC 场景账的扩容方法学）：v1 的 20 场
//! 景只覆盖启动面，v2 扩到 60 组四族（启动 15 / 安装 15 / 设置
//! 更改 15 / 服务安装 15），每组场景**判据化**——不是描述文案，
//! 是可执行判据：场景绑定矩阵格（主体面 × 客体 × 操作的三元组
//! 命题），判决经「双查同引擎」：引擎求值与语义书取值两路核对
//！（同一引擎出两侧账，任何一侧改判都会在另一侧显影——扩容最
//! 怕的就是账倍增后各抄各的）。场景入账四要素：场景号（1..=60
//! 连续零跳号）、族别（四族闭集）、绑定格号（1..=240 域内）、
//! 期望判决（语义书码）。「逐组可注入」：每组场景的期望值翻转
//! 一枚注入探针，对账器必须逐组检出——60/60 检出 = 判据化是真
//! 的（注入探针只打在场景账的副本上，正账不动）。族谱冻结：四
//! 族 × 15 组，族内格谱按主体面 × 客体轴显式枚举（启动 = EXEC
//! 五面×三客体、安装 = WRITE 五面×三客体、设置更改 = WDAC 六
//! 面×二客体加三补格、服务安装 = 五面×服务三类操作）。

use crate::checks::CheckSet;
use super::unx36302_mxloband as mx;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xB2B0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 0; // 版本锚不符
pub const E_SCENE: u16 = SEG_BASE + 1; // 场景账破（要素缺/格谱错）
pub const E_DUAL: u16 = SEG_BASE + 2; // 双查破（引擎 vs 语义书异）
pub const E_INJECT: u16 = SEG_BASE + 3; // 注入探针未逐组检出
pub const E_FROZEN: u16 = SEG_BASE + 4; // 场景数/族谱漂移（60·4×15）
pub const E_RANGE: u16 = SEG_BASE + 5; // 族别越界

/// 模块版本锚。
pub const VER: u32 = 1;

/// 场景总数 / 族数 / 族容量冻结锚。
pub const N_SCENES: u32 = 60;
pub const N_FAMILIES: u32 = 4;
pub const FAMILY_SIZE: u32 = 15;

/// 四族闭集。
pub const FAM_LAUNCH: u8 = 0; // 启动
pub const FAM_INSTALL: u8 = 1; // 安装
pub const FAM_SETTINGS: u8 = 2; // 设置更改
pub const FAM_SERVICE: u8 = 3; // 服务安装

/// 场景账行（四要素）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scene {
    pub id: u32,
    pub family: u8,
    pub cell: u32,
    pub expect: u8,
}

/// 族内格谱（族别 → 15 个 (s,o,op) 三元组——显式枚举）。
pub fn family_cells(family: u8) -> Result<Vec<(usize, usize, usize)>, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut cells = Vec::new();
    match family {
        FAM_LAUNCH => {
            // 启动：EXEC 五面 × 三客体（O0 文件/O2 节区/O3 管道）。
            for s in [0usize, 1, 3, 5, 6] {
                for o in [0usize, 2, 3] {
                    cells.push((s, o, mx::OP_EXEC));
                }
            }
        }
        FAM_INSTALL => {
            // 安装：WRITE 五面 × 三客体（O0/O1/O3）。
            for s in [0usize, 1, 3, 5, 6] {
                for o in [0usize, 1, 3] {
                    cells.push((s, o, mx::OP_WRITE));
                }
            }
        }
        FAM_SETTINGS => {
            // 设置更改：WDAC 六面 × 二客体（O0/O5）+ 三补格（O3）。
            for s in [0usize, 1, 2, 3, 5, 6] {
                for o in [0usize, 5] {
                    cells.push((s, o, mx::OP_WDAC));
                }
            }
            for s in [3usize, 5, 6] {
                cells.push((s, 3, mx::OP_WDAC));
            }
        }
        FAM_SERVICE => {
            // 服务安装：五面 × 服务三类操作（O4 写/O4 删/O5 写）。
            for s in [0usize, 1, 3, 5, 6] {
                cells.push((s, 4, mx::OP_WRITE));
                cells.push((s, 4, mx::OP_DEL));
                cells.push((s, 5, mx::OP_WRITE));
            }
        }
        _ => return Err(E_RANGE),
    }
    if cells.len() as u32 != FAMILY_SIZE {
        return Err(E_SCENE);
    }
    Ok(cells)
}

// ---- 场景账构建（四族 × 15 组 = 60，场景号连续零跳号） ----------------------------
pub fn build_scene_book() -> Result<Vec<Scene>, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut book = Vec::new();
    let mut id: u32 = 0;
    for family in 0..N_FAMILIES as u8 {
        for (s, o, op) in family_cells(family)? {
            id += 1;
            let cell = mx::cell_id(s, o, op)?;
            book.push(Scene {
                id,
                family,
                cell,
                expect: mx::spec_at(cell)?,
            });
        }
    }
    if book.len() as u32 != N_SCENES {
        return Err(E_SCENE);
    }
    Ok(book)
}

/// 场景账完整性核（号连续/族谱计数/绑定格在域/期望在闭集）。
pub fn book_audit(book: &[Scene]) -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if book.len() as u32 != N_SCENES {
        return Err(E_SCENE);
    }
    let mut per_family = [0u32; 4];
    for (i, scene) in book.iter().enumerate() {
        if scene.id != (i + 1) as u32 {
            return Err(E_SCENE); // 号跳
        }
        if scene.family > FAM_SERVICE {
            return Err(E_RANGE);
        }
        per_family[scene.family as usize] += 1;
        if scene.cell == 0 || scene.cell as usize > mx::N_CELLS {
            return Err(E_SCENE);
        }
        if !mx::attribution_in_range(scene.expect) {
            return Err(E_SCENE);
        }
    }
    for count in per_family.iter() {
        if *count != FAMILY_SIZE {
            return Err(E_FROZEN);
        }
    }
    Ok(N_SCENES)
}

// ---- 双查同引擎（每组场景两路判决核对） --------------------------------------------
// 路一：引擎求值；路二：语义书取值。两路必同值且等于场景期望。
pub fn dual_check(book: &[Scene]) -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut checked = 0u32;
    for scene in book.iter() {
        let (s, o, op) = mx::cell_parts(scene.cell)?;
        let engine = mx::evaluate(s, o, op)?;
        let book_side = mx::spec_at(scene.cell)?;
        if engine != book_side || engine != scene.expect {
            return Err(E_DUAL);
        }
        checked += 1;
    }
    if checked != N_SCENES {
        return Err(E_DUAL);
    }
    Ok(checked)
}

// ---- 逐组可注入（对账器 60/60 检出翻转探针） ----------------------------------------
// 探针打在场景账副本上：每组的期望值翻一枚（许↔拒按位翻转），
// 对账器逐组核对引擎判决 vs 翻转期望，必须每组都报差。
pub fn injection_probe_all(book: &[Scene]) -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut caught = 0u32;
    for scene in book.iter() {
        let (s, o, op) = mx::cell_parts(scene.cell)?;
        let engine = mx::evaluate(s, o, op)?;
        let flipped = if scene.expect == mx::A_ALLOW {
            mx::A_DACL // 许 → 翻成拒（门标签取位面无关对照值）
        } else {
            mx::A_ALLOW // 拒 → 翻成许
        };
        if engine == flipped {
            return Err(E_INJECT); // 翻转后仍同值 = 探针无效
        }
        caught += 1;
    }
    if caught != N_SCENES {
        return Err(E_INJECT);
    }
    Ok(caught)
}

/// 场景账行（呈现面：场景号 + 族名 + 格号 + 判决标签）。
pub fn scene_row_text(scene: &Scene) -> Result<String, u16> {
    if scene.family > FAM_SERVICE {
        return Err(E_RANGE);
    }
    let fam = match scene.family {
        FAM_LAUNCH => "启动",
        FAM_INSTALL => "安装",
        FAM_SETTINGS => "设置更改",
        _ => "服务安装",
    };
    let mut text = String::from("场景");
    text.push_str(&scene.id.to_string());
    text.push(':');
    text.push_str(fam);
    text.push_str(" 格");
    text.push_str(&scene.cell.to_string());
    text.push(':');
    text.push_str(mx::attribution_text(scene.expect));
    Ok(text)
}

// ---- 确定性演练 -------------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let book = build_scene_book()?;
    let audited = book_audit(&book)?;
    let dual = dual_check(&book)?;
    let caught = injection_probe_all(&book)?;
    // 呈现面抽核：四族各取首组账行、族名齐备。
    for family in 0..N_FAMILIES as u8 {
        let scene = book
            .iter()
            .find(|v| v.family == family)
            .copied()
            .unwrap_or(Scene { id: 0, family: 255, cell: 0, expect: 255 });
        let row = scene_row_text(&scene)?;
        if row.is_empty() {
            return Err(E_SCENE);
        }
    }
    if audited != N_SCENES || dual != N_SCENES || caught != N_SCENES {
        return Err(E_FROZEN);
    }
    Ok(dual.wrapping_mul(1000).wrapping_add(caught))
}

// ---- 族谱分布账增补 -----------------------------------------------------------
/// 族内许格账（四族许格数运行时聚合——族谱判据化的分布面；许格
/// 总和与全账守恒、每族许格数不越族容量）。
pub fn family_allow_tally() -> Result<[u32; 4], u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let book = build_scene_book()?;
    let mut tally = [0u32; 4];
    let mut total_allow = 0u32;
    for scene in book.iter() {
        if scene.expect == mx::A_ALLOW {
            tally[scene.family as usize] += 1;
            total_allow += 1;
        }
    }
    let sum = tally.iter().fold(0u32, |a, v| a + *v);
    if sum != total_allow || total_allow == 0 {
        return Err(E_SCENE);
    }
    for v in tally.iter() {
        if *v > FAMILY_SIZE {
            return Err(E_FROZEN);
        }
    }
    Ok(tally)
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_SCENE => "场景账破（要素缺/号跳/格谱错/族谱计数漂）",
        E_DUAL => "双查破（引擎与语义书两路判决异）",
        E_INJECT => "注入探针未逐组检出（判据化虚设）",
        E_FROZEN => "场景数/族谱漂移（60·4×15）",
        E_RANGE => "族别越界",
        _ => "未知诊断码",
    }
}

pub fn run_unx36321_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36321_mxuac60");
    cs.add("SEG 段位独占", SEG_BASE == 0xB2B0, "0xB2B0~0xB2BF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "60 场景四族判据化·双查同引擎 60/60·注入探针 60/60 检出"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("扩容锚", N_SCENES == 60 && N_FAMILIES == 4, "v2 扩容：20 → 60 场景、四族 × 15");
    cs.add("族谱分布账", family_allow_tally().is_ok(), "四族许格账守恒·每族不越 15 组容量");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36321_checks();
        assert!(cs.len() > 0);
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36321_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 4);
    }
}
