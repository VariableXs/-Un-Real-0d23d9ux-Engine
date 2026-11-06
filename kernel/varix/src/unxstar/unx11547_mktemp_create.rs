//! UNX-F11547 mktemp 临时件安全创建
//!
//! `mktemp` 的全部安全性来自一个不变式：**名字先随机生成，创建必须是原子的
//! 独占创建**。名字里带随机位只是为了让冲突概率低，真正防止两个进程拿到同一
//! 个临时件的，是「创建时若已存在就失败」这一条。本模块把这个不变式拆成三层，
//! 每层都可以单独观测：
//!
//! 1. **模板层**（`Spec`）：模板是「目录 + 词干 + 占位串 + 后缀」。占位串必须
//!    是名字末尾一段连续的 `X`，且至少三位。少于三位时搜索空间太小——在共享
//!    目录里三位随机名是会被抢注的，GNU 与 BSD 都拒绝。
//! 2. **生成层**（`Gen`）：随机位由确定性伪随机流填充，同一个种子必然给出同一
//!    串。可确定性不是实现偷懒，而是为了让「同一种子的重放」能验证账本；真机
//!    上种子来自系统熵源，这一层不关心熵从哪来。
//! 3. **台账层**（`Bench`）：台账记录已创建的临时件。一次创建会消耗至多
//!    `TRYMAX` 个候选名，每撞一次名记一次重试；耗尽仍是撞名，就整体失败并报
//!    「候选名全冲突」，绝不返回一个未经确认的名字。
//!
//! 三个容易被忽略的语义，本模块都单独落地：
//!
//! - **后缀在占位串之后**：`--suffix .tmp` 不会把 `.tmp` 算进占位串的尾部，
//!   否则 `fileXXXXXX.tmp` 里末尾的 `X` 就不再是尾部的了——解析时会先按模板
//!   校验占位串，再把后缀拼到生成结果上。
//! - **预览不消耗随机流**：`--dry-run` 只打印名字，不创建、也不应推进生成器；
//!   若推进了，同一次运行里「预览一个名字，再创建一个」会得到两个不同的名字，
//!   与用户读到的输出不符。
//! - **删除即回收槽位**：台账槽位是连续数组，删除必须把后面整体前移；留下空洞
//!   会让「槽位数」与「实际件数」分叉。
//!
//! 闸门次序：
//!
//! - `Spec::parse`：空模板 → 超长 → 非法字节 → 名字为空 → 名字超长 → 目录超长
//!   → 无 `X` → `X` 不在尾部 → `X` 太少 → `X` 太多；
//! - `Spec::with_suffix`：非法字节 → 出现路径分隔符 → 总长超限；
//! - `Bench::create_with`：模式非法 → 台账已满 → 占位串过短 → 候选撞名（重试）
//!   → 重试耗尽。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf340;

/// 模板为空或名字为占位符。
pub const MK_EINVAL: u32 = SEG_BASE + 0x01;
/// 占位串少于三位。
pub const MK_ESHORT: u32 = SEG_BASE + 0x02;
/// 占位串不在名字尾部。
pub const MK_ETAIL: u32 = SEG_BASE + 0x03;
/// 模板或后缀含非法字节。
pub const MK_EBYTE: u32 = SEG_BASE + 0x04;
/// 模板、目录、名字或后缀超长。
pub const MK_ELEN: u32 = SEG_BASE + 0x05;
/// 候选名全部撞名。
pub const MK_EXIST: u32 = SEG_BASE + 0x06;
/// 台账已满。
pub const MK_EFULL: u32 = SEG_BASE + 0x07;
/// 创建模式非法。
pub const MK_EMODE: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const MK_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const MK_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const MK_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const MK_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const MK_ENOBS: u32 = SEG_BASE + 0x0d;

/// 模板串上限。
pub const TMPLMAX: usize = 96;
/// 目录段上限。
pub const DIRMAX: usize = 48;
/// 生成的名字上限。
pub const NAMEMAX: usize = 128;
/// 占位串下限。
pub const XSMIN: usize = 3;
/// 占位串上限。
pub const XSMAX: usize = 12;
/// 单次创建的候选重试上限。
pub const TRYMAX: u32 = 24;
/// 台账容量。
pub const CAP: usize = 6;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 普通临时件模式：仅属主可读写。
pub const MODE_FILE: u32 = 0o600;
/// 临时目录模式：仅属主可进入。
pub const MODE_DIR: u32 = 0o700;

/// 随机位字符集。
pub const ALPHA: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// 操作码：解析模板。
pub const OP_PARSE: u8 = 1;
/// 操作码：创建。
pub const OP_CREATE: u8 = 2;
/// 操作码：统计。
pub const OP_STAT: u8 = 3;
/// 操作码：预览。
pub const OP_PREVIEW: u8 = 4;
/// 操作码：删除。
pub const OP_REMOVE: u8 = 5;

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

/// 是否可打印 ASCII。模板只接受可见字符，控制字符会让生成的路径名不可读，
/// 也会让错误信息本身难以呈现。
pub fn is_printable(b: u8) -> bool {
    b >= 0x20 && b <= 0x7e
}

/// 一段字节是否全部可打印。
pub fn all_printable(s: &[u8]) -> bool {
    let mut i = 0usize;
    while i < s.len() {
        match s.get(i) {
            Some(b) => {
                if !is_printable(*b) {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 名字末尾连续 `X` 的个数。
pub fn tail_xs(name: &[u8]) -> usize {
    let mut n = 0usize;
    let mut i = name.len();
    while i > 0 {
        i -= 1;
        match name.get(i) {
            Some(b'X') => n += 1,
            _ => return n,
        }
    }
    n
}

/// 名字里 `X` 的总个数。与 [`tail_xs`] 相等才说明占位串是尾部唯一的一段。
pub fn total_xs(name: &[u8]) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < name.len() {
        match name.get(i) {
            Some(b'X') => n += 1,
            _ => {}
        }
        i += 1;
    }
    n
}

/// 名字是否含路径分隔符。
pub fn has_slash(s: &[u8]) -> bool {
    let mut i = 0usize;
    while i < s.len() {
        match s.get(i) {
            Some(b'/') => return true,
            _ => {}
        }
        i += 1;
    }
    false
}

/// 确定性伪随机流。
///
/// 状态推进用 PCG 常数（同一组常数在无符号 64 位下周期为 2^64），取高位作为
/// 字符下标：低位在高阶线性同余里质量差，直接取低位会让相邻名字看起来成串。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gen {
    state: u64,
    alen: usize,
}

impl Gen {
    /// 默认字符集生成器。
    pub const fn new(seed: u64) -> Gen {
        Gen { state: seed, alen: ALPHA.len() }
    }

    /// 窄字符集生成器：只取字符集前 `alen` 个字符。
    ///
    /// 这是为了能**构造**撞名——`alen == 1` 时每次填充都是同一个字符，第 25 次
    /// 候选必然撞名。撞名路径无法靠真随机测到，必须能被确定性地打到。
    pub const fn narrow(seed: u64, alen: usize) -> Gen {
        let a = if alen == 0 {
            1
        } else if alen > ALPHA.len() {
            ALPHA.len()
        } else {
            alen
        };
        Gen { state: seed, alen: a }
    }

    /// 当前状态。
    pub fn peek(&self) -> u64 {
        self.state
    }

    /// 字符集宽度。
    pub fn width(&self) -> usize {
        self.alen
    }

    /// 推进一步并返回新状态。
    pub fn step(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(0x5851_f42d_4c95_7f2d)
            .wrapping_add(0x1405_7b7e_f767_814f);
        self.state
    }

    /// 取一个随机字符。
    pub fn pick(&mut self) -> u8 {
        let x = self.step();
        let idx = (x >> 33) as usize % self.alen;
        match ALPHA.get(idx) {
            Some(b) => *b,
            None => b'x',
        }
    }

    /// 填充 `n` 个随机字符。
    pub fn fill(&mut self, n: usize) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < n {
            s.push(self.pick() as char);
            i += 1;
        }
        s
    }
}

/// 解析后的模板。
///
/// 目录与后缀都存成 `String`，因为它们要原样拼进结果名；词干单独存，是为了让
/// 「占位串长度」成为 `Spec` 的显式字段，而不是每次从名字尾部重新数。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spec {
    dir: String,
    stem: String,
    xrun: usize,
    suffix: String,
}

impl Spec {
    /// 空模板。
    pub fn empty() -> Spec {
        Spec { dir: String::new(), stem: String::new(), xrun: 0, suffix: String::new() }
    }

    /// 解析模板。
    pub fn parse(t: &[u8]) -> Result<Spec, u32> {
        if t.is_empty() {
            return Err(MK_EINVAL);
        }
        if t.len() > TMPLMAX {
            return Err(MK_ELEN);
        }
        if !all_printable(t) {
            return Err(MK_EBYTE);
        }

        let mut cut = t.len();
        let mut j = t.len();
        while j > 0 {
            j -= 1;
            match t.get(j) {
                Some(b'/') => {
                    cut = j;
                    break;
                }
                _ => {}
            }
        }

        let (dir_part, name_part): (&[u8], &[u8]) = if cut == t.len() {
            (b"", t)
        } else if cut == 0 {
            (b"/", t.get(1..).unwrap_or(b""))
        } else {
            (t.get(..cut).unwrap_or(b""), t.get(cut + 1..).unwrap_or(b""))
        };

        if name_part.is_empty() {
            return Err(MK_EINVAL);
        }
        if name_part.len() > NAMEMAX {
            return Err(MK_ELEN);
        }
        if dir_part.len() > DIRMAX {
            return Err(MK_ELEN);
        }

        let tail = tail_xs(name_part);
        let total = total_xs(name_part);
        if total == 0 {
            return Err(MK_EINVAL);
        }
        if total != tail {
            return Err(MK_ETAIL);
        }
        if total < XSMIN {
            return Err(MK_ESHORT);
        }
        if total > XSMAX {
            return Err(MK_ELEN);
        }

        let stem_end = name_part.len() - tail;
        let stem = match name_part.get(..stem_end) {
            Some(v) => v,
            None => return Err(MK_EINVAL),
        };

        Ok(Spec {
            dir: bytes_to_string(dir_part),
            stem: bytes_to_string(stem),
            xrun: tail,
            suffix: String::new(),
        })
    }

    /// 目录段。
    pub fn dir(&self) -> &str {
        self.dir.as_str()
    }

    /// 词干。
    pub fn stem(&self) -> &str {
        self.stem.as_str()
    }

    /// 占位串长度。
    pub fn xrun(&self) -> usize {
        self.xrun
    }

    /// 后缀。
    pub fn suffix(&self) -> &str {
        self.suffix.as_str()
    }

    /// 生成名的长度（不含目录）。
    pub fn name_len(&self) -> usize {
        self.stem.len() + self.xrun + self.suffix.len()
    }

    /// 生成名的总长度（含目录与分隔符）。
    pub fn total_len(&self) -> usize {
        let mut n = self.name_len();
        if !self.dir.is_empty() {
            n += self.dir.len();
            if !dir_has_slash(&self.dir) {
                n += 1;
            }
        }
        n
    }

    /// 换目录。
    pub fn in_dir(self, d: &[u8]) -> Result<Spec, u32> {
        if !all_printable(d) {
            return Err(MK_EBYTE);
        }
        if d.len() > DIRMAX {
            return Err(MK_ELEN);
        }
        Ok(Spec { dir: bytes_to_string(d), stem: self.stem, xrun: self.xrun, suffix: self.suffix })
    }

    /// 加后缀。后缀排在占位串之后，因此不参与「尾部必须是 `X`」的校验。
    pub fn with_suffix(self, sfx: &[u8]) -> Result<Spec, u32> {
        if !all_printable(sfx) {
            return Err(MK_EBYTE);
        }
        if has_slash(sfx) {
            return Err(MK_EINVAL);
        }
        let probe = Spec {
            dir: self.dir,
            stem: self.stem,
            xrun: self.xrun,
            suffix: String::new(),
        };
        if probe.name_len() + sfx.len() > NAMEMAX {
            return Err(MK_ELEN);
        }
        Ok(Spec { dir: probe.dir, stem: probe.stem, xrun: probe.xrun, suffix: bytes_to_string(sfx) })
    }

    /// 用给定的随机位渲染名字。随机位长度必须与占位串长度相等——这是调用方
    /// 与模板之间的契约，少一位就会生成一个搜索空间更小的名字。
    pub fn render(&self, fill: &str) -> Result<String, u32> {
        if fill.len() != self.xrun {
            return Err(MK_EINVAL);
        }
        let mut s = String::new();
        if !self.dir.is_empty() {
            s.push_str(&self.dir);
            if !dir_has_slash(&self.dir) {
                s.push('/');
            }
        }
        s.push_str(&self.stem);
        s.push_str(fill);
        s.push_str(&self.suffix);
        if s.len() > NAMEMAX + DIRMAX + 2 {
            return Err(MK_ELEN);
        }
        Ok(s)
    }
}

/// 目录段是否已带结尾分隔符。
fn dir_has_slash(d: &String) -> bool {
    match d.as_bytes().last() {
        Some(b'/') => true,
        _ => false,
    }
}

/// 字节切片转 `String`（模板已保证全部可打印 ASCII）。
fn bytes_to_string(s: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while i < s.len() {
        match s.get(i) {
            Some(b) => out.push(*b as char),
            None => break,
        }
        i += 1;
    }
    out
}

/// 台账里的一件临时件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Item {
    name: [u8; NAMEMAX],
    nlen: u8,
    mode: u32,
}

impl Item {
    /// 空格。
    pub const fn blank() -> Item {
        Item { name: [0; NAMEMAX], nlen: 0, mode: MODE_FILE }
    }

    /// 名字字节。
    pub fn name(&self) -> &[u8] {
        self.name.get(..self.nlen as usize).unwrap_or(&[])
    }

    /// 名字是否等于 `other`。
    pub fn name_is(&self, other: &[u8]) -> bool {
        self.name() == other
    }

    /// 名字长度。
    pub fn len(&self) -> usize {
        self.nlen as usize
    }

    /// 是否空名。
    pub fn is_empty(&self) -> bool {
        self.nlen == 0
    }

    /// 模式。
    pub fn mode(&self) -> u32 {
        self.mode
    }
}

/// 临时件台账。
///
/// 台账只记「已经创建成功」的名字：撞名失败的名字不登记，也不占用槽位——它从未
/// 存在过，登记它会把失败伪装成部分成功。
#[derive(Clone, Copy, Debug)]
pub struct Bench {
    items: [Item; CAP],
    n: usize,
    attempts: u32,
    retries: u32,
    dirs: u32,
    rejects: u32,
}

impl Bench {
    /// 空台账。
    pub const fn new() -> Bench {
        Bench { items: [Item::blank(); CAP], n: 0, attempts: 0, retries: 0, dirs: 0, rejects: 0 }
    }

    /// 件数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否空台账。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 候选尝试总次数。
    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    /// 撞名重试次数。
    pub fn retries(&self) -> u32 {
        self.retries
    }

    /// 已创建目录数。
    pub fn dirs(&self) -> u32 {
        self.dirs
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 第 i 件。
    pub fn item(&self, i: usize) -> Option<Item> {
        self.items.get(i).copied()
    }

    /// 第 i 件的名字长度。
    pub fn name_len(&self, i: usize) -> Option<usize> {
        match self.items.get(i) {
            Some(v) => Some(v.len()),
            None => None,
        }
    }

    /// 第 i 件是否叫 `other`。
    pub fn name_is(&self, i: usize, other: &[u8]) -> Option<bool> {
        match self.items.get(i) {
            Some(v) => Some(v.name_is(other)),
            None => None,
        }
    }

    /// 名字是否存在。
    pub fn contains(&self, name: &[u8]) -> bool {
        self.index_of(name).is_some()
    }

    /// 名字所在槽位。
    pub fn index_of(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            match self.items.get(i) {
                Some(v) => {
                    if v.name_is(name) {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 名字的模式。
    pub fn mode_of(&self, name: &[u8]) -> Option<u32> {
        match self.index_of(name) {
            Some(i) => match self.items.get(i) {
                Some(v) => Some(v.mode()),
                None => None,
            },
            None => None,
        }
    }

    /// 登记一个已确认不存在的名字。
    pub fn push(&mut self, name: &[u8], mode: u32) -> Result<usize, u32> {
        if name.is_empty() || name.len() > NAMEMAX {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MK_EINVAL);
        }
        if mode != MODE_FILE && mode != MODE_DIR {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MK_EMODE);
        }
        if self.n >= CAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MK_EFULL);
        }
        if self.index_of(name).is_some() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MK_EXIST);
        }
        let mut it = Item::blank();
        it.mode = mode;
        let mut i = 0usize;
        while i < name.len() {
            match name.get(i) {
                Some(b) => {
                    if let Some(d) = it.name.get_mut(i) {
                        *d = *b;
                    }
                }
                None => break,
            }
            i += 1;
        }
        it.nlen = name.len() as u8;
        if let Some(dst) = self.items.get_mut(self.n) {
            *dst = it;
        }
        self.n += 1;
        Ok(self.n - 1)
    }

    /// 创建临时件。
    ///
    /// 一次成功创建可能消耗多个候选名：每撞一次名就重填一次随机位。全部候选都
    /// 撞名时报 [`MK_EXIST`]，此时台账不变——这是「要么拿到一个确认独占的名字，
    /// 要么什么都没有」。
    pub fn create_with(&mut self, spec: &Spec, g: &mut Gen, mode: u32) -> Result<usize, u32> {
        if mode != MODE_FILE && mode != MODE_DIR {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MK_EMODE);
        }
        if self.n >= CAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MK_EFULL);
        }
        if spec.xrun() < XSMIN {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MK_ESHORT);
        }
        let mut t = 0u32;
        let mut last = MK_EXIST;
        while t < TRYMAX {
            let fill = g.fill(spec.xrun());
            self.attempts = self.attempts.saturating_add(1);
            t += 1;
            match spec.render(&fill) {
                Ok(name) => {
                    if self.index_of(name.as_bytes()).is_none() {
                        let idx = self.push(name.as_bytes(), mode)?;
                        if mode == MODE_DIR {
                            self.dirs = self.dirs.saturating_add(1);
                        }
                        return Ok(idx);
                    }
                    self.retries = self.retries.saturating_add(1);
                }
                Err(c) => {
                    last = c;
                    break;
                }
            }
        }
        self.rejects = self.rejects.saturating_add(1);
        Err(last)
    }

    /// 以普通文件模式创建。
    pub fn create(&mut self, spec: &Spec, g: &mut Gen) -> Result<usize, u32> {
        self.create_with(spec, g, MODE_FILE)
    }

    /// 以目录模式创建。
    pub fn create_dir(&mut self, spec: &Spec, g: &mut Gen) -> Result<usize, u32> {
        self.create_with(spec, g, MODE_DIR)
    }

    /// 预览下一个名字：不登记、不推进生成器。内部拷贝生成器状态，因此调用方手里
    /// 的 `g` 保持原状。
    pub fn preview(&self, spec: &Spec, g: &Gen) -> Result<String, u32> {
        let mut c = *g;
        let fill = c.fill(spec.xrun());
        spec.render(&fill)
    }

    /// 按下标删除，后面的槽位整体前移。
    pub fn remove_at(&mut self, idx: usize) -> Result<usize, u32> {
        if idx >= self.n {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MK_EINVAL);
        }
        let mut was_dir = false;
        if let Some(v) = self.items.get(idx) {
            was_dir = v.mode() == MODE_DIR;
        }
        let mut k = idx;
        while k + 1 < self.n {
            let next = match self.items.get(k + 1) {
                Some(v) => *v,
                None => break,
            };
            if let Some(dst) = self.items.get_mut(k) {
                *dst = next;
            }
            k += 1;
        }
        if let Some(dst) = self.items.get_mut(self.n - 1) {
            *dst = Item::blank();
        }
        self.n -= 1;
        if was_dir {
            self.dirs = self.dirs.saturating_sub(1);
        }
        Ok(idx)
    }

    /// 按名字删除。
    pub fn remove(&mut self, name: &[u8]) -> Result<usize, u32> {
        match self.index_of(name) {
            Some(i) => self.remove_at(i),
            None => {
                self.rejects = self.rejects.saturating_add(1);
                Err(MK_EINVAL)
            }
        }
    }

    /// 台账自洽：槽位连续、名字非空且互不相同、目录数与目录件数一致。
    pub fn verify(&self) -> bool {
        if self.n > CAP {
            return false;
        }
        let mut dirs = 0u32;
        let mut i = 0usize;
        while i < self.n {
            match self.items.get(i) {
                Some(v) => {
                    if v.is_empty() {
                        return false;
                    }
                    if v.mode() == MODE_DIR {
                        dirs += 1;
                    }
                    let mut k = i + 1;
                    while k < self.n {
                        match self.items.get(k) {
                            Some(w) => {
                                if w.name_is(v.name()) {
                                    return false;
                                }
                            }
                            None => return false,
                        }
                        k += 1;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        dirs == self.dirs
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

/// `Result<usize, u32>` → 样本。
fn count_cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
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

    /// 登记一格。槽位必须严格递增且等于当前格数。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MK_ESLOT);
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

    /// 是否空账本。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        self.rows.get(i).copied()
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.ok {
                        n += 1;
                    }
                }
                None => return n,
            }
            i += 1;
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 审计：形状、槽位顺序、成功/失败值序列。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(MK_EBAL);
        }
        if self.frozen && self.seal() != self.root {
            return Err(MK_ESEAL);
        }
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < SAMPLES {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(MK_ENOBS),
            };
            if s.slot as usize != i {
                return Err(MK_ESLOT);
            }
            if s.op == 0 {
                return Err(MK_EVALUE);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(MK_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if *w != s.value {
                            return Err(MK_EVALUE);
                        }
                    }
                    None => return Err(MK_EBAL),
                }
                vi += 1;
            } else {
                if s.value != 0 {
                    return Err(MK_EVALUE);
                }
                match CODES.get(ci) {
                    Some(w) => {
                        if *w != s.code {
                            return Err(MK_EVALUE);
                        }
                    }
                    None => return Err(MK_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(MK_EBAL);
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
            self.rejects = self.rejects.saturating_add(1);
            return Err(MK_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放：从头构造一份账本，指纹必须与当前一致。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let r = fresh.seal();
        if r == self.seal() {
            Ok(r)
        } else {
            Err(MK_ESEAL)
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

/// 八个成功格的真实观测值。
const VALS: [u32; 8] = [6, 0, 1, 2, 13, 0, 1, 13];

/// 六个失败格的错误码，按槽位顺序。
const CODES: [u32; 6] = [MK_ETAIL, MK_ESHORT, MK_EXIST, MK_EMODE, MK_EBYTE, MK_ELEN];

/// 十四格账本：解析、创建、预览、删除与六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut g = Gen::new(0x1234_5678_9abc_def0);
    let mut b = Bench::new();

    let spec = match Spec::parse(b"/tmp/vxXXXXXX") {
        Ok(v) => v,
        Err(_) => return l,
    };

    let _ = l.register(count_cell(0, OP_PARSE, Ok(spec.xrun())));
    let _ = l.register(count_cell(1, OP_CREATE, b.create(&spec, &mut g)));
    let _ = l.register(count_cell(2, OP_CREATE, b.create(&spec, &mut g)));
    let _ = l.register(count_cell(3, OP_STAT, Ok(b.len())));
    let _ = l.register(count_cell(4, OP_PREVIEW, b.preview(&spec, &g).map(|s| s.len())));
    let _ = l.register(count_cell(5, OP_REMOVE, b.remove_at(0)));
    let _ = l.register(count_cell(6, OP_STAT, Ok(b.len())));
    let _ = l.register(count_cell(7, OP_STAT, b.name_len(0).ok_or(MK_ENOBS)));

    let _ = l.register(count_cell(8, OP_PARSE, Spec::parse(b"/tmp/vxXX.XX").map(|s| s.xrun())));
    let _ = l.register(count_cell(9, OP_PARSE, Spec::parse(b"/tmp/vxXX").map(|s| s.xrun())));

    let narrow_spec = match Spec::parse(b"tmpXXXX") {
        Ok(v) => v,
        Err(_) => return l,
    };
    let mut nb = Bench::new();
    let mut ng = Gen::narrow(0x99, 1);
    let _ = nb.create(&narrow_spec, &mut ng);
    let _ = l.register(count_cell(10, OP_CREATE, nb.create(&narrow_spec, &mut ng)));

    let _ = l.register(count_cell(11, OP_CREATE, b.create_with(&spec, &mut g, 0o644)));

    let _ = l.register(count_cell(12, OP_PARSE, Spec::parse(b"/tmp/vx\x7fXX").map(|s| s.xrun())));

    let long = [b'a'; TMPLMAX + 1];
    let _ = l.register(count_cell(13, OP_PARSE, Spec::parse(&long).map(|s| s.xrun())));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：模板形状、渲染拼接与后缀。
    let a = match Spec::parse(b"/tmp/vxXXXXXX") {
        Ok(s) => s.dir() == "/tmp" && s.stem() == "vx" && s.xrun() == 6 && s.total_len() == 13,
        Err(_) => false,
    };
    let b = match Spec::parse(b"abXXX") {
        Ok(s) => s.dir() == "" && s.stem() == "ab" && s.render("123").map(|v| v == "ab123") == Ok(true),
        Err(_) => false,
    };
    let c = match Spec::parse(b"/XXX") {
        Ok(s) => s.dir() == "/" && s.render("ABC").map(|v| v == "/ABC") == Ok(true),
        Err(_) => false,
    };
    let d = match Spec::parse(b"XXXX") {
        Ok(s) => s.stem() == "" && s.name_len() == 4,
        Err(_) => false,
    };
    let e = match Spec::parse(b"/tmp/fXXXXXX") {
        Ok(s) => match s.with_suffix(b".tmp") {
            Ok(t) => t.render("abc123").map(|v| v == "/tmp/fabc123.tmp") == Ok(true) && t.xrun() == 6,
            Err(_) => false,
        },
        Err(_) => false,
    };
    out[1] = a && b && c && d && e;

    // 二路：生成器确定性、预览不推进、创建与删除。
    let f1 = Gen::new(7).fill(6);
    let f2 = Gen::new(7).fill(6);
    let same = f1 == f2 && f1.len() == 6;
    let mut g = Gen::new(11);
    let before = g.peek();
    let spec = match Spec::parse(b"vXXXX") {
        Ok(v) => v,
        Err(_) => return out,
    };
    let mut bench = Bench::new();
    let pv = bench.preview(&spec, &g);
    let still = g.peek() == before;
    let one = bench.create(&spec, &mut g);
    let two = bench.create(&spec, &mut g);
    let three = bench.create(&spec, &mut g);
    let named = bench.contains(b"vnope");
    let mid = bench.remove_at(1);
    let shifted = bench.name_is(1, b"vnope") == Some(false) && bench.len() == 2;
    out[2] = same
        && still
        && pv.is_ok()
        && one == Ok(0)
        && two == Ok(1)
        && three == Ok(2)
        && !named
        && mid == Ok(1)
        && shifted
        && bench.verify();

    // 三路：六类拒绝与冻结重放。
    let tail = Spec::parse(b"/tmp/vxXX.XX") == Err(MK_ETAIL);
    let short = Spec::parse(b"/tmp/vxXX") == Err(MK_ESHORT);
    let byte = Spec::parse(b"/tmp/vx\x7fXX") == Err(MK_EBYTE);
    let len = Spec::parse(&[b'a'; TMPLMAX + 1]) == Err(MK_ELEN);
    let mode = bench.create_with(&spec, &mut g, 0o644) == Err(MK_EMODE);

    let mut nb = Bench::new();
    let mut ng = Gen::narrow(3, 1);
    let s2 = match Spec::parse(b"zXXXX") {
        Ok(v) => v,
        Err(_) => return out,
    };
    let first = nb.create(&s2, &mut ng) == Ok(0);
    let clash = nb.create(&s2, &mut ng) == Err(MK_EXIST);
    let counts = nb.retries() == TRYMAX && nb.attempts() == TRYMAX + 1 && nb.len() == 1;

    let mut led = build_ledger();
    let one_f = led.freeze();
    let rep_f = led.replay();
    let two_f = led.freeze();
    let reg = led.register(Sample::blank());
    out[3] = tail
        && short
        && byte
        && len
        && mode
        && first
        && clash
        && counts
        && one_f.is_ok()
        && rep_f == one_f
        && two_f.is_err()
        && reg == Err(MK_ESLOT)
        && led.is_frozen()
        && led.rejects() == 2;

    out
}

/// 十三项判据。
pub fn run_unx11547_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11547.mktemp");

    s.add(
        "MK01-模板解析",
        {
            let a = match Spec::parse(b"/tmp/vxXXXXXX") {
                Ok(v) => {
                    v.dir() == "/tmp"
                        && v.stem() == "vx"
                        && v.xrun() == 6
                        && v.suffix() == ""
                        && v.name_len() == 8
                        && v.total_len() == 13
                }
                Err(_) => false,
            };
            let b = match Spec::parse(b"abXXX") {
                Ok(v) => v.dir() == "" && v.stem() == "ab" && v.xrun() == 3,
                Err(_) => false,
            };
            let c = match Spec::parse(b"/XXX") {
                Ok(v) => v.dir() == "/" && v.stem() == "" && v.xrun() == 3,
                Err(_) => false,
            };
            let d = match Spec::parse(b"XXXX") {
                Ok(v) => v.dir() == "" && v.stem() == "" && v.name_len() == 4,
                Err(_) => false,
            };
            a && b && c && d
        },
        "static",
    );

    s.add(
        "MK02-占位串必须在尾部",
        {
            let a = Spec::parse(b"/tmp/vxXX.XX") == Err(MK_ETAIL);
            let b = Spec::parse(b"/tmp/vXXxXX") == Err(MK_ETAIL);
            let c = Spec::parse(b"/tmp/abcXXXx") == Err(MK_ETAIL);
            let d = Spec::parse(b"/tmp/none") == Err(MK_EINVAL);
            let e = match Spec::parse(b"/tmp/aXXX") {
                Ok(v) => v.stem() == "a" && v.xrun() == 3,
                Err(_) => false,
            };
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "MK03-占位串长度上下限",
        {
            let short = Spec::parse(b"abXX") == Err(MK_ESHORT);
            let zero = Spec::parse(b"ab") == Err(MK_EINVAL);
            let ok = match Spec::parse(b"abXXX") {
                Ok(v) => v.xrun() == 3,
                Err(_) => false,
            };
            let ok12 = match Spec::parse(b"abXXXXXXXXXXXX") {
                Ok(v) => v.xrun() == XSMAX,
                Err(_) => false,
            };
            let long = Spec::parse(b"abXXXXXXXXXXXXX") == Err(MK_ELEN);
            short && zero && ok && ok12 && long
        },
        "static",
    );

    s.add(
        "MK04-非法字节与空模板",
        {
            let empty = Spec::parse(b"") == Err(MK_EINVAL);
            let del = Spec::parse(b"ab\x7fXXX") == Err(MK_EBYTE);
            let nl = Spec::parse(b"ab\nXXX") == Err(MK_EBYTE);
            let space = match Spec::parse(b"ab XXX") {
                Ok(v) => v.stem() == "ab ",
                Err(_) => false,
            };
            let sfx = match Spec::parse(b"abXXX") {
                Ok(v) => {
                    v.clone().with_suffix(b"a/b") == Err(MK_EINVAL)
                        && v.with_suffix(b"a\x01b") == Err(MK_EBYTE)
                }
                Err(_) => false,
            };
            empty && del && nl && space && sfx
        },
        "static",
    );

    s.add(
        "MK05-生成器字符集",
        {
            let mut g = Gen::new(0xdead_beef);
            let f = g.fill(10);
            let mut all = f.len() == 10;
            let mut i = 0usize;
            while i < f.len() {
                match f.as_bytes().get(i) {
                    Some(b) => {
                        if ALPHA.iter().all(|a| a != b) {
                            all = false;
                        }
                    }
                    None => all = false,
                }
                i += 1;
            }
            let narrow = Gen::narrow(1, 1).fill(4);
            let wide = Gen::narrow(1, 0).fill(4);
            all && narrow.len() == 4 && wide.len() == 4 && Gen::narrow(1, 0).width() == 1
        },
        "static",
    );

    s.add(
        "MK06-确定性重放与预览不推进",
        {
            let a = Gen::new(42).fill(6) == Gen::new(42).fill(6);
            let b = Gen::new(42).fill(6) != Gen::new(43).fill(6);
            let spec = match Spec::parse(b"tXXXX") {
                Ok(v) => v,
                Err(_) => Spec::empty(),
            };
            let bench = Bench::new();
            let g = Gen::new(5);
            let p1 = bench.preview(&spec, &g);
            let p2 = bench.preview(&spec, &g);
            let mut h = g;
            let direct = spec.render(&h.fill(4));
            a && b && spec.xrun() == 4 && p1.is_ok() && p1 == p2 && p1 == direct
        },
        "static",
    );

    s.add(
        "MK07-渲染与目录拼接",
        {
            let a = match Spec::parse(b"/tmp/vxXXXXXX") {
                Ok(v) => v.render("abc123").map(|s| s == "/tmp/vxabc123") == Ok(true),
                Err(_) => false,
            };
            let b = match Spec::parse(b"abXXX") {
                Ok(v) => v.render("ABC").map(|s| s == "abABC") == Ok(true),
                Err(_) => false,
            };
            let c = match Spec::parse(b"/tmp/XXXX") {
                Ok(v) => v.render("abcd").map(|s| s == "/tmp/abcd") == Ok(true),
                Err(_) => false,
            };
            let d = match Spec::parse(b"/XXX") {
                Ok(v) => v.render("ABC").map(|s| s == "/ABC") == Ok(true),
                Err(_) => false,
            };
            let e = match Spec::parse(b"fXXXXXX") {
                Ok(v) => match v.with_suffix(b".tmp") {
                    Ok(t) => {
                        t.render("abc123").map(|s| s == "fabc123.tmp") == Ok(true)
                            && t.suffix() == ".tmp"
                            && t.name_len() == 11
                            && Spec::parse(b"fXXXXXX").unwrap_or(Spec::empty()).render("abc").is_err()
                    }
                    Err(_) => false,
                },
                Err(_) => false,
            };
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "MK08-创建登记",
        {
            let mut g = Gen::new(0x0102_0304);
            let mut b = Bench::new();
            let spec = match Spec::parse(b"/tmp/vxXXXXXX") {
                Ok(v) => v,
                Err(_) => Spec::empty(),
            };
            let a = b.create(&spec, &mut g) == Ok(0);
            let c = b.create(&spec, &mut g) == Ok(1);
            let names = b.name_is(0, b"/tmp/vx000000") == Some(false)
                && b.item(0).map(|v| v.len()) == Some(13)
                && b.item(1).map(|v| v.len()) == Some(13);
            a && c
                && spec.xrun() == 6
                && names
                && b.len() == 2
                && b.attempts() == 2
                && b.retries() == 0
                && b.rejects() == 0
                && b.verify()
        },
        "static",
    );

    s.add(
        "MK09-撞名重试与耗尽",
        {
            let mut b = Bench::new();
            let mut g = Gen::narrow(0x77, 1);
            let spec = match Spec::parse(b"qXXXX") {
                Ok(v) => v,
                Err(_) => Spec::empty(),
            };
            let a = b.create(&spec, &mut g) == Ok(0);
            let c = b.create(&spec, &mut g) == Err(MK_EXIST);
            a && c
                && spec.xrun() == 4
                && b.len() == 1
                && b.retries() == TRYMAX
                && b.attempts() == TRYMAX + 1
                && b.rejects() == 1
        },
        "static",
    );

    s.add(
        "MK10-删除与槽位回收",
        {
            let mut b = Bench::new();
            let mut g = Gen::new(0x5152_5354);
            let spec = match Spec::parse(b"rXXXXXX") {
                Ok(v) => v,
                Err(_) => Spec::empty(),
            };
            let _ = b.create(&spec, &mut g);
            let _ = b.create(&spec, &mut g);
            let _ = b.create(&spec, &mut g);
            let n1 = match b.item(1) {
                Some(v) => Some(v.len()),
                None => None,
            };
            let mid = b.remove_at(1);
            let left = b.len() == 2;
            let gone = b.index_of(b"rnope").is_none();
            let bad = b.remove_at(5) == Err(MK_EINVAL);
            let miss = b.remove(b"rnone2") == Err(MK_EINVAL);
            mid == Ok(1)
                && spec.xrun() == 6
                && left
                && gone
                && bad
                && miss
                && n1 == Some(7)
                && b.verify()
        },
        "static",
    );

    s.add(
        "MK11-模式约束与容量",
        {
            let mut b = Bench::new();
            let mut g = Gen::new(0x6162_6364);
            let spec = match Spec::parse(b"mXXXXXX") {
                Ok(v) => v,
                Err(_) => Spec::empty(),
            };
            let bad = b.create_with(&spec, &mut g, 0o644) == Err(MK_EMODE);
            let okf = b.create_with(&spec, &mut g, MODE_FILE) == Ok(0);
            let file_mode = b.item(0).map(|v| v.mode()) == Some(MODE_FILE);
            let okd = b.create_dir(&spec, &mut g) == Ok(1);
            let dir_mode = b.item(1).map(|v| v.mode()) == Some(MODE_DIR);
            let mut full = true;
            let mut i = 0usize;
            while i + 2 < CAP {
                if b.create(&spec, &mut g) != Ok(i + 2) {
                    full = false;
                }
                i += 1;
            }
            let over = b.create(&spec, &mut g) == Err(MK_EFULL);
            bad
                && okf
                && file_mode
                && okd
                && dir_mode
                && full
                && over
                && spec.xrun() == 6
                && b.len() == CAP
                && b.dirs() == 1
                && b.verify()
        },
        "static",
    );

    s.add(
        "MK12-账本审计",
        {
            let l = build_ledger();
            l.len() == SAMPLES
                && l.ok_count() == 8
                && l.err_count() == 6
                && l.audit().is_ok()
                && l.rejects() == 0
                && l.sample(0).map(|s| s.value) == Some(6)
                && l.sample(13).map(|s| s.code) == Some(MK_ELEN)
        },
        "static",
    );

    s.add(
        "MK13-冻结重放四注入",
        inject_four() == [true; 4] && build_ledger().audit().is_ok(),
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_shapes() {
        let v = match Spec::parse(b"/tmp/vxXXXXXX") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(v.dir(), "/tmp");
        assert_eq!(v.stem(), "vx");
        assert_eq!(v.xrun(), 6);
        assert_eq!(v.name_len(), 8);
        assert_eq!(v.total_len(), 13);

        let root = match Spec::parse(b"/XXX") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(root.dir(), "/");
        assert_eq!(root.render("ABC"), Ok(String::from("/ABC")));
    }

    #[test]
    fn parse_rejects() {
        assert_eq!(Spec::parse(b""), Err(MK_EINVAL));
        assert_eq!(Spec::parse(b"/tmp/none"), Err(MK_EINVAL));
        assert_eq!(Spec::parse(b"/tmp/aXX.XX"), Err(MK_ETAIL));
        assert_eq!(Spec::parse(b"/tmp/aXX"), Err(MK_ESHORT));
        assert_eq!(Spec::parse(b"/tmp/a\x7fXX"), Err(MK_EBYTE));
        assert_eq!(Spec::parse(&[b'z'; TMPLMAX + 1]), Err(MK_ELEN));
    }

    #[test]
    fn render_join() {
        let a = match Spec::parse(b"abXXX") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(a.render("123"), Ok(String::from("ab123")));
        assert_eq!(a.render("12"), Err(MK_EINVAL));

        let b = match Spec::parse(b"fXXXXXX") {
            Ok(v) => v,
            Err(_) => return,
        };
        let c = match b.with_suffix(b".tmp") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(c.xrun(), 6);
        assert_eq!(c.render("abc123"), Ok(String::from("fabc123.tmp")));
    }

    #[test]
    fn generator_is_deterministic() {
        let a = Gen::new(9).fill(8);
        let b = Gen::new(9).fill(8);
        assert_eq!(a, b);
        assert_eq!(a.len(), 8);
        assert_ne!(Gen::new(9).fill(8), Gen::new(10).fill(8));
        assert_eq!(Gen::narrow(4, 1).fill(3).len(), 3);
    }

    #[test]
    fn collision_exhaustion() {
        let mut b = Bench::new();
        let mut g = Gen::narrow(2, 1);
        let spec = match Spec::parse(b"cXXXX") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(b.create(&spec, &mut g), Ok(0));
        assert_eq!(b.create(&spec, &mut g), Err(MK_EXIST));
        assert_eq!(b.len(), 1);
        assert_eq!(b.retries(), TRYMAX);
        assert_eq!(b.attempts(), TRYMAX + 1);
    }

    #[test]
    fn removal_shifts_slots() {
        let mut b = Bench::new();
        let mut g = Gen::new(0x99);
        let spec = match Spec::parse(b"dXXXX") {
            Ok(v) => v,
            Err(_) => return,
        };
        let _ = b.create(&spec, &mut g);
        let _ = b.create(&spec, &mut g);
        let _ = b.create(&spec, &mut g);
        assert_eq!(b.remove_at(1), Ok(1));
        assert_eq!(b.len(), 2);
        assert_eq!(b.remove_at(9), Err(MK_EINVAL));
        assert!(b.verify());
    }

    #[test]
    fn mode_and_capacity() {
        let mut b = Bench::new();
        let mut g = Gen::new(0x1234);
        let spec = match Spec::parse(b"eXXXXXX") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(b.create_with(&spec, &mut g, 0o755), Err(MK_EMODE));
        assert_eq!(b.create_dir(&spec, &mut g), Ok(0));
        assert_eq!(b.item(0).map(|v| v.mode()), Some(MODE_DIR));
        assert_eq!(b.dirs(), 1);
        let mut i = 1usize;
        while i < CAP {
            assert!(b.create(&spec, &mut g).is_ok());
            i += 1;
        }
        assert_eq!(b.create(&spec, &mut g), Err(MK_EFULL));
        assert!(b.verify());
    }

    #[test]
    fn ledger_audits_and_freezes() {
        let mut l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert!(l.audit().is_ok());
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        let one = l.freeze();
        assert!(one.is_ok());
        assert_eq!(l.replay(), one);
        assert!(l.freeze().is_err());
        assert_eq!(l.register(Sample::blank()), Err(MK_ESLOT));
        assert_eq!(l.rejects(), 2);
    }
}
