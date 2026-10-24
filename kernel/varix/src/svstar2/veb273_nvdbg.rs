//! veb273_nvdbg —— VE-F0273 NVIDIA 调试通道。
//! 锚点（CoRun Unxreal 主册）：VE-F0273 · NVIDIA 调试通道 · 目标 300 行
//! 判据原文：VE-F0273-J1 调试通道环形日志：定容环（64 槽）+
//!           严重级（trace/info/warn/error）；级过滤读（只取 ≥
//!           某级）；环回绕覆盖（写满覆最旧）；通道快照哈希
//!           （内容序 FNV32）；错误级日志计数（error 数 != 0
//!           提示有故障在册）。
//! 组件：环形日志 + 级过滤 + 快照哈希。
//! 职责定位：调试通道是「NVIDIA 通路的行车记录仪」——定容环
//!           保证总有最近 64 条现场：太老的现场本来就该被新现
//!           场覆盖；级过滤让运维只看 error 不用翻 64 条 info；
//!           error 计数非零就是亮红灯，不用人翻日志。
//! 性能逐项分解：写 O(1)；过滤读 O(环容)；哈希 O(环容)。
//! 诊断码段：0x30D40（0x30D41~0x30D49 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const DB_VERSION: &str = "veb273_nvdbg-v1";

/// 诊断段基址（0x30D40 段 · 本模块独占）。
pub const DB_SEG_BASE: u32 = 0x30D40;
pub const XC_MAGIC: u32 = 0x30D41; // 账魔数破
pub const XC_LV_OOR: u32 = 0x30D42; // 级越域
pub const XC_EMPTY: u32 = 0x30D43; // 空环读

/// 环容。
pub const RING_CAP: usize = 64;

/// 严重级。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Lv {
    Trace = 0,
    Info = 1,
    Warn = 2,
    Error = 3,
}

impl Lv {
    pub fn from_u32(v: u32) -> Option<Lv> {
        match v {
            0 => Some(Lv::Trace),
            1 => Some(Lv::Info),
            2 => Some(Lv::Warn),
            3 => Some(Lv::Error),
            _ => None,
        }
    }
}

/// 日志条。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogItem {
    pub seq: u64,
    pub lv: Lv,
    pub code: u32,
}

fn fnv32g(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 环形调试日志。
pub struct DbgRing {
    buf: Vec<LogItem>,
    head: usize, // 下一写位
    total: u64,
    magic: u32,
}

impl DbgRing {
    /// 建。
    pub fn new() -> DbgRing {
        let mut buf = Vec::new();
        for _ in 0..RING_CAP {
            buf.push(LogItem { seq: 0, lv: Lv::Trace, code: 0 });
        }
        DbgRing { buf, head: 0, total: 0, magic: 0x0D40_0B6A }
    }

    /// 写一条（满则覆最旧）。
    pub fn write(&mut self, lv: Lv, code: u32) -> Result<u64, u32> {
        if self.magic != 0x0D40_0B6A {
            return Err(XC_MAGIC);
        }
        let seq = self.total + 1;
        self.buf[self.head] = LogItem { seq, lv, code };
        self.head = (self.head + 1) % RING_CAP;
        self.total = seq;
        Ok(seq)
    }

    /// 现存条数。
    pub fn live(&self) -> usize {
        if self.total as usize >= RING_CAP {
            RING_CAP
        } else {
            self.total as usize
        }
    }

    /// 级过滤读（seq 升序，只取 ≥ floor）。
    pub fn read_ge(&self, floor: Lv) -> Result<Vec<LogItem>, u32> {
        if self.magic != 0x0D40_0B6A {
            return Err(XC_MAGIC);
        }
        if self.live() == 0 {
            return Err(XC_EMPTY);
        }
        // 收集现存条目按 seq 升序。
        let mut items: Vec<LogItem> = Vec::new();
        let start = if self.total as usize >= RING_CAP {
            self.head // head 指向最旧
        } else {
            0
        };
        for k in 0..self.live() {
            let it = self.buf[(start + k) % RING_CAP];
            if it.lv >= floor {
                items.push(it);
            }
        }
        Ok(items)
    }

    /// error 计数。
    pub fn err_count(&self) -> Result<u32, u32> {
        let items = self.read_ge(Lv::Error)?;
        Ok(items.len() as u32)
    }

    /// 快照哈希（现存条目 seq + lv + code 序）。
    pub fn snapshot(&self) -> Result<u32, u32> {
        let items = self.read_ge(Lv::Trace)?;
        let mut buf: Vec<u8> = Vec::new();
        for it in items.iter() {
            buf.extend_from_slice(&it.seq.to_le_bytes());
            buf.push(it.lv as u8);
            buf.extend_from_slice(&it.code.to_le_bytes());
        }
        Ok(fnv32g(&buf))
    }
}

pub fn run_veb273_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0273");
    // 族一：写与级。
    let mut r = DbgRing::new();
    s.add("空环读拒", r.read_ge(Lv::Trace) == Err(XC_EMPTY), "无现场");
    let q1 = r.write(Lv::Info, 0x100);
    s.add("写 1", q1 == Ok(1), "序 1");
    let _ = r.write(Lv::Warn, 0x200);
    let _ = r.write(Lv::Error, 0x300);
    let _ = r.write(Lv::Trace, 0x400);
    s.add("现存 4", r.live() == 4, "四条");
    // 族二：级过滤。
    let all = r.read_ge(Lv::Trace);
    s.add("全读 4", matches!(&all, Ok(v) if v.len() == 4), "全量");
    let warn_up = r.read_ge(Lv::Warn);
    s.add("warn+ 2", matches!(&warn_up, Ok(v) if v.len() == 2), "0x200/0x300");
    let err_only = r.read_ge(Lv::Error);
    s.add("error 1", matches!(&err_only, Ok(v) if v.len() == 1 && v[0].code == 0x300), "红灯一盏");
    s.add("error 计 1", r.err_count() == Ok(1), "计数同");
    // 序升序核。
    s.add("序升", matches!(&all, Ok(v) if v[0].seq == 1 && v[3].seq == 4), "时序");
    // 族三：回绕覆盖。
    let mut w = DbgRing::new();
    for i in 0..70u32 {
        let _ = w.write(Lv::Info, i);
    }
    s.add("满环 64", w.live() == 64, "定容");
    let tail = w.read_ge(Lv::Trace);
    s.add("旧 1~6 被覆", matches!(&tail, Ok(v) if v[0].seq == 7 && v[0].code == 6), "最旧 6 条没了");
    s.add("尾 70 在", matches!(&tail, Ok(v) if v[63].seq == 70), "最新在");
    // 覆盖后快照稳定。
    let h1 = w.snapshot();
    let h2 = w.snapshot();
    s.add("快照稳", h1 == h2, "两遍同");
    // 族四：级闭集。
    s.add("级闭集", Lv::from_u32(3) == Some(Lv::Error) && Lv::from_u32(4).is_none(), "4 级");
    s.add("级序", Lv::Trace < Lv::Error, "严重度序");
    s.add("魔数核", DB_SEG_BASE == 0x30D40, "段基址");
    s
}
