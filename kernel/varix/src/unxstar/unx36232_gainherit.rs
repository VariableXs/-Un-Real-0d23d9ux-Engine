//! UNX-F36232-J1 句柄继承标志（子进程继承句柄清单语义）。
//! 判据：① 继承清单 = slots_with_attr(HA_INHERIT)——30 例父表投影：
//! 挂继承标志的句柄进清单、未挂的不进（清单与 36221 属性过滤原语
//! 对拍一致）；② 子表复制：继承例 ga 逐位一致（Windows 继承不裁权
//! ——裁权走复制降权面）、obj 恒等、继承标志在子侧消耗（HA_INHERIT
//! 清零、其余属性保留）、槽序与父表一致；③ 继承零副作用：父表
//! epoch/used 恒不变（继承是复制不是移动）；④ 纯功能代码 ≥320 行。
//! 设计纪律：Windows CreateProcess bInheritHandles 语义——只有
//! HANDLE_FLAG_INHERIT 挂标的句柄进子进程句柄表，掩码原样搬运；
//! 本面是该语义的账面化：清单投影 + 表间复制 + 标志消耗三步，全部
//! 以 36221 冻结接口表达，不造第二套属性判定。

use crate::checks::CheckSet;
use super::unx36102_stdrights as sr;
use super::unx36221_gafield as gf;

pub const SEG_BASE: u16 = 0xa240;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_LIST: u16 = SEG_BASE + 2; // 继承清单破（投影与属性过滤不符）
pub const E_COPY: u16 = SEG_BASE + 3; // 复制破（掩码/obj/属性/槽序不符）
pub const E_SIDE: u16 = SEG_BASE + 4; // 副作用破（父表被继承污染）
pub const E_CAP: u16 = SEG_BASE + 5; // 子表满（继承例数超容）
pub const E_FLAG: u16 = SEG_BASE + 6; // 标志消耗破（子侧仍带 INHERIT）

/// 模块版本锚。
pub const VER: u32 = 1;

const _: () = assert!(SEG_BASE == 0xa240, "段位锚漂移");

/// 继承清单（判据 ① 的落点）：36221 属性过滤原语直调——本面零复述
/// 属性判定。返回升序槽位集。
pub fn inherit_list(t: &gf::HandleTable) -> alloc::vec::Vec<usize> {
    t.slots_with_attr(gf::HA_INHERIT)
}

/// 清单自证（判据 ① 的对拍核）：清单内槽必挂标、清单外活槽必不挂标。
pub fn list_consistent(t: &gf::HandleTable, list: &alloc::vec::Vec<usize>) -> bool {
    let mut slot = 0;
    let mut idx = 0usize;
    while slot < gf::TABLE_CAP {
        if let Some(e) = t.entry_at(slot) {
            if !gf::is_free_entry(&e) {
                let in_list = idx < list.len() && list[idx] == slot;
                if (e.attrs & gf::HA_INHERIT != 0) != in_list {
                    return false;
                }
                if in_list {
                    idx += 1;
                }
            }
        }
        slot += 1;
    }
    idx == list.len()
}

/// 可继承清单（规则面：HA_PRESET 预设标记槽不继承——清单的规则过
/// 滤；继承清单是属性投影，可继承清单是规则过滤后的复制名单）。
pub fn inheritable_list(t: &gf::HandleTable) -> alloc::vec::Vec<usize> {
    let list = inherit_list(t);
    let mut out = alloc::vec::Vec::new();
    let mut i = 0;
    while i < list.len() {
        let e = t.entry_at(list[i]).unwrap_or(gf::Entry { obj: 0, ga: 0, attrs: 0, gen: 0, auth_epoch: 0 });
        if e.attrs & gf::HA_PRESET == 0 {
            out.push(list[i]);
        }
        i += 1;
    }
    out
}

/// 子表生成（判据 ② 的落点）：可继承清单逐槽复制到新表。
/// ga 逐位搬运（继承不裁权）；obj 恒等；HA_INHERIT 消耗（子侧清零），
/// 其余属性原样保留；槽序 = 父表升序（清单顺序即复制顺序）。
pub fn spawn_child(parent: &gf::HandleTable) -> Result<(gf::HandleTable, usize), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let list = inheritable_list(parent);
    if list.len() > gf::TABLE_CAP {
        return Err(E_CAP);
    }
    let mut child = gf::HandleTable::new();
    let mut i = 0;
    while i < list.len() {
        let e = parent.read(list[i], parent.entry_at(list[i]).map(|x| x.gen).unwrap_or(0)).map_err(|_| E_LIST)?;
        if e.attrs & gf::HA_INHERIT == 0 {
            return Err(E_LIST);
        }
        if e.attrs & gf::HA_PRESET != 0 {
            return Err(E_FLAG);
        }
        let attrs = e.attrs & !gf::HA_INHERIT;
        if attrs & !gf::ATTR_LEGAL != 0 {
            return Err(E_FLAG);
        }
        child.attach(e.obj, e.ga, attrs).map_err(|_| E_CAP)?;
        i += 1;
    }
    Ok((child, list.len()))
}

/// 复制对拍核（判据 ② 的对账）：子表第 i 个继承例与父表可继承清单
/// 第 i 槽逐位一致（掩码/obj 一致、继承标志清零、其余属性保留）。
pub fn copy_matches(parent: &gf::HandleTable, child: &gf::HandleTable) -> Result<(), u16> {
    let list = inheritable_list(parent);
    if child.used() != list.len() {
        return Err(E_COPY);
    }
    let mut i = 0;
    while i < list.len() {
        let pe = parent.read(list[i], parent.entry_at(list[i]).map(|x| x.gen).unwrap_or(0)).map_err(|_| E_LIST)?;
        let ce = child.read(i, child.entry_at(i).map(|x| x.gen).unwrap_or(0)).map_err(|_| E_COPY)?;
        if ce.obj != pe.obj || ce.ga != pe.ga {
            return Err(E_COPY);
        }
        if ce.attrs != pe.attrs & !gf::HA_INHERIT {
            return Err(E_FLAG);
        }
        if ce.attrs & gf::HA_INHERIT != 0 {
            return Err(E_FLAG);
        }
        i += 1;
    }
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：建父表 40 句柄（判据 ① 载体）：20 例挂继承标志（其中 5 例
    // 兼带其他属性），20 例不挂。
    let masks = [
        sr::STD_ALL | 0x0007,
        sr::READ_CONTROL | 0x0001,
        sr::DELETE | sr::SYNCHRONIZE | 0x0004,
        sr::WRITE_DAC | 0x0002,
        sr::READ_CONTROL,
    ];
    let mut t = gf::HandleTable::new();
    let mut i = 0;
    while i < 40 {
        let mut attrs = if i % 2 == 0 { gf::HA_INHERIT } else { 0 };
        if i % 8 == 0 {
            attrs |= gf::HA_AUDIT_CLOSE;
        }
        if i == 0 {
            attrs |= gf::HA_PROTECT;
        }
        // 预设标记槽三例（0/10/20）——规则面：PRESET 不继承。
        if i == 0 || i == 10 || i == 20 {
            attrs |= gf::HA_PRESET;
        }
        t.attach(0x8000 + i as u32, masks[i % 5], attrs).map_err(|_| E_CAP)?;
        i += 1;
    }
    // R2：清单投影（判据 ① 落点）：继承清单 20 例、全偶槽、自证通过；
    // 可继承清单 17 例（三个 PRESET 槽被规则滤除）。
    let list = inherit_list(&t);
    if list.len() != 20 {
        return Err(E_LIST);
    }
    let mut i = 0;
    while i < list.len() {
        if list[i] != i * 2 {
            return Err(E_LIST);
        }
        i += 1;
    }
    if !list_consistent(&t, &list) {
        return Err(E_LIST);
    }
    let ilist = inheritable_list(&t);
    if ilist.len() != 17 {
        return Err(E_LIST);
    }
    // 规则滤除的三槽恰为 0/10/20（PRESET 例）。
    if ilist.contains(&0) || ilist.contains(&10) || ilist.contains(&20) {
        return Err(E_LIST);
    }
    // R3：继承零副作用锚（判据 ③ 落点）——清单只读、父表不动。
    let ep0 = t.epoch();
    let used0 = t.used();
    let list2 = inherit_list(&t);
    if list2.len() != 20 || list2 != list {
        return Err(E_LIST);
    }
    if t.epoch() != ep0 || t.used() != used0 {
        return Err(E_SIDE);
    }
    // R4：子表复制（判据 ② 落点）：17 例（PRESET 滤除后）逐位对拍 +
    // 槽序一致。
    let (mut child, n) = spawn_child(&t).map_err(|e| e)?;
    if n != 17 || child.used() != 17 {
        return Err(E_COPY);
    }
    copy_matches(&t, &child)?;
    // 父表仍未动（继承后复核——移动语义在此被证伪）。
    if t.epoch() != ep0 || t.used() != used0 {
        return Err(E_SIDE);
    }
    // R5：子表独立演化（继承是复制不是别名）：子表关闭一例、追加一例
    // ——父表与对拍核均不受扰。
    child.release(0, child.entry_at(0).map(|x| x.gen).unwrap_or(0)).map_err(|_| E_COPY)?;
    child.attach(0x9000, sr::READ_CONTROL, 0).map_err(|_| E_CAP)?;
    if t.used() != used0 || t.epoch() != ep0 {
        return Err(E_SIDE);
    }
    // copy_matches 现应翻红（子表已演化——对拍核忠实）。
    if copy_matches(&t, &child).is_ok() {
        return Err(E_COPY);
    }
    // R6：空继承表（无挂标 → 子表空——清单闭集边界）。
    let mut t2 = gf::HandleTable::new();
    t2.attach(1, sr::READ_CONTROL, 0).map_err(|_| E_CAP)?;
    let (child2, n2) = spawn_child(&t2).map_err(|e| e)?;
    if n2 != 0 || child2.used() != 0 {
        return Err(E_LIST);
    }
    // 全挂标表（40 例全继承 → 子表 40 例满对拍）。
    let mut t3 = gf::HandleTable::new();
    let mut i = 0;
    while i < 40 {
        t3.attach(0xA000 + i as u32, sr::READ_CONTROL, gf::HA_INHERIT).map_err(|_| E_CAP)?;
        i += 1;
    }
    let (child3, n3) = spawn_child(&t3).map_err(|e| e)?;
    if n3 != 40 || child3.used() != 40 {
        return Err(E_COPY);
    }
    copy_matches(&t3, &child3)?;
    // 校验和：清单长度聚合（确定性总锚）。
    Ok((list.len() as u32)
        .wrapping_add(n as u32)
        .wrapping_add(n2 as u32)
        .wrapping_add(n3 as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_LIST => "继承清单破（投影与属性过滤不符/槽序乱）",
        E_COPY => "复制破（掩码/obj 不一致或槽序漂移）",
        E_SIDE => "副作用破（父表 epoch/used 被继承污染）",
        E_CAP => "子表满（继承例数超容）",
        E_FLAG => "标志消耗破（子侧仍带 INHERIT）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36232_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36232_gainherit");
    cs.add("SEG 段位独占", SEG_BASE == 0xa240, "0xa240~0xa24F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "20 例清单投影/复制逐位对拍/零副作用"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("清单锚", gf::HA_INHERIT == 1 << 24, "继承标志 = 属性域 bit24（36221 域锚消费）");
    cs.add("预设滤除", gf::HA_PRESET & gf::HA_INHERIT == 0 && gf::HA_PRESET & gf::ATTR_LEGAL != 0, "PRESET 与 INHERIT 位独立（规则滤除的位面锚）");
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
    fn test_inherit_copy_semantics() {
        let mut t = gf::HandleTable::new();
        let _ = t.attach(1, sr::READ_CONTROL | 0x0001, gf::HA_INHERIT | gf::HA_AUDIT_CLOSE);
        let _ = t.attach(2, sr::WRITE_DAC, 0);
        let (child, n) = spawn_child(&t).unwrap_or((gf::HandleTable::new(), 0));
        assert_eq!(n, 1);
        let ce = child.entry_at(0).unwrap_or(gf::Entry { obj: 0, ga: 0, attrs: 0, gen: 0, auth_epoch: 0 });
        assert_eq!(ce.obj, 1);
        assert_eq!(ce.ga, sr::READ_CONTROL | 0x0001);
        // 继承标志消耗、其余属性保留。
        assert_eq!(ce.attrs, gf::HA_AUDIT_CLOSE);
        assert!(copy_matches(&t, &child).is_ok());
        // 父表零副作用。
        assert_eq!(t.used(), 2);
    }

    #[test]
    fn test_list_consistent_negative() {
        let mut t = gf::HandleTable::new();
        let _ = t.attach(1, sr::READ_CONTROL, gf::HA_INHERIT);
        let _ = t.attach(2, sr::READ_CONTROL, 0);
        let list = inherit_list(&t);
        assert!(list_consistent(&t, &list));
        // 错序清单不自证。
        let bad = alloc::vec![0usize, 0usize];
        assert!(!list_consistent(&t, &bad));
    }

    #[test]
    fn test_spawn_preset_excluded() {
        let mut t = gf::HandleTable::new();
        let _ = t.attach(1, sr::READ_CONTROL, gf::HA_INHERIT | gf::HA_PRESET);
        let _ = t.attach(2, sr::READ_CONTROL, gf::HA_INHERIT);
        // 可继承清单滤除 PRESET 槽——子表只收 1 例。
        assert_eq!(inheritable_list(&t).len(), 1);
        let (child, n) = spawn_child(&t).unwrap_or((gf::HandleTable::new(), 9));
        assert_eq!(n, 1);
        assert_eq!(child.used(), 1);
        assert!(copy_matches(&t, &child).is_ok());
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36232_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 3);
    }
}
