//! unx4087_dbind — UNX-F4087 dentry 与 inode 绑定/解绑（诊断段 0x8D00）。
//!
//! 判据原文：UNX-F4087-J1 绑定/解绑幂等（双次操作零二次效果），解绑后查找返回
//!           未命中且不悬空访问（空指针注入零崩）。
//! 可验性质（本模块自证面）：
//!   J1 幂等：对已 Bound 的同一 (dentry, inode) 再 bind 一次，除 noop 账 +1 外
//!      不产生任何二次效果（gen 不变、binds 不变、绑定表不变）；对已 Unbound 的
//!      dentry 再 unbind 一次同理；
//!   J2 解绑后查找必须未命中（resolve 返回 Ok(None)），且 dentry 仍可在表内被
//!      查到（态 Unbound，gen 已换代）——不悬空、不误命中陈旧 inode；
//!   J3 换代语义：解绑使 gen +1，用旧 gen 解析必须失败（E_STALE_GEN）——这就是
//!      "不悬空访问"的可验面（防 ABA：旧引用拿到新绑定）；
//!   J4 冲突拒收：已绑定 A 再绑 B → E_D_CONFLICT，绑定不变、冲突账 +1；
//!      对不存在的 dentry 做任何操作 → E_NO_SLOT；
//!   J5 空指针注入零崩：解析接口一律返回 Option/Result，inode 号 0 视为空指针，
//!      绑定 0 → E_NULL_INO，解析到 0 不返回 Some(0)。
//! 设计纪律：
//!   1. 绑定关系只存 inode 号（u64），不存"inode 对象"：本模块在纯功能线里不持有
//!      跨模块引用，避免任何解引用面（没有指针就没有悬空指针）。
//!   2. dentry 与 inode 的关联是单向的：dentry → inode。反向计数由 F4086 的 d_count
//!      与 F4076 的伪 inode 在世账各自负责，本模块不重复记账。
//!   3. gen 是"绑定代次"，只在解绑时自增：这样旧代次的解析请求可以被识别并按
//!      E_STALE_GEN 拒绝，等价于 VFS 里 dentry 被 dcache 摘除后的 d_seq 换代。
//!   4. 所有写路径先判态再动表，态判定与表修改之间不插入任何可能失败的步骤
//!      （失败一律前置），保证"要么全改要么不改"。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!
//! 诊断段 0x8D00 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8D00;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NO_SLOT: u16 = SEG_BASE + 1; // dentry 槽不存在（含 0 号）
const E_D_CONFLICT: u16 = SEG_BASE + 2; // 已绑定其它 inode，冲突
const E_NULL_INO: u16 = SEG_BASE + 3; // 绑定/解析 0 号 inode（空指针）
const E_STALE_GEN: u16 = SEG_BASE + 4; // 用旧代次解析（悬空引用）
const E_DUP_SLOT: u16 = SEG_BASE + 5; // dentry 槽重复建立
const E_BAL: u16 = SEG_BASE + 6; // 绑定账不平
const E_UNBOUND_YET: u16 = SEG_BASE + 7; // 从未绑定即要求解绑（非幂等场景）
const E_SELF: u16 = SEG_BASE + 8; // 自绑（dentry 号与 inode 号同一值是配置错）

/// 绑定态：未绑 / 已绑 / 陈旧（解绑后保留的代次墓碑）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BindState {
    Unbound = 0, // 未绑定（可直接绑）
    Bound = 1,   // 已绑定
    Stale = 2,   // 解绑后墓碑（保留代次，供旧引用识别）
}

/// 绑定裁决（drill 用于区分四种写结果）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BindVerdict {
    Bound = 0,     // 本次真绑上
    Noop = 1,      // 幂等重放，无二次效果
    Unbound = 2,   // 本次真解绑
    NoopUnbind = 3, // 幂等重解绑
}

/// 单槽绑定记录。
pub struct BindSlot {
    pub dentry: u64,      // dentry 标识（主键）
    pub inode: Option<u64>, // 绑定的 inode 号（None = 未绑）
    pub gen: u32,         // 绑定代次（解绑自增）
    pub state: BindState, // 态
    pub binds: u32,       // 真绑定次数（幂等重放不计）
    pub unbinds: u32,     // 真解绑次数（幂等重放不计）
}

impl BindSlot {
    /// 当前是否绑着某个 inode。
    pub fn is_bound(&self) -> bool {
        self.state == BindState::Bound && self.inode.is_some()
    }
}

/// 绑定表：槽表 + 幂等/冲突账。
pub struct BindTable {
    pub slots: Vec<BindSlot>, // 槽表
    pub noops: u32,           // 幂等重放次数（bind + unbind 合计）
    pub conflicts: u32,       // 绑定冲突拒收
    pub resolves: u32,        // 解析次数
    pub stale_rejects: u32,   // 旧代次拒收
    pub null_rejects: u32,    // 空指针拒收
}

impl BindTable {
    pub fn new() -> BindTable {
        BindTable {
            slots: Vec::new(),
            noops: 0,
            conflicts: 0,
            resolves: 0,
            stale_rejects: 0,
            null_rejects: 0,
        }
    }

    /// 建槽：dentry 号唯一且非 0。
    pub fn add_slot(&mut self, dentry: u64) -> Result<(), u16> {
        if dentry == 0 {
            return Err(E_NO_SLOT);
        }
        if self.find(dentry).is_some() {
            return Err(E_DUP_SLOT);
        }
        self.slots.push(BindSlot {
            dentry,
            inode: None,
            gen: 0,
            state: BindState::Unbound,
            binds: 0,
            unbinds: 0,
        });
        Ok(())
    }

    /// 定位槽下标。
    pub fn find(&self, dentry: u64) -> Option<usize> {
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].dentry == dentry {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 绑定：幂等 + 冲突 + 空指针三路判定（J1/J4/J5）。
    pub fn bind(&mut self, dentry: u64, inode: u64) -> Result<BindVerdict, u16> {
        if inode == 0 {
            self.null_rejects += 1;
            return Err(E_NULL_INO);
        }
        if dentry == inode {
            return Err(E_SELF);
        }
        let idx = match self.find(dentry) {
            Some(i) => i,
            None => return Err(E_NO_SLOT),
        };
        match self.slots[idx].state {
            BindState::Bound => {
                match self.slots[idx].inode {
                    Some(cur) if cur == inode => {
                        // 幂等重放：除 noop 账外零二次效果（gen/binds 都不动）
                        self.noops += 1;
                        Ok(BindVerdict::Noop)
                    }
                    _ => {
                        self.conflicts += 1;
                        Err(E_D_CONFLICT)
                    }
                }
            }
            BindState::Unbound | BindState::Stale => {
                self.slots[idx].inode = Some(inode);
                self.slots[idx].state = BindState::Bound;
                self.slots[idx].binds += 1;
                Ok(BindVerdict::Bound)
            }
        }
    }

    /// 解绑：幂等（已 Unbound 再解绑只 +noop）；解绑后态 Stale 且 gen 换代（J2/J3）。
    pub fn unbind(&mut self, dentry: u64) -> Result<BindVerdict, u16> {
        let idx = match self.find(dentry) {
            Some(i) => i,
            None => return Err(E_NO_SLOT),
        };
        match self.slots[idx].state {
            BindState::Unbound => {
                self.noops += 1;
                Ok(BindVerdict::NoopUnbind)
            }
            BindState::Stale => {
                self.noops += 1;
                Ok(BindVerdict::NoopUnbind) // 已是墓碑，再解绑同样幂等
            }
            BindState::Bound => {
                self.slots[idx].inode = None;
                self.slots[idx].state = BindState::Stale;
                self.slots[idx].gen += 1; // 换代：旧引用从此不可解析
                self.slots[idx].unbinds += 1;
                Ok(BindVerdict::Unbound)
            }
        }
    }

    /// 解析：带代次校验（gen 不符即 E_STALE_GEN，杜绝悬空）。
    pub fn resolve(&mut self, dentry: u64, gen: u32) -> Result<Option<u64>, u16> {
        let idx = match self.find(dentry) {
            Some(i) => i,
            None => return Err(E_NO_SLOT),
        };
        self.resolves += 1;
        if gen != self.slots[idx].gen {
            self.stale_rejects += 1;
            return Err(E_STALE_GEN);
        }
        match self.slots[idx].state {
            BindState::Bound => match self.slots[idx].inode {
                Some(v) if v != 0 => Ok(Some(v)),
                _ => Err(E_NULL_INO), // 绑着 0 号属账坏，按空指针拒
            },
            _ => Ok(None), // 未绑/墓碑：未命中，不返回 inode
        }
    }

    /// 无代次解析（宽松口，供 d_hash 查完直接取用）：仍不得返回 0。
    pub fn resolve_latest(&mut self, dentry: u64) -> Result<Option<u64>, u16> {
        let idx = match self.find(dentry) {
            Some(i) => i,
            None => return Err(E_NO_SLOT),
        };
        let gen = self.slots[idx].gen;
        self.resolve(dentry, gen)
    }

    /// 当前代次读数。
    pub fn gen_of(&self, dentry: u64) -> Result<u32, u16> {
        match self.find(dentry) {
            Some(i) => Ok(self.slots[i].gen),
            None => Err(E_NO_SLOT),
        }
    }

    /// 账自审：binds - unbinds == 当前 Bound 槽数（真操作账）。
    pub fn balanced(&self) -> bool {
        let mut bound = 0u32;
        let mut binds = 0u32;
        let mut unbinds = 0u32;
        let mut i = 0usize;
        while i < self.slots.len() {
            binds += self.slots[i].binds;
            unbinds += self.slots[i].unbinds;
            if self.slots[i].state == BindState::Bound {
                bound += 1;
            }
            i += 1;
        }
        binds == unbinds + bound
    }

    /// 态自审：Bound ⇒ inode.is_some()，其余 ⇒ inode.is_none()。
    pub fn state_audit(&self) -> bool {
        let mut i = 0usize;
        while i < self.slots.len() {
            let s = &self.slots[i];
            match s.state {
                BindState::Bound => {
                    if s.inode.is_none() {
                        return false;
                    }
                }
                _ => {
                    if s.inode.is_some() {
                        return false;
                    }
                }
            }
            i += 1;
        }
        true
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NO_SLOT => "dentry 槽不存在（含 0 号）",
        E_D_CONFLICT => "已绑定其它 inode，冲突",
        E_NULL_INO => "绑定/解析 0 号 inode（空指针）",
        E_STALE_GEN => "用旧代次解析（悬空引用）",
        E_DUP_SLOT => "dentry 槽重复建立",
        E_BAL => "绑定账不平",
        E_UNBOUND_YET => "从未绑定即要求解绑",
        E_SELF => "自绑（dentry 号与 inode 号同值）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：绑定幂等 → 解绑换代 → 冲突/悬空拦截 → 账平。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：建 100 个槽，逐槽绑定唯一 inode，binds == 100，态全 Bound（J1 正面）。
    let mut t = BindTable::new();
    let mut i = 0u64;
    while i < 100 {
        t.add_slot(0x1000 + i)?;
        i += 1;
    }
    i = 0;
    while i < 100 {
        if t.bind(0x1000 + i, 0x8000 + i)? != BindVerdict::Bound {
            return Err(E_BAL);
        }
        i += 1;
    }
    if !t.balanced() || !t.state_audit() {
        return Err(E_BAL);
    }
    if t.noops != 0 || t.conflicts != 0 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：绑定幂等——同一 (dentry,inode) 重放 1,000 次，只有 noop 账增，无二次效果（J1）。
    let binds_before = t.slots[t.find(0x1005).ok_or(E_NO_SLOT)?].binds;
    let gen_before = t.gen_of(0x1005)?;
    let mut k = 0u32;
    while k < 1_000 {
        if t.bind(0x1005, 0x8005)? != BindVerdict::Noop {
            return Err(E_BAL);
        }
        k += 1;
    }
    if t.noops != 1_000 {
        return Err(E_BAL);
    }
    let idx5 = t.find(0x1005).ok_or(E_NO_SLOT)?;
    if t.slots[idx5].binds != binds_before || t.slots[idx5].gen != gen_before {
        return Err(E_BAL); // 幂等不得产生二次效果
    }
    if t.slots.len() != 100 {
        return Err(E_BAL); // 也不得偷偷建新槽
    }
    hits += 1;

    // R3：解析——当代次可解析到 inode；解绑后同代次未命中且换代，旧代次解析必拒（J2/J3）。
    let g0 = t.gen_of(0x1005)?;
    if t.resolve(0x1005, g0)? != Some(0x8005) {
        return Err(E_BAL);
    }
    if t.unbind(0x1005)? != BindVerdict::Unbound {
        return Err(E_BAL);
    }
    let g1 = t.gen_of(0x1005)?;
    if g1 != g0 + 1 {
        return Err(E_BAL); // 换代 +1
    }
    if t.resolve(0x1005, g1)? != None {
        return Err(E_BAL); // 解绑后查找未命中
    }
    if t.resolve(0x1005, g0) != Err(E_STALE_GEN) {
        return Err(E_STALE_GEN); // 旧代次必拒（不悬空）
    }
    if t.stale_rejects != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：解绑幂等——再解绑 1,000 次只增 noop，gen 不再变（J1 后半）。
    let gen_after = t.gen_of(0x1005)?;
    let mut m = 0u32;
    while m < 1_000 {
        if t.unbind(0x1005)? != BindVerdict::NoopUnbind {
            return Err(E_BAL);
        }
        m += 1;
    }
    if t.gen_of(0x1005)? != gen_after {
        return Err(E_BAL);
    }
    if t.noops != 2_000 {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：重绑——解绑后绑定新 inode 合法（Stale → Bound），binds 计数 +1。
    if t.bind(0x1005, 0x9005)? != BindVerdict::Bound {
        return Err(E_BAL);
    }
    let idx5b = t.find(0x1005).ok_or(E_NO_SLOT)?;
    if t.slots[idx5b].binds != binds_before + 1 {
        return Err(E_BAL);
    }
    let g2 = t.gen_of(0x1005)?;
    if t.resolve(0x1005, g2)? != Some(0x9005) {
        return Err(E_BAL);
    }
    hits += 1;

    // R6：批量换绑——100 个槽全部解绑再全部绑新 inode，账仍平（万次级负载的缩小版）。
    i = 0;
    while i < 100 {
        t.unbind(0x1000 + i)?;
        i += 1;
    }
    i = 0;
    while i < 100 {
        if t.bind(0x1000 + i, 0xA000 + i)? != BindVerdict::Bound {
            return Err(E_BAL);
        }
        i += 1;
    }
    if !t.balanced() || !t.state_audit() {
        return Err(E_BAL);
    }
    i = 0;
    while i < 100 {
        let gi = t.gen_of(0x1000 + i)?;
        if t.resolve(0x1000 + i, gi)? != Some(0xA000 + i) {
            return Err(E_BAL);
        }
        i += 1;
    }
    hits += 1;

    // I1：已绑 A 再绑 B → E_D_CONFLICT，绑定不变、冲突账 +1（J4）。
    if t.bind(0x1007, 0xDEAD) != Err(E_D_CONFLICT) {
        return Err(E_D_CONFLICT);
    }
    if t.conflicts != 1 {
        return Err(E_BAL);
    }
    let g7 = t.gen_of(0x1007)?;
    if t.resolve(0x1007, g7)? != Some(0xA007) {
        return Err(E_BAL); // 冲突不改绑定
    }
    hits += 1;

    // I2：空指针注入——绑 0 号 → E_NULL_INO，不得把 0 当合法 inode（J5）。
    if t.bind(0x1009, 0) != Err(E_NULL_INO) {
        return Err(E_NULL_INO);
    }
    if t.null_rejects != 1 {
        return Err(E_BAL);
    }
    if t.resolve(0x1009, t.gen_of(0x1009)?)? == Some(0) {
        return Err(E_NULL_INO); // 解析口永不返回 Some(0)
    }
    hits += 1;

    // I3：不存在的槽三路拒（bind/unbind/resolve）→ E_NO_SLOT，不建隐式槽。
    let n_before = t.slots.len();
    if t.bind(0xBEEF, 1) != Err(E_NO_SLOT) {
        return Err(E_NO_SLOT);
    }
    if t.unbind(0xBEEF) != Err(E_NO_SLOT) {
        return Err(E_NO_SLOT);
    }
    if t.resolve(0xBEEF, 0) != Err(E_NO_SLOT) {
        return Err(E_NO_SLOT);
    }
    if t.slots.len() != n_before {
        return Err(E_BAL);
    }
    hits += 1;

    // I4：重复建槽 → E_DUP_SLOT；0 号槽 → E_NO_SLOT；自绑 → E_SELF。
    if t.add_slot(0x1000) != Err(E_DUP_SLOT) {
        return Err(E_DUP_SLOT);
    }
    if t.add_slot(0) != Err(E_NO_SLOT) {
        return Err(E_NO_SLOT);
    }
    if t.bind(0x1001, 0x1001) != Err(E_SELF) {
        return Err(E_SELF);
    }
    hits += 1;

    // I5：宽松解析口在墓碑态也必须未命中（防"解绑后仍能拿到旧 inode"）。
    t.unbind(0x1002)?;
    if t.resolve_latest(0x1002)? != None {
        return Err(E_BAL);
    }
    hits += 1;

    // 终审：账平 + 态自审 + 每槽代次单调（gen 从未回退）。
    if !t.balanced() || !t.state_audit() {
        return Err(E_BAL);
    }
    let mut gsum = 0u32;
    i = 0;
    while i < 100 {
        gsum += t.gen_of(0x1000 + i)?;
        i += 1;
    }
    if gsum < 100 {
        return Err(E_BAL); // 100 个槽至少各换过一代
    }
    hits += 1;

    Ok(hits)
}
