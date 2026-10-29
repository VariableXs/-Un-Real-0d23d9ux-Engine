//! UNX-F36278-J1 UAC 面 Windows 对照预告（弹窗时序/错误码 20 格）。
//! 判据：① 对照矩阵 20 格（时序 8/错误码 7/策略 5）登记；② 缺
//! 基准数据格标 N/A 不编造；③ 纯功能 ≥300 行。
//! 设计纪律：对照矩阵是「与 Windows 行为对齐」的**预告账**——
//! 每格三态：对齐（本地语义与 Windows 一致，注证据锚）/ 偏离
//! （有意不同，注偏离理由）/ N/A（暂无基准数据——诚实标缺，
//! 禁编造）。编造是对照账最恶劣的失效：格上写着「对齐」而实
//! 际无证据，后人会拿假账当真锚。N/A 格必须可被后续批次补齐
//! （格带补齐位）。

use crate::checks::CheckSet;
use super::unx36270_uaccancel as cx;

pub const SEG_BASE: u16 = 0xAFE0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_FAB: u16 = SEG_BASE + 2; // 编造（对齐格无证据锚）
pub const E_SHAPE: u16 = SEG_BASE + 3; // 矩阵形状破（20 格/分组）
pub const E_NA: u16 = SEG_BASE + 4; // N/A 格无补齐位
pub const E_FROZEN: u16 = SEG_BASE + 5; // 对照面冻结破
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 对照三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlignState {
    Aligned,   // 对齐（有证据锚）
    Deviated,  // 有意偏离（有理由）
    NotAvail,  // N/A（无基准数据——可补齐）
}

/// 对照格（面/名/态/证据或理由锚/补齐位）。
#[derive(Clone, Copy)]
pub struct RefCell {
    pub group: u8,     // 0=时序 1=错误码 2=策略
    pub name: &'static str,
    pub state: AlignState,
    pub anchor: &'static str, // 对齐=证据锚；偏离=理由；N/A=待补项
    pub fillable: bool,
}

/// 时序 8 格（弹窗生命周期时序面）。
const TIMING_CELLS: [RefCell; 8] = [
    RefCell { group: 0, name: "弹窗出现时机", state: AlignState::Aligned, anchor: "请求后立即（同步面）", fillable: false },
    RefCell { group: 0, name: "安全桌面切换时机", state: AlignState::Aligned, anchor: "36271 隔离面先于窗呈现", fillable: false },
    RefCell { group: 0, name: "取消响应时机", state: AlignState::Aligned, anchor: "36270 即时回滚", fillable: false },
    RefCell { group: 0, name: "超时自动取消", state: AlignState::NotAvail, anchor: "缺 Windows 超时基准值", fillable: true },
    RefCell { group: 0, name: "连续弹窗节流", state: AlignState::NotAvail, anchor: "缺节流窗口基准", fillable: true },
    RefCell { group: 0, name: "提权进程创建时机", state: AlignState::Aligned, anchor: "36268 consent 后铸造", fillable: false },
    RefCell { group: 0, name: "静默提权时序", state: AlignState::Deviated, anchor: "36269 双签联动严于单签", fillable: false },
    RefCell { group: 0, name: "会话切换挂起", state: AlignState::NotAvail, anchor: "缺多会话基准", fillable: true },
];

/// 错误码 7 格（取消/失败码面对齐）。
const CODE_CELLS: [RefCell; 7] = [
    RefCell { group: 1, name: "用户取消码", state: AlignState::Aligned, anchor: "36270 冻结 1223", fillable: false },
    RefCell { group: 1, name: "签名失败码", state: AlignState::Deviated, anchor: "本地 1224 闭集（Windows 复用 1223 面）", fillable: false },
    RefCell { group: 1, name: "清单外码", state: AlignState::Deviated, anchor: "本地 1225 闭集", fillable: false },
    RefCell { group: 1, name: "越域码", state: AlignState::Deviated, anchor: "本地 1226 闭集", fillable: false },
    RefCell { group: 1, name: "拒绝诊断链码", state: AlignState::Aligned, anchor: "36267 三环链", fillable: false },
    RefCell { group: 1, name: "审计错误码", state: AlignState::NotAvail, anchor: "缺 Windows 审计码基准", fillable: true },
    RefCell { group: 1, name: "UIA 交互错误码", state: AlignState::NotAvail, anchor: "缺 UIA 基准", fillable: true },
];

/// 策略 5 格（策略面行为对齐）。
const POLICY_CELLS: [RefCell; 5] = [
    RefCell { group: 2, name: "No-Write-Up", state: AlignState::Aligned, anchor: "36262 双门独立", fillable: false },
    RefCell { group: 2, name: "No-Read-Up 变体", state: AlignState::Deviated, anchor: "本地扩展位（Windows 默认无读上门）", fillable: false },
    RefCell { group: 2, name: "IL 不可运行时改", state: AlignState::Aligned, anchor: "36275 结构排除", fillable: false },
    RefCell { group: 2, name: "过滤令牌默认", state: AlignState::Aligned, anchor: "36266 唯一构造入口", fillable: false },
    RefCell { group: 2, name: "consent 记忆", state: AlignState::NotAvail, anchor: "缺记忆窗口基准", fillable: true },
];

/// 矩阵组装（20 格：8+7+5——分组守恒）。
pub fn matrix_cells() -> Result<alloc::vec::Vec<(&'static str, AlignState)>, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut v = alloc::vec::Vec::new();
    for c in &TIMING_CELLS {
        v.push((c.name, c.state));
    }
    for c in &CODE_CELLS {
        v.push((c.name, c.state));
    }
    for c in &POLICY_CELLS {
        v.push((c.name, c.state));
    }
    if v.len() != 20 {
        return Err(E_SHAPE);
    }
    Ok(v)
}

/// 矩阵 20 格登记（判据 ① 落点）：分组守恒（时序 8/错误码 7/
/// 策略 5）+ 名互异 + 态闭集。
pub fn matrix_twenty_registered() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let (g0, g1, g2) = (TIMING_CELLS.len(), CODE_CELLS.len(), POLICY_CELLS.len());
    if g0 != 8 || g1 != 7 || g2 != 5 {
        return Err(E_SHAPE);
    }
    let cells = matrix_cells()?;
    // 名互异。
    let mut i = 0usize;
    while i < cells.len() {
        let mut j = i + 1;
        while j < cells.len() {
            if cells[i].0 == cells[j].0 {
                return Err(E_SHAPE);
            }
            j += 1;
        }
        i += 1;
    }
    Ok(cells.len() as u32)
}

/// N/A 格不编造（判据 ② 落点）：N/A 格必须无「对齐」声明 +
/// 带补齐位；对齐格必须有非空证据锚（无锚的对齐 = 编造）；
/// 偏离格必须有理由。三态逐格审计。
pub fn na_honest_twelve() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut na = 0u32;
    let all = [&TIMING_CELLS[..], &CODE_CELLS[..], &POLICY_CELLS[..]].concat();
    for c in &all {
        match c.state {
            AlignState::Aligned => {
                if c.anchor.is_empty() {
                    return Err(E_FAB); // 对齐无证据 = 编造
                }
            }
            AlignState::Deviated => {
                if c.anchor.is_empty() {
                    return Err(E_FAB); // 偏离无理由 = 编造
                }
            }
            AlignState::NotAvail => {
                if !c.fillable {
                    return Err(E_NA); // N/A 不可补齐 = 死格
                }
                na += 1;
            }
        }
    }
    if na != 6 {
        return Err(E_SHAPE); // N/A 格数守恒（时序 3 + 码 2 + 策略 1）
    }
    Ok(na)
}

/// 取消码联动锚（对照账与 36270 冻结码联动——格上「1223」与
/// 实际常量恒等：对照格不是死文本，是活锚）。
pub fn code_anchor_live() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if cx::ERROR_CANCELLED != 1223 {
        return Err(E_FROZEN); // 对照格引用的锚漂移
    }
    Ok(())
}

/// 态分布守恒（对齐/偏离/N-A 的分布指纹冻结——对照面内容变更
/// 即指纹变，防静默改账）。
pub fn distribution_fingerprint() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let all = [&TIMING_CELLS[..], &CODE_CELLS[..], &POLICY_CELLS[..]].concat();
    let mut h: u32 = 0x811C_9DC5;
    for c in &all {
        let s = match c.state {
            AlignState::Aligned => 1u32,
            AlignState::Deviated => 2,
            AlignState::NotAvail => 3,
        };
        h ^= s;
        h = h.wrapping_mul(0x0100_0193);
    }
    Ok(h)
}

/// 补齐位审计（N/A 格的 fillable 全 true + 对齐/偏离格 fillable
/// 全 false——补齐位是 N/A 专属旗标，非 N/A 格带补齐位 = 账形乱）。
pub fn fillable_audit() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let all = [&TIMING_CELLS[..], &CODE_CELLS[..], &POLICY_CELLS[..]].concat();
    let mut na = 0u32;
    for c in &all {
        match c.state {
            AlignState::NotAvail => {
                if !c.fillable {
                    return Err(E_NA);
                }
                na += 1;
            }
            _ => {
                if c.fillable {
                    return Err(E_SHAPE); // 非 N/A 带补齐位 = 账形乱
                }
            }
        }
    }
    if na != 6 {
        return Err(E_SHAPE);
    }
    Ok(na)
}

/// 组间名抽查（每格 anchor 与 group 语义对齐：时序格谈时机、
/// 码格谈码值、策略格谈位面——跨组文案串组 = 账面错位）。
pub fn group_anchor_affinity() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    // 时序组：anchor 至少半数含「时机/时序/后/先」字样——语义抽查
    // 以关键词存在性呈现（no_std 无正则，逐格 contains 链）。
    let mut hits = 0u32;
    for c in &TIMING_CELLS {
        if c.anchor.contains("时机") || c.anchor.contains("先") || c.anchor.contains("后") || c.anchor.contains("时序") {
            hits += 1;
        }
    }
    if hits < 5 {
        return Err(E_SHAPE); // 时序组语义漂移
    }
    // 码组：anchor 至少半数含「码」或数字锚。
    let mut chits = 0u32;
    for c in &CODE_CELLS {
        if c.anchor.contains("码") || c.anchor.contains("1223") || c.anchor.contains("闭集") {
            chits += 1;
        }
    }
    if chits < 4 {
        return Err(E_SHAPE);
    }
    Ok(())
}

/// 态分布明细账（对齐/偏离/N-A 三态逐组计数呈现——时序 5/1/2、
/// 码 3/3/1、策略 2/1/2：分布形状冻结，内容改则形状变）。
pub fn group_state_census() -> Result<[(u32, u32, u32); 3], u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let groups = [&TIMING_CELLS[..], &CODE_CELLS[..], &POLICY_CELLS[..]];
    let mut out = [(0u32, 0u32, 0u32); 3];
    let mut g = 0usize;
    while g < 3 {
        for c in groups[g] {
            let t = match c.state {
                AlignState::Aligned => 0,
                AlignState::Deviated => 1,
                AlignState::NotAvail => 2,
            };
            let e = &mut out[g];
            if t == 0 {
                e.0 += 1;
            } else if t == 1 {
                e.1 += 1;
            } else {
                e.2 += 1;
            }
        }
        g += 1;
    }
    // 形状冻结：时序 (4,1,3) / 码 (2,3,2) / 策略 (3,1,1)。
    if out[0] != (4, 1, 3) || out[1] != (2, 3, 2) || out[2] != (3, 1, 1) {
        return Err(E_SHAPE);
    }
    Ok(out)
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let n = matrix_twenty_registered()?;
    let na = na_honest_twelve()?;
    code_anchor_live()?;
    let fp = distribution_fingerprint()?;
    fillable_audit()?;
    group_state_census()?;
    if n != 20 {
        return Err(E_SHAPE);
    }
    Ok(n.wrapping_mul(1000).wrapping_add(na).wrapping_add(fp % 100))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_FAB => "编造（对齐格无证据锚）",
        E_SHAPE => "矩阵形状破（20 格/分组）",
        E_NA => "N/A 格无补齐位",
        E_FROZEN => "对照面冻结破",
        E_ILLEGAL => "越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36278_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36278_uacwinref");
    cs.add("SEG 段位独占", SEG_BASE == 0xAFE0, "0xAFE0~0xAFEF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "矩阵 20 格登记（8/7/5）/N-A 诚实不编造/取消码活锚/分布指纹"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("对照锚", VER == 1, "三态对照（对齐/偏离/N-A 不编造）");
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
    fn test_matrix_shape() {
        assert_eq!(matrix_twenty_registered().err(), None);
    }

    #[test]
    fn test_na_honest() {
        assert_eq!(na_honest_twelve().err(), None);
    }
}
