//! UNX-F11423 chown 名字与数字解析序（0x20a30 段）
//!
//! 属主实参的解析是一条有严格先后次序的流水线：先做词法判定（空串、
//! 多个冒号、超长都在这层被挡），再解析用户字段，最后解析组字段。用户
//! 字段失败时组字段根本不会被查表——这条短路是账本要记下来的事实，而不
//! 只是实现细节，因为基准机的报错信息与失败位置都依赖它。
//!
//! 单个字段的判定顺序是“数字优先”：纯十进制串直接当编号使用，哪怕名表
//! 里存在同字面的条目；只有含非数字字符的串才进名表。数字带前导零按十
//! 进制解释（`033` 是 33 而不是八进制 27），超过 0x7fff_ffff 报 ENUM；
//! 负号与加号都不是数字，会落到名表里并因查不到而报 ENAME。
//!
//! 名表允许重名，重名时取表中靠前的一项，并记录被遮蔽的条目数，避免
//! “解析结果取决于遍历方向”的隐性不确定。账本登记十二个样本，按阶段
//! 统计词法失败、用户失败、组失败与成功四类，逐格保存三个字段的中间
//! 结论，冻结前整体重放。

use crate::checks::CheckSet;
use crate::alloc::format;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20a30;

/// 编号非法。
pub const RA_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const RA_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const RA_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const RA_EFROZEN: u32 = SEG_BASE + 0x04;
/// 词法非法。
pub const RA_ELEX: u32 = SEG_BASE + 0x05;
/// 名字不存在。
pub const RA_ENAME: u32 = SEG_BASE + 0x06;
/// 数字越界。
pub const RA_ENUM: u32 = SEG_BASE + 0x07;
/// 实参超长。
pub const RA_ECAP: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const RA_EBAL: u32 = SEG_BASE + 0x09;
/// 票号不一致。
pub const RA_ESEAL: u32 = SEG_BASE + 0x0a;
/// 观测不存在的格。
pub const RA_ENOBS: u32 = SEG_BASE + 0x0b;
/// 阶段倒流。
pub const RA_ESTAGE: u32 = SEG_BASE + 0x0c;

/// 阶段：词法。
pub const ST_LEX: u8 = 0;
/// 阶段：用户字段。
pub const ST_USER: u8 = 1;
/// 阶段：组字段。
pub const ST_GROUP: u8 = 2;
/// 阶段：完成。
pub const ST_DONE: u8 = 3;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 实参缓冲上限。
pub const SPEC_CAP: usize = 24;
/// 样本格数。
pub const SAMPLES: usize = 12;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 名表项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: u32,
    pub name: &'static str,
    pub shadowed: bool,
}

/// 用户表：`1000` 是字面名字（编号 77），用来检验数字优先；`dup` 重名一项。
pub const USERS: [Entry; 8] = [
    Entry { id: 0, name: "root", shadowed: false },
    Entry { id: 1, name: "daemon", shadowed: false },
    Entry { id: 2, name: "bin", shadowed: false },
    Entry { id: 33, name: "www", shadowed: false },
    Entry { id: 77, name: "1000", shadowed: false },
    Entry { id: 88, name: "dup", shadowed: false },
    Entry { id: 99, name: "dup", shadowed: true },
    Entry { id: 1000, name: "user", shadowed: false },
];

/// 组表。
pub const GROUPS: [Entry; 6] = [
    Entry { id: 0, name: "root", shadowed: false },
    Entry { id: 1, name: "daemon", shadowed: false },
    Entry { id: 2, name: "bin", shadowed: false },
    Entry { id: 33, name: "www", shadowed: false },
    Entry { id: 44, name: "0", shadowed: false },
    Entry { id: 100, name: "staff", shadowed: false },
];

/// 单字段解析结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Field {
    pub id: u32,
    pub numeric: bool,
}

/// 整体结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolution {
    pub user: Option<u32>,
    pub group: Option<u32>,
    pub stage: u8,
    pub verdict: u32,
}

impl Resolution {
    /// 空结论。
    pub const fn none() -> Resolution {
        Resolution { user: None, group: None, stage: ST_LEX, verdict: RA_ELEX }
    }
}

fn digits(s: &str) -> bool {
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

fn numeric(s: &str) -> Result<u32, u32> {
    let b = s.as_bytes();
    if b.len() > 10 {
        return Err(RA_ENUM);
    }
    let mut v: u64 = 0;
    let mut i = 0;
    while i < b.len() {
        v = v * 10 + (b[i] - b'0') as u64;
        if v > 0x7fff_ffff {
            return Err(RA_ENUM);
        }
        i += 1;
    }
    Ok(v as u32)
}

fn lookup(table: &[Entry], name: &str) -> Option<Entry> {
    let mut i = 0;
    while i < table.len() {
        if table[i].name == name {
            return Some(table[i]);
        }
        i += 1;
    }
    None
}

/// 单字段：数字优先，其次名表。
pub fn resolve_field(tok: &str, table: &[Entry]) -> Result<Field, u32> {
    if tok.is_empty() {
        return Err(RA_ELEX);
    }
    if digits(tok) {
        return Ok(Field { id: numeric(tok)?, numeric: true });
    }
    match lookup(table, tok) {
        Some(e) => Ok(Field { id: e.id, numeric: false }),
        None => Err(RA_ENAME),
    }
}

/// 统计名表里被同名前项遮蔽的条目数。
pub fn shadow_count(table: &[Entry]) -> u32 {
    let mut n = 0u32;
    for e in table {
        if e.shadowed {
            n += 1;
        }
    }
    n
}

/// 完整解析：词法 → 用户 → 组，短路规则内建。
pub fn resolve(spec: &str) -> Result<Resolution, u32> {
    if spec.is_empty() || spec.len() > SPEC_CAP {
        return Err(RA_ELEX);
    }
    let b = spec.as_bytes();
    let mut colon = None;
    let mut i = 0;
    let mut extra = false;
    while i < b.len() {
        if b[i] == b':' {
            if colon.is_some() {
                extra = true;
            }
            colon = Some(i);
        }
        i += 1;
    }
    if extra {
        return Err(RA_ELEX);
    }
    match colon {
        None => {
            let f = resolve_field(spec, &USERS)?;
            Ok(Resolution { user: Some(f.id), group: None, stage: ST_DONE, verdict: V_OK })
        }
        Some(at) => {
            let u = match spec.get(..at) {
                Some(v) => v,
                None => return Err(RA_ELEX),
            };
            let g = match spec.get(at + 1..) {
                Some(v) => v,
                None => return Err(RA_ELEX),
            };
            if u.is_empty() && g.is_empty() {
                return Err(RA_ELEX);
            }
            let user = if u.is_empty() {
                None
            } else {
                match resolve_field(u, &USERS) {
                    Ok(f) => Some(f.id),
                    Err(code) => return Err(code),
                }
            };
            let group = if g.is_empty() {
                None
            } else {
                match resolve_field(g, &GROUPS) {
                    Ok(f) => Some(f.id),
                    Err(code) => return Err(code),
                }
            };
            Ok(Resolution { user, group, stage: ST_DONE, verdict: V_OK })
        }
    }
}

/// 解释一格的流水线轨迹：返回 (阶段, 用户字段是否已查, 组字段是否已查, 结论)。
pub fn trace(spec: &str) -> (u8, bool, bool, u32) {
    if spec.is_empty() || spec.len() > SPEC_CAP {
        return (ST_LEX, false, false, RA_ELEX);
    }
    let mut colons = 0usize;
    let mut at = 0usize;
    let b = spec.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b':' {
            colons += 1;
            at = i;
        }
        i += 1;
    }
    if colons > 1 {
        return (ST_LEX, false, false, RA_ELEX);
    }
    if colons == 0 {
        return match resolve_field(spec, &USERS) {
            Ok(_) => (ST_DONE, true, false, V_OK),
            Err(c) => (ST_USER, true, false, c),
        };
    }
    let u = match spec.get(..at) {
        Some(v) => v,
        None => return (ST_LEX, false, false, RA_ELEX),
    };
    let g = match spec.get(at + 1..) {
        Some(v) => v,
        None => return (ST_LEX, false, false, RA_ELEX),
    };
    if u.is_empty() && g.is_empty() {
        return (ST_LEX, false, false, RA_ELEX);
    }
    if !u.is_empty() {
        if let Err(c) = resolve_field(u, &USERS) {
            return (ST_USER, true, false, c);
        }
    }
    if !g.is_empty() {
        if let Err(c) = resolve_field(g, &GROUPS) {
            return (ST_GROUP, true, true, c);
        }
    }
    (ST_DONE, !u.is_empty(), !g.is_empty(), V_OK)
}

/// 一格样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub bytes: [u8; SPEC_CAP],
    pub len: u8,
}

impl Sample {
    /// 实参文本。
    pub fn text(&self) -> &str {
        let n = self.len as usize;
        if n > SPEC_CAP {
            return "";
        }
        match core::str::from_utf8(self.bytes.get(..n).unwrap_or(&[])) {
            Ok(v) => v,
            Err(_) => "",
        }
    }
}

/// 一格记账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rec {
    pub stage: u8,
    pub user_seen: bool,
    pub group_seen: bool,
    pub verdict: u32,
    pub user: Option<u32>,
    pub group: Option<u32>,
}

/// 解析账本。
pub struct ResolveLedger {
    samples: Vec<Sample>,
    recs: Vec<Rec>,
    frozen: bool,
    root: u64,
    rejects: u32,
    failures: u32,
}

impl ResolveLedger {
    /// 空账本。
    pub fn new() -> ResolveLedger {
        ResolveLedger {
            samples: Vec::new(),
            recs: Vec::new(),
            frozen: false,
            root: 0,
            rejects: 0,
            failures: 0,
        }
    }

    /// 登记一格。
    pub fn register(&mut self, slot: u8, spec: &str) -> Result<u32, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(RA_EFROZEN);
        }
        if slot as usize >= SAMPLES {
            self.rejects += 1;
            return Err(RA_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(RA_EDUP);
        }
        if slot as usize != self.samples.len() {
            self.rejects += 1;
            return Err(RA_EORDER);
        }
        if spec.len() > SPEC_CAP {
            self.rejects += 1;
            return Err(RA_ECAP);
        }
        let mut bytes = [0u8; SPEC_CAP];
        let b = spec.as_bytes();
        let mut i = 0;
        while i < b.len() {
            bytes[i] = b[i];
            i += 1;
        }
        let (stage, user_seen, group_seen, verdict) = trace(spec);
        let mut user = None;
        let mut group = None;
        if verdict == V_OK {
            if let Ok(r) = resolve(spec) {
                user = r.user;
                group = r.group;
            }
        } else {
            self.failures += 1;
        }
        self.samples.push(Sample { bytes, len: b.len() as u8 });
        self.recs.push(Rec { stage, user_seen, group_seen, verdict, user, group });
        Ok(verdict)
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// 实参文本。
    pub fn spec_at(&self, i: usize) -> Option<&str> {
        match self.samples.get(i) {
            Some(s) => Some(s.text()),
            None => None,
        }
    }

    /// 记录。
    pub fn rec(&self, i: usize) -> Option<Rec> {
        self.recs.get(i).copied()
    }

    /// 某阶段失败数。
    pub fn failures_at(&self, stage: u8) -> u32 {
        let mut n = 0u32;
        for r in &self.recs {
            if r.verdict != V_OK && r.stage == stage {
                n += 1;
            }
        }
        n
    }

    /// 成功数。
    pub fn ok_count(&self) -> u32 {
        let mut n = 0u32;
        for r in &self.recs {
            if r.verdict == V_OK {
                n += 1;
            }
        }
        n
    }

    /// 失败总数。
    pub fn failures(&self) -> u32 {
        self.failures
    }

    /// 结构拒绝数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 观测一格。
    pub fn observe(&self, i: usize) -> Result<Rec, u32> {
        match self.recs.get(i) {
            Some(r) => Ok(*r),
            None => Err(RA_ENOBS),
        }
    }

    /// 自洽。
    pub fn audit(&self) -> Result<(), u32> {
        if self.samples.len() != self.recs.len() {
            return Err(RA_EBAL);
        }
        let mut fail = 0u32;
        for r in &self.recs {
            if r.verdict != V_OK {
                fail += 1;
                if r.user.is_some() || r.group.is_some() {
                    return Err(RA_EBAL);
                }
                if r.stage == ST_LEX && (r.user_seen || r.group_seen) {
                    return Err(RA_EBAL);
                }
                if r.stage == ST_DONE {
                    return Err(RA_EBAL);
                }
            } else {
                if r.stage != ST_DONE {
                    return Err(RA_EBAL);
                }
            }
        }
        if fail != self.failures {
            return Err(RA_EBAL);
        }
        Ok(())
    }

    /// 折叠。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = mix(h, self.samples.len() as u64);
        for i in 0..self.samples.len() {
            let s = self.samples[i];
            let r = self.recs[i];
            h = mix(h, s.len as u64);
            let mut j = 0;
            while j < s.len as usize {
                h ^= s.bytes[j] as u64;
                h = h.wrapping_mul(FNV_PRIME);
                j += 1;
            }
            h = mix(h, r.stage as u64);
            h = mix(h, if r.user_seen { 1 } else { 0 });
            h = mix(h, if r.group_seen { 1 } else { 0 });
            h = mix(h, r.verdict as u64);
            h = mix(h, r.user.unwrap_or(0xffff_ffff) as u64);
            h = mix(h, r.group.unwrap_or(0xffff_ffff) as u64);
        }
        h = mix(h, self.failures as u64);
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(RA_EFROZEN);
        }
        self.audit()?;
        self.root = self.fold();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放：逐格重算轨迹与结论。
    pub fn replay(&self) -> Result<u64, u32> {
        let mut fail = 0u32;
        let mut h = FNV_OFF;
        h = mix(h, self.samples.len() as u64);
        for i in 0..self.samples.len() {
            let s = self.samples[i];
            let r = self.recs[i];
            let (stage, us, gs, verdict) = trace(s.text());
            let mut user = None;
            let mut group = None;
            if verdict == V_OK {
                if let Ok(v) = resolve(s.text()) {
                    user = v.user;
                    group = v.group;
                }
            } else {
                fail += 1;
            }
            if stage != r.stage || us != r.user_seen || gs != r.group_seen || verdict != r.verdict {
                return Err(RA_ESEAL);
            }
            if user != r.user || group != r.group {
                return Err(RA_ESEAL);
            }
            h = mix(h, s.len as u64);
            let mut j = 0;
            while j < s.len as usize {
                h ^= s.bytes[j] as u64;
                h = h.wrapping_mul(FNV_PRIME);
                j += 1;
            }
            h = mix(h, r.stage as u64);
            h = mix(h, if r.user_seen { 1 } else { 0 });
            h = mix(h, if r.group_seen { 1 } else { 0 });
            h = mix(h, r.verdict as u64);
            h = mix(h, r.user.unwrap_or(0xffff_ffff) as u64);
            h = mix(h, r.group.unwrap_or(0xffff_ffff) as u64);
        }
        if fail != self.failures {
            return Err(RA_ESEAL);
        }
        h = mix(h, self.failures as u64);
        Ok(h)
    }

    /// 冻结根。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
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

/// 构建十二样本账本。
pub fn build_ledger() -> ResolveLedger {
    let mut l = ResolveLedger::new();
    let _ = l.register(0, "root:root");
    let _ = l.register(1, "daemon:bin");
    let _ = l.register(2, "1000");
    let _ = l.register(3, "033");
    let _ = l.register(4, "user");
    let _ = l.register(5, ":www");
    let _ = l.register(6, "root:");
    let _ = l.register(7, "");
    let _ = l.register(8, ":");
    let _ = l.register(9, "ghost");
    let _ = l.register(10, "root:ghost");
    let _ = l.register(11, "3000000000");
    l
}

/// 四路注入：基线、数字优先、短路、冻结。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.ok_count() == 7 && l.failures() == 5 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    out[0] = base && root != 0 && rep == Ok(root);

    out[1] = resolve_field("1000", &USERS) == Ok(Field { id: 1000, numeric: true })
        && resolve_field("033", &USERS) == Ok(Field { id: 33, numeric: true })
        && resolve_field("0", &GROUPS) == Ok(Field { id: 0, numeric: true });

    let (stage, us, gs, code) = trace("ghost:bin");
    out[2] = stage == ST_USER && us && !gs && code == RA_ENAME;
    let (stage2, us2, gs2, code2) = trace("root:ghost");
    out[2] = out[2] && stage2 == ST_GROUP && us2 && gs2 && code2 == RA_ENAME;

    let mut f = build_ledger();
    let froz = f.freeze().is_ok();
    let deny = f.register(12, "root");
    out[3] = froz && deny == Err(RA_EFROZEN) && f.rejects() == 1 && f.len() == SAMPLES;

    out
}

/// 十三项判据。
pub fn run_unx11423_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11423.chownname");
    let l = build_ledger();

    s.add(
        "RA1-账本十二格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.spec_at(0) == Some("root:root"),
        "static",
    );

    s.add_dyn(
        "RA2-四类分布正确",
        l.ok_count() == 7
            && l.failures_at(ST_LEX) == 2
            && l.failures_at(ST_USER) == 2
            && l.failures_at(ST_GROUP) == 1
            && l.failures() == 5,
        &format!(
            "ok={} lex={} user={} group={}",
            l.ok_count(),
            l.failures_at(ST_LEX),
            l.failures_at(ST_USER),
            l.failures_at(ST_GROUP)
        ),
    );

    let (st, us, gs, code) = trace("ghost:bin");
    let (st2, us2, gs2, code2) = trace("root:ghost");
    s.add_dyn(
        "RA3-用户失败短路不查组",
        st == ST_USER && us && !gs && code == RA_ENAME && st2 == ST_GROUP && us2 && gs2 && code2 == RA_ENAME,
        &format!("a={} b={}", code, code2),
    );

    s.add(
        "RA4-数字优先于字面名",
        lookup(&USERS, "1000") == Some(Entry { id: 77, name: "1000", shadowed: false })
            && resolve_field("1000", &USERS) == Ok(Field { id: 1000, numeric: true }),
        "static",
    );

    s.add(
        "RA5-前导零按十进制",
        resolve_field("033", &USERS) == Ok(Field { id: 33, numeric: true })
            && resolve_field("000006", &GROUPS) == Ok(Field { id: 6, numeric: true }),
        "static",
    );

    s.add(
        "RA6-越界报ENUM",
        resolve_field("3000000000", &USERS) == Err(RA_ENUM)
            && resolve_field("99999999999", &USERS) == Err(RA_ENUM),
        "static",
    );

    s.add(
        "RA7-符号串走名表",
        resolve_field("-1", &USERS) == Err(RA_ENAME)
            && resolve_field("+1", &USERS) == Err(RA_ENAME)
            && resolve_field("1.5", &USERS) == Err(RA_ENAME),
        "static",
    );

    s.add(
        "RA8-名表查解析",
        resolve_field("daemon", &USERS) == Ok(Field { id: 1, numeric: false })
            && resolve_field("staff", &GROUPS) == Ok(Field { id: 100, numeric: false }),
        "static",
    );

    s.add_dyn(
        "RA9-重名取前项并记账",
        resolve_field("dup", &USERS) == Ok(Field { id: 88, numeric: false })
            && shadow_count(&USERS) == 1
            && shadow_count(&GROUPS) == 0,
        &format!("shadow={}", shadow_count(&USERS)),
    );

    s.add(
        "RA10-空字段规则",
        resolve(":") == Err(RA_ELEX)
            && resolve("") == Err(RA_ELEX)
            && resolve("root:") == Ok(Resolution { user: Some(0), group: None, stage: ST_DONE, verdict: V_OK })
            && resolve(":www") == Ok(Resolution { user: None, group: Some(33), stage: ST_DONE, verdict: V_OK }),
        "static",
    );

    s.add_dyn(
        "RA11-账本平衡复算一致",
        l.audit().is_ok() && l.failures() == 5 && l.failures() == l.failures_at(ST_LEX) + l.failures_at(ST_USER) + l.failures_at(ST_GROUP),
        &format!("failures={}", l.failures()),
    );

    let mut f = build_ledger();
    let rep = f.replay();
    let f1 = f.freeze();
    let root = match f1 {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(RA_EFROZEN);
    let locked = f.register(12, "root") == Err(RA_EFROZEN);
    s.add_dyn(
        "RA12-冻结重放幂等",
        root != 0 && rep == Ok(root) && again && locked && f.root() == root,
        &format!("root={:016x}", root),
    );

    let inj = inject_four();
    s.add_dyn("RA13-四路注入全绿", inj == [true; 4], &format!("inject={:?}", inj));

    s
}

/// 摘要。
pub fn summary(l: &ResolveLedger) -> alloc::string::String {
    format!("ok={} fail={} rejects={}", l.ok_count(), l.failures(), l.rejects())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_stages_and_short_circuit() {
        assert_eq!(trace("root:root"), (ST_DONE, true, true, V_OK));
        assert_eq!(trace("root"), (ST_DONE, true, false, V_OK));
        assert_eq!(trace(":bin"), (ST_DONE, false, true, V_OK));
        assert_eq!(trace("ghost:bin"), (ST_USER, true, false, RA_ENAME));
        assert_eq!(trace("root:ghost"), (ST_GROUP, true, true, RA_ENAME));
        assert_eq!(trace(""), (ST_LEX, false, false, RA_ELEX));
        assert_eq!(trace(":"), (ST_LEX, false, false, RA_ELEX));
        assert_eq!(trace("a:b:c"), (ST_LEX, false, false, RA_ELEX));
    }

    #[test]
    fn numeric_beats_literal_name() {
        assert_eq!(lookup(&USERS, "1000").map(|e| e.id), Some(77));
        assert_eq!(resolve_field("1000", &USERS), Ok(Field { id: 1000, numeric: true }));
        assert_eq!(resolve_field("033", &USERS), Ok(Field { id: 33, numeric: true }));
        assert_eq!(resolve_field("3000000000", &USERS), Err(RA_ENUM));
    }

    #[test]
    fn names_and_duplicates() {
        assert_eq!(resolve_field("daemon", &USERS), Ok(Field { id: 1, numeric: false }));
        assert_eq!(resolve_field("dup", &USERS), Ok(Field { id: 88, numeric: false }));
        assert_eq!(shadow_count(&USERS), 1);
        assert_eq!(resolve_field("ghost", &USERS), Err(RA_ENAME));
        assert_eq!(resolve_field("-1", &USERS), Err(RA_ENAME));
    }

    #[test]
    fn ledger_balance() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 7);
        assert_eq!(l.failures(), 5);
        assert_eq!(l.failures_at(ST_LEX), 2);
        assert!(l.audit().is_ok());
        assert_eq!(l.rec(2).map(|r| r.user), Some(Some(1000)));
        assert_eq!(l.rec(7).map(|r| r.verdict), Some(RA_ELEX));
    }

    #[test]
    fn refusal_matrix() {
        let mut l = ResolveLedger::new();
        assert_eq!(l.register(0, "root"), Ok(V_OK));
        assert_eq!(l.register(0, "bin"), Err(RA_EDUP));
        assert_eq!(l.register(4, "bin"), Err(RA_EORDER));
        assert_eq!(l.register(12, "bin"), Err(RA_EID));
        assert_eq!(l.register(1, "aaaaaaaaaaaaaaaaaaaaaaaaaaaa"), Err(RA_ECAP));
        let _ = l.freeze();
        assert_eq!(l.register(1, "bin"), Err(RA_EFROZEN));
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
        assert_eq!(l.freeze(), Err(RA_EFROZEN));
        assert_eq!(l.root(), root);
        assert!(l.replay().is_ok());
    }

    #[test]
    fn checks_green_and_injection() {
        let s = run_unx11423_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
        assert!(summary(&build_ledger()).starts_with("ok=7"));
    }
}