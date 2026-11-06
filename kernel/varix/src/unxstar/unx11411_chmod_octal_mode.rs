//! UNX-F11411 chmod 八进制模式语义
//!
//! # 领域模型
//!
//! `chmod` 的八进制形式是“绝对赋值”：参数串被当作一个 4 位八进制数，
//! 直接替换 inode 的 12 个模式位（setuid、setgid、sticky 与 rwxrwxrwx），
//! 与进程的 `umask` 无关。这一点常被误解：`umask` 只约束 `open`/`mkdir`
//! 之类“新建对象”的权限推导，`chmod` 是显式授权，必须逐位照抄。
//!
//! 本模型把模式限制在 `0o7777`：四位八进制数的上界。参数串可以带前导
//! 零（`000644` 与 `644` 同义），但不接受符号前缀（`+x`、`-w` 属于符号
//! 模式，见下一张卡），也不接受十进制数字 8、9 或任何字母。
//!
//! 文件系统侧有四种对象：普通文件、目录、符号链接与根目录。修改模式
//! 遵循“不跟随符号链接”的保守策略：对链接本身 `chmod` 返回 `ELINK`，
//! 调用者必须先解析到目标。只读挂载整体拒绝 `EROFS`；非属主且无
//! `CAP_FOWNER` 等价能力返回 `EPERM`。这三类拒绝的先后是有意的：
//! 对象种类先于挂载状态，挂载状态先于身份检查。
//!
//! # 闸门次序
//!
//! `chmod(路径, 参数串, 调用者, 是否具备特权)` 的裁决顺序：
//!
//! 1. 解析参数串：
//!    * 空串 → `EEMPTY`；
//!    * 首字符为 `+` 或 `-` → `ESIGN`（八进制形式不接受符号）；
//!    * 出现 `0`–`7` 之外的字节 → `EDIGIT`；
//!    * 数值超过 `0o7777` 或算术溢出 → `ERANGE`；
//! 2. 逐段解析路径（空路径即根）；任一段缺失、中间分量不是目录、
//!    或中间分量是链接 → `ENOENT`；
//! 3. 目标 inode 是符号链接 → `ELINK`；
//! 4. 文件系统只读 → `EROFS`；
//! 5. 调用者既非属主又无特权 → `EPERM`；
//! 6. 落位：`mode = 参数 & 0o7777`，返回新模式。
//!
//! 解析先于路径解析，是“语法先于语义”的直接体现：哪怕路径根本不存在，
//! 一个空参数串仍然先得到 `EEMPTY`。符号链接先于只读、只读先于身份，
//! 保证同一现场只暴露最“结构性”的那一类错误。
//!
//! # 账本契约
//!
//! `build_ledger()` 十四格、八成功六失败。成功格：四次解析
//! （`600`、`755`、`4755`、`1777`）和四次 `chmod`（文件改 `640`、
//! 目录改 `750`、文件改 `604`、文件再改 `755`），观测值是十进制表示的
//! 模式数值。失败格依次触发 `EEMPTY`、`ESIGN`、`EDIGIT`、`ERANGE`、
//! `ELINK`、`EPERM`。
//!
//! `audit()` 核对格数、成败计数、槽位编号、成功值序列
//! `[384, 493, 2541, 1023, 416, 488, 388, 493]` 与错误码序列
//! `[EEMPTY, ESIGN, EDIGIT, ERANGE, ELINK, EPERM]`。失败格观测值必须为
//! 零，成功格错误码必须为零。
//!
//! `seal()` 用 FNV-1a 折叠槽位、操作码、成败、错误码、观测值与拒绝
//! 计数；`freeze()` 固化；`replay()` 重建并比对，指纹不符即 `ESEAL`。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址：本卡独占的错误码窗口。
pub const SEG_BASE: u32 = 0xefb0;

/// 参数串为空。
pub const MD_EEMPTY: u32 = SEG_BASE + 0x01;
/// 八进制形式不接受 `+`/`-` 符号前缀。
pub const MD_ESIGN: u32 = SEG_BASE + 0x02;
/// 出现八进制数字之外的字节。
pub const MD_EDIGIT: u32 = SEG_BASE + 0x03;
/// 数值超过 `0o7777` 或算术溢出。
pub const MD_ERANGE: u32 = SEG_BASE + 0x04;
/// 路径缺失或中间分量不可穿越。
pub const MD_ENOENT: u32 = SEG_BASE + 0x05;
/// 目标为符号链接（不跟随）。
pub const MD_ELINK: u32 = SEG_BASE + 0x06;
/// 文件系统只读。
pub const MD_EROFS: u32 = SEG_BASE + 0x07;
/// 非属主且无特权。
pub const MD_EPERM: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const MD_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位错乱。
pub const MD_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const MD_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const MD_ESEAL: u32 = SEG_BASE + 0x0c;
/// 账本无此观测。
pub const MD_ENOBS: u32 = SEG_BASE + 0x0d;

/// 节点容量。
pub const CAP: usize = 8;
/// 根 inode。
pub const ROOT: u32 = 1;
/// 普通文件。
pub const KIND_FILE: u8 = 1;
/// 目录。
pub const KIND_DIR: u8 = 2;
/// 符号链接。
pub const KIND_LINK: u8 = 3;

/// 合法模式位上限。
pub const MODE_MAX: u16 = 0o7777;
/// 特权位（setuid/setgid）。
pub const MODE_SPECIAL: u16 = 0o6000;

/// 解析操作。
pub const OP_PARSE: u8 = 1;
/// 修改模式。
pub const OP_CHMOD: u8 = 2;
/// 查询。
pub const OP_QUERY: u8 = 3;

/// 样本格数。
pub const SAMPLES: usize = 14;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个 64 位字。
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

/// 八进制参数串 → 模式。
///
/// 只接受 `0`–`7`，允许前导零；空串、符号前缀、非法数字、越界分别
/// 返回 `EEMPTY`、`ESIGN`、`EDIGIT`、`ERANGE`。不做 `umask` 推导。
pub fn parse_octal(spec: &[u8]) -> Result<u16, u32> {
    if spec.is_empty() {
        return Err(MD_EEMPTY);
    }
    match spec.get(0) {
        Some(b'+') | Some(b'-') => return Err(MD_ESIGN),
        Some(_) => {}
        None => return Err(MD_EEMPTY),
    }
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < spec.len() {
        let b = match spec.get(i) {
            Some(x) => *x,
            None => return Err(MD_EDIGIT),
        };
        if b < b'0' || b > b'7' {
            return Err(MD_EDIGIT);
        }
        v = match v.checked_mul(8) {
            Some(x) => x,
            None => return Err(MD_ERANGE),
        };
        v = match v.checked_add((b - b'0') as u32) {
            Some(x) => x,
            None => return Err(MD_ERANGE),
        };
        if v > MODE_MAX as u32 {
            return Err(MD_ERANGE);
        }
        i += 1;
    }
    Ok(v as u16)
}

/// 模式 → 十进制打包（供人读的观测值）。
///
/// `0o644` 渲染为 `644`，`0o0644` 同样为 `644`，`0o4755` 为 `4755`。
pub fn render(mode: u16) -> u32 {
    let mut v = (mode & MODE_MAX) as u32;
    let mut out = 0u32;
    let mut mul = 1u32;
    let mut i = 0u32;
    while i < 4 {
        out = out.saturating_add((v & 7).saturating_mul(mul));
        v >>= 3;
        mul = mul.saturating_mul(10);
        i += 1;
    }
    out
}

/// 一个文件系统节点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    /// inode。
    pub ino: u32,
    /// 父目录。
    pub parent: u32,
    /// 名字。
    pub name: u16,
    /// 种类：文件/目录/链接。
    pub kind: u8,
    /// 模式位（12 位）。
    pub mode: u16,
    /// 属主。
    pub owner: u16,
    /// 是否占用。
    pub used: bool,
}

impl Node {
    /// 空白节点。
    pub const fn blank() -> Node {
        Node { ino: 0, parent: 0, name: 0, kind: 0, mode: 0, owner: 0, used: false }
    }
}

/// 扁平节点表构成的文件系统。
#[derive(Clone, Copy, Debug)]
pub struct Fs {
    nodes: [Node; CAP],
    ro: bool,
    umask: u16,
    hops: u32,
}

impl Fs {
    /// 建立只有根的文件系统（根目录 0o755，属主 100，umask 0o022）。
    pub fn new() -> Fs {
        let mut f = Fs { nodes: [Node::blank(); CAP], ro: false, umask: 0o022, hops: 0 };
        if let Some(r) = f.nodes.get_mut(0) {
            *r = Node {
                ino: ROOT,
                parent: ROOT,
                name: 0,
                kind: KIND_DIR,
                mode: 0o755,
                owner: 100,
                used: true,
            };
        }
        f
    }

    /// inode 下标。
    fn index_of(&self, ino: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < CAP {
            if let Some(n) = self.nodes.get(i) {
                if n.used && n.ino == ino {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 取节点副本。
    pub fn node(&self, ino: u32) -> Result<Node, u32> {
        match self.index_of(ino) {
            Some(i) => match self.nodes.get(i) {
                Some(n) => Ok(*n),
                None => Err(MD_ENOENT),
            },
            None => Err(MD_ENOENT),
        }
    }

    /// 建节点。
    pub fn add(
        &mut self,
        parent: u32,
        name: u16,
        ino: u32,
        kind: u8,
        mode: u16,
        owner: u16,
    ) -> Result<u32, u32> {
        let _ = self.node(parent)?;
        if name == 0 || ino == 0 {
            return Err(MD_ERANGE);
        }
        if kind != KIND_FILE && kind != KIND_DIR && kind != KIND_LINK {
            return Err(MD_ERANGE);
        }
        if self.index_of(ino).is_some() {
            return Err(MD_ERANGE);
        }
        let mut k = 0usize;
        while k < CAP {
            if let Some(n) = self.nodes.get(k) {
                if n.used && n.parent == parent && n.name == name {
                    return Err(MD_ERANGE);
                }
            }
            k += 1;
        }
        let mut free = CAP;
        let mut i = 0usize;
        while i < CAP {
            if let Some(n) = self.nodes.get(i) {
                if !n.used {
                    free = i;
                    break;
                }
            }
            i += 1;
        }
        if free == CAP {
            return Err(MD_ERANGE);
        }
        if let Some(n) = self.nodes.get_mut(free) {
            *n = Node { ino, parent, name, kind, mode: mode & MODE_MAX, owner, used: true };
        }
        Ok(ino)
    }

    /// 逐段解析，空路径即根。
    pub fn resolve(&mut self, path: &[u16]) -> Result<u32, u32> {
        if path.len() > CAP {
            return Err(MD_ENOENT);
        }
        let mut cur = ROOT;
        let mut i = 0usize;
        while i < path.len() {
            let name = match path.get(i) {
                Some(v) => *v,
                None => return Err(MD_ENOENT),
            };
            let rec = self.node(cur)?;
            if rec.kind != KIND_DIR {
                return Err(MD_ENOENT);
            }
            let mut found: Option<u32> = None;
            let mut k = 0usize;
            while k < CAP {
                if let Some(n) = self.nodes.get(k) {
                    if n.used && n.parent == cur && n.name == name {
                        found = Some(n.ino);
                        break;
                    }
                }
                k += 1;
            }
            cur = match found {
                Some(v) => v,
                None => return Err(MD_ENOENT),
            };
            self.hops = self.hops.saturating_add(1);
            i += 1;
        }
        Ok(cur)
    }

    /// 修改模式。参数串先解析，再走路径。
    pub fn chmod(
        &mut self,
        path: &[u16],
        spec: &[u8],
        actor: u16,
        capable: bool,
    ) -> Result<u16, u32> {
        let want = parse_octal(spec)?;
        let ino = self.resolve(path)?;
        let rec = self.node(ino)?;
        if rec.kind == KIND_LINK {
            return Err(MD_ELINK);
        }
        if self.ro {
            return Err(MD_EROFS);
        }
        if actor != rec.owner && !capable {
            return Err(MD_EPERM);
        }
        let idx = match self.index_of(ino) {
            Some(i) => i,
            None => return Err(MD_ENOENT),
        };
        let fresh = want & MODE_MAX;
        if let Some(n) = self.nodes.get_mut(idx) {
            n.mode = fresh;
        }
        Ok(fresh)
    }

    /// 取模式。
    pub fn mode_of(&self, ino: u32) -> Result<u16, u32> {
        Ok(self.node(ino)?.mode)
    }

    /// 取属主。
    pub fn owner_of(&self, ino: u32) -> Result<u16, u32> {
        Ok(self.node(ino)?.owner)
    }

    /// 设置只读挂载。
    pub fn set_ro(&mut self, on: bool) {
        self.ro = on;
    }

    /// 是否只读。
    pub fn is_ro(&self) -> bool {
        self.ro
    }

    /// 设置 umask（只影响新建对象，不影响 chmod）。
    pub fn set_umask(&mut self, m: u16) {
        self.umask = m & MODE_MAX;
    }

    /// 当前 umask。
    pub fn umask(&self) -> u16 {
        self.umask
    }

    /// 解析过的路径段数。
    pub fn hops(&self) -> u32 {
        self.hops
    }

    /// 不变式：根存在、所有占用节点父指针有效、名字非零、模式不越界。
    pub fn consistent(&self) -> bool {
        match self.nodes.get(0) {
            Some(r) => {
                if !r.used || r.ino != ROOT || r.kind != KIND_DIR {
                    return false;
                }
            }
            None => return false,
        }
        let mut i = 0usize;
        while i < CAP {
            if let Some(n) = self.nodes.get(i) {
                if n.used {
                    if n.ino == 0 || n.mode > MODE_MAX {
                        return false;
                    }
                    if self.node(n.parent).is_err() {
                        return false;
                    }
                }
            }
            i += 1;
        }
        true
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

    /// 登记一格。
    pub fn register(&mut self, row: Sample) -> Result<(), u32> {
        if self.frozen || (row.slot as usize) != self.len || (row.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MD_ESLOT);
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

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(MD_EBAL);
        }
        const VALS: [u32; 8] = [384, 493, 2541, 1023, 416, 488, 388, 493];
        const CODES: [u32; 6] =
            [MD_EEMPTY, MD_ESIGN, MD_EDIGIT, MD_ERANGE, MD_ELINK, MD_EPERM];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let r = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(MD_EBAL),
            };
            if r.slot as usize != i {
                return Err(MD_ESLOT);
            }
            if r.ok {
                if r.code != 0 {
                    return Err(MD_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if r.value != *w {
                            return Err(MD_EVALUE);
                        }
                    }
                    None => return Err(MD_EBAL),
                }
                vi = vi.saturating_add(1);
            } else {
                if r.value != 0 {
                    return Err(MD_EVALUE);
                }
                match CODES.get(ci) {
                    Some(w) => {
                        if r.code != *w {
                            return Err(MD_EVALUE);
                        }
                    }
                    None => return Err(MD_EBAL),
                }
                ci = ci.saturating_add(1);
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(MD_EBAL);
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
            return Err(MD_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        if !self.frozen {
            return Err(MD_ESEAL);
        }
        let fresh = build_ledger();
        let again = fresh.seal();
        if again == self.seal() && again == self.root {
            Ok(again)
        } else {
            Err(MD_ESEAL)
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
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut f = Fs::new();
    let _ = f.add(ROOT, 10, 2, KIND_FILE, 0o644, 100);
    let _ = f.add(ROOT, 20, 3, KIND_FILE, 0o600, 200);
    let _ = f.add(ROOT, 30, 4, KIND_LINK, 0o777, 100);
    let _ = f.add(ROOT, 40, 5, KIND_DIR, 0o755, 100);
    let _ = f.add(ROOT, 50, 6, KIND_FILE, 0o644, 100);

    let _ = l.register(word_cell(0, OP_PARSE, parse_octal(b"600").map(|v| v as u32)));
    let _ = l.register(word_cell(1, OP_PARSE, parse_octal(b"755").map(|v| v as u32)));
    let _ = l.register(word_cell(2, OP_PARSE, parse_octal(b"4755").map(|v| v as u32)));
    let _ = l.register(word_cell(3, OP_PARSE, parse_octal(b"1777").map(|v| v as u32)));

    let _ = l.register(word_cell(4, OP_CHMOD, f.chmod(&[10], b"640", 100, false).map(|v| v as u32)));
    let _ = l.register(word_cell(5, OP_CHMOD, f.chmod(&[40], b"750", 100, false).map(|v| v as u32)));
    let _ = l.register(word_cell(6, OP_CHMOD, f.chmod(&[50], b"604", 100, false).map(|v| v as u32)));
    let _ = l.register(word_cell(7, OP_CHMOD, f.chmod(&[10], b"755", 100, false).map(|v| v as u32)));

    let _ = l.register(word_cell(8, OP_PARSE, parse_octal(b"").map(|v| v as u32)));
    let _ = l.register(word_cell(9, OP_PARSE, parse_octal(b"+x").map(|v| v as u32)));
    let _ = l.register(word_cell(10, OP_PARSE, parse_octal(b"8").map(|v| v as u32)));
    let _ = l.register(word_cell(11, OP_PARSE, parse_octal(b"17777").map(|v| v as u32)));
    let _ = l.register(word_cell(12, OP_CHMOD, f.chmod(&[30], b"700", 100, false).map(|v| v as u32)));
    let _ = l.register(word_cell(13, OP_CHMOD, f.chmod(&[20], b"700", 100, false).map(|v| v as u32)));

    l
}

/// 四路注入：账本身、解析边界、修改语义、闸门次序。
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

    out[1] = parse_octal(b"0") == Ok(0)
        && parse_octal(b"000644") == Ok(0o644)
        && parse_octal(b"7777") == Ok(0o7777)
        && parse_octal(b"10") == Ok(8)
        && parse_octal(b"").is_err()
        && parse_octal(b"+7") == Err(MD_ESIGN)
        && parse_octal(b"-7") == Err(MD_ESIGN)
        && parse_octal(b"9") == Err(MD_EDIGIT)
        && parse_octal(b"178") == Err(MD_EDIGIT)
        && parse_octal(b"17777") == Err(MD_ERANGE)
        && parse_octal(b"777777777777") == Err(MD_ERANGE);

    let mut t = Fs::new();
    let built = t.add(ROOT, 10, 2, KIND_FILE, 0o644, 100).is_ok()
        && t.add(ROOT, 20, 3, KIND_FILE, 0o600, 200).is_ok()
        && t.add(ROOT, 30, 4, KIND_LINK, 0o777, 100).is_ok();
    t.set_umask(0o077);
    let full = t.chmod(&[10], b"777", 100, false);
    let link = t.chmod(&[30], b"700", 100, false);
    let denied = t.chmod(&[20], b"700", 100, false);
    let before_cap = t.mode_of(3);
    let cap = t.chmod(&[20], b"741", 100, true);
    out[2] = built
        && full == Ok(0o777)
        && t.mode_of(2) == Ok(0o777)
        && t.umask() == 0o077
        && link == Err(MD_ELINK)
        && t.mode_of(4) == Ok(0o777)
        && denied == Err(MD_EPERM)
        && before_cap == Ok(0o600)
        && cap == Ok(0o741)
        && t.mode_of(3) == Ok(0o741)
        && t.consistent();

    let mut a = Fs::new();
    let _ = a.add(ROOT, 10, 2, KIND_FILE, 0o644, 100);
    let _ = a.add(ROOT, 20, 3, KIND_FILE, 0o600, 200);
    let _ = a.add(ROOT, 30, 4, KIND_LINK, 0o777, 100);
    a.set_ro(true);
    out[3] = a.chmod(&[99], b"", 100, false) == Err(MD_EEMPTY)
        && a.chmod(&[99], b"644", 100, false) == Err(MD_ENOENT)
        && a.chmod(&[30], b"644", 100, false) == Err(MD_ELINK)
        && a.chmod(&[20], b"644", 100, false) == Err(MD_EROFS)
        && a.chmod(&[20], b"644", 100, true) == Err(MD_EROFS);

    out
}

/// 十三项检查。
pub fn run_unx11411_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11411");
    let l = build_ledger();

    s.add(
        "MD1-台账格数与审计",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        "static",
    );

    s.add(
        "MD2-八正例值序",
        {
            const VALS: [u32; 8] = [384, 493, 2541, 1023, 416, 488, 388, 493];
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
        "MD3-六错误码序",
        {
            const CODES: [u32; 6] = [MD_EEMPTY, MD_ESIGN, MD_EDIGIT, MD_ERANGE, MD_ELINK, MD_EPERM];
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
        "MD4-八进制解析边界",
        parse_octal(b"0") == Ok(0)
            && parse_octal(b"7") == Ok(7)
            && parse_octal(b"10") == Ok(8)
            && parse_octal(b"644") == Ok(0o644)
            && parse_octal(b"0644") == Ok(0o644)
            && parse_octal(b"000644") == Ok(0o644)
            && parse_octal(b"7777") == Ok(0o7777)
            && render(0o644) == 644
            && render(0o4755) == 4755
            && render(0o0644) == 644,
        "static",
    );

    s.add(
        "MD5-非法数字与符号",
        parse_octal(b"") == Err(MD_EEMPTY)
            && parse_octal(b"+x") == Err(MD_ESIGN)
            && parse_octal(b"-w") == Err(MD_ESIGN)
            && parse_octal(b"8") == Err(MD_EDIGIT)
            && parse_octal(b"64a") == Err(MD_EDIGIT)
            && parse_octal(b"0x1ff") == Err(MD_EDIGIT)
            && parse_octal(b"17777") == Err(MD_ERANGE)
            && parse_octal(b"77777777") == Err(MD_ERANGE),
        "static",
    );

    s.add(
        "MD6-完整替换忽略掩码",
        {
            let mut f = Fs::new();
            let built = f.add(ROOT, 10, 2, KIND_FILE, 0o644, 100).is_ok();
            f.set_umask(0o077);
            let m = f.chmod(&[10], b"777", 100, false);
            let zero = f.chmod(&[10], b"0", 100, false);
            built
                && m == Ok(0o777)
                && zero == Ok(0)
                && f.mode_of(2) == Ok(0)
                && f.umask() == 0o077
                && f.consistent()
        },
        "static",
    );

    s.add(
        "MD7-符号链接拒绝",
        {
            let mut f = Fs::new();
            let built = f
                .add(ROOT, 10, 2, KIND_FILE, 0o644, 100)
                .is_ok()
                && f.add(ROOT, 30, 4, KIND_LINK, 0o777, 100).is_ok();
            let r = f.chmod(&[30], b"700", 100, false);
            built
                && r == Err(MD_ELINK)
                && f.mode_of(4) == Ok(0o777)
                && f.chmod(&[10], b"700", 100, false) == Ok(0o700)
        },
        "static",
    );

    s.add(
        "MD8-非属主与特权",
        {
            let mut f = Fs::new();
            let built = f.add(ROOT, 20, 3, KIND_FILE, 0o600, 200).is_ok();
            let denied = f.chmod(&[20], b"644", 100, false);
            let before = f.mode_of(3);
            let opened = f.chmod(&[20], b"640", 100, true);
            let after = f.mode_of(3);
            built
                && denied == Err(MD_EPERM)
                && before == Ok(0o600)
                && opened == Ok(0o640)
                && after == Ok(0o640)
                && f.owner_of(3) == Ok(200)
                && f.chmod(&[20], b"600", 200, false) == Ok(0o600)
                && f.mode_of(3) == Ok(0o600)
        },
        "static",
    );

    s.add(
        "MD9-只读挂载拒绝",
        {
            let mut f = Fs::new();
            let built = f.add(ROOT, 10, 2, KIND_FILE, 0o644, 100).is_ok();
            let before = f.mode_of(2);
            f.set_ro(true);
            let r = f.chmod(&[10], b"777", 100, false);
            let ro = f.is_ro();
            f.set_ro(false);
            built && r == Err(MD_EROFS) && before == Ok(0o644) && f.mode_of(2) == Ok(0o644)
                && ro
                && f.chmod(&[10], b"755", 100, false) == Ok(0o755)
        },
        "static",
    );

    s.add(
        "MD10-路径缺失",
        {
            let mut f = Fs::new();
            let built = f.add(ROOT, 10, 2, KIND_FILE, 0o644, 100).is_ok();
            built
                && f.chmod(&[99], b"644", 100, false) == Err(MD_ENOENT)
                && f.chmod(&[10, 99], b"644", 100, false) == Err(MD_ENOENT)
                && f.chmod(&[77, 88], b"644", 100, false) == Err(MD_ENOENT)
                && f.mode_of(2) == Ok(0o644)
        },
        "static",
    );

    let (md11_ok, md11_mode) = {
        let mut f = Fs::new();
        let built = f.add(ROOT, 10, 2, KIND_FILE, 0o644, 100).is_ok();
        let suid = f.chmod(&[10], b"4755", 100, false);
        let all = f.chmod(&[10], b"7777", 100, false);
        let over = f.chmod(&[10], b"17777", 100, false);
        let mode = f.mode_of(2);
        let good = built
            && suid == Ok(0o4755)
            && all == Ok(0o7777)
            && over == Err(MD_ERANGE)
            && mode == Ok(0o7777)
            && (mode.unwrap_or(0) & MODE_SPECIAL) == MODE_SPECIAL;
        (good, mode.unwrap_or(0))
    };
    s.add_dyn(
        "MD11-特权位与掩码",
        md11_ok,
        &format!("mode={} special={}", render(md11_mode), md11_mode & MODE_SPECIAL),
    );

    s.add(
        "MD12-门序优先",
        {
            let mut f = Fs::new();
            let built = f
                .add(ROOT, 20, 3, KIND_FILE, 0o600, 200)
                .is_ok()
                && f.add(ROOT, 30, 4, KIND_LINK, 0o777, 100).is_ok();
            f.set_ro(true);
            built
                && f.chmod(&[99], b"", 100, false) == Err(MD_EEMPTY)
                && f.chmod(&[99], b"644", 100, false) == Err(MD_ENOENT)
                && f.chmod(&[30], b"644", 100, false) == Err(MD_ELINK)
                && f.chmod(&[20], b"644", 100, false) == Err(MD_EROFS)
                && f.chmod(&[20], b"644", 100, true) == Err(MD_EROFS)
        },
        "static",
    );

    s.add(
        "MD13-冻结重放四注入",
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
        let s = run_unx11411_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn parse_octal_matrix() {
        assert_eq!(parse_octal(b"0"), Ok(0));
        assert_eq!(parse_octal(b"644"), Ok(420));
        assert_eq!(parse_octal(b"000755"), Ok(493));
        assert_eq!(parse_octal(b"7777"), Ok(4095));
        assert_eq!(parse_octal(b""), Err(MD_EEMPTY));
        assert_eq!(parse_octal(b"+r"), Err(MD_ESIGN));
        assert_eq!(parse_octal(b"9"), Err(MD_EDIGIT));
        assert_eq!(parse_octal(b"17777"), Err(MD_ERANGE));
    }

    #[test]
    fn chmod_replaces_mode() {
        let mut f = Fs::new();
        assert_eq!(f.add(ROOT, 10, 2, KIND_FILE, 0o644, 100), Ok(2));
        f.set_umask(0o077);
        assert_eq!(f.chmod(&[10], b"777", 100, false), Ok(0o777));
        assert_eq!(f.mode_of(2), Ok(0o777));
        assert_eq!(f.umask(), 0o077);
        assert_eq!(f.chmod(&[10], b"4755", 100, false), Ok(0o4755));
        assert_eq!(f.mode_of(2), Ok(0o4755));
        assert_eq!(f.chmod(&[10], b"0", 100, false), Ok(0));
        assert_eq!(f.mode_of(2), Ok(0));
        assert!(f.consistent());
    }

    #[test]
    fn symlink_and_owner_gates() {
        let mut f = Fs::new();
        assert_eq!(f.add(ROOT, 20, 3, KIND_FILE, 0o600, 200), Ok(3));
        assert_eq!(f.add(ROOT, 30, 4, KIND_LINK, 0o777, 100), Ok(4));
        assert_eq!(f.chmod(&[30], b"700", 100, false), Err(MD_ELINK));
        assert_eq!(f.mode_of(4), Ok(0o777));
        assert_eq!(f.chmod(&[20], b"644", 100, false), Err(MD_EPERM));
        assert_eq!(f.mode_of(3), Ok(0o600));
        assert_eq!(f.chmod(&[20], b"644", 100, true), Ok(0o644));
        assert_eq!(f.chmod(&[20], b"600", 200, false), Ok(0o600));
    }

    #[test]
    fn readonly_and_missing() {
        let mut f = Fs::new();
        assert_eq!(f.add(ROOT, 10, 2, KIND_FILE, 0o644, 100), Ok(2));
        f.set_ro(true);
        assert_eq!(f.chmod(&[10], b"777", 100, false), Err(MD_EROFS));
        assert_eq!(f.mode_of(2), Ok(0o644));
        assert_eq!(f.chmod(&[99], b"644", 100, false), Err(MD_ENOENT));
        f.set_ro(false);
        assert_eq!(f.chmod(&[10], b"755", 100, false), Ok(0o755));
        assert_eq!(f.chmod(&[10, 99], b"644", 100, false), Err(MD_ENOENT));
    }

    #[test]
    fn gate_order() {
        let mut f = Fs::new();
        assert_eq!(f.add(ROOT, 20, 3, KIND_FILE, 0o600, 200), Ok(3));
        assert_eq!(f.add(ROOT, 30, 4, KIND_LINK, 0o777, 100), Ok(4));
        f.set_ro(true);
        assert_eq!(f.chmod(&[99], b"", 100, false), Err(MD_EEMPTY));
        assert_eq!(f.chmod(&[99], b"644", 100, false), Err(MD_ENOENT));
        assert_eq!(f.chmod(&[30], b"644", 100, false), Err(MD_ELINK));
        assert_eq!(f.chmod(&[20], b"644", 100, false), Err(MD_EROFS));
    }

    #[test]
    fn inject_four_holds() {
        let inj = inject_four();
        assert_eq!(inj, [true, true, true, true]);
    }
}
