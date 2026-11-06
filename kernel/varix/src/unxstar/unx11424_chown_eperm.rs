//! UNX-F11424 chown 权限边界（0x20a40 段）
//!
//! chown 的授权面比 chmod 更窄：属性主通常只有具备 CAP_CHOWN 的进程才能
//! 任意改写；普通进程只被允许把属主写回自分（等价于无操作），或把属组
//! 改成自己有效组/附加组之一。模型把这一判断拆成两个独立子判定：属主子
//! 判定与属组子判定，二者都不通过时返回 EPERM，只有通过的那一侧落盘。
//!
//! 凭据包含 euid、egid、附加组向量、保存 uid 与能力位。附加组是判定属组
//! 写入的关键：只比较 egid 会漏掉 `newgrp` 之后的常见场景。能力位一旦置
//! 位，两个子判定直接放行，且不再查询组向量——这一点必须在账本里体现出
//! 来，否则“root 改属”与“普通用户改自属组”会在轨迹上不可区分。
//!
//! 拒绝路径不允许产生任何副作用：节点属主属组保持原值，拒绝计数加一，
//! 且同一次调用里若属主通过而属组被拒，整次调用仍按失败处理、不回写属主
//! ——这是本模型刻意选择的“字段原子性”，与基准机上 chown 失败后文件
//! 身份不变的观察一致。账本登记十二个样本并冻结重放。

use crate::checks::CheckSet;
use crate::alloc::format;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20a40;

/// 编号非法。
pub const CP_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const CP_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const CP_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const CP_EFROZEN: u32 = SEG_BASE + 0x04;
/// 权限不足。
pub const CP_EPERM: u32 = SEG_BASE + 0x05;
/// 实参非法。
pub const CP_ESPEC: u32 = SEG_BASE + 0x06;
/// 组向量满。
pub const CP_EFULL: u32 = SEG_BASE + 0x07;
/// 账本失衡。
pub const CP_EBAL: u32 = SEG_BASE + 0x08;
/// 票号不一致。
pub const CP_ESEAL: u32 = SEG_BASE + 0x09;
/// 观测不存在的格。
pub const CP_ENOBS: u32 = SEG_BASE + 0x0a;
/// 能力位非法。
pub const CP_ECAP: u32 = SEG_BASE + 0x0b;
/// 组号重复。
pub const CP_EGROUP: u32 = SEG_BASE + 0x0c;

/// 能力：改写属主。
pub const CAP_CHOWN: u32 = 1 << 0;
/// 能力：改写属组。
pub const CAP_CHGRP: u32 = 1 << 1;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 属主通过位。
pub const B_USER: u8 = 0x01;
/// 属组通过位。
pub const B_GROUP: u8 = 0x02;
/// 能力直通位。
pub const B_CAP: u8 = 0x04;

/// 附加组容量。
pub const GROUP_CAP: usize = 4;
/// 样本格数。
pub const SAMPLES: usize = 12;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 进程凭据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cred {
    pub euid: u32,
    pub egid: u32,
    pub suid: u32,
    pub caps: u32,
    pub groups: [u32; GROUP_CAP],
    pub ngroups: u8,
}

impl Cred {
    /// root 全权。
    pub const fn root() -> Cred {
        Cred { euid: 0, egid: 0, suid: 0, caps: CAP_CHOWN | CAP_CHGRP, groups: [0, 0, 0, 0], ngroups: 0 }
    }

    /// 普通用户。
    pub const fn user(euid: u32, egid: u32) -> Cred {
        Cred { euid, egid, suid: euid, caps: 0, groups: [0, 0, 0, 0], ngroups: 0 }
    }

    /// 是否在附加组里。
    pub fn in_group(&self, gid: u32) -> bool {
        if gid == self.egid {
            return true;
        }
        let mut i = 0;
        while i < self.ngroups as usize && i < GROUP_CAP {
            if self.groups[i] == gid {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 追加附加组。
    pub fn add_group(&mut self, gid: u32) -> Result<(), u32> {
        if self.in_group(gid) {
            return Err(CP_EGROUP);
        }
        if self.ngroups as usize >= GROUP_CAP {
            return Err(CP_EFULL);
        }
        self.groups[self.ngroups as usize] = gid;
        self.ngroups += 1;
        Ok(())
    }
}

/// 待改的节点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    pub uid: u32,
    pub gid: u32,
}

impl Node {
    /// 构造。
    pub const fn make(uid: u32, gid: u32) -> Node {
        Node { uid, gid }
    }
}

/// 一次请求。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub uid: Option<u32>,
    pub gid: Option<u32>,
}

/// 授权结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decision {
    pub bits: u8,
    pub verdict: u32,
    pub after: Node,
}

/// 解析 `u:g` 数字实参（本卡只关心权限面，因此只收数字）。
pub fn parse_request(spec: &str) -> Result<Request, u32> {
    if spec.is_empty() {
        return Err(CP_ESPEC);
    }
    let b = spec.as_bytes();
    let mut at = None;
    let mut i = 0;
    let mut extra = false;
    while i < b.len() {
        if b[i] == b':' {
            if at.is_some() {
                extra = true;
            }
            at = Some(i);
        }
        i += 1;
    }
    if extra {
        return Err(CP_ESPEC);
    }
    match at {
        None => Ok(Request { uid: Some(num(spec)?), gid: None }),
        Some(p) => {
            let u = match spec.get(..p) {
                Some(v) => v,
                None => return Err(CP_ESPEC),
            };
            let g = match spec.get(p + 1..) {
                Some(v) => v,
                None => return Err(CP_ESPEC),
            };
            if u.is_empty() && g.is_empty() {
                return Err(CP_ESPEC);
            }
            let uid = if u.is_empty() { None } else { Some(num(u)?) };
            let gid = if g.is_empty() { None } else { Some(num(g)?) };
            Ok(Request { uid, gid })
        }
    }
}

fn num(s: &str) -> Result<u32, u32> {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 10 {
        return Err(CP_ESPEC);
    }
    let mut v: u64 = 0;
    let mut i = 0;
    while i < b.len() {
        if b[i] < b'0' || b[i] > b'9' {
            return Err(CP_ESPEC);
        }
        v = v * 10 + (b[i] - b'0') as u64;
        if v > 0x7fff_ffff {
            return Err(CP_ESPEC);
        }
        i += 1;
    }
    Ok(v as u32)
}

/// 判定一次改属。
pub fn decide(cred: &Cred, req: &Request, node: Node) -> Decision {
    let mut bits = 0u8;
    if cred.caps & (CAP_CHOWN | CAP_CHGRP) != 0 {
        bits |= B_CAP;
    }
    let user_ok = match req.uid {
        None => true,
        Some(u) => {
            if cred.caps & CAP_CHOWN != 0 {
                bits |= B_USER;
                true
            } else if u == cred.euid {
                bits |= B_USER;
                true
            } else {
                false
            }
        }
    };
    let group_ok = match req.gid {
        None => true,
        Some(g) => {
            if cred.caps & CAP_CHGRP != 0 {
                bits |= B_GROUP;
                true
            } else if cred.in_group(g) {
                bits |= B_GROUP;
                true
            } else {
                false
            }
        }
    };
    if !user_ok || !group_ok {
        return Decision { bits, verdict: CP_EPERM, after: node };
    }
    Decision {
        bits,
        verdict: V_OK,
        after: Node {
            uid: match req.uid {
                Some(u) => u,
                None => node.uid,
            },
            gid: match req.gid {
                Some(g) => g,
                None => node.gid,
            },
        },
    }
}

/// 一格样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub bytes: [u8; 16],
    pub len: u8,
    pub cred: Cred,
    pub node: Node,
}

impl Sample {
    /// 文本。
    pub fn text(&self) -> &str {
        let n = self.len as usize;
        if n > 16 {
            return "";
        }
        match core::str::from_utf8(self.bytes.get(..n).unwrap_or(&[])) {
            Ok(v) => v,
            Err(_) => "",
        }
    }
}

/// 授权账本。
pub struct PolicyLedger {
    samples: Vec<Sample>,
    decisions: Vec<Decision>,
    frozen: bool,
    root: u64,
    rejects: u32,
    denies: u32,
}

impl PolicyLedger {
    /// 空账本。
    pub fn new() -> PolicyLedger {
        PolicyLedger {
            samples: Vec::new(),
            decisions: Vec::new(),
            frozen: false,
            root: 0,
            rejects: 0,
            denies: 0,
        }
    }

    /// 登记一格。
    pub fn register(&mut self, slot: u8, spec: &str, cred: Cred, node: Node) -> Result<u32, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(CP_EFROZEN);
        }
        if slot as usize >= SAMPLES {
            self.rejects += 1;
            return Err(CP_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(CP_EDUP);
        }
        if slot as usize != self.samples.len() {
            self.rejects += 1;
            return Err(CP_EORDER);
        }
        if spec.len() > 16 {
            self.rejects += 1;
            return Err(CP_ESPEC);
        }
        if cred.ngroups as usize > GROUP_CAP {
            self.rejects += 1;
            return Err(CP_ECAP);
        }
        let req = match parse_request(spec) {
            Ok(v) => v,
            Err(code) => {
                self.rejects += 1;
                return Err(code);
            }
        };
        let mut bytes = [0u8; 16];
        let b = spec.as_bytes();
        let mut i = 0;
        while i < b.len() {
            bytes[i] = b[i];
            i += 1;
        }
        let d = decide(&cred, &req, node);
        if d.verdict != V_OK {
            self.denies += 1;
        }
        self.samples.push(Sample { bytes, len: b.len() as u8, cred, node });
        self.decisions.push(d);
        Ok(d.verdict)
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

    /// 结论。
    pub fn decision(&self, i: usize) -> Option<Decision> {
        self.decisions.get(i).copied()
    }

    /// 成功数。
    pub fn ok_count(&self) -> u32 {
        let mut n = 0u32;
        for d in &self.decisions {
            if d.verdict == V_OK {
                n += 1;
            }
        }
        n
    }

    /// 拒绝数。
    pub fn denies(&self) -> u32 {
        self.denies
    }

    /// 结构拒绝数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 带能力直通位的格数。
    pub fn cap_count(&self) -> u32 {
        let mut n = 0u32;
        for d in &self.decisions {
            if d.bits & B_CAP != 0 {
                n += 1;
            }
        }
        n
    }

    /// 观测一格。
    pub fn observe(&self, i: usize) -> Result<Decision, u32> {
        match self.decisions.get(i) {
            Some(d) => Ok(*d),
            None => Err(CP_ENOBS),
        }
    }

    /// 自洽。
    pub fn audit(&self) -> Result<(), u32> {
        if self.samples.len() != self.decisions.len() {
            return Err(CP_EBAL);
        }
        let mut deny = 0u32;
        for i in 0..self.decisions.len() {
            let d = self.decisions[i];
            if d.verdict != V_OK {
                deny += 1;
                if d.after != self.samples[i].node {
                    return Err(CP_EBAL);
                }
            }
        }
        if deny != self.denies {
            return Err(CP_EBAL);
        }
        Ok(())
    }

    /// 折叠。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = mix(h, self.samples.len() as u64);
        for i in 0..self.samples.len() {
            let s = self.samples[i];
            let d = self.decisions[i];
            h = mix(h, s.len as u64);
            let mut j = 0;
            while j < s.len as usize {
                h ^= s.bytes[j] as u64;
                h = h.wrapping_mul(FNV_PRIME);
                j += 1;
            }
            h = mix(h, s.cred.euid as u64);
            h = mix(h, s.cred.egid as u64);
            h = mix(h, s.cred.caps as u64);
            h = mix(h, s.cred.ngroups as u64);
            let mut k = 0;
            while k < s.cred.ngroups as usize && k < GROUP_CAP {
                h = mix(h, s.cred.groups[k] as u64);
                k += 1;
            }
            h = mix(h, s.node.uid as u64);
            h = mix(h, s.node.gid as u64);
            h = mix(h, d.bits as u64);
            h = mix(h, d.verdict as u64);
            h = mix(h, d.after.uid as u64);
            h = mix(h, d.after.gid as u64);
        }
        h = mix(h, self.denies as u64);
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(CP_EFROZEN);
        }
        self.audit()?;
        self.root = self.fold();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let mut deny = 0u32;
        let mut h = FNV_OFF;
        h = mix(h, self.samples.len() as u64);
        for i in 0..self.samples.len() {
            let s = self.samples[i];
            let d = self.decisions[i];
            let req = match parse_request(s.text()) {
                Ok(v) => v,
                Err(_) => return Err(CP_ESEAL),
            };
            let e = decide(&s.cred, &req, s.node);
            if e != d {
                return Err(CP_ESEAL);
            }
            if e.verdict != V_OK {
                deny += 1;
            }
            h = mix(h, s.len as u64);
            let mut j = 0;
            while j < s.len as usize {
                h ^= s.bytes[j] as u64;
                h = h.wrapping_mul(FNV_PRIME);
                j += 1;
            }
            h = mix(h, s.cred.euid as u64);
            h = mix(h, s.cred.egid as u64);
            h = mix(h, s.cred.caps as u64);
            h = mix(h, s.cred.ngroups as u64);
            let mut k = 0;
            while k < s.cred.ngroups as usize && k < GROUP_CAP {
                h = mix(h, s.cred.groups[k] as u64);
                k += 1;
            }
            h = mix(h, s.node.uid as u64);
            h = mix(h, s.node.gid as u64);
            h = mix(h, e.bits as u64);
            h = mix(h, e.verdict as u64);
            h = mix(h, e.after.uid as u64);
            h = mix(h, e.after.gid as u64);
        }
        if deny != self.denies {
            return Err(CP_ESEAL);
        }
        h = mix(h, self.denies as u64);
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

/// 构造带附加组的普通凭据。
pub fn user_with_groups(uid: u32, egid: u32, extra: &[u32]) -> Cred {
    let mut c = Cred::user(uid, egid);
    let mut i = 0;
    while i < extra.len() {
        let _ = c.add_group(extra[i]);
        i += 1;
    }
    c
}

/// 构建十二样本账本：8 通过、4 拒绝。
pub fn build_ledger() -> PolicyLedger {
    let mut l = PolicyLedger::new();
    let root = Cred::root();
    let u = user_with_groups(1000, 1000, &[44, 100]);
    let n = Node::make(500, 500);
    let _ = l.register(0, "0:0", root, n);
    let _ = l.register(1, "1000:1000", u, n);
    let _ = l.register(2, ":1000", u, n);
    let _ = l.register(3, ":44", u, n);
    let _ = l.register(4, ":100", u, n);
    let _ = l.register(5, "1000", u, n);
    let _ = l.register(6, "1000:44", u, n);
    let _ = l.register(7, ":0", u, n);
    let _ = l.register(8, "0", u, n);
    let _ = l.register(9, "0:1000", u, n);
    let _ = l.register(10, "1000:100", u, n);
    let _ = l.register(11, "0:44", u, n);
    l
}

/// 四路注入：能力直通、附加组、原子拒绝、冻结。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.ok_count() == 8 && l.denies() == 4 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    out[0] = base && root != 0 && rep == Ok(root) && l.cap_count() == 1;

    let u = user_with_groups(1000, 1000, &[44, 100]);
    let n = Node::make(500, 500);
    let d = decide(&u, &Request { uid: None, gid: Some(100) }, n);
    out[1] = d.verdict == V_OK && d.bits & B_GROUP != 0 && d.bits & B_CAP == 0 && d.after.gid == 100;

    let d2 = decide(&u, &Request { uid: Some(77), gid: Some(44) }, n);
    out[2] = d2.verdict == CP_EPERM && d2.after == n && d2.bits & B_GROUP != 0 && d2.bits & B_USER == 0;

    let mut f = build_ledger();
    let froz = f.freeze().is_ok();
    let deny = f.register(12, "0:0", Cred::root(), n);
    out[3] = froz && deny == Err(CP_EFROZEN) && f.rejects() == 1 && f.len() == SAMPLES;

    out
}

/// 十三项判据。
pub fn run_unx11424_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11424.chownperm");
    let l = build_ledger();

    s.add(
        "CP1-账本十二格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.spec_at(0) == Some("0:0"),
        "static",
    );

    s.add_dyn(
        "CP2-八应四拒分布正确",
        l.ok_count() == 8 && l.denies() == 4 && l.audit().is_ok(),
        &format!("ok={} deny={}", l.ok_count(), l.denies()),
    );

    let root = Cred::root();
    let u = user_with_groups(1000, 1000, &[44, 100]);
    let n = Node::make(500, 500);
    s.add(
        "CP3-能力直通双字段",
        decide(&root, &Request { uid: Some(7), gid: Some(7) }, n).verdict == V_OK
            && decide(&root, &Request { uid: Some(7), gid: Some(7) }, n).bits & B_CAP != 0
            && decide(&root, &Request { uid: Some(7), gid: Some(7) }, n).after == Node::make(7, 7),
        "static",
    );

    s.add(
        "CP4-普通用户仅自属主",
        decide(&u, &Request { uid: Some(1000), gid: None }, n).verdict == V_OK
            && decide(&u, &Request { uid: Some(77), gid: None }, n).verdict == CP_EPERM,
        "static",
    );

    s.add(
        "CP5-附加组授权",
        decide(&u, &Request { uid: None, gid: Some(44) }, n).verdict == V_OK
            && decide(&u, &Request { uid: None, gid: Some(100) }, n).verdict == V_OK
            && decide(&u, &Request { uid: None, gid: Some(555) }, n).verdict == CP_EPERM,
        "static",
    );

    s.add(
        "CP6-组向量去重与容量",
        {
            let mut c = Cred::user(1000, 1000);
            c.add_group(44) == Ok(())
                && c.add_group(44) == Err(CP_EGROUP)
                && c.add_group(1000) == Err(CP_EGROUP)
                && c.ngroups == 1
        },
        "static",
    );

    s.add(
        "CP7-组向量满拒绝",
        {
            let mut c = Cred::user(1000, 1000);
            let mut ok = true;
            let mut i = 1;
            while i <= GROUP_CAP {
                if c.add_group(i as u32) != Ok(()) {
                    ok = false;
                }
                i += 1;
            }
            ok && c.add_group(99) == Err(CP_EFULL) && c.ngroups as usize == GROUP_CAP
        },
        "static",
    );

    s.add(
        "CP8-单字段通过另侧保持",
        l.decision(2) == Some(Decision { bits: B_GROUP, verdict: V_OK, after: Node::make(500, 1000) })
            && l.decision(1) == Some(Decision { bits: B_USER | B_GROUP, verdict: V_OK, after: Node::make(1000, 1000) }),
        "static",
    );

    s.add(
        "CP9-拒绝零副作用",
        l.decision(7).map(|d| d.after) == Some(n)
            && l.decision(8).map(|d| d.after) == Some(n)
            && l.decision(11).map(|d| d.verdict) == Some(CP_EPERM)
            && l.decision(11).map(|d| d.bits) == Some(B_GROUP),
        "static",
    );

    s.add(
        "CP10-实参解析与拒绝",
        parse_request("0:0") == Ok(Request { uid: Some(0), gid: Some(0) })
            && parse_request(":44") == Ok(Request { uid: None, gid: Some(44) })
            && parse_request("1000:") == Ok(Request { uid: Some(1000), gid: None })
            && parse_request(":") == Err(CP_ESPEC)
            && parse_request("") == Err(CP_ESPEC)
            && parse_request("a:1") == Err(CP_ESPEC),
        "static",
    );

    let mut r = PolicyLedger::new();
    let a = r.register(0, "0:0", Cred::root(), n) == Ok(V_OK);
    let dup = r.register(0, "0:0", Cred::root(), n) == Err(CP_EDUP);
    let ord = r.register(3, "0:0", Cred::root(), n) == Err(CP_EORDER);
    let id = r.register(12, "0:0", Cred::root(), n) == Err(CP_EID);
    let bad = r.register(1, "a:1", Cred::root(), n) == Err(CP_ESPEC);
    let _ = r.freeze();
    let fro = r.register(1, "0:0", Cred::root(), n) == Err(CP_EFROZEN);
    s.add_dyn(
        "CP11-结构拒绝矩阵五门",
        a && dup && ord && id && bad && fro && r.rejects() == 5 && r.len() == 1,
        &format!("rejects={}", r.rejects()),
    );

    let mut f = build_ledger();
    let rep = f.replay();
    let f1 = f.freeze();
    let rootv = match f1 {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(CP_EFROZEN);
    let locked = f.register(12, "0:0", Cred::root(), n) == Err(CP_EFROZEN);
    s.add_dyn(
        "CP12-冻结重放幂等",
        rootv != 0 && rep == Ok(rootv) && again && locked && f.root() == rootv,
        &format!("root={:016x}", rootv),
    );

    let inj = inject_four();
    s.add_dyn("CP13-四路注入全绿", inj == [true; 4], &format!("inject={:?}", inj));

    s
}

/// 摘要。
pub fn summary(l: &PolicyLedger) -> alloc::string::String {
    format!("ok={} deny={} cap={} rejects={}", l.ok_count(), l.denies(), l.cap_count(), l.rejects())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_shortcut_and_groups() {
        let root = Cred::root();
        let u = user_with_groups(1000, 1000, &[44, 100]);
        let n = Node::make(500, 500);
        assert_eq!(decide(&root, &Request { uid: Some(9), gid: Some(9) }, n).after, Node::make(9, 9));
        assert_eq!(decide(&u, &Request { uid: None, gid: Some(44) }, n).verdict, V_OK);
        assert_eq!(decide(&u, &Request { uid: Some(9), gid: None }, n).verdict, CP_EPERM);
        assert_eq!(decide(&u, &Request { uid: None, gid: Some(7) }, n).verdict, CP_EPERM);
    }

    #[test]
    fn rejection_keeps_node() {
        let u = user_with_groups(1000, 1000, &[44]);
        let n = Node::make(500, 500);
        let d = decide(&u, &Request { uid: Some(77), gid: Some(44) }, n);
        assert_eq!(d.verdict, CP_EPERM);
        assert_eq!(d.after, n);
        assert_eq!(d.bits & B_GROUP, B_GROUP);
    }

    #[test]
    fn group_vector_limits() {
        let mut c = Cred::user(1000, 1000);
        assert_eq!(c.add_group(44), Ok(()));
        assert_eq!(c.add_group(44), Err(CP_EGROUP));
        let mut d = Cred::user(1000, 1000);
        let mut i = 1;
        while i <= GROUP_CAP {
            assert_eq!(d.add_group(i as u32), Ok(()));
            i += 1;
        }
        assert_eq!(d.add_group(77), Err(CP_EFULL));
    }

    #[test]
    fn ledger_distribution() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.denies(), 4);
        assert_eq!(l.cap_count(), 1);
        assert!(l.audit().is_ok());
        assert_eq!(l.decision(7).map(|d| d.after), Some(Node::make(500, 500)));
    }

    #[test]
    fn refusal_matrix() {
        let n = Node::make(1, 1);
        let mut r = PolicyLedger::new();
        assert_eq!(r.register(0, "0:0", Cred::root(), n), Ok(V_OK));
        assert_eq!(r.register(0, "0:0", Cred::root(), n), Err(CP_EDUP));
        assert_eq!(r.register(4, "0:0", Cred::root(), n), Err(CP_EORDER));
        assert_eq!(r.register(12, "0:0", Cred::root(), n), Err(CP_EID));
        assert_eq!(r.register(1, "a", Cred::root(), n), Err(CP_ESPEC));
        let _ = r.freeze();
        assert_eq!(r.register(1, "0:0", Cred::root(), n), Err(CP_EFROZEN));
        assert_eq!(r.rejects(), 5);
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
        assert_eq!(l.freeze(), Err(CP_EFROZEN));
        assert_eq!(l.root(), root);
    }

    #[test]
    fn checks_green_and_injection() {
        let s = run_unx11424_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
        assert!(summary(&build_ledger()).starts_with("ok=8"));
    }
}