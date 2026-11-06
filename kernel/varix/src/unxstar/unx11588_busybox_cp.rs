//! UNX-F11588 busybox cp 语义对照判据
//!
//! busybox 的 `cp` 与 GNU coreutils 的 `cp` 共享同一组短选项，但对**目标形态**
//! 的裁决并不相同。本模块不描述“命令行怎么拼”，而是把 `cp` 拆成四层可判定
//! 的模型：
//!
//! 1. **复制模式表**：递归、保留权限、保留时间、不覆盖、稀疏、解引用、归档、
//!    强制、交互、硬链接、仅更新，共十一位，落在 `u16` 的低十一位里。归档
//!    是一位**宏位**：置位时展开成“递归 + 保留权限 + 保留时间 + 不解引用”，
//!    并在有效标志里抹掉自身；有效标志才是计划真正执行的那一份。
//! 2. **源/目标形状表**：源分常规文件、目录、符号链接、FIFO 四类，目标分
//!    不存在、常规文件、目录三类，共十二格。`目录 → 文件` 是唯一的非法格，
//!    必须在递归判定之前就被拒绝——否则 `cp -r d f` 会先报“缺递归”再报
//!    “形态非法”，错误码与 GNU 对不上。
//! 3. **冲突裁决**：强制 > 交互 > 不覆盖 > 仅更新 > 默认，五档优先级固定。
//!    `强制` 与 `不覆盖` 同时出现是自相矛盾的组合，直接判标志冲突，而不是
//!    让优先级悄悄决定结果。
//! 4. **路径折算**：目标目录拼接 basename 时处理重复斜杠、绝对名、`..` 穿越
//!    与长度上限；basename 本身要剥掉尾部斜杠，且 `/` 没有 basename。
//!
//! 符号链接策略单独成闸：默认解引用；归档模式不解引用；显式解引用与归档
//! 同时出现判策略冲突（`ELINK`）；硬链接模式与解引用、稀疏、递归同时出现
//! 同样判冲突。保留权限/时间落在未解引用的符号链接上无法落地（`EMODE`），
//! 稀疏复制落在非常规文件上无法成立（`ESPARE`）。
//!
//! `-u`（仅更新）不是错误路径：当目标已存在且目标 mtime 不早于源 mtime 时，
//! 计划降级为“跳过”，动作码 `ACT_SKIP`，账本第七格就抽查这条降级。
//!
//! 闸门次序：`judge`：未知标志 → 互斥标志 → 归档/解引用冲突 → 形状越界 →
//! 目录形态 → 目录递归 → 特殊文件 → 不覆盖 → 链接策略 → 保留 → 稀疏 →
//! 动作归并；`path_join`：空串 → 绝对名 → 穿越 → 长度；`basename`：空串 →
//! 尾斜杠剥离 → 无组件。
//!
//! 账本十四格：八格成功（file→dir 追加、dir→dir 递归、归档展开、强制冲突
//! 裁决、路径拼接、basename、形状表、仅更新跳过），六格失败（目录→文件、
//! 缺递归、不覆盖命中、链接策略冲突、未知标志、稀疏用于链接）。冻结票号
//! 折叠十四格的五字段与拒绝计数。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfa10;

/// 路径为空、绝对名、穿越或超长。
pub const CP_EPATH: u32 = SEG_BASE + 0x01;
/// 源/目标形状非法（目录 → 文件、未知类型）。
pub const CP_ETYPE: u32 = SEG_BASE + 0x02;
/// 目录源缺递归。
pub const CP_EREC: u32 = SEG_BASE + 0x03;
/// 不覆盖命中已存在目标。
pub const CP_EEXIST: u32 = SEG_BASE + 0x04;
/// 符号链接策略冲突。
pub const CP_ELINK: u32 = SEG_BASE + 0x05;
/// 保留权限/时间无法落地。
pub const CP_EMODE: u32 = SEG_BASE + 0x06;
/// 稀疏复制用于非常规文件。
pub const CP_ESPARE: u32 = SEG_BASE + 0x07;
/// 未知标志位或互斥标志组合。
pub const CP_EFLAG: u32 = SEG_BASE + 0x08;
/// 账本形状不符（14/8/6）。
pub const CP_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序或饱和拒收。
pub const CP_ESLOT: u32 = SEG_BASE + 0x0a;
/// 复算值与登记值不符。
pub const CP_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放票号不一致。
pub const CP_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const CP_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 源：常规文件。
pub const K_REG: u8 = 0;
/// 源：目录。
pub const K_DIR: u8 = 1;
/// 源：符号链接。
pub const K_LINK: u8 = 2;
/// 源：FIFO。
pub const K_FIFO: u8 = 3;

/// 目标：不存在。
pub const D_NONE: u8 = 0;
/// 目标：常规文件。
pub const D_FILE: u8 = 1;
/// 目标：目录。
pub const D_DIR: u8 = 2;

/// 模式位：递归。
pub const F_RECURSIVE: u16 = 0x0001;
/// 模式位：保留权限。
pub const F_PRESERVE_MODE: u16 = 0x0002;
/// 模式位：保留时间。
pub const F_PRESERVE_TIME: u16 = 0x0004;
/// 模式位：不覆盖。
pub const F_NO_CLOBBER: u16 = 0x0008;
/// 模式位：稀疏。
pub const F_SPARSE: u16 = 0x0010;
/// 模式位：解引用符号链接。
pub const F_DEREF: u16 = 0x0020;
/// 模式位：归档。
pub const F_ARCHIVE: u16 = 0x0040;
/// 模式位：强制。
pub const F_FORCE: u16 = 0x0080;
/// 模式位：交互。
pub const F_INTERACTIVE: u16 = 0x0100;
/// 模式位：硬链接代替复制。
pub const F_LINK: u16 = 0x0200;
/// 模式位：仅更新。
pub const F_UPDATE: u16 = 0x0400;

/// 合法标志掩码（低十一位）。
pub const MASK: u16 = 0x07ff;

/// 动作：复制到文件。
pub const ACT_COPY: u8 = 0;
/// 动作：复制进目录（追加 basename）。
pub const ACT_COPY_INTO: u8 = 1;
/// 动作：递归复制目录。
pub const ACT_RECURSE: u8 = 2;
/// 动作：硬链接。
pub const ACT_LINK: u8 = 3;
/// 动作：跳过（仅更新降级）。
pub const ACT_SKIP: u8 = 4;

/// 冲突裁决：覆盖。
pub const DEC_OVERWRITE: u8 = 0;
/// 冲突裁决：询问。
pub const DEC_ASK: u8 = 1;
/// 冲突裁决：跳过。
pub const DEC_SKIP: u8 = 2;
/// 冲突裁决：直接复制（目标不存在）。
pub const DEC_COPY: u8 = 3;

/// 形状：常规 → 文件/不存在。
pub const SHAPE_F2F: u8 = 0;
/// 形状：常规 → 目录。
pub const SHAPE_F2D: u8 = 1;
/// 形状：目录 → 目录/不存在。
pub const SHAPE_D2D: u8 = 2;
/// 形状：目录 → 文件（非法）。
pub const SHAPE_D2F: u8 = 3;
/// 形状：链接 → 目录。
pub const SHAPE_L2D: u8 = 4;
/// 形状：链接 → 文件/不存在。
pub const SHAPE_L2F: u8 = 5;
/// 形状：FIFO → 目录。
pub const SHAPE_S2D: u8 = 6;
/// 形状：FIFO → 文件/不存在。
pub const SHAPE_S2F: u8 = 7;

/// 操作码：判定。
pub const OP_JUDGE: u8 = 1;
/// 操作码：冲突裁决。
pub const OP_CONFLICT: u8 = 2;
/// 操作码：路径拼接。
pub const OP_PATH: u8 = 3;
/// 操作码：basename。
pub const OP_BASE: u8 = 4;
/// 操作码：形状表。
pub const OP_SHAPE: u8 = 5;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个 64 位字（每字节一次乘法，共八次）。
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

/// 标志位是否置位。
pub fn has(flags: u16, bit: u16) -> bool {
    flags & bit != 0
}

/// 有效标志：展开归档宏位并抹掉自身与显式解引用。
pub fn effective(flags: u16) -> u16 {
    let mut e = flags & MASK;
    if has(e, F_ARCHIVE) {
        e |= F_RECURSIVE | F_PRESERVE_MODE | F_PRESERVE_TIME;
        e &= !F_ARCHIVE;
        e &= !F_DEREF;
    }
    e
}

/// 链接是否被解引用：显式解引用，或既非归档也非硬链接模式（默认解引用）。
pub fn follows(flags: u16) -> bool {
    if has(flags, F_DEREF) {
        return true;
    }
    !has(flags, F_ARCHIVE) && !has(flags, F_LINK)
}

/// 一次复制请求（含两端 mtime，供仅更新判定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    /// 源类型。
    pub src_kind: u8,
    /// 目标形态。
    pub dst_kind: u8,
    /// 标志位。
    pub flags: u16,
    /// 源 mtime。
    pub src_mtime: u32,
    /// 目标 mtime（目标不存在时为 0）。
    pub dst_mtime: u32,
}

impl Request {
    /// 空请求。
    pub const fn none() -> Request {
        Request { src_kind: K_REG, dst_kind: D_NONE, flags: 0, src_mtime: 0, dst_mtime: 0 }
    }

    /// 构造请求。
    pub const fn make(src_kind: u8, dst_kind: u8, flags: u16) -> Request {
        Request { src_kind, dst_kind, flags, src_mtime: 0, dst_mtime: 0 }
    }

    /// 追加 mtime。
    pub const fn times(mut self, src_mtime: u32, dst_mtime: u32) -> Request {
        self.src_mtime = src_mtime;
        self.dst_mtime = dst_mtime;
        self
    }
}

/// 判定后的复制计划。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    /// 有效标志。
    pub flags: u16,
    /// 动作码。
    pub action: u8,
    /// 目标为目录时是否追加 basename。
    pub append_name: bool,
    /// 是否解引用源链接。
    pub follow_link: bool,
    /// 是否保留权限。
    pub mode_kept: bool,
    /// 是否保留时间。
    pub time_kept: bool,
    /// 是否稀疏写。
    pub sparse: bool,
}

impl Plan {
    /// 空计划。
    pub const fn empty() -> Plan {
        Plan {
            flags: 0,
            action: ACT_COPY,
            append_name: false,
            follow_link: false,
            mode_kept: false,
            time_kept: false,
            sparse: false,
        }
    }

    /// 有效标志的位计数。
    pub fn mode_bits(&self) -> u32 {
        (self.flags & MASK).count_ones()
    }
}

/// 形状表：源 × 目标 → 形状码；唯一的非法格是目录 → 文件。
pub fn shape_of(src: u8, dst: u8) -> Result<u8, u32> {
    if src > K_FIFO || dst > D_DIR {
        return Err(CP_ETYPE);
    }
    if src == K_DIR && dst == D_FILE {
        return Err(CP_ETYPE);
    }
    match src {
        K_REG => {
            if dst == D_DIR {
                Ok(SHAPE_F2D)
            } else {
                Ok(SHAPE_F2F)
            }
        }
        K_DIR => Ok(SHAPE_D2D),
        K_LINK => {
            if dst == D_DIR {
                Ok(SHAPE_L2D)
            } else {
                Ok(SHAPE_L2F)
            }
        }
        K_FIFO => {
            if dst == D_DIR {
                Ok(SHAPE_S2D)
            } else {
                Ok(SHAPE_S2F)
            }
        }
        _ => Err(CP_ETYPE),
    }
}

/// 形状码的中文标签。
pub fn shape_label(code: u8) -> Result<&'static str, u32> {
    match code {
        SHAPE_F2F => Ok("常规→文件"),
        SHAPE_F2D => Ok("常规→目录"),
        SHAPE_D2D => Ok("目录→目录"),
        SHAPE_D2F => Ok("目录→文件"),
        SHAPE_L2D => Ok("链接→目录"),
        SHAPE_L2F => Ok("链接→文件"),
        SHAPE_S2D => Ok("FIFO→目录"),
        SHAPE_S2F => Ok("FIFO→文件"),
        _ => Err(CP_ETYPE),
    }
}

/// 冲突优先级：强制 0、交互 1、不覆盖 2、仅更新 3、默认 4。
pub fn conflict_rank(flags: u16) -> Result<u8, u32> {
    if flags & !MASK != 0 {
        return Err(CP_EFLAG);
    }
    if has(flags, F_FORCE) && has(flags, F_NO_CLOBBER) {
        return Err(CP_EFLAG);
    }
    if has(flags, F_FORCE) {
        return Ok(0);
    }
    if has(flags, F_INTERACTIVE) {
        return Ok(1);
    }
    if has(flags, F_NO_CLOBBER) {
        return Ok(2);
    }
    if has(flags, F_UPDATE) {
        return Ok(3);
    }
    Ok(4)
}

/// 冲突裁决：目标存在时按优先级落档，不存在时直接复制。
pub fn resolve_conflict(flags: u16, dst_exists: bool) -> Result<u8, u32> {
    let rank = conflict_rank(flags)?;
    if !dst_exists {
        return Ok(DEC_COPY);
    }
    match rank {
        0 => Ok(DEC_OVERWRITE),
        1 => Ok(DEC_ASK),
        2 => Ok(DEC_SKIP),
        3 => Ok(DEC_SKIP),
        _ => Ok(DEC_OVERWRITE),
    }
}

/// 判定一次复制：返回计划或拒绝码。
pub fn judge(req: Request) -> Result<Plan, u32> {
    let f = req.flags;
    if f & !MASK != 0 {
        return Err(CP_EFLAG);
    }
    if has(f, F_NO_CLOBBER) && has(f, F_FORCE) {
        return Err(CP_EFLAG);
    }
    if has(f, F_LINK)
        && (has(f, F_SPARSE) || has(f, F_RECURSIVE) || has(f, F_ARCHIVE) || has(f, F_DEREF))
    {
        return Err(CP_EFLAG);
    }
    if has(f, F_ARCHIVE) && has(f, F_DEREF) {
        return Err(CP_ELINK);
    }
    if req.src_kind > K_FIFO || req.dst_kind > D_DIR {
        return Err(CP_ETYPE);
    }
    if req.src_kind == K_DIR {
        if req.dst_kind == D_FILE {
            return Err(CP_ETYPE);
        }
        if !has(f, F_RECURSIVE) && !has(f, F_ARCHIVE) {
            return Err(CP_EREC);
        }
    }
    if req.src_kind == K_FIFO && !has(f, F_ARCHIVE) {
        return Err(CP_ETYPE);
    }
    if has(f, F_NO_CLOBBER) && req.dst_kind == D_FILE && req.src_kind != K_DIR {
        return Err(CP_EEXIST);
    }
    if req.src_kind == K_LINK && has(f, F_LINK) {
        return Err(CP_ELINK);
    }
    let follow = follows(f);
    if has(f, F_LINK) && (has(f, F_PRESERVE_MODE) || has(f, F_PRESERVE_TIME)) {
        return Err(CP_EMODE);
    }
    if has(f, F_SPARSE) && req.src_kind != K_REG {
        return Err(CP_ESPARE);
    }

    let eff = effective(f);
    let mut action = if has(eff, F_LINK) { ACT_LINK } else { ACT_COPY };
    if req.src_kind == K_DIR {
        action = ACT_RECURSE;
    } else if req.dst_kind == D_DIR {
        action = ACT_COPY_INTO;
    }
    if has(f, F_UPDATE) && req.dst_kind != D_NONE && req.dst_mtime >= req.src_mtime {
        action = ACT_SKIP;
    }
    Ok(Plan {
        flags: eff,
        action,
        append_name: req.dst_kind == D_DIR,
        follow_link: req.src_kind == K_LINK && follow,
        mode_kept: has(eff, F_PRESERVE_MODE),
        time_kept: has(eff, F_PRESERVE_TIME),
        sparse: has(eff, F_SPARSE),
    })
}

/// 目标目录拼接基名：拒绝空串、绝对名、`..` 穿越与超长。
pub fn path_join(dst: &str, name: &str, cap: usize) -> Result<String, u32> {
    if dst.is_empty() || name.is_empty() || cap == 0 {
        return Err(CP_EPATH);
    }
    if name.as_bytes().first() == Some(&b'/') {
        return Err(CP_EPATH);
    }
    if name == ".." || name == "." {
        return Err(CP_EPATH);
    }
    if name.starts_with("../") || name.ends_with("/..") {
        return Err(CP_EPATH);
    }
    let bytes = name.as_bytes();
    let mut i = 0usize;
    while i + 2 < bytes.len() {
        if let (Some(a), Some(b), Some(c)) = (bytes.get(i), bytes.get(i + 1), bytes.get(i + 2)) {
            if *a == b'/' && *b == b'.' && *c == b'.' {
                return Err(CP_EPATH);
            }
        }
        i += 1;
    }
    let mut out = String::new();
    out.push_str(dst);
    let ends_slash = match dst.as_bytes().last() {
        Some(b) => *b == b'/',
        None => false,
    };
    if !ends_slash {
        out.push('/');
    }
    out.push_str(name);
    if out.len() > cap {
        return Err(CP_EPATH);
    }
    Ok(out)
}

/// 取路径最后一个组件：剥掉尾部斜杠；`/` 与空串没有基名。
pub fn basename(path: &str) -> Result<&str, u32> {
    let bytes = path.as_bytes();
    if bytes.is_empty() {
        return Err(CP_EPATH);
    }
    let mut end = bytes.len();
    while end > 0 {
        match bytes.get(end - 1) {
            Some(c) => {
                if *c == b'/' {
                    end -= 1;
                } else {
                    break;
                }
            }
            None => break,
        }
    }
    if end == 0 {
        return Err(CP_EPATH);
    }
    let mut start = end;
    while start > 0 {
        match bytes.get(start - 1) {
            Some(c) => {
                if *c == b'/' {
                    break;
                }
                start -= 1;
            }
            None => break,
        }
    }
    match path.get(start..end) {
        Some(s) => Ok(s),
        None => Err(CP_EPATH),
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

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
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

    /// 登记一格。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(CP_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len += 1;
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
            if let Some(s) = self.rows.get(i) {
                if s.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(CP_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 0, 9, 3, 1, 4];
        const CODES: [u32; 6] =
            [CP_ETYPE, CP_EREC, CP_EEXIST, CP_ELINK, CP_EFLAG, CP_ESPARE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(CP_EBAL),
            };
            if s.slot as usize != i {
                return Err(CP_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(CP_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(CP_EVALUE);
                        }
                    }
                    None => return Err(CP_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(CP_EVALUE);
                        }
                    }
                    None => return Err(CP_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(CP_EBAL);
        }
        Ok(())
    }

    /// 票号。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold(h, s.slot as u64);
                h = fold(h, s.op as u64);
                h = fold(h, s.ok as u64);
                h = fold(h, s.code as u64);
                h = fold(h, s.value as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(CP_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(CP_ESEAL)
        }
    }

    /// 冻结票号。
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

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(word_cell(
        0,
        OP_JUDGE,
        judge(Request::make(K_REG, D_DIR, 0)).map(|p| p.action as u32),
    ));
    let _ = l.register(word_cell(
        1,
        OP_JUDGE,
        judge(Request::make(K_DIR, D_DIR, F_RECURSIVE)).map(|p| p.action as u32),
    ));
    let _ = l.register(word_cell(
        2,
        OP_JUDGE,
        judge(Request::make(K_REG, D_NONE, F_ARCHIVE)).map(|p| p.mode_bits()),
    ));
    let _ = l.register(word_cell(
        3,
        OP_CONFLICT,
        resolve_conflict(F_FORCE, true).map(|v| v as u32),
    ));
    let _ = l.register(word_cell(
        4,
        OP_PATH,
        path_join("out", "a.txt", 255).map(|s| s.len() as u32),
    ));
    let _ = l.register(word_cell(
        5,
        OP_BASE,
        basename("/usr//lib/").map(|s| s.len() as u32),
    ));
    let _ = l.register(word_cell(6, OP_SHAPE, shape_of(K_REG, D_DIR).map(|v| v as u32)));
    let _ = l.register(word_cell(
        7,
        OP_JUDGE,
        judge(Request::make(K_REG, D_FILE, F_UPDATE).times(100, 200)).map(|p| p.action as u32),
    ));

    let _ = l.register(word_cell(
        8,
        OP_JUDGE,
        judge(Request::make(K_DIR, D_FILE, F_RECURSIVE)).map(|p| p.action as u32),
    ));
    let _ = l.register(word_cell(
        9,
        OP_JUDGE,
        judge(Request::make(K_DIR, D_NONE, 0)).map(|p| p.action as u32),
    ));
    let _ = l.register(word_cell(
        10,
        OP_JUDGE,
        judge(Request::make(K_REG, D_FILE, F_NO_CLOBBER)).map(|p| p.action as u32),
    ));
    let _ = l.register(word_cell(
        11,
        OP_JUDGE,
        judge(Request::make(K_REG, D_NONE, F_ARCHIVE | F_DEREF)).map(|p| p.action as u32),
    ));
    let _ = l.register(word_cell(
        12,
        OP_JUDGE,
        judge(Request::make(K_REG, D_NONE, 0x8000)).map(|p| p.action as u32),
    ));
    let _ = l.register(word_cell(
        13,
        OP_JUDGE,
        judge(Request::make(K_LINK, D_NONE, F_SPARSE)).map(|p| p.action as u32),
    ));

    l
}

/// 四路注入：标志与形状 / 判定动作 / 拒绝闸门 / 路径与冲突。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut valid = 0u32;
    let mut src = 0u8;
    while src <= K_FIFO {
        let mut dst = 0u8;
        while dst <= D_DIR {
            if shape_of(src, dst).is_ok() {
                valid += 1;
            }
            dst += 1;
        }
        src += 1;
    }
    out[0] = MASK == 0x07ff
        && conflict_rank(0) == Ok(4)
        && effective(0) == 0
        && effective(F_ARCHIVE) == (F_RECURSIVE | F_PRESERVE_MODE | F_PRESERVE_TIME)
        && valid == 11
        && shape_of(K_DIR, D_FILE) == Err(CP_ETYPE)
        && shape_of(K_DIR, D_NONE) == Ok(SHAPE_D2D)
        && shape_label(SHAPE_S2F) == Ok("FIFO→文件")
        && shape_label(9) == Err(CP_ETYPE);

    let f2d = judge(Request::make(K_REG, D_DIR, 0));
    let d2d = judge(Request::make(K_DIR, D_DIR, F_RECURSIVE));
    let arch = judge(Request::make(K_REG, D_NONE, F_ARCHIVE));
    let skip = judge(Request::make(K_REG, D_FILE, F_UPDATE).times(500, 500));
    let newer = judge(Request::make(K_REG, D_FILE, F_UPDATE).times(500, 100));
    let link = judge(Request::make(K_LINK, D_NONE, F_DEREF));
    let kept = judge(Request::make(K_LINK, D_NONE, F_ARCHIVE));
    out[1] = f2d == Ok(Plan {
        flags: 0,
        action: ACT_COPY_INTO,
        append_name: true,
        follow_link: false,
        mode_kept: false,
        time_kept: false,
        sparse: false,
    }) && d2d.map(|p| p.action) == Ok(ACT_RECURSE)
        && d2d.map(|p| p.append_name) == Ok(true)
        && arch.map(|p| p.mode_bits()) == Ok(3)
        && arch.map(|p| p.action) == Ok(ACT_COPY)
        && skip.map(|p| p.action) == Ok(ACT_SKIP)
        && newer.map(|p| p.action) == Ok(ACT_COPY)
        && link.map(|p| p.follow_link) == Ok(true)
        && kept.map(|p| p.follow_link) == Ok(false)
        && kept.map(|p| p.mode_kept) == Ok(true);

    let dir_file = judge(Request::make(K_DIR, D_FILE, F_RECURSIVE)) == Err(CP_ETYPE);
    let dir_norec = judge(Request::make(K_DIR, D_DIR, 0)) == Err(CP_EREC);
    let exist = judge(Request::make(K_REG, D_FILE, F_NO_CLOBBER)) == Err(CP_EEXIST);
    let link_conf = judge(Request::make(K_REG, D_NONE, F_ARCHIVE | F_DEREF)) == Err(CP_ELINK);
    let bad_flag = judge(Request::make(K_REG, D_NONE, 0x8000)) == Err(CP_EFLAG);
    let sparse = judge(Request::make(K_LINK, D_NONE, F_SPARSE)) == Err(CP_ESPARE);
    let mode = judge(Request::make(K_REG, D_NONE, F_LINK | F_PRESERVE_MODE)) == Err(CP_EMODE);
    let fifo = judge(Request::make(K_FIFO, D_NONE, 0)) == Err(CP_ETYPE);
    let both = judge(Request::make(K_REG, D_NONE, F_FORCE | F_NO_CLOBBER)) == Err(CP_EFLAG);
    out[2] = dir_file && dir_norec && exist && link_conf && bad_flag && sparse && mode && fifo && both;

    let join = path_join("out", "a.txt", 255) == Ok(String::from("out/a.txt"));
    let trailing = path_join("out/", "b", 255) == Ok(String::from("out/b"));
    let escape = path_join("out", "../etc", 255) == Err(CP_EPATH);
    let abs = path_join("out", "/etc/passwd", 255) == Err(CP_EPATH);
    let long = path_join("out", "abcdef", 8) == Err(CP_EPATH);
    let base = basename("/usr//lib/") == Ok("lib") && basename("a") == Ok("a");
    let root = basename("/") == Err(CP_EPATH) && basename("") == Err(CP_EPATH);
    let prec = conflict_rank(F_FORCE) == Ok(0)
        && conflict_rank(F_INTERACTIVE | F_UPDATE) == Ok(1)
        && conflict_rank(F_NO_CLOBBER) == Ok(2)
        && conflict_rank(F_UPDATE) == Ok(3)
        && conflict_rank(F_FORCE | F_NO_CLOBBER) == Err(CP_EFLAG);
    let decisions = resolve_conflict(F_FORCE, true) == Ok(DEC_OVERWRITE)
        && resolve_conflict(F_INTERACTIVE, true) == Ok(DEC_ASK)
        && resolve_conflict(F_NO_CLOBBER, true) == Ok(DEC_SKIP)
        && resolve_conflict(0, false) == Ok(DEC_COPY)
        && resolve_conflict(0, true) == Ok(DEC_OVERWRITE);
    out[3] = join && trailing && escape && abs && long && base && root && prec && decisions;

    out
}

/// 十三项判据。
pub fn run_unx11588_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11588.busybox_cp");
    let l = build_ledger();

    s.add(
        "CP1-十一位模式空间",
        l.len() == SAMPLES
            && l.audit().is_ok()
            && MASK == 0x07ff
            && MASK.count_ones() == 11
            && judge(Request::none()).is_ok()
            && conflict_rank(0x8000) == Err(CP_EFLAG)
            && judge(Request::make(K_REG, D_NONE, 0x8000)) == Err(CP_EFLAG),
        "static",
    );

    s.add(
        "CP2-十二格形状表",
        {
            let mut valid = 0u32;
            let mut src = 0u8;
            while src <= K_FIFO {
                let mut dst = 0u8;
                while dst <= D_DIR {
                    if shape_of(src, dst).is_ok() {
                        valid += 1;
                    }
                    dst += 1;
                }
                src += 1;
            }
            valid == 11
                && shape_of(K_DIR, D_FILE) == Err(CP_ETYPE)
                && shape_of(K_REG, D_FILE) == Ok(SHAPE_F2F)
                && shape_of(K_FIFO, D_DIR) == Ok(SHAPE_S2D)
                && shape_of(9, D_DIR) == Err(CP_ETYPE)
        },
        "static",
    );

    s.add(
        "CP3-文件进目录追加基名",
        judge(Request::make(K_REG, D_DIR, 0)) == Ok(Plan {
            flags: 0,
            action: ACT_COPY_INTO,
            append_name: true,
            follow_link: false,
            mode_kept: false,
            time_kept: false,
            sparse: false,
        }) && judge(Request::make(K_REG, D_FILE, 0)).map(|p| p.append_name) == Ok(false),
        "static",
    );

    s.add(
        "CP4-目录递归双闸",
        judge(Request::make(K_DIR, D_FILE, F_RECURSIVE)) == Err(CP_ETYPE)
            && judge(Request::make(K_DIR, D_DIR, 0)) == Err(CP_EREC)
            && judge(Request::make(K_DIR, D_DIR, F_RECURSIVE)).map(|p| p.action) == Ok(ACT_RECURSE)
            && judge(Request::make(K_DIR, D_NONE, F_ARCHIVE)).is_ok(),
        "static",
    );

    s.add(
        "CP5-不覆盖命中即拒",
        judge(Request::make(K_REG, D_FILE, F_NO_CLOBBER)) == Err(CP_EEXIST)
            && judge(Request::make(K_REG, D_NONE, F_NO_CLOBBER)).is_ok()
            && resolve_conflict(F_NO_CLOBBER, true) == Ok(DEC_SKIP),
        "static",
    );

    s.add(
        "CP6-链接策略三态",
        judge(Request::make(K_LINK, D_NONE, 0)).map(|p| p.follow_link) == Ok(true)
            && judge(Request::make(K_LINK, D_NONE, F_ARCHIVE)).map(|p| p.follow_link) == Ok(false)
            && judge(Request::make(K_REG, D_NONE, F_ARCHIVE | F_DEREF)) == Err(CP_ELINK)
            && judge(Request::make(K_LINK, D_NONE, F_LINK)) == Err(CP_ELINK),
        "static",
    );

    s.add(
        "CP7-归档宏位展开",
        effective(F_ARCHIVE) == (F_RECURSIVE | F_PRESERVE_MODE | F_PRESERVE_TIME)
            && (effective(F_ARCHIVE) & F_ARCHIVE) == 0
            && judge(Request::make(K_REG, D_NONE, F_ARCHIVE)).map(|p| p.mode_bits()) == Ok(3)
            && judge(Request::make(K_REG, D_NONE, F_ARCHIVE)).map(|p| p.time_kept) == Ok(true),
        "static",
    );

    s.add(
        "CP8-保留权限需可落地",
        judge(Request::make(K_REG, D_NONE, F_LINK | F_PRESERVE_MODE)) == Err(CP_EMODE)
            && judge(Request::make(K_REG, D_NONE, F_LINK | F_PRESERVE_TIME)) == Err(CP_EMODE)
            && judge(Request::make(K_LINK, D_NONE, F_PRESERVE_MODE | F_DEREF))
                .map(|p| p.mode_kept)
                == Ok(true)
            && judge(Request::make(K_REG, D_NONE, F_PRESERVE_TIME)).map(|p| p.time_kept) == Ok(true),
        "static",
    );

    s.add(
        "CP9-稀疏仅限常规文件",
        judge(Request::make(K_LINK, D_NONE, F_SPARSE)) == Err(CP_ESPARE)
            && judge(Request::make(K_DIR, D_DIR, F_RECURSIVE | F_SPARSE)) == Err(CP_ESPARE)
            && judge(Request::make(K_REG, D_NONE, F_SPARSE)).map(|p| p.sparse) == Ok(true),
        "static",
    );

    s.add(
        "CP10-冲突优先级五档",
        conflict_rank(F_FORCE) == Ok(0)
            && conflict_rank(F_INTERACTIVE) == Ok(1)
            && conflict_rank(F_NO_CLOBBER) == Ok(2)
            && conflict_rank(F_UPDATE) == Ok(3)
            && conflict_rank(0) == Ok(4)
            && conflict_rank(F_FORCE | F_NO_CLOBBER) == Err(CP_EFLAG),
        "static",
    );

    s.add(
        "CP11-拼接与基名护栏",
        path_join("out", "a.txt", 255) == Ok(String::from("out/a.txt"))
            && path_join("out/", "a.txt", 255) == Ok(String::from("out/a.txt"))
            && path_join("out", "../x", 255) == Err(CP_EPATH)
            && path_join("out", "/x", 255) == Err(CP_EPATH)
            && basename("/usr//lib/") == Ok("lib")
            && basename("/") == Err(CP_EPATH),
        "static",
    );

    s.add(
        "CP12-仅更新降级为跳过",
        judge(Request::make(K_REG, D_FILE, F_UPDATE).times(100, 200)).map(|p| p.action)
            == Ok(ACT_SKIP)
            && judge(Request::make(K_REG, D_FILE, F_UPDATE).times(200, 100)).map(|p| p.action)
                == Ok(ACT_COPY)
            && judge(Request::make(K_REG, D_NONE, F_UPDATE).times(1, 0)).map(|p| p.action)
                == Ok(ACT_COPY)
            && judge(Request::make(K_REG, D_FILE, F_LINK)).map(|p| p.action) == Ok(ACT_LINK),
        "static",
    );

    let mut f = build_ledger();
    let rep = f.replay();
    let one = f.freeze();
    let two = f.freeze();
    let inj = inject_four();
    s.add(
        "CP13-冻结重放四注入",
        one.is_ok()
            && one == rep
            && two.is_err()
            && f.is_frozen()
            && f.root() != 0
            && f.rejects() == 1
            && inj[0]
            && inj[1]
            && inj[2]
            && inj[3],
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
        assert_eq!(l.sample(1).map(|s| s.value), Some(2));
        assert_eq!(l.sample(10).map(|s| s.code), Some(CP_EEXIST));
    }

    #[test]
    fn flag_space_and_archive() {
        assert_eq!(MASK.count_ones(), 11);
        assert!(has(F_ARCHIVE | F_SPARSE, F_SPARSE));
        assert!(!has(F_SPARSE, F_ARCHIVE));
        let e = effective(F_ARCHIVE);
        assert_eq!(e, F_RECURSIVE | F_PRESERVE_MODE | F_PRESERVE_TIME);
        assert_eq!(e & F_ARCHIVE, 0);
        assert_eq!(effective(0xffff) & !MASK, 0);
    }

    #[test]
    fn shape_table() {
        assert_eq!(shape_of(K_REG, D_NONE), Ok(SHAPE_F2F));
        assert_eq!(shape_of(K_REG, D_DIR), Ok(SHAPE_F2D));
        assert_eq!(shape_of(K_DIR, D_DIR), Ok(SHAPE_D2D));
        assert_eq!(shape_of(K_DIR, D_FILE), Err(CP_ETYPE));
        assert_eq!(shape_of(K_LINK, D_DIR), Ok(SHAPE_L2D));
        assert_eq!(shape_of(K_FIFO, D_FILE), Ok(SHAPE_S2F));
        assert_eq!(shape_of(4, D_FILE), Err(CP_ETYPE));
        assert_eq!(shape_of(K_REG, 3), Err(CP_ETYPE));
    }

    #[test]
    fn file_into_dir_plan() {
        let p = match judge(Request::make(K_REG, D_DIR, 0)) {
            Ok(v) => v,
            Err(_) => Plan::empty(),
        };
        assert_eq!(p.action, ACT_COPY_INTO);
        assert!(p.append_name);
        assert_eq!(p.mode_bits(), 0);
        assert_eq!(judge(Request::make(K_REG, D_FILE, 0)).map(|v| v.append_name), Ok(false));
    }

    #[test]
    fn dir_recursion_gate() {
        assert_eq!(judge(Request::make(K_DIR, D_FILE, F_RECURSIVE)), Err(CP_ETYPE));
        assert_eq!(judge(Request::make(K_DIR, D_DIR, 0)), Err(CP_EREC));
        assert_eq!(
            judge(Request::make(K_DIR, D_DIR, F_RECURSIVE)).map(|p| p.action),
            Ok(ACT_RECURSE)
        );
        assert!(judge(Request::make(K_DIR, D_NONE, F_ARCHIVE)).is_ok());
    }

    #[test]
    fn no_clobber_gate() {
        assert_eq!(judge(Request::make(K_REG, D_FILE, F_NO_CLOBBER)), Err(CP_EEXIST));
        assert!(judge(Request::make(K_REG, D_NONE, F_NO_CLOBBER)).is_ok());
        assert_eq!(judge(Request::make(K_REG, D_NONE, F_FORCE | F_NO_CLOBBER)), Err(CP_EFLAG));
    }

    #[test]
    fn link_policy() {
        assert_eq!(judge(Request::make(K_LINK, D_NONE, 0)).map(|p| p.follow_link), Ok(true));
        assert_eq!(
            judge(Request::make(K_LINK, D_NONE, F_ARCHIVE)).map(|p| p.follow_link),
            Ok(false)
        );
        assert_eq!(judge(Request::make(K_REG, D_NONE, F_ARCHIVE | F_DEREF)), Err(CP_ELINK));
        assert_eq!(judge(Request::make(K_LINK, D_NONE, F_LINK)), Err(CP_ELINK));
        assert!(follows(F_DEREF));
        assert!(!follows(F_ARCHIVE));
    }

    #[test]
    fn preserve_and_sparse() {
        assert_eq!(judge(Request::make(K_REG, D_NONE, F_LINK | F_PRESERVE_MODE)), Err(CP_EMODE));
        assert_eq!(judge(Request::make(K_LINK, D_NONE, F_SPARSE)), Err(CP_ESPARE));
        assert_eq!(
            judge(Request::make(K_REG, D_NONE, F_SPARSE)).map(|p| p.sparse),
            Ok(true)
        );
        assert_eq!(
            judge(Request::make(K_FIFO, D_NONE, 0)),
            Err(CP_ETYPE)
        );
        assert!(judge(Request::make(K_FIFO, D_NONE, F_ARCHIVE)).is_ok());
    }

    #[test]
    fn conflict_ranks_and_decisions() {
        assert_eq!(conflict_rank(F_FORCE), Ok(0));
        assert_eq!(conflict_rank(F_INTERACTIVE), Ok(1));
        assert_eq!(conflict_rank(F_NO_CLOBBER), Ok(2));
        assert_eq!(conflict_rank(F_UPDATE), Ok(3));
        assert_eq!(conflict_rank(0), Ok(4));
        assert_eq!(conflict_rank(0x8000), Err(CP_EFLAG));
        assert_eq!(resolve_conflict(F_FORCE, true), Ok(DEC_OVERWRITE));
        assert_eq!(resolve_conflict(F_INTERACTIVE, true), Ok(DEC_ASK));
        assert_eq!(resolve_conflict(F_NO_CLOBBER, true), Ok(DEC_SKIP));
        assert_eq!(resolve_conflict(0, false), Ok(DEC_COPY));
    }

    #[test]
    fn path_join_rules() {
        assert_eq!(path_join("out", "a", 255), Ok(String::from("out/a")));
        assert_eq!(path_join("out/", "a", 255), Ok(String::from("out/a")));
        assert_eq!(path_join("out", "a/b", 255), Ok(String::from("out/a/b")));
        assert_eq!(path_join("", "a", 255), Err(CP_EPATH));
        assert_eq!(path_join("out", "", 255), Err(CP_EPATH));
        assert_eq!(path_join("out", ".", 255), Err(CP_EPATH));
        assert_eq!(path_join("out", "a/../b", 255), Err(CP_EPATH));
        assert_eq!(path_join("out", "/abs", 255), Err(CP_EPATH));
        assert_eq!(path_join("out", "abcdef", 8), Err(CP_EPATH));
        assert_eq!(path_join("out", "ab", 6), Ok(String::from("out/ab")));
    }

    #[test]
    fn basename_rules() {
        assert_eq!(basename("a/b"), Ok("b"));
        assert_eq!(basename("a/b/"), Ok("b"));
        assert_eq!(basename("a//b//"), Ok("b"));
        assert_eq!(basename("name"), Ok("name"));
        assert_eq!(basename("/"), Err(CP_EPATH));
        assert_eq!(basename(""), Err(CP_EPATH));
        assert_eq!(basename("///"), Err(CP_EPATH));
    }

    #[test]
    fn update_and_link_actions() {
        assert_eq!(
            judge(Request::make(K_REG, D_FILE, F_UPDATE).times(100, 200)).map(|p| p.action),
            Ok(ACT_SKIP)
        );
        assert_eq!(
            judge(Request::make(K_REG, D_FILE, F_UPDATE).times(200, 100)).map(|p| p.action),
            Ok(ACT_COPY)
        );
        assert_eq!(
            judge(Request::make(K_REG, D_FILE, F_LINK)).map(|p| p.action),
            Ok(ACT_LINK)
        );
        assert_eq!(judge(Request::make(K_REG, D_FILE, F_LINK | F_SPARSE)), Err(CP_EFLAG));
    }

    #[test]
    fn unknown_bits_and_types() {
        assert_eq!(judge(Request::make(K_REG, D_NONE, 0x8000)), Err(CP_EFLAG));
        assert_eq!(judge(Request::make(9, D_NONE, 0)), Err(CP_ETYPE));
        assert_eq!(judge(Request::make(K_REG, 9, 0)), Err(CP_ETYPE));
        assert_eq!(shape_label(9), Err(CP_ETYPE));
        assert_eq!(shape_label(SHAPE_D2D), Ok("目录→目录"));
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = match l.freeze() {
            Ok(v) => v,
            Err(_) => 0,
        };
        assert_ne!(root, 0);
        assert!(l.is_frozen());
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(CP_EBAL));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.root(), root);
    }

    #[test]
    fn ledger_slot_guard() {
        let mut l = Ledger::new();
        let bad = Sample { slot: 1, op: OP_JUDGE, ok: true, code: 0, value: 1 };
        assert_eq!(l.register(bad), Err(CP_ESLOT));
        let good = Sample { slot: 0, op: OP_JUDGE, ok: true, code: 0, value: 1 };
        assert_eq!(l.register(good), Ok(()));
        assert_eq!(l.register(good), Err(CP_ESLOT));
        assert_eq!(l.rejects(), 2);
        assert_eq!(l.is_empty(), false);
    }

    #[test]
    fn injections() {
        let f2d = judge(Request::make(K_REG, D_DIR, 0));
        assert_eq!(f2d.map(|p| p.action), Ok(ACT_COPY_INTO), "f2d action");
        assert_eq!(f2d.map(|p| p.append_name), Ok(true), "f2d append");
        let d2d = judge(Request::make(K_DIR, D_DIR, F_RECURSIVE));
        assert_eq!(d2d.map(|p| p.action), Ok(ACT_RECURSE), "d2d action");
        assert_eq!(d2d.map(|p| p.append_name), Ok(true), "d2d append");
        let arch = judge(Request::make(K_REG, D_NONE, F_ARCHIVE));
        assert_eq!(arch.map(|p| p.mode_bits()), Ok(3), "arch bits");
        assert_eq!(arch.map(|p| p.action), Ok(ACT_COPY), "arch action");
        assert_eq!(
            judge(Request::make(K_REG, D_FILE, F_UPDATE).times(500, 500)).map(|p| p.action),
            Ok(ACT_SKIP),
            "skip"
        );
        assert_eq!(
            judge(Request::make(K_REG, D_FILE, F_UPDATE).times(500, 100)).map(|p| p.action),
            Ok(ACT_COPY),
            "newer"
        );
        assert_eq!(judge(Request::make(K_LINK, D_NONE, F_DEREF)).map(|p| p.follow_link), Ok(true), "link");
        let kept = judge(Request::make(K_LINK, D_NONE, F_ARCHIVE));
        assert_eq!(kept.map(|p| p.follow_link), Ok(false), "kept follow");
        assert_eq!(kept.map(|p| p.mode_kept), Ok(true), "kept mode");
        assert_eq!(inject_four(), [true, true, true, true]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11588_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }
}
