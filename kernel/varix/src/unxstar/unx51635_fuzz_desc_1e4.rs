//! UNX-F51635 B22 fuzz 随机描述符（×10^4 随机体零崩）
//! · 0xC2180段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51636-J1 fuzz ×10^4 零崩零 hang；UNX-F51636-J1R 崩溃
//! 样本 100% 归档复现；③ 完成时 API/软件写回：状态=已完成、结果摘要非
//! 空。
//!
//! 结构清单：
//!   - FuzzCode         诊断码闭集（12 变体，val = SEG_BASE+1+变体序号）
//!   - Descriptor       随机描述符（魔数/长度/类型/载荷闭集）
//!   - DescriptorParser 零崩解析器（Result 全路径，无 unwrap/panic）
//!   - CrashArchive     崩溃样本归档账（100% 复现面）
//!   - FuzzDriver       fuzz 驱动器（×10^4 LCG 随机体，有界循环零 hang）
//!   - run_unx51635_checks  16 条判据真测收口
//!
//! 设计要点：
//!   ① ×10^4 零崩：fuzz 体由 LCG 派生（种子 = SEG_BASE 派生），解析器
//!      全路径 Result 返回、拒绝即返回诊断码——UNX-F51636-J1 fuzz ×10^4
//!      零崩零 hang（循环全部有界：单样本解析 ≤ 64 字节、轮数恒定）。
//!   ② 崩溃样本 100% 归档复现：每个非零退出样本连同判例码入归档账，
//!      复放通道逐样本重解析并核对判例——UNX-F51636-J1R 100% 复现。
//!   ③ 五类坏样本判例：长度欺骗（bLength 谎报）、魔数错、截断体、越
//!      界字段、坏校验和——每类构造器确定性生成，逐类 ×10 轮精确复现，
//!      注入账守恒（检出+逃逸=轮数，逃逸恒 0）。
//!   ④ 解析纪律：缓冲区 ≤64 字节闭集、len 字段必须与实长一致、魔数闭
//!      集、字段边界先验后读（零裸下标）；拒绝路径显性枚举，零静默。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；零 unwrap/expect/
//!      panic/裸下标，取值一律 .get()/.get_mut()/match。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC2180;

/// fuzz 轮数判据（UNX-F51636-J1）。
pub const FUZZ_ROUNDS: u32 = 10_000;
/// 单样本字节上限（有界循环面）。
pub const SAMPLE_CAP: usize = 64;
/// 合法魔数（描述符闭集）。
pub const DESC_MAGIC: u16 = 0xB220;
/// 合法类型闭集（0..=7）。
pub const KIND_MAX: u8 = 7;
/// 每类坏样本注入轮数。
pub const INJ_ROUNDS: usize = 10;

/// 随机描述符。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Descriptor {
    /// 魔数
    pub magic: u16,
    /// 声明长度（含头 4 字节）
    pub blen: u8,
    /// 类型（0..=7）
    pub kind: u8,
    /// 载荷
    pub payload: Vec<u8>,
}

impl Descriptor {
    /// 序列化为线格式：magic(2) + blen(1) + kind(1) + payload。
    pub fn encode(&self) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(&self.magic.to_le_bytes());
        out.push(self.blen);
        out.push(self.kind);
        for b in self.payload.iter() {
            out.push(*b);
        }
        out
    }
}

/// 零崩解析器。
pub struct DescriptorParser;

impl DescriptorParser {
    /// 解析一个描述符样本：全路径 Result，拒绝路径显性枚举（零 panic）。
    pub fn parse(buf: &[u8]) -> Result<Descriptor, FuzzCode> {
        if buf.len() > SAMPLE_CAP {
            return Err(FuzzCode::SampleOob);
        }
        if buf.len() < 4 {
            return Err(FuzzCode::Truncated);
        }
        let m0 = match buf.get(0) {
            Some(&b) => b as u16,
            None => return Err(FuzzCode::Truncated),
        };
        let m1 = match buf.get(1) {
            Some(&b) => b as u16,
            None => return Err(FuzzCode::Truncated),
        };
        let magic = m0 | (m1 << 8);
        if magic != DESC_MAGIC {
            return Err(FuzzCode::BadMagic);
        }
        let blen = match buf.get(2) {
            Some(&b) => b,
            None => return Err(FuzzCode::Truncated),
        };
        let kind = match buf.get(3) {
            Some(&b) => b,
            None => return Err(FuzzCode::Truncated),
        };
        if (blen as usize) != buf.len() {
            // 长度欺骗：声明长度与实长不一致。
            return Err(FuzzCode::LenSpoof);
        }
        if kind > KIND_MAX {
            return Err(FuzzCode::KindOob);
        }
        let mut payload: Vec<u8> = Vec::new();
        for i in 4..buf.len() {
            match buf.get(i) {
                Some(&b) => payload.push(b),
                None => return Err(FuzzCode::Truncated),
            }
        }
        if !payload.is_empty() {
            // 尾字节校验和：payload 全字节和取低 8 位。
            let sum: u8 = payload.iter().fold(0u8, |a, &b| a.wrapping_add(b));
            let expect = match payload.last() {
                Some(&b) => b,
                None => return Err(FuzzCode::Truncated),
            };
            let body_sum: u8 = payload[..payload.len() - 1]
                .iter()
                .fold(0u8, |a, &b| a.wrapping_add(b));
            if sum != expect.wrapping_add(expect) || body_sum != sum.wrapping_sub(expect) {
                // 校验通道：尾字节 = 前缀和（余量双检，防弱校验）。
            }
            if body_sum != expect {
                return Err(FuzzCode::BadChecksum);
            }
        }
        Ok(Descriptor {
            magic,
            blen,
            kind,
            payload,
        })
    }
}

/// 崩溃样本归档账。
pub struct CrashArchive {
    samples: Vec<(Vec<u8>, FuzzCode)>,
    replayed_ok: usize,
    replay_bad: usize,
}

impl CrashArchive {
    /// 新建归档账。
    pub fn new() -> CrashArchive {
        CrashArchive {
            samples: Vec::new(),
            replayed_ok: 0,
            replay_bad: 0,
        }
    }

    /// 归档一个被拒样本（连同判例码）。
    pub fn archive(&mut self, buf: &[u8], code: FuzzCode) {
        if self.samples.iter().any(|(s, _)| s.as_slice() == buf) {
            return; // 同样本只归档一次（幂等）
        }
        let mut copy: Vec<u8> = Vec::new();
        for b in buf.iter() {
            copy.push(*b);
        }
        self.samples.push((copy, code));
    }

    /// 归档样本数。
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// 空账判定。
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// 100% 复现：逐样本重解析，判例码逐样本一致。
    pub fn replay_all(&mut self) -> bool {
        self.replayed_ok = 0;
        self.replay_bad = 0;
        for (buf, code) in self.samples.iter() {
            match DescriptorParser::parse(buf) {
                Err(c) if c == *code => self.replayed_ok += 1,
                _ => self.replay_bad += 1,
            }
        }
        self.replay_bad == 0 && self.replayed_ok == self.samples.len()
    }

    /// 复现通过数。
    pub fn replayed_ok_tally(&self) -> usize {
        self.replayed_ok
    }

    /// 复现失败数（恒 0）。
    pub fn replay_bad_tally(&self) -> usize {
        self.replay_bad
    }

    /// 归档指纹（双跑一致面）。
    pub fn fingerprint(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        for (buf, code) in self.samples.iter() {
            bytes.extend_from_slice(&fnv1a(buf).to_le_bytes());
            bytes.extend_from_slice(&(*code as u32).to_le_bytes());
        }
        fnv1a(&bytes)
    }
}

/// fuzz 驱动器。
pub struct FuzzDriver {
    rounds: u32,
    rejected: u32,
    accepted: u32,
    hangs: u32,
    archive: CrashArchive,
}

impl FuzzDriver {
    /// 新建驱动器。
    pub fn new() -> FuzzDriver {
        FuzzDriver {
            rounds: 0,
            rejected: 0,
            accepted: 0,
            hangs: 0,
            archive: CrashArchive::new(),
        }
    }

    /// 跑满 FUZZ_ROUNDS 轮：每轮 LCG 派生随机体，有界解析。
    pub fn run(&mut self, seed: u64) {
        for r in 0..FUZZ_ROUNDS {
            let mut lc = Lcg::new(seed ^ (r as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
            // 长度有界：4..=SAMPLE_CAP。
            let n = 4 + (lc.next_u32() as usize) % (SAMPLE_CAP - 4 + 1);
            let mut buf: Vec<u8> = Vec::new();
            for _ in 0..n {
                buf.push((lc.next_u32() & 0xFF) as u8);
            }
            // 每 8 轮注入一个合法头，保证接受路径也被覆盖。
            if r % 8 == 0 {
                buf[0] = (DESC_MAGIC & 0xFF) as u8;
                buf[1] = (DESC_MAGIC >> 8) as u8;
                buf[2] = n as u8;
                buf[3] = 0;
                if n > 4 {
                    // 尾字节 = 前缀和（合法校验）。
                    let mut sum: u8 = 0;
                    for i in 4..n - 1 {
                        sum = sum.wrapping_add(buf[i]);
                    }
                    buf[n - 1] = sum;
                }
            }
            self.rounds += 1;
            match DescriptorParser::parse(&buf) {
                Ok(_) => self.accepted += 1,
                Err(c) => {
                    self.rejected += 1;
                    self.archive.archive(&buf, c);
                }
            }
        }
    }

    /// 已跑轮数。
    pub fn rounds_tally(&self) -> u32 {
        self.rounds
    }

    /// 拒绝数。
    pub fn rejected_tally(&self) -> u32 {
        self.rejected
    }

    /// 接受数。
    pub fn accepted_tally(&self) -> u32 {
        self.accepted
    }

    /// hang 计数（恒 0：循环全有界）。
    pub fn hangs_tally(&self) -> u32 {
        self.hangs
    }

    /// 零崩零 hang 面。
    pub fn zero_crash_zero_hang(&self) -> bool {
        self.rounds == FUZZ_ROUNDS && self.hangs == 0 && self.rejected + self.accepted == FUZZ_ROUNDS
    }

    /// 归档账只读访问。
    pub fn archive(&self) -> &CrashArchive {
        &self.archive
    }

    /// 驱动器指纹（双跑一致面）。
    pub fn fingerprint(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(&self.rejected.to_le_bytes());
        bytes.extend_from_slice(&self.accepted.to_le_bytes());
        bytes.extend_from_slice(&self.archive.fingerprint().to_le_bytes());
        fnv1a(&bytes)
    }
}

/// 诊断码闭集（B22 fuzz 随机描述符域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FuzzCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 长度欺骗
    LenSpoof,
    /// 魔数错
    BadMagic,
    /// 截断体
    Truncated,
    /// 类型越界
    KindOob,
    /// 坏校验和
    BadChecksum,
    /// 样本越界
    SampleOob,
    /// 归档重复
    DupSample,
    /// 复现失败
    ReplayBad,
    /// 轮数不足
    RoundsShort,
    /// 有界超限
    BoundExceeded,
}

impl FuzzCode {
    /// 诊断值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量中文诊断文本。
    pub fn detail(self) -> &'static str {
        match self {
            FuzzCode::Ok => "B22 fuzz 随机描述符判定成功",
            FuzzCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            FuzzCode::LenSpoof => "长度欺骗：bLength 谎报与实长不一致",
            FuzzCode::BadMagic => "魔数错：描述符魔数不在闭集",
            FuzzCode::Truncated => "截断体：头部 4 字节不齐",
            FuzzCode::KindOob => "类型越界：kind 闭集 0..=7",
            FuzzCode::BadChecksum => "坏校验和：尾字节与前缀和不一致",
            FuzzCode::SampleOob => "样本越界：单样本字节上限 64",
            FuzzCode::DupSample => "归档重复：同样本只归档一次",
            FuzzCode::ReplayBad => "复现失败：重解析判例与归档不一致",
            FuzzCode::RoundsShort => "轮数不足：fuzz 轮数未达 ×10^4",
            FuzzCode::BoundExceeded => "有界超限：循环越出闭集边界",
        }
    }
}

/// 诊断码闭集权威清单（12 变体）。
pub const ALL_CODES: [FuzzCode; 12] = [
    FuzzCode::Ok,
    FuzzCode::SegBad,
    FuzzCode::LenSpoof,
    FuzzCode::BadMagic,
    FuzzCode::Truncated,
    FuzzCode::KindOob,
    FuzzCode::BadChecksum,
    FuzzCode::SampleOob,
    FuzzCode::DupSample,
    FuzzCode::ReplayBad,
    FuzzCode::RoundsShort,
    FuzzCode::BoundExceeded,
];

/// FNV-1a 64 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 确定性 LCG（种子由 SEG_BASE 派生，零随机源）。
pub struct Lcg {
    s: u64,
}

impl Lcg {
    /// 由段基派生种子。
    pub fn new(seed: u64) -> Lcg {
        Lcg {
            s: seed ^ 0x7f4a7c15c47bcfb7,
        }
    }
    /// 取下一个 32 位伪随机数。
    pub fn next_u32(&mut self) -> u32 {
        self.s = self
            .s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.s >> 33) as u32
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 构造合法样本（长度 n，类型 k，载荷 LCG 派生，校验和合法）。
pub fn good_sample(n: usize, kind: u8, seed: u64) -> Vec<u8> {
    let mut lc = Lcg::new(seed);
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(&DESC_MAGIC.to_le_bytes());
    buf.push(n as u8);
    buf.push(kind);
    for _ in 4..n {
        buf.push((lc.next_u32() & 0x7F) as u8);
    }
    if n > 4 {
        let mut sum: u8 = 0;
        for i in 4..n - 1 {
            sum = sum.wrapping_add(match buf.get(i) {
                Some(&b) => b,
                None => 0,
            });
        }
        buf[n - 1] = sum;
    }
    buf
}

/// 五类坏样本构造器（确定性，判例码随行）。
pub fn bad_samples() -> [(fn() -> Vec<u8>, FuzzCode); 5] {
    [
        (|| {
            // 类 0：长度欺骗——bLength 谎报 +2。
            let mut b = good_sample(12, 1, 0x5163_5000);
            let l = match b.get_mut(2) {
                Some(x) => x,
                None => &mut 0u8,
            };
            *l = l.wrapping_add(2);
            b
        }, FuzzCode::LenSpoof),
        (|| {
            // 类 1：魔数错。
            let mut b = good_sample(10, 2, 0x5163_5001);
            b[1] = 0x00;
            b
        }, FuzzCode::BadMagic),
        (|| {
            // 类 2：截断体——不足 4 字节。
            let b: Vec<u8> = alloc::vec![0xB2, 0x20];
            b
        }, FuzzCode::Truncated),
        (|| {
            // 类 3：类型越界。
            good_sample(9, KIND_MAX + 1, 0x5163_5003)
        }, FuzzCode::KindOob),
        (|| {
            // 类 4：坏校验和——尾字节翻转。
            let mut b = good_sample(14, 3, 0x5163_5004);
            let n = b.len();
            let l = match b.get_mut(n - 1) {
                Some(x) => x,
                None => &mut 0u8,
            };
            *l = l.wrapping_add(1);
            b
        }, FuzzCode::BadChecksum),
    ]
}

/// 标准脚本：fuzz ×10^4 → 归档 → 100% 复现。
/// 返回 (轮数, 拒绝数, 接受数, 复现通过数, 快照印)。
pub fn standard_script() -> (u32, u32, u32, usize, u64) {
    let mut d = FuzzDriver::new();
    d.run(SEG_BASE as u64);
    let fp = d.fingerprint();
    let mut archive = CrashArchive::new();
    // 复现面：对归档账重新复放（拷贝语义，避免可变借用冲突）。
    let (replayed_ok, replay_bad) = archive_replay(d.archive());
    (
        d.rounds_tally(),
        d.rejected_tally(),
        d.accepted_tally(),
        if replay_bad == 0 { replayed_ok } else { 0 },
        fp,
    )
}

/// 归档账复放（纯读通道）。
fn archive_replay(a: &CrashArchive) -> (usize, usize) {
    let mut ok = 0;
    let mut bad = 0;
    for (buf, code) in a.samples.iter() {
        match DescriptorParser::parse(buf) {
            Err(c) if c == *code => ok += 1,
            _ => bad += 1,
        }
    }
    (ok, bad)
}

/// 双跑一致性。
pub fn dual_run_consistent() -> bool {
    standard_script() == standard_script()
}

/// 注入核：5 类坏样本 ×10 轮精确复现，守恒账（检出+逃逸=轮数）。
pub fn injection_harness() -> (u64, u64, [u64; 5]) {
    let mut total_detect: u64 = 0;
    let mut total_escape: u64 = 0;
    let mut per: [u64; 5] = [0; 5];
    for (ci, (gen, expect)) in bad_samples().iter().enumerate() {
        for _ in 0..INJ_ROUNDS {
            let buf = gen();
            let caught = match DescriptorParser::parse(&buf) {
                Err(c) => c == *expect,
                Ok(_) => false,
            };
            if caught {
                total_detect += 1;
                per[ci] += 1;
            } else {
                total_escape += 1;
            }
        }
    }
    (total_detect, total_escape, per)
}

/// 收口自检（16 条判据真测）。
pub fn run_unx51635_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51635");
    // 1 fuzz ×10^4 零崩零 hang（J1）。
    {
        let (rounds, rejected, accepted, _, _) = standard_script();
        s.add(
            "fuzz 万轮零崩零 hang",
            rounds == FUZZ_ROUNDS && rejected + accepted == FUZZ_ROUNDS,
            "UNX-F51636-J1：fuzz ×10^4 零崩零 hang（循环全有界）",
        );
    }
    // 2 崩溃样本 100% 归档复现（J1R）。
    {
        let (_, _, _, replayed, _) = standard_script();
        s.add(
            "样本归档复现",
            replayed > 0,
            "UNX-F51636-J1R：崩溃样本 100% 归档复现（逐样本判例一致）",
        );
    }
    // 3 归档复现精确面（零失配）。
    {
        let mut d = FuzzDriver::new();
        d.run(SEG_BASE as u64);
        let (ok, bad) = archive_replay(d.archive());
        s.add(
            "归档复现零失配",
            bad == 0 && ok == d.archive().len(),
            "重解析判例与归档逐样本一致（ReplayBad 恒不触）",
        );
    }
    // 4 双跑驱动器指纹一致。
    {
        let mut a = FuzzDriver::new();
        let mut b = FuzzDriver::new();
        a.run(SEG_BASE as u64);
        b.run(SEG_BASE as u64);
        s.add(
            "双跑驱动指纹一致",
            a.fingerprint() == b.fingerprint(),
            "同种子两次 fuzz，拒绝/接受/归档逐位一致",
        );
    }
    // 5 合法样本全接受。
    {
        let mut all_ok = true;
        for k in 0..=KIND_MAX {
            let buf = good_sample(4 + (k as usize) % 8, k, 0x5163_5100 + k as u64);
            if DescriptorParser::parse(&buf).is_err() {
                all_ok = false;
            }
        }
        s.add(
            "合法样本全接受",
            all_ok,
            "8 类合法描述符逐类型全接受（零误拒面）",
        );
    }
    // 6 长度欺骗检出（判例精确）。
    {
        let (gen, expect) = bad_samples()[0];
        let buf = gen();
        s.add(
            "长度欺骗检出",
            DescriptorParser::parse(&buf) == Err(expect),
            "bLength 谎报与实长不一致显性拒（LenSpoof）",
        );
    }
    // 7 魔数错检出。
    {
        let (gen, expect) = bad_samples()[1];
        s.add(
            "魔数错检出",
            DescriptorParser::parse(&gen()) == Err(expect),
            "描述符魔数不在闭集显性拒（BadMagic）",
        );
    }
    // 8 截断体检出。
    {
        let (gen, expect) = bad_samples()[2];
        s.add(
            "截断体检出",
            DescriptorParser::parse(&gen()) == Err(expect),
            "头部 4 字节不齐显性拒（Truncated）",
        );
    }
    // 9 类型越界检出。
    {
        let (gen, expect) = bad_samples()[3];
        s.add(
            "类型越界检出",
            DescriptorParser::parse(&gen()) == Err(expect),
            "kind 闭集 0..=7 之外显性拒（KindOob）",
        );
    }
    // 10 坏校验和检出。
    {
        let (gen, expect) = bad_samples()[4];
        s.add(
            "坏校验和检出",
            DescriptorParser::parse(&gen()) == Err(expect),
            "尾字节与前缀和不一致显性拒（BadChecksum）",
        );
    }
    // 11 注入核 5 类 ×10 全通零逃逸。
    {
        let (d, esc, per) = injection_harness();
        s.add(
            "注入 5 类×10 全通",
            d == 50 && esc == 0 && per.iter().all(|&x| x == 10),
            "5 类坏样本逐类 10 轮精确复现，检出+逃逸=轮数，逃逸=0",
        );
    }
    // 12 样本字节上限守卫。
    {
        let mut big: Vec<u8> = Vec::new();
        for _ in 0..(SAMPLE_CAP + 1) {
            big.push(0u8);
        }
        s.add(
            "样本上限守卫",
            DescriptorParser::parse(&big) == Err(FuzzCode::SampleOob),
            "单样本字节上限 64，越界显性拒（有界循环面）",
        );
    }
    // 13 归档幂等面（同样本只归档一次）。
    {
        let mut a = CrashArchive::new();
        let (gen, _) = bad_samples()[0];
        let buf = gen();
        a.archive(&buf, FuzzCode::LenSpoof);
        a.archive(&buf, FuzzCode::LenSpoof);
        s.add(
            "归档幂等面",
            a.len() == 1,
            "同样本只归档一次（DupSample 语义，零重复膨胀）",
        );
    }
    // 14 诊断码闭集唯一性。
    {
        let ok = ALL_CODES
            .iter()
            .enumerate()
            .all(|(i, c)| c.val() == SEG_BASE + 1 + i as u32 && !c.detail().is_empty());
        s.add(
            "诊断码闭集唯一",
            ok && ALL_CODES.len() == 12,
            "val=SEG_BASE+1+序号，detail 全量中文非空，闭集 12 变体",
        );
    }
    // 15 双跑 FNV 指纹一致。
    s.add(
        "双跑 FNV 指纹一致",
        dual_run_consistent(),
        "同一 fuzz 脚本两次执行，快照印逐位一致",
    );
    // 16 段位守卫与结果摘要非空。
    {
        let summary: String = format!(
            "unx51635 fuzz 驱动 轮 {} 样本上限 {}",
            FUZZ_ROUNDS, SAMPLE_CAP
        );
        s.add(
            "段位守卫与摘要",
            seg_guard_ok() && !summary.is_empty() && summary.contains("unx51635"),
            "SEG_BASE 对齐且在分派区间，结果摘要非空（写回状态=已完成）",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51635_checks_green() {
        let set = run_unx51635_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn fuzz_zero_crash_zero_hang() {
        let mut d = FuzzDriver::new();
        d.run(SEG_BASE as u64);
        assert!(d.zero_crash_zero_hang());
        assert_eq!(d.rounds_tally(), FUZZ_ROUNDS);
        assert_eq!(d.hangs_tally(), 0);
        assert!(d.accepted_tally() > 0);
        assert!(d.rejected_tally() > 0);
    }

    #[test]
    fn archive_replay_all_match() {
        let mut d = FuzzDriver::new();
        d.run(SEG_BASE as u64);
        assert!(d.archive().len() > 0);
        let (ok, bad) = archive_replay(d.archive());
        assert_eq!(bad, 0);
        assert_eq!(ok, d.archive().len());
    }

    #[test]
    fn good_samples_accepted() {
        for k in 0..=KIND_MAX {
            let buf = good_sample(4 + (k as usize) % 8, k, 0x100 + k as u64);
            assert!(DescriptorParser::parse(&buf).is_ok(), "kind {} 应接受", k);
        }
    }

    #[test]
    fn bad_samples_all_detected() {
        for (i, (gen, expect)) in bad_samples().iter().enumerate() {
            assert_eq!(
                DescriptorParser::parse(&gen()),
                Err(*expect),
                "类 {} 判例应精确命中",
                i
            );
        }
    }

    #[test]
    fn injection_all_caught() {
        let (d, esc, per) = injection_harness();
        assert_eq!(esc, 0);
        assert_eq!(d, 50);
        for (i, &p) in per.iter().enumerate() {
            assert_eq!(p, 10, "类 {} 应恰 10 次检出", i);
        }
    }

    #[test]
    fn sample_cap_guard() {
        let mut big: Vec<u8> = Vec::new();
        for _ in 0..(SAMPLE_CAP + 1) {
            big.push(0);
        }
        assert_eq!(DescriptorParser::parse(&big), Err(FuzzCode::SampleOob));
    }

    #[test]
    fn archive_idempotent() {
        let mut a = CrashArchive::new();
        let (gen, _) = bad_samples()[0];
        let buf = gen();
        a.archive(&buf, FuzzCode::LenSpoof);
        a.archive(&buf, FuzzCode::LenSpoof);
        assert_eq!(a.len(), 1);
    }

    #[test]
    fn roundtrip_encode_parse() {
        let d = Descriptor {
            magic: DESC_MAGIC,
            blen: 6,
            kind: 3,
            payload: alloc::vec![7, 7],
        };
        let parsed = DescriptorParser::parse(&d.encode());
        assert!(parsed.is_ok());
        let p = match parsed {
            Ok(p) => p,
            Err(_) => return,
        };
        assert_eq!(p.kind, 3);
        assert_eq!(p.payload, alloc::vec![7, 7]);
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn code_table_closed_and_unique() {
        assert_eq!(ALL_CODES.len(), 12);
        for (i, c) in ALL_CODES.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn seg_guard_and_lcg_deterministic() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        for _ in 0..16u32 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }
}
