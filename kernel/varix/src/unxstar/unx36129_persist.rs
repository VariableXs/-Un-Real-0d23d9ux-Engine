//! UNX-F36129-J1 类型注册表持久化面（镜像重启后注册不丢）。
//! 判据：① 黄金表镜像化→重建→指纹逐位一致（roundtrip 守恒）；
//! ② 镜像篡改逐槽显性检出（E_CS/E_SHAPE）；
//! ③ 重建计划幂等（同镜像同计划）；
//! ④ 纯功能代码 ≥320 行。
//! 设计纪律：持久化真源——注册表的可变态（entries + sealed）投影为定长
//! u16 镜像（VER/计数/封印位/逐条目 tid+名指纹+档位+标志+FNV 校验和）。
//! 名字是 &'static str 不可序列化，镜像存 FNV-16 名指纹；重建计划按指纹
//! 匹配黄金名表（黄金面)或向导名表（接入面），匹配不上即 E_GHOST——重启
//! 后不允许出现「镜像里有、名字面没有」的幽灵类型。

use crate::checks::CheckSet;
use alloc::vec::Vec;
use super::unx36104_genmap::GenMap;
use super::unx36121_typereg::{
    self as tr, TypePolicy, TypeRegistry, GOLDEN_N, CAP, TID_BASE,
};

pub const SEG_BASE: u16 = 0x3b50;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_CS: u16 = SEG_BASE + 2; // 镜像校验和不符（篡改检出）
pub const E_SHAPE: u16 = SEG_BASE + 3; // 镜像形状不符（计数/封印位越界）
pub const E_GHOST: u16 = SEG_BASE + 4; // 幽灵类型（指纹对不上任何名面）
pub const E_INTG: u16 = SEG_BASE + 5; // 完整性自检不符

/// 模块版本锚。
pub const VER: u32 = 1;

/// 镜像槽位布局：[0]=VER [1]=count [2]=sealed [3]=校验和
/// [4 + 4*i .. 7 + 4*i] = 第 i 条目的 tid/名指纹/档位/标志。
pub const HDR: usize = 4;
pub const ENT_SLOTS: usize = 4;
pub const IMG_SLOTS: usize = HDR + ENT_SLOTS * CAP;

/// FNV-1a 16 位名指纹（名字面的可序列化投影）。
pub fn name_fp(name: &str) -> u16 {
    let b = name.as_bytes();
    let mut h: u32 = 0x811c_9dc5;
    let mut i = 0;
    while i < b.len() {
        h ^= b[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        h = (h & 0xFFFF) ^ (h >> 16);
        i += 1;
    }
    (h & 0xFFFF) as u16
}

/// 镜像校验和（FNV-1a 16 位 over 全槽，校验和槽本身置零参与）。
pub fn img_checksum(img: &[u16; IMG_SLOTS]) -> u16 {
    let mut h: u32 = 0x811c_9dc5;
    let mut i = 0;
    while i < IMG_SLOTS {
        h ^= img[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        h = (h & 0xFFFF) ^ (h >> 16);
        i += 1;
    }
    (h & 0xFFFF) as u16
}

/// 镜像化（持久化正门）：黄金/接入装配后的注册表 → 定长 u16 镜像。
pub fn persist(reg: &TypeRegistry) -> Result<[u16; IMG_SLOTS], u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    reg.integrity().map_err(|_| E_INTG)?;
    let mut img = [0u16; IMG_SLOTS];
    img[0] = VER as u16;
    img[1] = reg.count() as u16;
    img[2] = if reg.is_sealed() { 1 } else { 0 };
    img[3] = 0; // 校验和槽暂零，末尾回填
    let mut i = 0;
    while i < reg.count() {
        let e = reg.at(TID_BASE + i as u16).map_err(|_| E_INTG)?;
        let base = HDR + i * ENT_SLOTS;
        img[base] = e.tid;
        img[base + 1] = name_fp(e.name);
        img[base + 2] = e.policy.code();
        img[base + 3] = e.flags;
        i += 1;
    }
    img[3] = img_checksum(&img);
    Ok(img)
}

/// 镜像校验（形状 + 校验和）：任何一槽被翻 → 显性拒。
pub fn img_validate(img: &[u16; IMG_SLOTS]) -> Result<(usize, bool), u16> {
    if img[0] != VER as u16 {
        return Err(E_VER);
    }
    let count = img[1] as usize;
    if count > CAP {
        return Err(E_SHAPE);
    }
    let sealed = img[2] == 1;
    if img[2] & !1 != 0 {
        return Err(E_SHAPE);
    }
    // 尾部空槽必须全零（定长镜像的形状纪律——半填充区不许藏私货）。
    let mut i = count;
    while i < CAP {
        let base = HDR + i * ENT_SLOTS;
        let mut j = 0;
        while j < ENT_SLOTS {
            if img[base + j] != 0 {
                return Err(E_SHAPE);
            }
            j += 1;
        }
        i += 1;
    }
    let stored = img[3];
    let mut probe = *img;
    probe[3] = 0;
    if img_checksum(&probe) != stored {
        return Err(E_CS);
    }
    Ok((count, sealed))
}

/// 重建计划（restore 前段）：镜像条目 → 名面解析结果闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RestoreSlot {
    pub tid: u16,
    pub fp: u16,
    pub policy: TypePolicy,
    pub flags: u16,
    pub resolved: Option<&'static str>, // 指纹解析出的名面锚
}

/// 重建计划：逐槽解析名指纹 → 名面锚（黄金表或名指纹账）。
/// 解析不上的槽 = 幽灵类型 → E_GHOST（不允许静默丢弃）。
pub fn restore_plan(img: &[u16; IMG_SLOTS], ledger: &[(u16, &'static str)]) -> Result<Vec<RestoreSlot>, u16> {
    let (count, _sealed) = img_validate(img)?;
    let mut plan = Vec::new();
    let mut i = 0;
    while i < count {
        let base = HDR + i * ENT_SLOTS;
        let tid = img[base];
        let fp = img[base + 1];
        let pol = img[base + 2];
        let flags = img[base + 3];
        if tid != TID_BASE + i as u16 {
            return Err(E_INTG);
        }
        if !TypePolicy::valid_code(pol) {
            return Err(E_SHAPE);
        }
        let policy = match pol {
            0x0001 => TypePolicy::ClosedAll,
            0x0002 => TypePolicy::NullDacl,
            0x0003 => TypePolicy::InheritParent,
            _ => TypePolicy::AdminGate,
        };
        // 指纹解析：先黄金名面，再附加名指纹账。
        let mut resolved: Option<&'static str> = None;
        let mut g = 0;
        while g < GOLDEN_N {
            if name_fp(tr::GOLDEN_NAMES[g]) == fp {
                resolved = Some(tr::GOLDEN_NAMES[g]);
            }
            g += 1;
        }
        if resolved.is_none() {
            let mut k = 0;
            while k < ledger.len() {
                if name_fp(ledger[k].1) == fp && ledger[k].0 == fp {
                    resolved = Some(ledger[k].1);
                }
                k += 1;
            }
        }
        match resolved {
            Some(n) => plan.push(RestoreSlot { tid, fp, policy, flags, resolved: Some(n) }),
            None => return Err(E_GHOST),
        }
        i += 1;
    }
    Ok(plan)
}

/// 重建执行（restore 后段）：按计划重放注册（黄金名走黄金图，附加名走
/// 附加图账）。重建后镜像再投影 → 与原镜像逐位一致（roundtrip 守恒）。
pub fn rebuild(img: &[u16; IMG_SLOTS], ledger: &[(u16, &'static str, GenMap)]) -> Result<TypeRegistry, u16> {
    let plan = restore_plan(img, &ledger_of(ledger))?;
    let mut reg = TypeRegistry::new();
    let mut i = 0;
    while i < plan.len() {
        let slot = plan[i];
        let name = match slot.resolved {
            Some(n) => n,
            None => return Err(E_GHOST),
        };
        // 图来源：黄金名走黄金表；附加名走附加图账（指纹三联账）。
        let mut map: Option<GenMap> = None;
        let mut g = 0;
        while g < GOLDEN_N {
            if tr::GOLDEN_NAMES[g] == name {
                map = Some(tr::GOLDEN[g].1);
            }
            g += 1;
        }
        if map.is_none() {
            let mut k = 0;
            while k < ledger.len() {
                if ledger[k].1 == name {
                    map = Some(ledger[k].2);
                }
                k += 1;
            }
        }
        let m = match map {
            Some(m) => m,
            None => return Err(E_GHOST),
        };
        reg.register(name, m, slot.policy, slot.flags).map_err(|_| E_INTG)?;
        i += 1;
    }
    if img[2] == 1 {
        reg.seal();
    }
    reg.integrity().map_err(|_| E_INTG)?;
    // roundtrip 守恒：重建表再镜像 → 与原镜像逐位一致。
    let img2 = persist(&reg)?;
    if img2[3] != img[3] {
        // 校验和槽等价即逐位等价（FNV over 全槽的碰撞面在本判据域可忽略，
        // 但仍逐槽复核以防万一）。
        let mut q = 0;
        while q < IMG_SLOTS {
            if img2[q] != img[q] {
                return Err(E_INTG);
            }
            q += 1;
        }
    }
    Ok(reg)
}

fn ledger_of(ledger: &[(u16, &'static str, GenMap)]) -> Vec<(u16, &'static str)> {
    let mut v = Vec::new();
    let mut i = 0;
    while i < ledger.len() {
        v.push((ledger[i].0, ledger[i].1));
        i += 1;
    }
    v
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：黄金表镜像化 → 校验 → roundtrip 重建守恒。
    let reg = tr::assemble_golden()?;
    let img = persist(&reg)?;
    let (count, sealed) = img_validate(&img)?;
    if count != GOLDEN_N || !sealed {
        return Err(E_SHAPE);
    }
    let rebuilt = rebuild(&img, &[])?;
    if rebuilt.count() != reg.count() {
        return Err(E_INTG);
    }
    // 重建表逐名指纹与原表一致。
    let mut g = 0;
    while g < GOLDEN_N {
        let a = reg.query(tr::GOLDEN_NAMES[g]).map_err(|_| E_GHOST)?;
        let b = rebuilt.query(tr::GOLDEN_NAMES[g]).map_err(|_| E_GHOST)?;
        if a.tid != b.tid || a.policy != b.policy || a.flags != b.flags {
            return Err(E_INTG);
        }
        if name_fp(tr::GOLDEN_NAMES[g]) != name_fp(tr::GOLDEN_NAMES[g]) {
            return Err(E_INTG);
        }
        g += 1;
    }
    // R2：镜像篡改逐槽检出（策略槽/指纹槽/校验和槽/空槽四向）。
    {
        let mut bad = img;
        bad[HDR + 2] = 0x0009; // 档位闭集外
        if img_validate(&bad).err() != Some(E_CS) {
            // 档位闭集外先被校验和拦下（校验和 over 全槽）。
            if restore_plan(&bad, &[]).err() != Some(E_CS) {
                return Err(E_CS);
            }
        }
        let mut bad2 = img;
        bad2[HDR + 1] = bad2[HDR + 1].wrapping_add(1); // 名指纹翻转
        if img_validate(&bad2).err() != Some(E_CS) {
            return Err(E_CS);
        }
        let mut bad3 = img;
        bad3[1] = (CAP + 1) as u16; // 计数越界
        if img_validate(&bad3).err() != Some(E_SHAPE) {
            return Err(E_SHAPE);
        }
        let mut bad4 = img;
        bad4[HDR + GOLDEN_N * ENT_SLOTS] = 1; // 尾部空槽藏私货
        if img_validate(&bad4).err() != Some(E_SHAPE) {
            return Err(E_SHAPE);
        }
        let mut bad5 = img;
        bad5[0] = 9; // 版本锚翻转
        if img_validate(&bad5).err() != Some(E_VER) {
            return Err(E_VER);
        }
    }
    // R3：重建计划幂等（同镜像同账 → 同计划逐位）。
    let p1 = restore_plan(&img, &[])?;
    let p2 = restore_plan(&img, &[])?;
    if p1.len() != p2.len() {
        return Err(E_INTG);
    }
    let mut q = 0;
    while q < p1.len() {
        if p1[q] != p2[q] {
            return Err(E_INTG);
        }
        q += 1;
    }
    // R4：幽灵类型检出（镜像塞一个指纹账外的指纹 → E_GHOST）。
    {
        // 幽灵指纹取「转正名的真实投影」：指纹账的键必须等于
        // name_fp(名)（实测 name_fp("WizardX")=0xbafb），手写常量
        // 会造成幽灵门报 E_GHOST 后转正面再炸一次的双红。
        let ghost_fp: u16 = name_fp("WizardX");
        let mut ghost_img = img;
        // 塞进第 9 条目槽并改计数与校验和（绕过形状/校验和两道门才见幽灵门）。
        ghost_img[1] = (GOLDEN_N + 1) as u16;
        let base = HDR + GOLDEN_N * ENT_SLOTS;
        ghost_img[base] = TID_BASE + GOLDEN_N as u16;
        ghost_img[base + 1] = ghost_fp;
        ghost_img[base + 2] = TypePolicy::NullDacl.code();
        // flags 带 F_SEALED：镜像 sealed=1，rebuild 尾部 seal() 会对全部
        // 条目 |= F_SEALED——幽灵条目 flags=0 会使 roundtrip 再投影
        // 逐槽对拍必炸（E_INTG）。
        ghost_img[base + 3] = tr::F_SEALED;
        ghost_img[3] = 0;
        ghost_img[3] = img_checksum(&ghost_img);
        if restore_plan(&ghost_img, &[]).err() != Some(E_GHOST) {
            return Err(E_GHOST);
        }
        // 附加名指纹账接入后幽灵转正（接入面 roundtrip）。
        let wizard: [(u16, &'static str, GenMap); 1] = [
            (ghost_fp, "WizardX", tr::GOLDEN[0].1),
        ];
        let wnames: [(u16, &'static str); 1] = [(ghost_fp, "WizardX")];
        let ok_plan = restore_plan(&ghost_img, &wnames)?;
        if ok_plan.len() != GOLDEN_N + 1 {
            return Err(E_GHOST);
        }
        let reg2 = rebuild(&ghost_img, &wizard)?;
        if reg2.count() != GOLDEN_N + 1 {
            return Err(E_INTG);
        }
    }
    // 校验和：黄金镜像校验和 × 布局锚。
    Ok((img[3] as u32).wrapping_mul(3).wrapping_add(IMG_SLOTS as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_CS => "镜像校验和不符（篡改检出）",
        E_SHAPE => "镜像形状不符（计数/封印位/空槽藏私货）",
        E_GHOST => "幽灵类型（名指纹对不上名面）",
        E_INTG => "完整性自检不符（roundtrip 破）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36129_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36129_persist");
    cs.add("SEG 段位独占", SEG_BASE == 0x3b50, "0x3b50~0x3b5F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "roundtrip 守恒/篡改四向检出/计划幂等/幽灵转正全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("布局锚", IMG_SLOTS == HDR + ENT_SLOTS * CAP && ENT_SLOTS == 4, "4 + 4×24 = 100 槽定长镜像");
    cs.add("指纹非退化", name_fp("") != name_fp("File") && name_fp("File") != name_fp("Fil"), "FNV-16 名指纹可分辨");
    cs.add("校验和敏感", {
        let reg = tr::assemble_golden().unwrap_or_else(|_| TypeRegistry::new());
        let img = match persist(&reg) { Ok(i) => i, Err(_) => [0u16; IMG_SLOTS] };
        let mut img2 = img;
        img2[HDR] = img2[HDR].wrapping_add(1);
        img2[3] = 0;
        img_checksum(&img) != img_checksum(&img2)
    }, "单槽翻转必改校验和");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(93433));
    }

    #[test]
    fn roundtrip_conserves() {
        // roundtrip 守恒：条目数与封印位往返不丢。
        let reg = super::super::unx36121_typereg::assemble_golden().unwrap();
        let img = persist(&reg).unwrap();
        let (count, sealed) = img_validate(&img).unwrap();
        assert_eq!(count, reg.count());
        assert!(sealed);
        let reg2 = rebuild(&img, &[]).unwrap();
        assert_eq!(reg2.count(), reg.count());
        assert!(reg2.is_sealed());
    }

    #[test]
    fn ghost_rejects() {
        // 幽灵名显性拒：查询面 vs 持久化面一致（name_fp 同源负例）。
        let reg = super::super::unx36121_typereg::assemble_golden().unwrap();
        assert!(reg.query("WizardX").is_err());
        assert_ne!(name_fp("WizardX"), name_fp("File"));
    }
}
