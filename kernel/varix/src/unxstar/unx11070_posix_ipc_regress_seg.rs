//! UNX-F11070 POSIX IPC 回归段（0x1e8c0 段）
//!
//! B15/B28 判据在此重放：命名对象（命名信号量/共享内存/消息队列）
//! 以 `/name` 单斜杠命名空间登记，创建时校验名字形状，open 计数与
//! unlink 语义分离——unlink 后名字立即消失，但对象活到最后一个引用
//! 释放（refs == 0）才可回收。
//!
//! 不变式：opens == Σrefs；mq 的 used 恒等于在队字节；名字全局唯一且
//! 形如 `/x`（无内嵌斜杠）；冻结后 create/acquire/send 全以 PX_FROZEN 拒。
//!
//! 结构：NameProbe（名字探针）/ PosixObj（命名对象）/ PosixLedger
//! （两批总账 + 重放 + 冻结）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11070 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e8c0;

/// 空账：空名字、零字节发送或查无此名
pub const PX_EMPTY: u32 = SEG_BASE + 0x01;
/// 越界：名字超长、kind 非法或容量为 0
pub const PX_RANGE: u32 = SEG_BASE + 0x02;
/// 重号：同名对象二次创建或重复 unlink
pub const PX_DUP: u32 = SEG_BASE + 0x03;
/// 失配：名字形状非法、查已 unlink 名、或引用下溢
pub const PX_MISMATCH: u32 = SEG_BASE + 0x04;
/// 短缺：引用表满、mq 水位不足
pub const PX_SHORT: u32 = SEG_BASE + 0x05;
/// 已封：段账冻结后的创建/打开/发送
pub const PX_FROZEN: u32 = SEG_BASE + 0x06;

/// FNV 折前缀：低两字节 0x70 标记 11070 号卡
pub const PX_TAG: u64 = 0x1e80_70;

/// 本段固定十席（4 sem + 3 shm + 3 mq）
const SEATS: usize = 10;
/// POSIX 名字上限（NAME_MAX）
const NAME_MAX: usize = 255;
/// 单对象引用上限
const REF_CAP: u16 = 1024;

fn fnv_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(0x0000_0100_0000_01b3)
}

fn fnv_u16(h: u64, v: u16) -> u64 {
    fnv_byte(fnv_byte(h, (v & 0xff) as u8), ((v >> 8) & 0xff) as u8)
}

fn fnv_u32(mut h: u64, v: u32) -> u64 {
    let mut k = 0u32;
    while k < 4 {
        h = fnv_byte(h, ((v >> k.wrapping_mul(8)) & 0xff) as u8);
        k = k.wrapping_add(1);
    }
    h
}

fn fnv_str(mut h: u64, s: &str) -> u64 {
    let bytes = s.as_bytes();
    let mut k = 0usize;
    while k < bytes.len() {
        h = fnv_byte(h, bytes[k]);
        k += 1;
    }
    h
}

/// 命名空间探针：名字形状 + 失败因
#[derive(Clone, Debug)]
pub struct NameProbe {
    /// 原始名字
    pub name: String,
    /// 失败因（0 = 合法）
    pub reason: u32,
    /// 是否合法
    pub valid: bool,
}

impl NameProbe {
    /// 只做形状探测，不返回错误
    pub fn check(name: &str) -> NameProbe {
        let mut reason = 0u32;
        if name.is_empty() {
            reason = PX_EMPTY;
        } else if name.len() > NAME_MAX {
            reason = PX_RANGE;
        } else if !name.starts_with('/') || name.len() < 2 {
            reason = PX_MISMATCH;
        } else if name[1..].contains('/') {
            reason = PX_MISMATCH;
        }
        NameProbe { name: name.to_string(), reason, valid: reason == 0 }
    }

    /// 探测并转成 Result
    pub fn validate(name: &str) -> Result<NameProbe, u32> {
        let probe = NameProbe::check(name);
        if probe.valid {
            Ok(probe)
        } else {
            Err(probe.reason)
        }
    }

    /// 名字指纹
    pub fn digest(&self, mut h: u64) -> u64 {
        h = fnv_str(h, &self.name);
        fnv_u32(h, self.reason)
    }
}

/// 命名 IPC 对象
#[derive(Clone, Debug)]
pub struct PosixObj {
    /// 对象 id（1..=10）
    pub id: u16,
    /// 0 = 命名信号量，1 = 共享内存，2 = 消息队列
    pub kind: u8,
    /// 全局唯一名
    pub name: String,
    /// 当前引用数
    pub refs: u16,
    /// mq 已用字节
    pub used: u32,
    /// mq 容量
    pub cap: u32,
    /// 名字已 unlink
    pub unlinked: bool,
}

impl PosixObj {
    /// 新建未打开对象
    pub fn new(id: u16, kind: u8, name: String, cap: u32) -> PosixObj {
        PosixObj { id, kind, name, refs: 0, used: 0, cap, unlinked: false }
    }

    /// 打开引用：已 unlink 拒，引用表满拒
    pub fn acquire(&mut self) -> Result<u16, u32> {
        if self.unlinked {
            return Err(PX_MISMATCH);
        }
        if self.kind > 2 {
            return Err(PX_RANGE);
        }
        if self.refs >= REF_CAP {
            return Err(PX_SHORT);
        }
        self.refs = self.refs.wrapping_add(1);
        Ok(self.refs)
    }

    /// 释放引用：零引用不可再放
    pub fn release(&mut self) -> Result<u16, u32> {
        if self.refs == 0 {
            return Err(PX_EMPTY);
        }
        self.refs = self.refs.wrapping_sub(1);
        Ok(self.refs)
    }

    /// mq 发送：非 mq / 已 unlink / 零长 / 超水位四路拒
    pub fn mq_send(&mut self, bytes: u32) -> Result<u32, u32> {
        if self.kind != 2 {
            return Err(PX_RANGE);
        }
        if self.unlinked {
            return Err(PX_MISMATCH);
        }
        if bytes == 0 {
            return Err(PX_EMPTY);
        }
        let next = self.used.wrapping_add(bytes);
        if next > self.cap {
            return Err(PX_SHORT);
        }
        self.used = next;
        Ok(next)
    }

    /// mq 接收：不得超已用
    pub fn mq_recv(&mut self, bytes: u32) -> Result<u32, u32> {
        if self.kind != 2 {
            return Err(PX_RANGE);
        }
        if bytes == 0 {
            return Err(PX_EMPTY);
        }
        if bytes > self.used {
            return Err(PX_SHORT);
        }
        self.used = self.used.wrapping_sub(bytes);
        Ok(self.used)
    }

    /// 可回收：已 unlink 且零引用
    pub fn destroyable(&self) -> bool {
        self.unlinked && self.refs == 0
    }

    /// 对象指纹
    pub fn digest(&self, mut h: u64) -> u64 {
        h = fnv_u16(h, self.id);
        h = fnv_byte(h, self.kind);
        h = fnv_str(h, &self.name);
        h = fnv_u16(h, self.refs);
        h = fnv_u32(h, self.used);
        h = fnv_u32(h, self.cap);
        fnv_byte(h, if self.unlinked { 1 } else { 0 })
    }
}

/// B15/B28 命名对象总账
#[derive(Clone, Debug)]
pub struct PosixLedger {
    /// 对象表
    pub objs: Vec<PosixObj>,
    /// 累计 open
    pub opens: u32,
    /// 累计 unlink
    pub unlinks: u32,
    /// 下一个对象 id
    pub next_id: u16,
    /// 最近一次重放合法对象数
    pub verified: u16,
    frozen: Option<u64>,
}

impl PosixLedger {
    /// 空总账
    pub fn new() -> PosixLedger {
        PosixLedger { objs: Vec::new(), opens: 0, unlinks: 0, next_id: 1, verified: 0, frozen: None }
    }

    fn guard(&self) -> Result<(), u32> {
        if self.frozen.is_some() {
            return Err(PX_FROZEN);
        }
        Ok(())
    }

    fn find(&self, name: &str) -> Option<usize> {
        let mut k = 0usize;
        while k < self.objs.len() {
            if self.objs[k].name == name {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    fn by_id(&self, id: u16) -> Option<usize> {
        let mut k = 0usize;
        while k < self.objs.len() {
            if self.objs[k].id == id {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 创建命名对象：kind/容量/名字/重名四道闸
    pub fn create(&mut self, kind: u8, name: &str, cap: u32) -> Result<u16, u32> {
        self.guard()?;
        if kind > 2 {
            return Err(PX_RANGE);
        }
        if cap == 0 {
            return Err(PX_RANGE);
        }
        let probe = NameProbe::validate(name)?;
        if self.find(&probe.name).is_some() {
            return Err(PX_DUP);
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.objs.push(PosixObj::new(id, kind, probe.name, cap));
        Ok(id)
    }

    /// 按名查找：已 unlink 名按 ENOENT 处理
    pub fn lookup(&self, name: &str) -> Result<u16, u32> {
        let idx = match self.find(name) {
            Some(i) => i,
            None => return Err(PX_EMPTY),
        };
        if self.objs[idx].unlinked {
            return Err(PX_MISMATCH);
        }
        Ok(self.objs[idx].id)
    }

    /// 打开引用
    pub fn acquire(&mut self, id: u16) -> Result<u16, u32> {
        self.guard()?;
        let idx = match self.by_id(id) {
            Some(i) => i,
            None => return Err(PX_EMPTY),
        };
        let refs = self.objs[idx].acquire()?;
        self.opens = self.opens.wrapping_add(1);
        Ok(refs)
    }

    /// 释放引用
    pub fn release(&mut self, id: u16) -> Result<u16, u32> {
        self.guard()?;
        let idx = match self.by_id(id) {
            Some(i) => i,
            None => return Err(PX_EMPTY),
        };
        let refs = self.objs[idx].release()?;
        self.opens = self.opens.wrapping_sub(1);
        Ok(refs)
    }

    /// unlink 名字：返回对象是否当场可回收
    pub fn unlink(&mut self, name: &str) -> Result<bool, u32> {
        self.guard()?;
        let idx = match self.find(name) {
            Some(i) => i,
            None => return Err(PX_EMPTY),
        };
        if self.objs[idx].unlinked {
            return Err(PX_DUP);
        }
        self.objs[idx].unlinked = true;
        self.unlinks = self.unlinks.wrapping_add(1);
        Ok(self.objs[idx].destroyable())
    }

    /// mq 发送
    pub fn mq_send(&mut self, id: u16, bytes: u32) -> Result<u32, u32> {
        self.guard()?;
        let idx = match self.by_id(id) {
            Some(i) => i,
            None => return Err(PX_EMPTY),
        };
        self.objs[idx].mq_send(bytes)
    }

    /// mq 接收
    pub fn mq_recv(&mut self, id: u16, bytes: u32) -> Result<u32, u32> {
        self.guard()?;
        let idx = match self.by_id(id) {
            Some(i) => i,
            None => return Err(PX_EMPTY),
        };
        self.objs[idx].mq_recv(bytes)
    }

    /// 引用总数
    pub fn refs_total(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.objs.len() {
            n = n.wrapping_add(self.objs[k].refs as u32);
            k += 1;
        }
        n
    }

    /// 在队字节总数
    pub fn used_total(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.objs.len() {
            n = n.wrapping_add(self.objs[k].used);
            k += 1;
        }
        n
    }

    /// 命名空间一致：名字合法、唯一、opens == Σrefs
    pub fn namespace_consistent(&self) -> bool {
        if self.opens != self.refs_total() {
            return false;
        }
        let mut k = 0usize;
        while k < self.objs.len() {
            let obj = &self.objs[k];
            if !NameProbe::check(&obj.name).valid {
                return false;
            }
            if obj.kind > 2 || obj.used > obj.cap {
                return false;
            }
            let mut j = k.wrapping_add(1);
            while j < self.objs.len() {
                if self.objs[j].name == obj.name {
                    return false;
                }
                j += 1;
            }
            k += 1;
        }
        true
    }

    /// 重放：不足十席 PX_SHORT，命名/引用不合 PX_MISMATCH
    pub fn verify(&mut self) -> Result<u16, u32> {
        self.guard()?;
        if self.objs.len() < SEATS {
            return Err(PX_SHORT);
        }
        let mut ok = 0u16;
        let mut k = 0usize;
        while k < self.objs.len() {
            let obj = &self.objs[k];
            if NameProbe::check(&obj.name).valid && obj.kind <= 2 && obj.used <= obj.cap {
                ok = ok.wrapping_add(1);
            }
            k += 1;
        }
        self.verified = ok;
        if ok as usize != self.objs.len() || !self.namespace_consistent() {
            return Err(PX_MISMATCH);
        }
        Ok(ok)
    }

    /// 总账指纹
    pub fn fold(&self) -> u64 {
        let mut h = PX_TAG;
        h = fnv_u32(h, self.opens);
        h = fnv_u32(h, self.unlinks);
        h = fnv_u16(h, self.next_id);
        h = fnv_u16(h, self.verified);
        let mut k = 0usize;
        while k < self.objs.len() {
            h = self.objs[k].digest(h);
            k += 1;
        }
        h
    }

    /// 冻结总账
    pub fn freeze(&mut self) -> u64 {
        let f = self.fold();
        self.frozen = Some(f);
        f
    }
}

/// 四条负路径：空名 / 超长名 / 重名 / 查已 unlink 名
pub fn inject_four() -> [bool; 4] {
    let mut l0 = PosixLedger::new();
    let empty = matches!(l0.create(0, "", 64), Err(PX_EMPTY));
    let mut l1 = PosixLedger::new();
    let long = {
        let mut name = "/".to_string();
        let mut k = 0usize;
        while k < NAME_MAX + 1 {
            name.push('a');
            k += 1;
        }
        name
    };
    let range = matches!(l1.create(0, &long, 64), Err(PX_RANGE));
    let mut l2 = PosixLedger::new();
    let first = l2.create(0, "/sem", 64).is_ok();
    let dup = first && matches!(l2.create(1, "/sem", 4096), Err(PX_DUP));
    let mut l3 = PosixLedger::new();
    let seeded = match l3.create(2, "/mq", 128) {
        Ok(_) => l3.unlink("/mq").is_ok(),
        Err(_) => false,
    };
    let mismatch = seeded && matches!(l3.lookup("/mq"), Err(PX_MISMATCH));
    [empty, range, dup, mismatch]
}

/// 构造命名对象样本（4 sem + 3 shm + 3 mq）
fn sample_ledger() -> Result<PosixLedger, u32> {
    let mut led = PosixLedger::new();
    let s0 = led.create(0, "/sem0", 64)?;
    led.create(0, "/sem1", 64)?;
    led.create(0, "/sem2", 64)?;
    led.create(0, "/sem3", 64)?;
    let h0 = led.create(1, "/shm0", 4096)?;
    led.create(1, "/shm1", 8192)?;
    led.create(1, "/shm2", 4096)?;
    let q0 = led.create(2, "/mq0", 1024)?;
    let q1 = led.create(2, "/mq1", 2048)?;
    led.create(2, "/mq2", 512)?;
    led.acquire(s0)?;
    led.acquire(s0)?;
    led.acquire(h0)?;
    led.acquire(q0)?;
    led.mq_send(q0, 100)?;
    led.mq_send(q1, 200)?;
    led.mq_recv(q0, 40)?;
    let dropped = led.unlink("/mq2")?;
    if !dropped {
        return Err(PX_MISMATCH);
    }
    Ok(led)
}

/// UNX-F11070 判据入口
pub fn run_unx11070_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11070");
    let mut led = match sample_ledger() {
        Ok(v) => v,
        Err(_) => PosixLedger::new(),
    };
    let verified = led.verify().unwrap_or(0);
    s.add(
        "J1a-POSIX判据全量重放",
        verified == 10 && led.objs.len() == SEATS,
        "B15/B28 十对象必须全部合法",
    );
    let probe_ok = NameProbe::check("/ok").valid
        && !NameProbe::check("bad").valid
        && !NameProbe::check("/a/b").valid;
    s.add(
        "J1b-命名语义一致",
        led.namespace_consistent() && probe_ok,
        "名字单斜杠、opens==Σrefs、非法名三探针全中",
    );
    s.add(
        "J2-引用与字节守恒",
        led.opens == led.refs_total() && led.refs_total() == 4 && led.used_total() == 260,
        "opens 等于引用和，mq 在队字节分毫不差",
    );
    let inj = inject_four();
    s.add("J3a-空名拒止", inj[0], "空名创建必须 PX_EMPTY");
    s.add("J3b-超长名拒止", inj[1], "256 字节名必须 PX_RANGE");
    s.add("J3c-重名拒止", inj[2], "同名跨 kind 创建必须 PX_DUP");
    s.add("J3d-已unlink名拒", inj[3], "查 unlink 名必须 PX_MISMATCH");
    let mut fz = led.clone();
    let fp = fz.freeze();
    let frozen_ok = matches!(fz.create(0, "/new", 8), Err(PX_FROZEN))
        && matches!(fz.acquire(1), Err(PX_FROZEN))
        && matches!(fz.mq_send(8, 1), Err(PX_FROZEN));
    s.add(
        "J4-冻结不可写",
        frozen_ok && fz.fold() == fp,
        "冻结后创建/打开/发送全拒且指纹不变",
    );
    let fp_a = led.clone().fold();
    let mut b = led.clone();
    let sens = b.acquire(1).is_ok() && b.fold() != fp_a;
    s.add("J5-指纹敏感性", sens, "多一次引用必须改变 fold");
    let inner_slash = matches!(PosixLedger::new().create(0, "/a/b", 8), Err(PX_MISMATCH));
    let zero_send = {
        let mut g = PosixLedger::new();
        match g.create(2, "/m", 8) {
            Ok(id) => matches!(g.mq_send(id, 0), Err(PX_EMPTY)),
            Err(_) => false,
        }
    };
    let over_cap = {
        let mut g = PosixLedger::new();
        match g.create(2, "/m2", 8) {
            Ok(id) => matches!(g.mq_send(id, 9), Err(PX_SHORT)),
            Err(_) => false,
        }
    };
    s.add(
        "J6-范围与空账拒止",
        inner_slash && zero_send && over_cap,
        "内嵌斜杠/空发送/超容量三路都必须拒",
    );
    let mut names_unique = true;
    let mut k = 0usize;
    while k < led.objs.len() {
        if led.objs[k].id == 0 {
            names_unique = false;
        }
        k += 1;
    }
    let dup_reject = {
        let mut g = PosixLedger::new();
        g.create(0, "/x", 8).is_ok() && matches!(g.create(0, "/x", 8), Err(PX_DUP))
    };
    s.add(
        "J7-唯一与序不变式",
        names_unique && led.next_id == 11 && dup_reject,
        "id 从 1 连续、名字唯一、重名当场拒",
    );
    let mut partial = match sample_ledger() {
        Ok(v) => v,
        Err(_) => PosixLedger::new(),
    };
    while partial.objs.len() > 5 {
        partial.objs.pop();
    }
    let negative = partial.objs.len() == 5 && matches!(partial.verify(), Err(PX_SHORT));
    s.add(
        "J8-半成品不得全绿",
        negative,
        "五对象半账必须 PX_SHORT，不得伪装成绿",
    );
    let ref_cycle = {
        let mut g = PosixLedger::new();
        match g.create(1, "/shmX", 4096) {
            Ok(id) => {
                let a = g.acquire(id).is_ok();
                let b = g.release(id).is_ok();
                a && b && matches!(g.release(id), Err(PX_EMPTY))
            }
            Err(_) => false,
        }
    };
    let unlink_twice = {
        let mut g = PosixLedger::new();
        match g.create(2, "/mqX", 64) {
            Ok(_) => g.unlink("/mqX").is_ok() && matches!(g.unlink("/mqX"), Err(PX_DUP)),
            Err(_) => false,
        }
    };
    s.add(
        "J10-引用与unlink幂等",
        ref_cycle && unlink_twice,
        "归零后再放必须拒、重复 unlink 必须 PX_DUP",
    );
    s.add_dyn(
        "J9-动态账",
        verified == 10 && led.used_total() == 260,
        &format!(
            "verified={} opens={} refs={} used={} unlinks={}",
            verified,
            led.opens,
            led.refs_total(),
            led.used_total(),
            led.unlinks
        ),
    );
    s.add_dyn(
        "J9b-指纹账",
        led.fold() != 0,
        &format!("fold={:016x} tag={:x}", led.fold(), PX_TAG),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11070_flow() {
        let mut led = sample_ledger().unwrap_or(PosixLedger::new());
        assert_eq!(led.verify().unwrap_or(0), 10);
        assert!(led.namespace_consistent());
        assert_eq!(led.lookup("/sem1").unwrap_or(0), 2);
        assert!(matches!(led.lookup("/mq2"), Err(PX_MISMATCH)));
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11070_checks_green() {
        assert!(run_unx11070_checks().all_passed(), "UNX-F11070 判据未全绿");
    }
}
