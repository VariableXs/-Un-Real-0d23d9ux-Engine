//! cgb830_security — CGPU-F0830 Intel 安全。
//!
//! 判据原文（验收行摘录）：Intel 安全：入口参数校验（长度/魔数/界内三
//! 闸）；权限边界（调用方能力环 0-3，环越权即拒）；沙箱参数账（提交批的
//! 内存窗口必须落在沙箱窗内——越界即拒）；安全事件账（拒收事件落账、按
//! 类别计数守恒）。判据：三闸校验、能力环、沙箱窗、事件守恒账。
//!
//! 诊断段 0x7240（本单独占）。
//!
//! 设计纪律：no_std+alloc，禁 unwrap/expect/panic/浮点/std；失败路径
//! Result<_, u16> 闭集错误码；界内判定全程防溢出（用加法前界比较）；定
//! 容事件账；drill() 自例程返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const MOD_VERSION: &str = "W3-cgb830-v1";

/// 诊断段基（本单独占 0x7240）。
pub const SEG_BASE: u16 = 0x7240;

/// 魔数不符（入口首字非 0xA5）。
pub const XC_BAD_MAGIC: u16 = SEG_BASE + 1;
/// 长度越限（载荷超上限）。
pub const XC_LEN_EXCEEDED: u16 = SEG_BASE + 2;
/// 界外访问（声明缓冲区间越出载荷界）。
pub const XC_OUT_OF_RANGE: u16 = SEG_BASE + 3;
/// 能力环越权（调用方环号大于所需环）。
pub const XC_RING_DENIED: u16 = SEG_BASE + 4;
/// 沙箱越界（内存窗口落在沙箱窗外）。
pub const XC_SANDBOX_ESCAPE: u16 = SEG_BASE + 5;
/// 事件账漂移（类别计数守恒破坏）。
pub const XC_LEDGER_DRIFT: u16 = SEG_BASE + 6;
/// 事件账满（定容事件账耗尽）。
pub const XC_EVENT_FULL: u16 = SEG_BASE + 7;

/// 载荷上限。
pub const PAYLOAD_CAP: usize = 64;
/// 魔数。
pub const ENTRY_MAGIC: u8 = 0xA5;
/// 事件账定容。
pub const EVENT_CAP: usize = 32;

/// 能力环（0 最高权）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Ring(pub u8);

/// 环常量。
pub const RING_KERNEL: Ring = Ring(0);
/// 环常量。
pub const RING_DRIVER: Ring = Ring(1);
/// 环常量。
pub const RING_SERVICE: Ring = Ring(2);
/// 环常量。
pub const RING_USER: Ring = Ring(3);

/// 内存窗口（半开区间 [base, base+len)）。
#[derive(Clone, Copy, Debug)]
pub struct Window {
    /// 基址。
    pub base: u64,
    /// 长度。
    pub len: u64,
}

impl Window {
    /// 界内判定（防溢出：base+len 溢出前先比上界）。
    pub fn within(&self, outer: &Window) -> Result<(), u16> {
        if self.len == 0 {
            return Ok(());
        }
        let self_end = match self.base.checked_add(self.len - 1) {
            Some(e) => e,
            None => return Err(XC_OUT_OF_RANGE),
        };
        let outer_end = match outer.base.checked_add(outer.len) {
            Some(e) => e,
            None => return Err(XC_SANDBOX_ESCAPE),
        };
        if self.base < outer.base || self_end >= outer_end {
            return Err(XC_SANDBOX_ESCAPE);
        }
        Ok(())
    }
}

/// 安全事件类别闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvKind {
    /// 魔数违例。
    BadMagic,
    /// 长度违例。
    BadLen,
    /// 界外违例。
    BadRange,
    /// 环越权。
    RingDeny,
    /// 沙箱越界。
    SandboxDeny,
}

impl EvKind {
    /// 类别代号（守恒账下标）。
    pub fn idx(self) -> usize {
        match self {
            EvKind::BadMagic => 0,
            EvKind::BadLen => 1,
            EvKind::BadRange => 2,
            EvKind::RingDeny => 3,
            EvKind::SandboxDeny => 4,
        }
    }
    /// 类别名。
    pub fn name(self) -> &'static str {
        match self {
            EvKind::BadMagic => "magic",
            EvKind::BadLen => "len",
            EvKind::BadRange => "range",
            EvKind::RingDeny => "ring",
            EvKind::SandboxDeny => "sandbox",
        }
    }
}

/// 安全事件。
#[derive(Clone, Debug)]
pub struct SecEvent {
    /// 类别。
    pub kind: EvKind,
    /// 调用方环。
    pub ring: u8,
    /// 事件序号。
    pub seq: u64,
}

/// 入口守门器。
pub struct Gate {
    /// 守门器名。
    pub name: String,
    /// 沙箱窗（初始化声明）。
    sandbox: Window,
    /// 事件账（定容）。
    pub events: Vec<SecEvent>,
    /// 类别计数（守恒项）。
    kind_cnt: [u64; 5],
    /// 放行计数。
    pub passed: u64,
    /// 事件序号。
    seq: u64,
}

/// 守恒恒等式：events.len() == Σ kind_cnt（五类之和）。
/// 每次落账后 verify() 复核。
impl Gate {
    /// 构造。
    pub fn new(name: &str, sandbox: Window) -> Self {
        Gate {
            name: name.to_string(),
            sandbox,
            events: Vec::new(),
            kind_cnt: [0; 5],
            passed: 0,
            seq: 0,
        }
    }

    /// 沙箱窗只读。
    pub fn sandbox(&self) -> Window {
        self.sandbox
    }

    /// 落账（判据"安全事件账"）。
    fn log(&mut self, kind: EvKind, ring: u8) -> Result<(), u16> {
        if self.events.len() >= EVENT_CAP {
            return Err(XC_EVENT_FULL);
        }
        self.seq += 1;
        self.events.push(SecEvent { kind, ring, seq: self.seq });
        self.kind_cnt[kind.idx()] += 1;
        self.verify()?;
        Ok(())
    }

    /// 入口三闸 + 权限 + 沙箱（判据核心路径）。
    /// payload：入口载荷；decl：声明缓冲（载荷内偏移+长）；need：所需权限环。
    pub fn submit(
        &mut self,
        ring: Ring,
        need: Ring,
        payload: &[u8],
        decl: Window,
        mem: Window,
    ) -> Result<(), u16> {
        // 闸 0：能力环（判据"环越权即拒"）。
        if ring > need {
            self.log(EvKind::RingDeny, ring.0)?;
            return Err(XC_RING_DENIED);
        }
        // 闸 1：魔数。
        if payload.is_empty() || payload[0] != ENTRY_MAGIC {
            self.log(EvKind::BadMagic, ring.0)?;
            return Err(XC_BAD_MAGIC);
        }
        // 闸 2：长度。
        if payload.len() > PAYLOAD_CAP {
            self.log(EvKind::BadLen, ring.0)?;
            return Err(XC_LEN_EXCEEDED);
        }
        // 闸 3：声明缓冲界内（判据"界内三闸"）。
        let plen = payload.len() as u64;
        if decl.len > plen {
            self.log(EvKind::BadRange, ring.0)?;
            return Err(XC_OUT_OF_RANGE);
        }
        let decl_end = match decl.base.checked_add(decl.len) {
            Some(e) => e,
            None => {
                self.log(EvKind::BadRange, ring.0)?;
                return Err(XC_OUT_OF_RANGE);
            }
        };
        if decl_end > plen {
            self.log(EvKind::BadRange, ring.0)?;
            return Err(XC_OUT_OF_RANGE);
        }
        // 闸 4：内存窗落沙箱内（判据"越界即拒"）。
        if let Err(e) = mem.within(&self.sandbox) {
            self.log(EvKind::SandboxDeny, ring.0)?;
            return Err(e);
        }
        self.passed += 1;
        Ok(())
    }

    /// 类别计数。
    pub fn count(&self, kind: EvKind) -> u64 {
        self.kind_cnt[kind.idx()]
    }

    /// 守恒校验。
    pub fn verify(&self) -> Result<(), u16> {
        let mut sum = 0u64;
        for c in self.kind_cnt.iter() {
            sum += *c;
        }
        if sum != self.events.len() as u64 {
            return Err(XC_LEDGER_DRIFT);
        }
        Ok(())
    }

    /// 审计摘要。
    pub fn audit(&self) -> String {
        let mut s = String::new();
        s.push_str("cgb830[");
        s.push_str(&self.name);
        s.push_str("] events=");
        s.push_str(&self.events.len().to_string());
        s.push_str(" pass=");
        s.push_str(&self.passed.to_string());
        for i in 0..5 {
            s.push_str(&String::from(" k"));
            s.push_str(&i.to_string());
            s.push('=');
            s.push_str(&self.kind_cnt[i].to_string());
        }
        s
    }
}

/// 自例程：构造正/负样本走核心路径；全过返回 0，失败返回错误码。
pub fn drill() -> Result<u32, u16> {
    let sb = Window { base: 0x1000, len: 0x4000 };
    let mut g = Gate::new("w3-drill-830", sb);

    // 负样本 1：环越权——USER(3) 提交需 KERNEL(0)。
    let ok_payload = [ENTRY_MAGIC, 1, 2, 3];
    match g.submit(RING_USER, RING_KERNEL, &ok_payload, Window { base: 0, len: 4 }, Window { base: 0x1000, len: 8 }) {
        Err(code) if code == XC_RING_DENIED => {}
        Ok(_) => return Err(XC_RING_DENIED),
        Err(other) => return Err(other),
    }
    // 负样本 2：魔数不符。
    let bad = [0x5A, 1, 2];
    match g.submit(RING_SERVICE, RING_SERVICE, &bad, Window { base: 0, len: 3 }, Window { base: 0x1000, len: 8 }) {
        Err(code) if code == XC_BAD_MAGIC => {}
        Ok(_) => return Err(XC_BAD_MAGIC),
        Err(other) => return Err(other),
    }
    // 负样本 3：长度越限。
    let long = [ENTRY_MAGIC; PAYLOAD_CAP + 1];
    match g.submit(RING_SERVICE, RING_SERVICE, &long, Window { base: 0, len: 4 }, Window { base: 0x1000, len: 8 }) {
        Err(code) if code == XC_LEN_EXCEEDED => {}
        Ok(_) => return Err(XC_LEN_EXCEEDED),
        Err(other) => return Err(other),
    }
    // 负样本 4：声明缓冲界外。
    match g.submit(RING_SERVICE, RING_SERVICE, &ok_payload, Window { base: 2, len: 8 }, Window { base: 0x1000, len: 8 }) {
        Err(code) if code == XC_OUT_OF_RANGE => {}
        Ok(_) => return Err(XC_OUT_OF_RANGE),
        Err(other) => return Err(other),
    }
    // 负样本 5：沙箱越界（窗尾越过沙箱上界）。
    match g.submit(RING_SERVICE, RING_SERVICE, &ok_payload, Window { base: 0, len: 4 }, Window { base: 0x4FF0, len: 0x100 }) {
        Err(code) if code == XC_SANDBOX_ESCAPE => {}
        Ok(_) => return Err(XC_SANDBOX_ESCAPE),
        Err(other) => return Err(other),
    }
    // 负样本 6：沙箱下界穿越。
    match g.submit(RING_SERVICE, RING_SERVICE, &ok_payload, Window { base: 0, len: 4 }, Window { base: 0x800, len: 0x100 }) {
        Err(code) if code == XC_SANDBOX_ESCAPE => {}
        Ok(_) => return Err(XC_SANDBOX_ESCAPE),
        Err(other) => return Err(other),
    }

    // 正样本：环同级 + 全闸通过 ×2（不同窗口）。
    g.submit(RING_DRIVER, RING_DRIVER, &ok_payload, Window { base: 0, len: 4 }, Window { base: 0x1000, len: 0x10 })?;
    g.submit(RING_KERNEL, RING_DRIVER, &ok_payload, Window { base: 0, len: 4 }, Window { base: 0x2000, len: 0x10 })?;
    // 零长窗应放行（半开区间语义）。
    g.submit(RING_SERVICE, RING_SERVICE, &ok_payload, Window { base: 0, len: 4 }, Window { base: 0x1000, len: 0 })?;

    // 溢出防护：声明窗 base 溢出。
    match g.submit(RING_SERVICE, RING_SERVICE, &ok_payload, Window { base: u64::MAX - 2, len: 8 }, Window { base: 0x1000, len: 8 }) {
        Err(code) if code == XC_OUT_OF_RANGE => {}
        Ok(_) => return Err(XC_OUT_OF_RANGE),
        Err(other) => return Err(other),
    }

    g.verify()?;
    if g.count(EvKind::BadMagic) != 1 || g.count(EvKind::BadLen) != 1 || g.count(EvKind::BadRange) != 2
        || g.count(EvKind::RingDeny) != 1 || g.count(EvKind::SandboxDeny) != 2
    {
        return Err(XC_LEDGER_DRIFT);
    }
    if g.passed != 3 {
        return Err(XC_LEDGER_DRIFT);
    }
    Ok(0)
}
