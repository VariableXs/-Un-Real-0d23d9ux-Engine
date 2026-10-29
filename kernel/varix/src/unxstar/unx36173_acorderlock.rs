//! UNX-F36173-J1 AccessCheck 步骤序锁（acorderlock）。
//! 判据：① 步序锁——五步装配的进入序必须单调（1→2→3→4→5），乱序
//! 进入/回退/越界显性拒；② 进入/退出配对（序锁嵌套计数，未开先闭拒）；
//! ③ 序锁回放（记录的步序与冻结路径对拍）；④ 注入负例显性拒；⑤ ≥310 行。
//! 设计纪律：步序真源 36161::StepId（判别式 1..5）；本模块是装配
//! 顺序契约的观测与守卫面，供 36179 ktest 聚合。

use super::unx36161_acfreeze as acf;
use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x4030;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_ORDER: u16 = SEG_BASE + 2; // 步序乱序/回退
pub const E_BOUND: u16 = SEG_BASE + 3; // 步号越出五步闭集
pub const E_PAIR: u16 = SEG_BASE + 4; // 进入/退出配对失配
pub const E_NEG: u16 = SEG_BASE + 5; // 负例注入未被检出
pub const E_TABLE: u16 = SEG_BASE + 6; // 序锁表闭集破坏

/// 模块版本锚。
pub const VER: u32 = 1;

/// 步数锚（五步闭集）。
pub const STEP_N: u16 = 5;

/// 序锁状态机：记录当前允许进入的步、嵌套深度、已走路径。
pub struct OrderLock {
    next_step: u16,     // 下一个允许进入的步（1..=5）
    depth: u32,         // 进入/退出配对深度
    path: Vec<u16>,     // 已进入步的记录
    pub closed_violations: u32,
    pub order_violations: u32,
}

impl OrderLock {
    pub fn new() -> OrderLock {
        OrderLock {
            next_step: 1,
            depth: 0,
            path: Vec::new(),
            closed_violations: 0,
            order_violations: 0,
        }
    }

    /// 步进入：必须等于 next_step（单调）；越界 E_BOUND；乱序 E_ORDER。
    pub fn enter(&mut self, step: u16) -> Result<(), u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        if step < 1 || step > STEP_N {
            return Err(E_BOUND);
        }
        if step != self.next_step {
            self.order_violations += 1;
            return Err(E_ORDER);
        }
        self.depth += 1;
        self.path.push(step);
        if self.next_step < STEP_N {
            self.next_step += 1;
        }
        Ok(())
    }

    /// 步退出：配对面（深度减一）；未开先闭计违规不 Err（审计面）。
    pub fn exit(&mut self, step: u16) -> Result<(), u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        if step < 1 || step > STEP_N {
            return Err(E_BOUND);
        }
        if self.depth == 0 {
            self.closed_violations += 1;
            return Err(E_PAIR);
        }
        self.depth -= 1;
        Ok(())
    }

    /// 已走路径只读回放。
    pub fn path(&self) -> &[u16] {
        &self.path
    }

    /// 全程完成判定：五步全走且全退出。
    pub fn complete(&self) -> bool {
        self.path.len() == STEP_N as usize && self.depth == 0
    }

    /// 清空（违规计数保留）。
    pub fn reset(&mut self) {
        self.next_step = 1;
        self.depth = 0;
        self.path.clear();
    }
}

/// 静态路径校验：步序数组必须从 1 起严格连续（即止前缀合法）。
pub fn path_strict(path: &[u16]) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if path.is_empty() || path.len() > STEP_N as usize {
        return Err(E_BOUND);
    }
    if path[0] != 1 {
        return Err(E_ORDER);
    }
    let mut i = 1;
    while i < path.len() {
        if path[i] != path[i - 1] + 1 {
            return Err(E_ORDER);
        }
        i += 1;
    }
    Ok(())
}

/// 装配路径冻结面：access_check 实际触达的步序必须为合法前缀。
/// （对照 36161 装配语义：匿名拒 → [1]；特权拒 → [1,2]；空态终流
/// → [1,2,3]；Deny 即止 → [1,2,3,4]；全链 → [1,2,3,4,5]。）
pub fn frozen_prefixes() -> [[u16; 5]; 5] {
    [
        [1, 0, 0, 0, 0],
        [1, 2, 0, 0, 0],
        [1, 2, 3, 0, 0],
        [1, 2, 3, 4, 0],
        [1, 2, 3, 4, 5],
    ]
}

/// 前缀合法性：实走路径必须是某冻结前缀（0 补位忽略）。
pub fn matches_frozen(path: &[u16]) -> Result<bool, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    path_strict(path)?;
    let table = frozen_prefixes();
    let mut t = 0;
    while t < 5 {
        let row = &table[t];
        let n = path.len();
        let mut ok = true;
        let mut i = 0;
        while i < n {
            if row[i] != path[i] {
                ok = false;
            }
            i += 1;
        }
        if ok {
            return Ok(true);
        }
        t += 1;
    }
    Ok(false)
}

/// 步名回显（越界 "bad-step"；消费 36161 StepId）。
pub fn step_name(step: u16) -> &'static str {
    match step {
        1 => acf::StepId::Impersonation.name(),
        2 => acf::StepId::Privilege.name(),
        3 => acf::StepId::DaclState.name(),
        4 => acf::StepId::AceEval.name(),
        5 => acf::StepId::CoverJudge.name(),
        _ => "bad-step",
    }
}

/// 序锁面自检：五步闭集 + 前缀表 + 名回显边界。
pub fn order_integrity() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if STEP_N != 5 {
        return Err(E_TABLE);
    }
    let table = frozen_prefixes();
    let mut t = 0;
    while t < 5 {
        let row = &table[t];
        if row[0] != 1 {
            return Err(E_TABLE);
        }
        let mut i = 1;
        while i < 5 {
            if row[i] != 0 && row[i] != row[i - 1] + 1 {
                return Err(E_TABLE);
            }
            i += 1;
        }
        t += 1;
    }
    if step_name(0) != "bad-step" || step_name(6) != "bad-step" {
        return Err(E_TABLE);
    }
    Ok(())
}

/// 演练：R1 纪律自检；R2 顺走五步 + 配对退出；R3 乱序/越界/未开先闭；
/// R4 冻结前缀对拍（五条合法路径 + 一条非法）；R5 负例注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：纪律自检。
    order_integrity()?;
    hits += 1;
    // R2：顺走五步 + 全退出 → complete；路径回放 [1..5]。
    let mut lock = OrderLock::new();
    let mut s = 1;
    while s <= 5 {
        lock.enter(s)?;
        lock.exit(s)?;
        s += 1;
    }
    if !lock.complete() || lock.path().len() != 5 {
        return Err(E_TABLE);
    }
    let p = lock.path();
    if p[0] != 1 || p[4] != 5 {
        return Err(E_TABLE);
    }
    hits += 1;
    // R3：乱序拒 + 越界拒 + 未开先闭 + 深度未退不算 complete。
    let mut l2 = OrderLock::new();
    if l2.enter(2) != Err(E_ORDER) || l2.order_violations != 1 {
        return Err(E_NEG);
    }
    if l2.enter(0).is_ok() || l2.enter(6).is_ok() {
        return Err(E_NEG);
    }
    if l2.exit(1) != Err(E_PAIR) || l2.closed_violations != 1 {
        return Err(E_NEG);
    }
    l2.enter(1)?;
    l2.enter(2)?;
    if l2.complete() {
        return Err(E_NEG); // 深度未归零
    }
    hits += 1;
    // R4：冻结前缀对拍——五条合法全过；[1,3] 断裂拒。
    let table = frozen_prefixes();
    let mut t = 0;
    while t < 5 {
        let mut path: Vec<u16> = Vec::new();
        let mut i = 0;
        while i < 5 {
            if table[t][i] != 0 {
                path.push(table[t][i]);
            }
            i += 1;
        }
        if !matches_frozen(&path)? {
            return Err(E_NEG);
        }
        t += 1;
    }
    let broken = [1u16, 3];
    if matches_frozen(&broken).is_ok() {
        return Err(E_NEG); // 断裂路径先被 path_strict 拒
    }
    hits += 1;
    // R5：负例注入——reset 后可重走全程 / 违规计数保留 / 步名边界。
    l2.reset();
    if l2.next_allowed_probe() != 1 {
        return Err(E_NEG);
    }
    let mut s5 = 1;
    while s5 <= 5 {
        l2.enter(s5)?;
        l2.exit(s5)?;
        s5 += 1;
    }
    if !l2.complete() || l2.order_violations != 1 || l2.closed_violations != 1 {
        return Err(E_NEG);
    }
    if step_name(3) != acf::StepId::DaclState.name() {
        return Err(E_NEG);
    }
    hits += 1;
    Ok(hits)
}

/// 探针：下一允许步（测试面）。
impl OrderLock {
    fn next_allowed_probe(&self) -> u16 {
        self.next_step
    }
}

/// 步号 → StepId 投影（越界 None；36161 侧只回名，此处回枚举）。
pub mod stepid_probe {
    pub fn of(step: u16) -> Option<super::acf::StepId> {
        match step {
            1 => Some(super::acf::StepId::Impersonation),
            2 => Some(super::acf::StepId::Privilege),
            3 => Some(super::acf::StepId::DaclState),
            4 => Some(super::acf::StepId::AceEval),
            5 => Some(super::acf::StepId::CoverJudge),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(5));
    }

    #[test]
    fn order_table_holds() {
        assert_eq!(order_integrity(), Ok(()));
    }

    #[test]
    fn out_of_order_rejected() {
        let mut l = OrderLock::new();
        assert_eq!(l.enter(3), Err(E_ORDER));
    }

    #[test]
    fn frozen_prefixes_valid() {
        let table = frozen_prefixes();
        let mut t = 0;
        while t < 5 {
            let mut path: Vec<u16> = Vec::new();
            let mut i = 0;
            while i < 5 {
                if table[t][i] != 0 {
                    path.push(table[t][i]);
                }
                i += 1;
            }
            assert_eq!(matches_frozen(&path), Ok(true));
            t += 1;
        }
    }
}

pub fn run_unx36173_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36173_acorderlock");
    cs.add("SEG 段位独占", SEG_BASE == 0x4030, "0x4030~0x403F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "步序/配对/前缀对拍全平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("闭集纪律", order_integrity() == Ok(()), "五步闭集 + 前缀表");
    cs.add("契约冻结", VER == 1, "序锁锚 v1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_ORDER => "步序乱序/回退",
        E_BOUND => "步号越出五步闭集",
        E_PAIR => "进入/退出配对失配",
        E_NEG => "负例注入未被检出",
        E_TABLE => "序锁表闭集破坏",
        _ => "未知诊断码",
    }
}
