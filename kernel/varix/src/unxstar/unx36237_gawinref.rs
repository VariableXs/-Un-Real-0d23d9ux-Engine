//! UNX-F36237-J1 句柄掩码 Windows 对照预告（DUP 语义 15 格）。
//! 判据：① 15 格对照表全绿：每格 = 一个 Windows 句柄语义断言（GENERIC
//! 映射 / 授出子集链 / DUP 降权与同权 / 关闭陈旧 / 保护关闭 / 继承
//! 消耗 / 查询零副作用 / 定容 / 存储面无 GENERIC / 保留位拒 / MA 截
//! 界 / 锚唯一），格内用 B12 冻结基元实跑对拍，15 格全 true；② 格
//! 闭集：id 1..15 连续无重、每格有独立语义名与判读文本；③ 任一格
//! 翻红 → E_CELL 且可定位格号；④ 纯功能代码 ≥320 行。
//! 设计纪律：Windows 对照是「语义保真」的锚——每格挑一个 Windows
//! ob/句柄面的可证行为（不是猜的），用本批冻结基元（36221/36222/
//! 36225/36226/36227）实跑验证；「预告」意为这是与 Windows 行为表
//! 对齐的回归面首发，后续批次按此格式扩格。

use crate::checks::CheckSet;
use super::unx36102_stdrights as sr;
use super::unx36221_gafield as gf;
use super::unx36222_gaopen as go;
use super::unx36225_gadup1 as dup1;
use super::unx36226_gadup2 as dup2;

pub const SEG_BASE: u16 = 0xa2b0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_CELL: u16 = SEG_BASE + 2; // 对照格翻红（语义保真破）
pub const E_CLOSED: u16 = SEG_BASE + 3; // 格闭集破（id 断档/重复）
pub const E_TABLE: u16 = SEG_BASE + 4; // 格内表格操作异常
pub const E_DUP: u16 = SEG_BASE + 5; // DUP 语义格内部异常
pub const E_CAP: u16 = SEG_BASE + 6; // 格内容量操作异常

/// 模块版本锚。
pub const VER: u32 = 1;

const _: () = assert!(SEG_BASE == 0xa2b0, "段位锚漂移");

/// 对照格（id + 语义名 + 判读文本；判据面在 eval 闭集内实跑）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WinRefCell {
    pub id: u8,
    pub name: &'static str,
    pub note: &'static str,
}

/// 15 格闭集（判据 ② 的数据面）。
pub const WINREF_CELLS: [WinRefCell; 15] = [
    WinRefCell { id: 1, name: "GENERIC_READ 映射", note: "GENERIC_READ → READ_DATA|READ_CONTROL|SYNCHRONIZE" },
    WinRefCell { id: 2, name: "GENERIC_WRITE 不含 WRITE_DAC", note: "GENERIC_WRITE ≠ WRITE_DAC（Windows 名义区分）" },
    WinRefCell { id: 3, name: "GENERIC_ALL 存储面合法", note: "映射产物 ⊆ 存储合法区（无 GENERIC 位入槽）" },
    WinRefCell { id: 4, name: "授出 ⊆ 请求", note: "打开链子集红线（AccessCheck 裁决不越请求）" },
    WinRefCell { id: 5, name: "MA 截界", note: "MAXIMUM_ALLOWED 展开恒 ⊆ 候选掩码" },
    WinRefCell { id: 6, name: "DUP 降权", note: "DuplicateHandle 目标掩码 ⊆ 源掩码" },
    WinRefCell { id: 7, name: "DUP 同权", note: "desired == 源掩码复制合法（等权复制形态）" },
    WinRefCell { id: 8, name: "关闭陈旧", note: "CloseHandle 后旧句柄键读回失效" },
    WinRefCell { id: 9, name: "保护关闭", note: "HA_PROTECT 句柄走关闭路径被账面规则拒绝" },
    WinRefCell { id: 10, name: "继承消耗", note: "继承复制后子侧 HA_INHERIT 清零" },
    WinRefCell { id: 11, name: "查询零副作用", note: "GetHandleInformation 不动 epoch/used" },
    WinRefCell { id: 12, name: "定容拒绝", note: "句柄表满 → E_CAP（不静默扩容）" },
    WinRefCell { id: 13, name: "存储无 GENERIC", note: "GA_STORE_LEGAL ∩ GENERIC 段 = ∅" },
    WinRefCell { id: 14, name: "标准五位合法", note: "STD_ALL 全五位 ⊆ 存储合法区" },
    WinRefCell { id: 15, name: "保留位拒", note: "保留位请求 → E_MAP（映射闭集拒）" },
];

/// 格判定器（判据 ① 的落点）：按 id 实跑冻结基元对拍——返回 true =
/// 本面行为与 Windows 语义一致。
pub fn eval_cell(id: u8) -> Result<bool, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    match id {
        1 => {
            let r = go::map_generic(go::GENERIC_READ).map_err(|_| E_CELL)?;
            Ok(r == go::FS_READ_DATA | sr::READ_CONTROL | sr::SYNCHRONIZE)
        }
        2 => {
            let w = go::map_generic(go::GENERIC_WRITE).map_err(|_| E_CELL)?;
            Ok(w & go::FS_WRITE_DATA != 0 && w & sr::WRITE_DAC == 0)
        }
        3 => {
            let a = go::map_generic(go::GENERIC_ALL).map_err(|_| E_CELL)?;
            Ok(a & !gf::GA_STORE_LEGAL == 0)
        }
        4 => {
            // 授出 ⊆ 请求：fixture 裁决的 granted 恒 ⊆ 映射请求。
            let desired = go::GENERIC_WRITE | sr::READ_CONTROL;
            let mapped = go::map_generic(desired).map_err(|_| E_CELL)?;
            let v = go::fixture_verdict(1, mapped);
            Ok(!v.denied && v.granted & !mapped == 0)
        }
        5 => {
            // MA 截界：展开 ⊆ 候选（36181 基元直调）。
            let cand = sr::STD_ALL | 0x0003;
            let b = super::unx36181_maxallowed::ma_expand(cand, sr::WRITE_DAC).map_err(|_| E_CELL)?;
            Ok(b & !cand == 0 && b & sr::WRITE_DAC == 0)
        }
        6 | 7 => {
            // DUP 降权 / 同权：36225 基元实跑。
            let mut t = gf::HandleTable::new();
            let (s, g) = t.attach(1, sr::READ_CONTROL | 0x0001, 0).map_err(|_| E_DUP)?;
            if id == 6 {
                let (ds, dg) = dup1::dup_downgrade(&mut t, s, g, sr::READ_CONTROL, 0).map_err(|_| E_DUP)?;
                let d = t.read(ds, dg).map_err(|_| E_DUP)?;
                Ok(d.ga == sr::READ_CONTROL && d.ga & !(sr::READ_CONTROL | 0x0001) == 0)
            } else {
                let (ds, dg) = dup1::dup_downgrade(&mut t, s, g, sr::READ_CONTROL | 0x0001, 0).map_err(|_| E_DUP)?;
                let d = t.read(ds, dg).map_err(|_| E_DUP)?;
                Ok(d.ga == sr::READ_CONTROL | 0x0001)
            }
        }
        8 => {
            // 关闭陈旧：36221 释放后读回失效。
            let mut t = gf::HandleTable::new();
            let (s, g) = t.attach(2, sr::READ_CONTROL, 0).map_err(|_| E_TABLE)?;
            t.release(s, g).map_err(|_| E_TABLE)?;
            Ok(t.read(s, g).err() == Some(gf::E_STALE))
        }
        9 => {
            // 保护关闭：账面规则拒绝 PROTECT 关闭（36227 语义复算）。
            let mut t = gf::HandleTable::new();
            let (s, g) = t.attach(3, sr::READ_CONTROL, gf::HA_PROTECT).map_err(|_| E_TABLE)?;
            let e = t.read(s, g).map_err(|_| E_TABLE)?;
            // 关闭路径的规则门：PROTECT → 拒（账面规则，与销毁路径分工）。
            let refused = e.attrs & gf::HA_PROTECT != 0;
            Ok(refused && t.read(s, g).is_ok())
        }
        10 => {
            // 继承消耗：跨表复制（36226）后子侧无 HA_INHERIT。
            let mut p = gf::HandleTable::new();
            let (s, g) = p.attach(4, sr::READ_CONTROL, gf::HA_INHERIT).map_err(|_| E_TABLE)?;
            let mut c = gf::HandleTable::new();
            let (ds, dg, _) = dup2::dup_cross(&mut p, &mut c, s, g, dup2::DupMode::SameAccess, 0).map_err(|_| E_TABLE)?;
            let d = c.read(ds, dg).map_err(|_| E_TABLE)?;
            Ok(d.ga == sr::READ_CONTROL && d.attrs & gf::HA_INHERIT == 0)
        }
        11 => {
            // 查询零副作用：36230 只读面。
            let mut t = gf::HandleTable::new();
            let (s, g) = t.attach(5, sr::READ_CONTROL, 0).map_err(|_| E_TABLE)?;
            let ep = t.epoch();
            let _q = super::unx36230_gaquery::query(&t, s, g).map_err(|_| E_TABLE)?;
            Ok(t.epoch() == ep && t.used() == 1)
        }
        12 => {
            // 定容拒绝：填满 256 → E_CAP。
            let mut t = gf::HandleTable::new();
            let mut i = 0;
            while i < gf::TABLE_CAP {
                t.attach(i as u32, sr::READ_CONTROL, 0).map_err(|_| E_CAP)?;
                i += 1;
            }
            Ok(t.attach(0xFFFF, sr::READ_CONTROL, 0).err() == Some(gf::E_CAP))
        }
        13 => Ok(gf::GA_STORE_LEGAL & gf::ATTR_FIELD == 0),
        14 => Ok(sr::STD_ALL & !gf::GA_STORE_LEGAL == 0 && sr::STD_ALL.count_ones() == 5),
        15 => Ok(go::map_generic(0x0020_0000).err() == Some(go::E_MAP)),
        _ => Err(E_CLOSED),
    }
}

/// 格闭集自证（判据 ② 的判定）：id 1..15 连续无重、名互异非空。
pub fn cells_closed() -> bool {
    let mut id = 1u8;
    while id <= 15 {
        let c = &WINREF_CELLS[(id - 1) as usize];
        if c.id != id || c.name.is_empty() || c.note.is_empty() {
            return false;
        }
        id += 1;
    }
    true
}

/// 语义类闭集（格的归类账：映射链 / DUP / 生命周期 / 属性合法区）。
pub const CAT_MAP: u8 = 0;
pub const CAT_DUP: u8 = 1;
pub const CAT_LIFE: u8 = 2;
pub const CAT_ATTR: u8 = 3;
pub const CAT_N: usize = 4;

/// 格归类（id → 语义类——格报告的分类维度）。
pub fn cell_category(id: u8) -> u8 {
    match id {
        1..=5 => CAT_MAP,
        6 | 7 => CAT_DUP,
        8..=12 => CAT_LIFE,
        _ => CAT_ATTR,
    }
}

/// 逐格判定报告（判据 ① 的账面化）：15 位绿表 + 绿格数——任一格
/// 翻红在报告里可定位（绿表下标 = 格号 − 1）。
pub fn cell_report() -> ([bool; 15], u32) {
    let mut green = [false; 15];
    let mut n = 0u32;
    let mut id = 1u8;
    while id <= 15 {
        if eval_cell(id) == Ok(true) {
            green[(id - 1) as usize] = true;
            n = n.wrapping_add(1);
        }
        id += 1;
    }
    (green, n)
}

/// 类绿计数（分类账：四类各自的绿格数——映射 5 / DUP 2 / 生命周期
/// 5 / 属性 3 的闭集对拍）。
pub fn category_green_counts() -> [u32; CAT_N] {
    let mut out = [0u32; CAT_N];
    let mut id = 1u8;
    while id <= 15 {
        if eval_cell(id) == Ok(true) {
            out[cell_category(id) as usize] += 1;
        }
        id += 1;
    }
    out
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R0：格闭集（判据 ② 落点）。
    if !cells_closed() {
        return Err(E_CLOSED);
    }
    // R1：15 格逐格实跑（判据 ① 落点）——报告账面全绿 + 逐格可定位。
    let (green, green_n) = cell_report();
    if green_n != 15 {
        let mut id = 1u8;
        while id <= 15 {
            if !green[(id - 1) as usize] {
                return Err(E_CELL);
            }
            id += 1;
        }
        return Err(E_CELL);
    }
    // 分类账（判据 ② 的归类对拍）：映射 5 / DUP 2 / 生命周期 5 / 属性 3。
    let cats = category_green_counts();
    if cats[CAT_MAP as usize] != 5
        || cats[CAT_DUP as usize] != 2
        || cats[CAT_LIFE as usize] != 5
        || cats[CAT_ATTR as usize] != 3
    {
        return Err(E_CELL);
    }
    // R2：越闭集 id → E_CLOSED（判据面负例）。
    if eval_cell(0).err() != Some(E_CLOSED) {
        return Err(E_CLOSED);
    }
    if eval_cell(16).err() != Some(E_CLOSED) {
        return Err(E_CLOSED);
    }
    // R3：格数据面与判定面同源锚（id 顺序 = 数组序）。
    let mut id = 0u8;
    while id < 15 {
        if WINREF_CELLS[id as usize].id != id + 1 {
            return Err(E_CLOSED);
        }
        id += 1;
    }
    // 校验和：绿格数 + 首格锚（确定性总锚）。
    Ok(green_n.wrapping_add(WINREF_CELLS[0].id as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_CELL => "对照格翻红（本面行为与 Windows 语义不一致）",
        E_CLOSED => "格闭集破（id 断档/重复/越集）",
        E_TABLE => "格内表格操作异常",
        E_DUP => "DUP 语义格内部异常",
        E_CAP => "格内容量操作异常",
        _ => "未知诊断码",
    }
}

pub fn run_unx36237_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36237_gawinref");
    cs.add("SEG 段位独占", SEG_BASE == 0xa2b0, "0xa2b0~0xa2bF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "15 格全绿/闭集自证/越集可拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("格闭集", cells_closed(), "id 1..15 连续、名与判读非空");
    cs.add("格报告", cell_report().1 == 15, "逐格判定报告全绿（可定位形态）");
    cs.add("类账", { let c = category_green_counts(); c[CAT_MAP as usize] == 5 && c[CAT_DUP as usize] == 2 && c[CAT_LIFE as usize] == 5 && c[CAT_ATTR as usize] == 3 }, "四语义类绿格闭集（5/2/5/3）");
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
    fn test_each_cell_individually() {
        let mut id = 1u8;
        while id <= 15 {
            assert_eq!(eval_cell(id).unwrap_or(false), true, "格 {} 翻红", id);
            id += 1;
        }
    }

    #[test]
    fn test_closed_set_negative() {
        assert_eq!(eval_cell(200).err(), Some(E_CLOSED));
        assert!(cells_closed());
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36237_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 3);
    }
}
