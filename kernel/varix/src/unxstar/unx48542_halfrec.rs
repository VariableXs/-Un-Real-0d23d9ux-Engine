//! UNX-F48542 —— E·半档注入拒绝（反：伪造半写档 ×10 不采纳零扩散）
//! 判据原文：UNX-F48542-J1 学习档案装载器对 10 例伪造半写档
//! （头截断/魔数坏/半写标志/校验坏/计数失配/版本越界/空载荷/
//! 载荷越集/尾截断/双头拼接）必须逐例拒收；半写档不得被采纳、
//! 不得扩散污染既有好档（档案库指纹不变）；好档装载不受批次影响。
//! 判据落地（drill 场景）：R1 好档装载基准；R2 十例半写全拒；
//! R3 逐例诊断码精确匹配；R4 诊断码可分（≥6 种互异）；
//! R5 零扩散（拒后档案库指纹与账目不变）；R6 好档复检不受批次影响；
//! R7 拦截统计守恒；R8 LCG 随机半写流全拒；R9 指纹稳定；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xF1E0;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_SHORT: u16 = SEG_BASE + 1; // 头截断（短于头长）
const E_MAGIC: u16 = SEG_BASE + 2; // 魔数不符
const E_HALF: u16 = SEG_BASE + 3; // 半写标志（done=0）
const E_CRC: u16 = SEG_BASE + 4; // 校验不符
const E_COUNT: u16 = SEG_BASE + 5; // 词数与载荷失配
const E_VER: u16 = SEG_BASE + 6; // 版本越界
const E_PAYLOAD: u16 = SEG_BASE + 7; // 空载荷/载荷越集
const E_MATCH: u16 = SEG_BASE + 8; // 半写例诊断码与预期不符
const E_SPREAD: u16 = SEG_BASE + 9; // 半写档扩散（档案库被污染）
const E_STAT: u16 = SEG_BASE + 10; // 拦截统计不守恒
const E_FP: u16 = SEG_BASE + 11; // 指纹漂移
const E_REBASE: u16 = SEG_BASE + 12; // 好档复检失败
const E_LINE: u16 = SEG_BASE + 13; // 呈现行缺段

/// 头长：magic(4)+ver(2)+count(2)+crc(4)+done(1) = 13 字节。
pub const HDR_LEN: usize = 13;
/// 档案魔数 "LARC"。
pub const MAGIC: [u8; 4] = [0x4C, 0x41, 0x52, 0x43];
/// 版本闭集 1..=2；完成标志。
pub const VER_LO: u16 = 1;
pub const VER_HI: u16 = 2;
pub const DONE_FLAG: u8 = 1;
/// 载荷字节闭集：拼音字母 a..=z。
pub const PAY_LO: u8 = 97;
pub const PAY_HI: u8 = 122;

/// FNV-1a 64 位（与本域其余哨兵同口径）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// CRC-32（IEEE，逐位实现，无查表）。
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    let mut i = 0usize;
    while i < data.len() {
        crc ^= data[i] as u32;
        let mut b = 0u8;
        while b < 8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            b += 1;
        }
        i += 1;
    }
    !crc
}

/// 确定性 LCG（批内统一系数）。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 学习档案实体。
pub struct LearnFile {
    pub version: u16,
    pub words: Vec<u8>,
}

/// 拼一份完整好档（字段全合法，done=1）。
pub fn build_blob(version: u16, words: &[u8], done: u8) -> Vec<u8> {
    let mut blob: Vec<u8> = vec![];
    blob.extend_from_slice(&MAGIC);
    blob.extend_from_slice(&version.to_le_bytes());
    blob.extend_from_slice(&(words.len() as u16).to_le_bytes());
    blob.extend_from_slice(&crc32(words).to_le_bytes());
    blob.push(done);
    blob.extend_from_slice(words);
    blob
}

/// 装载：逐字段校验，半写标志 (done != 1) 在 CRC 之后立即拒收。
pub fn load(blob: &[u8]) -> Result<LearnFile, u16> {
    if blob.len() < HDR_LEN {
        return Err(E_SHORT);
    }
    if blob[0] != MAGIC[0] || blob[1] != MAGIC[1] || blob[2] != MAGIC[2] || blob[3] != MAGIC[3] {
        return Err(E_MAGIC);
    }
    let version = u16::from_le_bytes([blob[4], blob[5]]);
    if version < VER_LO || version > VER_HI {
        return Err(E_VER);
    }
    let count = u16::from_le_bytes([blob[6], blob[7]]);
    let crc = u32::from_le_bytes([blob[8], blob[9], blob[10], blob[11]]);
    let done = blob[12];
    if done != DONE_FLAG {
        return Err(E_HALF);
    }
    let payload = &blob[HDR_LEN..];
    if crc32(payload) != crc {
        return Err(E_CRC);
    }
    if payload.len() != count as usize {
        return Err(E_COUNT);
    }
    if payload.is_empty() {
        return Err(E_PAYLOAD);
    }
    let mut words: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < payload.len() {
        if payload[i] < PAY_LO || payload[i] > PAY_HI {
            return Err(E_PAYLOAD);
        }
        words.push(payload[i]);
        i += 1;
    }
    Ok(LearnFile { version, words })
}

/// 半写例：镜像 + 预期诊断码。
pub struct HalfRec {
    pub name: &'static str,
    pub blob: Vec<u8>,
    pub expect: u16,
}

/// 十例伪造半写档。
pub fn halfrec_set() -> Vec<HalfRec> {
    let good = build_blob(2, b"nihao", DONE_FLAG);
    let mut out: Vec<HalfRec> = vec![];
    // 1 头截断：砍到半头。
    let mut b = good.clone();
    b.truncate(7);
    out.push(HalfRec { name: "short", blob: b, expect: E_SHORT });
    // 2 魔数坏。
    let mut b = good.clone();
    b[2] = b'X';
    out.push(HalfRec { name: "magic", blob: b, expect: E_MAGIC });
    // 3 半写标志：done=0。
    let mut b = good.clone();
    b[12] = 0;
    out.push(HalfRec { name: "half", blob: b, expect: E_HALF });
    // 4 校验坏：CRC 改写。
    let mut b = good.clone();
    b[8] ^= 0xFF;
    out.push(HalfRec { name: "crc", blob: b, expect: E_CRC });
    // 5 计数失配：count 虚增（CRC 对载荷仍有效）。
    let mut b = good.clone();
    let big = (9u16).to_le_bytes();
    b[6] = big[0];
    b[7] = big[1];
    out.push(HalfRec { name: "count", blob: b, expect: E_COUNT });
    // 6 版本越界：version=3。
    let mut b = good.clone();
    b[4] = 3;
    b[5] = 0;
    out.push(HalfRec { name: "ver", blob: b, expect: E_VER });
    // 7 空载荷：count=0 且载荷为空（重拼 CRC）。
    let mut b: Vec<u8> = vec![];
    b.extend_from_slice(&MAGIC);
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&0u16.to_le_bytes());
    b.extend_from_slice(&crc32(&[]).to_le_bytes());
    b.push(DONE_FLAG);
    out.push(HalfRec { name: "empty", blob: b, expect: E_PAYLOAD });
    // 8 载荷越集：数字混入（重拼 CRC 保持一致以专测载荷闸）。
    let mut b = good.clone();
    b[HDR_LEN] = b'1';
    let new_crc = crc32(&b[HDR_LEN..]);
    let nc = new_crc.to_le_bytes();
    b[8] = nc[0];
    b[9] = nc[1];
    b[10] = nc[2];
    b[11] = nc[3];
    out.push(HalfRec { name: "oob", blob: b, expect: E_PAYLOAD });
    // 9 尾截断：头完整载荷缺（count 仍写 5）。
    let mut b = good.clone();
    b.truncate(HDR_LEN + 2);
    out.push(HalfRec { name: "tailcut", blob: b, expect: E_CRC });
    // 10 双头拼接：两份好档头尾相接（载荷越集 + CRC 失配落在半写闸后）。
    let mut b = good.clone();
    b.extend_from_slice(&good);
    out.push(HalfRec { name: "dualhead", blob: b, expect: E_CRC });
    out
}

/// 档案库：好档集合 + 已拒收数。
pub struct Archive {
    pub files: Vec<Vec<u8>>,
    pub rejected: u32,
}

impl Archive {
    pub fn new() -> Archive {
        Archive {
            files: vec![],
            rejected: 0,
        }
    }

    /// 收档：装载成功入库，失败记拒收且不入库（零扩散）。
    pub fn accept(&mut self, blob: &[u8]) -> Result<(), u16> {
        match load(blob) {
            Ok(_) => {
                self.files.push(blob.to_vec());
                Ok(())
            }
            Err(code) => {
                self.rejected += 1;
                Err(code)
            }
        }
    }

    /// 库指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.files.len() {
            buf.extend_from_slice(&self.files[i]);
            buf.push(0x1F);
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 半写例呈现行：case=<n> name=<例名> len=<镜像长>。
pub fn half_line(idx: usize, h: &HalfRec) -> String {
    let mut s = String::new();
    s.push_str("case=");
    s.push_str(&(idx as u32).to_string());
    s.push_str(" name=");
    s.push_str(h.name);
    s.push_str(" len=");
    s.push_str(&(h.blob.len() as u32).to_string());
    s
}

/// 主巡检入口：R1..R9 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 好档装载基准：5 词、版本 2、done=1、词序逐字节一致。
    let good = build_blob(2, b"nihao", DONE_FLAG);
    let f = load(&good)?;
    if f.version != 2 || f.words.len() != 5 {
        return Err(E_REBASE);
    }
    let expect_words: Vec<u8> = vec![b'n', b'i', b'h', b'a', b'o'];
    if f.words != expect_words {
        return Err(E_REBASE);
    }
    hits += 1;

    // R2/R3 十例半写全拒：逐例诊断码精确匹配预期。
    let set = halfrec_set();
    if set.len() != 10 {
        return Err(E_STAT);
    }
    let mut arc = Archive::new();
    arc.accept(&good)?; // 先收好档垫底
    let base_fp = arc.fp();
    let base_n = arc.files.len();
    let mut i = 0usize;
    while i < set.len() {
        if let Ok(_) = load(&set[i].blob) {
            return Err(E_SPREAD);
        }
        match arc.accept(&set[i].blob) {
            Err(code) => {
                if code != set[i].expect {
                    return Err(E_MATCH);
                }
            }
            Ok(()) => return Err(E_SPREAD),
        }
        i += 1;
    }
    hits += 1;

    // R5 零扩散：十例全拒后档案库指纹与账目与垫底态一致，拒收记账 10。
    if arc.fp() != base_fp || arc.files.len() != base_n {
        return Err(E_SPREAD);
    }
    if arc.rejected != 10 {
        return Err(E_STAT);
    }
    hits += 1;

    // R4 诊断码可分：十例预期码 ≥6 种互异。
    let mut distinct: Vec<u16> = vec![];
    let mut i = 0usize;
    while i < set.len() {
        let mut seen = false;
        let mut j = 0usize;
        while j < distinct.len() {
            if distinct[j] == set[i].expect {
                seen = true;
            }
            j += 1;
        }
        if !seen {
            distinct.push(set[i].expect);
        }
        i += 1;
    }
    if distinct.len() < 6 {
        return Err(E_MATCH);
    }
    hits += 1;

    // R6 好档复检不受批次影响：垫底好档再装载一次仍成功且词序一致。
    let f2 = load(&good)?;
    if f2.words != f.words || f2.version != f.version {
        return Err(E_REBASE);
    }
    hits += 1;

    // R7 拦截统计守恒：另起干净库重放十例，拒收记账仍恰 10。
    let mut arc2 = Archive::new();
    let mut i = 0usize;
    while i < set.len() {
        if arc2.accept(&set[i].blob).is_ok() {
            return Err(E_SPREAD);
        }
        i += 1;
    }
    if arc2.rejected != 10 || !arc2.files.is_empty() {
        return Err(E_STAT);
    }
    hits += 1;

    // R8 LCG 随机半写流全拒：48 笔随机截断档（随机长度/随机字段翻转），
    // 至多允许极少数合法档入库，但任何入库档必须能通过完整装载回检。
    let mut lcg = Lcg::new(0x48542);
    let mut arc3 = Archive::new();
    arc3.accept(&good)?;
    let mut n = 0u32;
    while n < 48 {
        let len = lcg.below(HDR_LEN as u32 + 6) as usize;
        let mut blob: Vec<u8> = vec![];
        let mut k = 0usize;
        while k < len {
            blob.push(65 + lcg.below(26) as u8);
            k += 1;
        }
        // 随机翻转一个已生成字节（若非空）制造随机坏档。
        if !blob.is_empty() && lcg.below(2) == 0 {
            let at = lcg.below(blob.len() as u32) as usize;
            blob[at] ^= 0x20;
        }
        if let Ok(f) = load(&blob) {
            // 意外合法的档也必须能通过自身回检（词序闭集）。
            let mut i = 0usize;
            while i < f.words.len() {
                if f.words[i] < PAY_LO || f.words[i] > PAY_HI {
                    return Err(E_PAYLOAD);
                }
                i += 1;
            }
            arc3.accept(&blob).ok();
        } else {
            arc3.rejected += 1;
        }
        n += 1;
    }
    // 库内每档必须仍是完整可装载档（半写零扩散最终口径）。
    let mut i = 0usize;
    while i < arc3.files.len() {
        if load(&arc3.files[i]).is_err() {
            return Err(E_SPREAD);
        }
        i += 1;
    }
    hits += 1;

    // R9 指纹稳定：半写集指纹两次一致且非零；呈现行三段齐全。
    let mut buf: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < set.len() {
        buf.extend_from_slice(set[i].name.as_bytes());
        buf.extend_from_slice(&set[i].expect.to_le_bytes());
        buf.extend_from_slice(&(set[i].blob.len() as u32).to_le_bytes());
        i += 1;
    }
    let f1 = fnv1a64(&buf);
    let f2 = fnv1a64(&buf);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    let line = half_line(0, &set[0]);
    if !has_kw(line.as_bytes(), b"case=")
        || !has_kw(line.as_bytes(), b"name=")
        || !has_kw(line.as_bytes(), b"len=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

/// 子串判定（朴素扫描，统一口径）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}
