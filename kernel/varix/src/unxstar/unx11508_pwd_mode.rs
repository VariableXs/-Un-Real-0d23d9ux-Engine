//! UNX-F11508 pwd -P/-L 逻辑物理路径（0xf0d0 段）
//!
//! `pwd` 只有两种答案：**逻辑路径**（-L，默认）与**物理路径**（-P）。
//! 逻辑路径保留调用者"看见"的样子：`..` 只在字符串上退一级，符号链接
//! 不被解开；物理路径则把每个分量依次落到真实目录上，链接目标替换当前
//! 前缀，`..` 退的是替换之后的父目录。两者不是"同一算法的开关"，而是
//! 两条独立的走法，本卡用同一台分量行走机承载它们，靠 `mode` 决定
//! 每压入一个分量后是否查链接表。
//!
//! 路径先过规范化闸门：cwd 必须是绝对路径、无 `//`、无 `.`/`..` 分量、
//! 不以 `/` 结尾；否则 `EBADPATH`。参数里允许 `//`、`.`、`..`，行走时
//! 折叠：`.` 跳过，`..` 从当前分量栈弹一级（根上弹不动），空分量忽略。
//! 分量长度超过 16 是 `EBADNAME`，分量总数超过 16 是 `EDEPTH`，缓冲
//! 不足是 `EBUF`，链接展开超过 8 跳是 `ELOOP`。
//!
//! 链接表是这张卡的文件系统替身：每条记录是"绝对路径 → 绝对路径"，
//! 两端都必须已经规范（无 `.`/`..`/`//`/尾斜线），自指记录直接拒。
//! 物理模式每压入一个分量就查一次表，命中就把当前前缀整体换成目标，
//! 再继续查，直到不再命中；这条"展开到不动点"的循环由跳数上限兜底。
//!
//! `pwd -L` 的可信规则是 POSIX 的老规矩：环境里的 `PWD` 只有在"它解析
//! 出来的物理目录与 `.` 解析出来的物理目录相同"时才被采信，否则退回
//! 物理解析。所以 `-L` 可以打印带链接的名字，但绝不会打印一个指向别处
//! 的假名字。`-P` 无条件走物理解析，`PWD` 连看都不看。
//!
//! 选项闸门：空参数默认 -L；`-L/--logical` 与 `-P/--physical` 同时出现是
//! `EBOTH`（本卡不玩"后者覆盖"），同一选项重复是幂等的；其余带横线的词
//! 是 `EOPT`；裸词是 `EOPERAND`，因为 pwd 不接受位置参数。
//!
//! 账本十四格：八格成功（默认模式、-P、逻辑走法、单链接、点点折叠、
//! 嵌套链接、-L 采信 PWD、-P 忽略 PWD），六格拒绝（未知选项、双模式、
//! 链接环、超深、缓冲不足、相对 cwd）。审计逐格复算，封印折叠全账。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf0d0;

/// 未知选项。
pub const PW_EOPT: u32 = SEG_BASE + 0x01;
/// -L 与 -P 同时出现。
pub const PW_EBOTH: u32 = SEG_BASE + 0x02;
/// 链接展开成环。
pub const PW_ELOOP: u32 = SEG_BASE + 0x03;
/// 分量过多。
pub const PW_EDEPTH: u32 = SEG_BASE + 0x04;
/// 缓冲不足。
pub const PW_EBUF: u32 = SEG_BASE + 0x05;
/// 路径不规范。
pub const PW_EBADPATH: u32 = SEG_BASE + 0x06;
/// 分量过长。
pub const PW_EBADNAME: u32 = SEG_BASE + 0x07;
/// pwd 不接受位置参数。
pub const PW_EOPERAND: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const PW_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const PW_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const PW_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const PW_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const PW_ENOBS: u32 = SEG_BASE + 0x0d;

/// 路径缓冲。
pub const CAP: usize = 64;
/// 单个分量长度上限。
pub const COMP_MAX: usize = 16;
/// 分量个数上限。
pub const DEPTH_MAX: usize = 16;
/// 链接跳数上限。
pub const HOPS_MAX: u32 = 8;
/// 链接表容量。
pub const LINKS_MAX: usize = 8;
/// 链接串容量。
pub const LINK_CAP: usize = 32;
/// 逻辑模式。
pub const MODE_L: u8 = 1;
/// 物理模式。
pub const MODE_P: u8 = 2;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：解选项。
pub const OP_MODE: u8 = 1;
/// 操作码：规范化路径。
pub const OP_CANON: u8 = 2;
/// 操作码：问 pwd。
pub const OP_PWD: u8 = 3;

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

/// 路径是否已规范：绝对、无空分量、无点分量、无尾斜线、分量不超长。
pub fn normalized(p: &[u8]) -> bool {
    if p.get(0).copied() != Some(b'/') {
        return false;
    }
    if p.len() > 1 && p.get(p.len() - 1).copied() == Some(b'/') {
        return false;
    }
    let mut i = 1usize;
    while i < p.len() {
        let start = i;
        while i < p.len() && p.get(i).copied() != Some(b'/') {
            i += 1;
        }
        let comp: &[u8] = match p.get(start..i) {
            Some(v) => v,
            None => return false,
        };
        if comp.is_empty() || comp == b"." || comp == b".." || comp.len() > COMP_MAX {
            return false;
        }
        if i < p.len() && p.get(i + 1).copied() == Some(b'/') {
            return false;
        }
        i += 1;
    }
    true
}

/// 分量个数。
fn depth(p: &[u8]) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < p.len() {
        if p.get(i).copied() == Some(b'/') {
            n += 1;
        }
        i += 1;
    }
    if p.len() <= 1 {
        0
    } else {
        n
    }
}

/// 造一条 n 段的全 a 深路径。
fn deep_path(buf: &mut [u8], n: usize) -> Result<&[u8], u32> {
    let mut at = 0usize;
    let mut i = 0usize;
    while i < n {
        if let Some(c) = buf.get_mut(at) {
            *c = b'/';
        }
        at += 1;
        if let Some(c) = buf.get_mut(at) {
            *c = b'a';
        }
        at += 1;
        i += 1;
    }
    match buf.get(..at) {
        Some(v) => Ok(v),
        None => Err(PW_EBUF),
    }
}

/// 一条链接记录。
#[derive(Clone, Copy, Debug)]
pub struct Link {
    /// 起点。
    pub from: [u8; LINK_CAP],
    /// 起点长度。
    pub flen: u8,
    /// 目标。
    pub to: [u8; LINK_CAP],
    /// 目标长度。
    pub tlen: u8,
}

impl Link {
    /// 空白记录。
    pub const fn blank() -> Link {
        Link { from: [0u8; LINK_CAP], flen: 0, to: [0u8; LINK_CAP], tlen: 0 }
    }
}

/// 链接表。
#[derive(Clone, Copy, Debug)]
pub struct Links {
    rows: [Link; LINKS_MAX],
    n: usize,
}

impl Links {
    /// 空表。
    pub const fn new() -> Links {
        Links { rows: [Link::blank(); LINKS_MAX], n: 0 }
    }

    /// 登记一条链接。
    pub fn add(&mut self, from: &[u8], to: &[u8]) -> Result<(), u32> {
        if self.n >= LINKS_MAX {
            return Err(PW_EBUF);
        }
        if from.len() > LINK_CAP || to.len() > LINK_CAP {
            return Err(PW_EBUF);
        }
        if !normalized(from) || !normalized(to) {
            return Err(PW_EBADPATH);
        }
        if from == to {
            return Err(PW_ELOOP);
        }
        let mut r = Link::blank();
        r.flen = copy(&mut r.from, from);
        r.tlen = copy(&mut r.to, to);
        if let Some(dst) = self.rows.get_mut(self.n) {
            *dst = r;
        }
        self.n += 1;
        Ok(())
    }

    /// 表内条数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 查一条链接，命中则把目标抄进 `dst` 并返回长度。
    fn find(&self, path: &[u8], dst: &mut [u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            if let Some(r) = self.rows.get(i) {
                let from: &[u8] = match r.from.get(..r.flen as usize) {
                    Some(v) => v,
                    None => return None,
                };
                if from == path {
                    let to: &[u8] = match r.to.get(..r.tlen as usize) {
                        Some(v) => v,
                        None => return None,
                    };
                    if to.len() > dst.len() {
                        return None;
                    }
                    let mut k = 0usize;
                    while k < to.len() {
                        if let (Some(d), Some(s)) = (dst.get_mut(k), to.get(k)) {
                            *d = *s;
                        }
                        k += 1;
                    }
                    return Some(to.len());
                }
            }
            i += 1;
        }
        None
    }
}

/// 抄字节串，返回长度。
fn copy(dst: &mut [u8], src: &[u8]) -> u8 {
    let mut i = 0usize;
    while i < src.len() && i < dst.len() {
        if let (Some(d), Some(s)) = (dst.get_mut(i), src.get(i)) {
            *d = *s;
        }
        i += 1;
    }
    i as u8
}

/// 喂入一条路径，返回新前缀长度。
fn feed(
    cur: &mut [u8],
    mut clen: usize,
    path: &[u8],
    mode: u8,
    links: &Links,
    hops: &mut u32,
) -> Result<usize, u32> {
    let mut i = 0usize;
    while i < path.len() {
        while i < path.len() && path.get(i).copied() == Some(b'/') {
            i += 1;
        }
        let start = i;
        while i < path.len() && path.get(i).copied() != Some(b'/') {
            i += 1;
        }
        let comp: &[u8] = match path.get(start..i) {
            Some(v) => v,
            None => break,
        };
        if comp.is_empty() || comp == b"." {
            continue;
        }
        if comp == b".." {
            while clen > 1 && cur.get(clen - 1).copied() != Some(b'/') {
                clen -= 1;
            }
            if clen > 1 {
                clen -= 1;
            }
            continue;
        }
        if comp.len() > COMP_MAX {
            return Err(PW_EBADNAME);
        }
        if clen > 1 {
            if clen >= CAP {
                return Err(PW_EBUF);
            }
            if let Some(c) = cur.get_mut(clen) {
                *c = b'/';
            }
            clen += 1;
        }
        if clen + comp.len() > CAP {
            return Err(PW_EBUF);
        }
        let mut k = 0usize;
        while k < comp.len() {
            if let (Some(d), Some(s)) = (cur.get_mut(clen + k), comp.get(k)) {
                *d = *s;
            }
            k += 1;
        }
        clen += comp.len();
        match cur.get(..clen) {
            Some(seen) => {
                if depth(seen) > DEPTH_MAX {
                    return Err(PW_EDEPTH);
                }
            }
            None => return Err(PW_EBUF),
        }
        if mode != MODE_P {
            continue;
        }
        loop {
            let mut tmp = [0u8; CAP];
            let hit = match cur.get(..clen) {
                Some(seen) => links.find(seen, &mut tmp),
                None => return Err(PW_EBUF),
            };
            let n = match hit {
                Some(v) => v,
                None => break,
            };
            *hops += 1;
            if *hops > HOPS_MAX {
                return Err(PW_ELOOP);
            }
            let mut k2 = 0usize;
            while k2 < n {
                if let (Some(d), Some(s)) = (cur.get_mut(k2), tmp.get(k2)) {
                    *d = *s;
                }
                k2 += 1;
            }
            clen = n;
            match cur.get(..clen) {
                Some(seen) => {
                    if depth(seen) > DEPTH_MAX {
                        return Err(PW_EDEPTH);
                    }
                }
                None => return Err(PW_EBUF),
            }
        }
    }
    Ok(clen)
}

/// 把 cwd + arg 走成一条规范路径。
pub fn canon(
    cwd: &[u8],
    arg: &[u8],
    mode: u8,
    links: &Links,
    dst: &mut [u8],
) -> Result<u32, u32> {
    if mode != MODE_L && mode != MODE_P {
        return Err(PW_EOPT);
    }
    if !normalized(cwd) {
        return Err(PW_EBADPATH);
    }
    let mut cur = [0u8; CAP];
    if let Some(c) = cur.get_mut(0) {
        *c = b'/';
    }
    let mut clen = 1usize;
    let mut hops = 0u32;
    let absolute = arg.get(0).copied() == Some(b'/');
    if !absolute {
        clen = feed(&mut cur, clen, cwd, mode, links, &mut hops)?;
    }
    clen = feed(&mut cur, clen, arg, mode, links, &mut hops)?;
    if clen > dst.len() {
        return Err(PW_EBUF);
    }
    let mut i = 0usize;
    while i < clen {
        if let (Some(d), Some(s)) = (dst.get_mut(i), cur.get(i)) {
            *d = *s;
        }
        i += 1;
    }
    Ok(clen as u32)
}

/// `pwd` 本体：-P 直接物理，-L 按 PWD 可信规则。
pub fn pwd(
    mode: u8,
    pwd_env: &[u8],
    cwd: &[u8],
    links: &Links,
    dst: &mut [u8],
) -> Result<u32, u32> {
    if mode != MODE_L && mode != MODE_P {
        return Err(PW_EOPT);
    }
    if !normalized(cwd) {
        return Err(PW_EBADPATH);
    }
    if mode == MODE_P || pwd_env.is_empty() || !normalized(pwd_env) {
        return canon(cwd, b"", MODE_P, links, dst);
    }
    let mut a = [0u8; CAP];
    let mut b = [0u8; CAP];
    let pa = canon(pwd_env, b"", MODE_P, links, &mut a)? as usize;
    let pb = canon(cwd, b"", MODE_P, links, &mut b)? as usize;
    let same = match (a.get(..pa), b.get(..pb)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    };
    if !same {
        return canon(cwd, b"", MODE_P, links, dst);
    }
    if pwd_env.len() > dst.len() {
        return Err(PW_EBUF);
    }
    copy(dst, pwd_env);
    Ok(pwd_env.len() as u32)
}

/// 解析模式选项。
pub fn parse_mode(args: &[&[u8]]) -> Result<u8, u32> {
    let mut mode = MODE_L;
    let mut seen_l = false;
    let mut seen_p = false;
    let mut i = 0usize;
    while i < args.len() {
        let a: &[u8] = match args.get(i) {
            Some(v) => *v,
            None => break,
        };
        if a == b"-L" || a == b"--logical" {
            if seen_p {
                return Err(PW_EBOTH);
            }
            seen_l = true;
            mode = MODE_L;
        } else if a == b"-P" || a == b"--physical" {
            if seen_l {
                return Err(PW_EBOTH);
            }
            seen_p = true;
            mode = MODE_P;
        } else if a.get(0).copied() == Some(b'-') {
            return Err(PW_EOPT);
        } else {
            return Err(PW_EOPERAND);
        }
        i += 1;
    }
    Ok(mode)
}

/// 本卡用的链接表。
pub fn sample_links() -> Links {
    let mut t = Links::new();
    let _ = t.add(b"/home/user/link", b"/opt/app");
    let _ = t.add(b"/opt/app/bin", b"/opt/lib");
    let _ = t.add(b"/home/user/src", b"/srv/src");
    let _ = t.add(b"/loop/a", b"/loop/b");
    let _ = t.add(b"/loop/b", b"/loop/a");
    t
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
            return Err(PW_ESLOT);
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
            return Err(PW_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 14, 8, 11, 8, 14, 8];
        const CODES: [u32; 6] =
            [PW_EOPT, PW_EBOTH, PW_ELOOP, PW_EDEPTH, PW_EBUF, PW_EBADPATH];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(PW_EBAL),
            };
            if s.slot as usize != i {
                return Err(PW_ESLOT);
            }
            if s.op == 0 {
                return Err(PW_EVALUE);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(PW_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(PW_EVALUE);
                        }
                    }
                    None => return Err(PW_EBAL),
                }
                vi += 1;
            } else {
                if s.value != 0 {
                    return Err(PW_EVALUE);
                }
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(PW_EVALUE);
                        }
                    }
                    None => return Err(PW_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(PW_EBAL);
        }
        Ok(())
    }

    /// 指纹。
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
            return Err(PW_EBAL);
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
            Err(PW_ESEAL)
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

/// 十四格账本：八个成功观测 + 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let links = sample_links();
    let mut out = [0u8; CAP];

    let _ = l.register(word_cell(0, OP_MODE, parse_mode(&[]).map(|m| m as u32)));
    let lflag: &[u8] = b"-L";
    let pflag: &[u8] = b"-P";
    let _ = l.register(word_cell(1, OP_MODE, parse_mode(&[pflag]).map(|m| m as u32)));
    let _ = l.register(word_cell(
        2,
        OP_CANON,
        canon(b"/home/user", b"src", MODE_L, &links, &mut out),
    ));
    let _ = l.register(word_cell(
        3,
        OP_CANON,
        canon(b"/home/user", b"link", MODE_P, &links, &mut out),
    ));
    let _ = l.register(word_cell(
        4,
        OP_CANON,
        canon(b"/home/user", b"..//doc/./a", MODE_L, &links, &mut out),
    ));
    let _ = l.register(word_cell(
        5,
        OP_CANON,
        canon(b"/home/user", b"link/bin", MODE_P, &links, &mut out),
    ));
    let _ = l.register(word_cell(
        6,
        OP_PWD,
        pwd(MODE_L, b"/home/user/src", b"/home/user/src", &links, &mut out),
    ));
    let _ = l.register(word_cell(
        7,
        OP_PWD,
        pwd(MODE_P, b"/home/user/src", b"/home/user/src", &links, &mut out),
    ));

    let xflag: &[u8] = b"-x";
    let _ = l.register(word_cell(8, OP_MODE, parse_mode(&[xflag]).map(|m| m as u32)));
    let _ = l.register(word_cell(9, OP_MODE, parse_mode(&[lflag, pflag]).map(|m| m as u32)));
    let _ = l.register(word_cell(
        10,
        OP_CANON,
        canon(b"/loop/a", b"", MODE_P, &links, &mut out),
    ));

    let mut dbuf = [0u8; CAP];
    let deep = match deep_path(&mut dbuf, DEPTH_MAX + 1) {
        Ok(v) => v,
        Err(c) => {
            let _ = l.register(word_cell(11, OP_CANON, Err(c)));
            return l;
        }
    };
    let _ = l.register(word_cell(11, OP_CANON, canon(deep, b"", MODE_L, &links, &mut out)));

    let mut tiny = [0u8; 4];
    let _ = l.register(word_cell(
        12,
        OP_CANON,
        canon(b"/home/user", b"src", MODE_P, &links, &mut tiny),
    ));
    let _ = l.register(word_cell(
        13,
        OP_CANON,
        canon(b"home", b"src", MODE_L, &links, &mut out),
    ));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：逻辑模式只折字符串。
    let links = sample_links();
    let mut a = [0u8; CAP];
    let mut b = [0u8; CAP];
    let mut c = [0u8; CAP];
    let r1 = canon(b"/home/user", b"..//doc/./a", MODE_L, &links, &mut a);
    let r2 = canon(b"/", b"a/./b/../c", MODE_L, &links, &mut b);
    let r3 = canon(b"/home/user", b"../../..", MODE_L, &links, &mut c);
    out[1] = r1 == Ok(11)
        && a.get(..11) == Some(&b"/home/doc/a"[..])
        && r2 == Ok(4)
        && b.get(..4) == Some(&b"/a/c"[..])
        && r3 == Ok(1)
        && c.get(..1) == Some(&b"/"[..]);

    // 二路：物理模式逐段解链接。
    let mut d = [0u8; CAP];
    let mut e = [0u8; CAP];
    let mut f = [0u8; CAP];
    let r4 = canon(b"/home/user", b"link", MODE_P, &links, &mut d);
    let r5 = canon(b"/home/user", b"link/bin", MODE_P, &links, &mut e);
    let r6 = canon(b"/", b"home/user/src", MODE_P, &links, &mut f);
    out[2] = r4 == Ok(8)
        && d.get(..8) == Some(&b"/opt/app"[..])
        && r5 == Ok(8)
        && e.get(..8) == Some(&b"/opt/lib"[..])
        && r6 == Ok(8)
        && f.get(..8) == Some(&b"/srv/src"[..]);

    // 三路：PWD 可信与否、各类拒绝。
    let mut g = [0u8; CAP];
    let mut h = [0u8; CAP];
    let ltrust = pwd(MODE_L, b"/home/user/src", b"/home/user/src", &links, &mut g);
    let ptrust = pwd(MODE_P, b"/home/user/src", b"/home/user/src", &links, &mut h);
    let lflag: &[u8] = b"-L";
    let pflag: &[u8] = b"-P";
    let xflag: &[u8] = b"-x";
    let bare: &[u8] = b"src";
    let mut dbuf = [0u8; CAP];
    let deep = match deep_path(&mut dbuf, DEPTH_MAX + 1) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let mut tiny = [0u8; 4];
    out[3] = ltrust == Ok(14)
        && g.get(..14) == Some(&b"/home/user/src"[..])
        && ptrust == Ok(8)
        && h.get(..8) == Some(&b"/srv/src"[..])
        && parse_mode(&[lflag, pflag]) == Err(PW_EBOTH)
        && parse_mode(&[xflag]) == Err(PW_EOPT)
        && parse_mode(&[bare]) == Err(PW_EOPERAND)
        && canon(b"/loop/a", b"", MODE_P, &links, &mut tiny) == Err(PW_ELOOP)
        && canon(deep, b"", MODE_L, &links, &mut tiny) == Err(PW_EDEPTH)
        && canon(b"/home/user", b"src", MODE_P, &links, &mut tiny) == Err(PW_EBUF)
        && canon(b"home", b"src", MODE_L, &links, &mut tiny) == Err(PW_EBADPATH);

    out
}

/// 十三项判据。
pub fn run_unx11508_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11508.pwd_mode");
    let l = build_ledger();
    let links = sample_links();

    s.add_dyn(
        "PW01-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "PW02-默认逻辑模式",
        {
            let lf: &[u8] = b"-L";
            let ll: &[u8] = b"--logical";
            parse_mode(&[]) == Ok(MODE_L)
                && parse_mode(&[lf]) == Ok(MODE_L)
                && parse_mode(&[ll]) == Ok(MODE_L)
                && MODE_L == 1
                && l.sample(0).map(|x| x.value) == Some(1)
        },
        "static",
    );

    s.add(
        "PW03-双模式互斥",
        {
            let lf: &[u8] = b"-L";
            let pf: &[u8] = b"-P";
            let ll: &[u8] = b"--logical";
            let pl: &[u8] = b"--physical";
            parse_mode(&[pf]) == Ok(MODE_P)
                && parse_mode(&[pl]) == Ok(MODE_P)
                && parse_mode(&[lf, pf]) == Err(PW_EBOTH)
                && parse_mode(&[pf, lf]) == Err(PW_EBOTH)
                && parse_mode(&[ll, pl]) == Err(PW_EBOTH)
                && parse_mode(&[lf, lf]) == Ok(MODE_L)
                && parse_mode(&[pf, pf]) == Ok(MODE_P)
                && l.sample(1).map(|x| x.value) == Some(2)
        },
        "static",
    );

    s.add(
        "PW04-逻辑去点折斜",
        {
            let mut a = [0u8; CAP];
            let mut b = [0u8; CAP];
            let mut c = [0u8; CAP];
            canon(b"/home/user", b"..//doc/./a", MODE_L, &links, &mut a) == Ok(11)
                && a.get(..11) == Some(&b"/home/doc/a"[..])
                && canon(b"/", b"a/./b/../c", MODE_L, &links, &mut b) == Ok(4)
                && b.get(..4) == Some(&b"/a/c"[..])
                && canon(b"/home/user", b"../../..", MODE_L, &links, &mut c) == Ok(1)
                && c.get(..1) == Some(&b"/"[..])
                && l.sample(4).map(|x| x.value) == Some(11)
        },
        "static",
    );

    s.add(
        "PW05-物理解单链接",
        {
            let mut a = [0u8; CAP];
            let mut b = [0u8; CAP];
            canon(b"/home/user", b"link", MODE_P, &links, &mut a) == Ok(8)
                && a.get(..8) == Some(&b"/opt/app"[..])
                && canon(b"/home/user", b"link", MODE_L, &links, &mut b) == Ok(15)
                && b.get(..15) == Some(&b"/home/user/link"[..])
                && l.sample(3).map(|x| x.value) == Some(8)
        },
        "static",
    );

    s.add(
        "PW06-嵌套链接展开",
        {
            let mut a = [0u8; CAP];
            let mut b = [0u8; CAP];
            canon(b"/home/user", b"link/bin", MODE_P, &links, &mut a) == Ok(8)
                && a.get(..8) == Some(&b"/opt/lib"[..])
                && canon(b"/home/user", b"link/bin", MODE_L, &links, &mut b) == Ok(19)
                && b.get(..19) == Some(&b"/home/user/link/bin"[..])
                && l.sample(5).map(|x| x.value) == Some(8)
        },
        "static",
    );

    s.add(
        "PW07-逻辑采信PWD",
        {
            let mut a = [0u8; CAP];
            let mut b = [0u8; CAP];
            pwd(MODE_L, b"/home/user/src", b"/home/user/src", &links, &mut a) == Ok(14)
                && a.get(..14) == Some(&b"/home/user/src"[..])
                && pwd(MODE_L, b"/home/user", b"/home/user/src", &links, &mut b) == Ok(8)
                && b.get(..8) == Some(&b"/srv/src"[..])
                && l.sample(6).map(|x| x.value) == Some(14)
        },
        "static",
    );

    s.add(
        "PW08-物理忽略PWD",
        {
            let mut a = [0u8; CAP];
            let mut b = [0u8; CAP];
            pwd(MODE_P, b"/home/user/src", b"/home/user/src", &links, &mut a) == Ok(8)
                && a.get(..8) == Some(&b"/srv/src"[..])
                && pwd(MODE_P, b"", b"/home/user", &links, &mut b) == Ok(10)
                && b.get(..10) == Some(&b"/home/user"[..])
                && l.sample(7).map(|x| x.value) == Some(8)
        },
        "static",
    );

    s.add(
        "PW09-链接环被拒",
        {
            let mut a = [0u8; CAP];
            canon(b"/loop/a", b"", MODE_P, &links, &mut a) == Err(PW_ELOOP)
                && canon(b"/loop", b"a", MODE_P, &links, &mut a) == Err(PW_ELOOP)
                && canon(b"/loop/a", b"", MODE_L, &links, &mut a) == Ok(7)
                && l.sample(10).map(|x| x.code) == Some(PW_ELOOP)
        },
        "static",
    );

    s.add(
        "PW10-深度与分量上界",
        {
            let mut b1 = [0u8; CAP];
            let mut b2 = [0u8; CAP];
            let mut out = [0u8; CAP];
            let six = match deep_path(&mut b1, DEPTH_MAX) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let over = match deep_path(&mut b2, DEPTH_MAX + 1) {
                Ok(v) => v,
                Err(_) => return s,
            };
            canon(six, b"", MODE_L, &links, &mut out) == Ok(32)
                && canon(over, b"", MODE_L, &links, &mut out) == Err(PW_EDEPTH)
                && canon(b"/", b"aaaaaaaaaaaaaaaaa", MODE_L, &links, &mut out)
                    == Err(PW_EBADNAME)
                && l.sample(11).map(|x| x.code) == Some(PW_EDEPTH)
        },
        "static",
    );

    s.add(
        "PW11-缓冲与相对路径",
        {
            let mut exact = [0u8; 14];
            let mut short = [0u8; 13];
            canon(b"/home/user", b"src", MODE_L, &links, &mut exact) == Ok(14)
                && exact.get(..14) == Some(&b"/home/user/src"[..])
                && canon(b"/home/user", b"src", MODE_L, &links, &mut short) == Err(PW_EBUF)
                && canon(b"home/user", b"src", MODE_L, &links, &mut exact) == Err(PW_EBADPATH)
                && canon(b"/home", b"user/../../..", MODE_L, &links, &mut exact) == Ok(1)
                && l.sample(12).map(|x| x.code) == Some(PW_EBUF)
        },
        "static",
    );

    s.add(
        "PW12-选项表闸门",
        {
            let lf: &[u8] = b"-L";
            let pf: &[u8] = b"-P";
            let xf: &[u8] = b"-x";
            let dash: &[u8] = b"--";
            let bare: &[u8] = b"src";
            parse_mode(&[xf]) == Err(PW_EOPT)
                && parse_mode(&[dash]) == Err(PW_EOPT)
                && parse_mode(&[bare]) == Err(PW_EOPERAND)
                && parse_mode(&[lf, lf, pf]) == Err(PW_EBOTH)
                && l.sample(8).map(|x| x.code) == Some(PW_EOPT)
                && l.sample(9).map(|x| x.code) == Some(PW_EBOTH)
        },
        "static",
    );

    s.add(
        "PW13-冻结重放四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two.is_err()
                && f.is_frozen()
                && f.root() != 0
                && f.rejects() == 1
                && inj == [true; 4]
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
        assert_eq!(l.rejects(), 0);
        assert!(l.audit().is_ok());
        let mut i = 0usize;
        while i < l.len() {
            assert_eq!(l.sample(i).map(|x| x.slot as usize), Some(i));
            i += 1;
        }
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11508_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn mode_parsing() {
        let lf: &[u8] = b"-L";
        let pf: &[u8] = b"-P";
        let ll: &[u8] = b"--logical";
        let pl: &[u8] = b"--physical";
        let xf: &[u8] = b"-x";
        let bare: &[u8] = b"x";
        assert_eq!(parse_mode(&[]), Ok(MODE_L));
        assert_eq!(parse_mode(&[lf]), Ok(MODE_L));
        assert_eq!(parse_mode(&[pf]), Ok(MODE_P));
        assert_eq!(parse_mode(&[ll]), Ok(MODE_L));
        assert_eq!(parse_mode(&[pl]), Ok(MODE_P));
        assert_eq!(parse_mode(&[lf, pf]), Err(PW_EBOTH));
        assert_eq!(parse_mode(&[xf]), Err(PW_EOPT));
        assert_eq!(parse_mode(&[bare]), Err(PW_EOPERAND));
    }

    #[test]
    fn logical_walk() {
        let links = sample_links();
        let mut b = [0u8; CAP];
        assert_eq!(canon(b"/home/user", b"..//doc/./a", MODE_L, &links, &mut b), Ok(11));
        assert_eq!(&b[..11], b"/home/doc/a");
        assert_eq!(canon(b"/", b"a/./b/../c", MODE_L, &links, &mut b), Ok(4));
        assert_eq!(&b[..4], b"/a/c");
        assert_eq!(canon(b"/home/user", b"../../..", MODE_L, &links, &mut b), Ok(1));
        assert_eq!(&b[..1], b"/");
    }

    #[test]
    fn physical_links() {
        let links = sample_links();
        let mut b = [0u8; CAP];
        assert_eq!(canon(b"/home/user", b"link", MODE_P, &links, &mut b), Ok(8));
        assert_eq!(&b[..8], b"/opt/app");
        assert_eq!(canon(b"/home/user", b"link/bin", MODE_P, &links, &mut b), Ok(8));
        assert_eq!(&b[..8], b"/opt/lib");
        assert_eq!(canon(b"/", b"home/user/src", MODE_P, &links, &mut b), Ok(8));
        assert_eq!(&b[..8], b"/srv/src");
        assert_eq!(canon(b"/home/user", b"link/..", MODE_P, &links, &mut b), Ok(4));
        assert_eq!(&b[..4], b"/opt");
    }

    #[test]
    fn pwd_trust() {
        let links = sample_links();
        let mut b = [0u8; CAP];
        assert_eq!(
            pwd(MODE_L, b"/home/user/src", b"/home/user/src", &links, &mut b),
            Ok(14)
        );
        assert_eq!(&b[..14], b"/home/user/src");
        assert_eq!(
            pwd(MODE_P, b"/home/user/src", b"/home/user/src", &links, &mut b),
            Ok(8)
        );
        assert_eq!(&b[..8], b"/srv/src");
        assert_eq!(
            pwd(MODE_L, b"/home/user", b"/home/user/src", &links, &mut b),
            Ok(8)
        );
    }

    #[test]
    fn loops_depth_buffers() {
        let links = sample_links();
        let mut b = [0u8; CAP];
        assert_eq!(canon(b"/loop/a", b"", MODE_P, &links, &mut b), Err(PW_ELOOP));
        let mut deep = [0u8; CAP];
        let path = match deep_path(&mut deep, DEPTH_MAX + 1) {
            Ok(v) => v,
            Err(_) => panic!("deep"),
        };
        assert_eq!(canon(path, b"", MODE_L, &links, &mut b), Err(PW_EDEPTH));
        let mut short = [0u8; 4];
        assert_eq!(
            canon(b"/home/user", b"src", MODE_P, &links, &mut short),
            Err(PW_EBUF)
        );
        assert_eq!(
            canon(b"home", b"src", MODE_L, &links, &mut short),
            Err(PW_EBADPATH)
        );
        assert_eq!(
            canon(b"/", b"aaaaaaaaaaaaaaaaa", MODE_L, &links, &mut b),
            Err(PW_EBADNAME)
        );
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
