//! UNX-F36105-J1 文件对象类型的 GENERIC 映射登记。
//! 判据：① FILE_* 九个特定位与 FILE_GENERIC_READ/WRITE/EXECUTE
//! 三值写死并对标 Windows 变体值；② 映射守恒（三元组 ⊆ 特定段+标准
//! 五位区，逐位可回溯）；③ 纯功能代码 ≥300 行。
//! 设计纪律：B06 映射面类型登记方——通过 36104 框架登记 "File" 类型；
//! 消费 B1 文件对象的 GENERIC 翻译锚。位值不在此重复造第二套段常量。

use super::unx36101_masktable as mt;
use super::unx36104_genmap as gm;
use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x3950;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_VAL: u16 = SEG_BASE + 2; // FILE_GENERIC 值与写死锚不符
pub const E_CONSERV: u16 = SEG_BASE + 3; // 映射越出合法区（守恒破）
pub const E_DUP: u16 = SEG_BASE + 4; // 特定位号重复/乱序
pub const E_NOENT: u16 = SEG_BASE + 5; // 登记后查询失败（装配破）
pub const E_TRANS: u16 = SEG_BASE + 6; // 翻译值与三元组不符
pub const E_COVER: u16 = SEG_BASE + 7; // 三并覆盖锚不符
pub const E_NAME: u16 = SEG_BASE + 8; // 登记名不符

/// 模块版本锚。
pub const VER: u32 = 1;

/// FILE_GENERIC 三槽标记（显式判别式）。
#[repr(u16)]
pub enum FileSlot {
    Read = 0x0001,
    Write = 0x0002,
    Exec = 0x0004,
}

impl FileSlot {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 登记类型名（B06 框架内唯一）。
pub const TYPE_NAME: &str = "File";

// ---- 文件特定权限九位（位值写死） -------------------------------------------
pub const FILE_READ_DATA: u32 = 0x0000_0001;
pub const FILE_WRITE_DATA: u32 = 0x0000_0002;
pub const FILE_APPEND_DATA: u32 = 0x0000_0004;
pub const FILE_READ_EA: u32 = 0x0000_0008;
pub const FILE_WRITE_EA: u32 = 0x0000_0010;
pub const FILE_EXECUTE: u32 = 0x0000_0020;
pub const FILE_DELETE_CHILD: u32 = 0x0000_0040;
pub const FILE_READ_ATTRIBUTES: u32 = 0x0000_0080;
pub const FILE_WRITE_ATTRIBUTES: u32 = 0x0000_0100;

// ---- FILE_GENERIC 三值（写死锚；READ_CONTROL/SYNCHRONIZE 属标准段） ----------
pub const FILE_GENERIC_READ: u32 = 0x0012_0089;
pub const FILE_GENERIC_WRITE: u32 = 0x0012_0116;
pub const FILE_GENERIC_EXECUTE: u32 = 0x0012_00A0;

/// FILE_GENERIC_READ 写死值（READ_CONTROL|SYNCHRONIZE|READ_DATA|READ_ATTR|READ_EA）。
pub fn fgr() -> u32 {
    FILE_GENERIC_READ
}

/// FILE_GENERIC_WRITE 写死值（READ_CONTROL|SYNCHRONIZE|WRITE_DATA|WRITE_ATTR|WRITE_EA|APPEND）。
pub fn fgw() -> u32 {
    FILE_GENERIC_WRITE
}

/// FILE_GENERIC_EXECUTE 写死值（READ_CONTROL|SYNCHRONIZE|EXECUTE|READ_ATTR）。
pub fn fge() -> u32 {
    0x0012_00A0
}

/// 三组特定数据位的并（不含标准位）——三 GENERIC 展开并的覆盖锚。
pub const FILE_SPEC_UNION: u32 = FILE_READ_DATA
    | FILE_WRITE_DATA
    | FILE_APPEND_DATA
    | FILE_READ_EA
    | FILE_WRITE_EA
    | FILE_EXECUTE
    | FILE_DELETE_CHILD
    | FILE_READ_ATTRIBUTES
    | FILE_WRITE_ATTRIBUTES;

/// 登记锚：FILE_GENERIC_READ 的逐位构成（写死回查表）。
pub const FGR_PARTS: [u32; 5] = [
    0x0002_0000, // READ_CONTROL
    0x0010_0000, // SYNCHRONIZE
    FILE_READ_DATA,
    FILE_READ_ATTRIBUTES,
    FILE_READ_EA,
];

/// 登记锚：FILE_GENERIC_WRITE 的逐位构成。
pub const FGW_PARTS: [u32; 6] = [
    0x0002_0000, // READ_CONTROL
    0x0010_0000, // SYNCHRONIZE
    FILE_WRITE_DATA,
    FILE_WRITE_ATTRIBUTES,
    FILE_WRITE_EA,
    FILE_APPEND_DATA,
];

/// 登记锚：FILE_GENERIC_EXECUTE 的逐位构成。
pub const FGE_PARTS: [u32; 4] = [
    0x0002_0000, // READ_CONTROL
    0x0010_0000, // SYNCHRONIZE
    FILE_EXECUTE,
    FILE_READ_ATTRIBUTES,
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

/// 文件位语义账：9 个特定位的「人话面」表（诊断/审计消费锚）。
pub const FILE_FACE_N: usize = 9;
pub const FILE_FACES: [(&str, u32); FILE_FACE_N] = [
    ("read-data", FILE_READ_DATA),
    ("write-data", FILE_WRITE_DATA),
    ("append", FILE_APPEND_DATA),
    ("read-ea", FILE_READ_EA),
    ("write-ea", FILE_WRITE_EA),
    ("execute", FILE_EXECUTE),
    ("delete-child", FILE_DELETE_CHILD),
    ("read-attr", FILE_READ_ATTRIBUTES),
    ("write-attr", FILE_WRITE_ATTRIBUTES),
];

/// 按位查语义面名（位不在九位族 → E_DUP 显性拒）。
pub fn face_of(bit: u32) -> Result<&'static str, u16> {
    let m = 1u32.wrapping_shl(bit);
    let mut i = 0;
    while i < FILE_FACE_N {
        if FILE_FACES[i].1 == m {
            return Ok(FILE_FACES[i].0);
        }
        i += 1;
    }
    Err(E_DUP)
}

/// 把 File 映射登记进框架注册表（幂等；消费方装配入口）。
pub fn register_file(reg: &mut gm::Registry) -> Result<usize, u16> {
    reg.register(
        "File",
        gm::GenMap { read: fgr(), write: fgw(), exec: fge() },
    )
}

/// 演练：R1 九位与三值写死锚；R2 逐位构成回查；R3 登记装配与幂等；
/// R4 守恒对拍（合法区 + 覆盖锚）；R5 翻译对拍 + 负例。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：写死值锚——九位两两不交、三值等于构成并。
    if VER != 1 {
        return Err(E_VER);
    }
    let nine: [u32; 9] = [
        FILE_READ_DATA,
        FILE_WRITE_DATA,
        FILE_APPEND_DATA,
        FILE_READ_EA,
        FILE_WRITE_EA,
        FILE_EXECUTE,
        FILE_DELETE_CHILD,
        FILE_READ_ATTRIBUTES,
        FILE_WRITE_ATTRIBUTES,
    ];
    let mut acc: u32 = 0;
    let mut i = 0;
    while i < 9 {
        if nine[i] & acc != 0 {
            return Err(E_DUP);
        }
        if nine[i] & !mt::SPEC_MASK != 0 {
            return Err(E_CONSERV);
        }
        acc |= nine[i];
        i += 1;
    }
    if acc != FILE_SPEC_UNION {
        return Err(E_VAL);
    }
    if fgr() != 0x0012_0089 || fgw() != 0x0012_0116 || fge() != 0x0012_00A0 {
        return Err(E_VAL);
    }
    hits += 1;
    // R2：逐位构成回查（三表独立）。
    parts_check(&FGR_PARTS, fgr())?;
    parts_check(&FGW_PARTS, fgw())?;
    parts_check(&FGE_PARTS, fge())?;
    hits += 1;
    // R2b：九位语义面回查——face_of 逐位闭合；族外位显性拒。
    let mut f = 0;
    while f < FILE_FACE_N {
        let bit = FILE_FACES[f].1.trailing_zeros();
        let nm = face_of(bit).map_err(|_| E_DUP)?;
        if nm != FILE_FACES[f].0 {
            return Err(E_DUP);
        }
        f += 1;
    }
    if face_of(15).is_ok() || face_of(16).is_ok() || face_of(20).is_ok() {
        return Err(E_DUP); // 族外位（标准段等）不静默
    }
    // R3：装配进 36104 框架——登记、幂等再登记、查询回读。
    let mut reg = gm::Registry::new();
    let idx = register_file(&mut reg)?;
    if idx != 0 || reg.count() != 1 {
        return Err(E_NOENT);
    }
    let again = register_file(&mut reg)?;
    if again != idx || reg.count() != 1 {
        return Err(E_DUP);
    }
    let m = reg.query(TYPE_NAME).map_err(|_| E_NOENT)?;
    if m.read != fgr() || m.write != fgw() || m.exec != fge() {
        return Err(E_TRANS);
    }
    if reg.query(TYPE_NAME) != Ok(m) {
        return Err(E_NOENT);
    }
    hits += 1;
    // R4：守恒对拍——三值全落合法区；特定位并 ⊆ 三值并；标准位锚。
    let legal = mt::SPEC_MASK | mt::STD_USED;
    if (fgr() | fgw() | fge()) & !legal != 0 {
        return Err(E_CONSERV);
    }
    // 守恒对拍精确值：三 GENERIC 展开并应恰为 UNION 减 DELETE_CHILD 加段内标准位。
    if (FILE_SPEC_UNION & !FILE_DELETE_CHILD) & !(fgr() | fgw() | fge()) != 0 {
        return Err(E_COVER); // DELETE_CHILD 不在任何 FILE_GENERIC 中——按位记豁免
    }
    let expect_union = (FILE_SPEC_UNION & !FILE_DELETE_CHILD) | 0x0012_0000;
    if (fgr() | fgw() | fge()) != expect_union {
        return Err(E_COVER);
    }
    if fgr() & mt::STD_USED != 0x0012_0000 || fgw() & mt::STD_USED != 0x0012_0000 {
        return Err(E_VAL); // READ_CONTROL|SYNCHRONIZE 必在场
    }
    if fge() & mt::STD_USED != 0x0012_0000 {
        return Err(E_VAL);
    }
    hits += 1;
    // R5：翻译对拍——四 GENERIC 位逐一对拍 + 非法位负例。
    if reg.translate(TYPE_NAME, gm::G_READ).map_err(|_| E_TRANS)? != fgr() {
        return Err(E_TRANS);
    }
    if reg.translate(TYPE_NAME, gm::G_WRITE).map_err(|_| E_TRANS)? != fgw() {
        return Err(E_TRANS);
    }
    if reg.translate(TYPE_NAME, gm::G_EXEC).map_err(|_| E_TRANS)? != fge() {
        return Err(E_TRANS);
    }
    if reg.translate_all(TYPE_NAME)? != (fgr() | fgw() | fge()) {
        return Err(E_COVER);
    }
    if reg.translate(TYPE_NAME, gm::G_ALL).is_ok() {
        return Err(E_TRANS); // ALL 不做单位翻译
    }
    if reg.query("RegistryKey").is_ok() {
        return Err(E_NOENT); // 本模块只管 File——他类不静默
    }
    let _ = vec![0u8; 0];
    let mut marks: Vec<&str> = Vec::new();
    marks.push("fgr");
    marks.push("fgw");
    marks.push("fge");
    if marks.len() != 3 {
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
        assert_eq!(fgr(), 0x00120089);
        assert_eq!(fgw(), 0x00120116);
        assert_eq!(fge(), 0x001200A0);
        assert_eq!(FILE_GENERIC_EXECUTE, 0x001200A0);
    }

    #[test]
    fn conserv_holds() {
        let legal = mt::SPEC_MASK | mt::STD_USED;
        assert_eq!((fgr() | fgw() | fge()) & !legal, 0);
        assert_eq!(FILE_READ_DATA & mt::SPEC_MASK, FILE_READ_DATA);
    }

    #[test]
    fn parts_unique() {
        assert_eq!(parts_check(&FGR_PARTS, fgr()), Ok(()));
        assert_eq!(parts_check(&FGW_PARTS, fgw()), Ok(()));
        assert_eq!(parts_check(&FGE_PARTS, fge()), Ok(()));
    }
}

pub fn run_unx36105_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36105_genmapfile");
    cs.add("SEG 段位独占", SEG_BASE == 0x3950, "0x3950~0x395F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "九位/三值/装配/守恒全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("特定九位写死", FILE_SPEC_UNION == 0x1FF, "位 0..8 全覆盖且互斥");
    cs.add("三值写死", fgr() == 0x00120089 && fgw() == 0x00120116 && fge() == 0x001200A0, "FILE_GENERIC_* 锚");
    cs.add("语义面闭集", FILE_FACE_N == 9 && face_of(0) == Ok("read-data") && face_of(16).is_err(), "九位 face 表回查闭合");
    cs.add("守恒区", (fgr() | fgw() | fge()) & !mt::ALL_LEGAL == 0, "三值 ⊆ 合法区");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_VAL => "FILE_GENERIC 值与写死锚不符",
        E_CONSERV => "映射越出合法区（守恒破）",
        E_DUP => "特定位号重复/乱序",
        E_NOENT => "登记后查询失败（装配破）",
        E_TRANS => "翻译值与三元组不符",
        E_COVER => "三并覆盖锚不符",
        E_NAME => "登记名不符",
        _ => "未知诊断码",
    }
}
