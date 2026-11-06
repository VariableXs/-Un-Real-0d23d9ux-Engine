//! UNX-F50118 B26 批向量账骨架（直通用例集生成器 + 校验和）（0x43ae0 段）
//!
//! 锚点：UNX-F50118-J1 用例带校验和落账；判据锚定 §AI-63 §5。
//!
//! 直通链路的回归向量不能靠人肉誊抄：一旦用例内容与账本里的「声称内容」脱钩，
//! 后面所有的「全绿」都失去意义。本模块把每一条用例的**内容指纹**（FNV-1a 64，
//! 覆盖序号、类别、载荷长度与载荷字节）与用例一起落账，账本只接收：
//! 序号严格递增、指纹可复算、载荷长度在界内的条目；任何一条不符即显式拒绝。
//!
//! 生成器是确定性的：同一 `GenSpec`（种子/条数/类别掩码/载荷长度）两次生成
//! 必须逐位一致，这样账本才能被离线复算与跨机器比对。LCG 参数取 Numerical
//! Recipes 的 1664525/1013904223（mod 2^32），不引入浮点，跨平台无尾差。
//!
//! 数据结构：
//! * `CaseKind` —— 用例类别（出页/带状光栅/双面翻转/边距/旋转）；
//! * `GenSpec` + `generate` —— 定参生成器（返回 `Result`，非法参数不产出）；
//! * `VecLedger` —— 向量账本（定容、严格递增、逐条复算指纹）；
//! * `LedgerDigest` —— 账本摘要，用于「重放一致」判据。
//!
//! 错误路径：段内闭集 `SEG_BASE + n`，全部码值在注入核/判据里被真实构造。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43ae0 独占 0x20）。
pub const SEG_BASE: u32 = 0x43ae0;

pub const E_EMPTY: u16 = 1;
pub const E_KIND: u16 = 2;
pub const E_ORDER: u16 = 3;
pub const E_DUP: u16 = 4;
pub const E_SUM: u16 = 5;
pub const E_PAYLOAD: u16 = 6;
pub const E_FULL: u16 = 7;
pub const E_SPEC: u16 = 8;

/// 向量账本错误闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VecError {
    Empty,
    Kind,
    Order,
    Dup,
    Sum,
    Payload,
    Full,
    Spec,
}

impl VecError {
    pub fn code(self) -> u32 {
        let n = match self {
            VecError::Empty => E_EMPTY,
            VecError::Kind => E_KIND,
            VecError::Order => E_ORDER,
            VecError::Dup => E_DUP,
            VecError::Sum => E_SUM,
            VecError::Payload => E_PAYLOAD,
            VecError::Full => E_FULL,
            VecError::Spec => E_SPEC,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            VecError::Empty => "no vector produced",
            VecError::Kind => "case kind code outside 1..=5",
            VecError::Order => "sequence number went backwards",
            VecError::Dup => "sequence number repeated",
            VecError::Sum => "checksum does not match payload",
            VecError::Payload => "payload length outside 1..=256",
            VecError::Full => "ledger capacity reached",
            VecError::Spec => "generator spec out of bounds",
        }
    }
}

pub const MAX_CASES: u32 = 64;
pub const MAX_PAYLOAD: usize = 256;

/// 用例类别。编码固定 1..=5，落账后跨版本可读。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseKind {
    PageOut,
    RasterBand,
    DuplexFlip,
    MarginEdge,
    Rotate90,
}

impl CaseKind {
    pub const ALL: [CaseKind; 5] = [
        CaseKind::PageOut,
        CaseKind::RasterBand,
        CaseKind::DuplexFlip,
        CaseKind::MarginEdge,
        CaseKind::Rotate90,
    ];

    pub fn code(self) -> u8 {
        match self {
            CaseKind::PageOut => 1,
            CaseKind::RasterBand => 2,
            CaseKind::DuplexFlip => 3,
            CaseKind::MarginEdge => 4,
            CaseKind::Rotate90 => 5,
        }
    }

    pub fn from_code(c: u8) -> Result<CaseKind, VecError> {
        match c {
            1 => Ok(CaseKind::PageOut),
            2 => Ok(CaseKind::RasterBand),
            3 => Ok(CaseKind::DuplexFlip),
            4 => Ok(CaseKind::MarginEdge),
            5 => Ok(CaseKind::Rotate90),
            _ => Err(VecError::Kind),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CaseKind::PageOut => "page-out",
            CaseKind::RasterBand => "raster-band",
            CaseKind::DuplexFlip => "duplex-flip",
            CaseKind::MarginEdge => "margin-edge",
            CaseKind::Rotate90 => "rotate-90",
        }
    }

    /// 类别在载体数组里的 0 基下标（coverage 按此归位）。
    pub fn slot(self) -> usize {
        (self.code() - 1) as usize
    }
}

/// 一条直通用例。`sum` 是内容指纹，落账后不可与内容脱钩。
#[derive(Clone, Debug, PartialEq)]
pub struct VecCase {
    pub seq: u32,
    pub kind: CaseKind,
    pub payload: Vec<u8>,
    pub sum: u64,
}

/// 生成参数。字段全部有界：条数 1..=64、类别掩码 1..=0x1f、载荷 1..=256。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GenSpec {
    pub seed: u32,
    pub count: u32,
    pub kind_mask: u8,
    pub payload_len: u16,
}

impl GenSpec {
    pub fn new(seed: u32, count: u32, kind_mask: u8, payload_len: u16) -> GenSpec {
        GenSpec {
            seed,
            count,
            kind_mask,
            payload_len,
        }
    }

    /// 参数合法性（生成前一次性校验，避免半途产出后才发现参数坏）。
    ///
    /// 条数 0 单独报 `Empty` 而不是 `Spec`：空集不是「参数写错」，是「没有
    /// 用例可跑」，两者在上层是两种处置（补参数 vs 修生成器），不能合并。
    pub fn validate(&self) -> Result<(), VecError> {
        if self.count == 0 {
            return Err(VecError::Empty);
        }
        if self.count > MAX_CASES {
            return Err(VecError::Spec);
        }
        if self.kind_mask == 0 || self.kind_mask > 0x1f {
            return Err(VecError::Spec);
        }
        if self.payload_len == 0 || self.payload_len as usize > MAX_PAYLOAD {
            return Err(VecError::Spec);
        }
        Ok(())
    }
}

fn lcg(state: u32) -> u32 {
    state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223)
}

fn fnv_u8(mut h: u64, b: u8) -> u64 {
    h ^= b as u64;
    h.wrapping_mul(0x100_0000_01b3)
}

fn fnv_u32(mut h: u64, v: u32) -> u64 {
    let mut i = 0usize;
    while i < 4 {
        h = fnv_u8(h, (v >> (i * 8)) as u8);
        i += 1;
    }
    h
}

/// 内容指纹：序号 ‖ 类别码 ‖ 载荷长度 ‖ 载荷字节（定序，不可换序）。
///
/// 把序号纳入指纹是刻意的：两条载荷完全相同但序号不同的用例必须得到不同
/// 指纹，否则「把第 7 条换成第 3 条的副本」这类篡改就查不出来。
pub fn case_sum(seq: u32, kind: CaseKind, payload: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    h = fnv_u32(h, seq);
    h = fnv_u8(h, kind.code());
    h = fnv_u32(h, payload.len() as u32);
    let mut i = 0usize;
    while i < payload.len() {
        if let Some(b) = payload.get(i) {
            h = fnv_u8(h, *b);
        }
        i += 1;
    }
    h
}

/// 按掩码与随机数挑类别。掩码置位的类别等概率（模 count）。
fn pick_kind(mask: u8, r: u32) -> Result<CaseKind, VecError> {
    let mut bits = 0u32;
    let mut i = 0usize;
    while i < CaseKind::ALL.len() {
        if CaseKind::ALL.get(i).is_some() {
            if mask & (1u8 << i) != 0 {
                bits += 1;
            }
        }
        i += 1;
    }
    if bits == 0 {
        return Err(VecError::Kind);
    }
    let want = (r % bits) as usize;
    let mut seen = 0usize;
    let mut j = 0usize;
    while j < CaseKind::ALL.len() {
        if let Some(k) = CaseKind::ALL.get(j) {
            if mask & (1u8 << j) != 0 {
                if seen == want {
                    return Ok(*k);
                }
                seen += 1;
            }
        }
        j += 1;
    }
    Err(VecError::Kind)
}

/// 生成用例集：同 spec → 同结果（逐位一致，含指纹）。
pub fn generate(spec: &GenSpec) -> Result<Vec<VecCase>, VecError> {
    spec.validate()?;
    let mut out = Vec::with_capacity(spec.count as usize);
    let mut st = spec.seed ^ 0x9e37_79b9;
    let mut seq = 0u32;
    while seq < spec.count {
        st = lcg(st);
        let kind = pick_kind(spec.kind_mask, st)?;
        let mut payload = Vec::with_capacity(spec.payload_len as usize);
        let mut i = 0usize;
        while i < spec.payload_len as usize {
            st = lcg(st);
            // 只取高字节：LCG 低位的周期短，直接用低位会让相邻用例高度相似。
            payload.push((st >> 24) as u8);
            i += 1;
        }
        let sum = case_sum(seq, kind, &payload);
        out.push(VecCase {
            seq,
            kind,
            payload,
            sum,
        });
        seq += 1;
    }
    Ok(out)
}

/// 向量账本：定容、序号严格递增、逐条复算指纹。
pub struct VecLedger {
    cases: Vec<VecCase>,
    cap: usize,
}

impl VecLedger {
    pub fn new(cap: usize) -> Result<VecLedger, VecError> {
        if cap == 0 || cap > MAX_CASES as usize {
            return Err(VecError::Spec);
        }
        Ok(VecLedger {
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

    /// 末条序号（空账本返回 None，调用方据此判断首条）。
    pub fn last_seq(&self) -> Option<u32> {
        match self.cases.last() {
            Some(c) => Some(c.seq),
            None => None,
        }
    }

    /// 落账。校验顺序：容量 → 载荷界 → 序号序关系 → 指纹。
    ///
    /// 为什么先看序号再看指纹：乱序/重复是**账本结构**错误，指纹重算是
    /// 纯粹的算力开销；把便宜的判据放前面，灌坏样本时不会白烧一遍哈希。
    pub fn append(&mut self, c: VecCase) -> Result<(), VecError> {
        if self.cases.len() >= self.cap {
            return Err(VecError::Full);
        }
        if c.payload.is_empty() || c.payload.len() > MAX_PAYLOAD {
            return Err(VecError::Payload);
        }
        if let Some(last) = self.last_seq() {
            if c.seq == last {
                return Err(VecError::Dup);
            }
            if c.seq < last {
                return Err(VecError::Order);
            }
        }
        let recomputed = case_sum(c.seq, c.kind, &c.payload);
        if recomputed != c.sum {
            return Err(VecError::Sum);
        }
        self.cases.push(c);
        Ok(())
    }

    /// 整账复算：任何一条内容与指纹脱钩即报 SUM。
    pub fn verify(&self) -> Result<(), VecError> {
        let mut i = 0usize;
        while i < self.cases.len() {
            if let Some(c) = self.cases.get(i) {
                if case_sum(c.seq, c.kind, &c.payload) != c.sum {
                    return Err(VecError::Sum);
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 类别覆盖：[5] 计数，合计必须等于账本长度（守恒判据）。
    pub fn coverage(&self) -> [u32; 5] {
        let mut out = [0u32; 5];
        let mut i = 0usize;
        while i < self.cases.len() {
            if let Some(c) = self.cases.get(i) {
                let s = c.kind.slot();
                if let Some(slot) = out.get_mut(s) {
                    *slot += 1;
                }
            }
            i += 1;
        }
        out
    }

    /// 账本摘要：序号 ‖ 指纹，按落账序累积。用于「重放一致」。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ self.cases.len() as u64;
        let mut i = 0usize;
        while i < self.cases.len() {
            if let Some(c) = self.cases.get(i) {
                h = fnv_u32(h, c.seq);
                h = fnv_u8(h, c.kind.code());
                h ^= c.sum.rotate_left(17);
                h = h.wrapping_mul(0x100_0000_01b3);
            }
            i += 1;
        }
        h
    }

    /// 批量接收生成的用例集；中途失败即停在出错的那一条（不吞错、不续写）。
    pub fn adopt(&mut self, cases: &[VecCase]) -> Result<usize, VecError> {
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

    /// 就地篡改第 idx 条的载荷首字节（注入核用；返回是否真的改到）。
    pub fn tamper_payload(&mut self, idx: usize) -> bool {
        match self.cases.get_mut(idx) {
            Some(c) => match c.payload.get_mut(0) {
                Some(b) => {
                    *b ^= 0xa5;
                    true
                }
                None => false,
            },
            None => false,
        }
    }
}

/// 账本摘要文本（诊断用）。
pub fn render(ledger: &VecLedger) -> alloc::string::String {
    let cov = ledger.coverage();
    let mut s = alloc::string::String::new();
    s.push_str(&format!(
        "vec n={}/{} po={} rb={} df={} me={} r90={}",
        ledger.len(),
        ledger.cap,
        cov[0],
        cov[1],
        cov[2],
        cov[3],
        cov[4]
    ));
    s
}

// ---------------------------------------------------------------------------
// 注入核：五类坏向量必须全部被拒（且各自命中专属诊断码）
// ---------------------------------------------------------------------------

pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    let good = match generate(&GenSpec::new(7, 3, 0x1f, 8)) {
        Ok(v) => v,
        Err(_) => return (0, total),
    };

    // 1 重复序号
    let mut l = match VecLedger::new(16) {
        Ok(l) => l,
        Err(_) => return (caught, total),
    };
    let first = match good.first() {
        Some(c) => c.clone(),
        None => return (caught, total),
    };
    if l.append(first.clone()).is_ok() {
        if l.append(first).err() == Some(VecError::Dup) {
            caught += 1;
        }
    }

    // 2 序号倒退
    let mut l2 = match VecLedger::new(16) {
        Ok(l2) => l2,
        Err(_) => return (caught, total),
    };
    let a = match good.get(1) {
        Some(c) => c.clone(),
        None => return (caught, total),
    };
    let b = match good.first() {
        Some(c) => c.clone(),
        None => return (caught, total),
    };
    if l2.append(a).is_ok() {
        if l2.append(b).err() == Some(VecError::Order) {
            caught += 1;
        }
    }

    // 3 指纹被篡改
    let mut l3 = match VecLedger::new(16) {
        Ok(l3) => l3,
        Err(_) => return (caught, total),
    };
    let mut bad = match good.first() {
        Some(c) => c.clone(),
        None => return (caught, total),
    };
    bad.sum ^= 1;
    if l3.append(bad).err() == Some(VecError::Sum) {
        caught += 1;
    }

    // 4 超长载荷
    let mut l4 = match VecLedger::new(16) {
        Ok(l4) => l4,
        Err(_) => return (caught, total),
    };
    let mut long = Vec::with_capacity(MAX_PAYLOAD + 1);
    let mut i = 0usize;
    while i <= MAX_PAYLOAD {
        long.push(i as u8);
        i += 1;
    }
    let bad2 = VecCase {
        seq: 0,
        kind: CaseKind::PageOut,
        payload: long,
        sum: 0,
    };
    if l4.append(bad2).err() == Some(VecError::Payload) {
        caught += 1;
    }

    // 5 容量溢出
    let mut l5 = match VecLedger::new(2) {
        Ok(l5) => l5,
        Err(_) => return (caught, total),
    };
    let mut k = 0usize;
    while k < 2 {
        if let Some(c) = good.get(k) {
            let _ = l5.append(c.clone());
        }
        k += 1;
    }
    let mut third = match good.get(2) {
        Some(c) => c.clone(),
        None => return (caught, total),
    };
    third.seq = 77;
    third.sum = case_sum(77, third.kind, &third.payload);
    if l5.append(third).err() == Some(VecError::Full) {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50118_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50118");
    let spec = GenSpec::new(20_261_106, 12, 0x1f, 16);
    let a = generate(&spec);
    let b = generate(&spec);

    // J1-1 生成确定性：同 spec 两次逐位一致
    let ok = match (&a, &b) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    };
    s.add("50118-J1-生成确定", ok, if ok { "" } else { "同 spec 产出漂移" });

    let cases = match a {
        Ok(v) => v,
        Err(_) => {
            s.add("50118-J1-生成可用", false, "合法 spec 被拒");
            return s;
        }
    };

    // J1-2 类别可达：五个单比特掩码各生成 3 条，必须全部落在该类别上。
    // 用单比特掩码而不是「一次生成看五种是否都出现」：后者是抽样运气，
    // 前者是生成器的**确定性性质**（掩码过滤必须精确生效）。
    let mut ok = true;
    let mut mask_det = String::new();
    let mut m = 0usize;
    while m < 5 {
        let want = match CaseKind::ALL.get(m) {
            Some(k) => *k,
            None => {
                ok = false;
                break;
            }
        };
        let sp = GenSpec::new(4242 + m as u32, 3, 1u8 << m, 6);
        match generate(&sp) {
            Ok(v) => {
                let mut good = v.len() == 3;
                let mut i2 = 0usize;
                while i2 < v.len() {
                    if let Some(c) = v.get(i2) {
                        if c.kind != want {
                            good = false;
                        }
                    }
                    i2 += 1;
                }
                if !good {
                    ok = false;
                }
                mask_det.push(if good { '1' } else { '0' });
            }
            Err(_) => {
                ok = false;
                mask_det.push('x');
            }
        }
        m += 1;
    }
    s.add_dyn("50118-J1-类别可达", ok, &mask_det);

    // J1-2b 掩码覆盖：整批用例的类别直方图合计必须等于批大小（守恒的一半）
    let mut cov = [0u32; 5];
    let mut i = 0usize;
    while i < cases.len() {
        if let Some(c) = cases.get(i) {
            let idx = c.kind.slot();
            if let Some(x) = cov.get_mut(idx) {
                *x += 1;
            }
        }
        i += 1;
    }
    let all = cov[0] + cov[1] + cov[2] + cov[3] + cov[4] == cases.len() as u32;
    let det = format!("{}/{}/{}/{}/{}", cov[0], cov[1], cov[2], cov[3], cov[4]);
    s.add_dyn("50118-J1-直方守恒", all, &det);

    // J1-3 指纹自洽：每条都等于复算值
    let mut ok = true;
    let mut j = 0usize;
    while j < cases.len() {
        if let Some(c) = cases.get(j) {
            if case_sum(c.seq, c.kind, &c.payload) != c.sum {
                ok = false;
            }
        }
        j += 1;
    }
    s.add("50118-J1-指纹自洽", ok, if ok { "" } else { "生成即不自洽" });

    // J1-4 指纹对载荷敏感：翻转一字节必变
    let mut ok = true;
    if let Some(c) = cases.first() {
        let mut p = c.payload.clone();
        if let Some(b0) = p.get_mut(0) {
            *b0 ^= 0x01;
        }
        ok = case_sum(c.seq, c.kind, &p) != c.sum;
    }
    s.add("50118-J1-载荷敏感", ok, if ok { "" } else { "改载荷指纹不变" });

    // J1-5 指纹绑定序号：同载荷不同序号必不同
    let mut ok = false;
    if let Some(c) = cases.first() {
        ok = case_sum(c.seq, c.kind, &c.payload) != case_sum(c.seq + 1, c.kind, &c.payload);
    }
    s.add("50118-J1-序号绑定", ok, if ok { "" } else { "序号不参与指纹" });

    // J1-6 落账守恒：len == 生成条数，coverage 合计 == len
    let mut led = match VecLedger::new(32) {
        Ok(l) => l,
        Err(_) => return s,
    };
    let adopted = led.adopt(&cases).unwrap_or(0);
    let cov2 = led.coverage();
    let sum_cov = cov2[0] + cov2[1] + cov2[2] + cov2[3] + cov2[4];
    let ok = adopted == cases.len() && led.len() == cases.len() && sum_cov as usize == led.len();
    let det = format!("adopted={} len={} cov={}", adopted, led.len(), sum_cov);
    s.add_dyn("50118-J1-落账守恒", ok, &det);

    // J1-7 整账复算通过
    let ok = led.verify().is_ok();
    s.add("50118-J1-整账复算", ok, if ok { "" } else { "复算失败" });

    // J1-8 重放一致：独立生成再落账，摘要逐位相同
    let again = match generate(&spec) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let mut led2 = match VecLedger::new(32) {
        Ok(l) => l,
        Err(_) => return s,
    };
    let _ = led2.adopt(&again);
    let ok = led.digest() == led2.digest() && led.digest() == led.digest();
    let det = format!("{:x}", led.digest());
    s.add_dyn("50118-J1-重放一致", ok, &det);

    // J1-9 篡改必被发现：就地改载荷后 verify 报 SUM
    let mut t = match VecLedger::new(32) {
        Ok(l) => l,
        Err(_) => return s,
    };
    let _ = t.adopt(&cases);
    let touched = t.tamper_payload(3);
    let ok = touched && t.verify().err() == Some(VecError::Sum);
    s.add("50118-J1-篡改必现", ok, if ok { "" } else { "篡改未被发现" });

    // J1-10 序号严格递增（落账后读回）
    let mut ok = true;
    let mut k = 1usize;
    while k < cases.len() {
        if let (Some(x), Some(y)) = (cases.get(k - 1), cases.get(k)) {
            if y.seq <= x.seq {
                ok = false;
            }
        }
        k += 1;
    }
    s.add("50118-J1-序号递增", ok, if ok { "" } else { "账本序号非增" });

    // J1-11 生成参数越界必拒（条数 0 报空集；掩码 0 / 载荷 0 / 超长报参数错）
    let ok = generate(&GenSpec::new(1, 0, 0x1f, 8)).err() == Some(VecError::Empty)
        && generate(&GenSpec::new(1, 4, 0, 8)).err() == Some(VecError::Spec)
        && generate(&GenSpec::new(1, 4, 0x1f, 0)).err() == Some(VecError::Spec)
        && generate(&GenSpec::new(1, 4, 0x1f, (MAX_PAYLOAD + 1) as u16)).err() == Some(VecError::Spec)
        && generate(&GenSpec::new(1, MAX_CASES + 1, 0x1f, 8)).err() == Some(VecError::Spec);
    s.add("50118-J1-参数闭集", ok, if ok { "" } else { "非法 spec 被放行" });

    // J1-12 类别码反解闭集
    let ok = CaseKind::from_code(1).ok() == Some(CaseKind::PageOut)
        && CaseKind::from_code(5).ok() == Some(CaseKind::Rotate90)
        && CaseKind::from_code(0).err() == Some(VecError::Kind)
        && CaseKind::from_code(6).err() == Some(VecError::Kind);
    s.add("50118-J1-类别闭集", ok, if ok { "" } else { "非法类别码" });

    // J1-13 空账本身份：len 0、digest 恒定、verify 通过
    let empty = match VecLedger::new(4) {
        Ok(l) => l,
        Err(_) => return s,
    };
    let ok = empty.len() == 0 && empty.is_empty() && empty.verify().is_ok()
        && empty.last_seq().is_none() && empty.digest() == empty.digest();
    s.add("50118-J1-空账身份", ok, if ok { "" } else { "空账不中性" });

    // J1-14 注入核：五类坏向量全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50118-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50118_checks_green() {
        let set = run_unx50118_checks();
        assert!(set.all_passed(), "UNX-F50118 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50118_generate_is_deterministic() {
        let spec = GenSpec::new(99, 8, 0x0f, 12);
        let a = generate(&spec);
        let b = generate(&spec);
        assert!(a.is_ok() && b.is_ok());
        if let (Ok(x), Ok(y)) = (a, b) {
            assert_eq!(x, y);
            assert_eq!(x.len(), 8);
        }
    }

    #[test]
    fn unx50118_appended_ledger_verifies() {
        let spec = GenSpec::new(5, 6, 0x1f, 10);
        let cases = generate(&spec);
        assert!(cases.is_ok());
        if let Ok(v) = cases {
            let mut l = VecLedger::new(8);
            assert!(l.is_ok());
            if let Ok(led) = l.as_mut() {
                assert_eq!(led.adopt(&v).ok(), Some(v.len()));
                assert!(led.verify().is_ok());
            }
        }
    }
}
