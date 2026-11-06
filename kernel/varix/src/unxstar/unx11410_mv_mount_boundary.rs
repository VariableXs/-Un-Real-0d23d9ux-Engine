//! UNX-F11410 mv 目录移动与挂载点边界
//!
//! # 领域模型
//!
//! 本卡研究“目录移动”在挂载点边界上的行为。内核里的 `mv` 并不是一个
//! 单一系统调用：同设备内是 `rename(2)`，跨设备则是“复制整棵子树，再
//! 删除源”。挂载点把这两条路径都切断了：
//!
//! * 挂载根本身不能改名、不能搬移。它代表“另一台设备从这里接入”，
//!   目录项虽然仍挂在父目录里，但名字/位置属于挂载表而非普通目录项，
//!   于是移动它一律 `EBUSY`。
//! * 含挂载点的子树整体搬移同样 `EBUSY`。子树里有一个挂载根，意味着
//!   子树跨越了设备视图；把它整体挪走等于悄悄卸载，真实实现拒绝。
//! * 把目录移进挂载根本身是跨设备操作，返回 `EXDEV`；回退路径是逐项
//!   复制，但本卡只裁决“是否允许直接改名”，因此停在 `EXDEV`。
//! * 用另一目录覆盖挂载点同样 `EBUSY`：替换会把挂载根从名字空间里
//!   抹掉，这是卸载而不是改名。
//!
//! 树模型只有目录记录与目录项两类对象。目录记录带 `dev`（它所属设备）、
//! `mnt`（是否为挂载根）、`ro`（只读）、`busy`（被钉住）；目录项只是
//! `(父目录, 名字, 目标 inode)` 三元组，可以指向目录，也可以指向文件
//! （文件不占目录记录，用于构造 `ENOTDIR`）。挂载把目录记录的 `dev`
//! 改成新设备，并把 `mnt` 置位；卸载恢复父目录的设备号。挂载点必须
//! 为空目录：非空挂载在真实内核里会遮蔽旧内容，本模型拒绝 `ENOTEMPTY`。
//!
//! # 闸门次序
//!
//! `move_ent(源路径, 目标目录, 新名字)` 严格按下列次序裁决，先命中者
//! 即返回，后续条件不再观察：
//!
//! 1. 源路径为空 → `EINVAL`（根不可移动）；
//! 2. 源名字为零 → `EINVAL`；源名字超过 255 → `ENAMETOOLONG`；
//! 3. 新名字为零 → `EINVAL`；新名字超过 255 → `ENAMETOOLONG`；
//! 4. 源父路径逐段解析失败 → `ENOENT`；
//! 5. 源目录项不存在 → `ENOENT`；
//! 6. 源 inode 是挂载根，或子树内含挂载根 → `EBUSY`；
//! 7. 目标目录不存在 → `ENOENT`；
//! 8. 目标位置上的目录项是挂载根 → `EBUSY`；
//! 9. 源目录记录被钉住 → `EBUSY`；
//! 10. 源父目录或目标目录只读 → `EROFS`；
//! 11. 目标目录落在源子树内（含自身） → `EINVAL`；
//! 12. 源父目录与目标目录设备号不同 → `EXDEV`；
//! 13. 同目录同名 → 无操作成功（`rename` 自指语义）；
//! 14. 目标位置上已有对象：是文件 → `ENOTDIR`；是自身别名 → `EINVAL`；
//!     是非空目录 → `ENOTEMPTY`；
//! 15. 目录项表无空位 → `EINVAL`；
//! 16. 落位：先删目标项，再删源项，追加新项，改写源目录记录的父指针。
//!
//! 注意第 2、3 步在解析路径之前：名字形状是语法检查，路径存在性是
//! 语义检查，语法先于语义。这与 `mv` 先解析参数、再碰文件系统一致。
//!
//! # 账本契约
//!
//! `build_ledger()` 产生十四格：八次成功、六次失败（六类互不相同的
//! 错误码，绝不使用账本自身的 E*/ESLOT/EVALUE/ESEAL/ENOBS）。成功格
//! 依次是三次 `mkdir`、一次同设备目录移动、一次挂载、一次移回、一次
//! `mkdir`、一次 `link`；失败格依次触发 `EINVAL`（空路径）、`EBUSY`
//! （挂载根）、`ENAMETOOLONG`（名字过长）、`EXDEV`（移入挂载点）、
//! `ENOENT`（源缺失）、`ENOTEMPTY`（覆盖非空目录）。
//!
//! `audit()` 逐格核对：槽位必须从 0 连续编号；八个成功格的观测值必须
//! 精确等于 `[2, 3, 4, 4, 1, 4, 5, 1]`；六个失败格的错误码必须精确
//! 等于 `[EINVAL, EBUSY, ENAMETOOLONG, EXDEV, ENOENT, ENOTEMPTY]`；
//! 失败格的观测值必须为零，成功格的错误码必须为零。
//!
//! `seal()` 用 FNV-1a 把每格的槽位、操作码、成败、错误码、观测值和
//! 拒绝计数折成 64 位指纹；`freeze()` 固化指纹并置冻结位；`replay()`
//! 重新构建账本并按同一规则折叠，指纹必须与冻结值相同，否则 `ESEAL`。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址：本卡独占的 16 个错误码窗口。
pub const SEG_BASE: u32 = 0xefa0;

/// 源路径为空、名字为零、覆盖自身别名、目录项表满。
pub const MB_EINVAL: u32 = SEG_BASE + 0x01;
/// 源目录、源目录项或目标目录不存在。
pub const MB_ENOENT: u32 = SEG_BASE + 0x02;
/// 目录不能覆盖文件。
pub const MB_ENOTDIR: u32 = SEG_BASE + 0x03;
/// 目标目录非空，或挂载点非空。
pub const MB_ENOTEMPTY: u32 = SEG_BASE + 0x04;
/// 挂载根、含挂载根的子树、被钉住的目录。
pub const MB_EBUSY: u32 = SEG_BASE + 0x05;
/// 源父目录或目标目录只读。
pub const MB_EROFS: u32 = SEG_BASE + 0x06;
/// 源父目录与目标目录设备号不同（含移入挂载点）。
pub const MB_EXDEV: u32 = SEG_BASE + 0x07;
/// 名字长度超过 `NAME_MAX`。
pub const MB_ENAMETOOLONG: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const MB_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位错乱。
pub const MB_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const MB_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const MB_ESEAL: u32 = SEG_BASE + 0x0c;
/// 账本无此观测。
pub const MB_ENOBS: u32 = SEG_BASE + 0x0d;

/// 目录记录容量。
pub const CAP: usize = 6;
/// 目录项容量。
pub const CAP_ENT: usize = 12;
/// 根 inode 编号。
pub const ROOT: u32 = 1;
/// 名字长度上限。
pub const NAME_MAX: u16 = 0xff;

/// 空目标位。
pub const KIND_NONE: u8 = 0;
/// 目录。
pub const KIND_DIR: u8 = 1;
/// 文件（不占目录记录）。
pub const KIND_FILE: u8 = 2;

/// 建目录。
pub const OP_MKDIR: u8 = 1;
/// 移动目录项。
pub const OP_MOVE: u8 = 2;
/// 挂载。
pub const OP_MOUNT: u8 = 3;
/// 硬链接（用于制造非空目录与文件目标）。
pub const OP_LINK: u8 = 4;
/// 查询。
pub const OP_QUERY: u8 = 5;

/// 样本格数。
pub const SAMPLES: usize = 14;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个 64 位字（低 8 字节，逐字节）。
pub fn fold(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut n = v;
    let mut i = 0u32;
    while i < 8 {
        x ^= n & 0xff;
        x = x.wrapping_mul(FNV_PRIME);
        n >>= 8;
        i += 1;
    }
    x
}

/// 一条目录记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirRec {
    /// inode 编号。
    pub ino: u32,
    /// 父目录 inode（根指向自身）。
    pub parent: u32,
    /// 是否占用。
    pub used: bool,
    /// 所属设备号。
    pub dev: u16,
    /// 是否为挂载根。
    pub mnt: bool,
    /// 是否只读。
    pub ro: bool,
    /// 是否被钉住。
    pub busy: bool,
}

impl DirRec {
    /// 空白记录。
    pub const fn blank() -> DirRec {
        DirRec {
            ino: 0,
            parent: 0,
            used: false,
            dev: 0,
            mnt: false,
            ro: false,
            busy: false,
        }
    }
}

/// 一个目录项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ent {
    /// 所在目录。
    pub dir: u32,
    /// 名字。
    pub name: u16,
    /// 目标 inode。
    pub ino: u32,
}

impl Ent {
    /// 空白项。
    pub const fn blank() -> Ent {
        Ent { dir: 0, name: 0, ino: 0 }
    }
}

/// 目录树：定长目录记录表 + 定长目录项表。
#[derive(Clone, Copy, Debug)]
pub struct Tree {
    dirs: [DirRec; CAP],
    ents: [Ent; CAP_ENT],
    nent: usize,
    hops: u32,
}

impl Tree {
    /// 以给定设备号建立只有根的树。
    pub fn new(dev: u16) -> Tree {
        let mut t = Tree {
            dirs: [DirRec::blank(); CAP],
            ents: [Ent::blank(); CAP_ENT],
            nent: 0,
            hops: 0,
        };
        if let Some(d) = t.dirs.get_mut(0) {
            *d = DirRec {
                ino: ROOT,
                parent: ROOT,
                used: true,
                dev,
                mnt: false,
                ro: false,
                busy: false,
            };
        }
        t
    }

    /// 目录记录表下标。
    fn dir_of(&self, ino: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < CAP {
            if let Some(d) = self.dirs.get(i) {
                if d.used && d.ino == ino {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 取目录记录副本。
    pub fn dir_rec(&self, ino: u32) -> Result<DirRec, u32> {
        match self.dir_of(ino) {
            Some(i) => match self.dirs.get(i) {
                Some(d) => Ok(*d),
                None => Err(MB_ENOENT),
            },
            None => Err(MB_ENOENT),
        }
    }

    /// 目录是否存在。
    pub fn exists(&self, ino: u32) -> bool {
        self.dir_of(ino).is_some()
    }

    /// 目录项下标。
    fn find_idx(&self, dir: u32, name: u16) -> Result<usize, u32> {
        let mut i = 0usize;
        while i < self.nent {
            if let Some(e) = self.ents.get(i) {
                if e.dir == dir && e.name == name {
                    return Ok(i);
                }
            }
            i += 1;
        }
        Err(MB_ENOENT)
    }

    /// 查一个名字。
    pub fn lookup(&self, dir: u32, name: u16) -> Result<u32, u32> {
        let i = self.find_idx(dir, name)?;
        match self.ents.get(i) {
            Some(e) => Ok(e.ino),
            None => Err(MB_ENOENT),
        }
    }

    /// 目录下的子项数（记录不存在按 0 计）。
    pub fn children(&self, ino: u32) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.nent {
            if let Some(e) = self.ents.get(i) {
                if e.dir == ino {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 子项数，但要求目录存在。
    pub fn count_children(&self, ino: u32) -> Result<u32, u32> {
        let _ = self.dir_rec(ino)?;
        Ok(self.children(ino))
    }

    /// 父目录。
    pub fn parent_of(&self, ino: u32) -> Result<u32, u32> {
        Ok(self.dir_rec(ino)?.parent)
    }

    /// 标志位：bit0 占用、bit1 挂载根、bit2 只读、bit3 钉住。
    pub fn flags(&self, ino: u32) -> Result<u8, u32> {
        let r = self.dir_rec(ino)?;
        let mut f = 1u8;
        if r.mnt {
            f |= 2;
        }
        if r.ro {
            f |= 4;
        }
        if r.busy {
            f |= 8;
        }
        Ok(f)
    }

    /// 从 `ino` 沿父链上溯，判断 `anc` 是否为祖先（含自身）。
    pub fn within(&self, ino: u32, anc: u32) -> Result<bool, u32> {
        let mut cur = ino;
        let mut n = 0usize;
        while n <= CAP {
            if cur == anc {
                return Ok(true);
            }
            let rec = self.dir_rec(cur)?;
            if rec.parent == cur {
                return Ok(false);
            }
            cur = rec.parent;
            n += 1;
        }
        Ok(false)
    }

    /// 子树内是否含挂载根（含自身为挂载根）。
    pub fn holds_mount(&self, ino: u32) -> bool {
        let mut i = 0usize;
        while i < CAP {
            if let Some(d) = self.dirs.get(i) {
                if d.used && d.mnt {
                    if d.ino == ino {
                        return true;
                    }
                    if matches!(self.within(d.ino, ino), Ok(true)) {
                        return true;
                    }
                }
            }
            i += 1;
        }
        false
    }

    /// 逐段解析路径，空路径即根。
    pub fn resolve(&mut self, path: &[u16]) -> Result<u32, u32> {
        if path.len() > CAP {
            return Err(MB_EINVAL);
        }
        let mut cur = ROOT;
        let mut i = 0usize;
        while i < path.len() {
            let name = match path.get(i) {
                Some(v) => *v,
                None => return Err(MB_ENOENT),
            };
            let _ = self.dir_rec(cur)?;
            self.hops = self.hops.saturating_add(1);
            cur = self.lookup(cur, name)?;
            i += 1;
        }
        Ok(cur)
    }

    /// 建目录。
    pub fn mkdir(&mut self, parent: u32, name: u16, ino: u32, dev: u16) -> Result<u32, u32> {
        let prec = self.dir_rec(parent)?;
        if name == 0 || ino == 0 {
            return Err(MB_EINVAL);
        }
        if name > NAME_MAX {
            return Err(MB_ENAMETOOLONG);
        }
        if prec.ro {
            return Err(MB_EROFS);
        }
        if self.find_idx(parent, name).is_ok() {
            return Err(MB_EINVAL);
        }
        if self.dir_of(ino).is_some() {
            return Err(MB_EINVAL);
        }
        if self.nent >= CAP_ENT {
            return Err(MB_EINVAL);
        }
        let mut free = CAP;
        let mut i = 0usize;
        while i < CAP {
            if let Some(d) = self.dirs.get(i) {
                if !d.used {
                    free = i;
                    break;
                }
            }
            i += 1;
        }
        if free == CAP {
            return Err(MB_EINVAL);
        }
        if let Some(d) = self.dirs.get_mut(free) {
            *d = DirRec {
                ino,
                parent,
                used: true,
                dev,
                mnt: false,
                ro: false,
                busy: false,
            };
        }
        if let Some(e) = self.ents.get_mut(self.nent) {
            *e = Ent { dir: parent, name, ino };
        }
        self.nent = self.nent.saturating_add(1);
        Ok(ino)
    }

    /// 追加目录项（目标可以是文件 inode）。
    pub fn link(&mut self, dir: u32, name: u16, ino: u32) -> Result<u32, u32> {
        let rec = self.dir_rec(dir)?;
        if name == 0 || ino == 0 {
            return Err(MB_EINVAL);
        }
        if name > NAME_MAX {
            return Err(MB_ENAMETOOLONG);
        }
        if rec.ro {
            return Err(MB_EROFS);
        }
        if self.find_idx(dir, name).is_ok() {
            return Err(MB_EINVAL);
        }
        if self.nent >= CAP_ENT {
            return Err(MB_EINVAL);
        }
        if let Some(e) = self.ents.get_mut(self.nent) {
            *e = Ent { dir, name, ino };
        }
        self.nent = self.nent.saturating_add(1);
        Ok(1)
    }

    /// 在空目录上挂载新设备，返回新设备号。
    pub fn mount(&mut self, ino: u32, dev: u16) -> Result<u32, u32> {
        let rec = self.dir_rec(ino)?;
        if rec.mnt {
            return Err(MB_EBUSY);
        }
        if self.children(ino) > 0 {
            return Err(MB_ENOTEMPTY);
        }
        if let Some(pos) = self.dir_of(ino) {
            if let Some(d) = self.dirs.get_mut(pos) {
                d.mnt = true;
                d.dev = dev;
            }
        }
        Ok(dev as u32)
    }

    /// 卸载，设备号回落到父目录。
    pub fn umount(&mut self, ino: u32) -> Result<u32, u32> {
        let rec = self.dir_rec(ino)?;
        if !rec.mnt {
            return Err(MB_EINVAL);
        }
        if rec.busy {
            return Err(MB_EBUSY);
        }
        let base = match self.dir_rec(rec.parent) {
            Ok(p) => p.dev,
            Err(_) => rec.dev,
        };
        if let Some(pos) = self.dir_of(ino) {
            if let Some(d) = self.dirs.get_mut(pos) {
                d.mnt = false;
                d.dev = base;
            }
        }
        Ok(base as u32)
    }

    /// 设置只读。
    pub fn set_ro(&mut self, ino: u32, on: bool) -> Result<(), u32> {
        let _ = self.dir_rec(ino)?;
        if let Some(pos) = self.dir_of(ino) {
            if let Some(d) = self.dirs.get_mut(pos) {
                d.ro = on;
            }
        }
        Ok(())
    }

    /// 设置钉住。
    pub fn set_busy(&mut self, ino: u32, on: bool) -> Result<(), u32> {
        let _ = self.dir_rec(ino)?;
        if let Some(pos) = self.dir_of(ino) {
            if let Some(d) = self.dirs.get_mut(pos) {
                d.busy = on;
            }
        }
        Ok(())
    }

    /// 删除第 `i` 个目录项并压实。
    fn drop_at(&mut self, i: usize) {
        if i >= self.nent {
            return;
        }
        let mut j = i;
        while j.saturating_add(1) < self.nent {
            if let Some(next) = self.ents.get(j.saturating_add(1)).copied() {
                if let Some(dst) = self.ents.get_mut(j) {
                    *dst = next;
                }
            }
            j = j.saturating_add(1);
        }
        let last = self.nent.saturating_sub(1);
        if let Some(e) = self.ents.get_mut(last) {
            *e = Ent::blank();
        }
        self.nent = self.nent.saturating_sub(1);
    }

    /// 移动目录项：`src_path` 的最后一段为源名字，其余为源父路径。
    pub fn move_ent(
        &mut self,
        src_path: &[u16],
        dst_dir: u32,
        newname: u16,
    ) -> Result<u32, u32> {
        if src_path.is_empty() {
            return Err(MB_EINVAL);
        }
        let (sparent, sname) = match split_last(src_path) {
            Some(v) => v,
            None => return Err(MB_EINVAL),
        };
        if sname == 0 {
            return Err(MB_EINVAL);
        }
        if sname > NAME_MAX {
            return Err(MB_ENAMETOOLONG);
        }
        if newname == 0 {
            return Err(MB_EINVAL);
        }
        if newname > NAME_MAX {
            return Err(MB_ENAMETOOLONG);
        }
        let sdir = self.resolve(sparent)?;
        let sidx = self.find_idx(sdir, sname)?;
        let ino = match self.ents.get(sidx) {
            Some(e) => e.ino,
            None => return Err(MB_ENOENT),
        };
        if self.holds_mount(ino) {
            return Err(MB_EBUSY);
        }
        let drec = self.dir_rec(dst_dir)?;
        let existing = self.find_idx(dst_dir, newname).ok();
        if let Some(i) = existing {
            if let Some(e) = self.ents.get(i) {
                if let Ok(r) = self.dir_rec(e.ino) {
                    if r.mnt {
                        return Err(MB_EBUSY);
                    }
                }
            }
        }
        let srec = self.dir_rec(ino)?;
        if srec.busy {
            return Err(MB_EBUSY);
        }
        let sprec = self.dir_rec(sdir)?;
        if sprec.ro || drec.ro {
            return Err(MB_EROFS);
        }
        if drec.ino == ino {
            return Err(MB_EINVAL);
        }
        if matches!(self.within(dst_dir, ino), Ok(true)) {
            return Err(MB_EINVAL);
        }
        if sprec.dev != drec.dev {
            return Err(MB_EXDEV);
        }
        if sdir == dst_dir && sname == newname {
            return Ok(ino);
        }
        let mut replace = false;
        if let Some(i) = existing {
            let dino = match self.ents.get(i) {
                Some(e) => e.ino,
                None => return Err(MB_ENOENT),
            };
            match self.dir_rec(dino) {
                Ok(r) => {
                    if r.ino == ino {
                        return Err(MB_EINVAL);
                    }
                    if self.children(dino) > 0 {
                        return Err(MB_ENOTEMPTY);
                    }
                    replace = true;
                }
                Err(_) => return Err(MB_ENOTDIR),
            }
        }
        if !replace && self.nent >= CAP_ENT {
            return Err(MB_EINVAL);
        }
        if replace {
            if let Ok(di) = self.find_idx(dst_dir, newname) {
                self.drop_at(di);
            }
        }
        let si = self.find_idx(sdir, sname)?;
        self.drop_at(si);
        if let Some(e) = self.ents.get_mut(self.nent) {
            *e = Ent { dir: dst_dir, name: newname, ino };
            self.nent = self.nent.saturating_add(1);
        }
        if let Some(pos) = self.dir_of(ino) {
            if let Some(d) = self.dirs.get_mut(pos) {
                d.parent = dst_dir;
            }
        }
        Ok(ino)
    }

    /// 树内不变式：根存在且自指、每条项指向已占用目录或非零文件 inode、
    /// 目录父指针存在。
    pub fn consistent(&self) -> bool {
        match self.dirs.get(0) {
            Some(r) => {
                if !r.used || r.ino != ROOT || r.parent != ROOT {
                    return false;
                }
            }
            None => return false,
        }
        let mut i = 0usize;
        while i < self.nent {
            match self.ents.get(i) {
                Some(e) => {
                    if e.dir == 0 || e.name == 0 || e.ino == 0 {
                        return false;
                    }
                    if self.dir_rec(e.dir).is_err() {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        let mut k = 0usize;
        while k < CAP {
            if let Some(d) = self.dirs.get(k) {
                if d.used && self.dir_rec(d.parent).is_err() {
                    return false;
                }
            }
            k += 1;
        }
        true
    }

    /// 查询次数（解析过的路径段总数）。
    pub fn hops(&self) -> u32 {
        self.hops
    }
}

/// 拆出路径最后一段。
fn split_last(path: &[u16]) -> Option<(&[u16], u16)> {
    let n = path.len();
    if n == 0 {
        return None;
    }
    let name = match path.get(n.saturating_sub(1)) {
        Some(v) => *v,
        None => return None,
    };
    match path.get(..n.saturating_sub(1)) {
        Some(p) => Some((p, name)),
        None => None,
    }
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽位。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 是否成功。
    pub ok: bool,
    /// 错误码。
    pub code: u32,
    /// 观测值。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }
}

/// `Result<usize, u32>` → 样本。
fn cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<u8, u32>` → 样本。
fn byte_cell(slot: u8, op: u8, r: Result<u8, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 样本账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    root: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记一格；槽位必须连续，冻结后拒绝。
    pub fn register(&mut self, row: Sample) -> Result<(), u32> {
        if self.frozen || (row.slot as usize) != self.len || (row.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MB_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = row;
        }
        self.len = self.len.saturating_add(1);
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                if r.ok {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len.saturating_sub(self.ok_count())
    }

    /// 对账：格数、成败计数、槽位编号、值序列、码序列。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(MB_EBAL);
        }
        const VALS: [u32; 8] = [2, 3, 4, 4, 1, 4, 5, 1];
        const CODES: [u32; 6] =
            [MB_EINVAL, MB_EBUSY, MB_ENAMETOOLONG, MB_EXDEV, MB_ENOENT, MB_ENOTEMPTY];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let r = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(MB_EBAL),
            };
            if r.slot as usize != i {
                return Err(MB_ESLOT);
            }
            if r.ok {
                if r.code != 0 {
                    return Err(MB_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if r.value != *w {
                            return Err(MB_EVALUE);
                        }
                    }
                    None => return Err(MB_EBAL),
                }
                vi = vi.saturating_add(1);
            } else {
                if r.value != 0 {
                    return Err(MB_EVALUE);
                }
                match CODES.get(ci) {
                    Some(w) => {
                        if r.code != *w {
                            return Err(MB_EVALUE);
                        }
                    }
                    None => return Err(MB_EBAL),
                }
                ci = ci.saturating_add(1);
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(MB_EBAL);
        }
        Ok(())
    }

    /// FNV-1a 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                h = fold(h, r.slot as u64);
                h = fold(h, r.op as u64);
                h = fold(h, r.ok as u64);
                h = fold(h, r.code as u64);
                h = fold(h, r.value as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MB_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放：重建账本并比对指纹。
    pub fn replay(&self) -> Result<u64, u32> {
        if !self.frozen {
            return Err(MB_ESEAL);
        }
        let fresh = build_ledger();
        let again = fresh.seal();
        if again == self.seal() && again == self.root {
            Ok(again)
        } else {
            Err(MB_ESEAL)
        }
    }

    /// 冻结指纹。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 十四格账本。
///
/// 布局：`mkdir 10`、`mkdir 20`、`mkdir 30`、把 `30` 移入 `10` 改名 `31`、
/// 在 `20` 上挂载设备 1、把 `10/31` 移回根改名 `30`、`mkdir 50`、
/// 在 `50` 下挂文件 `51`；随后空路径 `EINVAL`、搬挂载根 `EBUSY`、
/// 超长名 `ENAMETOOLONG`、移入挂载点 `EXDEV`、源缺失 `ENOENT`、
/// 覆盖非空目录 `ENOTEMPTY`。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut t = Tree::new(0);

    let _ = l.register(word_cell(0, OP_MKDIR, t.mkdir(ROOT, 10, 2, 0)));
    let _ = l.register(word_cell(1, OP_MKDIR, t.mkdir(2, 20, 3, 0)));
    let _ = l.register(word_cell(2, OP_MKDIR, t.mkdir(ROOT, 30, 4, 0)));
    let _ = l.register(word_cell(3, OP_MOVE, t.move_ent(&[30], 2, 31)));
    let _ = l.register(word_cell(4, OP_MOUNT, t.mount(3, 1)));
    let _ = l.register(word_cell(5, OP_MOVE, t.move_ent(&[10, 31], ROOT, 30)));
    let _ = l.register(word_cell(6, OP_MKDIR, t.mkdir(ROOT, 50, 5, 0)));
    let _ = l.register(word_cell(7, OP_LINK, t.link(5, 51, 9)));

    let _ = l.register(word_cell(8, OP_MOVE, t.move_ent(&[], ROOT, 30)));
    let _ = l.register(word_cell(9, OP_MOVE, t.move_ent(&[10, 20], ROOT, 25)));
    let _ = l.register(word_cell(10, OP_MKDIR, t.mkdir(ROOT, 999, 6, 0)));
    let _ = l.register(word_cell(11, OP_MOVE, t.move_ent(&[30], 3, 40)));
    let _ = l.register(word_cell(12, OP_MOVE, t.move_ent(&[77], ROOT, 78)));
    let _ = l.register(word_cell(13, OP_MOVE, t.move_ent(&[30], ROOT, 50)));

    l
}

/// 四路注入：账本身、挂载边界、同设备子树搬移、闸门次序。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let mut f = l;
    let frozen = f.freeze();
    out[0] = l.len() == SAMPLES
        && l.ok_count() == 8
        && l.err_count() == 6
        && l.audit().is_ok()
        && frozen.is_ok()
        && f.is_frozen()
        && f.root() != 0
        && f.replay() == Ok(f.root())
        && f.seal() == f.root();

    let mut a = Tree::new(0);
    let mut ok = a.mkdir(ROOT, 10, 2, 0).is_ok()
        && a.mkdir(2, 20, 3, 0).is_ok()
        && a.mount(3, 1).is_ok();
    ok = ok && a.move_ent(&[10, 20], ROOT, 25) == Err(MB_EBUSY);
    ok = ok && a.mkdir(ROOT, 30, 4, 0).is_ok();
    ok = ok && a.move_ent(&[30], 3, 40) == Err(MB_EXDEV);
    ok = ok && a.resolve(&[10, 20]) == Ok(3);
    ok = ok && a.flags(3) == Ok(3);
    out[1] = ok;

    let mut b = Tree::new(0);
    let mut ok2 = b.mkdir(ROOT, 10, 2, 0).is_ok()
        && b.mkdir(2, 40, 5, 0).is_ok()
        && b.mkdir(ROOT, 30, 4, 0).is_ok();
    ok2 = ok2 && b.move_ent(&[10, 40], 4, 41) == Ok(5);
    ok2 = ok2 && b.resolve(&[30, 41]) == Ok(5);
    ok2 = ok2 && b.parent_of(5) == Ok(4);
    ok2 = ok2 && b.count_children(2) == Ok(0);
    ok2 = ok2 && b.count_children(4) == Ok(1);
    out[2] = ok2;

    let mut c = Tree::new(0);
    let mut ok3 = c.mkdir(ROOT, 10, 2, 0).is_ok()
        && c.mkdir(2, 20, 3, 0).is_ok()
        && c.mount(3, 4).is_ok()
        && c.mkdir(ROOT, 30, 4, 0).is_ok();
    ok3 = ok3 && c.move_ent(&[], 0xffff, 0) == Err(MB_EINVAL);
    ok3 = ok3 && c.move_ent(&[30], 0xffff, 999) == Err(MB_ENAMETOOLONG);
    ok3 = ok3 && c.move_ent(&[10, 20], ROOT, 21) == Err(MB_EBUSY);
    ok3 = ok3 && c.move_ent(&[30], 3, 31) == Err(MB_EXDEV);
    out[3] = ok3;

    out
}

/// 十三项检查。
pub fn run_unx11410_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11410");
    let l = build_ledger();

    s.add(
        "MB1-台账格数与审计",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        "static",
    );

    s.add(
        "MB2-八正例值序",
        {
            const VALS: [u32; 8] = [2, 3, 4, 4, 1, 4, 5, 1];
            let mut good = true;
            let mut vi = 0usize;
            let mut i = 0usize;
            while i < SAMPLES {
                match l.sample(i) {
                    Some(r) => {
                        if r.ok {
                            if VALS.get(vi) != Some(&r.value) {
                                good = false;
                            }
                            vi = vi.saturating_add(1);
                        }
                    }
                    None => good = false,
                }
                i += 1;
            }
            good && vi == 8
        },
        "static",
    );

    s.add(
        "MB3-六错误码序",
        {
            const CODES: [u32; 6] =
                [MB_EINVAL, MB_EBUSY, MB_ENAMETOOLONG, MB_EXDEV, MB_ENOENT, MB_ENOTEMPTY];
            let mut good = true;
            let mut ci = 0usize;
            let mut i = 0usize;
            while i < SAMPLES {
                match l.sample(i) {
                    Some(r) => {
                        if !r.ok {
                            if CODES.get(ci) != Some(&r.code) {
                                good = false;
                            }
                            ci = ci.saturating_add(1);
                        }
                    }
                    None => good = false,
                }
                i += 1;
            }
            good && ci == 6
        },
        "static",
    );

    s.add(
        "MB4-挂载根拒绝搬移",
        {
            let mut t = Tree::new(0);
            let built = t.mkdir(ROOT, 10, 2, 0).is_ok()
                && t.mkdir(2, 20, 3, 0).is_ok()
                && t.mount(3, 5).is_ok();
            let out = t.move_ent(&[10, 20], ROOT, 25);
            let inner = t.move_ent(&[10, 20], 2, 25);
            let _ = t.move_ent(&[10, 20], 2, 26);
            built
                && out == Err(MB_EBUSY)
                && inner == Err(MB_EBUSY)
                && t.lookup(2, 20) == Ok(3)
                && t.flags(3) == Ok(3)
        },
        "static",
    );

    s.add(
        "MB5-跨设备边界EXDEV",
        {
            let mut t = Tree::new(0);
            let built = t.mkdir(ROOT, 10, 2, 0).is_ok()
                && t.mkdir(2, 20, 3, 0).is_ok()
                && t.mount(3, 7).is_ok()
                && t.mkdir(ROOT, 30, 4, 0).is_ok();
            let cross = t.move_ent(&[30], 3, 40);
            let inside = t.mkdir(3, 50, 6, 7);
            let back = t.move_ent(&[10, 20, 50], ROOT, 51);
            built
                && cross == Err(MB_EXDEV)
                && inside == Ok(6)
                && back == Err(MB_EXDEV)
                && t.resolve(&[10, 20]) == Ok(3)
        },
        "static",
    );

    s.add(
        "MB6-空路径与名字校验",
        {
            let mut t = Tree::new(0);
            let built = t.mkdir(ROOT, 10, 2, 0).is_ok();
            built
                && t.move_ent(&[], ROOT, 30) == Err(MB_EINVAL)
                && t.move_ent(&[], 0xffff, 0) == Err(MB_EINVAL)
                && t.mkdir(ROOT, 0, 3, 0) == Err(MB_EINVAL)
                && t.mkdir(ROOT, 999, 3, 0) == Err(MB_ENAMETOOLONG)
                && t.move_ent(&[10], ROOT, 0) == Err(MB_EINVAL)
                && t.move_ent(&[10], ROOT, 999) == Err(MB_ENAMETOOLONG)
                && t.move_ent(&[0], ROOT, 30) == Err(MB_EINVAL)
                && t.link(ROOT, 998, 4) == Err(MB_ENAMETOOLONG)
        },
        "static",
    );

    s.add(
        "MB7-缺失源与缺失目标",
        {
            let mut t = Tree::new(0);
            let built = t.mkdir(ROOT, 10, 2, 0).is_ok();
            built
                && t.move_ent(&[77], ROOT, 78) == Err(MB_ENOENT)
                && t.move_ent(&[10, 88], ROOT, 78) == Err(MB_ENOENT)
                && t.move_ent(&[10], 0xffff, 11) == Err(MB_ENOENT)
                && t.move_ent(&[10], 0, 11) == Err(MB_ENOENT)
                && t.mount(0xffff, 1) == Err(MB_ENOENT)
                && t.count_children(0xffff) == Err(MB_ENOENT)
        },
        "static",
    );

    s.add(
        "MB8-非空目标与文件目标",
        {
            let mut t = Tree::new(0);
            let built = t.mkdir(ROOT, 10, 2, 0).is_ok()
                && t.mkdir(ROOT, 30, 4, 0).is_ok()
                && t.mkdir(ROOT, 50, 5, 0).is_ok()
                && t.link(5, 51, 9).is_ok();
            let ne = t.move_ent(&[30], ROOT, 50);
            let keep = t.lookup(ROOT, 30) == Ok(4);
            let to = t.move_ent(&[30], ROOT, 60);
            let file = t.link(ROOT, 61, 11);
            let nd = t.move_ent(&[60], ROOT, 61);
            built && ne == Err(MB_ENOTEMPTY) && keep && to == Ok(4) && file == Ok(1)
                && nd == Err(MB_ENOTDIR)
        },
        "static",
    );

    s.add(
        "MB9-同目录改名保持节点",
        {
            let mut t = Tree::new(0);
            let built = t.mkdir(ROOT, 10, 2, 0).is_ok()
                && t.mkdir(ROOT, 30, 4, 0).is_ok()
                && t.mkdir(4, 40, 6, 0).is_ok();
            let ren = t.move_ent(&[30], ROOT, 31);
            let same = t.move_ent(&[31], ROOT, 31);
            built
                && ren == Ok(4)
                && same == Ok(4)
                && t.lookup(ROOT, 31) == Ok(4)
                && t.lookup(ROOT, 30) == Err(MB_ENOENT)
                && t.parent_of(6) == Ok(4)
                && t.count_children(4) == Ok(1)
                && t.count_children(ROOT) == Ok(2)
                && t.consistent()
        },
        "static",
    );

    s.add(
        "MB10-挂载子树内部移动",
        {
            let mut t = Tree::new(0);
            let built = t.mkdir(ROOT, 10, 2, 0).is_ok()
                && t.mkdir(2, 20, 3, 0).is_ok()
                && t.mount(3, 1).is_ok()
                && t.mkdir(3, 60, 7, 1).is_ok()
                && t.mkdir(7, 61, 8, 1).is_ok();
            let ren = t.move_ent(&[10, 20, 60, 61], 7, 62);
            built
                && ren == Ok(8)
                && t.resolve(&[10, 20, 60, 62]) == Ok(8)
                && t.count_children(7) == Ok(1)
                && t.parent_of(8) == Ok(7)
                && t.consistent()
        },
        "static",
    );

    let (mb11_ok, mb11_flags, mb11_kids) = {
        let mut t = Tree::new(0);
        let built = t.mkdir(ROOT, 10, 2, 0).is_ok() && t.mkdir(2, 20, 3, 0).is_ok();
        let before = t.count_children(3);
        let m = t.mount(3, 2);
        let kids = t.count_children(3);
        let fl = t.flags(3);
        let pack = byte_cell(0, OP_QUERY, fl);
        let up = t.resolve(&[10, 20]);
        let mut u = Tree::new(0);
        let bad = u.mkdir(ROOT, 10, 2, 0).is_ok()
            && u.mkdir(2, 20, 3, 0).is_ok()
            && u.mkdir(3, 30, 4, 0).is_ok()
            && u.mount(3, 1) == Err(MB_ENOTEMPTY);
        let good = built
            && before == Ok(0)
            && m == Ok(2)
            && kids == Ok(0)
            && fl == Ok(3)
            && pack.ok
            && pack.value == 3
            && up == Ok(3)
            && t.consistent()
            && bad;
        (good, fl.unwrap_or(0), kids.unwrap_or(0xffff_ffff))
    };
    s.add_dyn(
        "MB11-挂载标志与子项计数",
        mb11_ok,
        &format!("flags={} kids={}", mb11_flags, mb11_kids),
    );

    s.add(
        "MB12-门序优先",
        {
            let mut t = Tree::new(0);
            let built = t.mkdir(ROOT, 10, 2, 0).is_ok()
                && t.mkdir(2, 20, 3, 0).is_ok()
                && t.mount(3, 6).is_ok()
                && t.mkdir(ROOT, 30, 4, 0).is_ok();
            built
                && t.move_ent(&[], 0xffff, 30) == Err(MB_EINVAL)
                && t.move_ent(&[30], 0xffff, 999) == Err(MB_ENAMETOOLONG)
                && t.move_ent(&[30], 0xffff, 0) == Err(MB_EINVAL)
                && t.move_ent(&[10, 20], 2, 21) == Err(MB_EBUSY)
                && t.move_ent(&[30], 2, 20) == Err(MB_EBUSY)
                && t.move_ent(&[30], 3, 31) == Err(MB_EXDEV)
                && t.lookup(ROOT, 30) == Ok(4)
        },
        "static",
    );

    s.add(
        "MB13-冻结重放四注入",
        {
            let mut f = l;
            let sealed = f.freeze();
            let inj = inject_four();
            sealed.is_ok()
                && f.is_frozen()
                && f.root() != 0
                && f.replay() == Ok(f.root())
                && inj[0]
                && inj[1]
                && inj[2]
                && inj[3]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
        assert!(!l.is_empty());
        let mut f = l;
        let seal = f.freeze().unwrap();
        assert_ne!(seal, 0);
        assert!(f.is_frozen());
        assert_eq!(f.replay(), Ok(seal));
        assert_eq!(f.rejects(), 0);
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11410_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn mount_is_immovable() {
        let mut t = Tree::new(0);
        assert_eq!(t.mkdir(ROOT, 10, 2, 0), Ok(2));
        assert_eq!(t.mkdir(2, 20, 3, 0), Ok(3));
        assert_eq!(t.mount(3, 4), Ok(4));
        assert_eq!(t.move_ent(&[10, 20], ROOT, 21), Err(MB_EBUSY));
        assert_eq!(t.mkdir(ROOT, 30, 5, 0), Ok(5));
        assert_eq!(t.move_ent(&[30], 5, 31), Err(MB_EINVAL));
        assert_eq!(t.lookup(2, 20), Ok(3));
        assert_eq!(t.flags(3), Ok(3));
        assert_eq!(t.umount(3), Ok(0));
        assert_eq!(t.flags(3), Ok(1));
    }

    #[test]
    fn mount_boundary_exdev() {
        let mut t = Tree::new(0);
        assert_eq!(t.mkdir(ROOT, 10, 2, 0), Ok(2));
        assert_eq!(t.mkdir(2, 20, 3, 0), Ok(3));
        assert_eq!(t.mkdir(ROOT, 30, 4, 0), Ok(4));
        assert_eq!(t.mount(3, 9), Ok(9));
        assert_eq!(t.move_ent(&[30], 3, 31), Err(MB_EXDEV));
        assert_eq!(t.mkdir(3, 40, 5, 9), Ok(5));
        assert_eq!(t.move_ent(&[10, 20, 40], ROOT, 41), Err(MB_EXDEV));
        assert_eq!(t.resolve(&[10, 20, 40]), Ok(5));
    }

    #[test]
    fn same_device_move() {
        let mut t = Tree::new(0);
        assert_eq!(t.mkdir(ROOT, 10, 2, 0), Ok(2));
        assert_eq!(t.mkdir(2, 20, 3, 0), Ok(3));
        assert_eq!(t.mkdir(ROOT, 30, 4, 0), Ok(4));
        assert_eq!(t.move_ent(&[10, 20], 4, 21), Ok(3));
        assert_eq!(t.parent_of(3), Ok(4));
        assert_eq!(t.lookup(4, 21), Ok(3));
        assert_eq!(t.count_children(2), Ok(0));
        assert_eq!(t.count_children(4), Ok(1));
        assert_eq!(t.resolve(&[30, 21]), Ok(3));
        assert!(t.consistent());
    }

    #[test]
    fn name_and_path_gates() {
        let mut t = Tree::new(0);
        assert_eq!(t.mkdir(ROOT, 10, 2, 0), Ok(2));
        assert_eq!(t.move_ent(&[], ROOT, 11), Err(MB_EINVAL));
        assert_eq!(t.move_ent(&[10], ROOT, 0), Err(MB_EINVAL));
        assert_eq!(t.move_ent(&[10], ROOT, 999), Err(MB_ENAMETOOLONG));
        assert_eq!(t.move_ent(&[999], ROOT, 11), Err(MB_ENAMETOOLONG));
        assert_eq!(t.move_ent(&[77], ROOT, 11), Err(MB_ENOENT));
        assert_eq!(t.mkdir(ROOT, 0, 3, 0), Err(MB_EINVAL));
        assert_eq!(t.mkdir(ROOT, 999, 3, 0), Err(MB_ENAMETOOLONG));
        assert_eq!(t.mkdir(0xffff, 11, 3, 0), Err(MB_ENOENT));
    }

    #[test]
    fn nonempty_and_type_gates() {
        let mut t = Tree::new(0);
        assert_eq!(t.mkdir(ROOT, 10, 2, 0), Ok(2));
        assert_eq!(t.mkdir(ROOT, 30, 4, 0), Ok(4));
        assert_eq!(t.mkdir(ROOT, 50, 5, 0), Ok(5));
        assert_eq!(t.link(5, 51, 9), Ok(1));
        assert_eq!(t.move_ent(&[30], ROOT, 50), Err(MB_ENOTEMPTY));
        assert_eq!(t.link(ROOT, 60, 11), Ok(1));
        assert_eq!(t.move_ent(&[30], ROOT, 60), Err(MB_ENOTDIR));
        assert_eq!(t.move_ent(&[30], ROOT, 61), Ok(4));
        assert_eq!(t.mkdir(ROOT, 70, 6, 5), Ok(6));
        assert_eq!(t.mount(6, 5), Ok(5));
        assert_eq!(t.move_ent(&[10], 6, 11), Err(MB_EXDEV));
        assert_eq!(t.move_ent(&[50], 5, 52), Err(MB_EINVAL));
    }

    #[test]
    fn inject_four_holds() {
        let inj = inject_four();
        assert_eq!(inj, [true, true, true, true]);
    }
}
