//! UNX-F11068 epoll/select/poll/futex 回归段（0x1e880 段）
//!
//! B12/B19/B26 判据在此重放：兴趣表以 (fd, 事件掩码, 触发模式, 上次就绪位)
//! 记账，epoll 边沿模式只在 0→1 的位上计一次命中，水平模式每次就绪都计。
//! futex 字以 (地址, 值, 等待者, 已唤醒, 代数) 记账，park 必须值匹配，
//! wake 不得超过等待者数。
//!
//! 不变式：parked == waiters + woken（每字）；fired_total == 各位命中之和；
//! 超时被夹在 [-1, 600000] 毫秒；冻结后注册/poll/park 全以 EP_FROZEN 拒止。
//!
//! 结构：Interest（兴趣行）/ FutexWord（等待字）/ PollSegment
//! （三批总账 + 重放 + 冻结）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11068 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e880;

/// 空账：fd 为 0、空兴趣表、空就绪集或 wake(0)
pub const EP_EMPTY: u32 = SEG_BASE + 0x01;
/// 越界：掩码越界、触发码非法或超时越窗
pub const EP_RANGE: u32 = SEG_BASE + 0x02;
/// 重号：同 fd 二次注册或同 futex 地址二次登记
pub const EP_DUP: u32 = SEG_BASE + 0x03;
/// 失配：futex 值不匹配或就绪面与兴趣面冲突
pub const EP_MISMATCH: u32 = SEG_BASE + 0x04;
/// 短缺：唤醒数超过等待者或重放席位不足
pub const EP_SHORT: u32 = SEG_BASE + 0x05;
/// 已封：段账冻结后的注册/轮询/唤醒
pub const EP_FROZEN: u32 = SEG_BASE + 0x06;

/// FNV 折前缀：低两字节 0x68 标记 11068 号卡
pub const EP_TAG: u64 = 0x1e80_68;

/// 本段固定十席
const SEATS: usize = 10;
/// 合法事件掩码上界（EPOLLIN..EPOLLERR 六位）
const MASK_MAX: u16 = 0x003f;
/// 单字等待者上限
const WAIT_CAP: u16 = 1024;
/// 超时上界（毫秒）
const TIMEOUT_MAX: i64 = 600_000;

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

fn fnv_u64(mut h: u64, v: u64) -> u64 {
    let mut k = 0u32;
    while k < 8 {
        h = fnv_byte(h, ((v >> k.wrapping_mul(8)) & 0xff) as u8);
        k = k.wrapping_add(1);
    }
    h
}

/// 触发模式：0 水平、1 边沿
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// 水平触发：每次就绪都重报
    Level,
    /// 边沿触发：仅 0→1 当次报
    Edge,
}

impl Trigger {
    /// 落账判别码
    pub fn code(self) -> u8 {
        match self {
            Trigger::Level => 0,
            Trigger::Edge => 1,
        }
    }
}

/// 一条兴趣登记
#[derive(Clone, Debug)]
pub struct Interest {
    /// 被监视 fd
    pub fd: u32,
    /// 关注的事件位
    pub mask: u16,
    /// 触发模式码（0/1）
    pub trig: u8,
    /// 上次观察到的就绪位（边沿判定用）
    pub prev: u16,
    /// 命中轮次
    pub hits: u32,
    /// 累计命中位数
    pub fired_bits: u32,
}

impl Interest {
    /// 新建兴趣行
    pub fn new(fd: u32, mask: u16, trig: u8) -> Interest {
        Interest { fd, mask, trig, prev: 0, hits: 0, fired_bits: 0 }
    }

    /// 观察一次就绪面，返回本次应上报的位
    pub fn observe(&mut self, ready: u16) -> u16 {
        let masked = self.mask & ready;
        let fired = if self.trig == Trigger::Edge.code() {
            masked & !self.prev
        } else {
            masked
        };
        self.prev = masked;
        if fired != 0 {
            self.hits = self.hits.wrapping_add(1);
            self.fired_bits = self.fired_bits.wrapping_add(fired.count_ones());
        }
        fired
    }

    /// 兴趣行指纹
    pub fn digest(&self, mut h: u64) -> u64 {
        h = fnv_u32(h, self.fd);
        h = fnv_u16(h, self.mask);
        h = fnv_byte(h, self.trig);
        h = fnv_u16(h, self.prev);
        h = fnv_u32(h, self.hits);
        fnv_u32(h, self.fired_bits)
    }
}

/// futex 等待字
#[derive(Clone, Debug)]
pub struct FutexWord {
    /// 用户态地址
    pub addr: u64,
    /// 期望值
    pub value: u32,
    /// 当前等待者
    pub waiters: u16,
    /// 累计唤醒
    pub woken: u32,
    /// 累计挂起
    pub parked: u32,
    /// 代数（每次 park/wake 递增）
    pub generation: u32,
}

impl FutexWord {
    /// 以地址建字（初值 0）
    pub fn new(addr: u64) -> FutexWord {
        FutexWord { addr, value: 0, waiters: 0, woken: 0, parked: 0, generation: 0 }
    }

    /// FUTEX_WAIT：值不匹配拒，等待者满拒
    pub fn park(&mut self, expect: u32) -> Result<u32, u32> {
        if self.value != expect {
            return Err(EP_MISMATCH);
        }
        if self.waiters >= WAIT_CAP {
            return Err(EP_SHORT);
        }
        self.waiters = self.waiters.wrapping_add(1);
        self.parked = self.parked.wrapping_add(1);
        self.generation = self.generation.wrapping_add(1);
        Ok(self.generation)
    }

    /// FUTEX_WAKE：0 唤醒拒，超额唤醒拒
    pub fn wake(&mut self, n: u16) -> Result<u16, u32> {
        if n == 0 {
            return Err(EP_EMPTY);
        }
        if n > self.waiters {
            return Err(EP_SHORT);
        }
        self.waiters = self.waiters.wrapping_sub(n);
        self.woken = self.woken.wrapping_add(n as u32);
        self.generation = self.generation.wrapping_add(1);
        Ok(n)
    }

    /// 守恒：parked == waiters + woken
    pub fn conserved(&self) -> bool {
        (self.waiters as u32).wrapping_add(self.woken) == self.parked
    }

    /// 等待字指纹
    pub fn digest(&self, mut h: u64) -> u64 {
        h = fnv_u64(h, self.addr);
        h = fnv_u32(h, self.value);
        h = fnv_u16(h, self.waiters);
        h = fnv_u32(h, self.woken);
        h = fnv_u32(h, self.parked);
        fnv_u32(h, self.generation)
    }
}

/// B12/B19/B26 复用总账
#[derive(Clone, Debug)]
pub struct PollSegment {
    /// 兴趣表
    pub interests: Vec<Interest>,
    /// futex 字表
    pub futexes: Vec<FutexWord>,
    /// 等待超时（-1 = 永久）
    pub timeout_ms: i64,
    /// 轮询轮次
    pub dispatch: u32,
    /// 累计上报位数
    pub fired_total: u32,
    /// 最近一次重放合法行数
    pub verified: u16,
    frozen: Option<u64>,
}

impl PollSegment {
    /// 空段账（默认永久等待）
    pub fn new() -> PollSegment {
        PollSegment {
            interests: Vec::new(),
            futexes: Vec::new(),
            timeout_ms: -1,
            dispatch: 0,
            fired_total: 0,
            verified: 0,
            frozen: None,
        }
    }

    fn guard(&self) -> Result<(), u32> {
        if self.frozen.is_some() {
            return Err(EP_FROZEN);
        }
        Ok(())
    }

    /// 注册兴趣：0 号/空掩码/越界掩码/非法触发/重 fd 五道闸
    pub fn register(&mut self, it: Interest) -> Result<(), u32> {
        self.guard()?;
        if it.fd == 0 {
            return Err(EP_EMPTY);
        }
        if it.mask == 0 || it.mask > MASK_MAX {
            return Err(EP_RANGE);
        }
        if it.trig > Trigger::Edge.code() {
            return Err(EP_RANGE);
        }
        let mut k = 0usize;
        while k < self.interests.len() {
            if self.interests[k].fd == it.fd {
                return Err(EP_DUP);
            }
            k += 1;
        }
        self.interests.push(it);
        Ok(())
    }

    /// 登记 futex 字
    pub fn register_futex(&mut self, addr: u64) -> Result<(), u32> {
        self.guard()?;
        if addr == 0 {
            return Err(EP_EMPTY);
        }
        let mut k = 0usize;
        while k < self.futexes.len() {
            if self.futexes[k].addr == addr {
                return Err(EP_DUP);
            }
            k += 1;
        }
        self.futexes.push(FutexWord::new(addr));
        Ok(())
    }

    /// 设置超时：夹在 [-1, 600000]
    pub fn set_timeout(&mut self, ms: i64) -> Result<(), u32> {
        self.guard()?;
        if ms < -1 || ms > TIMEOUT_MAX {
            return Err(EP_RANGE);
        }
        self.timeout_ms = ms;
        Ok(())
    }

    fn futex_index(&self, addr: u64) -> Option<usize> {
        let mut k = 0usize;
        while k < self.futexes.len() {
            if self.futexes[k].addr == addr {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 等待一个 futex 字
    pub fn park(&mut self, addr: u64, expect: u32) -> Result<u32, u32> {
        self.guard()?;
        let idx = match self.futex_index(addr) {
            Some(i) => i,
            None => return Err(EP_EMPTY),
        };
        self.futexes[idx].park(expect)
    }

    /// 唤醒一个 futex 字
    pub fn wake(&mut self, addr: u64, n: u16) -> Result<u16, u32> {
        self.guard()?;
        let idx = match self.futex_index(addr) {
            Some(i) => i,
            None => return Err(EP_EMPTY),
        };
        self.futexes[idx].wake(n)
    }

    /// 轮询一轮：上报位数累加并推进代数
    pub fn poll(&mut self, ready: &[(u32, u16)]) -> Result<u32, u32> {
        self.guard()?;
        if self.interests.is_empty() || ready.is_empty() {
            return Err(EP_EMPTY);
        }
        let mut fired = 0u32;
        let mut r = 0usize;
        while r < ready.len() {
            let fd = ready[r].0;
            let mask = ready[r].1;
            let mut k = 0usize;
            while k < self.interests.len() {
                if self.interests[k].fd == fd {
                    let got = self.interests[k].observe(mask);
                    fired = fired.wrapping_add(got.count_ones());
                }
                k += 1;
            }
            r += 1;
        }
        self.dispatch = self.dispatch.wrapping_add(1);
        self.fired_total = self.fired_total.wrapping_add(fired);
        if fired == 0 {
            return Err(EP_SHORT);
        }
        Ok(fired)
    }

    /// 各位命中之和
    pub fn bit_sum(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.interests.len() {
            n = n.wrapping_add(self.interests[k].fired_bits);
            k += 1;
        }
        n
    }

    /// 模式语义：触发码合法、边沿行 prev 已就位、futex 守恒
    pub fn mode_semantics(&self) -> bool {
        let mut k = 0usize;
        while k < self.interests.len() {
            let it = &self.interests[k];
            if it.trig > Trigger::Edge.code() || it.mask == 0 {
                return false;
            }
            if it.trig == Trigger::Edge.code() && it.hits > 0 && it.prev == 0 {
                return false;
            }
            k += 1;
        }
        k = 0;
        while k < self.futexes.len() {
            if !self.futexes[k].conserved() {
                return false;
            }
            k += 1;
        }
        true
    }

    /// fd 唯一且递增
    pub fn fds_ordered(&self) -> bool {
        let mut k = 1usize;
        while k < self.interests.len() {
            if self.interests[k].fd <= self.interests[k - 1].fd {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 重放：不足十席 EP_SHORT，有未命中行 EP_MISMATCH
    pub fn verify(&mut self) -> Result<u16, u32> {
        self.guard()?;
        if self.interests.len() < SEATS {
            return Err(EP_SHORT);
        }
        let mut ok = 0u16;
        let mut k = 0usize;
        while k < self.interests.len() {
            if self.interests[k].hits > 0 {
                ok = ok.wrapping_add(1);
            }
            k += 1;
        }
        self.verified = ok;
        if ok as usize != self.interests.len() {
            return Err(EP_MISMATCH);
        }
        Ok(ok)
    }

    /// 段账指纹
    pub fn fold(&self) -> u64 {
        let mut h = EP_TAG;
        h = fnv_u32(h, self.timeout_ms as u32);
        h = fnv_u32(h, self.dispatch);
        h = fnv_u32(h, self.fired_total);
        h = fnv_u16(h, self.verified);
        let mut k = 0usize;
        while k < self.interests.len() {
            h = self.interests[k].digest(h);
            k += 1;
        }
        k = 0;
        while k < self.futexes.len() {
            h = self.futexes[k].digest(h);
            k += 1;
        }
        h
    }

    /// 冻结段账
    pub fn freeze(&mut self) -> u64 {
        let f = self.fold();
        self.frozen = Some(f);
        f
    }
}

/// 四条负路径：零 fd / 越界掩码 / 重 fd / futex 值不匹配
pub fn inject_four() -> [bool; 4] {
    let mut s0 = PollSegment::new();
    let empty = matches!(s0.register(Interest::new(0, 0x01, 0)), Err(EP_EMPTY));
    let mut s1 = PollSegment::new();
    let range = matches!(s1.register(Interest::new(1, 0x40, 0)), Err(EP_RANGE));
    let mut s2 = PollSegment::new();
    let first = s2.register(Interest::new(1, 0x01, 0)).is_ok();
    let dup = first && matches!(s2.register(Interest::new(1, 0x02, 1)), Err(EP_DUP));
    let mut s3 = PollSegment::new();
    let seeded = s3.register_futex(0x1000).is_ok();
    let mismatch = seeded && matches!(s3.park(0x1000, 7), Err(EP_MISMATCH));
    [empty, range, dup, mismatch]
}

/// 构造复用样本（十兴趣、一轮全就绪、三 futex 字）
fn sample_segment() -> Result<PollSegment, u32> {
    let mut seg = PollSegment::new();
    let mut i = 0u32;
    while i < 10 {
        let mask = match i % 3 {
            0 => 0x0001u16,
            1 => 0x0006u16,
            _ => 0x0018u16,
        };
        let trig = if i % 2 == 0 { Trigger::Level.code() } else { Trigger::Edge.code() };
        seg.register(Interest::new(10 + i, mask, trig))?;
        i = i.wrapping_add(1);
    }
    seg.set_timeout(250)?;
    let mut ready: Vec<(u32, u16)> = Vec::new();
    let mut j = 0u32;
    while j < 10 {
        ready.push((10 + j, MASK_MAX));
        j = j.wrapping_add(1);
    }
    seg.poll(&ready)?;
    seg.register_futex(0x1000)?;
    seg.register_futex(0x2000)?;
    seg.register_futex(0x3000)?;
    seg.park(0x1000, 0)?;
    seg.park(0x1000, 0)?;
    seg.park(0x1000, 0)?;
    seg.wake(0x1000, 2)?;
    seg.park(0x2000, 0)?;
    seg.park(0x2000, 0)?;
    seg.wake(0x2000, 2)?;
    Ok(seg)
}

/// UNX-F11068 判据入口
pub fn run_unx11068_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11068");
    let mut seg = match sample_segment() {
        Ok(v) => v,
        Err(_) => PollSegment::new(),
    };
    let verified = seg.verify().unwrap_or(0);
    s.add(
        "J1a-复用判据全量重放",
        verified == 10 && seg.interests.len() == SEATS,
        "B12/B19/B26 十兴趣必须全部命中",
    );
    s.add(
        "J1b-模式语义一致",
        seg.mode_semantics() && seg.futexes.len() == 3,
        "边沿 prev 就位、futex 三字各守恒",
    );
    let level_before = seg.interests[0].hits;
    let edge_before = seg.interests[1].hits;
    let mut ready: Vec<(u32, u16)> = Vec::new();
    let mut j = 0u32;
    while j < 10 {
        ready.push((10 + j, MASK_MAX));
        j = j.wrapping_add(1);
    }
    let second = seg.poll(&ready).unwrap_or(0);
    let level_adv = seg.interests[0].hits > level_before;
    let edge_still = seg.interests[1].hits == edge_before;
    s.add(
        "J2-命中守恒",
        seg.fired_total == seg.bit_sum() && seg.dispatch == 2 && second > 0,
        "累计上报位必须等于各位命中之和",
    );
    let inj = inject_four();
    s.add("J3a-零fd拒止", inj[0], "fd=0 注册必须 EP_EMPTY");
    s.add("J3b-越界掩码拒", inj[1], "0x40 掩码必须 EP_RANGE");
    s.add("J3c-重fd拒止", inj[2], "同 fd 二次注册必须 EP_DUP");
    s.add("J3d-futex值拒", inj[3], "期望值不符必须 EP_MISMATCH");
    let mut fz = seg.clone();
    let fp = fz.freeze();
    let frozen_ok = matches!(fz.poll(&[(10, 0x01)]), Err(EP_FROZEN))
        && matches!(fz.park(0x1000, 0), Err(EP_FROZEN))
        && matches!(fz.register(Interest::new(99, 0x01, 0)), Err(EP_FROZEN));
    s.add(
        "J4-冻结不可写",
        frozen_ok && fz.fold() == fp,
        "冻结后轮询/等待/注册全拒且指纹不变",
    );
    let fp_a = seg.clone().fold();
    let mut b = seg.clone();
    let sens = b.set_timeout(300).is_ok() && b.fold() != fp_a;
    s.add("J5-指纹敏感性", sens, "超时 250→300 必须改变 fold");
    let neg_timeout = matches!(PollSegment::new().set_timeout(-2), Err(EP_RANGE));
    let over_timeout = matches!(PollSegment::new().set_timeout(700_000), Err(EP_RANGE));
    let zero_wake = {
        let mut g = PollSegment::new();
        g.register_futex(0x1000).is_ok()
            && g.park(0x1000, 0).is_ok()
            && matches!(g.wake(0x1000, 0), Err(EP_EMPTY))
    };
    s.add(
        "J6-范围与空账拒止",
        neg_timeout && over_timeout && zero_wake,
        "负超时/超窗超时/零唤醒三路都必须拒",
    );
    let order = seg.fds_ordered();
    let mut dseg = PollSegment::new();
    let dup_reject = dseg.register(Interest::new(5, 0x01, 0)).is_ok()
        && matches!(dseg.register(Interest::new(5, 0x02, 0)), Err(EP_DUP));
    s.add(
        "J7-唯一与排序不变量",
        order && dup_reject,
        "fd 必须唯一递增且重号当场拒",
    );
    let mut partial = match sample_segment() {
        Ok(v) => v,
        Err(_) => PollSegment::new(),
    };
    while partial.interests.len() > 5 {
        partial.interests.pop();
    }
    let negative = partial.interests.len() == 5 && matches!(partial.verify(), Err(EP_SHORT));
    s.add(
        "J8-半成品不得全绿",
        negative,
        "五兴趣半账必须 EP_SHORT，不得伪装成绿",
    );
    let futex_ok = {
        let mut g = PollSegment::new();
        g.register_futex(0x9000).is_ok()
            && g.park(0x9000, 0).is_ok()
            && matches!(g.wake(0x9000, 2), Err(EP_SHORT))
    };
    s.add(
        "J10-边沿与水平分野",
        level_adv && edge_still && futex_ok,
        "第二轮水平必增、边沿不增、超额唤醒必拒",
    );
    s.add_dyn(
        "J9-动态账",
        verified == 10 && seg.futexes.len() == 3,
        &format!(
            "verified={} 轮次={} 位和={} futex={}",
            verified,
            seg.dispatch,
            seg.bit_sum(),
            seg.futexes.len()
        ),
    );
    s.add_dyn(
        "J9b-指纹账",
        seg.fold() != 0,
        &format!("fold={:016x} tag={:x}", seg.fold(), EP_TAG),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11068_flow() {
        let mut seg = sample_segment().unwrap_or(PollSegment::new());
        assert_eq!(seg.verify().unwrap_or(0), 10);
        assert!(seg.mode_semantics());
        assert!(seg.bit_sum() >= 10);
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11068_checks_green() {
        assert!(run_unx11068_checks().all_passed(), "UNX-F11068 判据未全绿");
    }
}
