//! UNX-F36281-J1 越权矩阵框架（5 主体×6 客体×8 操作三维账）。
//! 判据：① 三维框架 240 格全建（主体：普通/过滤/提权/低 IL/服务；
//! 客体：文件/键/窗口站/桌面/服务/句柄；操作：读/写/执行/改权/
//! 改主/复制/模拟/等待）；② 格定义与版本锚一致；③ ≥300 行。
//! 设计纪律：越权矩阵是 B15 结果账的**框架先行**——5×6×8 = 240
//! 格全部先建（格 = 三元组坐标 + 期望裁决槽位），结果账（36282
/// 首 120 格）后续逐格填。框架面三性质：坐标闭集（越域坐标必
/// 被拒）、格互异（240 格坐标两两不同）、版本锚一致（格定义
/// 版本 = 矩阵版本 = 单一锚）。期望槽位先置「未填」态——框架
/// 不预填裁决（预填 = 编造结果账）。

use crate::checks::CheckSet;
use super::unx36261_ilevel as ilv;

pub const SEG_BASE: u16 = 0xB030;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_COORD: u16 = SEG_BASE + 2; // 坐标越域
pub const E_DUP: u16 = SEG_BASE + 3; // 格坐标重复
pub const E_SHAPE: u16 = SEG_BASE + 4; // 框架形状破（240 格）
pub const E_PREFILL: u16 = SEG_BASE + 5; // 预填裁决（框架越权）
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 框架版本锚（格定义版本——36282 结果账消费同一锚）。
pub const FRAME_VER: u32 = 1;

/// 主体闭集（5 态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Subject {
    Normal,     // 普通用户令牌
    Filtered,   // 过滤令牌
    Elevated,   // 提权完整令牌
    LowIl,      // 低 IL 令牌
    Service,    // 服务令牌
}

/// 客体闭集（6 态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Object {
    File,
    RegistryKey,
    WindowStation,
    Desktop,
    Service,
    Handle,
}

/// 操作闭集（8 态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Read,
    Write,
    Execute,
    ChangeAcl,
    ChangeOwner,
    Duplicate,
    Impersonate,
    Wait,
}

pub const SUBJECT_N: usize = 5;
pub const OBJECT_N: usize = 6;
pub const ACTION_N: usize = 8;
pub const CELL_N: usize = SUBJECT_N * OBJECT_N * ACTION_N; // 240

/// 越权格（三元组坐标 + 期望槽位——框架面恒「未填」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    pub s: u8, // 主体序（0..5）
    pub o: u8, // 客体序（0..6）
    pub a: u8, // 操作序（0..8）
    pub filled: bool,
    pub verdict: u8, // 0=未填 1=许 2=拒（结果账填）
}

/// 坐标合法校验（闭集守卫：任一维越域即拒）。
pub fn coord_ok(s: u8, o: u8, a: u8) -> bool {
    (s as usize) < SUBJECT_N && (o as usize) < OBJECT_N && (a as usize) < ACTION_N
}

/// 框架构建（240 格全建：三重循环铺满——格坐标互异由构造保证，
/// 再两两复核防构造器缺陷）。
pub fn build_frame() -> Result<alloc::vec::Vec<Cell>, u16> {
    if VER != 1 || FRAME_VER != 1 {
        return Err(E_VER);
    }
    let mut v = alloc::vec::Vec::new();
    let mut s = 0u8;
    while (s as usize) < SUBJECT_N {
        let mut o = 0u8;
        while (o as usize) < OBJECT_N {
            let mut a = 0u8;
            while (a as usize) < ACTION_N {
                v.push(Cell { s, o, a, filled: false, verdict: 0 });
                a += 1;
            }
            o += 1;
        }
        s += 1;
    }
    if v.len() != CELL_N {
        return Err(E_SHAPE);
    }
    Ok(v)
}

/// 框架 240 格全建 + 三性质复核（判据 ① 落点）：格数守恒、坐标
/// 互异（两两复核）、全格「未填」态（框架不预填——预填 = 结果
/// 账编造）。
pub fn frame_240() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let v = build_frame()?;
    // 坐标互异（两两复核：240² /2 ≈ 28k 次——no_std 可承受）。
    let mut i = 0usize;
    while i < v.len() {
        let mut j = i + 1;
        while j < v.len() {
            if v[i].s == v[j].s && v[i].o == v[j].o && v[i].a == v[j].a {
                return Err(E_DUP);
            }
            j += 1;
        }
        i += 1;
    }
    // 全格未填（预填检出）。
    for c in &v {
        if c.filled || c.verdict != 0 {
            return Err(E_PREFILL);
        }
    }
    Ok(v.len() as u32)
}

/// 坐标越域注入（闭集守卫实证：三型越域坐标全部被 coord_ok 拒
/// ——主体越界/客体越界/操作越界各 4 例 = 12 例）。
pub fn coord_reject_twelve() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut rejected = 0u32;
    // 主体越界（5..8）。
    let mut s = SUBJECT_N as u8;
    while (s as usize) < SUBJECT_N + 4 {
        if coord_ok(s, 0, 0) {
            return Err(E_COORD);
        }
        rejected += 1;
        s += 1;
    }
    // 客体越界（6..9）。
    let mut o = OBJECT_N as u8;
    while (o as usize) < OBJECT_N + 4 {
        if coord_ok(0, o, 0) {
            return Err(E_COORD);
        }
        rejected += 1;
        o += 1;
    }
    // 操作越界（8..11）。
    let mut a = ACTION_N as u8;
    while (a as usize) < ACTION_N + 4 {
        if coord_ok(0, 0, a) {
            return Err(E_COORD);
        }
        rejected += 1;
        a += 1;
    }
    if rejected != 12 {
        return Err(E_COORD);
    }
    Ok(rejected)
}

/// 格查询（坐标 → 槽位：合法坐标必命中且唯一——框架读口）。
pub fn cell_at(frame: &[Cell], s: u8, o: u8, a: u8) -> Result<Cell, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if !coord_ok(s, o, a) {
        return Err(E_COORD);
    }
    for c in frame {
        if c.s == s && c.o == o && c.a == a {
            return Ok(*c);
        }
    }
    Err(E_SHAPE) // 合法坐标未命中 = 框架缺格
}

/// 全格可查（240 坐标逐一命中——框架覆盖完备性：任何合法坐标
/// 都有格，无空洞）。
pub fn all_cells_queryable(frame: &[Cell]) -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut n = 0u32;
    let mut s = 0u8;
    while (s as usize) < SUBJECT_N {
        let mut o = 0u8;
        while (o as usize) < OBJECT_N {
            let mut a = 0u8;
            while (a as usize) < ACTION_N {
                let c = cell_at(frame, s, o, a)?;
                if c.filled {
                    return Err(E_PREFILL);
                }
                n += 1;
                a += 1;
            }
            o += 1;
        }
        s += 1;
    }
    if n != CELL_N as u32 {
        return Err(E_SHAPE);
    }
    Ok(n)
}

/// 主体-IL 映射锚（五主体到 ilv 档的冻结映射：普通=Medium、
/// 过滤=Medium、提权=High、低 IL=Low、服务=High——框架面与
/// 档位真源一致，36282 结果账按此映射取主体档）。
pub fn subject_il_map(s: Subject) -> ilv::Il {
    match s {
        Subject::Normal => ilv::Il::Medium,
        Subject::Filtered => ilv::Il::Medium,
        Subject::Elevated => ilv::Il::High,
        Subject::LowIl => ilv::Il::Low,
        Subject::Service => ilv::Il::High,
    }
}

/// 映射对平（映射表互异面：过滤与普通同档但类型互异——映射
/// 值核对 + 操作闭集 8 态互异复核）。
pub fn map_anchor() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let subs = [
        Subject::Normal,
        Subject::Filtered,
        Subject::Elevated,
        Subject::LowIl,
        Subject::Service,
    ];
    let wants = [
        ilv::Il::Medium,
        ilv::Il::Medium,
        ilv::Il::High,
        ilv::Il::Low,
        ilv::Il::High,
    ];
    let mut i = 0usize;
    while i < SUBJECT_N {
        if subject_il_map(subs[i]) != wants[i] {
            return Err(E_SHAPE);
        }
        i += 1;
    }
    // 操作 8 态判别式互异（discriminant 面复核）。
    let acts = [
        Action::Read,
        Action::Write,
        Action::Execute,
        Action::ChangeAcl,
        Action::ChangeOwner,
        Action::Duplicate,
        Action::Impersonate,
        Action::Wait,
    ];
    let mut a = 0usize;
    while a < ACTION_N {
        let mut b = a + 1;
        while b < ACTION_N {
            if core::mem::discriminant(&acts[a]) == core::mem::discriminant(&acts[b]) {
                return Err(E_SHAPE);
            }
            b += 1;
        }
        a += 1;
    }
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let n = frame_240()?;
    let rej = coord_reject_twelve()?;
    let frame = build_frame()?;
    let q = all_cells_queryable(&frame)?;
    map_anchor()?;
    if n != 240 || rej != 12 || q != 240 {
        return Err(E_SHAPE);
    }
    Ok(n)
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_COORD => "坐标越域",
        E_DUP => "格坐标重复",
        E_SHAPE => "框架形状破（240 格）",
        E_PREFILL => "预填裁决（框架越权）",
        E_ILLEGAL => "越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36281_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36281_xmatrix");
    cs.add("SEG 段位独占", SEG_BASE == 0xB030, "0xB030~0xB03F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "240 格全建互异未填/越域 12 例拒/全格可查/映射与闭集锚"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("框架锚", FRAME_VER == 1 && CELL_N == 240, "5×6×8 = 240 格（36282 结果账消费）");
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
    fn test_frame() {
        let f = build_frame().unwrap_or(alloc::vec![]);
        assert!(f.len() == 240);
        assert!(all_cells_queryable(&f).is_ok());
    }

    #[test]
    fn test_coord_reject() {
        assert!(!coord_ok(5, 0, 0));
        assert!(!coord_ok(0, 6, 0));
        assert!(!coord_ok(0, 0, 8));
        assert!(coord_ok(4, 5, 7));
    }
}
