//! UNX-F50178 B29 批向量账骨架（Ticket 用例集生成器 + 校验和）（0x44260 段）
//!
//! 锚点：UNX-F50178-J1 用例带校验和落账；判据锚定 §AI-63 §5。
//!
//! B29 批的 Ticket 向量不能靠人肉誊抄，也不能靠「生成一次就信一次」。本模块
//! 用**混合基数里程表**（odometer）穷举用例坐标：三个轴（指令类别 / 纸张档 /
//! 边距档）做笛卡尔积，产出的序号天然从 0 连续递增、坐标覆盖天然守恒——这样
//! 「第 37 条到底是什么」可以只凭序号推出来，而不必相信生成器的随机数。
//!
//! 校验和选 CRC-32/ISO-HDLC（多项式 0xEDB88320），覆盖「序号 ‖ 三轴坐标 ‖
//! 载荷」。落账时逐条复算，任何一条内容与指纹脱钩即拒收；账本因此可以离线
//! 复算、跨机器比对。CRC 的参考向量（"123456789" → 0xCBF43926）写进判据，
//! 避免「自己实现和自己对账」的循环论证。
//!
//! 数据结构（两类以上真实结构）：
//! * `GenSpec` + `generate` —— 定参里程表生成器（返回 Result，非法参数不产出）；
//! * `TicketCase` —— 一条用例（序号 / 三轴坐标 / 载荷 / CRC-32）；
//! * `CaseBook` —— 定容账本：序号必须**恰好等于下一条**（断档、倒退分码报错）；
//! * `axis_histogram` —— 轴分布视图，用于「坐标覆盖守恒」判据。
//!
//! 错误路径：段内闭集 `SEG_BASE + n`，全部码值在注入核/判据里被真实构造。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x44260 独占 0x20）。
pub const SEG_BASE: u32 = 0x44260;

pub const E_EMPTY: u16 = 1;
pub const E_SPEC: u16 = 2;
pub const E_DIM: u16 = 3;
pub const E_CRC: u16 = 4;
pub const E_ORDER: u16 = 5;
pub const E_GAP: u16 = 6;
pub const E_PAYLOAD: u16 = 7;
pub const E_FULL: u16 = 8;
pub const E_AXIS: u16 = 9;

/// 向量账错误闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VecError {
    Empty,
    Spec,
    Dim,
    Crc,
    Order,
    Gap,
    Payload,
    Full,
    Axis,
}

impl VecError {
    pub fn code(self) -> u32 {
        let n = match self {
            VecError::Empty => E_EMPTY,
            VecError::Spec => E_SPEC,
            VecError::Dim => E_DIM,
            VecError::Crc => E_CRC,
            VecError::Order => E_ORDER,
            VecError::Gap => E_GAP,
            VecError::Payload => E_PAYLOAD,
            VecError::Full => E_FULL,
            VecError::Axis => E_AXIS,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            VecError::Empty => "generator produced an empty grid",
            VecError::Spec => "generator spec outside legal bounds",
            VecError::Dim => "case coordinate outside axis range",
            VecError::Crc => "CRC-32 does not match case content",
            VecError::Order => "sequence number went backwards",
            VecError::Gap => "sequence number skipped ahead",
            VecError::Payload => "payload length outside 1..=32",
            VecError::Full => "case book capacity reached",
            VecError::Axis => "axis index outside 0..3",
        }
    }
}

/// 单轴最大档数。4 档 × 4 档 × 4 档 = 64 条，正好是账本上限。
pub const MAX_DIM: u8 = 4;
pub const MAX_CASES: usize = 64;
pub const MAX_PAYLOAD: usize = 32;
/// CRC-32/ISO-HDLC 的多项式（反射形式）。
const CRC_POLY: u32 = 0xEDB8_8320;

/// 生成参数：三轴档数 + 载荷长度 + 盐。字段全部有界，生成前一次性校验。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GenSpec {
    pub dims: [u8; 3],
    pub payload_len: u8,
    pub salt: u8,
}

impl GenSpec {
    pub const fn new(dims: [u8; 3], payload_len: u8, salt: u8) -> GenSpec {
        GenSpec {
            dims,
            payload_len,
            salt,
        }
    }

    /// 参数校验。轴档数 0 与超界分开报：前者是「没有用例可生成」，
    /// 后者是「参数写错」，上层处置不同（补批次 vs 修配置）。
    pub fn validate(&self) -> Result<(), VecError> {
        let mut i = 0usize;
        let mut total = 1usize;
        while i < 3 {
            let d = match self.dims.get(i) {
                Some(v) => *v,
                None => return Err(VecError::Spec),
            };
            if d == 0 {
                return Err(VecError::Empty);
            }
            if d > MAX_DIM {
                return Err(VecError::Spec);
            }
            total *= d as usize;
            i += 1;
        }
        if total == 0 || total > MAX_CASES {
            return Err(VecError::Spec);
        }
        if self.payload_len == 0 || self.payload_len as usize > MAX_PAYLOAD {
            return Err(VecError::Spec);
        }
        Ok(())
    }

    /// 用例总数（调用方在 validate 之后使用；未 validate 也可能算出乘积，
    /// 但只有 validate 通过的值才有意义）。
    pub fn cardinality(&self) -> usize {
        self.dims[0] as usize * self.dims[1] as usize * self.dims[2] as usize
    }
}

/// 一条 Ticket 用例。`crc` 是内容指纹，落账后不可与内容脱钩。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TicketCase {
    pub seq: u32,
    pub dims: [u8; 3],
    pub payload: Vec<u8>,
    pub crc: u32,
}

/// CRC-32/ISO-HDLC：初值 0xFFFFFFFF、反射、末尾取反。
/// 逐位实现（不建查表）是为了让内核里只有 4 字节表项开销，速度对 64 条用例足够。
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    let mut i = 0usize;
    while i < data.len() {
        if let Some(b) = data.get(i) {
            crc ^= *b as u32;
            let mut k = 0usize;
            while k < 8 {
                if crc & 1 == 1 {
                    crc = (crc >> 1) ^ CRC_POLY;
                } else {
                    crc >>= 1;
                }
                k += 1;
            }
        }
        i += 1;
    }
    crc ^ 0xFFFF_FFFF
}

/// 用例指纹：序号（LE）‖ 三轴坐标 ‖ 载荷，按固定顺序喂给 CRC-32。
///
/// 把序号与坐标一起纳入指纹是刻意的：载荷相同但坐标不同的两条用例必须
/// 得到不同指纹，否则「把 (1,0,0) 的载荷挪到 (2,0,0)」这类篡改查不出来。
pub fn case_crc(seq: u32, dims: [u8; 3], payload: &[u8]) -> u32 {
    let mut buf: Vec<u8> = Vec::with_capacity(4 + 3 + payload.len());
    let mut i = 0usize;
    while i < 4 {
        buf.push((seq >> (i * 8)) as u8);
        i += 1;
    }
    let mut k = 0usize;
    while k < 3 {
        if let Some(d) = dims.get(k) {
            buf.push(*d);
        }
        k += 1;
    }
    let mut j = 0usize;
    while j < payload.len() {
        if let Some(b) = payload.get(j) {
            buf.push(*b);
        }
        j += 1;
    }
    crc32(&buf)
}

/// 里程表生成：轴 2 变化最快、轴 0 最慢，同 spec 逐位可复现。
pub fn generate(spec: &GenSpec) -> Result<Vec<TicketCase>, VecError> {
    spec.validate()?;
    let d0 = spec.dims[0] as usize;
    let d1 = spec.dims[1] as usize;
    let d2 = spec.dims[2] as usize;
    let mut out = Vec::with_capacity(d0 * d1 * d2);
    let mut a = 0usize;
    while a < d0 {
        let mut b = 0usize;
        while b < d1 {
            let mut c = 0usize;
            while c < d2 {
                let seq = out.len() as u32;
                let dims = [a as u8, b as u8, c as u8];
                let mut payload = Vec::with_capacity(spec.payload_len as usize);
                let mut t = 0usize;
                while t < spec.payload_len as usize {
                    // 坐标、序号、盐与位置各占一个互质乘子，避免不同坐标
                    // 在短载荷上撞出完全相同的字节串。
                    let v = spec
                        .salt
                        .wrapping_add(seq as u8)
                        .wrapping_add(dims[0].wrapping_mul(3))
                        .wrapping_add(dims[1].wrapping_mul(5))
                        .wrapping_add(dims[2].wrapping_mul(7))
                        .wrapping_add((t as u8).wrapping_mul(11));
                    payload.push(v);
                    t += 1;
                }
                let crc = case_crc(seq, dims, &payload);
                out.push(TicketCase {
                    seq,
                    dims,
                    payload,
                    crc,
                });
                c += 1;
            }
            b += 1;
        }
        a += 1;
    }
    Ok(out)
}

/// 定容用例账本。序号必须**恰好等于下一条**：断档与倒退分属两种错误，
/// 合并成一句「顺序错」会让离线复算无法判断缺的是哪一段。
pub struct CaseBook {
    cases: Vec<TicketCase>,
    cap: usize,
}

impl CaseBook {
    pub fn new(cap: usize) -> Result<CaseBook, VecError> {
        if cap == 0 || cap > MAX_CASES {
            return Err(VecError::Spec);
        }
        Ok(CaseBook {
            cases: Vec::with_capacity(cap),
            cap,
        })
    }

    pub fn len(&self) -> usize {
        self.cases.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.cap
    }

    /// 下一条应接收的序号。
    pub fn next_seq(&self) -> u32 {
        self.cases.len() as u32
    }

    /// 落账。校验顺序：容量 → 载荷界 → 坐标界 → 序号 → CRC。
    ///
    /// 结构性错误（容量/长度/坐标/序号）先于 CRC：它们是廉价整数判断，
    /// 把 CRC 放最后，灌坏样本时不至于为一条注定被拒的用例白算一遍指纹。
    pub fn append(&mut self, c: TicketCase) -> Result<(), VecError> {
        if self.cases.len() >= self.cap {
            return Err(VecError::Full);
        }
        if c.payload.is_empty() || c.payload.len() > MAX_PAYLOAD {
            return Err(VecError::Payload);
        }
        let mut i = 0usize;
        while i < 3 {
            let d = match c.dims.get(i) {
                Some(v) => *v,
                None => return Err(VecError::Dim),
            };
            if d >= MAX_DIM {
                return Err(VecError::Dim);
            }
            i += 1;
        }
        let expected = self.next_seq();
        if c.seq < expected {
            return Err(VecError::Order);
        }
        if c.seq > expected {
            return Err(VecError::Gap);
        }
        if case_crc(c.seq, c.dims, &c.payload) != c.crc {
            return Err(VecError::Crc);
        }
        self.cases.push(c);
        Ok(())
    }

    /// 批量接收；中途失败即停在出错的那一条（不吞错、不续写）。
    pub fn adopt(&mut self, cases: &[TicketCase]) -> Result<usize, VecError> {
        let mut n = 0usize;
        while n < cases.len() {
            match cases.get(n) {
                Some(c) => {
                    self.append(c.clone())?;
                    n += 1;
                }
                None => break,
            }
        }
        Ok(n)
    }

    /// 整账复算：任何一条内容与 CRC 脱钩即报 Crc。
    pub fn verify(&self) -> Result<(), VecError> {
        let mut i = 0usize;
        while i < self.cases.len() {
            if let Some(c) = self.cases.get(i) {
                if case_crc(c.seq, c.dims, &c.payload) != c.crc {
                    return Err(VecError::Crc);
                }
                if c.seq != i as u32 {
                    return Err(VecError::Order);
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 轴分布：[MAX_DIM] 计数。用于「每档出现 total/d 次」的守恒判据。
    pub fn axis_histogram(&self, axis: usize) -> Result<[u32; MAX_DIM as usize], VecError> {
        if axis >= 3 {
            return Err(VecError::Axis);
        }
        let mut out = [0u32; MAX_DIM as usize];
        let mut i = 0usize;
        while i < self.cases.len() {
            if let Some(c) = self.cases.get(i) {
                if let Some(v) = c.dims.get(axis) {
                    if let Some(slot) = out.get_mut(*v as usize) {
                        *slot += 1;
                    }
                }
            }
            i += 1;
        }
        Ok(out)
    }

    /// 账本摘要：序号 ‖ CRC ‖ 坐标，按落账序累积。用于「重放一致」。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ self.cases.len() as u64;
        let mut i = 0usize;
        while i < self.cases.len() {
            if let Some(c) = self.cases.get(i) {
                h = fnv_u8(h, (c.seq >> 8) as u8);
                h = fnv_u8(h, c.seq as u8);
                let mut k = 0usize;
                while k < 3 {
                    if let Some(d) = c.dims.get(k) {
                        h = fnv_u8(h, *d);
                    }
                    k += 1;
                }
                h ^= c.crc as u64;
                h = h.wrapping_mul(0x100_0000_01b3);
            }
            i += 1;
        }
        h
    }

    /// 就地篡改第 idx 条的载荷首字节（注入核用；返回是否真的改到）。
    pub fn tamper_payload(&mut self, idx: usize) -> bool {
        match self.cases.get_mut(idx) {
            Some(c) => match c.payload.get_mut(0) {
                Some(b) => {
                    *b ^= 0x5a;
                    true
                }
                None => false,
            },
            None => false,
        }
    }

    /// 诊断摘要文本。
    pub fn render(&self) -> String {
        format!("cases={}/{} next={}", self.cases.len(), self.cap, self.next_seq())
    }
}

fn fnv_u8(mut h: u64, b: u8) -> u64 {
    h ^= b as u64;
    h.wrapping_mul(0x100_0000_01b3)
}

// ---------------------------------------------------------------------------
// 注入核：八类坏向量，逐类断言专属诊断码
// ---------------------------------------------------------------------------

pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 8usize;
    let spec = GenSpec::new([2, 2, 2], 8, 3);
    let good = match generate(&spec) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    let first = match good.first() {
        Some(c) => c.clone(),
        None => return (caught, total),
    };

    // 1 CRC 被篡改
    let mut l1 = match CaseBook::new(8) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    let mut bad = first.clone();
    bad.crc ^= 1;
    if l1.append(bad).err() == Some(VecError::Crc) {
        caught += 1;
    }

    // 2 序号断档（首条就写 2，跳过 0/1）
    let mut l2 = match CaseBook::new(8) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    let mut gap = first.clone();
    gap.seq = 2;
    gap.crc = case_crc(2, gap.dims, &gap.payload);
    if l2.append(gap).err() == Some(VecError::Gap) {
        caught += 1;
    }

    // 3 序号倒退（重复首条）
    let mut l3 = match CaseBook::new(8) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    if l3.append(first.clone()).is_ok() && l3.append(first.clone()).err() == Some(VecError::Order) {
        caught += 1;
    }

    // 4 坐标越界
    let mut l4 = match CaseBook::new(8) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    let mut dimbad = first.clone();
    dimbad.dims = [9, 0, 0];
    dimbad.crc = case_crc(dimbad.seq, dimbad.dims, &dimbad.payload);
    if l4.append(dimbad).err() == Some(VecError::Dim) {
        caught += 1;
    }

    // 5 载荷超长
    let mut l5 = match CaseBook::new(8) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    let mut long = Vec::with_capacity(MAX_PAYLOAD + 1);
    let mut i = 0usize;
    while i <= MAX_PAYLOAD {
        long.push(i as u8);
        i += 1;
    }
    let bad5 = TicketCase {
        seq: 0,
        dims: [0, 0, 0],
        payload: long,
        crc: 0,
    };
    if l5.append(bad5).err() == Some(VecError::Payload) {
        caught += 1;
    }

    // 6 账本容量满
    let mut l6 = match CaseBook::new(2) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    if let (Some(c0), Some(c1), Some(c2)) = (good.get(0), good.get(1), good.get(2)) {
        if l6.append(c0.clone()).is_ok() && l6.append(c1.clone()).is_ok() {
            if l6.append(c2.clone()).err() == Some(VecError::Full) {
                caught += 1;
            }
        }
    }

    // 7 空网格（某轴 0 档）
    if generate(&GenSpec::new([0, 2, 2], 8, 0)).err() == Some(VecError::Empty) {
        caught += 1;
    }

    // 8 参数超界（轴档数 > MAX_DIM）
    if generate(&GenSpec::new([MAX_DIM + 1, 1, 1], 8, 0)).err() == Some(VecError::Spec) {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50178_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50178");

    // J1-1 CRC 参考向量（Sharma 无关，用的是 CRC 标准检验串 "123456789"）
    let check = b"123456789";
    let ok = crc32(check) == 0xCBF4_3926;
    let det = format!("crc={:08x}", crc32(check));
    s.add_dyn("50178-J1-CRC参考", ok, &det);

    // J1-2 CRC 空输入为 0（初值取反自洽）
    let ok = crc32(&[]) == 0;
    s.add("50178-J1-CRC空值", ok, if ok { "" } else { "空输入 CRC 非 0" });

    // J1-3 CRC 单字节敏感：翻转任一字节，指纹必变
    let mut ok = true;
    let mut p = Vec::new();
    let mut i = 0usize;
    while i < 16 {
        p.push((i as u8).wrapping_mul(37));
        i += 1;
    }
    let base = crc32(&p);
    let mut k = 0usize;
    while k < p.len() {
        let mut q = Vec::new();
        let mut j = 0usize;
        while j < p.len() {
            if let Some(b) = p.get(j) {
                q.push(if j == k { b ^ 0x01 } else { *b });
            }
            j += 1;
        }
        if crc32(&q) == base {
            ok = false;
        }
        k += 1;
    }
    s.add("50178-J1-CRC敏感", ok, if ok { "" } else { "单比特翻转 CRC 不变" });

    // J1-4 生成确定性：同 spec 两次逐位一致
    let spec = GenSpec::new([3, 2, 4], 12, 7);
    let a = generate(&spec);
    let b = generate(&spec);
    let ok = match (&a, &b) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    };
    s.add("50178-J1-生成确定", ok, if ok { "" } else { "同 spec 产出漂移" });

    let cases = match a {
        Ok(v) => v,
        Err(_) => {
            s.add("50178-J1-生成可用", false, "合法 spec 被拒");
            return s;
        }
    };

    // J1-5 基数守恒：条数 == 三轴档数之积
    let ok = cases.len() == spec.cardinality() && cases.len() == 24;
    let det = format!("n={} card={}", cases.len(), spec.cardinality());
    s.add_dyn("50178-J1-基数守恒", ok, &det);

    // J1-6 坐标覆盖守恒：每轴每档恰好出现 total/d 次
    let mut ok = true;
    let mut det = String::new();
    let mut axis = 0usize;
    while axis < 3 {
        let d = match spec.dims.get(axis) {
            Some(v) => *v as u32,
            None => 0,
        };
        let mut hist = [0u32; MAX_DIM as usize];
        let mut i2 = 0usize;
        while i2 < cases.len() {
            if let Some(c) = cases.get(i2) {
                if let Some(v) = c.dims.get(axis) {
                    if let Some(slot) = hist.get_mut(*v as usize) {
                        *slot += 1;
                    }
                }
            }
            i2 += 1;
        }
        let want = cases.len() as u32 / d;
        let mut v = 0usize;
        while v < d as usize {
            match hist.get(v) {
                Some(x) => {
                    if *x != want {
                        ok = false;
                    }
                    det.push(if *x == want { '1' } else { '0' });
                }
                None => ok = false,
            }
            v += 1;
        }
        axis += 1;
    }
    s.add_dyn("50178-J1-覆盖守恒", ok, &det);

    // J1-7 序号从 0 连续无洞
    let mut ok = true;
    let mut i = 0usize;
    while i < cases.len() {
        match cases.get(i) {
            Some(c) => {
                if c.seq != i as u32 {
                    ok = false;
                }
            }
            None => ok = false,
        }
        i += 1;
    }
    s.add("50178-J1-序号连续", ok, if ok { "" } else { "序号有洞" });

    // J1-8 每条指纹自洽
    let mut ok = true;
    let mut j = 0usize;
    while j < cases.len() {
        if let Some(c) = cases.get(j) {
            if case_crc(c.seq, c.dims, &c.payload) != c.crc {
                ok = false;
            }
        }
        j += 1;
    }
    s.add("50178-J1-指纹自洽", ok, if ok { "" } else { "生成即不自洽" });

    // J1-9 落账守恒 + 整账复算
    let mut book = match CaseBook::new(MAX_CASES) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let adopted = book.adopt(&cases).unwrap_or(0);
    let ok = adopted == cases.len()
        && book.len() == cases.len()
        && book.next_seq() == cases.len() as u32
        && book.verify().is_ok();
    let det = format!("adopted={} len={}", adopted, book.len());
    s.add_dyn("50178-J1-落账守恒", ok, &det);

    // J1-10 重放一致：独立生成再落账，摘要逐位相同
    let again = match generate(&spec) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let mut book2 = match CaseBook::new(MAX_CASES) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let _ = book2.adopt(&again);
    let ok = book.digest() == book2.digest() && book.digest() == book.digest();
    let det = format!("{:x}", book.digest());
    s.add_dyn("50178-J1-重放一致", ok, &det);

    // J1-11 篡改必被发现：就地改载荷后 verify 报 Crc
    let mut t = match CaseBook::new(MAX_CASES) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let _ = t.adopt(&cases);
    let touched = t.tamper_payload(5);
    let ok = touched && t.verify().err() == Some(VecError::Crc);
    s.add("50178-J1-篡改必现", ok, if ok { "" } else { "篡改未被发现" });

    // J1-12 序号断档必拒（Gap）
    let mut g = match CaseBook::new(8) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let mut skip = match cases.get(3) {
        Some(c) => c.clone(),
        None => return s,
    };
    skip.seq = 9;
    skip.crc = case_crc(9, skip.dims, &skip.payload);
    let ok = g.append(skip).err() == Some(VecError::Gap);
    s.add("50178-J1-断档必拒", ok, if ok { "" } else { "跳号被接受" });

    // J1-13 序号倒退/重复必拒（Order）
    let mut o = match CaseBook::new(8) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let mut ok = false;
    if let Some(c0) = cases.first() {
        if o.append(c0.clone()).is_ok() {
            ok = o.append(c0.clone()).err() == Some(VecError::Order);
        }
    }
    s.add("50178-J1-倒退必拒", ok, if ok { "" } else { "倒退序号被接受" });

    // J1-14 坐标越界必拒（Dim）
    let mut d = match CaseBook::new(8) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let mut dimbad = match cases.first() {
        Some(c) => c.clone(),
        None => return s,
    };
    dimbad.dims = [1, MAX_DIM, 0];
    dimbad.crc = case_crc(dimbad.seq, dimbad.dims, &dimbad.payload);
    let ok = d.append(dimbad).err() == Some(VecError::Dim);
    s.add("50178-J1-坐标越界", ok, if ok { "" } else { "越界坐标被接受" });

    // J1-15 载荷长度越界必拒（Payload）
    let mut pl = match CaseBook::new(8) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let mut long = Vec::with_capacity(MAX_PAYLOAD + 2);
    let mut i3 = 0usize;
    while i3 <= MAX_PAYLOAD + 1 {
        long.push(0u8);
        i3 += 1;
    }
    let bad = TicketCase {
        seq: 0,
        dims: [0, 0, 0],
        payload: long,
        crc: 0,
    };
    let ok = pl.append(bad).err() == Some(VecError::Payload);
    s.add("50178-J1-载荷越界", ok, if ok { "" } else { "超长载荷被接受" });

    // J1-16 容量满必拒（Full）
    let mut f = match CaseBook::new(1) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let mut ok = false;
    if let (Some(c0), Some(c1)) = (cases.get(0), cases.get(1)) {
        if f.append(c0.clone()).is_ok() {
            ok = f.append(c1.clone()).err() == Some(VecError::Full);
        }
    }
    s.add("50178-J1-容量满拒", ok, if ok { "" } else { "超容被静默吞" });

    // J1-17 非法生成参数闭集
    let ok = generate(&GenSpec::new([0, 2, 2], 8, 0)).err() == Some(VecError::Empty)
        && generate(&GenSpec::new([MAX_DIM + 1, 1, 1], 8, 0)).err() == Some(VecError::Spec)
        && generate(&GenSpec::new([2, 2, 2], 0, 0)).err() == Some(VecError::Spec)
        && generate(&GenSpec::new([2, 2, 2], (MAX_PAYLOAD + 1) as u8, 0)).err()
            == Some(VecError::Spec);
    s.add("50178-J1-参数闭集", ok, if ok { "" } else { "非法 spec 被放行" });

    // J1-18 轴索引越界必拒（Axis）
    let ok = book.axis_histogram(3).err() == Some(VecError::Axis)
        && book.axis_histogram(0).is_ok()
        && book.axis_histogram(0).ok() == book.axis_histogram(0).ok();
    s.add("50178-J1-轴索引闭集", ok, if ok { "" } else { "轴越界被接受" });

    // J1-19 注入核：八类坏向量全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50178-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50178_checks_green() {
        let set = run_unx50178_checks();
        assert!(set.all_passed(), "UNX-F50178 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50178_crc_reference() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(&[]), 0);
    }

    #[test]
    fn unx50178_book_accepts_generated() {
        let spec = GenSpec::new([2, 3, 2], 10, 1);
        let cases = generate(&spec);
        assert!(cases.is_ok());
        if let Ok(v) = cases {
            let mut book = CaseBook::new(16);
            assert!(book.is_ok());
            if let Ok(b) = book.as_mut() {
                assert_eq!(b.adopt(&v).ok(), Some(v.len()));
                assert!(b.verify().is_ok());
            }
        }
    }
}
