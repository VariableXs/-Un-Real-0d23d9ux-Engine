//! UNX-F36311-J1 矩阵诊断可解释性总测（应拒格解释率 100%）。
//! 判据：① 应拒格 100% 可解释（解释不了 = 实现有洞回炉纪律）；
//! ② 不可解释注入 10 例被质检拦截；③ 纯功能代码 ≥300 行。
//!
//! 设计纪律（对标 Windows 授权失败的诊断事件）：每个拒格必须能
//! 生成**完整解释链**——格号、主体面名、客体名、操作名、命中门
//! 名、门参（等级对/策略位/授权位/需求特权）。解释不是自由文本：
//! 生成器按模板拼装（结构化字段 → 人话串），质检器按结构断言
//! （格号前缀 + 「拒·」命中标记 + 五门标记之一 + 最短长度）——
//! 生成与质检是两套独立逻辑，质检标准在生成器之外硬编码。150
//! 个拒格逐格过质检（100% 可解释 = 逐格双绿）；不可解释注入 =
//! 故意构造 10 例坏解释（空前缀/丢命中标记/幽灵门名/截断体/
//! 许格伪装拒格），质检必须 10/10 拦截——质检器放走任何一例坏
//! 解释等于解释面名存实亡。「解释不了 = 回炉」在本卡落为：任何
//! 拒格解释失败即 E_REWORK，不静默跳过。

use crate::checks::CheckSet;
use super::unx36302_mxloband as mx;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xB210;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 0; // 版本锚不符
pub const E_EXPLAIN: u16 = SEG_BASE + 1; // 拒格解释生成失败
pub const E_QC: u16 = SEG_BASE + 2; // 质检器漏拦坏解释
pub const E_REWORK: u16 = SEG_BASE + 3; // 解释不了（回炉口径触发）
pub const E_RANGE: u16 = SEG_BASE + 4; // 格号越界
pub const E_FROZEN: u16 = SEG_BASE + 5; // 拒格总数漂移（150）

/// 模块版本锚。
pub const VER: u32 = 1;

/// 拒格总数冻结锚（240 - 90 许）。
pub const FROZEN_DENY: u32 = 150;

// ---- 主体面/客体/操作名册（解释链的字段真源） ----------------------------------
pub fn face_name(s: usize) -> Result<&'static str, u16> {
    Ok(match s {
        0 => "匿名不可信",
        1 => "基础低权",
        2 => "AppContainer 沙箱",
        3 => "标准用户",
        4 => "服务低IL请求者",
        5 => "沙箱消费预设",
        6 => "UAC过滤令牌",
        7 => "提权/系统",
        _ => return Err(E_RANGE),
    })
}

pub fn object_name(o: usize) -> Result<&'static str, u16> {
    Ok(match o {
        0 => "文件",
        1 => "注册表键",
        2 => "节区",
        3 => "命名管道",
        4 => "进程句柄",
        5 => "桌面堆",
        _ => return Err(E_RANGE),
    })
}

pub fn op_name(op: usize) -> Result<&'static str, u16> {
    Ok(match op {
        0 => "读",
        1 => "写",
        2 => "执行",
        3 => "删除",
        4 => "改权",
        _ => return Err(E_RANGE),
    })
}

// ---- 解释链生成器（判据 ① 的生成面） ------------------------------------------
// 模板：格N:面×客体×操作 -> 拒·门名(门参)。门参按归因码分支：
// IL 门给等级对与策略位、DACL 门给组别与授权位、特权门给需求
// 特权、掩码门给支持位面、过滤门给剥除位面。
pub fn explain_cell(id: u32) -> Result<String, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let (s, o, op) = mx::cell_parts(id)?;
    let code = mx::evaluate(s, o, op)?;
    if code == mx::A_ALLOW {
        return Err(E_EXPLAIN); // 许格无解释链（解释只服务拒格）
    }
    let subj = mx::subject_at(s)?;
    let obj = mx::object_at(o)?;
    let mut text = String::from("格");
    text.push_str(&id.to_string());
    text.push(':');
    text.push_str(face_name(s)?);
    text.push('×');
    text.push_str(object_name(o)?);
    text.push('×');
    text.push_str(op_name(op)?);
    text.push_str(" -> ");
    text.push_str(mx::attribution_text(code));
    match code {
        mx::A_IL => {
            text.push_str("|主体rank=");
            text.push_str(&subj.rank.to_string());
            text.push_str(" 客体rank=");
            text.push_str(&obj.rank.to_string());
            text.push_str(" 策略位=");
            text.push_str(&obj.policy.to_string());
        }
        mx::A_DACL => {
            text.push_str("|组别=");
            text.push_str(&subj.group.to_string());
            text.push_str(" DACL态=");
            text.push_str(&obj.dacl_state.to_string());
        }
        mx::A_PRIV => {
            text.push_str("|需求特权=");
            text.push_str(&obj.needpriv.to_string());
            text.push_str(" 持权=");
            text.push_str(&subj.privs.to_string());
        }
        mx::A_MASK => {
            text.push_str("|支持位面=");
            text.push_str(&obj.supported.to_string());
        }
        _ => {
            text.push_str("|剥除位面=");
            text.push_str(&subj.deny.to_string());
        }
    }
    Ok(text)
}

// ---- 质检器（判据标准在生成器之外硬编码） --------------------------------------
// 结构断言六条：① 以「格」开头（带格号前缀）；② 含「拒·」命中
// 标记；③ 含五门标记之一（门名标签闭集）；④ 长度 ≥ 24 字节
// （最短合法解释的下限——门参至少一组）；⑤ 含「×」字段分隔
// （面×客体×操作三段在场）；⑥ 含「 -> 」判决引导符。
pub fn qc_validate(text: &str) -> bool {
    if !text.starts_with("格") || !text.contains("拒·") || text.len() < 24 {
        return false;
    }
    if !text.contains("×") || !text.contains(" -> ") {
        return false;
    }
    let gates = [
        "拒·DACL 门",
        "拒·IL 门",
        "拒·特权门",
        "拒·掩码门",
        "拒·令牌过滤门",
    ];
    gates.iter().any(|g| text.contains(g))
}

/// 全拒格解释过质检（应拒格 100% 可解释——逐格双绿）。
pub fn explain_all_denied() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut explained = 0u32;
    for id in 1..=mx::N_CELLS as u32 {
        let code = mx::spec_at(id)?;
        if code == mx::A_ALLOW {
            continue;
        }
        let text = explain_cell(id)?;
        if !qc_validate(&text) {
            return Err(E_REWORK); // 解释不了 = 回炉，不静默
        }
        explained += 1;
    }
    if explained != FROZEN_DENY {
        return Err(E_FROZEN);
    }
    Ok(explained)
}

// ---- 不可解释注入 10 例（判据 ②：质检 10/10 拦截） -----------------------------
pub fn qc_intercept_injections() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let bad: [String; 10] = [
        String::from(""), // 空串
        String::from("拒·IL 门"), // 无格号前缀
        String::from("格153:沙箱消费预设×文件×写 -> 许"), // 许格伪装
        String::from("格153 拒·幽灵门(参)"), // 幽灵门名
        String::from("格153:拒·"), // 截断体（丢门名）
        String::from("格X:拒·IL 门|主体rank=1"), // 格号非数
        String::from("格153:拒·il 门|lower"), // 门名小写漂移
        String::from("格1"), // 过短
        String::from("拒·DACL 门 格153"), // 前缀错位
        String::from("格153:拒·过滤门|剥除=26"), // 门名缺「令牌」字样
    ];
    let mut caught = 0u32;
    for text in bad.iter() {
        if qc_validate(text) {
            return Err(E_QC); // 放走坏解释 = 质检破
        }
        caught += 1;
    }
    if caught != 10 {
        return Err(E_QC);
    }
    Ok(caught)
}

/// 解释账行（呈现面：抽三格各门一例的账行文本）。
pub fn explain_sample_rows() -> Result<Vec<String>, u16> {
    let mut rows = Vec::new();
    // 过滤门例：S6 WDAC 文件（格 185）。
    rows.push(explain_cell(mx::cell_id(mx::S_FILTERED, 0, mx::OP_WDAC)?)?);
    // 掩码门例：S7 EXEC 注册表键（格 218）。
    rows.push(explain_cell(mx::cell_id(mx::S_ELEVATED, 1, mx::OP_EXEC)?)?);
    // IL 门例：S1 WRITE 文件（格 32）。
    rows.push(explain_cell(mx::cell_id(mx::S_LOW, 0, mx::OP_WRITE)?)?);
    if rows.len() != 3 {
        return Err(E_EXPLAIN);
    }
    Ok(rows)
}

// ---- 确定性演练 -------------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let explained = explain_all_denied()?;
    let caught = qc_intercept_injections()?;
    let rows = explain_sample_rows()?;
    // 三例账行分别命中过滤/掩码/IL 门标记。
    if !rows[0].contains("拒·令牌过滤门") || !rows[1].contains("拒·掩码门") || !rows[2].contains("拒·IL 门") {
        return Err(E_EXPLAIN);
    }
    if explained != FROZEN_DENY || caught != 10 {
        return Err(E_FROZEN);
    }
    Ok(explained.wrapping_mul(1000).wrapping_add(caught))
}

// ---- 面样例解释账增补 -----------------------------------------------------------
/// 八面各取首个拒格的解释账（逐行过质检——全格可解释的抽面呈现；
/// 每面至少一拒格是矩阵分布的既定事实，面内全许即账面破）。
pub fn face_sample_explanations() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut n = 0u32;
    for s in 0..mx::N_SUBJ {
        let mut found = false;
        for o in 0..mx::N_OBJ {
            for op in 0..mx::N_OP {
                let id = mx::cell_id(s, o, op)?;
                if mx::spec_at(id)? == mx::A_ALLOW {
                    continue;
                }
                let text = explain_cell(id)?;
                if !qc_validate(&text) {
                    return Err(E_REWORK);
                }
                found = true;
                break;
            }
            if found {
                break;
            }
        }
        if !found {
            return Err(E_EXPLAIN); // 面内全许 = 分布漂移
        }
        n += 1;
    }
    if n != mx::N_SUBJ as u32 {
        return Err(E_EXPLAIN);
    }
    Ok(n)
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_EXPLAIN => "拒格解释生成失败（许格误入/字段缺）",
        E_QC => "质检器漏拦坏解释",
        E_REWORK => "解释不了（回炉口径触发）",
        E_RANGE => "格号/面客体操作下标越界",
        E_FROZEN => "拒格总数漂移（150）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36311_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36311_mxexplain");
    cs.add("SEG 段位独占", SEG_BASE == 0xB210, "0xB210~0xB21F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "拒格 150/150 过质检·坏解释 10/10 拦截·三门样例账行命中"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("可解释锚", explain_all_denied() == Ok(FROZEN_DENY), "应拒格解释率 100%");
    cs.add("拦截锚", qc_intercept_injections() == Ok(10), "不可解释注入 10 例全拦");
    cs.add("面样例账", face_sample_explanations() == Ok(8), "八面各一拒格解释过质检");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36311_checks();
        assert!(cs.len() > 0);
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36311_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 4);
    }
}
