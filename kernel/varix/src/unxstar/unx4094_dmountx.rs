//! unx4094_dmountx — UNX-F4094 dentry 与 mount 交叉（跨挂载点 walk 边界）（诊断段 0x8DE0）。
//!
//! 判据原文：UNX-F4094-J1 挂载点 dentry 双面（被遮蔽面/挂入面）可区分（标记位断言），
//!           walk 到挂载点的穿越接口为 B12 供给（接口存在性断言通过）。
//! 可验性质（本模块自证面）：
//!   J1 一次挂载产生两个面：被遮蔽面（挂载点原 dentry，标 MNT_COVERED）与挂入面
//!      （新 fs 根 ino，标 MNT_ROOT），两面可分别查询且互不混淆；
//!   J2 穿越接口存在且行为确定：cross(被遮蔽 dentry) 返回挂入面根 ino；
//!      对被遮蔽面之外的 dentry 一律 E_NO_CROSS（不误穿越）；
//!   J3 嵌套挂载：某 dentry 既是上一层挂入面、又是下一层被遮蔽面时，face 必须是
//!      Both（两面并存），且穿越链可续（cross 给出下一层根）；
//!   J4 账守恒：mounts 记录数 == 被遮蔽面数；挂入面集合大小 == 被遮蔽面数
//!      （一挂载一遮蔽一根）；卸载后两账同步减 1；
//!   J5 非法挂载拒：同一 dentry 重复挂载（E_MNT_DUP）、自挂（E_SELF_MNT，
//!      挂载点与根同号）、0 号 ino（E_INO_ZERO）、卸载未挂载点（E_NO_MNT）。
//! 设计纪律：
//!   1. 双面用标记位表达，不建两套表：一条 MountPoint 记录同时持有 dentry（被遮蔽面）
//!      与 mount_root_ino（挂入面），查询面时按两条索引各查一次。
//!   2. 挂入面集合单独维护（mounted_roots: Vec<u64>），因为"某 ino 是不是挂入面根"
//!      与"某 dentry 是不是挂载点"是两个独立判定，混用会漏判嵌套。
//!   3. 穿越是 walk 的边界动作：本模块只提供 cross() 这个原子接口，不实现 walk 循环
//!      （那是 F4090 的职责），边界清晰才能各自被单独回归。
//!   4. 卸载只摘记录、不动 ino 分配：挂载记录是"当前挂载态"的快照，历史不保留。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!
//! 诊断段 0x8DE0 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8DE0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_MNT_DUP: u16 = SEG_BASE + 1; // 同一 dentry 重复挂载
const E_SELF_MNT: u16 = SEG_BASE + 2; // 自挂（挂载点与根同号）
const E_INO_ZERO: u16 = SEG_BASE + 3; // 0 号 ino 保留
const E_NO_MNT: u16 = SEG_BASE + 4; // 卸载未挂载点
const E_NO_CROSS: u16 = SEG_BASE + 5; // 该 dentry 不是挂载点，无穿越
const E_FACE_OOR: u16 = SEG_BASE + 6; // 面码越界
const E_BAL: u16 = SEG_BASE + 7; // 挂载账不平
const E_ROOT_DUP: u16 = SEG_BASE + 8; // 同一 ino 被两次挂为根（多挂载冲突）

/// 被遮蔽面标记：该 dentry 上原本的对象被新 fs 遮蔽。
pub const MNT_COVERED: u32 = 1;

/// 挂入面标记：该 ino 是某个新 fs 的根。
pub const MNT_ROOT: u32 = 2;

/// 面枚举：四个取值覆盖两面及其叠加。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MountFace {
    None = 0,      // 普通 dentry（不参与挂载）
    Covered = 1,   // 仅被遮蔽面
    MountRoot = 2, // 仅挂入面
    Both = 3,      // 两面叠加（嵌套挂载的中间层）
}

/// 面码 → 闭集（drill 与上层共用）。
pub fn face_by_code(code: u16) -> Result<MountFace, u16> {
    match code {
        0 => Ok(MountFace::None),
        1 => Ok(MountFace::Covered),
        2 => Ok(MountFace::MountRoot),
        3 => Ok(MountFace::Both),
        _ => Err(E_FACE_OOR),
    }
}

/// 单条挂载记录：一条记录 = 一面遮蔽 + 一根挂入。
pub struct MountPoint {
    pub dentry: u64,        // 被遮蔽面 dentry（挂载点）
    pub mount_root_ino: u64, // 挂入面根 ino
    pub flags: u32,         // 标记位（MNT_COVERED | MNT_ROOT）
    pub seq: u32,           // 挂载序号（挂载顺序，卸载按记录摘）
}

/// 挂载表：记录 + 挂入面集合 + 账。
pub struct MountTable {
    pub mounts: Vec<MountPoint>, // 挂载记录
    pub roots: Vec<u64>,         // 挂入面根集合（去重）
    pub lookups: u32,            // 面查询次数
    pub crossings: u32,          // 穿越次数
    pub dup_rejects: u32,        // 重复挂载拒收
    pub umounts: u32,            // 卸载次数
}

impl MountTable {
    pub fn new() -> MountTable {
        MountTable {
            mounts: Vec::new(),
            roots: Vec::new(),
            lookups: 0,
            crossings: 0,
            dup_rejects: 0,
            umounts: 0,
        }
    }

    /// 记录定位（按被遮蔽 dentry）。
    pub fn find_mount(&self, dentry: u64) -> Option<usize> {
        let mut i = 0usize;
        while i < self.mounts.len() {
            if self.mounts[i].dentry == dentry {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 根集合定位。
    pub fn is_root(&self, ino: u64) -> bool {
        let mut i = 0usize;
        while i < self.roots.len() {
            if self.roots[i] == ino {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 挂载：一次建两面（J1），重复/自挂/0 号全拒（J5）。
    pub fn attach(&mut self, dentry: u64, root_ino: u64) -> Result<u32, u16> {
        if dentry == 0 || root_ino == 0 {
            return Err(E_INO_ZERO);
        }
        if dentry == root_ino {
            return Err(E_SELF_MNT);
        }
        if self.find_mount(dentry).is_some() {
            self.dup_rejects += 1;
            return Err(E_MNT_DUP);
        }
        // 同一 ino 被挂为两个根：多挂载到同一根在本模型里视为冲突（避免根集合歧义）
        if self.is_root(root_ino) {
            return Err(E_ROOT_DUP);
        }
        let seq = self.mounts.len() as u32 + 1;
        self.mounts.push(MountPoint {
            dentry,
            mount_root_ino: root_ino,
            flags: MNT_COVERED | MNT_ROOT,
            seq,
        });
        self.roots.push(root_ino);
        Ok(seq)
    }

    /// 卸载：按被遮蔽 dentry 摘记录，并从根集合移除（J4 两账同步减）。
    pub fn detach(&mut self, dentry: u64) -> Result<u64, u16> {
        let idx = match self.find_mount(dentry) {
            Some(i) => i,
            None => return Err(E_NO_MNT),
        };
        let root = self.mounts[idx].mount_root_ino;
        self.mounts.remove(idx);
        let mut nr: Vec<u64> = Vec::new();
        let mut i = 0usize;
        while i < self.roots.len() {
            if self.roots[i] != root {
                nr.push(self.roots[i]);
            }
            i += 1;
        }
        self.roots = nr;
        self.umounts += 1;
        Ok(root)
    }

    /// 面查询（J1/J3）：两条索引各查一次，叠加成 Both。
    pub fn face_of(&mut self, id: u64) -> MountFace {
        self.lookups += 1;
        let covered = self.find_mount(id).is_some();
        let root = self.is_root(id);
        match (covered, root) {
            (true, true) => MountFace::Both,
            (true, false) => MountFace::Covered,
            (false, true) => MountFace::MountRoot,
            (false, false) => MountFace::None,
        }
    }

    /// 穿越接口（J2，供 B12 调用）：被遮蔽面 → 挂入面根 ino。
    pub fn cross(&mut self, dentry: u64) -> Result<u64, u16> {
        let idx = match self.find_mount(dentry) {
            Some(i) => i,
            None => return Err(E_NO_CROSS),
        };
        self.crossings += 1;
        Ok(self.mounts[idx].mount_root_ino)
    }

    /// 反向：给挂入面根 ino，回查被遮蔽 dentry（B12 卸载时用）。
    pub fn underlying(&self, root_ino: u64) -> Option<u64> {
        let mut i = 0usize;
        while i < self.mounts.len() {
            if self.mounts[i].mount_root_ino == root_ino {
                return Some(self.mounts[i].dentry);
            }
            i += 1;
        }
        None
    }

    /// 标记位读数（J1 的位断言面）：返回该 dentry 记录的 flags（无记录返回 0）。
    pub fn flags_of(&self, dentry: u64) -> u32 {
        match self.find_mount(dentry) {
            Some(i) => self.mounts[i].flags,
            None => 0,
        }
    }

    /// 账守恒：记录数 == 根集合大小（一挂载一遮蔽一根，J4）。
    pub fn balanced(&self) -> bool {
        self.mounts.len() == self.roots.len()
    }

    /// 双面自审：每条记录的 flags 必须同时含两面标记。
    pub fn faces_audited(&self) -> bool {
        let mut i = 0usize;
        while i < self.mounts.len() {
            let f = self.mounts[i].flags;
            if (f & MNT_COVERED) == 0 || (f & MNT_ROOT) == 0 {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_MNT_DUP => "同一 dentry 重复挂载",
        E_SELF_MNT => "自挂（挂载点与根同号）",
        E_INO_ZERO => "0 号 ino 保留",
        E_NO_MNT => "卸载未挂载点",
        E_NO_CROSS => "该 dentry 不是挂载点，无穿越",
        E_FACE_OOR => "面码越界（合法 0..3）",
        E_BAL => "挂载账不平",
        E_ROOT_DUP => "同一 ino 被两次挂为根",
        _ => "未知诊断码",
    }
}

/// 确定性演练：两面断言 → 穿越 → 嵌套 → 卸载 → 注入拦截 → 账平。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut t = MountTable::new();

    // R1：三次挂载（100→200、101→201、102→202），记录 3 条、根集合 3 个。
    t.attach(100, 200)?;
    t.attach(101, 201)?;
    t.attach(102, 202)?;
    if t.mounts.len() != 3 || t.roots.len() != 3 {
        return Err(E_BAL);
    }
    if !t.balanced() || !t.faces_audited() {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：双面可区分——100 是被遮蔽面，200 是挂入面，二者不混（J1 主面）。
    if t.face_of(100) != MountFace::Covered {
        return Err(E_BAL);
    }
    if t.face_of(200) != MountFace::MountRoot {
        return Err(E_BAL);
    }
    if t.face_of(103) != MountFace::None {
        return Err(E_BAL); // 普通 dentry
    }
    if t.flags_of(100) != (MNT_COVERED | MNT_ROOT) {
        return Err(E_BAL); // 记录本身两面标记俱全
    }
    if t.flags_of(103) != 0 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：穿越接口——cross(100) == 200，反向 underlying(200) == 100（J2）。
    if t.cross(100)? != 200 {
        return Err(E_BAL);
    }
    if t.cross(102)? != 202 {
        return Err(E_BAL);
    }
    if t.underlying(201) != Some(101) {
        return Err(E_BAL);
    }
    if t.cross(103) != Err(E_NO_CROSS) {
        return Err(E_NO_CROSS); // 非挂载点不穿越
    }
    if t.crossings != 2 {
        return Err(E_BAL); // 拒收不计穿越
    }
    hits += 1;

    // R4：嵌套挂载——在 200（上一层挂入面）上再挂 300，200 变 Both（J3）。
    t.attach(200, 300)?;
    if t.face_of(200) != MountFace::Both {
        return Err(E_BAL); // 既是根又是挂载点
    }
    if t.face_of(300) != MountFace::MountRoot {
        return Err(E_BAL);
    }
    if t.cross(200)? != 300 {
        return Err(E_BAL); // 穿越链可续：100→200→300
    }
    if t.cross(100)? != 200 {
        return Err(E_BAL);
    }
    if !t.balanced() || !t.faces_audited() {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：卸载中间层——摘 200→300 记录后 200 回到 MountRoot，账同步减 1（J4）。
    let r = t.detach(200)?;
    if r != 300 {
        return Err(E_BAL);
    }
    if t.face_of(200) != MountFace::MountRoot {
        return Err(E_BAL);
    }
    if t.cross(200) != Err(E_NO_CROSS) {
        return Err(E_NO_CROSS);
    }
    if t.mounts.len() != 3 || t.roots.len() != 3 || t.umounts != 1 {
        return Err(E_BAL);
    }
    if !t.balanced() {
        return Err(E_BAL);
    }
    hits += 1;

    // R6：反复挂/卸 100 次同一对，账始终平（挂载表无泄漏）。
    let mut k = 0u32;
    while k < 100 {
        t.attach(500, 600)?;
        if t.face_of(500) != MountFace::Covered {
            return Err(E_BAL);
        }
        t.detach(500)?;
        if !t.balanced() {
            return Err(E_BAL);
        }
        k += 1;
    }
    if t.mounts.len() != 3 || t.roots.len() != 3 {
        return Err(E_BAL);
    }
    if t.umounts != 101 {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：重复挂载同 dentry → E_MNT_DUP，dup_rejects +1，记录数不变（J5）。
    let n0 = t.mounts.len();
    if t.attach(100, 900) != Err(E_MNT_DUP) {
        return Err(E_MNT_DUP);
    }
    if t.dup_rejects != 1 || t.mounts.len() != n0 {
        return Err(E_BAL);
    }
    hits += 1;

    // I2：自挂 → E_SELF_MNT；0 号 ino → E_INO_ZERO（挂载点/根双点）。
    if t.attach(700, 700) != Err(E_SELF_MNT) {
        return Err(E_SELF_MNT);
    }
    if t.attach(0, 800) != Err(E_INO_ZERO) {
        return Err(E_INO_ZERO);
    }
    if t.attach(700, 0) != Err(E_INO_ZERO) {
        return Err(E_INO_ZERO);
    }
    hits += 1;

    // I3：同一 ino 挂两次根 → E_ROOT_DUP（根集合不得有歧义）。
    if t.attach(701, 200) != Err(E_ROOT_DUP) {
        return Err(E_ROOT_DUP);
    }
    hits += 1;

    // I4：卸载未挂载点 → E_NO_MNT；面码越界 → E_FACE_OOR。
    if t.detach(0xBEEF) != Err(E_NO_MNT) {
        return Err(E_NO_MNT);
    }
    if face_by_code(4) != Err(E_FACE_OOR) {
        return Err(E_FACE_OOR);
    }
    if face_by_code(0xFFFF) != Err(E_FACE_OOR) {
        return Err(E_FACE_OOR);
    }
    hits += 1;

    // I5：面码自洽——四个面码逐一映射回闭集，且 Both = Covered | MountRoot。
    if face_by_code(0)? != MountFace::None
        || face_by_code(1)? != MountFace::Covered
        || face_by_code(2)? != MountFace::MountRoot
        || face_by_code(3)? != MountFace::Both
    {
        return Err(E_FACE_OOR);
    }
    if (MountFace::Covered as u32) | (MountFace::MountRoot as u32) != MountFace::Both as u32 {
        return Err(E_BAL); // 位叠加语义与枚举值必须一致
    }
    hits += 1;

    // 终审：账平 + 双面标记全绿 + 根集合无重复。
    if !t.balanced() || !t.faces_audited() {
        return Err(E_BAL);
    }
    let mut i = 0usize;
    while i < t.roots.len() {
        let mut j = i + 1;
        while j < t.roots.len() {
            if t.roots[i] == t.roots[j] {
                return Err(E_ROOT_DUP);
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;

    Ok(hits)
}
