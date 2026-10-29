//! UNX-F36107-J1 窗口站/桌面对象的 GENERIC 映射登记。
//! 判据：① WINSTA_* 十位与 DESKTOP_* 九位、WindowStation/Desktop
//! 两类 GENERIC 三元组写死；② 映射守恒 + 双类型装配幂等；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：B06 映射面类型登记方——通过 36104 框架同时登记
//! "WindowStation" 与 "Desktop" 两类；消费 D4/J2 会话面翻译锚。

use super::unx36101_masktable as mt;
use super::unx36104_genmap as gm;
use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x3970;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_VAL: u16 = SEG_BASE + 2; // WINSTA_/DESKTOP_ 值与写死锚不符
pub const E_CONSERV: u16 = SEG_BASE + 3; // 映射越出合法区（守恒破）
pub const E_DUP: u16 = SEG_BASE + 4; // 特定位号重复/乱序
pub const E_NOENT: u16 = SEG_BASE + 5; // 登记后查询失败（装配破）
pub const E_TRANS: u16 = SEG_BASE + 6; // 翻译值与三元组不符
pub const E_COVER: u16 = SEG_BASE + 7; // 三并覆盖锚不符
pub const E_NAME: u16 = SEG_BASE + 8; // 登记名不符

/// 模块版本锚。
pub const VER: u32 = 1;

/// 双类型六槽标记（显式判别式：窗站三槽 + 桌面三槽）。
#[repr(u16)]
pub enum WsDtSlot {
    WsRead = 0x0001,
    WsWrite = 0x0002,
    WsExec = 0x0004,
    DtRead = 0x0008,
    DtWrite = 0x0010,
    DtExec = 0x0020,
}

impl WsDtSlot {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 两类登记名（B06 框架内唯一）。
pub const WS_NAME: &str = "WindowStation";
pub const DT_NAME: &str = "Desktop";

// ---- 窗口站特定权限十位（位值写死） ------------------------------------------
pub const WINSTA_ENUMDESKTOPS: u32 = 0x0000_0001;
pub const WINSTA_READATTRIBUTES: u32 = 0x0000_0002;
pub const WINSTA_ACCESSCLIPBOARD: u32 = 0x0000_0004;
pub const WINSTA_CREATEDESKTOP: u32 = 0x0000_0008;
pub const WINSTA_WRITEATTRIBUTES: u32 = 0x0000_0010;
pub const WINSTA_ACCESSGLOBALATOMS: u32 = 0x0000_0020;
pub const WINSTA_EXITWINDOWS: u32 = 0x0000_0040;
pub const WINSTA_ENUMERATE: u32 = 0x0000_0100;
pub const WINSTA_READSCREEN: u32 = 0x0000_0200;
pub const WINSTA_ALL_ACCESS: u32 = 0x0000_037F;

// ---- 桌面特定权限九位（位值写死） --------------------------------------------
pub const DESKTOP_READOBJECTS: u32 = 0x0000_0001;
pub const DESKTOP_CREATEWINDOW: u32 = 0x0000_0002;
pub const DESKTOP_CREATEMENU: u32 = 0x0000_0004;
pub const DESKTOP_HOOKCONTROL: u32 = 0x0000_0008;
pub const DESKTOP_JOURNALRECORD: u32 = 0x0000_0010;
pub const DESKTOP_JOURNALPLAYBACK: u32 = 0x0000_0020;
pub const DESKTOP_ENUMERATE: u32 = 0x0000_0040;
pub const DESKTOP_WRITEOBJECTS: u32 = 0x0000_0080;
pub const DESKTOP_SWITCHDESKTOP: u32 = 0x0000_0100;

// ---- 两类 GENERIC 三元组（变体值写死；READ_CONTROL 属标准段） ------------------
pub const WS_GEN_READ: u32 = 0x0002_0303;
pub const WS_GEN_WRITE: u32 = 0x0002_003C;
pub const WS_GEN_EXEC: u32 = 0x0002_0041;
pub const DT_GEN_READ: u32 = 0x0002_0041;
pub const DT_GEN_WRITE: u32 = 0x0002_0086;
pub const DT_GEN_EXEC: u32 = 0x0002_0108;

/// 窗口站十位并。
pub const WINSTA_SPEC_UNION: u32 = WINSTA_ENUMDESKTOPS
    | WINSTA_READATTRIBUTES
    | WINSTA_ACCESSCLIPBOARD
    | WINSTA_CREATEDESKTOP
    | WINSTA_WRITEATTRIBUTES
    | WINSTA_ACCESSGLOBALATOMS
    | WINSTA_EXITWINDOWS
    | WINSTA_ENUMERATE
    | WINSTA_READSCREEN;

/// 桌面九位并。
pub const DESKTOP_SPEC_UNION: u32 = DESKTOP_READOBJECTS
    | DESKTOP_CREATEWINDOW
    | DESKTOP_CREATEMENU
    | DESKTOP_HOOKCONTROL
    | DESKTOP_JOURNALRECORD
    | DESKTOP_JOURNALPLAYBACK
    | DESKTOP_ENUMERATE
    | DESKTOP_WRITEOBJECTS
    | DESKTOP_SWITCHDESKTOP;

/// 桌面 GENERIC 展开并守恒锚（写死值）。
pub const DT_EXPANSION_UNION: u32 = DT_GEN_READ | DT_GEN_WRITE | DT_GEN_EXEC;

/// 窗口站 GENERIC 展开并守恒锚（写死值）。
pub const WS_EXPANSION_UNION: u32 = WS_GEN_READ | WS_GEN_WRITE | WS_GEN_EXEC;

/// 桌面位语义面账：9 位「人话面」表（诊断/审计消费锚）。
pub const DT_FACE_N: usize = 9;
pub const DT_FACES: [(&str, u32); DT_FACE_N] = [
    ("read-objects", DESKTOP_READOBJECTS),
    ("create-window", DESKTOP_CREATEWINDOW),
    ("create-menu", DESKTOP_CREATEMENU),
    ("hook-control", DESKTOP_HOOKCONTROL),
    ("journal-record", DESKTOP_JOURNALRECORD),
    ("journal-playback", DESKTOP_JOURNALPLAYBACK),
    ("enumerate", DESKTOP_ENUMERATE),
    ("write-objects", DESKTOP_WRITEOBJECTS),
    ("switch-desktop", DESKTOP_SWITCHDESKTOP),
];

/// 按位查桌面语义面名（族外位显性拒）。
pub fn dt_face_of(bit: u32) -> Result<&'static str, u16> {
    let m = 1u32.wrapping_shl(bit);
    let mut i = 0;
    while i < DT_FACE_N {
        if DT_FACES[i].1 == m {
            return Ok(DT_FACES[i].0);
        }
        i += 1;
    }
    Err(E_DUP)
}

/// 把两类映射登记进框架注册表（幂等；消费方装配入口）。
pub fn register_winsta(reg: &mut gm::Registry) -> Result<usize, u16> {
    reg.register(
        "WindowStation",
        gm::GenMap { read: WS_GEN_READ, write: WS_GEN_WRITE, exec: WS_GEN_EXEC },
    )
}

pub fn register_desktop(reg: &mut gm::Registry) -> Result<usize, u16> {
    reg.register(
        "Desktop",
        gm::GenMap { read: DT_GEN_READ, write: DT_GEN_WRITE, exec: DT_GEN_EXEC },
    )
}

/// WINSTA_ALL_ACCESS 构成回查（位 0..6 + ENUMERATE + READSCREEN）。
pub fn winsta_all_compose() -> u32 {
    WINSTA_ENUMDESKTOPS
        | WINSTA_READATTRIBUTES
        | WINSTA_ACCESSCLIPBOARD
        | WINSTA_CREATEDESKTOP
        | WINSTA_WRITEATTRIBUTES
        | WINSTA_ACCESSGLOBALATOMS
        | WINSTA_EXITWINDOWS
        | WINSTA_ENUMERATE
        | WINSTA_READSCREEN
}

/// 演练：R1 两族位值写死锚；R2 WINSTA_ALL 构成回查；R3 双类型装配
/// 幂等与回读；R4 守恒对拍；R5 翻译对拍 + 负例。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：位值写死锚——两族各自互斥、全落特定段、并值正确。
    if VER != 1 {
        return Err(E_VER);
    }
    let ws: [u32; 10] = [
        WINSTA_ENUMDESKTOPS,
        WINSTA_READATTRIBUTES,
        WINSTA_ACCESSCLIPBOARD,
        WINSTA_CREATEDESKTOP,
        WINSTA_WRITEATTRIBUTES,
        WINSTA_ACCESSGLOBALATOMS,
        WINSTA_EXITWINDOWS,
        WINSTA_ENUMERATE,
        WINSTA_READSCREEN,
        WINSTA_ALL_ACCESS,
    ];
    let mut acc: u32 = 0;
    let mut i = 0;
    while i < 10 {
        if ws[i] & acc != 0 && i < 9 {
            return Err(E_DUP);
        }
        if i < 9 && ws[i] & !mt::SPEC_MASK != 0 {
            return Err(E_CONSERV);
        }
        if i < 9 {
            acc |= ws[i];
        }
        i += 1;
    }
    if acc != WINSTA_SPEC_UNION || WINSTA_ALL_ACCESS != 0x37F {
        return Err(E_VAL);
    }
    let dt: [u32; 9] = [
        DESKTOP_READOBJECTS,
        DESKTOP_CREATEWINDOW,
        DESKTOP_CREATEMENU,
        DESKTOP_HOOKCONTROL,
        DESKTOP_JOURNALRECORD,
        DESKTOP_JOURNALPLAYBACK,
        DESKTOP_ENUMERATE,
        DESKTOP_WRITEOBJECTS,
        DESKTOP_SWITCHDESKTOP,
    ];
    let mut acc2: u32 = 0;
    let mut i = 0;
    while i < 9 {
        if dt[i] & acc2 != 0 {
            return Err(E_DUP);
        }
        if dt[i] & !mt::SPEC_MASK != 0 {
            return Err(E_CONSERV);
        }
        acc2 |= dt[i];
        i += 1;
    }
    if acc2 != DESKTOP_SPEC_UNION || DESKTOP_SPEC_UNION != 0x1FF {
        return Err(E_VAL);
    }
    hits += 1;
    // R2：WINSTA_ALL_ACCESS 构成回查（= 九位并，含 ENUMERATE/READSCREEN）。
    if winsta_all_compose() != WINSTA_ALL_ACCESS {
        return Err(E_VAL);
    }
    if WINSTA_ALL_ACCESS & !WINSTA_SPEC_UNION != 0 {
        return Err(E_VAL);
    }
    if WS_EXPANSION_UNION != 0x0002_037F || DT_EXPANSION_UNION != 0x0002_01CF {
        return Err(E_COVER); // 展开并写死锚
    }
    let mut f = 0;
    while f < DT_FACE_N {
        let bit = DT_FACES[f].1.trailing_zeros();
        let nm = dt_face_of(bit).map_err(|_| E_DUP)?;
        if nm != DT_FACES[f].0 {
            return Err(E_DUP);
        }
        f += 1;
    }
    if dt_face_of(9).is_ok() || dt_face_of(16).is_ok() {
        return Err(E_DUP);
    }
    hits += 1;
    // R3：双类型装配——登记、幂等、互不干扰、回读。
    let mut reg = gm::Registry::new();
    let i1 = register_winsta(&mut reg)?;
    let i2 = register_desktop(&mut reg)?;
    if i1 != 0 || i2 != 1 || reg.count() != 2 {
        return Err(E_NOENT);
    }
    if register_winsta(&mut reg)? != i1 || register_desktop(&mut reg)? != i2 {
        return Err(E_DUP);
    }
    if reg.count() != 2 {
        return Err(E_DUP);
    }
    let mw = reg.query(WS_NAME).map_err(|_| E_NOENT)?;
    let md = reg.query(DT_NAME).map_err(|_| E_NOENT)?;
    if mw.read != WS_GEN_READ || md.read != DT_GEN_READ {
        return Err(E_TRANS);
    }
    if mw.read == md.read && WS_GEN_READ != DT_GEN_READ {
        return Err(E_TRANS);
    }
    hits += 1;
    // R4：守恒对拍——六值全落合法区；展开并 ⊆ 类型特定位 + 标准位。
    let legal = mt::SPEC_MASK | mt::STD_USED;
    if (WS_GEN_READ | WS_GEN_WRITE | WS_GEN_EXEC | DT_GEN_READ | DT_GEN_WRITE | DT_GEN_EXEC) & !legal
        != 0
    {
        return Err(E_CONSERV);
    }
    if (WS_GEN_READ | WS_GEN_WRITE | WS_GEN_EXEC) & !WINSTA_SPEC_UNION & !0x0002_0000 != 0 {
        return Err(E_COVER);
    }
    if (DT_GEN_READ | DT_GEN_WRITE | DT_GEN_EXEC) & !DESKTOP_SPEC_UNION & !0x0002_0000 != 0 {
        return Err(E_COVER);
    }
    if WS_GEN_READ & mt::STD_USED != 0x0002_0000 {
        return Err(E_VAL); // 每展开值必含 READ_CONTROL
    }
    hits += 1;
    // R5：翻译对拍 + 负例。
    if reg.translate(WS_NAME, gm::G_READ).map_err(|_| E_TRANS)? != WS_GEN_READ {
        return Err(E_TRANS);
    }
    if reg.translate(DT_NAME, gm::G_WRITE).map_err(|_| E_TRANS)? != DT_GEN_WRITE {
        return Err(E_TRANS);
    }
    if reg.translate(WS_NAME, gm::G_EXEC).map_err(|_| E_TRANS)? != WS_GEN_EXEC {
        return Err(E_TRANS);
    }
    if reg.translate_all(DT_NAME)? != (DT_GEN_READ | DT_GEN_WRITE | DT_GEN_EXEC) {
        return Err(E_COVER);
    }
    if reg.translate(WS_NAME, gm::G_ALL).is_ok() {
        return Err(E_TRANS);
    }
    if reg.query("winstation").is_ok() || reg.query("Windowstation").is_ok() {
        return Err(E_NOENT); // 大小写敏感——非静默匹配
    }
    if reg.query("File").is_ok() || reg.query("RegistryKey").is_ok() {
        return Err(E_NOENT); // 本模块只管两类——他类不静默
    }
    let _ = vec![0u8; 0];
    let mut marks: Vec<&str> = Vec::new();
    marks.push(WS_NAME);
    marks.push(DT_NAME);
    if marks.len() != 2 || marks[0] == marks[1] {
        return Err(E_NAME);
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
        assert_eq!(WINSTA_ALL_ACCESS, 0x37F);
        assert_eq!(WINSTA_SPEC_UNION, 0x37F);
        assert_eq!(DESKTOP_SPEC_UNION, 0x1FF);
        assert_eq!(WS_GEN_READ, 0x00020303);
    }

    #[test]
    fn conserv_holds() {
        let legal = mt::SPEC_MASK | mt::STD_USED;
        let all6 = WS_GEN_READ | WS_GEN_WRITE | WS_GEN_EXEC | DT_GEN_READ | DT_GEN_WRITE | DT_GEN_EXEC;
        assert_eq!(all6 & !legal, 0);
    }

    #[test]
    fn all_access_compose() {
        assert_eq!(winsta_all_compose(), WINSTA_ALL_ACCESS);
    }
}

pub fn run_unx36107_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36107_genmapwinsta");
    cs.add("SEG 段位独占", SEG_BASE == 0x3970, "0x3970~0x397F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "两族/装配/守恒/翻译全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("位族写死", WINSTA_SPEC_UNION == 0x37F && DESKTOP_SPEC_UNION == 0x1FF, "WINSTA 10 位 / DESKTOP 9 位锚");
    cs.add("六值守恒", (WS_GEN_READ | WS_GEN_WRITE | WS_GEN_EXEC | DT_GEN_READ | DT_GEN_WRITE | DT_GEN_EXEC) & !mt::ALL_LEGAL == 0, "六展开值 ⊆ 合法区");
    cs.add("双类型装配", WS_NAME != DT_NAME, "WindowStation/Desktop 名唯一");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_VAL => "WINSTA_/DESKTOP_ 值与写死锚不符",
        E_CONSERV => "映射越出合法区（守恒破）",
        E_DUP => "特定位号重复/乱序",
        E_NOENT => "登记后查询失败（装配破）",
        E_TRANS => "翻译值与三元组不符",
        E_COVER => "三并覆盖锚不符",
        E_NAME => "登记名不符",
        _ => "未知诊断码",
    }
}
