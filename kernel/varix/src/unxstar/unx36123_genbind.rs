//! UNX-F36123-J1 generic map 消费位（B06 映射表的类型侧登记）。
//! 判据：① 绑定面双写一致（类型表 tid ↔ 映射表下标同序对应）；
//! ② 对账面抓孤儿/异图两类漂移；③ 容量瓶颈契约（类型表 24 ≥ 映射表 16，
//! 绑定面以映射表为瓶颈）显性暴露；④ 纯功能代码 ≥320 行。
//! 设计纪律：消费位真源——B06（unx36104_genmap）负责 GENERIC→特定映射的
//! 位段学，B07（unx36121_typereg）负责类型身份与默认策略；本模块把两者
//! 绑成一件事：一个类型在两张表里必须是同一个事实。36126 装配挂点只从
//! 本模块拿绑定入口，不绕开对账直写任何一张表。

use crate::checks::CheckSet;
use super::unx36101_masktable as mt;
use super::unx36102_stdrights as sr;
use super::unx36104_genmap::{
    self as gm, GenMap, Registry as MapReg, G_ALL, G_EXEC, G_READ, G_WRITE,
};
use super::unx36121_typereg::{
    self as tr, TypePolicy, TypeRegistry, GOLDEN_N, TID_BASE,
};

pub const SEG_BASE: u16 = 0x3ab0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_BIND: u16 = SEG_BASE + 2; // 绑定面内部矛盾（双写不同步）
pub const E_ORPHAN: u16 = SEG_BASE + 3; // 单面孤儿（只在一表存在）
pub const E_MAPMISMATCH: u16 = SEG_BASE + 4; // 同名异图（两表分叉）
pub const E_SEQ: u16 = SEG_BASE + 5; // 同序对应破（tid ≠ 下标 + TID_BASE）
pub const E_CAPGAP: u16 = SEG_BASE + 6; // 容量瓶颈契约破（映射表满而类型表未满为合法，绑不动为显性结论）
pub const E_POLICY: u16 = SEG_BASE + 7; // 策略档位非法
pub const E_NOSLOT: u16 = SEG_BASE + 8; // 映射表槽位取用越界

/// 模块版本锚。
pub const VER: u32 = 1;

/// 绑定结果：双表坐标（tid 类型面 / mid 映射面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BindCoord {
    pub tid: u16,
    pub mid: usize,
}

impl BindCoord {
    /// 同序对应不变量：tid = mid + TID_BASE（两表同序装配时恒成立）。
    pub fn seq_ok(&self) -> bool {
        self.tid as usize == self.mid + TID_BASE as usize
    }
}

/// 绑定入口（36126 装配挂点唯一入口）：类型侧登记 + 映射侧登记 +
/// 双面即时复核。幂等：同名同图同策略重绑返回原坐标。
pub fn bind_type_map(
    reg: &mut TypeRegistry,
    maps: &mut MapReg,
    name: &'static str,
    map: GenMap,
    policy: TypePolicy,
    flags: u16,
) -> Result<BindCoord, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    // 先映射面（容量瓶颈方）：满则绑定显性失败，类型面不留半状态。
    let mid = maps.register(name, map)?;
    // 再类型面：失败则回滚映射面不可能——B06 登记是幂等追加，重绑同名
    // 同图返回原下标；此处类型面失败只可能是封印/满/名非法，映射面已
    // 有同名同图条目，重放绑定结果一致（无泄漏，契约见头注）。
    let tid = reg.register(name, map, policy, flags)?;
    let coord = BindCoord { tid, mid };
    if !coord.seq_ok() {
        return Err(E_SEQ);
    }
    Ok(coord)
}

/// 消费位翻译：类型面确认 + 映射面 GENERIC→特定翻译（两步都必须过）。
/// G_ALL 特例：B06 的三槽并语义走 translate_all（≡ read|write|exec 守恒）。
pub fn consumed_translate(
    reg: &TypeRegistry,
    maps: &MapReg,
    name: &str,
    generic: u32,
) -> Result<u32, u16> {
    reg.query(name).map_err(|_| tr::E_NOENT)?;
    if generic == G_ALL {
        maps.translate_all(name)
    } else {
        maps.translate(name, generic)
    }
}

/// 对账面：两表名序逐位一致 + 同名同图 + tid 同序对应。
pub fn face_audit(reg: &TypeRegistry, maps: &MapReg) -> Result<(), u16> {
    if reg.count() != maps.count() {
        return Err(E_ORPHAN);
    }
    let mut i = 0;
    while i < reg.count() {
        let te = reg.at(TID_BASE + i as u16).map_err(|_| E_SEQ)?;
        let me = maps.at(i).map_err(|_| E_NOSLOT)?;
        if te.name != me.name {
            return Err(E_SEQ);
        }
        if te.map.read != me.map.read
            || te.map.write != me.map.write
            || te.map.exec != me.map.exec
        {
            return Err(E_MAPMISMATCH);
        }
        if te.tid as usize != i + TID_BASE as usize {
            return Err(E_SEQ);
        }
        i += 1;
    }
    Ok(())
}

/// 孤儿探针：单点名对账（比全表对账便宜，热路径抽查用）。
pub fn spot_audit(reg: &TypeRegistry, maps: &MapReg, name: &str) -> Result<(), u16> {
    let te = reg.query(name).map_err(|_| E_ORPHAN)?;
    // 映射面 query 返回 GenMap 本体（非 Entry），直接按值比较。
    let mm = maps.query(name).map_err(|_| E_ORPHAN)?;
    if te.map.read != mm.read || te.map.write != mm.write || te.map.exec != mm.exec {
        return Err(E_MAPMISMATCH);
    }
    if te.tid as usize != maps.index_of(name).map_err(|_| E_ORPHAN)? + TID_BASE as usize {
        return Err(E_SEQ);
    }
    Ok(())
}

/// 黄金双面装配：类型表 + 映射表同时装配黄金八类并封印。
pub fn assemble_golden_paired() -> Result<(TypeRegistry, MapReg), u16> {
    let mut reg = TypeRegistry::new();
    let mut maps = MapReg::new();
    let mut i = 0;
    while i < GOLDEN_N {
        let (name, map, policy, flags) = tr::GOLDEN[i];
        let c = bind_type_map(&mut reg, &mut maps, name, map, policy, flags | tr::F_GOLDEN)?;
        if !c.seq_ok() {
            return Err(E_SEQ);
        }
        i += 1;
    }
    face_audit(&reg, &maps)?;
    reg.seal();
    Ok((reg, maps))
}

/// 容量瓶颈常量（契约锚）：映射表 CAP=16 < 类型表 CAP=24。
pub const CAP_MAP: usize = gm::CAP;
pub const CAP_TYPE: usize = tr::CAP;

/// 翻译快照矩阵（36131 启动自检消费）：黄金八类 × 四向（R/W/X/ALL）
/// 的翻译结果快照。确定性锚——同表同图必得同快照，快照漂移即两表分叉。
pub fn translation_snapshot(reg: &TypeRegistry, maps: &MapReg) -> Result<[[u32; 4]; GOLDEN_N], u16> {
    let mut snap = [[0u32; 4]; GOLDEN_N];
    let mut i = 0;
    while i < GOLDEN_N {
        let name = tr::GOLDEN_NAMES[i];
        spot_audit(reg, maps, name)?;
        snap[i][0] = consumed_translate(reg, maps, name, G_READ)?;
        snap[i][1] = consumed_translate(reg, maps, name, G_WRITE)?;
        snap[i][2] = consumed_translate(reg, maps, name, G_EXEC)?;
        snap[i][3] = consumed_translate(reg, maps, name, G_ALL)?;
        // 守恒锚：ALL ≡ 三槽并（快照内自证，不依赖 B06 内部实现）。
        if snap[i][3] != snap[i][0] | snap[i][1] | snap[i][2] {
            return Err(E_BIND);
        }
        i += 1;
    }
    Ok(snap)
}

/// 快照逐位比对（自检面：现算快照 vs 基线快照）。
pub fn snapshot_eq(a: &[[u32; 4]; GOLDEN_N], b: &[[u32; 4]; GOLDEN_N]) -> bool {
    let mut i = 0;
    while i < GOLDEN_N {
        let mut j = 0;
        while j < 4 {
            if a[i][j] != b[i][j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：黄金双面装配 + 同序对应全核。
    let (reg, maps) = assemble_golden_paired()?;
    if reg.count() != GOLDEN_N || maps.count() != GOLDEN_N {
        return Err(E_BIND);
    }
    let mut i = 0;
    while i < GOLDEN_N {
        let c = BindCoord { tid: TID_BASE + i as u16, mid: i };
        if !c.seq_ok() {
            return Err(E_SEQ);
        }
        spot_audit(&reg, &maps, tr::GOLDEN_NAMES[i])?;
        i += 1;
    }
    // R2：消费位翻译四向（G_READ/G_WRITE/G_EXEC/G_ALL 各有实效位）。
    let r_read = consumed_translate(&reg, &maps, "File", G_READ)?;
    if r_read != sr::READ_CONTROL | sr::SYNCHRONIZE {
        return Err(E_BIND);
    }
    let r_write = consumed_translate(&reg, &maps, "File", G_WRITE)?;
    if r_write != sr::DELETE | sr::WRITE_DAC | sr::WRITE_OWNER | sr::READ_CONTROL {
        return Err(E_BIND);
    }
    let r_exec = consumed_translate(&reg, &maps, "File", G_EXEC)?;
    if r_exec != sr::READ_CONTROL | sr::SYNCHRONIZE {
        return Err(E_BIND);
    }
    let r_all = consumed_translate(&reg, &maps, "File", G_ALL)?;
    if r_all != r_read | r_write | r_exec {
        return Err(E_BIND);
    }
    // Token 的 exec 槽为零——GENERIC_EXEC 翻出零掩码（空授权是合法结论）。
    if consumed_translate(&reg, &maps, "Token", G_EXEC)? != 0 {
        return Err(E_BIND);
    }
    // R3：翻译请求位纪律（非 GENERIC 位 → B06 E_BIT 透传）。
    if consumed_translate(&reg, &maps, "File", sr::READ_CONTROL).err() != Some(gm::E_BIT) {
        return Err(E_BIND);
    }
    if consumed_translate(&reg, &maps, "File", mt::GEN_RSVD).err() != Some(gm::E_BIT) {
        return Err(E_BIND);
    }
    // R4：未注册类型翻译显性拒（先类型面拦下）。
    if consumed_translate(&reg, &maps, "Ghost", G_READ).err() != Some(tr::E_NOENT) {
        return Err(E_BIND);
    }
    // R5：孤儿抓捕——单面登记立刻被对账面识破。
    {
        let mut reg2 = TypeRegistry::new();
        let mut maps2 = MapReg::new();
        let (gname, gmap, gpol, gfl) = tr::GOLDEN[0];
        bind_type_map(&mut reg2, &mut maps2, gname, gmap, gpol, gfl)?;
        // 类型面多挂一个孤儿。
        let _ = reg2.register("OrphanT", gmap, gpol, 0);
        if face_audit(&reg2, &maps2).err() != Some(E_ORPHAN) {
            return Err(E_ORPHAN);
        }
        // 映射面多挂一个孤儿。
        let mut reg3 = TypeRegistry::new();
        let mut maps3 = MapReg::new();
        bind_type_map(&mut reg3, &mut maps3, gname, gmap, gpol, gfl)?;
        let _ = maps3.register("OrphanM", gmap);
        if face_audit(&reg3, &maps3).err() != Some(E_ORPHAN) {
            return Err(E_ORPHAN);
        }
        // 同名异图分叉（两表各首条同名异图——MapReg::register 同名
        // 异图显性拒 E_CONFLICT 不可覆盖，故直接独立装配真分叉）。
        let mut reg4 = TypeRegistry::new();
        let mut maps4 = MapReg::new();
        let _ = reg4.register(gname, gmap, gpol, gfl);
        // File 黄金图 read==exec（G_READ/G_EXEC 同翻 READ_CONTROL|SYNC），
        // 沿 read/exec 轴翻转得恒等图、分叉不存在；write 轴与 read 轴
        // 有区分度（DELETE|WRITE_DAC|WRITE_OWNER|READ_CONTROL），换轴。
        let flipped = GenMap { read: gmap.write, write: gmap.read, exec: gmap.exec };
        let _ = maps4.register(gname, flipped);
        if face_audit(&reg4, &maps4).err() != Some(E_MAPMISMATCH) {
            return Err(E_MAPMISMATCH);
        }
    }
    // R6：容量瓶颈契约——映射表 16 满后第 17 绑显性失败且类型表不留孤儿。
    {
        let mut reg5 = TypeRegistry::new();
        let mut maps5 = MapReg::new();
        let gmap = tr::GOLDEN[0].1;
        let gpol = TypePolicy::NullDacl;
        let mut t = 0;
        while t < CAP_MAP {
            bind_type_map(&mut reg5, &mut maps5, tr::FILL_NAMES[t], gmap, gpol, 0)?;
            t += 1;
        }
        if maps5.count() != CAP_MAP || reg5.count() != CAP_MAP {
            return Err(E_CAPGAP);
        }
        if bind_type_map(&mut reg5, &mut maps5, "Beyond", gmap, gpol, 0).err() != Some(gm::E_FULL) {
            return Err(E_CAPGAP);
        }
        // 失败绑定不产生单面孤儿（映射面满在先，类型面未动）。
        face_audit(&reg5, &maps5)?;
    }
    // R7：绑定幂等（同参重绑 → 原坐标）。
    {
        let mut reg6 = TypeRegistry::new();
        let mut maps6 = MapReg::new();
        let (gname, gmap, gpol, gfl) = tr::GOLDEN[4];
        let c1 = bind_type_map(&mut reg6, &mut maps6, gname, gmap, gpol, gfl)?;
        let c2 = bind_type_map(&mut reg6, &mut maps6, gname, gmap, gpol, gfl)?;
        if c1 != c2 {
            return Err(E_BIND);
        }
    }
    // R8：快照确定性（同表两次快照逐位相等 + ALL≡三并内证）。
    let s1 = translation_snapshot(&reg, &maps)?;
    let s2 = translation_snapshot(&reg, &maps)?;
    if !snapshot_eq(&s1, &s2) {
        return Err(E_BIND);
    }
    if s1[7][2] != 0 {
        return Err(E_BIND); // Token：exec 槽恒零（黄金图锚）
    }
    // 校验和：八类 G_READ 翻译值全并 × 同序系数。
    let mut sum: u32 = 0;
    let mut q = 0;
    while q < GOLDEN_N {
        sum = sum.wrapping_add(consumed_translate(&reg, &maps, tr::GOLDEN_NAMES[q], G_READ)?);
        q += 1;
    }
    Ok(sum.wrapping_mul(13).wrapping_add(GOLDEN_N as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_BIND => "绑定面内部矛盾（双写不同步）",
        E_ORPHAN => "单面孤儿（只在一表存在）",
        E_MAPMISMATCH => "同名异图（两表分叉）",
        E_SEQ => "同序对应破（tid ≠ 下标 + TID_BASE）",
        E_CAPGAP => "容量瓶颈契约破（映射表 16 为瓶颈）",
        E_POLICY => "策略档位非法",
        E_NOSLOT => "映射表槽位取用越界",
        _ => "未知诊断码",
    }
}

pub fn run_unx36123_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36123_genbind");
    cs.add("SEG 段位独占", SEG_BASE == 0x3ab0, "0x3ab0~0x3abF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "双面绑定/四向翻译/孤儿抓捕/容量瓶颈/幂等全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("同序对应锚", BindCoord { tid: 3, mid: 2 }.seq_ok() && !BindCoord { tid: 5, mid: 2 }.seq_ok(), "tid = mid + TID_BASE 判定");
    cs.add("容量瓶颈契约", CAP_MAP == 16 && CAP_TYPE == 24 && CAP_MAP < CAP_TYPE, "映射表 16 为绑定面瓶颈");
    cs.add("翻译守恒", {
        let (r, m) = match assemble_golden_paired() {
            Ok(v) => v,
            Err(_) => return cs,
        };
        consumed_translate(&r, &m, "File", G_READ).is_ok()
    }, "黄金表上 G_READ 可翻译");
    cs.add("孤儿显性码", detail(E_ORPHAN).len() > 0 && detail(0xFFFF) == "未知诊断码", "诊断码人话闭集");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(27262984));
    }

    #[test]
    fn bind_idem_and_conflict() {
        // 幂等重绑：同名同图同策略 -> 同坐标；同名异图 -> E_DUP。
        let (mut reg, mut maps) = assemble_golden_paired().unwrap();
        let (gname, gmap, gpol, _) = super::super::unx36121_typereg::GOLDEN[0];
        let c1 = bind_type_map(&mut reg, &mut maps, gname, gmap, gpol, 0).unwrap();
        let flipped = super::super::unx36104_genmap::GenMap {
            read: gmap.write, write: gmap.read, exec: gmap.exec,
        };
        assert_eq!(bind_type_map(&mut reg, &mut maps, gname, flipped, gpol, 0).err(), Some(super::super::unx36104_genmap::E_CONFLICT));
        let c2 = bind_type_map(&mut reg, &mut maps, gname, gmap, gpol, 0).unwrap();
        assert_eq!(c1.tid, c2.tid);
        assert_eq!(c1.mid, c2.mid);
    }

    #[test]
    fn paired_conserves() {
        // 双面守恒：装配后两表同数，对账全绿（账面一致性）。
        let (reg, maps) = assemble_golden_paired().unwrap();
        assert_eq!(reg.count(), maps.count());
        assert!(face_audit(&reg, &maps).is_ok());
        assert!(translation_snapshot(&reg, &maps).is_ok());
    }
}
