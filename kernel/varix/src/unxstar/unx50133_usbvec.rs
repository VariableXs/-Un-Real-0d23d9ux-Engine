//! UNX-F50133 USB 向量账（传输用例集 + 校验和）（0x43cc0 段）
//!
//! 锚点：UNX-F50133-J1 用例可复跑；判据锚定 §AI-63 §5。
//!
//! 「可复跑」不是「再打印一遍用例清单」，而是：同一条用例在任何时间、任何
//! 顺序、任何机器上重新展开，都必须逐字节一致，并且它的校验和与账本登记的
//! 值相符。因此本模块把用例做成**自展开**的：向量只记 (id, 档位, 长度, 种子)，
//! 载荷由固定 LCG 现场展开；校验和覆盖「头部 + 载荷」，账本登记一份期望值，
//! 复跑时重算并逐条对表。任何一条对不上，`mismatched` 都会如实计数——不静默、
//! 不取平均、不「大部分通过所以算过」。
//!
//! 五档用例与 USB 打印真实传输一一对应：
//! * `ctrl-out` —— 类请求（软复位 / 取端口状态 / 选择接口）；
//! * `ctrl-in`  —— 类请求回读（1284 状态字节）；
//! * `bulk-out` —— 打印数据主通道；
//! * `bulk-in`  —— 反向通道（缺纸/卡纸事件）；
//! * `clear-halt` —— 端点停后的恢复序列。
//! 用例集必须**五档齐备**：缺任一档，账本到底验过什么就说不清了。
//!
//! 数据结构：
//! * `VectorKind` —— 五档用例枚举（码值 1..=5 写死）；
//! * `Vector` —— 定长用例记录（含登记校验和）；
//! * `VectorLedger` —— 定容账本（重复 id 必拒、摘要与登记序无关）；
//! * `ReplayReport` —— 复跑报告（轮数、命中/失配、字节、逐轮摘要、稳定性）；
//! * `parse_manifest` / `render_manifest` —— 文本清单双向编解码（可落盘复盘）。
//!
//! 错误路径：`VecError::code()` 返回 `SEG_BASE + n`，段内闭集，全部码值都在
//! 判据或注入核里被真实构造。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43cc0 独占 0x20）。
pub const SEG_BASE: u32 = 0x43cc0;

pub const E_EMPTY: u16 = 1;
pub const E_CAPACITY: u16 = 2;
pub const E_DUP_ID: u16 = 3;
pub const E_UNKNOWN: u16 = 4;
pub const E_RANGE: u16 = 5;
pub const E_KIND: u16 = 6;
pub const E_SYNTAX: u16 = 7;
pub const E_ROUNDS: u16 = 8;
pub const E_MISMATCH: u16 = 9;
pub const E_KIND_GAP: u16 = 10;
pub const E_BAD_HEX: u16 = 11;

/// 单条用例载荷上限（512 字节 = 高速 bulk 单包；更大的用例应由分块账拆分）。
pub const MAX_LEN: u16 = 512;

/// 账本容量。五档 × 六条回归用例 + 余量。
pub const MAX_VECTORS: usize = 32;

/// 复跑轮数上限。8 轮足以暴露任何依赖「上一轮残留状态」的伪确定性。
pub const MAX_ROUNDS: u8 = 8;

/// 向量账错误闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VecError {
    Empty,
    Capacity,
    DupId,
    Unknown,
    Range,
    Kind,
    Syntax,
    Rounds,
    Mismatch,
    KindGap,
    BadHex,
}

impl VecError {
    /// 诊断码：段基址 + 段内序号（闭集，全仓唯一）。
    pub fn code(self) -> u32 {
        let n = match self {
            VecError::Empty => E_EMPTY,
            VecError::Capacity => E_CAPACITY,
            VecError::DupId => E_DUP_ID,
            VecError::Unknown => E_UNKNOWN,
            VecError::Range => E_RANGE,
            VecError::Kind => E_KIND,
            VecError::Syntax => E_SYNTAX,
            VecError::Rounds => E_ROUNDS,
            VecError::Mismatch => E_MISMATCH,
            VecError::KindGap => E_KIND_GAP,
            VecError::BadHex => E_BAD_HEX,
        };
        SEG_BASE + n as u32
    }

    /// 一行可读原因（诊断/日志共用）。
    pub fn message(self) -> &'static str {
        match self {
            VecError::Empty => "empty ledger or empty manifest",
            VecError::Capacity => "vector ledger full",
            VecError::DupId => "vector id already declared",
            VecError::Unknown => "vector id not in ledger",
            VecError::Range => "id/length/seed outside legal range",
            VecError::Kind => "unknown transfer kind label",
            VecError::Syntax => "manifest line malformed",
            VecError::Rounds => "replay rounds outside 1..=8",
            VecError::Mismatch => "stored checksum differs from replayed bytes",
            VecError::KindGap => "vector set misses a transfer kind",
            VecError::BadHex => "checksum field is not hex",
        }
    }
}

/// 传输用例五档。码值写死：落盘清单与日志都按它解释。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VectorKind {
    CtrlOut,
    CtrlIn,
    BulkOut,
    BulkIn,
    ClearHalt,
}

impl VectorKind {
    pub const ALL: [VectorKind; 5] = [
        VectorKind::CtrlOut,
        VectorKind::CtrlIn,
        VectorKind::BulkOut,
        VectorKind::BulkIn,
        VectorKind::ClearHalt,
    ];

    pub fn code(self) -> u8 {
        match self {
            VectorKind::CtrlOut => 1,
            VectorKind::CtrlIn => 2,
            VectorKind::BulkOut => 3,
            VectorKind::BulkIn => 4,
            VectorKind::ClearHalt => 5,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            VectorKind::CtrlOut => "ctrl-out",
            VectorKind::CtrlIn => "ctrl-in",
            VectorKind::BulkOut => "bulk-out",
            VectorKind::BulkIn => "bulk-in",
            VectorKind::ClearHalt => "clear-halt",
        }
    }

    /// 标签 → 档位。只认清单里写死的五个标签，大小写敏感（落盘格式即契约）。
    pub fn from_label(s: &str) -> Result<VectorKind, VecError> {
        if s == "ctrl-out" {
            Ok(VectorKind::CtrlOut)
        } else if s == "ctrl-in" {
            Ok(VectorKind::CtrlIn)
        } else if s == "bulk-out" {
            Ok(VectorKind::BulkOut)
        } else if s == "bulk-in" {
            Ok(VectorKind::BulkIn)
        } else if s == "clear-halt" {
            Ok(VectorKind::ClearHalt)
        } else {
            Err(VecError::Kind)
        }
    }

    pub fn from_code(c: u8) -> Result<VectorKind, VecError> {
        match c {
            1 => Ok(VectorKind::CtrlOut),
            2 => Ok(VectorKind::CtrlIn),
            3 => Ok(VectorKind::BulkOut),
            4 => Ok(VectorKind::BulkIn),
            5 => Ok(VectorKind::ClearHalt),
            _ => Err(VecError::Kind),
        }
    }
}

/// 一条传输用例。`expect` 是登记校验和，复跑时必须与重算值逐位相等。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Vector {
    pub id: u16,
    pub kind: VectorKind,
    pub len: u16,
    pub seed: u8,
    pub expect: u32,
}

fn fnv_u8_32(mut h: u32, b: u8) -> u32 {
    h ^= b as u32;
    h.wrapping_mul(0x0100_0193)
}

fn fnv_u8(mut h: u64, b: u8) -> u64 {
    h ^= b as u64;
    h.wrapping_mul(0x100_0000_01b3)
}

/// 确定性载荷展开（LCG，参数取 Numerical Recipes 的 32 位常量）。
///
/// 为什么不用哈希直接造数据：LCG 的单步可逆、状态只有 32 位，任何人在纸上
/// 都能验证第 n 个字节；用它当「可复跑」的基石，比任何伪随机库都更容易对账。
pub fn emit(v: &Vector) -> Vec<u8> {
    let mut x: u32 = 0x9e37_79b9
        ^ ((v.id as u32) << 16)
        ^ ((v.kind.code() as u32) << 8)
        ^ (v.seed as u32);
    let mut out = Vec::with_capacity(v.len as usize);
    let mut i = 0u16;
    while i < v.len {
        x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        out.push((x >> 24) as u8);
        i += 1;
    }
    out
}

/// 用例校验和：头部四字段 + 展开后的全部载荷字节（FNV-1a 32）。
pub fn checksum(v: &Vector) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    h = fnv_u8_32(h, (v.id & 0xff) as u8);
    h = fnv_u8_32(h, (v.id >> 8) as u8);
    h = fnv_u8_32(h, v.kind.code());
    h = fnv_u8_32(h, (v.len & 0xff) as u8);
    h = fnv_u8_32(h, (v.len >> 8) as u8);
    h = fnv_u8_32(h, v.seed);
    let payload = emit(v);
    let mut i = 0usize;
    while i < payload.len() {
        if let Some(b) = payload.get(i) {
            h = fnv_u8_32(h, *b);
        }
        i += 1;
    }
    h
}

/// 字段合法性（与账本无关的纯校验，解析与声明共用）。
fn validate(id: u16, kind: VectorKind, len: u16, seed: u8) -> Result<(), VecError> {
    if id == 0 {
        // id 0 保留给「清单头」，用例一律从 1 起编号。
        return Err(VecError::Range);
    }
    if len == 0 || len > MAX_LEN {
        return Err(VecError::Range);
    }
    if seed == 0 {
        // 种子 0 会让 LCG 首个输出退化成常量式序列（x=0 的倍数），
        // 载荷失去区分度，登记表会变成「全同用例」——显式拒绝。
        return Err(VecError::Range);
    }
    if VectorKind::from_code(kind.code()).is_err() {
        return Err(VecError::Kind);
    }
    Ok(())
}

/// 复跑报告。`stable` 是「多轮摘要一致且零失配」的合取，供 J1 直接引用。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplayReport {
    pub rounds: u8,
    pub vectors: usize,
    pub matched: u32,
    pub mismatched: u32,
    pub bytes: u64,
    pub digest: u64,
    pub stable: bool,
}

/// 向量账本：定容、id 唯一、摘要与登记序无关。
pub struct VectorLedger {
    vectors: Vec<Vector>,
    cap: usize,
}

impl VectorLedger {
    pub fn new(cap: usize) -> Result<VectorLedger, VecError> {
        if cap == 0 || cap > MAX_VECTORS {
            return Err(VecError::Capacity);
        }
        Ok(VectorLedger {
            vectors: Vec::with_capacity(cap),
            cap,
        })
    }

    pub fn len(&self) -> usize {
        self.vectors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vectors.is_empty()
    }

    pub fn get(&self, i: usize) -> Option<&Vector> {
        self.vectors.get(i)
    }

    /// 按 id 查（id 是账本主键；顺序由 id 决定，与登记次序无关）。
    pub fn find(&self, id: u16) -> Option<&Vector> {
        let mut i = 0usize;
        while i < self.vectors.len() {
            if let Some(v) = self.vectors.get(i) {
                if v.id == id {
                    return Some(v);
                }
            }
            i += 1;
        }
        None
    }

    fn push_checked(&mut self, v: Vector) -> Result<usize, VecError> {
        if self.find(v.id).is_some() {
            return Err(VecError::DupId);
        }
        if self.vectors.len() >= self.cap {
            return Err(VecError::Capacity);
        }
        self.vectors.push(v);
        Ok(self.vectors.len() - 1)
    }

    /// 声明一条用例：校验和由本模块现算，账本因此天然自洽。
    pub fn declare(
        &mut self,
        id: u16,
        kind: VectorKind,
        len: u16,
        seed: u8,
    ) -> Result<usize, VecError> {
        validate(id, kind, len, seed)?;
        let mut v = Vector {
            id,
            kind,
            len,
            seed,
            expect: 0,
        };
        v.expect = checksum(&v);
        self.push_checked(v)
    }

    /// 声明一条**带外部校验和**的用例（清单导入/篡改注入共用）。
    ///
    /// 与 `declare` 的分工：`declare` 自证，`declare_raw` 只做字段校验，
    /// 校验和留待复跑对表——这正是「导入别人的清单」时该有的姿态。
    pub fn declare_raw(
        &mut self,
        id: u16,
        kind: VectorKind,
        len: u16,
        seed: u8,
        expect: u32,
    ) -> Result<usize, VecError> {
        validate(id, kind, len, seed)?;
        self.push_checked(Vector {
            id,
            kind,
            len,
            seed,
            expect,
        })
    }

    /// 改写登记校验和（篡改注入入口；未知 id 显式报错）。
    pub fn replace_expect(&mut self, id: u16, expect: u32) -> Result<(), VecError> {
        let mut i = 0usize;
        while i < self.vectors.len() {
            let hit = match self.vectors.get(i) {
                Some(v) => v.id == id,
                None => false,
            };
            if hit {
                if let Some(v) = self.vectors.get_mut(i) {
                    v.expect = expect;
                }
                return Ok(());
            }
            i += 1;
        }
        Err(VecError::Unknown)
    }

    /// 账本摘要：**可交换**的折叠（逐条校验和做 wrapping_add）。
    ///
    /// 刻意不乘任何与位置有关的权重：用例集是集合而非序列，同一批用例
    /// 换个登记顺序必须得到同一摘要，否则「清单没变」这句话就无从验证。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ self.vectors.len() as u64;
        let mut i = 0usize;
        while i < self.vectors.len() {
            if let Some(v) = self.vectors.get(i) {
                h = h.wrapping_add(checksum(v) as u64);
            }
            i += 1;
        }
        h
    }

    /// 五档齐备检查。缺档返回缺失的档位码（1..=5），全齐返回 0。
    pub fn kind_gap(&self) -> u8 {
        let mut seen = [false; 5];
        let mut i = 0usize;
        while i < self.vectors.len() {
            if let Some(v) = self.vectors.get(i) {
                let c = v.kind.code();
                if c >= 1 && c <= 5 {
                    seen[(c - 1) as usize] = true;
                }
            }
            i += 1;
        }
        let mut k = 0usize;
        while k < 5 {
            if !seen[k] {
                return (k + 1) as u8;
            }
            k += 1;
        }
        0
    }

    /// 按 id 升序取下标序列（插入排序：账本 ≤32 条，小规模上最省）。
    fn sorted_indices(&self) -> Vec<usize> {
        let mut idx = Vec::with_capacity(self.vectors.len());
        let mut i = 0usize;
        while i < self.vectors.len() {
            idx.push(i);
            i += 1;
        }
        let mut a = 1usize;
        while a < idx.len() {
            let mut b = a;
            while b > 0 {
                // 先把「待比较的两个下标」取出来，再比较它们指向的 id；
                // 交换的是下标而不是 id（id 是账本数据，不能被排序改写）。
                let left_pos = match idx.get(b - 1) {
                    Some(v) => *v,
                    None => break,
                };
                let right_pos = match idx.get(b) {
                    Some(v) => *v,
                    None => break,
                };
                let left_id = match self.vectors.get(left_pos) {
                    Some(v) => v.id,
                    None => break,
                };
                let right_id = match self.vectors.get(right_pos) {
                    Some(v) => v.id,
                    None => break,
                };
                if left_id > right_id {
                    // 两次 get_mut 不能同时持有（借用冲突），按「先取后写」分两步。
                    if let Some(x) = idx.get_mut(b - 1) {
                        *x = right_pos;
                    }
                    if let Some(x) = idx.get_mut(b) {
                        *x = left_pos;
                    }
                } else {
                    break;
                }
                b -= 1;
            }
            a += 1;
        }
        idx
    }

    /// 严格自检：任何一条登记校验和与重算值不符即返回 `Mismatch`。
    pub fn verify(&self) -> Result<(), VecError> {
        let mut i = 0usize;
        while i < self.vectors.len() {
            if let Some(v) = self.vectors.get(i) {
                if checksum(v) != v.expect {
                    return Err(VecError::Mismatch);
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 复跑 `rounds` 轮：每轮按 id 升序重算全部校验和，逐条对表。
    ///
    /// 记分规则：`matched` 按「条 × 轮」累计，因此 J1 的分母恒为
    /// `rounds × vectors`，不存在「这轮没过但平均下来还行」的口径。
    pub fn replay(&self, rounds: u8) -> Result<ReplayReport, VecError> {
        if rounds == 0 || rounds > MAX_ROUNDS {
            return Err(VecError::Rounds);
        }
        if self.vectors.is_empty() {
            return Err(VecError::Empty);
        }
        let gap = self.kind_gap();
        if gap != 0 {
            return Err(VecError::KindGap);
        }
        let idx = self.sorted_indices();
        let mut matched: u32 = 0;
        let mut mismatched: u32 = 0;
        let mut bytes: u64 = 0;
        let mut first: u64 = 0;
        let mut stable = true;
        let mut r = 0u8;
        while r < rounds {
            let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ self.vectors.len() as u64;
            let mut k = 0usize;
            while k < idx.len() {
                if let Some(v) = idx.get(k).and_then(|p| self.vectors.get(*p)) {
                    let actual = checksum(v);
                    if actual == v.expect {
                        matched = matched.saturating_add(1);
                    } else {
                        mismatched = mismatched.saturating_add(1);
                    }
                    if r == 0 {
                        bytes = bytes.saturating_add(v.len as u64);
                    }
                    h = fnv_u8(h, (v.id & 0xff) as u8);
                    h = fnv_u8(h, (v.id >> 8) as u8);
                    let mut sh = 0usize;
                    while sh < 4 {
                        h = fnv_u8(h, ((actual >> (8 * sh)) & 0xff) as u8);
                        sh += 1;
                    }
                }
                k += 1;
            }
            if r == 0 {
                first = h;
            } else if h != first {
                stable = false;
            }
            r += 1;
        }
        if mismatched != 0 {
            stable = false;
        }
        Ok(ReplayReport {
            rounds,
            vectors: self.vectors.len(),
            matched,
            mismatched,
            bytes,
            digest: first,
            stable,
        })
    }

    /// 清单渲染：每行 `id kind len seed checksum`（checksum 为 8 位小写十六进制）。
    pub fn render_manifest(&self) -> String {
        let mut out = String::new();
        let mut i = 0usize;
        while i < self.vectors.len() {
            if let Some(v) = self.vectors.get(i) {
                out.push_str(&format!(
                    "{} {} {} {} {:08x}\n",
                    v.id,
                    v.kind.label(),
                    v.len,
                    v.seed,
                    v.expect
                ));
            }
            i += 1;
        }
        out
    }
}

fn dec_u32(tok: &str) -> Result<u32, VecError> {
    if tok.is_empty() {
        return Err(VecError::Syntax);
    }
    let b = tok.as_bytes();
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < b.len() {
        let c = match b.get(i) {
            Some(x) => *x,
            None => return Err(VecError::Syntax),
        };
        if !(0x30..=0x39).contains(&c) {
            return Err(VecError::Syntax);
        }
        v = v * 10 + (c - 0x30) as u64;
        if v > 0xffff_ffff {
            return Err(VecError::Range);
        }
        i += 1;
    }
    Ok(v as u32)
}

/// 十六进制解析（1..=8 位；越界/非十六进制都算坏样本，但错因分开报）。
fn hex_u32(tok: &str) -> Result<u32, VecError> {
    if tok.is_empty() || tok.len() > 8 {
        return Err(VecError::BadHex);
    }
    let b = tok.as_bytes();
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < b.len() {
        let c = match b.get(i) {
            Some(x) => *x,
            None => return Err(VecError::BadHex),
        };
        let d = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => return Err(VecError::BadHex),
        };
        v = (v << 4) | d as u32;
        i += 1;
    }
    Ok(v)
}

/// 解析清单文本（每行 5 字段）。空文本、字段数不符、档位未知、数值越界
/// 各有独立错误码——「清单坏了」这句话必须能定位到行与字段。
pub fn parse_manifest(text: &str) -> Result<VectorLedger, VecError> {
    if text.is_empty() {
        return Err(VecError::Empty);
    }
    let mut led = VectorLedger::new(MAX_VECTORS)?;
    let mut lines = text.lines();
    let mut any = false;
    loop {
        let line = match lines.next() {
            Some(l) => l,
            None => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        any = true;
        let mut it = trimmed.split_whitespace();
        let f0 = it.next();
        let f1 = it.next();
        let f2 = it.next();
        let f3 = it.next();
        let f4 = it.next();
        let extra = it.next();
        if f0.is_none() || f1.is_none() || f2.is_none() || f3.is_none() || f4.is_none() || extra.is_some() {
            return Err(VecError::Syntax);
        }
        let s_id = match f0 {
            Some(x) => x,
            None => return Err(VecError::Syntax),
        };
        let s_kind = match f1 {
            Some(x) => x,
            None => return Err(VecError::Syntax),
        };
        let s_len = match f2 {
            Some(x) => x,
            None => return Err(VecError::Syntax),
        };
        let s_seed = match f3 {
            Some(x) => x,
            None => return Err(VecError::Syntax),
        };
        let s_sum = match f4 {
            Some(x) => x,
            None => return Err(VecError::Syntax),
        };
        let id = dec_u32(s_id)?;
        let kind = VectorKind::from_label(s_kind)?;
        let len = dec_u32(s_len)?;
        let seed = dec_u32(s_seed)?;
        let expect = hex_u32(s_sum)?;
        if id > u16::MAX as u32 || len > u16::MAX as u32 || seed > 255 {
            return Err(VecError::Range);
        }
        led.declare_raw(id as u16, kind, len as u16, seed as u8, expect)?;
    }
    if !any {
        return Err(VecError::Empty);
    }
    Ok(led)
}

// ---------------------------------------------------------------------------
// 基准账本
// ---------------------------------------------------------------------------

/// 基准用例表：(id, 档位, 长度, 种子)。长度按真实传输量级取，
/// 覆盖「1 字节状态回读 → 512 字节满包 bulk」的整个区间。
pub const BASELINE: [(u16, VectorKind, u16, u8); 10] = [    (1, VectorKind::CtrlOut, 8, 0x11),
    (2, VectorKind::CtrlOut, 8, 0x22),
    (3, VectorKind::CtrlIn, 1, 0x33),
    (4, VectorKind::CtrlIn, 16, 0x44),
    (5, VectorKind::BulkOut, 512, 0x55),
    (6, VectorKind::BulkOut, 512, 0x66),
    (7, VectorKind::BulkOut, 256, 0x77),
    (8, VectorKind::BulkIn, 8, 0x88),
    (9, VectorKind::BulkIn, 64, 0x99),
    (10, VectorKind::ClearHalt, 2, 0xaa),
];

/// 基准账本单轮总字节：8+8+1+16+512+512+256+8+64+2 = 1387（由 `BASELINE`
/// 逐项相加得到；判据用它把「字节账」钉在具体数量上，而非只做自比对）。
pub const BASELINE_BYTES: u64 = 1387;

/// 构造基准账本（十条用例、五档齐备）。
pub fn baseline_ledger() -> Result<VectorLedger, VecError> {
    let mut led = VectorLedger::new(MAX_VECTORS)?;
    let mut i = 0usize;
    while i < BASELINE.len() {
        if let Some(e) = BASELINE.get(i) {
            led.declare(e.0, e.1, e.2, e.3)?;
        }
        i += 1;
    }
    Ok(led)
}

// ---------------------------------------------------------------------------
// 注入核：七类坏样本必须全部被抓
// ---------------------------------------------------------------------------

pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 7usize;

    // 1 空清单
    if parse_manifest("").err() == Some(VecError::Empty) {
        caught += 1;
    }
    // 2 字段数不足（语法）
    if parse_manifest("1 bulk-out 512\n").err() == Some(VecError::Syntax) {
        caught += 1;
    }
    // 3 未知档位标签
    if parse_manifest("1 usb-magic 512 7 00000000\n").err() == Some(VecError::Kind) {
        caught += 1;
    }
    // 4 长度越界（0）
    if parse_manifest("1 bulk-out 0 7 00000000\n").err() == Some(VecError::Range) {
        caught += 1;
    }
    // 5 校验和非十六进制
    if parse_manifest("1 bulk-out 8 7 zzzzzzzz\n").err() == Some(VecError::BadHex) {
        caught += 1;
    }
    // 6 重复 id
    let dup = parse_manifest("1 bulk-out 8 7 00000000\n1 bulk-out 8 8 00000000\n");
    if dup.err() == Some(VecError::DupId) {
        caught += 1;
    }
    // 7 缺档（只有 bulk 一类）
    let mut led = match VectorLedger::new(4) {
        Ok(l) => l,
        Err(_) => return (caught, total),
    };
    let a = led.declare(1, VectorKind::BulkOut, 16, 3);
    let b = led.declare(2, VectorKind::BulkOut, 32, 4);
    let gap = if a.is_ok() && b.is_ok() {
        led.replay(1).err()
    } else {
        None
    };
    if gap == Some(VecError::KindGap) {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50133_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50133");

    let led = match baseline_ledger() {
        Ok(l) => l,
        Err(_) => {
            s.add("50133-J1-基准账本", false, "基准账本构建失败");
            return s;
        }
    };

    // J1-1 五档齐备：kind_gap == 0，且档位码全部可反解
    let mut kinds_ok = led.kind_gap() == 0;
    let mut i = 0usize;
    while i < led.len() {
        if let Some(v) = led.get(i) {
            if VectorKind::from_code(v.kind.code()).is_err() {
                kinds_ok = false;
            }
        }
        i += 1;
    }
    let ok = kinds_ok && led.len() == 10;
    let det = format!("n={} gap={}", led.len(), led.kind_gap());
    s.add_dyn("50133-J1-五档齐备", ok, &det);

    // J1-2 复跑确定：同账本两次复跑（各 3 轮）摘要逐位一致
    let r1 = led.replay(3);
    let r2 = led.replay(3);
    let ok = match (r1, r2) {
        (Ok(a), Ok(b)) => a == b && a.digest == b.digest && a.digest != 0,
        _ => false,
    };
    s.add("50133-J1-复跑确定", ok, if ok { "" } else { "两次复跑摘要漂移" });

    // J1-3 多轮一致：matched == rounds × vectors，mismatched == 0，stable
    let rep = match led.replay(MAX_ROUNDS) {
        Ok(r) => r,
        Err(_) => return s,
    };
    let expected = rep.rounds as u32 * rep.vectors as u32;
    let ok = rep.mismatched == 0 && rep.matched == expected && rep.stable;
    let det = format!("matched={}/{} mismatch={} stable={}", rep.matched, expected, rep.mismatched, rep.stable);
    s.add_dyn("50133-J1-多轮一致", ok, &det);

    // J1-4 字节守恒：每条展开长度 == 声明长度；单轮总字节 == Σlen
    let mut sum: u64 = 0;
    let mut len_ok = true;
    let mut k = 0usize;
    while k < led.len() {
        if let Some(v) = led.get(k) {
            let body = emit(v);
            if body.len() != v.len as usize {
                len_ok = false;
            }
            sum += v.len as u64;
        }
        k += 1;
    }
    let ok = len_ok && rep.bytes == sum && sum == BASELINE_BYTES;
    let det = format!("bytes={} sum={}", rep.bytes, sum);
    s.add_dyn("50133-J1-字节守恒", ok, &det);

    // J1-5 校验和往返：declare 自证（登记值 == 重算值），verify 通过
    let ok = led.verify().is_ok();
    s.add("50133-J1-校验和自洽", ok, if ok { "" } else { "自证校验和不符" });

    // J1-6 摘要与登记序无关：倒序登记同一批用例，摘要相同
    let mut rev = match VectorLedger::new(MAX_VECTORS) {
        Ok(l) => l,
        Err(_) => return s,
    };
    let mut m = BASELINE.len();
    let mut built = true;
    while m > 0 {
        m -= 1;
        if let Some(e) = BASELINE.get(m) {
            if rev.declare(e.0, e.1, e.2, e.3).is_err() {
                built = false;
            }
        }
    }
    let ok = built && rev.digest() == led.digest() && rev.len() == led.len();
    s.add("50133-J1-摘要序无关", ok, if ok { "" } else { "摘要依赖登记顺序" });

    // J1-7 唯一性与容量：重复 id 必拒；容量满必拒
    let mut small = match VectorLedger::new(2) {
        Ok(l) => l,
        Err(_) => return s,
    };
    let a = small.declare(1, VectorKind::BulkOut, 8, 1);
    let b = small.declare(1, VectorKind::BulkOut, 8, 2);
    let c = small.declare(2, VectorKind::BulkOut, 8, 3);
    let d = small.declare(3, VectorKind::BulkOut, 8, 4);
    let ok = a.is_ok()
        && b.err() == Some(VecError::DupId)
        && c.is_ok()
        && d.err() == Some(VecError::Capacity)
        && VectorLedger::new(0).err() == Some(VecError::Capacity);
    s.add("50133-J1-唯一与容量", ok, if ok { "" } else { "重复 id 或超容被放行" });

    // J1-8 清单往返：render → parse → 逐条相等且摘要相同
    let text = led.render_manifest();
    let back = parse_manifest(&text);
    let ok = match back {
        Ok(p) => {
            let mut same = p.len() == led.len() && p.digest() == led.digest();
            let mut x = 0usize;
            while x < led.len() {
                if p.get(x) != led.get(x) {
                    same = false;
                }
                x += 1;
            }
            same
        }
        Err(_) => false,
    };
    s.add("50133-J1-清单往返", ok, if ok { "" } else { "清单往返失真" });

    // J1-9 清单语法：字段数不符/多余字段都必拒
    let four = parse_manifest("1 bulk-out 8 7\n");
    let six = parse_manifest("1 bulk-out 8 7 00000000 extra\n");
    let ok = four.err() == Some(VecError::Syntax) && six.err() == Some(VecError::Syntax);
    s.add("50133-J1-清单语法", ok, if ok { "" } else { "字段数错被放行" });

    // J1-10 未知档位：标签与码值两侧都必拒
    let bad_label = parse_manifest("1 bulk-mid 8 7 00000000\n");
    let bad_code = VectorKind::from_code(0);
    let ok = bad_label.err() == Some(VecError::Kind) && bad_code.err() == Some(VecError::Kind);
    s.add("50133-J1-档位闭集", ok, if ok { "" } else { "非法档位被放行" });

    // J1-11 数值越界：id 0 / len 0 / len 超 MAX / seed 0 全部必拒
    let mut probe = match VectorLedger::new(8) {
        Ok(l) => l,
        Err(_) => return s,
    };
    let id0 = probe.declare(0, VectorKind::BulkOut, 8, 1);
    let len0 = probe.declare(1, VectorKind::BulkOut, 0, 1);
    let lenmax = probe.declare(2, VectorKind::BulkOut, MAX_LEN + 1, 1);
    let seed0 = probe.declare(3, VectorKind::BulkOut, 8, 0);
    let ok = id0.err() == Some(VecError::Range)
        && len0.err() == Some(VecError::Range)
        && lenmax.err() == Some(VecError::Range)
        && seed0.err() == Some(VecError::Range)
        && probe.len() == 0;
    s.add("50133-J1-数值越界", ok, if ok { "" } else { "越界用例被收下" });

    // J1-12 篡改必现形：改一条期望校验和 → verify 报 Mismatch，
    //       复跑 matched 少一条、mismatched 恰为一条、stable 为假
    let mut tampered = match baseline_ledger() {
        Ok(l) => l,
        Err(_) => return s,
    };
    let _ = tampered.replace_expect(5, 0xdead_beef);
    let vres = tampered.verify();
    let trep = tampered.replay(1);
    let ok = vres.err() == Some(VecError::Mismatch)
        && tampered.replace_expect(999, 0).err() == Some(VecError::Unknown)
        && match trep {
            Ok(r) => r.mismatched == 1 && r.matched == 9 && !r.stable,
            Err(_) => false,
        };
    let det = format!("verify={:?}", vres.err());
    s.add_dyn("50133-J1-篡改现形", ok, &det);

    // J1-13 轮数边界：0 与 9 都必拒
    let zero = led.replay(0);
    let nine = led.replay(MAX_ROUNDS + 1);
    let ok = zero.err() == Some(VecError::Rounds) && nine.err() == Some(VecError::Rounds);
    s.add("50133-J1-轮数边界", ok, if ok { "" } else { "轮数越界被放行" });

    // J1-14 错误码闭集：11 个码互异且落在段内
    let codes = [
        VecError::Empty,
        VecError::Capacity,
        VecError::DupId,
        VecError::Unknown,
        VecError::Range,
        VecError::Kind,
        VecError::Syntax,
        VecError::Rounds,
        VecError::Mismatch,
        VecError::KindGap,
        VecError::BadHex,
    ];
    let mut distinct = true;
    let mut in_seg = true;
    let mut x = 0usize;
    while x < codes.len() {
        let cx = codes[x].code();
        if cx < SEG_BASE + 1 || cx > SEG_BASE + 11 {
            in_seg = false;
        }
        let mut y = x + 1;
        while y < codes.len() {
            if codes[y].code() == cx {
                distinct = false;
            }
            y += 1;
        }
        x += 1;
    }
    let ok = distinct && in_seg;
    s.add("50133-J1-错误码闭集", ok, if ok { "" } else { "码值重复或越段" });

    // J1-15 注入核：七类坏样本全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50133-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50133_checks_green() {
        let set = run_unx50133_checks();
        assert!(set.all_passed(), "UNX-F50133 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50133_replay_is_itentical_across_rounds() {
        let led = baseline_ledger();
        assert!(led.is_ok());
        if let Ok(l) = led {
            let a = l.replay(4);
            let b = l.replay(4);
            assert!(a.is_ok() && b.is_ok());
            if let (Ok(x), Ok(y)) = (a, b) {
                assert_eq!(x.digest, y.digest);
                assert_eq!(x.mismatched, 0);
                assert_eq!(x.matched, 40);
            }
        }
    }

    #[test]
    fn unx50133_manifest_roundtrip() {
        let led = baseline_ledger();
        assert!(led.is_ok());
        if let Ok(l) = led {
            let text = l.render_manifest();
            let back = parse_manifest(&text);
            assert!(back.is_ok());
            if let Ok(p) = back {
                assert_eq!(p.len(), l.len());
                assert_eq!(p.digest(), l.digest());
            }
        }
    }

    #[test]
    fn unx50133_emit_length_matches_declared() {
        let led = baseline_ledger();
        assert!(led.is_ok());
        if let Ok(l) = led {
            let mut i = 0usize;
            while i < l.len() {
                if let Some(v) = l.get(i) {
                    assert_eq!(emit(v).len(), v.len as usize);
                }
                i += 1;
            }
        }
    }
}
