//! UNX-F36221-J1 GrantedAccess 字段定义与句柄表挂接（冻结接口消费）。
//! 判据：① GA 字段 = 槽字低 24 位（0x00FF_FFFF），属性域 = 高 8 位
//! （0xFF00_0000），两域零重叠——静态断言（const 块）在编译期拦截任何
//! 域重叠形态；② 句柄表挂接 100 例读回一致（attach→readback 逐槽位
//! 恒等）；③ 字段越权写入（非本域路径：绕过 attach 直接改写 GA 域或
//! 属性域越出合法集）注入 10 例全部被守卫拦截；④ 纯功能代码 ≥320 行。
//! 设计纪律：对标 Windows ob 句柄表 OBJECT_TABLE_ENTRY——GrantedAccess
//! 与句柄属性同居一个槽字、以位域切分；存储面 GA 恒不含 GENERIC 位
//! （GENERIC 在打开时映射掉，存储的恒是特定|标准位）；表为定容开放
//! 定址（槽位顺序分配 + 代数 generation 防陈旧句柄复用）。

use crate::checks::CheckSet;
use super::unx36101_masktable as mt;
use super::unx36102_stdrights as sr;

pub const SEG_BASE: u16 = 0xa150;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_FIELD: u16 = SEG_BASE + 2; // GA 域越出低 24 位（域重叠/越界）
pub const E_ATTR: u16 = SEG_BASE + 3; // 属性域越出合法属性集
pub const E_ATTACH: u16 = SEG_BASE + 4; // 挂接非法（读回不一致/槽位非法）
pub const E_CAP: u16 = SEG_BASE + 5; // 表满/槽位越界
pub const E_STALE: u16 = SEG_BASE + 6; // 代数不符（陈旧句柄）

/// 模块版本锚。
pub const VER: u32 = 1;

// ---- 字段位域（冻结接口——B12 全批从这里取域锚） ---------------------------
/// GA 域掩码：槽字低 24 位（特定 0..15 | 标准 16..20 五位）。
pub const GA_FIELD: u32 = 0x00FF_FFFF;
/// 属性域掩码：槽字高 8 位（bit 24..31）。
pub const ATTR_FIELD: u32 = 0xFF00_0000;
/// 属性域合法位（闭集四属性）。
pub const HA_INHERIT: u32 = 1 << 24; // 子进程可继承
pub const HA_AUDIT_CLOSE: u32 = 1 << 25; // 关闭时审计
pub const HA_PROTECT: u32 = 1 << 26; // 保护关闭（不可句柄关闭，仅随表销毁）
pub const HA_PRESET: u32 = 1 << 27; // 打开即预设（首槽标记）
pub const ATTR_LEGAL: u32 = HA_INHERIT | HA_AUDIT_CLOSE | HA_PROTECT | HA_PRESET;

/// 存储面 GA 合法区：特定全段 + 标准五位（GENERIC 位打开时已映射掉——
/// Windows ob 语义：句柄表里不存 GENERIC 位）。
pub const GA_STORE_LEGAL: u32 = mt::SPEC_MASK | sr::STD_ALL;

// ---- 静态断言（字段域隔离的编译期红线——判据 ② 的第一道锁） ----------------
const _: () = assert!(GA_FIELD & ATTR_FIELD == 0, "GA 域与属性域重叠");
const _: () = assert!(GA_FIELD.count_ones() == 24, "GA 域必须是低 24 位");
const _: () = assert!(ATTR_FIELD.count_ones() == 8, "属性域必须是高 8 位");
const _: () = assert!(GA_FIELD | ATTR_FIELD == u32::MAX, "两域必须并满 32 位");
const _: () = assert!(GA_STORE_LEGAL & GA_FIELD == GA_STORE_LEGAL, "存储合法区越出 GA 域");
const _: () = assert!(ATTR_LEGAL & !ATTR_FIELD == 0, "合法属性集越出属性域");
const _: () = assert!(HA_INHERIT | HA_AUDIT_CLOSE | HA_PROTECT | HA_PRESET == ATTR_LEGAL, "属性闭集自洽破");

/// 域守卫：任何"非本域路径"对槽字的直接写都必须先过此门。
/// GA 域写入越出存储合法区 → E_FIELD；属性域写入越出合法属性集 → E_ATTR。
/// 这是字段越权注入 10 例的拦截器（判据 ③ 的落点）。
pub fn field_guard(word: u32) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let ga = word & GA_FIELD;
    let attrs = word & ATTR_FIELD;
    if ga & !GA_STORE_LEGAL != 0 {
        return Err(E_FIELD);
    }
    if attrs & !ATTR_LEGAL != 0 {
        return Err(E_ATTR);
    }
    Ok(())
}

/// 句柄槽（冻结接口：B12 全批消费本形态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entry {
    /// 对象标识（域内闭集编号）。
    pub obj: u32,
    /// 授出掩码（恒 ⊆ GA_STORE_LEGAL）。
    pub ga: u32,
    /// 属性位（恒 ⊆ ATTR_LEGAL）。
    pub attrs: u32,
    /// 槽代数（复用一次加一——防陈旧句柄）。
    pub gen: u32,
    /// 授权戳（打开路径写入；36222 消费）。
    pub auth_epoch: u32,
}

const NONE: Option<Entry> = None;

/// 空槽判别：release 留下的是「墓碑槽」（obj/ga/attrs 全零、gen 保留）——
/// attach 扫描时视作可复用空位，read 时视作陈旧（代数键不匹配即 E_STALE）。
pub fn is_free_entry(e: &Entry) -> bool {
    e.obj == 0 && e.ga == 0 && e.attrs == 0
}

/// 定容句柄表（256 槽——J1 域句柄面容量闭集）。
pub const TABLE_CAP: usize = 256;

pub struct HandleTable {
    slots: [Option<Entry>; TABLE_CAP],
    used: usize,
    epoch: u32,
}

impl HandleTable {
    pub fn new() -> HandleTable {
        HandleTable { slots: [NONE; TABLE_CAP], used: 0, epoch: 1 }
    }

    pub fn used(&self) -> usize {
        self.used
    }

    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    /// 挂接：最低空槽分配；槽字先过域守卫（字段越权在入口即拦）。
    /// 返回 (槽位, 代数)。
    pub fn attach(&mut self, obj: u32, ga: u32, attrs: u32) -> Result<(usize, u32), u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        // 先拼槽字再过守卫——与"非本域路径写入门"同一判据面。
        let word = (ga & GA_FIELD) | (attrs & ATTR_FIELD);
        field_guard(word)?;
        if self.used >= TABLE_CAP {
            return Err(E_CAP);
        }
        let mut slot = 0usize;
        while slot < TABLE_CAP {
            match self.slots[slot] {
                None => break,
                Some(ref e) if is_free_entry(e) => break,
                _ => slot += 1,
            }
        }
        if slot >= TABLE_CAP {
            return Err(E_CAP);
        }
        let gen = match self.slots[slot] {
            Some(e) => e.gen.wrapping_add(1),
            None => 1,
        };
        self.slots[slot] = Some(Entry { obj, ga, attrs, gen, auth_epoch: self.epoch });
        self.used += 1;
        self.epoch = self.epoch.wrapping_add(1);
        Ok((slot, gen))
    }

    /// 读回（判据 ② 的读回面：槽位+代数双键，防陈旧）。
    pub fn read(&self, slot: usize, gen: u32) -> Result<Entry, u16> {
        if slot >= TABLE_CAP {
            return Err(E_CAP);
        }
        match self.slots[slot] {
            // 墓碑槽（obj/ga/attrs 全零）虽 gen 相同也按陈旧处理——句柄已关。
            Some(e) if e.gen == gen && !is_free_entry(&e) => Ok(e),
            Some(_) => Err(E_STALE),
            None => Err(E_CAP),
        }
    }

    /// 非本域路径的直接写入口（仅供守卫对拍与注入演练使用）：
    /// 不做任何业务校验，只过域守卫——越权注入在此被拦。
    pub fn raw_write_probe(&self, word: u32) -> Result<(), u16> {
        field_guard(word)
    }

    /// 全表扫描：所有在槽条目域合法（GA ⊆ 合法区，属性 ⊆ 合法集）。
    pub fn sweep_legal(&self) -> Result<(), u16> {
        let mut i = 0;
        while i < TABLE_CAP {
            if let Some(e) = self.slots[i] {
                if e.ga & !GA_STORE_LEGAL != 0 {
                    return Err(E_FIELD);
                }
                if e.attrs & !ATTR_LEGAL != 0 {
                    return Err(E_ATTR);
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 直读（不看代数——清扫器/账面对账用；空槽返回 None）。
    pub fn entry_at(&self, slot: usize) -> Option<Entry> {
        if slot >= TABLE_CAP {
            return None;
        }
        self.slots[slot]
    }

    /// 属性过滤（36232 继承清单消费）。
    pub fn slots_with_attr(&self, attr: u32) -> alloc::vec::Vec<usize> {
        let mut out = alloc::vec::Vec::new();
        let mut i = 0;
        while i < TABLE_CAP {
            if let Some(e) = self.slots[i] {
                if e.attrs & attr != 0 {
                    out.push(i);
                }
            }
            i += 1;
        }
        out
    }

    /// 释放槽（gen 保留——读旧代数句柄 → E_STALE）。
    pub fn release(&mut self, slot: usize, gen: u32) -> Result<(), u16> {
        if slot >= TABLE_CAP {
            return Err(E_CAP);
        }
        match self.slots[slot] {
            // 墓碑槽不是活句柄——重复关闭按陈旧拒（清算账防双扣）。
            Some(e) if e.gen == gen && !is_free_entry(&e) => {
                self.slots[slot] = Some(Entry { obj: 0, ga: 0, attrs: 0, gen: e.gen, auth_epoch: 0 });
                self.used -= 1;
                self.epoch = self.epoch.wrapping_add(1);
                Ok(())
            }
            Some(_) => Err(E_STALE),
            None => Err(E_CAP),
        }
    }
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：静态断言面已经编译期成立；这里核域守卫的正反例。
    if field_guard(sr::READ_CONTROL | sr::SYNCHRONIZE).is_err() {
        return Err(E_FIELD);
    }
    if field_guard(HA_INHERIT | HA_AUDIT_CLOSE).is_err() {
        return Err(E_ATTR);
    }
    // GENERIC 位在存储面非法（打开时映射掉的语义锚）。
    if field_guard(0x8000_0000).err() != Some(E_ATTR) {
        return Err(E_ATTR);
    }
    // MA 位 0x0200_0000 在 36221 槽字布局里 = HA_AUDIT_CLOSE（合法属性）；
    // 属性域的非法代表位取 bit 28（B06 GEN_USED 区起点——两套布局不混用）。
    if field_guard(0x1000_0000).err() != Some(E_ATTR) {
        return Err(E_ATTR);
    }
    // 标准段保留位（bit 21..23）在存储面非法。
    if field_guard(0x0020_0000).err() != Some(E_FIELD) {
        return Err(E_FIELD);
    }
    // R2：字段越权写入注入 10 例（判据 ③ 落点）。
    // 注入集：GA 域越界 5 例 + 属性域越界 5 例，全被 field_guard 拦截。
    const GA_INJ: [u32; 5] = [
        0x0020_0000,               // 标准保留位 bit 21
        0x0040_0000,               // 标准保留位 bit 22
        0x0080_0000,               // 标准保留位 bit 23
        0x00FF_FFFF,               // GA 域满——含三位保留位，非法
        0x0010_0000 | 0x0040_0000, // SYNCHRONIZE + 保留位
    ];
    const ATTR_INJ: [u32; 5] = [
        1 << 28,       // 属性保留位 28
        1 << 29,       // 属性保留位 29
        1 << 30,       // 属性保留位 30
        1 << 31,       // 属性保留位 31
        ATTR_LEGAL | (1 << 28), // 合法集 + 越界位
    ];
    let mut inj_caught = 0u32;
    let mut i = 0;
    while i < 5 {
        // GA 注入例：GA 位段带非法位 → E_FIELD（属性位为零故只犯 GA 域）。
        let w = GA_INJ[i] & GA_FIELD;
        if field_guard(w).err() == Some(E_FIELD) {
            inj_caught += 1;
        }
        // 属性注入例。
        if field_guard(ATTR_INJ[i]).err() == Some(E_ATTR) {
            inj_caught += 1;
        }
        i += 1;
    }
    if inj_caught != 10 {
        return Err(E_ATTACH);
    }
    // R3：挂接 100 例读回一致（判据 ② 落点）。
    let mut t = HandleTable::new();
    let mut gens = [0u32; 100];
    let mut i = 0;
    while i < 100 {
        // 授出掩码循环取五个合法样本；属性循环取闭集组合。
        let ga = [
            sr::READ_CONTROL,
            sr::DELETE | sr::READ_CONTROL,
            sr::STD_ALL,
            sr::SYNCHRONIZE,
            sr::READ_CONTROL | sr::WRITE_DAC,
        ][i % 5];
        let attrs = [
            0,
            HA_INHERIT,
            HA_AUDIT_CLOSE,
            HA_INHERIT | HA_AUDIT_CLOSE,
            HA_PRESET,
        ][i % 5];
        let (slot, gen) = t.attach(0x1000 + i as u32, ga, attrs)?;
        let back = t.read(slot, gen)?;
        if back.obj != 0x1000 + i as u32 || back.ga != ga || back.attrs != attrs {
            return Err(E_ATTACH);
        }
        gens[i] = gen;
        i += 1;
    }
    if t.used() != 100 || t.sweep_legal().is_err() {
        return Err(E_ATTACH);
    }
    // 陈旧句柄：释放后按旧代数读 → E_STALE。
    t.release(3, gens[3])?;
    if t.read(3, gens[3]).err() != Some(E_STALE) {
        return Err(E_STALE);
    }
    // 复用槽位代数递增（墓碑槽被 attach 复用，旧代数句柄随后全数失效）。
    let (slot2, gen2) = t.attach(0x2000, sr::READ_CONTROL, 0)?;
    if slot2 != 3 || gen2 != gens[3] + 1 {
        return Err(E_ATTACH);
    }
    // 复用后账面代数同步——后续校验和循环按新代数读回。
    gens[3] = gen2;
    // R4：容量闭集（256 槽满 → E_CAP）。
    let mut big = HandleTable::new();
    let mut k = 0;
    while k < TABLE_CAP {
        big.attach(k as u32, sr::READ_CONTROL, 0)?;
        k += 1;
    }
    if big.attach(0, sr::READ_CONTROL, 0).err() != Some(E_CAP) {
        return Err(E_CAP);
    }
    if big.read(TABLE_CAP, 1).err() != Some(E_CAP) {
        return Err(E_CAP);
    }
    // 校验和：100 例读回掩码求和（确定性总锚）。
    let mut sum: u32 = 0;
    let mut i = 0;
    while i < 100 {
        let e = t.read(i, gens[i])?;
        sum = sum.wrapping_add(e.ga).wrapping_add(e.attrs).wrapping_add(e.gen);
        i += 1;
    }
    Ok(sum.wrapping_add(inj_caught).wrapping_add(t.epoch()))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_FIELD => "GA 域越出存储合法区（GENERIC/保留位入槽）",
        E_ATTR => "属性域越出合法属性集",
        E_ATTACH => "挂接/读回不一致或越权注入未被全拦",
        E_CAP => "表满或槽位越界",
        E_STALE => "代数不符（陈旧句柄）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36221_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36221_gafield");
    cs.add("SEG 段位独占", SEG_BASE == 0xa150, "0xa150~0xa15F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "域隔离/10 例注入全拦/100 例读回一致/容量闭集"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("字段域锚", GA_FIELD == 0x00FF_FFFF && ATTR_FIELD == 0xFF00_0000 && GA_FIELD & ATTR_FIELD == 0, "GA 低 24 位 / 属性高 8 位零重叠");
    cs.add("存储合法区", GA_STORE_LEGAL == mt::SPEC_MASK | sr::STD_ALL, "存储面恒无 GENERIC 位（打开时映射）");
    cs.add("属性闭集", ATTR_LEGAL == HA_INHERIT | HA_AUDIT_CLOSE | HA_PROTECT | HA_PRESET, "四属性闭集（继承/关审计/保护/预设）");
    cs.add("容量锚", TABLE_CAP == 256, "定容 256 槽（J1 句柄面容量闭集）");
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
    fn test_field_guard_negatives() {
        // 静态断言已编译期成立；运行期核守卫反例三态。
        assert_eq!(field_guard(0x0040_0000).err(), Some(E_FIELD));
        assert_eq!(field_guard(1 << 30).err(), Some(E_ATTR));
        assert!(field_guard(HA_PROTECT | sr::STD_ALL).is_ok());
    }

    #[test]
    fn test_attach_readback_and_stale() {
        let mut t = HandleTable::new();
        let (s, g) = t.attach(7, sr::STD_ALL, HA_INHERIT).unwrap_or((0, 0));
        let e = t.read(s, g).unwrap_or(Entry { obj: 0, ga: 0, attrs: 0, gen: 0, auth_epoch: 0 });
        assert_eq!(e.ga, sr::STD_ALL);
        assert_eq!(e.attrs, HA_INHERIT);
        t.release(s, g).unwrap_or(());
        assert_eq!(t.read(s, g).err(), Some(E_STALE));
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36221_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 6);
    }
}
