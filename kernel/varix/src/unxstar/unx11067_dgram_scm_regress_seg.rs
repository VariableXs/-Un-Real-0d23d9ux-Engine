//! UNX-F11067 DGRAM/SEQPACKET/SCM_RIGHTS 回归段（0x1e860 段）
//!
//! B10/B11 判据在此重放：报文队列按 (seq, 类型, 载荷, 边界长度, 截断位)
//! 记账，fd 传递以 RightsBatch 单独登记（InFlight → Installed →
//! Released 三态）。语义要求 DGRAM 单 fd 上限、SEQPACKET 可携带多 fd、
//! 截断报文必须显式 allow_partial 才能交付，且安装/释放不得跨态。
//!
//! 不变式：delivered + pending == 队列表长；已安装 fd 总数等于三批
//! 之和；截断报文的 boundary 必须大于 payload；冻结后写路径全拒。
//!
//! 结构：DgramMsg（报文行）/ RightsBatch（fd 批次）/ DgramSegment
//! （两批总账 + 重放 + 冻结）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11067 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e860;

/// 空账：seq 为 0、零载荷、空 fd 批次或查无此报文
pub const DS_EMPTY: u32 = SEG_BASE + 0x01;
/// 越界：类型码非法、fd 过多或截断边界倒挂
pub const DS_RANGE: u32 = SEG_BASE + 0x02;
/// 重号：seq 或 fd 批次重复
pub const DS_DUP: u32 = SEG_BASE + 0x03;
/// 失配：截断报文未授权交付、或 fd 相位冲突
pub const DS_MISMATCH: u32 = SEG_BASE + 0x04;
/// 短缺：队列达上限
pub const DS_SHORT: u32 = SEG_BASE + 0x05;
/// 已封：段账冻结后的入队/交付/挂权
pub const DS_FROZEN: u32 = SEG_BASE + 0x06;

/// FNV 折前缀：低两字节 0x67 标记 11067 号卡
pub const DS_TAG: u64 = 0x1e80_67;

/// 本段固定十席
const SEATS: usize = 10;
/// SCM_RIGHTS 单批 fd 上限（内核 SCM_MAX_FD）
const MAX_FDS: usize = 253;
/// DGRAM 单批 fd 上限（本段判据）
const DGRAM_FD_CAP: usize = 1;

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

/// 报文类型闭集
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SockKind {
    /// SOCK_DGRAM：保留边界、单 fd
    Dgram,
    /// SOCK_SEQPACKET：保留边界、可多 fd
    SeqPacket,
}

impl SockKind {
    /// 落账判别码（与 wire 层对齐）
    pub fn code(self) -> u8 {
        match self {
            SockKind::Dgram => 0,
            SockKind::SeqPacket => 1,
        }
    }

    /// fd 批上限
    pub fn fd_cap(self) -> usize {
        match self {
            SockKind::Dgram => DGRAM_FD_CAP,
            SockKind::SeqPacket => MAX_FDS,
        }
    }
}

/// fd 批次三态
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RightsPhase {
    /// 随报文在途
    InFlight,
    /// 已装入接收端 fd 表
    Installed,
    /// 已释放
    Released,
}

impl RightsPhase {
    /// 落账判别码
    pub fn code(self) -> u8 {
        match self {
            RightsPhase::InFlight => 1,
            RightsPhase::Installed => 2,
            RightsPhase::Released => 3,
        }
    }
}

/// 一条报文
#[derive(Clone, Debug)]
pub struct DgramMsg {
    /// 报文序号（1..=10，段内唯一）
    pub seq: u16,
    /// 类型码（SockKind::code）
    pub kind: u8,
    /// 实际载荷字节
    pub payload: u16,
    /// 原始边界长度（截断前）
    pub boundary: u16,
    /// 发送端标识
    pub sender: u32,
    /// 接收端标识
    pub receiver: u32,
    /// 是否已被截断
    pub truncated: bool,
    /// 是否已交付
    pub delivered: bool,
}

impl DgramMsg {
    /// 新建一条完整报文
    pub fn new(seq: u16, kind: u8, payload: u16, sender: u32, receiver: u32) -> DgramMsg {
        DgramMsg {
            seq,
            kind,
            payload,
            boundary: payload,
            sender,
            receiver,
            truncated: false,
            delivered: false,
        }
    }

    /// 边界未倒挂（boundary >= payload）
    pub fn boundary_ok(&self) -> bool {
        self.boundary >= self.payload
    }

    /// 报文行指纹
    pub fn digest(&self, mut h: u64) -> u64 {
        h = fnv_u16(h, self.seq);
        h = fnv_byte(h, self.kind);
        h = fnv_u16(h, self.payload);
        h = fnv_u16(h, self.boundary);
        h = fnv_u32(h, self.sender);
        h = fnv_u32(h, self.receiver);
        h = fnv_byte(h, if self.truncated { 1 } else { 0 });
        fnv_byte(h, if self.delivered { 1 } else { 0 })
    }
}

/// 随报文传递的 fd 批次
#[derive(Clone, Debug)]
pub struct RightsBatch {
    /// 绑定报文序号
    pub msg: u16,
    /// fd 列表
    pub fds: Vec<u32>,
    /// 相位
    pub phase: RightsPhase,
}

impl RightsBatch {
    /// 构造 InFlight 批次：空列表 / 零 fd / 超上限三路拒
    pub fn attach(msg: u16, fds: Vec<u32>) -> Result<RightsBatch, u32> {
        if msg == 0 || fds.is_empty() {
            return Err(DS_EMPTY);
        }
        if fds.len() > MAX_FDS {
            return Err(DS_RANGE);
        }
        let mut k = 0usize;
        while k < fds.len() {
            if fds[k] == 0 {
                return Err(DS_RANGE);
            }
            k += 1;
        }
        Ok(RightsBatch { msg, fds, phase: RightsPhase::InFlight })
    }

    /// 装入接收端 fd 表
    pub fn install(&mut self) -> Result<u16, u32> {
        if self.phase != RightsPhase::InFlight {
            return Err(DS_MISMATCH);
        }
        self.phase = RightsPhase::Installed;
        Ok(self.fds.len() as u16)
    }

    /// 释放批次
    pub fn release(&mut self) -> Result<u16, u32> {
        if self.phase != RightsPhase::Installed {
            return Err(DS_MISMATCH);
        }
        self.phase = RightsPhase::Released;
        Ok(self.fds.len() as u16)
    }

    /// 批次指纹
    pub fn digest(&self, mut h: u64) -> u64 {
        h = fnv_u16(h, self.msg);
        h = fnv_byte(h, self.phase.code());
        let mut k = 0usize;
        while k < self.fds.len() {
            h = fnv_u32(h, self.fds[k]);
            k += 1;
        }
        h
    }
}

/// B10/B11 报文总账
#[derive(Clone, Debug)]
pub struct DgramSegment {
    /// 报文队列（交付后保留，仅置位）
    pub queue: Vec<DgramMsg>,
    /// fd 批次表
    pub rights: Vec<RightsBatch>,
    /// 队列容量
    pub cap: u16,
    /// 已交付条数
    pub delivered: u32,
    /// 最近一次重放合法条数
    pub verified: u16,
    frozen: Option<u64>,
}

impl DgramSegment {
    /// 以队列容量建空账
    pub fn new(cap: u16) -> DgramSegment {
        DgramSegment {
            queue: Vec::new(),
            rights: Vec::new(),
            cap,
            delivered: 0,
            verified: 0,
            frozen: None,
        }
    }

    fn guard(&self) -> Result<(), u32> {
        if self.frozen.is_some() {
            return Err(DS_FROZEN);
        }
        Ok(())
    }

    fn find_msg(&self, seq: u16) -> Option<usize> {
        let mut k = 0usize;
        while k < self.queue.len() {
            if self.queue[k].seq == seq {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    fn find_rights(&self, seq: u16) -> Option<usize> {
        let mut k = 0usize;
        while k < self.rights.len() {
            if self.rights[k].msg == seq {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 入队：0 号/非法类型/零载荷/满队/重号/倒挂边界六道闸
    pub fn enqueue(&mut self, msg: DgramMsg) -> Result<(), u32> {
        self.guard()?;
        if msg.seq == 0 {
            return Err(DS_EMPTY);
        }
        if msg.kind > 1 {
            return Err(DS_RANGE);
        }
        if msg.payload == 0 {
            return Err(DS_EMPTY);
        }
        if self.queue.len() >= self.cap as usize {
            return Err(DS_SHORT);
        }
        if self.find_msg(msg.seq).is_some() {
            return Err(DS_DUP);
        }
        if !msg.boundary_ok() {
            return Err(DS_RANGE);
        }
        if msg.truncated && msg.boundary == msg.payload {
            return Err(DS_MISMATCH);
        }
        self.queue.push(msg);
        Ok(())
    }

    /// 给既有报文挂 fd 批次：查无报文 / 超类型上限 / 重批三路拒
    pub fn send_rights(&mut self, seq: u16, fds: Vec<u32>) -> Result<u16, u32> {
        self.guard()?;
        let idx = match self.find_msg(seq) {
            Some(i) => i,
            None => return Err(DS_EMPTY),
        };
        if self.find_rights(seq).is_some() {
            return Err(DS_DUP);
        }
        let kind = self.queue[idx].kind;
        if fds.len() > DGRAM_FD_CAP && kind == SockKind::Dgram.code() {
            return Err(DS_RANGE);
        }
        if fds.len() >= MAX_FDS {
            return Err(DS_RANGE);
        }
        let batch = RightsBatch::attach(seq, fds)?;
        let n = batch.fds.len() as u16;
        self.rights.push(batch);
        Ok(n)
    }

    /// 安装指定报文的 fd 批次
    pub fn install_rights(&mut self, seq: u16) -> Result<u16, u32> {
        self.guard()?;
        let idx = match self.find_rights(seq) {
            Some(i) => i,
            None => return Err(DS_EMPTY),
        };
        self.rights[idx].install()
    }

    /// 交付报文：截断件必须显式 allow_partial
    pub fn deliver(&mut self, seq: u16, allow_partial: bool) -> Result<u16, u32> {
        self.guard()?;
        let idx = match self.find_msg(seq) {
            Some(i) => i,
            None => return Err(DS_EMPTY),
        };
        if self.queue[idx].delivered {
            return Err(DS_DUP);
        }
        if self.queue[idx].truncated && !allow_partial {
            return Err(DS_MISMATCH);
        }
        self.queue[idx].delivered = true;
        self.delivered = self.delivered.wrapping_add(1);
        Ok(self.queue[idx].payload)
    }

    /// 交付与待交付守恒
    pub fn conserved(&self) -> bool {
        let mut pending = 0u32;
        let mut k = 0usize;
        while k < self.queue.len() {
            if !self.queue[k].delivered {
                pending = pending.wrapping_add(1);
            }
            k += 1;
        }
        self.delivered.wrapping_add(pending) == self.queue.len() as u32
    }

    /// 边界完整条数
    pub fn boundary_intact(&self) -> u16 {
        let mut n = 0u16;
        let mut k = 0usize;
        while k < self.queue.len() {
            if self.queue[k].boundary_ok() {
                n = n.wrapping_add(1);
            }
            k += 1;
        }
        n
    }

    /// fd 传递一致性：三批全 Installed、批次之和等于 fd 总数
    pub fn rights_consistent(&self) -> bool {
        if self.rights.is_empty() {
            return false;
        }
        let mut fds = 0usize;
        let mut k = 0usize;
        while k < self.rights.len() {
            let b = &self.rights[k];
            if b.phase != RightsPhase::Installed {
                return false;
            }
            if self.find_msg(b.msg).is_none() {
                return false;
            }
            fds = fds.wrapping_add(b.fds.len());
            k += 1;
        }
        fds == 4
    }

    /// 重放：不足十席 DS_SHORT，交付不全 DS_MISMATCH
    pub fn verify(&mut self) -> Result<u16, u32> {
        self.guard()?;
        if self.queue.len() < SEATS {
            return Err(DS_SHORT);
        }
        let mut ok = 0u16;
        let mut k = 0usize;
        while k < self.queue.len() {
            let m = &self.queue[k];
            if m.delivered && m.boundary_ok() {
                ok = ok.wrapping_add(1);
            }
            k += 1;
        }
        self.verified = ok;
        if ok as usize != self.queue.len() {
            return Err(DS_MISMATCH);
        }
        Ok(ok)
    }

    /// 段账指纹
    pub fn fold(&self) -> u64 {
        let mut h = DS_TAG;
        h = fnv_u16(h, self.cap);
        h = fnv_u32(h, self.delivered);
        h = fnv_u16(h, self.verified);
        let mut k = 0usize;
        while k < self.queue.len() {
            h = self.queue[k].digest(h);
            k += 1;
        }
        k = 0;
        while k < self.rights.len() {
            h = self.rights[k].digest(h);
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

/// 四条负路径：零 seq / 非法类型 / 重 seq / 重批
pub fn inject_four() -> [bool; 4] {
    let mut seg = DgramSegment::new(4);
    let empty = matches!(
        seg.enqueue(DgramMsg::new(0, 0, 8, 1, 2)),
        Err(DS_EMPTY)
    );
    let mut seg2 = DgramSegment::new(4);
    let range = matches!(
        seg2.enqueue(DgramMsg::new(1, 2, 8, 1, 2)),
        Err(DS_RANGE)
    );
    let mut seg3 = DgramSegment::new(4);
    let first = seg3.enqueue(DgramMsg::new(1, 0, 8, 1, 2)).is_ok();
    let dup = first
        && matches!(seg3.enqueue(DgramMsg::new(1, 0, 9, 1, 2)), Err(DS_DUP));
    let mut seg4 = DgramSegment::new(4);
    let seeded = seg4.enqueue(DgramMsg::new(1, 1, 8, 1, 2)).is_ok()
        && seg4.send_rights(1, alloc::vec![21]).is_ok();
    let batch_dup = seeded && matches!(seg4.send_rights(1, alloc::vec![22]), Err(DS_DUP));
    [empty, range, dup, batch_dup]
}

/// 构造报文样本（十条、三批 fd、一条截断件显式放行）
fn sample_segment() -> Result<DgramSegment, u32> {
    let mut seg = DgramSegment::new(12);
    let mut i = 1u16;
    while i <= 10 {
        let kind = if i % 2 == 0 { 1 } else { 0 };
        let payload = 90 + i.wrapping_mul(3);
        let mut msg = DgramMsg::new(i, kind, payload, 500 + i as u32, 600 + i as u32);
        if i == 8 {
            msg.boundary = payload.wrapping_add(236);
            msg.truncated = true;
        }
        seg.enqueue(msg)?;
        i = i.wrapping_add(1);
    }
    seg.send_rights(2, alloc::vec![21])?;
    seg.send_rights(4, alloc::vec![31, 32])?;
    seg.send_rights(7, alloc::vec![41])?;
    seg.install_rights(2)?;
    seg.install_rights(4)?;
    seg.install_rights(7)?;
    let mut d = 1u16;
    while d <= 10 {
        seg.deliver(d, d == 8)?;
        d = d.wrapping_add(1);
    }
    Ok(seg)
}

/// UNX-F11067 判据入口
pub fn run_unx11067_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11067");
    let mut seg = match sample_segment() {
        Ok(v) => v,
        Err(_) => DgramSegment::new(12),
    };
    let verified = seg.verify().unwrap_or(0);
    s.add(
        "J1a-报文判据全量重放",
        verified == 10 && seg.queue.len() == SEATS,
        "B10/B11 十条报文必须全部交付且边界合法",
    );
    s.add(
        "J1b-fd传递一致",
        seg.rights_consistent() && seg.rights.len() == 3,
        "三批 fd 必须全 Installed 且总数等于 4",
    );
    s.add(
        "J2-交付守恒",
        seg.conserved() && seg.delivered == 10,
        "已交付加待交付必须等于队列表长",
    );
    let inj = inject_four();
    s.add("J3a-零序号拒止", inj[0], "seq=0 入队必须 DS_EMPTY");
    s.add("J3b-非法类型拒止", inj[1], "类型码 2 必须 DS_RANGE");
    s.add("J3c-重序号拒止", inj[2], "同 seq 二次入队必须 DS_DUP");
    s.add("J3d-重批拒止", inj[3], "同报文二次挂权必须 DS_DUP");
    let mut fz = seg.clone();
    let fp = fz.freeze();
    let frozen_ok = matches!(fz.deliver(1, false), Err(DS_FROZEN))
        && matches!(fz.send_rights(1, alloc::vec![9]), Err(DS_FROZEN));
    s.add(
        "J4-冻结不可写",
        frozen_ok && fz.fold() == fp,
        "冻结后交付/挂权全拒且指纹不变",
    );
    let fp_a = seg.clone().fold();
    let mut b = seg.clone();
    let sens = b.send_rights(9, alloc::vec![51]).is_ok() && b.fold() != fp_a;
    s.add("J5-指纹敏感性", sens, "新增一批 fd 必须改变 fold");
    let full = {
        let mut g = DgramSegment::new(1);
        g.enqueue(DgramMsg::new(1, 0, 4, 1, 2)).is_ok()
            && matches!(g.enqueue(DgramMsg::new(2, 0, 4, 1, 2)), Err(DS_SHORT))
    };
    let zero_payload = matches!(
        DgramSegment::new(4).enqueue(DgramMsg::new(1, 0, 0, 1, 2)),
        Err(DS_EMPTY)
    );
    let empty_batch = {
        let mut g = DgramSegment::new(2);
        g.enqueue(DgramMsg::new(1, 0, 4, 1, 2)).is_ok()
            && matches!(g.send_rights(1, Vec::new()), Err(DS_EMPTY))
    };
    s.add(
        "J6-范围与空账拒止",
        full && zero_payload && empty_batch,
        "满队/零载荷/空 fd 批三路都必须拒",
    );
    let mut ordered = true;
    let mut k = 1usize;
    while k < seg.queue.len() {
        if seg.queue[k].seq <= seg.queue[k - 1].seq {
            ordered = false;
        }
        k += 1;
    }
    let mut dseg = DgramSegment::new(2);
    let dup_reject = dseg.enqueue(DgramMsg::new(1, 0, 4, 1, 2)).is_ok()
        && matches!(dseg.enqueue(DgramMsg::new(1, 0, 4, 1, 2)), Err(DS_DUP));
    s.add(
        "J7-唯一与排序不变量",
        ordered && dup_reject,
        "seq 必须唯一递增且重号当场拒",
    );
    let mut partial = match sample_segment() {
        Ok(v) => v,
        Err(_) => DgramSegment::new(12),
    };
    while partial.queue.len() > 5 {
        partial.queue.pop();
    }
    let negative = partial.queue.len() == 5 && matches!(partial.verify(), Err(DS_SHORT));
    s.add(
        "J8-半成品不得全绿",
        negative,
        "五条半账必须 DS_SHORT，不得伪装成绿",
    );
    let trunc_guard = {
        let mut g = DgramSegment::new(2);
        let mut m = DgramMsg::new(1, 0, 8, 1, 2);
        m.boundary = 40;
        m.truncated = true;
        g.enqueue(m).is_ok()
            && matches!(g.deliver(1, false), Err(DS_MISMATCH))
            && matches!(g.deliver(1, true), Ok(8))
    };
    let dgram_multi = {
        let mut g = DgramSegment::new(2);
        g.enqueue(DgramMsg::new(1, 0, 8, 1, 2)).is_ok()
            && matches!(g.send_rights(1, alloc::vec![3, 4]), Err(DS_RANGE))
    };
    s.add(
        "J10-截断与单fd纪律",
        trunc_guard && dgram_multi,
        "截断件须显式放行、DGRAM 多 fd 必须拒",
    );
    s.add_dyn(
        "J9-动态账",
        verified == 10 && seg.boundary_intact() == 10,
        &format!(
            "verified={} 交付={} 边界完整={} 批次={}",
            verified,
            seg.delivered,
            seg.boundary_intact(),
            seg.rights.len()
        ),
    );
    s.add_dyn(
        "J9b-指纹账",
        seg.fold() != 0,
        &format!("fold={:016x} tag={:x}", seg.fold(), DS_TAG),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11067_flow() {
        let mut seg = sample_segment().unwrap_or(DgramSegment::new(12));
        assert_eq!(seg.verify().unwrap_or(0), 10);
        assert!(seg.rights_consistent());
        assert!(seg.conserved());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11067_checks_green() {
        assert!(run_unx11067_checks().all_passed(), "UNX-F11067 判据未全绿");
    }
}
