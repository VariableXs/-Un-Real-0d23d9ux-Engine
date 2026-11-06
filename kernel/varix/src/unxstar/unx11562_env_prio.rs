//! UNX-F11562 locale 环境变量优先级链 LC_ALL > LC_* > LANG
//!
//! 问题的根：POSIX 说「域相关的环境变量」有**三层**，但没说清「设了但为空」算什么、
//! 「重复赋值」谁赢、以及某一域没有专门变量时到底落到哪一层。基准机的行为是：
//!
//! 1. `LC_ALL` 非空 → **十二个域全部**用它，哪怕同时设了 `LC_TIME`；
//! 2. `LC_ALL` 缺席或为空 → 每个域各自看 `LC_<CATEGORY>`；
//! 3. 该域变量缺席或为空 → `LANG`；
//! 4. 三层都没有 → 编译期缺省 `"C"`。
//!
//! 「为空」在这条链里等价于「未设置」——这是 `LC_ALL=` 能把总闸临时拆掉的原因，
//! 也是 `env LC_ALL= LC_TIME=en_US sort` 仍然走 en_US 排序的原因。若把空串当成
//! 一个值参与比较，链会整体错位，`LC_ALL=` 反而变成「比谁都优先的空 locale」。
//!
//! 本单把这条链做成**可追溯**的东西：
//!
//! * `Env` 是定容表（24 槽），`put` 是「后写覆盖」，并单独记 `overrides`；
//!    `put_strict` 则拒绝重复（`LP_EDUP`），两种语义都保留，避免「以为在测覆盖、
//!    其实测的是拒绝」。
//! * `resolve` 对十二个域逐个求值，产出 `Pick { value, layer, key }`。层号是
//!   可判定的枚举（Default=0/Lang=1/Category=2/All=3），不是字符串比较。
//! * `Trace` 保留整条候选链（All → Category → Lang），`first_set` 给出赢家，
//!    于是「为什么这个域取了这个值」可以逐项复核，而不是只看结果。
//! * `Resolution::uniform` 是 `LC_ALL` 生效的代数判据：十二域同值；`LC_ALL`
//!    缺席时它必须为假（除非十二个域恰好都被设成同一个值——那是另一种合法的
//!    均匀，层分布会不同，靠 `count_layer` 区分）。
//!
//! 闸门次序：`LP_ESHORT`（空名）→ `LP_EINVAL`（未知域名/非法 locale 名）→
//! `LP_ELEN`（值超长）→ `LP_EBYTE`（值含非法字节，如空格/高位字节）→
//! `LP_ERANGE`（表满）→ `LP_EDUP`（严格模式下重复）→ `LP_ESLOT`（域下标越界）
//! → `LP_ESTATE`（层不一致时强取单值）。前五道在 `Env::put`，重复在
//! `put_strict`，越界与状态在 `Resolution`。
//!
//! 账本语义：十四格 = 八格成功（解析域数、域层号、总闸层号、缺省域数、覆盖计数、
//! 域名下标、不同值个数、均匀标志）+ 六格失败（未知域名、值超长、值含空格、
//! 表满、域下标越界、空名）。`audit` 认「八值六码」，`seal` 折叠，`replay`
//! 重建比对，`freeze` 后拒登。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf430;

/// 域名非法。
pub const LP_EINVAL: u32 = SEG_BASE + 0x01;
/// 值超长。
pub const LP_ELEN: u32 = SEG_BASE + 0x02;
/// 值含非法字节。
pub const LP_EBYTE: u32 = SEG_BASE + 0x03;
/// 表满/下标越界区间。
pub const LP_ERANGE: u32 = SEG_BASE + 0x04;
/// 重复赋值（严格模式）。
pub const LP_EDUP: u32 = SEG_BASE + 0x05;
/// 空名/空输入。
pub const LP_ESHORT: u32 = SEG_BASE + 0x06;
/// 层不一致。
pub const LP_ESTATE: u32 = SEG_BASE + 0x07;
/// 值冲突（非法 locale）。
pub const LP_ECONF: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const LP_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位错位。
pub const LP_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const LP_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const LP_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观测缺失。
pub const LP_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 域数。
pub const CATS: usize = 12;

/// 环境表容量（12 域 + LANG + LC_ALL = 14，留 10 槽冗余）。
pub const ENV_CAP: usize = 24;

/// 值容量。
pub const VAL_CAP: usize = 24;

/// 域名容量。
pub const NAME_CAP: usize = 20;

/// 键：LC_ALL。
pub const KEY_ALL: u8 = 0x0d;
/// 键：LANG。
pub const KEY_LANG: u8 = 0x0c;

/// 操作码：解析。
pub const OP_RESOLVE: u8 = 0x01;
/// 操作码：层号。
pub const OP_LAYER: u8 = 0x02;
/// 操作码：总闸。
pub const OP_ALL: u8 = 0x03;
/// 操作码：缺省。
pub const OP_DEFAULT: u8 = 0x04;
/// 操作码：覆盖。
pub const OP_OVR: u8 = 0x05;
/// 操作码：域名。
pub const OP_KEY: u8 = 0x06;
/// 操作码：不同值。
pub const OP_DISTINCT: u8 = 0x07;
/// 操作码：均匀。
pub const OP_UNIFORM: u8 = 0x08;

/// FNV-1a 偏移。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节。
fn fold(h: u64, b: u64) -> u64 {
    let mut v = h ^ (b & 0xff);
    let mut i = 0u32;
    while i < 8 {
        v = v.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    v
}

/// 十进制拼接。
fn push_dec(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 20usize;
    while v > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    let mut k = i;
    while k < 20 {
        if let Some(b) = buf.get(k) {
            s.push(*b as char);
        }
        k += 1;
    }
}

/// 有符号十进制拼接。
fn push_i64(s: &mut String, v: i64) {
    if v < 0 {
        s.push('-');
        push_dec(s, v.unsigned_abs());
    } else {
        push_dec(s, v as u64);
    }
}

/// 另起一行（首行不加）。
fn newline(s: &mut String) {
    if !s.is_empty() {
        s.push('\n');
    }
}

/// 十二个域的名字，次序即下标。
pub const CAT_NAMES: [&str; CATS] = [
    "LC_CTYPE",
    "LC_NUMERIC",
    "LC_TIME",
    "LC_COLLATE",
    "LC_MONETARY",
    "LC_MESSAGES",
    "LC_PAPER",
    "LC_NAME",
    "LC_ADDRESS",
    "LC_TELEPHONE",
    "LC_MEASUREMENT",
    "LC_IDENTIFICATION",
];

/// 域名 → 键。`LANG` 是 12，`LC_ALL` 是 13，其余为域下标。
pub fn key_of(name: &[u8]) -> Result<u8, u32> {
    if name.is_empty() {
        return Err(LP_ESHORT);
    }
    if name.len() > NAME_CAP {
        return Err(LP_ELEN);
    }
    if name == b"LANG" {
        return Ok(KEY_LANG);
    }
    if name == b"LC_ALL" {
        return Ok(KEY_ALL);
    }
    let mut i = 0usize;
    while i < CATS {
        if let Some(n) = CAT_NAMES.get(i) {
            if n.as_bytes() == name {
                return Ok(i as u8);
            }
        }
        i += 1;
    }
    Err(LP_EINVAL)
}

/// 键 → 名字。
pub fn name_of(key: u8) -> Option<&'static str> {
    if key == KEY_LANG {
        return Some("LANG");
    }
    if key == KEY_ALL {
        return Some("LC_ALL");
    }
    CAT_NAMES.get(key as usize).copied()
}

/// 域下标是否合法。
pub fn valid_cat(i: usize) -> bool {
    i < CATS
}

/// 优先级层。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    /// 缺省 "C"。
    Default,
    /// LANG。
    Lang,
    /// LC_<CATEGORY>。
    Category,
    /// LC_ALL。
    All,
}

impl Layer {
    /// 四种，由低到高。
    pub fn all() -> [Layer; 4] {
        [Layer::Default, Layer::Lang, Layer::Category, Layer::All]
    }

    /// 优先级（0..=3）。
    pub fn rank(&self) -> u8 {
        match self {
            Layer::Default => 0,
            Layer::Lang => 1,
            Layer::Category => 2,
            Layer::All => 3,
        }
    }

    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Layer::Default => "default",
            Layer::Lang => "lang",
            Layer::Category => "cat",
            Layer::All => "all",
        }
    }

    /// 由秩还原。
    pub fn from_rank(r: u8) -> Option<Layer> {
        match r {
            0 => Some(Layer::Default),
            1 => Some(Layer::Lang),
            2 => Some(Layer::Category),
            3 => Some(Layer::All),
            _ => None,
        }
    }
}

/// 定容字符串值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Var {
    buf: [u8; VAL_CAP],
    len: usize,
    set: bool,
}

impl Var {
    /// 未设置。
    pub const fn empty() -> Var {
        Var { buf: [0u8; VAL_CAP], len: 0, set: false }
    }

    /// 由字节构造；空值等价于未设置。
    pub fn parse(raw: &[u8]) -> Result<Var, u32> {
        if raw.is_empty() {
            return Ok(Var::empty());
        }
        if raw.len() > VAL_CAP {
            return Err(LP_ELEN);
        }
        let mut buf = [0u8; VAL_CAP];
        let mut i = 0usize;
        while i < raw.len() {
            let b = match raw.get(i).copied() {
                Some(v) => v,
                None => return Err(LP_ESHORT),
            };
            let ok = matches!(b,
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'@' | b'-');
            if !ok {
                return Err(LP_EBYTE);
            }
            if let Some(d) = buf.get_mut(i) {
                *d = b;
            }
            i += 1;
        }
        Ok(Var { buf, len: raw.len(), set: true })
    }

    /// 强制赋值（供已校验的内部路径用）。
    fn force(&mut self, raw: &[u8]) {
        let mut i = 0usize;
        while i < raw.len() && i < VAL_CAP {
            if let Some(d) = self.buf.get_mut(i) {
                if let Some(s) = raw.get(i).copied() {
                    *d = s;
                }
            }
            i += 1;
        }
        self.len = if raw.len() <= VAL_CAP { raw.len() } else { VAL_CAP };
        self.set = true;
    }

    /// 是否设置且非空。
    pub fn is_set(&self) -> bool {
        self.set && self.len > 0
    }

    /// 取值。
    pub fn bytes(&self) -> &[u8] {
        if !self.set {
            return &[];
        }
        let n = if self.len <= VAL_CAP { self.len } else { VAL_CAP };
        match self.buf.get(0..n) {
            Some(s) => s,
            None => &[],
        }
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let v = self.bytes();
        let mut i = 0usize;
        while i < v.len() {
            if let Some(b) = v.get(i) {
                s.push(*b as char);
            }
            i += 1;
        }
        s
    }

    /// 与字面量比较。
    pub fn is(&self, raw: &[u8]) -> bool {
        self.bytes() == raw
    }

    /// 是否 C/POSIX。
    pub fn is_c(&self) -> bool {
        self.is(b"C") || self.is(b"POSIX")
    }

    /// 是否 C.UTF-8 一族（码点序，不是字节序）。
    pub fn is_c_utf8(&self) -> bool {
        self.is(b"C.UTF-8") || self.is(b"C.utf8") || self.is(b"C.UTF8")
    }

    /// 长度。
    pub fn len(&self) -> usize {
        if self.set && self.len <= VAL_CAP { self.len } else { 0 }
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// 一个环境槽。
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    /// 键（0..=13）。
    pub key: u8,
    /// 值。
    pub val: Var,
}

impl Slot {
    /// 空槽。
    pub const fn blank() -> Slot {
        Slot { key: 0xff, val: Var::empty() }
    }
}

/// 定容环境表。
#[derive(Clone, Copy, Debug)]
pub struct Env {
    slots: [Slot; ENV_CAP],
    len: usize,
    overrides: u32,
    ignored: u32,
}

impl Env {
    /// 空表。
    pub const fn new() -> Env {
        Env { slots: [Slot::blank(); ENV_CAP], len: 0, overrides: 0, ignored: 0 }
    }

    /// 查找键所在槽（后写者优先，故从尾部回扫）。
    fn find(&self, key: u8) -> Option<usize> {
        let mut i = self.len;
        while i > 0 {
            i -= 1;
            if let Some(s) = self.slots.get(i) {
                if s.key == key {
                    return Some(i);
                }
            }
        }
        None
    }

    /// 原始追加：允许同名多槽（真实 environ 允许重复项），`get` 取最后一条。
    pub fn append(&mut self, name: &[u8], raw: &[u8]) -> Result<(), u32> {
        let key = key_of(name)?;
        let v = Var::parse(raw)?;
        if self.len >= ENV_CAP {
            return Err(LP_ERANGE);
        }
        if let Some(s) = self.slots.get_mut(self.len) {
            *s = Slot { key, val: v };
        }
        self.len += 1;
        Ok(())
    }

    /// 污染注入：跳过字节校验写入（仅供「校验闸是否真的在工作」的负向探针）。
    pub fn inject(&mut self, name: &[u8], raw: &[u8]) -> Result<(), u32> {
        let key = key_of(name)?;
        if raw.len() > VAL_CAP {
            return Err(LP_ELEN);
        }
        if self.len >= ENV_CAP {
            return Err(LP_ERANGE);
        }
        let mut v = Var::empty();
        v.force(raw);
        if let Some(s) = self.slots.get_mut(self.len) {
            *s = Slot { key, val: v };
        }
        self.len += 1;
        Ok(())
    }

    /// 赋值：重复即后写覆盖，空值即取消。
    pub fn put(&mut self, name: &[u8], raw: &[u8]) -> Result<bool, u32> {
        let key = key_of(name)?;
        let v = Var::parse(raw)?;
        if let Some(i) = self.find(key) {
            if let Some(s) = self.slots.get_mut(i) {
                s.val = v;
            }
            self.overrides = self.overrides.saturating_add(1);
            return Ok(true);
        }
        if self.len >= ENV_CAP {
            return Err(LP_ERANGE);
        }
        if let Some(s) = self.slots.get_mut(self.len) {
            *s = Slot { key, val: v };
        }
        self.len += 1;
        Ok(false)
    }

    /// 严格赋值：重复即拒绝。
    pub fn put_strict(&mut self, name: &[u8], raw: &[u8]) -> Result<bool, u32> {
        let key = key_of(name)?;
        if self.find(key).is_some() {
            return Err(LP_EDUP);
        }
        self.put(name, raw)
    }

    /// 取值。
    pub fn get(&self, key: u8) -> Var {
        match self.find(key) {
            Some(i) => match self.slots.get(i) {
                Some(s) => s.val,
                None => Var::empty(),
            },
            None => Var::empty(),
        }
    }

    /// 某域变量。
    pub fn cat(&self, i: usize) -> Var {
        if !valid_cat(i) {
            return Var::empty();
        }
        self.get(i as u8)
    }

    /// LANG。
    pub fn lang(&self) -> Var {
        self.get(KEY_LANG)
    }

    /// LC_ALL。
    pub fn all(&self) -> Var {
        self.get(KEY_ALL)
    }

    /// 槽数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.len == 0 && self.ignored == 0
    }

    /// 覆盖次数。
    pub fn overrides(&self) -> u32 {
        self.overrides
    }

    /// 被忽略的键数（parse 里遇到不认识的变量名时只记账）。
    pub fn ignored(&self) -> u32 {
        self.ignored
    }

    /// 解析 `NAME=VALUE` 文本。未知变量名不报错，只计数（真实 environ 里
    /// PATH/TERM 等都在，不能因此判整个环境非法）。
    pub fn parse(text: &[u8]) -> Result<Env, u32> {
        let mut env = Env::new();
        let mut pos = 0usize;
        while pos < text.len() {
            let mut end = pos;
            while end < text.len() {
                match text.get(end).copied() {
                    Some(b'\n') => break,
                    Some(_) => end += 1,
                    None => break,
                }
            }
            let raw_line = text.get(pos..end).unwrap_or(&[]);
            let mut a = 0usize;
            while a < raw_line.len() {
                match raw_line.get(a).copied() {
                    Some(b' ') | Some(b'\t') => a += 1,
                    _ => break,
                }
            }
            let mut b = raw_line.len();
            while b > a {
                match raw_line.get(b - 1).copied() {
                    Some(b' ') | Some(b'\t') | Some(b'\r') => b -= 1,
                    _ => break,
                }
            }
            let line = raw_line.get(a..b).unwrap_or(&[]);
            if !line.is_empty() {
                let mut eq = line.len();
                let mut i = 0usize;
                while i < line.len() {
                    if let Some(c) = line.get(i).copied() {
                        if c == b'=' {
                            eq = i;
                            break;
                        }
                    }
                    i += 1;
                }
                if eq == 0 {
                    return Err(LP_ESHORT);
                }
                if eq == line.len() {
                    return Err(LP_EINVAL);
                }
                let key = line.get(0..eq).unwrap_or(&[]);
                let val = line.get(eq + 1..).unwrap_or(&[]);
                match key_of(key) {
                    Ok(_) => {
                        env.put(key, val)?;
                    }
                    Err(_) => {
                        env.ignored = env.ignored.saturating_add(1);
                    }
                }
            }
            pos = if end < text.len() { end + 1 } else { end };
        }
        Ok(env)
    }

    /// 渲染（只输出本表认识的键，按登记次序）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            newline(&mut s);
            if let Some(sl) = self.slots.get(i) {
                if let Some(n) = name_of(sl.key) {
                    s.push_str(n);
                }
                s.push('=');
                s.push_str(&sl.val.text());
            }
            i += 1;
        }
        s
    }
}

/// 一个域的判定结果。
#[derive(Clone, Copy, Debug)]
pub struct Pick {
    /// 域下标。
    pub cat: u8,
    /// 生效值。
    pub value: Var,
    /// 来源层。
    pub layer: Layer,
    /// 来源键（10=缺省、12=LANG、13=LC_ALL、其余域下标）。
    pub key: u8,
}

impl Pick {
    /// 空。
    pub const fn blank() -> Pick {
        Pick { cat: 0, value: Var::empty(), layer: Layer::Default, key: 0xff }
    }

    /// 来源描述。
    pub fn source(&self) -> &'static str {
        match self.key {
            0x0c => "LANG",
            0x0d => "LC_ALL",
            0x0a => "default",
            _ => match name_of(self.key) {
                Some(n) => n,
                None => "?",
            },
        }
    }
}

/// 十二域判定表。
#[derive(Clone, Copy, Debug)]
pub struct Resolution {
    picks: [Pick; CATS],
    len: usize,
}

impl Resolution {
    /// 空表。
    pub const fn blank() -> Resolution {
        Resolution { picks: [Pick::blank(); CATS], len: 0 }
    }

    /// 由环境求值。
    pub fn resolve(env: &Env) -> Resolution {
        let mut r = Resolution::blank();
        let all = env.all();
        let lang = env.lang();
        let mut i = 0usize;
        while i < CATS {
            let cv = env.cat(i);
            let pick = if all.is_set() {
                Pick { cat: i as u8, value: all, layer: Layer::All, key: KEY_ALL }
            } else if cv.is_set() {
                Pick { cat: i as u8, value: cv, layer: Layer::Category, key: i as u8 }
            } else if lang.is_set() {
                Pick { cat: i as u8, value: lang, layer: Layer::Lang, key: KEY_LANG }
            } else {
                Pick { cat: i as u8, value: Var::empty(), layer: Layer::Default, key: 0x0a }
            };
            if let Some(d) = r.picks.get_mut(i) {
                *d = pick;
            }
            i += 1;
        }
        r.len = CATS;
        r
    }

    /// 域数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取某域。
    pub fn pick(&self, i: usize) -> Result<Pick, u32> {
        if !valid_cat(i) {
            return Err(LP_ESLOT);
        }
        match self.picks.get(i) {
            Some(p) => Ok(*p),
            None => Err(LP_ESLOT),
        }
    }

    /// 某域的值。
    pub fn value(&self, i: usize) -> Result<Var, u32> {
        self.pick(i).map(|p| p.value)
    }

    /// 某域的层号。
    pub fn layer(&self, i: usize) -> Result<u8, u32> {
        self.pick(i).map(|p| p.layer.rank())
    }

    /// 某层的域数。
    pub fn count_layer(&self, l: Layer) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(p) = self.picks.get(i) {
                if p.layer == l {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 四层分布。
    pub fn layer_histogram(&self) -> [usize; 4] {
        let mut h = [0usize; 4];
        let mut i = 0usize;
        while i < self.len {
            if let Some(p) = self.picks.get(i) {
                let r = p.layer.rank() as usize;
                if let Some(d) = h.get_mut(r) {
                    *d += 1;
                }
            }
            i += 1;
        }
        h
    }

    /// 不同值个数（把未设置视作 "C" 之外的空值，不参与统一性）。
    pub fn distinct_values(&self) -> usize {
        let mut seen: [usize; CATS] = [usize::MAX; CATS];
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(p) = self.picks.get(i) {
                let mut dup = false;
                let mut k = 0usize;
                while k < n {
                    if let Some(j) = seen.get(k) {
                        if let Some(q) = self.picks.get(*j) {
                            if q.value.bytes() == p.value.bytes() {
                                dup = true;
                            }
                        }
                    }
                    k += 1;
                }
                if !dup {
                    if let Some(d) = seen.get_mut(n) {
                        *d = i;
                    }
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 十二域是否同值。
    pub fn uniform(&self) -> bool {
        if self.len != CATS {
            return false;
        }
        let first = match self.picks.first() {
            Some(p) => p.value,
            None => return false,
        };
        let mut i = 1usize;
        while i < self.len {
            if let Some(p) = self.picks.get(i) {
                if p.value.bytes() != first.bytes() {
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    /// 强取单值：不同值即 `LP_ESTATE`。
    pub fn single(&self) -> Result<Var, u32> {
        if self.len != CATS || !self.uniform() {
            return Err(LP_ESTATE);
        }
        match self.picks.first() {
            Some(p) => Ok(p.value),
            None => Err(LP_ESTATE),
        }
    }

    /// 校验每个生效值都是合法 locale 名（空值按缺省 "C" 处理）。
    pub fn validate(&self) -> Result<(), u32> {
        if self.len != CATS {
            return Err(LP_ESTATE);
        }
        let mut i = 0usize;
        while i < self.len {
            if let Some(p) = self.picks.get(i) {
                if p.layer != Layer::Default {
                    Var::parse(p.value.bytes())?;
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 渲染十二行。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            newline(&mut s);
            if let Some(p) = self.picks.get(i) {
                match name_of(p.cat) {
                    Some(n) => s.push_str(n),
                    None => s.push('?'),
                }
                s.push('=');
                if p.layer == Layer::Default {
                    s.push('C');
                } else {
                    s.push_str(&p.value.text());
                }
                s.push(' ');
                s.push('[');
                s.push_str(p.layer.text());
                s.push(']');
            }
            i += 1;
        }
        s
    }

    /// 层分布串。
    pub fn plan(&self) -> String {
        let h = self.layer_histogram();
        let mut s = String::new();
        s.push_str("default");
        push_dec(&mut s, h.first().copied().unwrap_or(0) as u64);
        s.push(' ');
        s.push_str("lang");
        push_dec(&mut s, h.get(1).copied().unwrap_or(0) as u64);
        s.push(' ');
        s.push_str("cat");
        push_dec(&mut s, h.get(2).copied().unwrap_or(0) as u64);
        s.push(' ');
        s.push_str("all");
        push_dec(&mut s, h.get(3).copied().unwrap_or(0) as u64);
        s
    }
}

/// 单域候选链（All → Category → Lang）。
#[derive(Clone, Copy, Debug)]
pub struct Trace {
    /// 候选值，未设置为空。
    pub cands: [Var; 3],
    /// 候选键。
    pub keys: [u8; 3],
    /// 域名。
    pub cat: u8,
    /// 有效候选数（恒为 3）。
    pub len: usize,
}

impl Trace {
    /// 空。
    pub const fn blank() -> Trace {
        Trace {
            cands: [Var::empty(); 3],
            keys: [0xff; 3],
            cat: 0,
            len: 0,
        }
    }

    /// 构造某域的链。
    pub fn of(env: &Env, cat: usize) -> Result<Trace, u32> {
        if !valid_cat(cat) {
            return Err(LP_ESLOT);
        }
        let mut t = Trace::blank();
        t.cat = cat as u8;
        t.len = 3;
        if let Some(d) = t.cands.get_mut(0) {
            *d = env.all();
        }
        if let Some(d) = t.keys.get_mut(0) {
            *d = KEY_ALL;
        }
        if let Some(d) = t.cands.get_mut(1) {
            *d = env.cat(cat);
        }
        if let Some(d) = t.keys.get_mut(1) {
            *d = cat as u8;
        }
        if let Some(d) = t.cands.get_mut(2) {
            *d = env.lang();
        }
        if let Some(d) = t.keys.get_mut(2) {
            *d = KEY_LANG;
        }
        Ok(t)
    }

    /// 第一个非空候选。
    pub fn first_set(&self) -> Option<(u8, Var)> {
        let mut i = 0usize;
        while i < self.len {
            match self.cands.get(i) {
                Some(v) => {
                    if v.is_set() {
                        let k = self.keys.get(i).copied().unwrap_or(0xff);
                        return Some((k, *v));
                    }
                }
                None => {}
            }
            i += 1;
        }
        None
    }

    /// 赢家层号（缺省为 0）。
    pub fn winner_rank(&self) -> u8 {
        match self.first_set() {
            Some((k, _)) => {
                if k == KEY_ALL {
                    3
                } else if k == KEY_LANG {
                    1
                } else if k == 0x0a {
                    0
                } else {
                    2
                }
            }
            None => 0,
        }
    }

    /// 渲染 `all=.. cat=.. lang=..`.
    pub fn render(&self) -> String {
        let mut s = String::new();
        let labels = ["all", "cat", "lang"];
        let mut i = 0usize;
        while i < self.len {
            if i > 0 {
                s.push(' ');
            }
            if let Some(l) = labels.get(i) {
                s.push_str(l);
            }
            s.push('=');
            match self.cands.get(i) {
                Some(v) => {
                    if v.is_set() {
                        s.push_str(&v.text());
                    } else {
                        s.push('-');
                    }
                }
                None => s.push('-'),
            }
            i += 1;
        }
        s
    }
}

/// 三层基准环境：LC_ALL 缺席，LC_COLLATE/LC_TIME 各设，LANG 兜底。
pub fn layered_env() -> Result<Env, u32> {
    let mut e = Env::new();
    e.put(b"LC_COLLATE", b"en_US.UTF-8")?;
    e.put(b"LC_TIME", b"ja_JP.UTF-8")?;
    e.put(b"LANG", b"de_DE.UTF-8")?;
    Ok(e)
}

/// 合闸环境。
pub fn all_env() -> Result<Env, u32> {
    let mut e = layered_env()?;
    e.put(b"LC_ALL", b"C")?;
    Ok(e)
}

/// 解析探针。
fn probe_resolve() -> Result<u32, u32> {
    let e = match layered_env() {
        Ok(v) => v,
        Err(c) => return Err(c),
    };
    let r = Resolution::resolve(&e);
    if r.len() == CATS {
        Ok(r.len() as u32)
    } else {
        Err(LP_ESTATE)
    }
}

/// 单域层号探针。
fn probe_layer() -> Result<u32, u32> {
    let e = match layered_env() {
        Ok(v) => v,
        Err(c) => return Err(c),
    };
    let r = Resolution::resolve(&e);
    r.layer(3).map(|v| v as u32)
}

/// 总闸层号探针。
fn probe_all() -> Result<u32, u32> {
    let e = match all_env() {
        Ok(v) => v,
        Err(c) => return Err(c),
    };
    let r = Resolution::resolve(&e);
    r.layer(3).map(|v| v as u32)
}

/// 缺省域数探针。
fn probe_default() -> Result<u32, u32> {
    let r = Resolution::resolve(&Env::new());
    Ok(r.count_layer(Layer::Default) as u32)
}

/// 覆盖计数探针。
fn probe_override() -> Result<u32, u32> {
    let mut e = Env::new();
    e.put(b"LANG", b"de_DE")?;
    e.put(b"LANG", b"fr_FR")?;
    Ok(e.overrides())
}

/// 域名下标探针。
fn probe_key() -> Result<u32, u32> {
    key_of(b"LC_CTYPE").map(|k| k as u32)
}

/// 不同值个数探针。
fn probe_distinct() -> Result<u32, u32> {
    let e = match layered_env() {
        Ok(v) => v,
        Err(c) => return Err(c),
    };
    Ok(Resolution::resolve(&e).distinct_values() as u32)
}

/// 均匀标志探针。
fn probe_uniform() -> Result<u32, u32> {
    let e = match all_env() {
        Ok(v) => v,
        Err(c) => return Err(c),
    };
    Ok(if Resolution::resolve(&e).uniform() { 1 } else { 0 })
}

/// 未知域名探针。
fn probe_badname() -> Result<u32, u32> {
    key_of(b"LC_FOO").map(|k| k as u32)
}

/// 值超长探针。
fn probe_len() -> Result<u32, u32> {
    let mut e = Env::new();
    e.put(b"LANG", &[b'x'; VAL_CAP + 1]).map(|_| 1)
}

/// 值含空格探针。
fn probe_byte() -> Result<u32, u32> {
    let mut e = Env::new();
    e.put(b"LANG", b"de DE").map(|_| 1)
}

/// 表满探针。
fn probe_range() -> Result<u32, u32> {
    let mut e = Env::new();
    let mut i = 0usize;
    while i < ENV_CAP {
        e.append(b"LC_CTYPE", b"C")?;
        i += 1;
    }
    e.append(b"LANG", b"C").map(|_| 1)
}

/// 域越界探针。
fn probe_slot() -> Result<u32, u32> {
    let r = Resolution::resolve(&Env::new());
    r.pick(CATS).map(|_| 1)
}

/// 空名探针。
fn probe_short() -> Result<u32, u32> {
    key_of(b"").map(|k| k as u32)
}

/// 观测格。
#[derive(Clone, Copy, Debug)]
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

/// 账本。
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

    /// 登记。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(LP_ESLOT);
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

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取格。
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
            return Err(LP_EBAL);
        }
        const VALS: [u32; 8] = [12, 2, 3, 12, 1, 0, 3, 1];
        const CODES: [u32; 6] = [LP_EINVAL, LP_ELEN, LP_EBYTE, LP_ERANGE, LP_ESLOT, LP_ESHORT];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(LP_EBAL),
            };
            if s.slot as usize != i {
                return Err(LP_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(LP_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(LP_EVALUE);
                        }
                    }
                    None => return Err(LP_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(LP_EVALUE);
                        }
                    }
                    None => return Err(LP_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(LP_EBAL);
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
            return Err(LP_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let a = fresh.seal();
        if a == self.seal() {
            Ok(a)
        } else {
            Err(LP_ESEAL)
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

    /// 拒绝次数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 构建账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(word_cell(0, OP_RESOLVE, probe_resolve()));
    let _ = l.register(word_cell(1, OP_LAYER, probe_layer()));
    let _ = l.register(word_cell(2, OP_ALL, probe_all()));
    let _ = l.register(word_cell(3, OP_DEFAULT, probe_default()));
    let _ = l.register(word_cell(4, OP_OVR, probe_override()));
    let _ = l.register(word_cell(5, OP_KEY, probe_key()));
    let _ = l.register(word_cell(6, OP_DISTINCT, probe_distinct()));
    let _ = l.register(word_cell(7, OP_UNIFORM, probe_uniform()));
    let _ = l.register(word_cell(8, OP_KEY, probe_badname()));
    let _ = l.register(word_cell(9, OP_RESOLVE, probe_len()));
    let _ = l.register(word_cell(10, OP_RESOLVE, probe_byte()));
    let _ = l.register(word_cell(11, OP_RESOLVE, probe_range()));
    let _ = l.register(word_cell(12, OP_LAYER, probe_slot()));
    let _ = l.register(word_cell(13, OP_KEY, probe_short()));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    out[0] = {
        let e = match all_env() {
            Ok(v) => v,
            Err(_) => Env::new(),
        };
        let r = Resolution::resolve(&e);
        r.len() == CATS
            && r.uniform()
            && r.count_layer(Layer::All) == CATS
            && r.single().is_ok()
            && r.validate().is_ok()
    };

    out[1] = {
        let e = match layered_env() {
            Ok(v) => v,
            Err(_) => Env::new(),
        };
        let r = Resolution::resolve(&e);
        let h = r.layer_histogram();
        h.first().copied() == Some(0)
            && h.get(1).copied() == Some(10)
            && h.get(2).copied() == Some(2)
            && h.get(3).copied() == Some(0)
            && r.distinct_values() == 3
            && !r.uniform()
            && r.value(3).map(|v| v.is(b"en_US.UTF-8")).unwrap_or(false)
            && r.value(2).map(|v| v.is(b"ja_JP.UTF-8")).unwrap_or(false)
            && r.value(0).map(|v| v.is(b"de_DE.UTF-8")).unwrap_or(false)
    };

    out[2] = {
        let mut l = build_ledger();
        let sealed = l.seal();
        let replayed = match l.replay() {
            Ok(v) => v == sealed,
            Err(_) => false,
        };
        let frozen = l.freeze().is_ok();
        replayed && frozen && l.is_frozen() && l.audit().is_ok()
    };

    out[3] = {
        let mut e = Env::new();
        let a = match e.put(b"LANG", b"de_DE") {
            Ok(v) => v,
            Err(_) => true,
        };
        let b = match e.put(b"LANG", b"fr_FR") {
            Ok(v) => v,
            Err(_) => false,
        };
        let c = e.get(KEY_LANG).is(b"fr_FR");
        let d = e.overrides() == 1;
        let t = match Trace::of(&e, 0) {
            Ok(v) => v,
            Err(_) => Trace::blank(),
        };
        let f = t.first_set().map(|(k, v)| k == KEY_LANG && v.is(b"fr_FR")).unwrap_or(false);
        let g = t.winner_rank() == 1;
        let h = t.render() == String::from("all=- cat=- lang=fr_FR");
        !a && b && c && d && f && g && h
    };

    out
}

/// 判据集合。
pub fn run_unx11562_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11562.envprio");

    s.add(
        "LP01-名表与键",
        {
            let mut ok = key_of(b"LC_ALL") == Ok(KEY_ALL);
            ok = ok && key_of(b"LANG") == Ok(KEY_LANG);
            ok = ok && key_of(b"LC_CTYPE") == Ok(0);
            ok = ok && key_of(b"LC_COLLATE") == Ok(3);
            ok = ok && key_of(b"LC_IDENTIFICATION") == Ok(11);
            ok = ok && key_of(b"LC_FOO") == Err(LP_EINVAL);
            ok = ok && key_of(b"") == Err(LP_ESHORT);
            ok = ok && key_of(&[b'X'; 21]) == Err(LP_ELEN);
            ok = ok && name_of(KEY_ALL) == Some("LC_ALL");
            ok = ok && name_of(KEY_LANG) == Some("LANG");
            ok = ok && name_of(3) == Some("LC_COLLATE");
            ok = ok && name_of(14).is_none();
            ok = ok && CAT_NAMES.len() == CATS && valid_cat(11) && !valid_cat(12);
            ok
        },
        "static",
    );

    s.add(
        "LP02-三层优先级",
        {
            let mut ok = true;
            match layered_env() {
                Ok(e) => {
                    let r = Resolution::resolve(&e);
                    ok = ok && r.layer(0) == Ok(1);
                    ok = ok && r.layer(2) == Ok(2);
                    ok = ok && r.layer(3) == Ok(2);
                    ok = ok && r.value(2).map(|v| v.is(b"ja_JP.UTF-8")).unwrap_or(false);
                    ok = ok && r.value(3).map(|v| v.is(b"en_US.UTF-8")).unwrap_or(false);
                    ok = ok && r.value(0).map(|v| v.is(b"de_DE.UTF-8")).unwrap_or(false);
                }
                Err(_) => ok = false,
            }
            match all_env() {
                Ok(e) => {
                    let r = Resolution::resolve(&e);
                    ok = ok && r.layer(2) == Ok(3) && r.layer(3) == Ok(3);
                    ok = ok && r.count_layer(Layer::Category) == 0;
                    ok = ok && r.single().map(|v| v.is_c()).unwrap_or(false);
                }
                Err(_) => ok = false,
            }
            let r0 = Resolution::resolve(&Env::new());
            ok = ok
                && r0.count_layer(Layer::Default) == CATS
                && r0.count_layer(Layer::Lang) == 0
                && r0.plan() == String::from("default12 lang0 cat0 all0");
            ok
        },
        "static",
    );

    s.add(
        "LP03-逐域覆盖序",
        {
            let mut ok = true;
            match layered_env() {
                Ok(e) => {
                    let r = Resolution::resolve(&e);
                    let h = r.layer_histogram();
                    ok = ok && h == [0, 10, 2, 0];
                    ok = ok && r.count_layer(Layer::Category) == 2;
                    let mut i = 0usize;
                    while i < CATS {
                        let p = match r.pick(i) {
                            Ok(v) => v,
                            Err(_) => {
                                ok = false;
                                break;
                            }
                        };
                        let want = if i == 2 || i == 3 { 2 } else { 1 };
                        if p.layer.rank() != want {
                            ok = false;
                        }
                        if p.cat as usize != i {
                            ok = false;
                        }
                        i += 1;
                    }
                }
                Err(_) => ok = false,
            }
            let mut e = Env::new();
            let _ = e.put(b"LC_ALL", b"POSIX");
            let mut i = 0usize;
            while i < CATS {
                if let Some(n) = CAT_NAMES.get(i) {
                    let _ = e.put(n.as_bytes(), b"xx");
                }
                i += 1;
            }
            let r = Resolution::resolve(&e);
            ok = ok
                && r.count_layer(Layer::All) == CATS
                && r.distinct_values() == 1
                && r.uniform();
            ok
        },
        "static",
    );

    s.add(
        "LP04-重复覆盖",
        {
            let mut e = Env::new();
            let a = e.put(b"LANG", b"de_DE") == Ok(false);
            let b = e.put(b"LANG", b"fr_FR") == Ok(true);
            let c = e.get(KEY_LANG).is(b"fr_FR");
            let d = e.overrides() == 1;
            let f = e.put_strict(b"LANG", b"it_IT").err() == Some(LP_EDUP);
            let g = e.len() == 1;
            let mut h = Env::new();
            let i = h.put(b"LC_TIME", b"") == Ok(false);
            let j = !h.get(2).is_set();
            let k = h.put(b"LC_TIME", b"C") == Ok(true);
            let l = h.overrides() == 1;
            a && b && c && d && f && g && i && j && k && l
        },
        "static",
    );

    s.add(
        "LP05-总闸均匀性",
        {
            let a = match all_env() {
                Ok(e) => {
                    let r = Resolution::resolve(&e);
                    r.uniform() && r.single().map(|v| v.is_c()).unwrap_or(false)
                }
                Err(_) => false,
            };
            let b = match layered_env() {
                Ok(e) => {
                    let r = Resolution::resolve(&e);
                    !r.uniform() && r.single() == Err(LP_ESTATE)
                }
                Err(_) => false,
            };
            let mut c = Env::new();
            let _ = c.put(b"LC_ALL", b"");
            let _ = c.put(b"LC_TIME", b"C");
            let r = Resolution::resolve(&c);
            let d = r.layer(2) == Ok(2) && r.count_layer(Layer::All) == 0;
            let mut f = Env::new();
            let _ = f.put(b"LC_ALL", b"");
            let _ = f.put(b"LANG", b"C");
            let r2 = Resolution::resolve(&f);
            let g = r2.count_layer(Layer::Lang) == CATS && r2.uniform();
            let h = Resolution::resolve(&Env::new()).single().map(|v| !v.is_set()).unwrap_or(false);
            a && b && d && g && h
        },
        "static",
    );

    s.add(
        "LP06-文本解析",
        {
            let e = match Env::parse(b"LC_ALL=C\nLANG=en_US.UTF-8\nLC_TIME=ja_JP\n") {
                Ok(v) => v,
                Err(_) => Env::new(),
            };
            let a = e.len() == 3;
            let b = e.get(KEY_ALL).is(b"C");
            let c = e.get(KEY_LANG).is(b"en_US.UTF-8");
            let d = e.get(2).is(b"ja_JP");
            let f = match Env::parse(b"PATH=/bin\nLC_CTYPE=C\n") {
                Ok(v) => v.ignored() == 1 && v.get(0).is(b"C"),
                Err(_) => false,
            };
            let g = Env::parse(b"NOEQUALS").err() == Some(LP_EINVAL);
            let h = Env::parse(b"=x").err() == Some(LP_ESHORT);
            let i = match Env::parse(b"  LANG=C  \r\n") {
                Ok(v) => v.get(KEY_LANG).is(b"C"),
                Err(_) => false,
            };
            a && b && c && d && f && g && h && i
        },
        "static",
    );

    s.add(
        "LP07-值字节校验",
        {
            let a = Var::parse(b"C") == Ok(Var::parse(b"C").unwrap_or(Var::empty()));
            let b = Var::parse(b"de DE").err() == Some(LP_EBYTE);
            let c = Var::parse(&[0x80u8]).err() == Some(LP_EBYTE);
            let d = Var::parse(b"a/b").err() == Some(LP_EBYTE);
            let e = Var::parse(&[b'x'; VAL_CAP + 1]).err() == Some(LP_ELEN);
            let f = Var::parse(b"").map(|v| v.is_set()) == Ok(false);
            let g = Var::parse(b"C.UTF-8").map(|v| v.is_c_utf8()).unwrap_or(false);
            let h = Var::parse(b"POSIX").map(|v| v.is_c()).unwrap_or(false);
            a && b && c && d && e && f && g && h
        },
        "static",
    );

    s.add(
        "LP08-容量闸",
        {
            let mut e = Env::new();
            let mut i = 0usize;
            let mut ok = true;
            while i < ENV_CAP {
                if e.append(b"LC_CTYPE", b"xx").is_err() {
                    ok = false;
                }
                i += 1;
            }
            ok = ok && e.len() == ENV_CAP;
            ok = ok && e.append(b"LANG", b"C").err() == Some(LP_ERANGE);
            ok = ok && e.get(0).is(b"xx");
            ok = ok && e.put(b"LC_CTYPE", b"C").is_ok() && e.overrides() == 1;
            ok = ok && e.get(0).is(b"C");
            let r = Resolution::resolve(&e);
            ok = ok && r.pick(CATS).err() == Some(LP_ESLOT);
            ok = ok && Trace::of(&e, CATS).err() == Some(LP_ESLOT);
            ok
        },
        "static",
    );

    s.add(
        "LP09-渲染与计划",
        {
            let e = match layered_env() {
                Ok(v) => v,
                Err(_) => Env::new(),
            };
            let r = Resolution::resolve(&e);
            let t = r.render();
            let a = t.split('\n').count() == 12;
            let b = t.starts_with("LC_CTYPE=de_DE.UTF-8 [lang]");
            let c = t.contains("LC_COLLATE=en_US.UTF-8 [cat]");
            let d = t.contains("LC_TIME=ja_JP.UTF-8 [cat]");
            let f = r.plan() == String::from("default0 lang10 cat2 all0");
            let g = {
                let r0 = Resolution::resolve(&Env::new());
                r0.render().starts_with("LC_CTYPE=C [default]")
            };
            let h = e.render().split('\n').count() == 3;
            a && b && c && d && f && g && h
        },
        "static",
    );

    s.add(
        "LP10-候选链迹线",
        {
            let e = match layered_env() {
                Ok(v) => v,
                Err(_) => Env::new(),
            };
            let t = match Trace::of(&e, 3) {
                Ok(v) => v,
                Err(_) => Trace::blank(),
            };
            let a = t.len == 3;
            let b = t.first_set().map(|(k, v)| k == 3 && v.is(b"en_US.UTF-8")).unwrap_or(false);
            let c = t.winner_rank() == 2;
            let d = t.render() == String::from("all=- cat=en_US.UTF-8 lang=de_DE.UTF-8");
            let mut e2 = Env::new();
            let _ = e2.put(b"LC_ALL", b"C");
            let t2 = match Trace::of(&e2, 3) {
                Ok(v) => v,
                Err(_) => Trace::blank(),
            };
            let f = t2.winner_rank() == 3;
            let g = t2.first_set().map(|(k, v)| k == KEY_ALL && v.is_c()).unwrap_or(false);
            let t3 = match Trace::of(&Env::new(), 0) {
                Ok(v) => v,
                Err(_) => Trace::blank(),
            };
            let h = t3.first_set().is_none() && t3.winner_rank() == 0;
            a && b && c && d && f && g && h
        },
        "static",
    );

    s.add(
        "LP11-账本对账",
        {
            let l = build_ledger();
            let a = l.len() == SAMPLES && !l.is_empty();
            let b = l.ok_count() == 8 && l.err_count() == 6;
            let c = l.audit().is_ok();
            let d = l.sample(0).map(|v| v.value) == Some(12);
            let f = l.sample(5).map(|v| v.value) == Some(0);
            let g = l.sample(8).map(|v| v.code) == Some(LP_EINVAL);
            let h = l.sample(13).map(|v| v.code) == Some(LP_ESHORT);
            a && b && c && d && f && g && h
        },
        "static",
    );

    s.add(
        "LP12-封印重放",
        {
            let mut l = build_ledger();
            let sealed = l.seal();
            let a = sealed != 0 && l.root() == 0;
            let b = l.replay().map(|v| v == sealed).unwrap_or(false);
            let c = l.freeze() == Ok(sealed) && l.is_frozen() && l.root() == sealed;
            let d = l.register(Sample::blank()).err() == Some(LP_ESLOT);
            let e = l.rejects() == 1;
            let f = l.freeze().err() == Some(LP_EBAL) && l.rejects() == 2;
            let mut g = build_ledger();
            if let Some(dst) = g.rows.get_mut(0) {
                *dst = Sample { slot: 0, op: OP_RESOLVE, ok: true, code: 0, value: 11 };
            }
            let h = g.audit().err() == Some(LP_EVALUE);
            a && b && c && d && e && f && h
        },
        "static",
    );

    s.add(
        "LP13-冻结重放四注入",
        {
            let inj = inject_four();
            let mut ok = inj[0] && inj[1] && inj[2] && inj[3];
            let mut e = Env::new();
            let _ = e.put(b"LC_ALL", b"");
            let _ = e.put(b"LC_COLLATE", b"en_US.UTF-8");
            let _ = e.put(b"LANG", b"C");
            let r = Resolution::resolve(&e);
            ok = ok && r.layer(3) == Ok(2) && r.layer(2) == Ok(1);
            ok = ok && r.count_layer(Layer::Lang) == 11;
            let h = r.layer_histogram();
            ok = ok && h == [0, 11, 1, 0];
            ok = ok && r.validate().is_ok();
            let mut bad = Env::new();
            let _ = bad.inject(b"LC_TIME", b"bad name");
            let rb = Resolution::resolve(&bad);
            ok = ok && rb.validate().err() == Some(LP_EBYTE);
            let mut l = build_ledger();
            ok = ok && l.audit().is_ok() && l.freeze().is_ok() && l.replay().is_ok();
            ok
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_table() {
        assert_eq!(key_of(b"LC_ALL"), Ok(KEY_ALL));
        assert_eq!(key_of(b"LANG"), Ok(KEY_LANG));
        assert_eq!(key_of(b"LC_CTYPE"), Ok(0));
        assert_eq!(key_of(b"LC_IDENTIFICATION"), Ok(11));
        assert_eq!(key_of(b"LC_FOO"), Err(LP_EINVAL));
        assert_eq!(key_of(b""), Err(LP_ESHORT));
        assert_eq!(name_of(3), Some("LC_COLLATE"));
        assert_eq!(name_of(KEY_LANG), Some("LANG"));
    }

    #[test]
    fn layer_ranks() {
        assert_eq!(Layer::Default.rank(), 0);
        assert_eq!(Layer::Lang.rank(), 1);
        assert_eq!(Layer::Category.rank(), 2);
        assert_eq!(Layer::All.rank(), 3);
        assert_eq!(Layer::from_rank(2), Some(Layer::Category));
        assert_eq!(Layer::from_rank(4), None);
        assert_eq!(Layer::All.text(), "all");
    }

    #[test]
    fn env_last_wins() {
        let mut e = Env::new();
        assert_eq!(e.put(b"LANG", b"de_DE"), Ok(false));
        assert_eq!(e.put(b"LANG", b"fr_FR"), Ok(true));
        assert!(e.get(KEY_LANG).is(b"fr_FR"));
        assert_eq!(e.overrides(), 1);
        assert_eq!(e.len(), 1);
    }

    #[test]
    fn env_strict_dup() {
        let mut e = Env::new();
        assert!(e.put_strict(b"LANG", b"de_DE").is_ok());
        assert_eq!(e.put_strict(b"LANG", b"fr_FR").err(), Some(LP_EDUP));
    }

    #[test]
    fn empty_value_unsets() {
        let mut e = Env::new();
        e.put(b"LC_ALL", b"C").expect("put");
        assert!(e.all().is_set());
        e.put(b"LC_ALL", b"").expect("put");
        assert!(!e.all().is_set());
    }

    #[test]
    fn chain_three_layers() {
        let e = layered_env().expect("env");
        let r = Resolution::resolve(&e);
        assert_eq!(r.layer(0), Ok(1));
        assert_eq!(r.layer(2), Ok(2));
        assert_eq!(r.layer(3), Ok(2));
        assert_eq!(r.count_layer(Layer::Lang), 10);
        assert_eq!(r.count_layer(Layer::Category), 2);
        assert_eq!(r.distinct_values(), 3);
    }

    #[test]
    fn all_overrides_everything() {
        let e = all_env().expect("env");
        let r = Resolution::resolve(&e);
        assert!(r.uniform());
        assert_eq!(r.count_layer(Layer::All), CATS);
        assert!(r.single().expect("single").is_c());
        assert_eq!(r.value(3).expect("v").is(b"C"), true);
    }

    #[test]
    fn default_layer() {
        let r = Resolution::resolve(&Env::new());
        assert_eq!(r.count_layer(Layer::Default), CATS);
        assert_eq!(r.layer(0), Ok(0));
        assert_eq!(r.plan(), String::from("default12 lang0 cat0 all0"));
        assert!(r.single().map(|v| !v.is_set()).unwrap_or(false));
        assert_eq!(r.render().split('\n').count(), 12);
    }

    #[test]
    fn empty_lc_all_falls_through() {
        let mut e = Env::new();
        e.put(b"LC_ALL", b"").expect("put");
        e.put(b"LC_TIME", b"C").expect("put");
        let r = Resolution::resolve(&e);
        assert_eq!(r.layer(2), Ok(2));
        assert_eq!(r.count_layer(Layer::All), 0);
        assert_eq!(r.count_layer(Layer::Default), 11);
    }

    #[test]
    fn parse_text_env() {
        let e = Env::parse(b"LC_ALL=C\nLANG=en_US.UTF-8\nLC_TIME=ja_JP\n").expect("env");
        assert_eq!(e.len(), 3);
        assert!(e.get(KEY_ALL).is(b"C"));
        assert!(e.get(2).is(b"ja_JP"));
    }

    #[test]
    fn parse_ignores_foreign() {
        let e = Env::parse(b"PATH=/bin\nTERM=xterm\nLC_CTYPE=C\n").expect("env");
        assert_eq!(e.ignored(), 2);
        assert!(e.get(0).is(b"C"));
        assert_eq!(e.len(), 1);
    }

    #[test]
    fn parse_errors() {
        assert_eq!(Env::parse(b"NOEQUALS").err(), Some(LP_EINVAL));
        assert_eq!(Env::parse(b"=x").err(), Some(LP_ESHORT));
    }

    #[test]
    fn value_gates() {
        assert_eq!(Var::parse(b"de DE").err(), Some(LP_EBYTE));
        assert_eq!(Var::parse(&[b'x'; 25]).err(), Some(LP_ELEN));
        assert_eq!(Var::parse(b"").map(|v| v.is_set()), Ok(false));
        assert!(Var::parse(b"sr_RS.UTF-8@latin").is_ok());
    }

    #[test]
    fn trace_order() {
        let e = layered_env().expect("env");
        let t = Trace::of(&e, 3).expect("trace");
        assert_eq!(t.winner_rank(), 2);
        assert_eq!(t.render(), String::from("all=- cat=en_US.UTF-8 lang=de_DE.UTF-8"));
        let t2 = Trace::of(&Env::new(), 0).expect("trace");
        assert!(t2.first_set().is_none());
    }

    #[test]
    fn capacity_gate() {
        let mut e = Env::new();
        let mut i = 0usize;
        while i < ENV_CAP {
            e.append(b"LC_CTYPE", b"xx").expect("append");
            i += 1;
        }
        assert_eq!(e.len(), ENV_CAP);
        assert_eq!(e.append(b"LANG", b"C").err(), Some(LP_ERANGE));
        assert!(e.get(0).is(b"xx"));
        assert!(e.put(b"LC_CTYPE", b"C").is_ok());
        assert!(e.get(0).is(b"C"));
    }

    #[test]
    fn ledger_balances() {
        let l = build_ledger();
        assert_eq!(l.len(), 14);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
        assert_eq!(l.sample(8).map(|v| v.code), Some(LP_EINVAL));
    }

    #[test]
    fn ledger_seal_freeze() {
        let mut l = build_ledger();
        let s = l.seal();
        assert_eq!(l.replay().expect("replay"), s);
        assert_eq!(l.freeze().expect("freeze"), s);
        assert!(l.is_frozen());
        assert_eq!(l.register(Sample::blank()).err(), Some(LP_ESLOT));
    }

    #[test]
    fn render_resolution() {
        let e = layered_env().expect("env");
        let r = Resolution::resolve(&e);
        let t = r.render();
        assert_eq!(t.split('\n').count(), 12);
        assert!(t.starts_with("LC_CTYPE=de_DE.UTF-8 [lang]"));
        assert!(t.contains("LC_TIME=ja_JP.UTF-8 [cat]"));
    }

    #[test]
    fn inject_all_true() {
        let i = inject_four();
        assert!(i[0] && i[1] && i[2] && i[3]);
    }

    #[test]
    fn checks_all_pass() {
        let c = run_unx11562_checks();
        assert!(c.all_passed());
        assert_eq!(c.passed_count(), 13);
    }
}
