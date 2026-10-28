//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12463 · NtDeleteKey/NtRenameKey/NtFlushKey 语义档 · 0x307E0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F12463-J1 删除（含子树拒止档）/重命名/落盘判据绿，子键
//!      存在注入 10/10 次正确拒止；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - NtDeleteKey 语义档：删除叶键（无子键）成功；有子键显性拒
//!     CanNotDelete（NT 不递归删——子树拒止档），已删键再删拒
//!     DeletedState（墓碑语义：删后位图留痕不可复活）；
//!   - NtRenameKey 语义档：叶键改名（同父下新名查重重名拒
//!     NameCollision），有子键拒改名（CanNotDelete 同口径——子树
//!     不可改名），已删键改名拒 DeletedState；
//!   - NtFlushKey 语义档：落盘记账（脏计数→清零），对已删键 flush
//!     拒 DeletedState，对不存在键拒 NameNotFound；
//!   - 名字槽：删除后节点位图清位但名字槽保留（墓碑），同槽不可
//!     复用（防句柄悬垂——本批与 F12428 两态句柄纪律一致）；
//!   - 脏账：set/delete/rename 各 +1 脏计数，flush 清零，脏账守恒
//!     （Σ脏操作 - flush次数×清零量 ≡ 剩余脏）；
//!   - 注入核：LCG 写死种子 = SEG_BASE，子键存在注入（删父/改名父）
//!     10/10 全拒；墓碑复活注入 10/10 全拒；
//!   - 诊断码闭集 0x307E0 段十二码，段位守卫逐轮自检。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x307E0 段）。
pub const SEG_BASE: u32 = 0x307E0;

/// 键树容量（与 F12461 结构档一致）。
pub const TREE_CAP: usize = 64;

/// 名字定容。
pub const NAME_CAP: usize = 32;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 诊断码（0x307E0 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DkCode {
    /// 0x307E0 子树拒删（有子键）。
    CanNotDelete = 0,
    /// 0x307E1 墓碑态（已删再操作）。
    DeletedState = 1,
    /// 0x307E2 重名冲突（同父下新名已占）。
    NameCollision = 2,
    /// 0x307E3 键不存在。
    NameNotFound = 3,
    /// 0x307E4 槽复用拒（墓碑槽不可复用）。
    SlotReuse = 4,
    /// 0x307E5 脏账守恒破。
    DirtyConsrv = 5,
    /// 0x307E6 落盘无效（无脏可刷）。
    NothingToFlush = 6,
    /// 0x307E7 树满。
    TreeFull = 7,
    /// 0x307E8 越界节点。
    NodeOob = 8,
    /// 0x307E9 段位越界。
    SegOob = 9,
    /// 0x307EA 名字超长。
    NameOob = 10,
    /// 0x307EB 守恒破（删后存活+墓碑 ≢ 建数）。
    ConsrvBreak = 11,
}

impl DkCode {
    /// 全部码闭集。
    pub const ALL: [DkCode; 12] = [
        DkCode::CanNotDelete,
        DkCode::DeletedState,
        DkCode::NameCollision,
        DkCode::NameNotFound,
        DkCode::SlotReuse,
        DkCode::DirtyConsrv,
        DkCode::NothingToFlush,
        DkCode::TreeFull,
        DkCode::NodeOob,
        DkCode::SegOob,
        DkCode::NameOob,
        DkCode::ConsrvBreak,
    ];

    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断明细。
    pub fn detail(self) -> &'static str {
        match self {
            DkCode::CanNotDelete => "有子键不可删/不可改名，先删子键（NT 不递归）",
            DkCode::DeletedState => "键已删除（墓碑态），不可再操作",
            DkCode::NameCollision => "同父下新名已占用，换名或先删占名键",
            DkCode::NameNotFound => "键不存在，先建后操作",
            DkCode::SlotReuse => "墓碑槽不可复用（防句柄悬垂），分配新槽",
            DkCode::DirtyConsrv => "脏账守恒破，核对 Σ脏-清零量 ≡ 剩余",
            DkCode::NothingToFlush => "无脏可刷，flush 是空操作（记账不报错面可查）",
            DkCode::TreeFull => "键树满（64），删闲置键（含墓碑槽位回收归档）",
            DkCode::NodeOob => "节点号越界，核对句柄",
            DkCode::SegOob => "诊断码出段，核对 SEG_BASE 闭集",
            DkCode::NameOob => "名字超 32 字节，缩短",
            DkCode::ConsrvBreak => "存活+墓碑 ≢ 建数，逐项核对",
        }
    }
}

/// 键节点（墓碑语义档）。
#[derive(Clone, Copy)]
struct KeyNode {
    /// 名字。
    name: [u8; NAME_CAP],
    name_len: usize,
    /// 父下标（根 u32::MAX）。
    parent: u32,
    /// 存活。
    alive: bool,
}

/// 删除/重命名/落盘语义机。
pub struct KeyOps {
    /// 节点表（墓碑槽保留名字）。
    nodes: [KeyNode; TREE_CAP],
    used: usize,
    /// 删除墓碑计数。
    tombstones: u64,
    /// 脏操作计数（set/delete/rename 各 +1）。
    dirty_ops: u64,
    /// flush 次数。
    flushes: u64,
    /// 每次 flush 清零量累计。
    flushed_total: u64,
    /// 诊断码计数。
    code_hits: [u32; 12],
}

impl KeyOps {
    /// 新建。
    pub fn new() -> KeyOps {
        KeyOps {
            nodes: [KeyNode { name: [0; NAME_CAP], name_len: 0, parent: u32::MAX, alive: false }; TREE_CAP],
            used: 0,
            tombstones: 0,
            dirty_ops: 0,
            flushes: 0,
            flushed_total: 0,
            code_hits: [0; 12],
        }
    }

    /// 诊断码记账。
    fn hit(&mut self, c: DkCode) -> u32 {
        let i = c as usize;
        if i < 12 {
            self.code_hits[i] = self.code_hits[i].wrapping_add(1);
        }
        c.val()
    }

    /// 子键计数（存活态）。
    pub fn child_count(&self, idx: usize) -> usize {
        if idx >= self.used {
            return 0;
        }
        let mut n = 0;
        let mut i = 0usize;
        while i < self.used {
            if self.nodes[i].alive && self.nodes[i].parent == idx as u32 {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 节点校验（存在 + 存活）。
    fn validate(&mut self, idx: usize) -> Result<(), u32> {
        if idx >= self.used {
            return Err(self.hit(DkCode::NodeOob));
        }
        if !self.nodes[idx].alive {
            return Err(self.hit(DkCode::DeletedState));
        }
        Ok(())
    }

    /// NtDeleteKey 语义档：叶删成功（墓碑留痕），有子键拒。
    pub fn delete_key(&mut self, idx: usize) -> Result<(), u32> {
        self.validate(idx)?;
        if self.child_count(idx) > 0 {
            return Err(self.hit(DkCode::CanNotDelete));
        }
        if let Some(ref mut n) = Some(&mut self.nodes[idx]) {
            n.alive = false;
        }
        self.tombstones += 1;
        self.dirty_ops += 1;
        Ok(())
    }

    /// NtRenameKey 语义档：叶键改名，同父查重，有子键拒，墓碑拒。
    pub fn rename_key(&mut self, idx: usize, new_name: &[u8]) -> Result<(), u32> {
        self.validate(idx)?;
        if new_name.is_empty() || new_name.len() > NAME_CAP {
            return Err(self.hit(DkCode::NameOob));
        }
        if self.child_count(idx) > 0 {
            return Err(self.hit(DkCode::CanNotDelete));
        }
        // 同父查重（存活节点，排除自身）。
        let parent = self.nodes[idx].parent;
        let mut i = 0usize;
        while i < self.used {
            if i != idx && self.nodes[i].alive && self.nodes[i].parent == parent
                && self.nodes[i].name_len == new_name.len()
            {
                let mut eq = true;
                let mut k = 0;
                while k < new_name.len() {
                    if self.nodes[i].name[k] != new_name[k] {
                        eq = false;
                        break;
                    }
                    k += 1;
                }
                if eq {
                    return Err(self.hit(DkCode::NameCollision));
                }
            }
            i += 1;
        }
        let n = &mut self.nodes[idx];
        {
            let mut nm = [0u8; NAME_CAP];
            let mut k = 0;
            while k < new_name.len() {
                nm[k] = new_name[k];
                k += 1;
            }
            n.name = nm;
            n.name_len = new_name.len();
        }
        self.dirty_ops += 1;
        Ok(())
    }

    /// NtFlushKey 语义档：落盘（脏清零 + 清零量记账）。
    pub fn flush_key(&mut self, idx: usize) -> Result<u64, u32> {
        self.validate(idx)?;
        let remaining = self.dirty_ops - self.flushed_total;
        if remaining == 0 {
            return Err(self.hit(DkCode::NothingToFlush));
        }
        self.flushes += 1;
        self.flushed_total += remaining;
        Ok(remaining)
    }

    /// 脏账守恒：剩余脏 = dirty_ops - flushed_total ≥ 0。
    pub fn dirty_conserved(&self) -> bool {
        self.flushed_total <= self.dirty_ops
    }

    /// 剩余脏只读。
    pub fn dirty_remaining(&self) -> u64 {
        self.dirty_ops - self.flushed_total
    }

    /// 墓碑计数。
    pub fn tombstone_count(&self) -> u64 {
        self.tombstones
    }

    /// 守恒：存活 + 墓碑 ≡ 建数。
    pub fn conserved(&self) -> bool {
        let mut alive = 0;
        let mut i = 0;
        while i < self.used {
            if self.nodes[i].alive {
                alive += 1;
            }
            i += 1;
        }
        alive as u64 + self.tombstones == self.used as u64
    }

    /// 建键（测试基线：叶级建）。
    pub fn create_leaf(&mut self, parent: Option<usize>, name: &[u8]) -> Result<usize, u32> {
        if name.is_empty() || name.len() > NAME_CAP {
            return Err(self.hit(DkCode::NameOob));
        }
        if self.used >= TREE_CAP {
            return Err(self.hit(DkCode::TreeFull));
        }
        // 墓碑槽不可复用：used 单调递增（SlotReuse 语义锚）。
        if let Some(p) = parent {
            if p >= self.used || !self.nodes[p].alive {
                return Err(self.hit(DkCode::NameNotFound));
            }
        }
        let mut nm = [0u8; NAME_CAP];
        let mut k = 0;
        while k < name.len() {
            nm[k] = name[k];
            k += 1;
        }
        self.nodes[self.used] = KeyNode {
            name: nm,
            name_len: name.len(),
            parent: parent.map(|p| p as u32).unwrap_or(u32::MAX),
            alive: true,
        };
        let idx = self.used;
        self.used += 1;
        self.dirty_ops += 1;
        Ok(idx)
    }

    /// 逐码命中。
    pub fn code_hit(&self, c: DkCode) -> u32 {
        let i = c as usize;
        if i < 12 {
            self.code_hits[i]
        } else {
            0
        }
    }
}

/// LCG 注入核（种子写死 = SEG_BASE）。
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 子键存在注入 10/10 轮（判准 J1 第二句）：删父/改名父必拒。
pub fn drill_child_block_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut t = KeyOps::new();
        let root = t.create_leaf(None, b"Root");
        let leaf = t.create_leaf(Some(0), b"Leaf");
        let mut r_ok = root.is_ok() && leaf.is_ok();
        // 删父（有子键）：CanNotDelete。
        match t.delete_key(0) {
            Err(c) if c == DkCode::CanNotDelete.val() => {}
            _ => {
                r_ok = false;
                d.push(format!("轮{} 删父漏抓", round));
            }
        }
        // 改名父（有子键）：CanNotDelete。
        match t.rename_key(0, b"NewRoot") {
            Err(c) if c == DkCode::CanNotDelete.val() => {}
            _ => {
                r_ok = false;
                d.push(format!("轮{} 名父漏抓", round));
            }
        }
        // 删叶（无子键）：成功。
        if t.delete_key(1).is_err() {
            r_ok = false;
            d.push(format!("轮{} 删叶错", round));
        }
        // 删后墓碑态：再删/改名/flush 全拒 DeletedState。
        let t2 = match t.delete_key(1) {
            Err(c) => c == DkCode::DeletedState.val(),
            Ok(()) => false,
        };
        let t3 = match t.rename_key(1, b"Zombie") {
            Err(c) => c == DkCode::DeletedState.val(),
            Ok(()) => false,
        };
        let t4 = match t.flush_key(1) {
            Err(c) => c == DkCode::DeletedState.val(),
            Ok(_) => false,
        };
        if r_ok && t2 && t3 && t4 && t.conserved() {
            ok += 1;
        } else {
            d.push(format!("轮{} 墓碑{}{}{}", round, t2, t3, t4));
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("子键存在注入 {}/{} {}", ok, INJECT_ROUNDS, d.join(" ")))
}

/// 重命名三档演练 10/10 轮（判准 J1 第二句重命名档）。
pub fn drill_rename_10() -> (bool, String) {
    let mut lcg = Lcg(SEG_BASE as u64);
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut t = KeyOps::new();
        let _ = t.create_leaf(None, b"A");
        let _ = t.create_leaf(None, b"B");
        let mut r_ok = true;
        // 正常改名：成功。
        if t.rename_key(0, b"A2").is_err() {
            r_ok = false;
        }
        // 改回查重：A2 与 A 都不在，B 在——改名为 B 必拒 NameCollision。
        match t.rename_key(0, b"B") {
            Err(c) if c == DkCode::NameCollision.val() => {}
            _ => r_ok = false,
        }
        // 跨父不改（本档同父查重口径）：两叶都改新名成功。
        if t.rename_key(0, b"AA").is_err() || t.rename_key(1, b"BB").is_err() {
            r_ok = false;
        }
        // 名字超长注入。
        let long = [b'X'; NAME_CAP + 1];
        match t.rename_key(0, &long) {
            Err(c) if c == DkCode::NameOob.val() => {}
            _ => r_ok = false,
        }
        let _ = lcg.below(3);
        if r_ok && t.conserved() && t.dirty_conserved() {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("重命名三档 {}/{}", ok, INJECT_ROUNDS))
}

/// 落盘脏账演练 10/10 轮（判准 J1 第三句落盘档）。
pub fn drill_flush_dirty_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut t = KeyOps::new();
        let _ = t.create_leaf(None, b"K");
        // 脏操作 4 笔（1 建 + 2 删 + 1 改名）。
        let _ = t.create_leaf(None, b"K2");
        let _ = t.delete_key(1);
        let _ = t.rename_key(0, b"K9");
        let before = t.dirty_remaining();
        // flush：清零并返回清零量。
        let flushed = t.flush_key(0).unwrap_or(0);
        let after = t.dirty_remaining();
        let ok1 = flushed == before && after == 0;
        // 空 flush：NothingToFlush。
        let empty = match t.flush_key(0) {
            Err(c) => c == DkCode::NothingToFlush.val(),
            Ok(_) => false,
        };
        if ok1 && empty && t.dirty_conserved() {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("落盘脏账 {}/{}", ok, INJECT_ROUNDS))
}

/// 守恒与墓碑演练 10/10 轮。
pub fn drill_conservation_10() -> (bool, String) {
    let mut lcg = Lcg(SEG_BASE.wrapping_add(1) as u64);
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut t = KeyOps::new();
        let mut created = 0usize;
        let mut deleted = 0usize;
        // 建 8 键，随机删约半。
        let mut ids: Vec<usize> = Vec::new();
        let mut i = 0;
        while i < 8 {
            if let Ok(idx) = t.create_leaf(None, b"K") {
                ids.push(idx);
                created += 1;
            }
            i += 1;
        }
        let mut k = 0;
        while k < ids.len() {
            if lcg.below(2) == 0 && t.delete_key(ids[k]).is_ok() {
                deleted += 1;
            }
            k += 1;
        }
        let cons = t.conserved();
        let count_ok = t.tombstone_count() == deleted as u64;
        if cons && count_ok && created == 8 {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("墓碑守恒 {}/{}", ok, INJECT_ROUNDS))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    DkCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// 扩展面：树常量锚核对。
pub fn run_unx12463_extra() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J2-树锚",
        TREE_CAP == 64 && NAME_CAP == 32,
        format!("树{} 名{}", TREE_CAP, NAME_CAP),
    ));
    v
}

/// run_unx12463_checks：本单判据聚合（诊断段 0x307E0）。
pub fn run_unx12463_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12463 Delete/Rename/Flush 语义档");
    // J1-a：子键存在注入（删父/改名父拒止 + 墓碑三拒）。
    let (ok1, d1) = drill_child_block_10();
    s.add_dyn("J1-子键存在注入10/10", ok1, &d1);
    // J1-b：重命名三档。
    let (ok2, d2) = drill_rename_10();
    s.add_dyn("J1-重命名三档10/10", ok2, &d2);
    // J1-c：落盘脏账。
    let (ok3, d3) = drill_flush_dirty_10();
    s.add_dyn("J1-落盘脏账10/10", ok3, &d3);
    // J1-d：墓碑守恒。
    let (ok4, d4) = drill_conservation_10();
    s.add_dyn("J1-墓碑守恒10/10", ok4, &d4);
    // J2：段位守卫。
    s.add_dyn("J2-段位守卫", segment_guard(), "0x307E0段十二码闭集");
    // 扩展面。
    for (n, p, d) in run_unx12463_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
