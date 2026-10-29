//! UNX-F11421 chown 属主:属组基本语义（0x20a10 段）
//!
//! chown 的属主实参有三种合法写法：全档 `user:group`、只改属主 `user` 或
//! `user:`、只改属组 `:group`。分隔符两侧同时为空（`:` 与空串）视为非法，
//! 不做任何推定。每个字段先按纯数字直用解析（十进制，允许前导零），只有
//! 非数字串才去用户/组名表里查找；数字串即使与表内的字面名字冲突也优先
//! 按数字解释——这与基准机行为一致，也是本模型单独设判据的边角。
//! 名字查找失败返回 ENAME，数字越界返回 ENUM，字段为空返回 ESPEC。
//!
//! 权限面按 euid 分层：root 可任意改属；非 root 只允许把属主改成自己的
//! uid、把属组改成自己的 gid，其余一律 EPERM，并且拒绝发生在任何落盘
//! 之前，文件节点保持原样。未在实参中出现的字段一律保持原值，不允许
//! 隐式归零——这是 chown 与 chmod 掩码语义最容易混淆的地方。
//!
//! 账本登记十二个样本：七个可应用、五个被拒。每个样本保存实参副本、调用
//! 者身份、改前节点与改后节点，重放时逐格重算并与票面比对。结构面另有
//! 编号越界、重复格、乱序、超长实参与冻结后写入五道门，计入拒绝计数而
//! 不动已落格的账目，保证任何时刻都能整体复算。

use crate::checks::CheckSet;
use crate::alloc::format;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20a10;

/// 编号非法。
pub const CW_EID: u32 = SEG_BASE + 0x01;
/// 状态顺序错误。
pub const CW_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const CW_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const CW_EFROZEN: u32 = SEG_BASE + 0x04;
/// 实参形态非法。
pub const CW_ESPEC: u32 = SEG_BASE + 0x05;
/// 名字不存在。
pub const CW_ENAME: u32 = SEG_BASE + 0x06;
/// 数字越界。
pub const CW_ENUM: u32 = SEG_BASE + 0x07;
/// 权限不足。
pub const CW_EPERM: u32 = SEG_BASE + 0x08;
/// 实参超长。
pub const CW_ECAP: u32 = SEG_BASE + 0x09;
/// 账本失衡。
pub const CW_EBAL: u32 = SEG_BASE + 0x0a;
/// 票号不一致。
pub const CW_ESEAL: u32 = SEG_BASE + 0x0b;
/// 观测不存在的格。
pub const CW_ENOBS: u32 = SEG_BASE + 0x0c;

/// 判决：已应用。
pub const V_APPLIED: u32 = 0;

/// 实参缓冲上限。
pub const SPEC_CAP: usize = 24;
/// 样本格数。
pub const SAMPLES: usize = 12;
/// 名字表容量。
pub const NAME_CAP: usize = 6;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 名字表项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdName {
    pub id: u32,
    pub name: &'static str,
}

/// 用户表。注意 `1000` 这一项故意用字面数字名，用来检验数字直用优先。
pub const USERS: [IdName; NAME_CAP] = [
    IdName { id: 0, name: "root" },
    IdName { id: 1, name: "daemon" },
    IdName { id: 2, name: "bin" },
    IdName { id: 33, name: "www" },
    IdName { id: 77, name: "1000" },
    IdName { id: 1000, name: "user" },
];

/// 组表。
pub const GROUPS: [IdName; NAME_CAP] = [
    IdName { id: 0, name: "root" },
    IdName { id: 1, name: "daemon" },
    IdName { id: 2, name: "bin" },
    IdName { id: 33, name: "www" },
    IdName { id: 55, name: "0" },
    IdName { id: 1000, name: "user" },
];

/// 调用者身份。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caller {
    pub uid: u32,
    pub gid: u32,
    pub root: bool,
}

impl Caller {
    /// root。
    pub const fn root() -> Caller {
        Caller { uid: 0, gid: 0, root: true }
    }

    /// 普通用户。
    pub const fn user(uid: u32, gid: u32) -> Caller {
        Caller { uid, gid, root: false }
    }
}

/// 文件节点（只保留本卡关心的身份字段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    pub id: u16,
    pub uid: u32,
    pub gid: u32,
}

impl Node {
    /// 构造节点。
    pub const fn make(id: u16, uid: u32, gid: u32) -> Node {
        Node { id, uid, gid }
    }
}

/// 解析后的实参：`None` 表示该字段不修改。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OwnerSpec {
    pub user: Option<u32>,
    pub group: Option<u32>,
}

impl OwnerSpec {
    /// 空实参。
    pub const fn none() -> OwnerSpec {
        OwnerSpec { user: None, group: None }
    }

    /// 是否有任何字段需要落盘。
    pub const fn touches_any(&self) -> bool {
        self.user.is_some() || self.group.is_some()
    }
}

fn is_digits(s: &str) -> bool {
    let b = s.as_bytes();
    if b.is_empty() {
        return false;
    }
    let mut i = 0;
    while i < b.len() {
        if b[i] < b'0' || b[i] > b'9' {
            return false;
        }
        i += 1;
    }
    true
}

fn parse_num(s: &str) -> Result<u32, u32> {
    let b = s.as_bytes();
    if b.len() > 10 {
        return Err(CW_ENUM);
    }
    let mut v: u64 = 0;
    let mut i = 0;
    while i < b.len() {
        v = v * 10 + (b[i] - b'0') as u64;
        if v > 0x7fff_ffff {
            return Err(CW_ENUM);
        }
        i += 1;
    }
    Ok(v as u32)
}

fn lookup(table: &[IdName], name: &str) -> Option<u32> {
    let mut i = 0;
    while i < table.len() {
        if table[i].name == name {
            return Some(table[i].id);
        }
        i += 1;
    }
    None
}

fn parse_field(tok: &str, table: &[IdName]) -> Result<u32, u32> {
    if tok.is_empty() {
        return Err(CW_ESPEC);
    }
    if is_digits(tok) {
        return parse_num(tok);
    }
    match lookup(table, tok) {
        Some(v) => Ok(v),
        None => Err(CW_ENAME),
    }
}

fn find_colon(spec: &str) -> Option<usize> {
    let b = spec.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b':' {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 解析属主实参。
pub fn parse_spec(spec: &str) -> Result<OwnerSpec, u32> {
    if spec.is_empty() || spec.len() > SPEC_CAP {
        return Err(CW_ESPEC);
    }
    match find_colon(spec) {
        None => Ok(OwnerSpec { user: Some(parse_field(spec, &USERS)?), group: None }),
        Some(at) => {
            let u = match spec.get(..at) {
                Some(v) => v,
                None => return Err(CW_ESPEC),
            };
            let g = match spec.get(at + 1..) {
                Some(v) => v,
                None => return Err(CW_ESPEC),
            };
            if u.is_empty() && g.is_empty() {
                return Err(CW_ESPEC);
            }
            let user = if u.is_empty() { None } else { Some(parse_field(u, &USERS)?) };
            let group = if g.is_empty() { None } else { Some(parse_field(g, &GROUPS)?) };
            Ok(OwnerSpec { user, group })
        }
    }
}

/// 把实参施加到节点：解析、鉴权、落地三步，任一步拒绝都保持节点原样。
pub fn apply(spec: &str, caller: Caller, node: Node) -> Result<Node, u32> {
    let s = parse_spec(spec)?;
    if !caller.root {
        if let Some(u) = s.user {
            if u != caller.uid {
                return Err(CW_EPERM);
            }
        }
        if let Some(g) = s.group {
            if g != caller.gid {
                return Err(CW_EPERM);
            }
        }
    }
    Ok(Node {
        id: node.id,
        uid: match s.user {
            Some(u) => u,
            None => node.uid,
        },
        gid: match s.group {
            Some(g) => g,
            None => node.gid,
        },
    })
}

/// 一格样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub spec: [u8; SPEC_CAP],
    pub len: u8,
    pub caller: Caller,
    pub before: Node,
}

impl Sample {
    /// 实参文本。
    pub fn spec_str(&self) -> &str {
        let n = self.len as usize;
        if n > SPEC_CAP {
            return "";
        }
        match core::str::from_utf8(self.spec.get(..n).unwrap_or(&[])) {
            Ok(v) => v,
            Err(_) => "",
        }
    }
}

/// 一格的记账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    pub before: Node,
    pub after: Node,
    pub verdict: u32,
}

/// 改属账本。
pub struct ChownLedger {
    samples: Vec<Sample>,
    records: Vec<Record>,
    frozen: bool,
    root: u64,
    rejects: u32,
    denials: u32,
}

impl ChownLedger {
    /// 空账本。
    pub fn new() -> ChownLedger {
        ChownLedger {
            samples: Vec::new(),
            records: Vec::new(),
            frozen: false,
            root: 0,
            rejects: 0,
            denials: 0,
        }
    }

    /// 登记一格。
    pub fn register(&mut self, slot: u8, spec: &str, caller: Caller, node: Node) -> Result<u32, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(CW_EFROZEN);
        }
        if slot as usize >= SAMPLES {
            self.rejects += 1;
            return Err(CW_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(CW_EDUP);
        }
        if slot as usize != self.samples.len() {
            self.rejects += 1;
            return Err(CW_EORDER);
        }
        if spec.len() > SPEC_CAP {
            self.rejects += 1;
            return Err(CW_ECAP);
        }
        let mut buf = [0u8; SPEC_CAP];
        let b = spec.as_bytes();
        let mut i = 0;
        while i < b.len() {
            buf[i] = b[i];
            i += 1;
        }
        let mut after = node;
        let verdict = match apply(spec, caller, node) {
            Ok(v) => {
                after = v;
                V_APPLIED
            }
            Err(code) => {
                self.denials += 1;
                code
            }
        };
        self.samples.push(Sample { spec: buf, len: b.len() as u8, caller, before: node });
        self.records.push(Record { before: node, after, verdict });
        Ok(verdict)
    }

    /// 已登记格数。
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// 取实参文本。
    pub fn spec_at(&self, i: usize) -> Option<&str> {
        match self.samples.get(i) {
            Some(s) => Some(s.spec_str()),
            None => None,
        }
    }

    /// 取调用者。
    pub fn caller_at(&self, i: usize) -> Option<Caller> {
        match self.samples.get(i) {
            Some(s) => Some(s.caller),
            None => None,
        }
    }

    /// 取改前节点。
    pub fn before(&self, i: usize) -> Option<Node> {
        match self.records.get(i) {
            Some(r) => Some(r.before),
            None => None,
        }
    }

    /// 取改后节点。
    pub fn after(&self, i: usize) -> Option<Node> {
        match self.records.get(i) {
            Some(r) => Some(r.after),
            None => None,
        }
    }

    /// 取判决。
    pub fn verdict(&self, i: usize) -> Option<u32> {
        match self.records.get(i) {
            Some(r) => Some(r.verdict),
            None => None,
        }
    }

    /// 已应用格数。
    pub fn applied(&self) -> u32 {
        let mut n = 0u32;
        for r in &self.records {
            if r.verdict == V_APPLIED {
                n += 1;
            }
        }
        n
    }

    /// 语义拒绝数。
    pub fn denials(&self) -> u32 {
        self.denials
    }

    /// 结构拒绝数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 观测一格：重算实参后的属主属组，并与票面比对。
    pub fn observe(&self, i: usize) -> Result<OwnerSpec, u32> {
        let s = match self.samples.get(i) {
            Some(v) => v,
            None => return Err(CW_ENOBS),
        };
        let r = match self.records.get(i) {
            Some(v) => v,
            None => return Err(CW_ENOBS),
        };
        let parsed = parse_spec(s.spec_str())?;
        let expect = match apply(s.spec_str(), s.caller, s.before) {
            Ok(v) => v,
            Err(_) => s.before,
        };
        if expect != r.after || expect.uid == s.before.uid && expect.gid == s.before.gid {
            if r.verdict == V_APPLIED {
                return Err(CW_EBAL);
            }
        }
        Ok(parsed)
    }

    /// 账本自洽。
    pub fn audit(&self) -> Result<(), u32> {
        if self.samples.len() != self.records.len() {
            return Err(CW_EBAL);
        }
        let mut denied = 0u32;
        for i in 0..self.records.len() {
            let r = self.records[i];
            if r.verdict != V_APPLIED {
                denied += 1;
                if r.after != r.before {
                    return Err(CW_EBAL);
                }
            }
        }
        if denied != self.denials {
            return Err(CW_EBAL);
        }
        Ok(())
    }

    /// 账本折叠。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = mix(h, self.samples.len() as u64);
        for i in 0..self.samples.len() {
            let s = self.samples[i];
            let r = self.records[i];
            h = mix(h, s.len as u64);
            let mut j = 0;
            while j < s.len as usize {
                h ^= s.spec[j] as u64;
                h = h.wrapping_mul(FNV_PRIME);
                j += 1;
            }
            h = mix(h, s.caller.uid as u64);
            h = mix(h, s.caller.gid as u64);
            h = mix(h, if s.caller.root { 1 } else { 0 });
            h = mix(h, r.before.uid as u64);
            h = mix(h, r.before.gid as u64);
            h = mix(h, r.after.uid as u64);
            h = mix(h, r.after.gid as u64);
            h = mix(h, r.verdict as u64);
        }
        h = mix(h, self.denials as u64);
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(CW_EFROZEN);
        }
        self.audit()?;
        self.root = self.fold();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放：逐格重算。
    pub fn replay(&self) -> Result<u64, u32> {
        let mut h = FNV_OFF;
        h = mix(h, self.samples.len() as u64);
        let mut denied = 0u32;
        for i in 0..self.samples.len() {
            let s = self.samples[i];
            let r = self.records[i];
            let (after, verdict) = match apply(s.spec_str(), s.caller, s.before) {
                Ok(v) => (v, V_APPLIED),
                Err(code) => {
                    denied += 1;
                    (s.before, code)
                }
            };
            if after != r.after || verdict != r.verdict {
                return Err(CW_ESEAL);
            }
            h = mix(h, s.len as u64);
            let mut j = 0;
            while j < s.len as usize {
                h ^= s.spec[j] as u64;
                h = h.wrapping_mul(FNV_PRIME);
                j += 1;
            }
            h = mix(h, s.caller.uid as u64);
            h = mix(h, s.caller.gid as u64);
            h = mix(h, if s.caller.root { 1 } else { 0 });
            h = mix(h, r.before.uid as u64);
            h = mix(h, r.before.gid as u64);
            h = mix(h, r.after.uid as u64);
            h = mix(h, r.after.gid as u64);
            h = mix(h, r.verdict as u64);
        }
        if denied != self.denials {
            return Err(CW_ESEAL);
        }
        h = mix(h, self.denials as u64);
        Ok(h)
    }

    /// 冻结根。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

fn mix(h: u64, v: u64) -> u64 {
    let mut h = h;
    let mut i = 0u32;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xff;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 按实参文本取名字表的长度，供摘要使用。
pub fn name_tables_len() -> usize {
    NAME_CAP * 2
}

/// 构建标准账本：7 应用 + 5 拒绝。
pub fn build_ledger() -> ChownLedger {
    let mut l = ChownLedger::new();
    let root = Caller::root();
    let u1000 = Caller::user(1000, 1000);
    let _ = l.register(0, "root:root", root, Node::make(1, 500, 600));
    let _ = l.register(1, "daemon", root, Node::make(2, 500, 600));
    let _ = l.register(2, ":bin", root, Node::make(3, 500, 600));
    let _ = l.register(3, "1000:1000", root, Node::make(4, 500, 600));
    let _ = l.register(4, "user:user", u1000, Node::make(5, 1000, 1000));
    let _ = l.register(5, ":user", u1000, Node::make(6, 1000, 500));
    let _ = l.register(6, "root:root", u1000, Node::make(7, 1000, 1000));
    let _ = l.register(7, "nosuch", root, Node::make(8, 500, 600));
    let _ = l.register(8, "root:nosuch", root, Node::make(9, 500, 600));
    let _ = l.register(9, ":", root, Node::make(10, 500, 600));
    let _ = l.register(10, "", root, Node::make(11, 500, 600));
    let _ = l.register(11, "1000", u1000, Node::make(12, 500, 600));
    l
}

/// 四路注入：基线、非法实参、越权、冻结后写入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.applied() == 7 && l.denials() == 5 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    out[0] = base && root != 0 && rep == Ok(root);

    out[1] = parse_spec(":") == Err(CW_ESPEC)
        && parse_spec("") == Err(CW_ESPEC)
        && parse_spec("ghost") == Err(CW_ENAME)
        && parse_spec("root:ghost") == Err(CW_ENAME);

    let n = Node::make(20, 1000, 1000);
    out[2] = apply("root:root", Caller::user(1000, 1000), n) == Err(CW_EPERM)
        && apply("1000:1000", Caller::user(1000, 1000), n) == Ok(Node::make(20, 1000, 1000))
        && apply(":0", Caller::user(1000, 1000), n) == Err(CW_EPERM);

    let mut f = build_ledger();
    let froz = f.freeze().is_ok();
    let deny = f.register(12, "root", Caller::root(), Node::make(30, 0, 0));
    out[3] = froz && deny == Err(CW_EFROZEN) && f.rejects() == 1 && f.len() == SAMPLES;

    out
}

/// 十三项判据。
pub fn run_unx11421_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11421.chown");
    let l = build_ledger();

    s.add(
        "CW1-账本十二格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.before(0).is_some(),
        "static",
    );

    s.add_dyn(
        "CW2-七应五拒分布正确",
        l.applied() == 7 && l.denials() == 5 && l.audit().is_ok(),
        &format!("applied={} denials={}", l.applied(), l.denials()),
    );

    let mut r = ChownLedger::new();
    let a = r.register(0, "root", Caller::root(), Node::make(1, 1, 1)) == Ok(V_APPLIED);
    let dup = r.register(0, "bin", Caller::root(), Node::make(2, 1, 1)) == Err(CW_EDUP);
    let ord = r.register(4, "bin", Caller::root(), Node::make(3, 1, 1)) == Err(CW_EORDER);
    let id = r.register(12, "bin", Caller::root(), Node::make(4, 1, 1)) == Err(CW_EID);
    let cap = r.register(1, "aaaaaaaaaaaaaaaaaaaaaaaaaaaa", Caller::root(), Node::make(5, 1, 1))
        == Err(CW_ECAP);
    let _ = r.freeze();
    let fro = r.register(1, "bin", Caller::root(), Node::make(6, 1, 1)) == Err(CW_EFROZEN);
    s.add_dyn(
        "CW3-结构拒绝矩阵五门",
        a && dup && ord && id && cap && fro && r.rejects() == 5 && r.len() == 1,
        &format!("rejects={} len={}", r.rejects(), r.len()),
    );

    s.add(
        "CW4-全档解析",
        parse_spec("root:root") == Ok(OwnerSpec { user: Some(0), group: Some(0) })
            && parse_spec("daemon:bin") == Ok(OwnerSpec { user: Some(1), group: Some(2) }),
        "static",
    );

    s.add(
        "CW5-单档三写法",
        parse_spec("daemon:") == Ok(OwnerSpec { user: Some(1), group: None })
            && parse_spec(":bin") == Ok(OwnerSpec { user: None, group: Some(2) })
            && parse_spec("www") == Ok(OwnerSpec { user: Some(33), group: None }),
        "static",
    );

    s.add_dyn(
        "CW6-数字直用优先",
        parse_spec("1000:1000") == Ok(OwnerSpec { user: Some(1000), group: Some(1000) })
            && parse_spec("033") == Ok(OwnerSpec { user: Some(33), group: None }),
        &format!("{:?}", parse_spec("1000")),
    );

    s.add(
        "CW7-名字查解析",
        parse_spec("user") == Ok(OwnerSpec { user: Some(1000), group: None })
            && parse_spec(":www") == Ok(OwnerSpec { user: None, group: Some(33) }),
        "static",
    );

    s.add(
        "CW8-未知名拒绝",
        parse_spec("ghost") == Err(CW_ENAME)
            && parse_spec("root:ghost") == Err(CW_ENAME)
            && parse_spec("ghost:root") == Err(CW_ENAME),
        "static",
    );

    s.add(
        "CW9-纯数字名特案",
        lookup(&USERS, "1000") == Some(77)
            && parse_spec("1000") == Ok(OwnerSpec { user: Some(1000), group: None }),
        "static",
    );

    let n = Node::make(40, 1000, 1000);
    s.add(
        "CW10-权限边界",
        apply("1000:1000", Caller::user(1000, 1000), n) == Ok(n)
            && apply(":1000", Caller::user(1000, 1000), n) == Ok(n)
            && apply("0:0", Caller::user(1000, 1000), n) == Err(CW_EPERM)
            && apply(":0", Caller::user(1000, 1000), n) == Err(CW_EPERM)
            && apply("0:0", Caller::root(), n) == Ok(Node::make(40, 0, 0)),
        "static",
    );

    s.add(
        "CW11-未改面保持原值",
        l.after(1) == Some(Node::make(2, 1, 600))
            && l.after(2) == Some(Node::make(3, 500, 2))
            && l.after(0) == Some(Node::make(1, 0, 0)),
        "static",
    );

    let mut f = build_ledger();
    let rep = f.replay();
    let f1 = f.freeze();
    let root = match f1 {
        Ok(v) => v,
        Err(_) => 0,
    };
    let locked = f.register(12, "root", Caller::root(), Node::make(50, 0, 0)) == Err(CW_EFROZEN);
    let again = f.freeze() == Err(CW_EFROZEN);
    s.add_dyn(
        "CW12-冻结重放幂等",
        root != 0 && rep == Ok(root) && locked && again && f.root() == root,
        &format!("root={:016x}", root),
    );

    let inj = inject_four();
    s.add_dyn(
        "CW13-四路注入全绿",
        inj == [true; 4],
        &format!("inject={:?}", inj),
    );

    s
}

/// 十二样本文本摘要。
pub fn summary(l: &ChownLedger) -> alloc::string::String {
    format!("applied={} denials={} rejects={}", l.applied(), l.denials(), l.rejects())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_spec_forms() {
        assert_eq!(parse_spec("daemon:bin"), Ok(OwnerSpec { user: Some(1), group: Some(2) }));
        assert_eq!(parse_spec("daemon:"), Ok(OwnerSpec { user: Some(1), group: None }));
        assert_eq!(parse_spec(":bin"), Ok(OwnerSpec { user: None, group: Some(2) }));
        assert_eq!(parse_spec("www"), Ok(OwnerSpec { user: Some(33), group: None }));
        assert_eq!(parse_spec(":"), Err(CW_ESPEC));
        assert_eq!(parse_spec(""), Err(CW_ESPEC));
    }

    #[test]
    fn numeric_precedence_over_literal_name() {
        assert_eq!(lookup(&USERS, "1000"), Some(77));
        assert_eq!(parse_spec("1000"), Ok(OwnerSpec { user: Some(1000), group: None }));
        assert_eq!(parse_spec("033"), Ok(OwnerSpec { user: Some(33), group: None }));
        assert_eq!(parse_spec("9999999999"), Err(CW_ENUM));
    }

    #[test]
    fn permission_boundary_keeps_node() {
        let n = Node::make(7, 1000, 1000);
        assert_eq!(apply("root:root", Caller::user(1000, 1000), n), Err(CW_EPERM));
        assert_eq!(apply(":0", Caller::user(1000, 1000), n), Err(CW_EPERM));
        assert_eq!(apply("1000:1000", Caller::user(1000, 1000), n), Ok(n));
        assert_eq!(apply(":1000", Caller::user(1000, 1000), n), Ok(n));
        assert_eq!(apply("daemon", Caller::root(), n), Ok(Node::make(7, 1, 1000)));
    }

    #[test]
    fn ledger_distribution_and_balance() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.applied(), 7);
        assert_eq!(l.denials(), 5);
        assert!(l.audit().is_ok());
        assert_eq!(l.after(6), l.before(6));
        assert_eq!(l.after(9), l.before(9));
    }

    #[test]
    fn refusal_matrix_codes() {
        let mut l = ChownLedger::new();
        assert_eq!(l.register(0, "root", Caller::root(), Node::make(1, 1, 1)), Ok(V_APPLIED));
        assert_eq!(l.register(0, "bin", Caller::root(), Node::make(2, 1, 1)), Err(CW_EDUP));
        assert_eq!(l.register(3, "bin", Caller::root(), Node::make(3, 1, 1)), Err(CW_EORDER));
        assert_eq!(l.register(12, "bin", Caller::root(), Node::make(4, 1, 1)), Err(CW_EID));
        assert_eq!(
            l.register(1, "aaaaaaaaaaaaaaaaaaaaaaaaaaaa", Caller::root(), Node::make(5, 1, 1)),
            Err(CW_ECAP)
        );
        let _ = l.freeze();
        assert_eq!(l.register(1, "bin", Caller::root(), Node::make(6, 1, 1)), Err(CW_EFROZEN));
        assert_eq!(l.rejects(), 5);
        assert_eq!(l.len(), 1);
    }

    #[test]
    fn freeze_replay_idempotent() {
        let mut l = build_ledger();
        let rep = l.replay();
        let f1 = l.freeze();
        let root = match f1 {
            Ok(v) => v,
            Err(_) => 0,
        };
        assert_ne!(root, 0);
        assert_eq!(rep, Ok(root));
        assert_eq!(l.freeze(), Err(CW_EFROZEN));
        assert_eq!(l.root(), root);
        assert!(l.replay().is_ok());
        assert!(l.audit().is_ok());
    }

    #[test]
    fn checks_green_and_injection() {
        let s = run_unx11421_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
        assert_eq!(inject_four(), [true; 4]);
        assert_eq!(name_tables_len(), 12);
        assert!(summary(&build_ledger()).starts_with("applied=7"));
    }
}