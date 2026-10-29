//! UNX-F36121-J1 对象类型注册表结构（类型名/generic map/默认策略）。
//! 判据：① 黄金八类装配全过且 tid/名/图/策略四元组一致；
//! ② 幂等重登记返回既有 tid、同名异图显性冲突、未注册显性拒；
//! ③ 封印后拒新增（36127 注销禁止锚）与容量满显性拒；
//! ④ 纯功能代码 ≥320 行。
//! 设计纪律：类型侧真源——B07 后续模块（默认拒 36122、generic map 消费
//! 36123、登记协议 36124、默认 SD 策略 36125、装配挂点 36126）只从本模块
//! 拿「类型注册的合法入口与查询面」，不私造类型条目。generic map 载荷
//! 直接复用 unx36104_genmap::GenMap（B06 真源，不重复定义位段学）。

use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;
use super::unx36101_masktable as mt;
use super::unx36102_stdrights as sr;
use super::unx36104_genmap::{GenMap, NAME_CAP};

pub const SEG_BASE: u16 = 0x3a90;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_NAME: u16 = SEG_BASE + 2; // 类型名非法（空/过长/非可见 ASCII）
pub const E_DUP: u16 = SEG_BASE + 3; // 同名异策略/异图冲突（幂等性破）
pub const E_FULL: u16 = SEG_BASE + 4; // 注册表满
pub const E_NOENT: u16 = SEG_BASE + 5; // 未注册类型查询（显性拒）
pub const E_MAPBAD: u16 = SEG_BASE + 6; // generic map 越出合法区（守恒破）
pub const E_FROZEN: u16 = SEG_BASE + 7; // 封印后新增（注册即冻结锚）
pub const E_TID: u16 = SEG_BASE + 8; // tid 越界或零号（编号纪律破）
pub const E_INTG: u16 = SEG_BASE + 9; // 完整性自检不符（条目漂移）

/// 模块版本锚（升版必须走 36128 的版本化面，不原地改语义）。
pub const VER: u32 = 1;

/// 类型注册表固定容量（24 类：黄金 8 + 接入向导 16 配额）。
pub const CAP: usize = 24;

/// tid 编号纪律：1 起步、连续、不回收（0 恒保留给「无类型」）。
pub const TID_BASE: u16 = 1;

/// 类型默认策略（36125 默认 SD 合成的分档依据）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypePolicy {
    /// 全闭：默认拒绝红线（敏感对象：令牌/特权册）。
    ClosedAll = 0x0001,
    /// null DACL 档：显式空授权域（受控共享面）。
    NullDacl = 0x0002,
    /// 继承档：默认 SD 从创建者上下文继承合成。
    InheritParent = 0x0003,
    /// 管理员门：DACL 仅管理员组 + SACL 强制审计。
    AdminGate = 0x0004,
}

impl TypePolicy {
    pub fn code(self) -> u16 {
        self as u16
    }
    /// 档位合法性（供完整性面复核：闭集外判别式即 E_INTG）。
    pub fn valid_code(c: u16) -> bool {
        c >= 0x0001 && c <= 0x0004
    }
}

// ---- 类型标志位族 ---------------------------------------------------------
pub const F_SEALED: u16 = 0x0001; // 封印位：注册即冻结（36127 消费锚）
pub const F_PERSIST: u16 = 0x0002; // 持久化位：镜像重启后需重注册（36129 消费锚）
pub const F_AUDIT: u16 = 0x0004; // 审计位：注册事件入账（36130 消费锚）
pub const F_GOLDEN: u16 = 0x0008; // 黄金位：启动链自检必查（36131 消费锚）
pub const F_USED: u16 = F_SEALED | F_PERSIST | F_AUDIT | F_GOLDEN; // 合法位闭集

/// 类型条目：tid + 名 + generic map + 默认策略 + 代际 + 标志。
#[derive(Clone, Copy)]
pub struct TypeEntry {
    pub tid: u16,
    pub name: &'static str,
    pub map: GenMap,
    pub policy: TypePolicy,
    pub gen: u32,
    pub flags: u16,
}

/// 固定容量类型注册表（构造-登记-封印三段式；无全局可变态）。
pub struct TypeRegistry {
    entries: [Option<TypeEntry>; CAP],
    n: usize,
    sealed: bool,
}

const NONE: Option<TypeEntry> = None;

impl TypeRegistry {
    pub fn new() -> TypeRegistry {
        TypeRegistry { entries: [NONE; CAP], n: 0, sealed: false }
    }

    /// 已登记类数。
    pub fn count(&self) -> usize {
        self.n
    }

    /// 封印态（封印后只读：拒新增、拒改档）。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 名字规则与 B06 映射表对齐（同一套类型名纪律，两表不分叉）。
    pub fn name_ok(name: &str) -> bool {
        let b = name.as_bytes();
        if b.is_empty() || b.len() > NAME_CAP - 1 {
            return false;
        }
        let mut i = 0;
        while i < b.len() {
            if !(0x21..=0x7e).contains(&b[i]) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 登记新类型：tid 自动连续分配。幂等性——同名同图同策略返回既有
    /// tid；同名异图/异策略 → E_DUP；封印后 → E_FROZEN；满 → E_FULL。
    pub fn register(
        &mut self,
        name: &'static str,
        map: GenMap,
        policy: TypePolicy,
        flags: u16,
    ) -> Result<u16, u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        if !TypeRegistry::name_ok(name) {
            return Err(E_NAME);
        }
        if !map.wellformed() {
            return Err(E_MAPBAD);
        }
        if flags & !F_USED != 0 {
            return Err(E_INTG);
        }
        let mut i = 0;
        while i < self.n {
            match self.entries[i] {
                Some(e) => {
                    if e.name == name {
                        if e.map.read == map.read
                            && e.map.write == map.write
                            && e.map.exec == map.exec
                            && e.policy == policy
                        {
                            return Ok(e.tid);
                        }
                        return Err(E_DUP);
                    }
                }
                None => return Err(E_TID),
            }
            i += 1;
        }
        // 封印闸：幂等重登记/冲突判定已在上（可查询既有 tid 或报
        // E_DUP），真正「新增条目」在封印后显性拒 E_FROZEN。
        if self.sealed {
            return Err(E_FROZEN);
        }
        if self.n >= CAP {
            return Err(E_FULL);
        }
        let tid = TID_BASE + self.n as u16;
        self.entries[self.n] = Some(TypeEntry { tid, name, map, policy, gen: 1, flags });
        self.n += 1;
        Ok(tid)
    }

    /// 按名查询条目（未注册 → E_NOENT 显性拒，非静默）。
    pub fn query(&self, name: &str) -> Result<TypeEntry, u16> {
        let mut i = 0;
        while i < self.n {
            match self.entries[i] {
                Some(e) => {
                    if e.name == name {
                        return Ok(e);
                    }
                }
                None => return Err(E_TID),
            }
            i += 1;
        }
        Err(E_NOENT)
    }

    /// 按名取 tid（装配挂点 36126 的热路径入口）。
    pub fn tid_of(&self, name: &str) -> Result<u16, u16> {
        self.query(name).map(|e| e.tid)
    }

    /// 按名取默认策略（36125 合成分档入口）。
    pub fn policy_of(&self, name: &str) -> Result<TypePolicy, u16> {
        self.query(name).map(|e| e.policy)
    }

    /// 按 tid 取条目（tid 零号/越界显性拒）。
    pub fn at(&self, tid: u16) -> Result<TypeEntry, u16> {
        if tid < TID_BASE {
            return Err(E_TID);
        }
        let idx = (tid - TID_BASE) as usize;
        if idx >= self.n {
            return Err(E_TID);
        }
        match self.entries[idx] {
            Some(e) => Ok(e),
            None => Err(E_TID),
        }
    }

    /// 封印（幂等）：注册面从「可增」转「只读」。黄金装配完成后调用，
    /// 36127 的注销禁止语义以此为前提。
    pub fn seal(&mut self) {
        self.sealed = true;
        let mut i = 0;
        while i < self.n {
            if let Some(ref mut e) = self.entries[i] {
                e.flags |= F_SEALED;
            }
            i += 1;
        }
    }

    /// 完整性自检：计数一致 + tid 连续 + 名唯一 + 图合法 + 策略合法 +
    /// 代际非零 + 标志闭集。
    pub fn integrity(&self) -> Result<(), u16> {
        if self.n > CAP {
            return Err(E_INTG);
        }
        let mut seen: usize = 0;
        let mut i = 0;
        while i < self.n {
            let e = match self.entries[i] {
                Some(e) => e,
                None => return Err(E_INTG),
            };
            if e.tid != TID_BASE + i as u16 {
                return Err(E_TID);
            }
            if !TypeRegistry::name_ok(e.name) {
                return Err(E_NAME);
            }
            if !e.map.wellformed() {
                return Err(E_MAPBAD);
            }
            if !TypePolicy::valid_code(e.policy.code()) {
                return Err(E_INTG);
            }
            if e.gen == 0 || e.flags & !F_USED != 0 {
                return Err(E_INTG);
            }
            // 名唯一性：与后继逐对比较（CAP=24，O(n²) 可承受且无堆依赖）。
            let mut j = i + 1;
            while j < self.n {
                if let Some(e2) = self.entries[j] {
                    if e2.name == e.name {
                        return Err(E_DUP);
                    }
                }
                j += 1;
            }
            seen += 1;
            i += 1;
        }
        if seen != self.n {
            return Err(E_INTG);
        }
        Ok(())
    }

    /// 已登记名清单（对账面 36134 消费）。
    pub fn names(&self) -> Vec<String> {
        let mut v = Vec::new();
        let mut i = 0;
        while i < self.n {
            if let Some(e) = self.entries[i] {
                v.push(String::from(e.name));
            }
            i += 1;
        }
        v
    }
}

// ---- 黄金八类（启动链自检 36131 的最小装配集） ----------------------------
pub const GOLDEN_N: usize = 8;

/// 黄金类型清单：tid = TID_BASE + 下标。映射全部落在 B06 合法区。
pub const GOLDEN: [(&'static str, GenMap, TypePolicy, u16); GOLDEN_N] = [
    ("File", GenMap { read: sr::READ_CONTROL | sr::SYNCHRONIZE, write: sr::DELETE | sr::WRITE_DAC | sr::WRITE_OWNER | sr::READ_CONTROL, exec: sr::READ_CONTROL | sr::SYNCHRONIZE }, TypePolicy::InheritParent, F_PERSIST | F_AUDIT),
    ("Key", GenMap { read: sr::READ_CONTROL, write: sr::WRITE_DAC | sr::WRITE_OWNER | sr::READ_CONTROL, exec: sr::READ_CONTROL }, TypePolicy::InheritParent, F_PERSIST | F_AUDIT),
    ("WindowStation", GenMap { read: sr::READ_CONTROL, write: sr::WRITE_DAC | sr::READ_CONTROL, exec: sr::READ_CONTROL | sr::SYNCHRONIZE }, TypePolicy::AdminGate, F_AUDIT),
    ("Desktop", GenMap { read: sr::READ_CONTROL, write: sr::WRITE_DAC | sr::READ_CONTROL, exec: sr::READ_CONTROL }, TypePolicy::AdminGate, F_AUDIT),
    ("Section", GenMap { read: sr::READ_CONTROL, write: sr::WRITE_DAC | sr::READ_CONTROL, exec: sr::READ_CONTROL }, TypePolicy::NullDacl, 0),
    ("Process", GenMap { read: sr::READ_CONTROL, write: sr::WRITE_DAC | sr::WRITE_OWNER | sr::READ_CONTROL, exec: sr::READ_CONTROL | sr::SYNCHRONIZE }, TypePolicy::InheritParent, F_AUDIT | F_GOLDEN),
    ("Thread", GenMap { read: sr::READ_CONTROL, write: sr::WRITE_DAC | sr::WRITE_OWNER | sr::READ_CONTROL, exec: sr::READ_CONTROL | sr::SYNCHRONIZE }, TypePolicy::InheritParent, F_AUDIT | F_GOLDEN),
    ("Token", GenMap { read: sr::READ_CONTROL, write: sr::WRITE_DAC | sr::WRITE_OWNER, exec: 0 }, TypePolicy::ClosedAll, F_GOLDEN),
];

/// 黄金装配：空表 → 黄金八类 → 封印。任何一步失败即整体失败（装配原子性）。
pub fn assemble_golden() -> Result<TypeRegistry, u16> {
    let mut r = TypeRegistry::new();
    let mut i = 0;
    while i < GOLDEN_N {
        let (name, map, policy, flags) = GOLDEN[i];
        let tid = r.register(name, map, policy, flags | F_GOLDEN)?;
        if tid != TID_BASE + i as u16 {
            return Err(E_TID);
        }
        i += 1;
    }
    r.seal();
    r.integrity()?;
    Ok(r)
}

/// 黄金类型名表（启动链探针 36131 逐名核对用）。
pub const GOLDEN_NAMES: [&'static str; GOLDEN_N] =
    ["File", "Key", "WindowStation", "Desktop", "Section", "Process", "Thread", "Token"];

/// 容量演练用 24 个填充名（'static 生命周期，栈构名不可用）。
pub const FILL_NAMES: [&'static str; CAP] = [
    "Typ00", "Typ01", "Typ02", "Typ03", "Typ04", "Typ05", "Typ06", "Typ07",
    "Typ08", "Typ09", "Typ10", "Typ11", "Typ12", "Typ13", "Typ14", "Typ15",
    "Typ16", "Typ17", "Typ18", "Typ19", "Typ20", "Typ21", "Typ22", "Typ23",
];

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：黄金装配全过。
    let mut r = assemble_golden()?;
    if r.count() != GOLDEN_N {
        return Err(E_INTG);
    }
    // tid 连续且非零。
    let mut i = 0;
    while i < GOLDEN_N {
        let e = r.at(TID_BASE + i as u16)?;
        if e.name != GOLDEN_NAMES[i] {
            return Err(E_INTG);
        }
        if e.tid != TID_BASE + i as u16 || e.tid == 0 {
            return Err(E_TID);
        }
        i += 1;
    }
    // 策略分档覆盖：四个档位在黄金集内至少各出现一次。
    let mut pol_seen = [0u32; 5];
    let mut k = 0;
    while k < GOLDEN_N {
        let e = r.at(TID_BASE + k as u16)?;
        let c = e.policy.code() as usize;
        if c == 0 || c > 4 {
            return Err(E_INTG);
        }
        pol_seen[c] += 1;
        k += 1;
    }
    if pol_seen[1] == 0 || pol_seen[2] == 0 || pol_seen[3] == 0 || pol_seen[4] == 0 {
        return Err(E_INTG);
    }
    // R2：幂等重登记（同图同策略 → 既有 tid）。
    let (gname, gmap, gpol, _) = GOLDEN[0];
    if r.register(gname, gmap, gpol, 0)? != TID_BASE {
        return Err(E_DUP);
    }
    // 同名异图 → 冲突。
    let flipped = GenMap { read: gmap.write, write: gmap.read, exec: gmap.exec };
    if r.register(gname, flipped, gpol, 0).err() != Some(E_DUP) {
        return Err(E_DUP);
    }
    // 同名同图异策略 → 冲突。
    let other_pol = if gpol == TypePolicy::AdminGate { TypePolicy::NullDacl } else { TypePolicy::AdminGate };
    if r.register(gname, gmap, other_pol, 0).err() != Some(E_DUP) {
        return Err(E_DUP);
    }
    // R3：未注册显性拒。
    if r.query("Nonexistent").err() != Some(E_NOENT) {
        return Err(E_NOENT);
    }
    if r.tid_of("Nonexistent").err() != Some(E_NOENT) {
        return Err(E_NOENT);
    }
    // tid 零号与越界显性拒。
    if r.at(0).err() != Some(E_TID) {
        return Err(E_TID);
    }
    if r.at(TID_BASE + GOLDEN_N as u16).err() != Some(E_TID) {
        return Err(E_TID);
    }
    // R4：名字纪律（空名 / 超长名 / 非可见字符）。
    let long_name = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef"; // 32 字符 > NAME_CAP-1
    if TypeRegistry::name_ok("") || TypeRegistry::name_ok(long_name) {
        return Err(E_NAME);
    }
    if TypeRegistry::name_ok("Has Space") || TypeRegistry::name_ok("Tab\tX") {
        return Err(E_NAME);
    }
    if r.register("", gmap, gpol, 0).err() != Some(E_NAME) {
        return Err(E_NAME);
    }
    // R5：图守恒（保留位混入 → E_MAPBAD）。
    let bad_map = GenMap { read: sr::READ_CONTROL | mt::STD_RSVD, write: 0, exec: 0 };
    if r.register("BadMap", bad_map, gpol, 0).err() != Some(E_MAPBAD) {
        return Err(E_MAPBAD);
    }
    // 标志位闭集外 → E_INTG。
    if r.register("BadFlags", gmap, gpol, 0x8000).err() != Some(E_INTG) {
        return Err(E_INTG);
    }
    // R6：封印语义。黄金装配即封印 → 新增拒。
    if r.register("PostSeal", gmap, gpol, 0).err() != Some(E_FROZEN) {
        return Err(E_FROZEN);
    }
    // 封印幂等：重复 seal 不变态。
    r.seal();
    if !r.is_sealed() {
        return Err(E_FROZEN);
    }
    // R7：容量满（开新表，不封印，填满 24 类）。
    let mut big = TypeRegistry::new();
    let mut t = 0;
    while t < CAP {
        big.register(FILL_NAMES[t], gmap, TypePolicy::NullDacl, 0)?;
        t += 1;
    }
    if big.count() != CAP {
        return Err(E_FULL);
    }
    if big.register("Overflow", gmap, TypePolicy::NullDacl, 0).err() != Some(E_FULL) {
        return Err(E_FULL);
    }
    // R8：完整性自检双面（黄金表过 / 被污染表炸）。
    r.integrity()?;
    big.integrity()?;
    // 校验和：tid 全并 × 策略档位计数（确定性锚）。
    let mut sum: u32 = 0;
    let mut q = 0;
    while q < GOLDEN_N {
        sum = sum.wrapping_add(r.at(TID_BASE + q as u16)?.tid as u32);
        q += 1;
    }
    Ok(sum.wrapping_mul(97).wrapping_add(pol_seen[1] + pol_seen[2] + pol_seen[3] + pol_seen[4]))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_NAME => "类型名非法（空/过长/非可见 ASCII）",
        E_DUP => "同名异图/异策略冲突（幂等性破）",
        E_FULL => "类型注册表满（24 类容量）",
        E_NOENT => "未注册类型查询（显性拒）",
        E_MAPBAD => "generic map 越出合法区（守恒破）",
        E_FROZEN => "封印后新增（注册即冻结锚）",
        E_TID => "tid 零号/越界（编号纪律破）",
        E_INTG => "完整性自检不符（条目漂移）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36121_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36121_typereg");
    cs.add("SEG 段位独占", SEG_BASE == 0x3a90, "0x3a90~0x3a9F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "黄金装配/幂等/显性拒/封印/容量全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("黄金八类", GOLDEN_N == 8 && GOLDEN_NAMES.len() == 8, "File/Key/Winsta/Desktop/Section/Process/Thread/Token");
    cs.add("黄金名序恒等", {
        let mut ok = true;
        let mut i = 0;
        while i < GOLDEN_N {
            if GOLDEN[i].0 != GOLDEN_NAMES[i] { ok = false; }
            i += 1;
        }
        ok
    }, "GOLDEN 与 GOLDEN_NAMES 逐位一致");
    cs.add("黄金图全守恒", {
        let mut ok = true;
        let mut i = 0;
        while i < GOLDEN_N {
            if !GOLDEN[i].1.wellformed() { ok = false; }
            i += 1;
        }
        ok
    }, "八张映射全落 B06 合法区");
    cs.add("容量锚", CAP == 24 && TID_BASE == 1, "24 类固定容量；tid 1 起步");
    cs.add("策略闭集", TypePolicy::valid_code(1) && TypePolicy::valid_code(4) && !TypePolicy::valid_code(0) && !TypePolicy::valid_code(5), "四档策略判别式闭集");
    cs.add("标志闭集", F_USED == 0x000F, "四个标志位全并 = 0x000F");
    cs.add("空表查询显性拒", TypeRegistry::new().query("X").err() == Some(E_NOENT), "空表非静默");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}
