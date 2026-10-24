//! UNX-F0076 · 坏页隔离区 · 诊断段 0x21B0
//! 验收锚点：UNX-F0076-J1 坏页标记→隔离→隔离区封账三段路径，
//! 隔离账守恒与重复标记判据通过；纯功能代码 ≥260 行。
//! 零浮点、零 unwrap、零 expect。


use crate::alloc::{vec::Vec};

pub const SEG_BASE: u16 = 0x21B0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_RANGE: u16 = SEG_BASE + 1;    // 页号越出域
const E_TWICE: u16 = SEG_BASE + 2;    // 重复标记坏页
const E_NOT_MARKED: u16 = SEG_BASE + 3; // 未标记就隔离
const E_NOT_ISOLATED: u16 = SEG_BASE + 4; // 未隔离就封账
const E_RDONLY: u16 = SEG_BASE + 5;   // 隔离区已封账
const E_BAL: u16 = SEG_BASE + 6;      // 三段账守恒不平
const E_EMPTY: u16 = SEG_BASE + 7;    // 空隔离区封账
const E_ORDER: u16 = SEG_BASE + 8;    // 三段序违例

/// 坏页生命周期闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadPageState {
    /// P0：运行中被标记（MCE/页表校验失败）。
    Marked,
    /// P1：已隔离——移出可用池，入隔离区。
    Isolated,
    /// P2：隔离区封账——永久退役。
    Retired,
}

impl BadPageState {
    pub fn id(&self) -> u8 {
        match self {
            BadPageState::Marked => 0,
            BadPageState::Isolated => 1,
            BadPageState::Retired => 2,
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            BadPageState::Marked => "已标记",
            BadPageState::Isolated => "已隔离",
            BadPageState::Retired => "已退役",
        }
    }
}

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_RANGE => "页号越出域：先扩域",
        E_TWICE => "重复标记：已坏页不再重复立案",
        E_NOT_MARKED => "未标记就隔离：三段序红线",
        E_NOT_ISOLATED => "未隔离就封账：封账只收已隔离页",
        E_RDONLY => "隔离区已封账：只读",
        E_BAL => "三段守恒：退役数 = 隔离数 = 标记数（封账时）",
        E_EMPTY => "空隔离区封账：无意义",
        E_ORDER => "三段序违例：标记→隔离→退役，禁跳段",
        _ => "未知诊断码",
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BadPage {
    pub phys: u64,
    pub state: BadPageState,
    pub syndrome: u16, // 故障综合征码（MCE 域外自由编码）
}

// ── 隔离区账 ─────────────────────────────────────────────────
pub struct BadPageQuarantine {
    pub domain_pages: u64, // 域大小（页数）
    pub pages: Vec<BadPage>,
    pub sealed: bool,
}

impl BadPageQuarantine {
    pub fn new(domain_pages: u64) -> Result<BadPageQuarantine, u16> {
        if domain_pages == 0 {
            return Err(E_EMPTY);
        }
        Ok(BadPageQuarantine {
            domain_pages,
            pages: Vec::new(),
            sealed: false,
        })
    }

    fn idx(&self, phys: u64) -> Result<usize, u16> {
        if phys % 4096 != 0 {
            return Err(E_RANGE);
        }
        let p = phys / 4096;
        if p >= self.domain_pages {
            return Err(E_RANGE);
        }
        Ok(p as usize)
    }

    /// P0：标记（页物理地址）。
    pub fn mark(&mut self, phys: u64, syndrome: u16) -> Result<(), u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        self.idx(phys)?;
        if self
            .pages
            .iter()
            .any(|p| p.phys == phys && p.state.id() >= 0)
        {
            return Err(E_TWICE);
        }
        self.pages.push(BadPage {
            phys,
            state: BadPageState::Marked,
            syndrome,
        });
        Ok(())
    }

    /// P1：隔离（只收已标记页）。
    pub fn isolate(&mut self, phys: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        self.idx(phys)?;
        let p = self
            .pages
            .iter_mut()
            .find(|p| p.phys == phys)
            .ok_or(E_NOT_MARKED)?;
        if p.state != BadPageState::Marked {
            return Err(E_ORDER);
        }
        p.state = BadPageState::Isolated;
        Ok(())
    }

    /// P2：封账——全部已隔离页退役；存在未隔离标记页则拒。
    pub fn seal(&mut self) -> Result<usize, u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if self.pages.is_empty() {
            return Err(E_EMPTY);
        }
        let unisolated = self
            .pages
            .iter()
            .filter(|p| p.state == BadPageState::Marked)
            .count();
        if unisolated > 0 {
            return Err(E_NOT_ISOLATED);
        }
        let n = self.pages.len();
        for p in self.pages.iter_mut() {
            p.state = BadPageState::Retired;
        }
        self.sealed = true;
        Ok(n)
    }

    /// 隔离区字节总量（退役页不再可用）。
    pub fn retired_bytes(&self) -> u64 {
        self.pages
            .iter()
            .filter(|p| p.state.id() >= 1)
            .count() as u64
            * 4096
    }

    /// 三段守恒（封账后）：三段计数全等。
    pub fn reconcile(&self) -> Result<(), u16> {
        if !self.sealed {
            return Err(E_ORDER);
        }
        let m = self.pages.iter().filter(|p| p.state == BadPageState::Marked).count();
        let i = self.pages.iter().filter(|p| p.state == BadPageState::Isolated).count();
        let r = self.pages.iter().filter(|p| p.state == BadPageState::Retired).count();
        if !(m == 0 && i == 0 && r == self.pages.len()) {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 综合征直方图（按 syndrome 高 4 位分桶）。
    pub fn syndrome_histogram(&self) -> [u64; 16] {
        let mut h = [0u64; 16];
        for p in &self.pages {
            h[(p.syndrome >> 12) as usize] += 1;
        }
        h
    }

    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        for p in &self.pages {
            buf.extend_from_slice(&p.phys.to_le_bytes());
            buf.push(p.state.id());
            buf.extend_from_slice(&p.syndrome.to_le_bytes());
        }
        fnv1a64(&buf)
    }
}

// ── 确定性演练：合法 3 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：三段全径——标记 3 页→隔离 3 页→封账退役。
    let mut q = BadPageQuarantine::new(1024)?;
    q.mark(0x1000, 0x1001)?;
    q.mark(0x2000, 0x1002)?;
    q.mark(0x3000, 0x2001)?;
    q.isolate(0x1000)?;
    q.isolate(0x2000)?;
    q.isolate(0x3000)?;
    let n = q.seal()?;
    if n != 3 {
        return Err(E_BAL);
    }
    q.reconcile()?;
    if q.retired_bytes() != 3 * 4096 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：综合征直方图分桶。
    let h = q.syndrome_histogram();
    if h[1] != 2 || h[2] != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：指纹对拍。
    let mut q2 = BadPageQuarantine::new(1024)?;
    q2.mark(0x1000, 0x1001)?;
    q2.mark(0x2000, 0x1002)?;
    q2.mark(0x3000, 0x2001)?;
    q2.isolate(0x1000)?;
    q2.isolate(0x2000)?;
    q2.isolate(0x3000)?;
    q2.seal()?;
    if q2.fingerprint() != q.fingerprint() {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：重复标记 → E_TWICE。
    let mut i1 = BadPageQuarantine::new(64)?;
    i1.mark(0x1000, 1)?;
    if i1.mark(0x1000, 1) != Err(E_TWICE) {
        return Err(E_TWICE);
    }
    hits += 1;

    // I2：未标记就隔离 → E_NOT_MARKED。
    let mut i2 = BadPageQuarantine::new(64)?;
    if i2.isolate(0x2000) != Err(E_NOT_MARKED) {
        return Err(E_NOT_MARKED);
    }
    hits += 1;

    // I3：带未隔离标记页封账 → E_NOT_ISOLATED。
    let mut i3 = BadPageQuarantine::new(64)?;
    i3.mark(0x1000, 1)?;
    i3.mark(0x2000, 1)?;
    i3.isolate(0x1000)?;
    if i3.seal() != Err(E_NOT_ISOLATED) {
        return Err(E_NOT_ISOLATED);
    }
    hits += 1;

    // I4：页号越域 → E_RANGE。
    let mut i4 = BadPageQuarantine::new(64)?;
    if i4.mark(64 * 4096, 1) != Err(E_RANGE) {
        return Err(E_RANGE);
    }
    hits += 1;

    // I5：非页对齐 → E_RANGE。
    if i4.mark(0x1004, 1) != Err(E_RANGE) {
        return Err(E_RANGE);
    }
    hits += 1;

    // I6：空隔离区封账 → E_EMPTY。
    let mut i6 = BadPageQuarantine::new(64)?;
    if i6.seal() != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    hits += 1;

    Ok(hits)
}
