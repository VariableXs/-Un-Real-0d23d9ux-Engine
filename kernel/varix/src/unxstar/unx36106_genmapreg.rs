//! UNX-F36106-J1 注册表键类型的 GENERIC 映射登记。
//! 判据：① KEY_* 六个特定位与 KEY_READ/WRITE/EXECUTE 三值写死；
//! ② 映射守恒（三元组 ⊆ 特定段+标准五位区）与装配幂等；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：B06 映射面类型登记方——通过 36104 框架登记
//! "RegistryKey" 类型；消费 D3 注册表对象的 GENERIC 翻译锚。

use super::unx36101_masktable as mt;
use super::unx36104_genmap as gm;
use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x3960;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_VAL: u16 = SEG_BASE + 2; // KEY_* 值与写死锚不符
pub const E_CONSERV: u16 = SEG_BASE + 3; // 映射越出合法区（守恒破）
pub const E_DUP: u16 = SEG_BASE + 4; // 特定位号重复/乱序
pub const E_NOENT: u16 = SEG_BASE + 5; // 登记后查询失败（装配破）
pub const E_TRANS: u16 = SEG_BASE + 6; // 翻译值与三元组不符
pub const E_COVER: u16 = SEG_BASE + 7; // 三并覆盖锚不符
pub const E_NAME: u16 = SEG_BASE + 8; // 登记名不符

/// 模块版本锚。
pub const VER: u32 = 1;

/// KEY_GENERIC 三槽标记（显式判别式）。
#[repr(u16)]
pub enum KeySlot {
    Read = 0x0001,
    Write = 0x0002,
    Exec = 0x0004,
}

impl KeySlot {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 登记类型名（B06 框架内唯一）。
pub const TYPE_NAME: &str = "RegistryKey";

// ---- 注册表键特定权限六位（位值写死） ----------------------------------------
pub const KEY_QUERY_VALUE: u32 = 0x0000_0001;
pub const KEY_SET_VALUE: u32 = 0x0000_0002;
pub const KEY_CREATE_SUB_KEY: u32 = 0x0000_0004;
pub const KEY_ENUMERATE_SUB_KEYS: u32 = 0x0000_0008;
pub const KEY_NOTIFY: u32 = 0x0000_0010;
pub const KEY_CREATE_LINK: u32 = 0x0000_0020;

// ---- KEY_* 三值（写死锚；READ_CONTROL 属标准段；本族无 SYNCHRONIZE） ----------
pub const KEY_READ: u32 = 0x0002_0019;
pub const KEY_WRITE: u32 = 0x0002_0006;
pub const KEY_EXECUTE: u32 = 0x0002_0019;

/// 六位并（特定段覆盖锚）。
pub const KEY_SPEC_UNION: u32 = KEY_QUERY_VALUE
    | KEY_SET_VALUE
    | KEY_CREATE_SUB_KEY
    | KEY_ENUMERATE_SUB_KEYS
    | KEY_NOTIFY
    | KEY_CREATE_LINK;

/// KEY_READ 构成回查表。
pub const KEY_READ_PARTS: [u32; 4] = [
    0x0002_0000, // READ_CONTROL
    KEY_QUERY_VALUE,
    KEY_ENUMERATE_SUB_KEYS,
    KEY_NOTIFY,
];

/// KEY_WRITE 构成回查表。
pub const KEY_WRITE_PARTS: [u32; 3] = [
    0x0002_0000, // READ_CONTROL
    KEY_SET_VALUE,
    KEY_CREATE_SUB_KEY,
];

/// 逐位构成回查：各部分独占一位且并入等于总值。
fn parts_check(parts: &[u32], total: u32) -> Result<(), u16> {
    let mut acc: u32 = 0;
    let mut i = 0;
    while i < parts.len() {
        if parts[i] & acc != 0 {
            return Err(E_DUP);
        }
        acc |= parts[i];
        i += 1;
    }
    if acc != total {
        return Err(E_VAL);
    }
    Ok(())
}

/// 把 RegistryKey 映射登记进框架注册表（幂等；消费方装配入口）。
pub fn register_key(reg: &mut gm::Registry) -> Result<usize, u16> {
    reg.register(
        "RegistryKey",
        gm::GenMap { read: KEY_READ, write: KEY_WRITE, exec: KEY_EXECUTE },
    )
}

/// 键值语义分类：值读/值写/枚举/通知四面的位需求表（等待/诊断消费）。
pub const KEY_FACE_N: usize = 4;
pub const KEY_FACES: [(&str, u32); KEY_FACE_N] = [
    ("query", KEY_QUERY_VALUE),
    ("set", KEY_SET_VALUE),
    ("enum", KEY_ENUMERATE_SUB_KEYS),
    ("notify", KEY_NOTIFY),
];

/// 键位全语义面账：6 位的「人话面」表（诊断/审计消费锚）。
pub const KEY_BITFACE_N: usize = 6;
pub const KEY_BITFACES: [(&str, u32); KEY_BITFACE_N] = [
    ("query-value", KEY_QUERY_VALUE),
    ("set-value", KEY_SET_VALUE),
    ("create-subkey", KEY_CREATE_SUB_KEY),
    ("enum-subkeys", KEY_ENUMERATE_SUB_KEYS),
    ("notify", KEY_NOTIFY),
    ("create-link", KEY_CREATE_LINK),
];

/// 按位查语义面名（族外位显性拒）。
pub fn bitface_of(bit: u32) -> Result<&'static str, u16> {
    let m = 1u32.wrapping_shl(bit);
    let mut i = 0;
    while i < KEY_BITFACE_N {
        if KEY_BITFACES[i].1 == m {
            return Ok(KEY_BITFACES[i].0);
        }
        i += 1;
    }
    Err(E_DUP)
}

/// 三值 ↔ 位族守恒账：每个 KEY_* 三值展开必须 ⊆ 位族合法区，
/// 且 CREATE_LINK 不出现在任何展开里（预留位语义）。
pub fn expansion_face_check() -> Result<(), u16> {
    let union = KEY_READ | KEY_WRITE | KEY_EXECUTE;
    if union & !KEY_SPEC_UNION & !0x0002_0000 != 0 {
        return Err(E_COVER);
    }
    if union & KEY_CREATE_LINK != 0 {
        return Err(E_COVER);
    }
    if KEY_QUERY_VALUE & union == 0 || KEY_SET_VALUE & union == 0 {
        return Err(E_COVER);
    }
    if KEY_ENUMERATE_SUB_KEYS & union == 0 || KEY_NOTIFY & union == 0 {
        return Err(E_COVER);
    }
    Ok(())
}

/// 演练：R1 六位与三值写死锚；R2 逐位构成回查；R3 装配幂等与回读；
/// R4 守恒对拍；R5 翻译对拍 + 负例。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：写死值锚——六位两两不交、全落特定段、并值正确。
    if VER != 1 {
        return Err(E_VER);
    }
    let six: [u32; 6] = [
        KEY_QUERY_VALUE,
        KEY_SET_VALUE,
        KEY_CREATE_SUB_KEY,
        KEY_ENUMERATE_SUB_KEYS,
        KEY_NOTIFY,
        KEY_CREATE_LINK,
    ];
    let mut acc: u32 = 0;
    let mut i = 0;
    while i < 6 {
        if six[i] & acc != 0 {
            return Err(E_DUP);
        }
        if six[i] & !mt::SPEC_MASK != 0 {
            return Err(E_CONSERV);
        }
        acc |= six[i];
        i += 1;
    }
    if acc != KEY_SPEC_UNION || KEY_SPEC_UNION != 0x3F {
        return Err(E_VAL);
    }
    if KEY_READ != 0x0002_0019 || KEY_WRITE != 0x0002_0006 || KEY_EXECUTE != 0x0002_0019 {
        return Err(E_VAL);
    }
    hits += 1;
    // R2：逐位构成回查。
    parts_check(&KEY_READ_PARTS, KEY_READ)?;
    parts_check(&KEY_WRITE_PARTS, KEY_WRITE)?;
    if KEY_EXECUTE != KEY_READ {
        return Err(E_VAL); // 键族无 SYNCHRONIZE：EXECUTE ≡ READ
    }
    if KEY_READ & mt::STD_USED != 0x0002_0000 {
        return Err(E_VAL); // 仅 READ_CONTROL，无 SYNCHRONIZE
    }
    hits += 1;
    // R3：装配进 36104 框架——登记、幂等、回读。
    let mut reg = gm::Registry::new();
    let idx = register_key(&mut reg)?;
    if idx != 0 || reg.count() != 1 {
        return Err(E_NOENT);
    }
    let again = register_key(&mut reg)?;
    if again != idx || reg.count() != 1 {
        return Err(E_DUP);
    }
    let m = reg.query(TYPE_NAME).map_err(|_| E_NOENT)?;
    if m.read != KEY_READ || m.write != KEY_WRITE || m.exec != KEY_EXECUTE {
        return Err(E_TRANS);
    }
    hits += 1;
    // R4：守恒对拍——三值落合法区；键位并 ⊆ 三值并（CREATE_LINK 除外）。
    let legal = mt::SPEC_MASK | mt::STD_USED;
    if (KEY_READ | KEY_WRITE | KEY_EXECUTE) & !legal != 0 {
        return Err(E_CONSERV);
    }
    let union = KEY_READ | KEY_WRITE | KEY_EXECUTE;
    if union != (0x0002_0000 | KEY_QUERY_VALUE | KEY_SET_VALUE | KEY_CREATE_SUB_KEY | KEY_ENUMERATE_SUB_KEYS | KEY_NOTIFY) {
        return Err(E_COVER);
    }
    if KEY_CREATE_LINK & union != 0 {
        return Err(E_COVER); // CREATE_LINK 不在 GENERIC 展开中（预留位语义）
    }
    if KEY_NOTIFY & union == 0 {
        return Err(E_COVER);
    }
    hits += 1;
    // R5：翻译对拍 + 四面位需求 + 负例。
    if reg.translate(TYPE_NAME, gm::G_READ).map_err(|_| E_TRANS)? != KEY_READ {
        return Err(E_TRANS);
    }
    if reg.translate(TYPE_NAME, gm::G_WRITE).map_err(|_| E_TRANS)? != KEY_WRITE {
        return Err(E_TRANS);
    }
    if reg.translate(TYPE_NAME, gm::G_EXEC).map_err(|_| E_TRANS)? != KEY_EXECUTE {
        return Err(E_TRANS);
    }
    if reg.translate_all(TYPE_NAME)? != union {
        return Err(E_COVER);
    }
    if reg.translate(TYPE_NAME, gm::G_ALL).is_ok() {
        return Err(E_TRANS);
    }
    let mut f = 0;
    while f < KEY_FACE_N {
        let (name, bit) = KEY_FACES[f];
        if name.is_empty() || bit & !KEY_SPEC_UNION != 0 {
            return Err(E_VAL);
        }
        f += 1;
    }
    // R5b：六位语义面回查 + 三值位族守恒账。
    let mut f = 0;
    while f < KEY_BITFACE_N {
        let bit = KEY_BITFACES[f].1.trailing_zeros();
        let nm = bitface_of(bit).map_err(|_| E_DUP)?;
        if nm != KEY_BITFACES[f].0 {
            return Err(E_DUP);
        }
        f += 1;
    }
    if bitface_of(6).is_ok() || bitface_of(16).is_ok() {
        return Err(E_DUP);
    }
    expansion_face_check()?;
    if reg.query("File").is_ok() {
        return Err(E_NOENT); // 本模块只管 RegistryKey——他类不静默
    }
    if reg.query("registrykey").is_ok() {
        return Err(E_NOENT); // 大小写敏感——非静默匹配
    }
    let _ = vec![0u8; 0];
    let mut marks: Vec<u32> = Vec::new();
    marks.push(KEY_READ);
    marks.push(KEY_WRITE);
    if marks.len() != 2 {
        return Err(E_DUP);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(5));
    }

    #[test]
    fn values_frozen() {
        assert_eq!(KEY_READ, 0x00020019);
        assert_eq!(KEY_WRITE, 0x00020006);
        assert_eq!(KEY_EXECUTE, 0x00020019);
        assert_eq!(KEY_SPEC_UNION, 0x3F);
    }

    #[test]
    fn conserv_holds() {
        let legal = mt::SPEC_MASK | mt::STD_USED;
        assert_eq!((KEY_READ | KEY_WRITE | KEY_EXECUTE) & !legal, 0);
    }

    #[test]
    fn exec_equals_read() {
        assert_eq!(KEY_EXECUTE, KEY_READ);
        assert_eq!(KEY_READ & mt::STD_USED, 0x0002_0000);
    }
}

pub fn run_unx36106_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36106_genmapreg");
    cs.add("SEG 段位独占", SEG_BASE == 0x3960, "0x3960~0x396F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "六位/三值/装配/守恒全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("特定六位写死", KEY_SPEC_UNION == 0x3F, "位 0..5 全覆盖且互斥");
    cs.add("三值写死", KEY_READ == 0x00020019 && KEY_WRITE == 0x00020006, "KEY_READ/WRITE 锚");
    cs.add("位族守恒", expansion_face_check() == Ok(()), "三值展开 ⊆ 位族 + CREATE_LINK 预留");
    cs.add("守恒区", (KEY_READ | KEY_WRITE | KEY_EXECUTE) & !mt::ALL_LEGAL == 0, "三值 ⊆ 合法区");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_VAL => "KEY_* 值与写死锚不符",
        E_CONSERV => "映射越出合法区（守恒破）",
        E_DUP => "特定位号重复/乱序",
        E_NOENT => "登记后查询失败（装配破）",
        E_TRANS => "翻译值与三元组不符",
        E_COVER => "三并覆盖锚不符",
        E_NAME => "登记名不符",
        _ => "未知诊断码",
    }
}
