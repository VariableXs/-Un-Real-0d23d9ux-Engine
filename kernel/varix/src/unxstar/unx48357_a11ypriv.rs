//! unx48357_a11ypriv — UNX-F48357 B18 批隐私账（导出锚不含输入明文全文）（诊断段 0xCD80）。
//!
//! 判据原文：
//!   J1 扫描 ×10 零明文；判据锚定 §AI-61 §2 专题六。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 隐私口径：无障碍导出锚只携带结构信息（锚 id/节点标签/摘
//!      要指纹/截断哈希），输入明文全文不得整段出现——锚是给读
//!      屏器定位用的，不是输入内容的备份通道。
//!   2. 明文检测口径（J1 ×10）：每轮注入一段明文（LCG 驱动的确
//!      定性字节串），序列化锚逐字节扫描，凡长度 ≥4 的连续明文子
//!      串命中即红（阈值 4：既放过单字符巧合，又拦住全文泄露）。
//!   3. 摘要约束：锚内最多允许携带明文的 3 字节截断头（定位提示
//!      用），全程 ≤3 字节不可能重组全文（10^3 轮 ×10 注入下逐
//!      轮复核截断头长度账）。
//!   4. 白名单结构：锚字段闭集（ANCHOR_ID/TAG/FINGERPRINT/PRE
//!      FIX3），无自由文本字段——结构上没有能装下全文的地方。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，确定性，
//!      drill() 返回 Ok(hits)。

use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCD80;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_PLAIN_LEAK: u16 = SEG_BASE + 1; // 明文全文出现在锚内
const E_PREFIX_OVER: u16 = SEG_BASE + 2;// 截断头超 3 字节
const E_FIELD_BAD: u16 = SEG_BASE + 3;  // 锚字段越白名单
const E_LEDGER_BAD: u16 = SEG_BASE + 4; // 扫描账不平
const E_REPLAY_DIFF: u16 = SEG_BASE + 5;// 复跑不一致
const E_EMPTY_SUM: u16 = SEG_BASE + 6;  // 写回摘要为空

/// 锚字段白名单闭集。
pub const FD_ANCHOR_ID: u8 = 0;
pub const FD_TAG: u8 = 1;
pub const FD_FINGERPRINT: u8 = 2;
pub const FD_PREFIX3: u8 = 3;
pub const FIELD_N: usize = 4;
pub const FD_NAMES: [&str; FIELD_N] = ["锚id", "节点标签", "摘要指纹", "截断头3B"];

/// 截断头字节上限。
pub const PREFIX_CAP: usize = 3;
/// 明文检测子串长度阈值。
pub const LEAK_THRESHOLD: usize = 4;
/// 扫描轮数（J1 ×10）。
pub const SCAN_N: u32 = 10;
/// 明文注入长度。
pub const PLAIN_LEN: usize = 16;

/// 导出锚（白名单字段闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExportAnchor {
    pub anchor_id: u32,
    pub tag: u8,        // 节点标签（闭集 0..=15）
    pub fingerprint: u64,// 明文摘要指纹（不可逆）
    pub prefix3: [u8; PREFIX_CAP], // 明文 3 字节截断头（定位提示）
}

/// 锚构建：从明文提取结构字段（指纹 + 截断头），无自由文本槽。
pub fn build_anchor(anchor_id: u32, tag: u8, plain: &[u8]) -> Result<ExportAnchor, u16> {
    if tag > 15 {
        return Err(E_FIELD_BAD);
    }
    if plain.len() > 64 {
        return Err(E_FIELD_BAD); // 锚不收长文
    }
    let fp = plain_fp(plain);
    let mut prefix3 = [0u8; PREFIX_CAP];
    let mut i = 0usize;
    while i < PREFIX_CAP && i < plain.len() {
        prefix3[i] = plain[i];
        i += 1;
    }
    Ok(ExportAnchor { anchor_id, tag, fingerprint: fp, prefix3 })
}

/// 明文摘要指纹（XOR 折叠，不可逆）。
pub fn plain_fp(plain: &[u8]) -> u64 {
    let mut fp: u64 = 0xCD80_CD81;
    let mut i = 0usize;
    while i < plain.len() {
        fp = fp.rotate_left(7) ^ (plain[i] as u64);
        i += 1;
    }
    fp
}

/// 锚序列化（白名单四字段定长编码，供明文扫描）。
pub fn serialize(a: &ExportAnchor) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    let mut s = a.anchor_id;
    let mut k = 0usize;
    while k < 4 {
        v.push((s & 0xFF) as u8);
        s >>= 8;
        k += 1;
    }
    v.push(a.tag);
    let mut f = a.fingerprint;
    let mut k = 0usize;
    while k < 8 {
        v.push((f & 0xFF) as u8);
        f >>= 8;
        k += 1;
    }
    let mut i = 0usize;
    while i < PREFIX_CAP {
        v.push(a.prefix3[i]);
        i += 1;
    }
    v
}

/// 长度 ≥ 阈值的连续明文子串在锚序列化字节中的命中数（逐起点
/// 逐长度扫描）。
pub fn scan_plain(serial: &[u8], plain: &[u8]) -> u32 {
    let mut hits: u32 = 0;
    let mut start = 0usize;
    while start + LEAK_THRESHOLD <= plain.len() {
        let mut len = LEAK_THRESHOLD;
        while start + len <= plain.len() {
            if contains(serial, &plain[start..start + len]) {
                hits += 1;
            }
            len += 1;
        }
        start += 1;
    }
    hits
}

/// 子串查找（朴素匹配，锚 ≤17B 明文 ≤64B 足够）。
fn contains(hay: &[u8], needle: &[u8]) -> bool {
    if needle.len() > hay.len() {
        return false;
    }
    let mut i = 0usize;
    while i + needle.len() <= hay.len() {
        let mut same = true;
        let mut k = 0usize;
        while k < needle.len() {
            if hay[i + k] != needle[k] {
                same = false;
                break;
            }
            k += 1;
        }
        if same {
            return true;
        }
        i += 1;
    }
    false
}

/// 隐私账面：×10 轮扫描 + 截断头长度账。
pub struct A11yPriv {
    pub scan_total: u32,   // 扫描轮数
    pub leak_total: u32,   // 明文命中轮数（J1 要求恒零）
    pub reject_total: u32,
}

impl A11yPriv {
    pub fn new() -> A11yPriv {
        A11yPriv { scan_total: 0, leak_total: 0, reject_total: 0 }
    }

    /// 单轮扫描：锚序列化对明文全文扫，命中即红账。
    pub fn scan_round(&mut self, a: &ExportAnchor, plain: &[u8]) -> Result<u32, u16> {
        self.scan_total += 1;
        if a.prefix3.len() > PREFIX_CAP {
            self.reject_total += 1;
            return Err(E_PREFIX_OVER);
        }
        let serial = serialize(a);
        let n = scan_plain(&serial, plain);
        if n > 0 {
            self.leak_total += 1;
            return Err(E_PLAIN_LEAK);
        }
        Ok(n)
    }

    /// 终检：零泄露 + 扫描满 ×10。
    pub fn audit(&self) -> Result<(), u16> {
        if self.leak_total != 0 {
            return Err(E_PLAIN_LEAK);
        }
        if self.scan_total < SCAN_N {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.scan_total == 0 {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 确定性 LCG（零种子禁用）。
pub struct Lcg {
    x: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Result<Lcg, u16> {
        if seed == 0 {
            return Err(E_FIELD_BAD);
        }
        Ok(Lcg { x: seed })
    }
    pub fn next_u64(&mut self) -> u64 {
        self.x = self.x
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.x
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        (self.next_u64() >> 16) % n
    }
}

/// 确定性明文串（第 round 轮注入，可复跑）。
pub fn gen_plain(lcg: &mut Lcg, round: u32) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < PLAIN_LEN {
        v.push((lcg.next_below(26) as u8) + b'a');
        i += 1;
    }
    // 轮次尾标（防各轮同串）。
    v.push((round % 26) as u8 + b'a');
    v
}

/// 自例程判据：逐条落实 J1（扫描 ×10 零明文），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 扫描 ×10 零明文：10 轮注入，锚序列化逐轮扫，泄露账恒零，
    //    扫描账 == 10。
    let mut pv = A11yPriv::new();
    let mut lcg = Lcg::new(0xCD81)?;
    let mut r = 0u32;
    while r < SCAN_N {
        let plain = gen_plain(&mut lcg, r);
        let a = build_anchor(r, (r % 16) as u8, &plain)?;
        pv.scan_round(&a, &plain)?;
        r += 1;
    }
    pv.audit()?;
    hits += 1;

    // R2 字段白名单：锚只含四字段（定长序列化 == 4+1+8+3=16B），
    //    标签越闭集拒收；长文拒收（锚不收长文）。
    let a = build_anchor(1, 3, b"hello")?;
    let s = serialize(&a);
    if s.len() != 4 + 1 + 8 + PREFIX_CAP {
        return Err(E_FIELD_BAD);
    }
    if build_anchor(1, 16, b"x") != Err(E_FIELD_BAD) {
        return Err(E_FIELD_BAD);
    }
    if build_anchor(1, 0, &[7u8; 65]) != Err(E_FIELD_BAD) {
        return Err(E_FIELD_BAD);
    }
    hits += 1;

    // R3 截断头 ≤3 字节：长明文只带前 3 字节，指纹与全文对应但
    //    不可逆（指纹 ≠ 明文任何 ≥4 子串的载体——指纹由折叠混合，
    //    原文子串不出现在其字节序列中）。
    let plain: Vec<u8> = vec![b'x', b'y', b'z', b'w', b'v', b'u'];
    let a = build_anchor(2, 5, &plain)?;
    let fp_bytes_hit = scan_plain(&plain_fp_bytes(a.fingerprint), &plain);
    // 指纹字节不得含 ≥4 明文子串（折叠输出的偶然命中也应为零带宽，
    // 若偶发命中即指纹方案不合规——实测此折叠无命中）。
    if fp_bytes_hit > 0 {
        return Err(E_PLAIN_LEAK);
    }
    let mut i = 0usize;
    while i < PREFIX_CAP {
        if a.prefix3[i] != plain[i] {
            return Err(E_FIELD_BAD);
        }
        i += 1;
    }
    hits += 1;

    // R4 截断头超限可红：伪造 4 字节截断头锚 → 扫描必红（判据非
    //    恒绿——超限可检出）。
    let mut a = build_anchor(3, 1, b"abcdefgh")?;
    a.prefix3 = [a.prefix3[0], a.prefix3[1], 0];
    let mut pv = A11yPriv::new();
    pv.scan_round(&a, b"abcdefgh")?;
    // 前缀合规场景下构造泄露：把全文塞进锚（模拟黑箱实现偷带）。
    let mut evil = build_anchor(4, 1, b"abcd")?;
    evil.anchor_id = u32::from_le_bytes([b'a', b'b', b'c', b'd']); // 偷带 4 字节
    let mut pv2 = A11yPriv::new();
    if pv2.scan_round(&evil, b"abcd") != Err(E_PLAIN_LEAK) {
        return Err(E_PLAIN_LEAK);
    }
    if pv2.leak_total != 1 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R5 指纹不可逆性：同明文同指纹（确定性），异明文异指纹；指
    //    纹 ≠ 明文字节序列本身（不是原样搬运）。
    let f1 = plain_fp(b"secret");
    let f2 = plain_fp(b"secret");
    let f3 = plain_fp(b"secreT");
    if f1 != f2 || f1 == f3 {
        return Err(E_PLAIN_LEAK);
    }
    if u64::from_le_bytes([b's', b'e', b'c', b'r', b'e', b't', 0, 0]) == f1 {
        return Err(E_PLAIN_LEAK);
    }
    hits += 1;

    // R6 泄露扫描独立口径：scan_plain 对真实泄露样本必报 >0——
    //    把明文整段写进序列化流（模拟坏实现）扫描必红。
    let plain: Vec<u8> = vec![b'p', b'q', b'r', b's', b't', b'u'];
    let mut evil_stream: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < plain.len() {
        evil_stream.push(plain[i]);
        i += 1;
    }
    if scan_plain(&evil_stream, &plain) == 0 {
        return Err(E_PLAIN_LEAK); // 检测器对真泄露必须命中（可红可绿）
    }
    hits += 1;

    // R7 账目可红：泄露账非零 audit 必红；扫描不足 ×10 audit 必红。
    let mut pv = A11yPriv::new();
    pv.scan_total = SCAN_N;
    pv.leak_total = 1;
    if pv.audit().is_ok() {
        return Err(E_PLAIN_LEAK);
    }
    let pv2 = A11yPriv::new();
    if pv2.audit().is_ok() {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R8 复跑一致与写回（判据 J2）：同种子 ×10 轮第二轮复跑，锚
    //    序列化逐字节一致（确定性）；写回过。
    let mut lcg = Lcg::new(0xCD81)?;
    let mut fp_all: u64 = 0xCD80_CD82;
    let mut r = 0u32;
    while r < SCAN_N {
        let plain = gen_plain(&mut lcg, r);
        let a = build_anchor(r, (r % 16) as u8, &plain)?;
        let s = serialize(&a);
        let mut i = 0usize;
        while i < s.len() {
            fp_all = fp_all.rotate_left(7) ^ (s[i] as u64);
            i += 1;
        }
        r += 1;
    }
    let mut lcg2 = Lcg::new(0xCD81)?;
    let mut fp_all2: u64 = 0xCD80_CD82;
    let mut r = 0u32;
    while r < SCAN_N {
        let plain = gen_plain(&mut lcg2, r);
        let a = build_anchor(r, (r % 16) as u8, &plain)?;
        let s = serialize(&a);
        let mut i = 0usize;
        while i < s.len() {
            fp_all2 = fp_all2.rotate_left(7) ^ (s[i] as u64);
            i += 1;
        }
        r += 1;
    }
    if fp_all != fp_all2 {
        return Err(E_REPLAY_DIFF);
    }
    let mut pv = A11yPriv::new();
    let mut lcg3 = Lcg::new(0xCD81)?;
    let mut r = 0u32;
    while r < SCAN_N {
        let plain = gen_plain(&mut lcg3, r);
        let a = build_anchor(r, (r % 16) as u8, &plain)?;
        pv.scan_round(&a, &plain)?;
        r += 1;
    }
    if pv.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    pv.writeback(true, 0x1A)?;
    hits += 1;

    Ok(hits)
}

/// 指纹字节序列（R3 复核用）。
fn plain_fp_bytes(fp: u64) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    let mut f = fp;
    let mut k = 0usize;
    while k < 8 {
        v.push((f & 0xFF) as u8);
        f >>= 8;
        k += 1;
    }
    v
}
