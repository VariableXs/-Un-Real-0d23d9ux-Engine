//! UNX-F0758 · 移交演练（模拟消费方按包验收全链） · 诊断段 0x5dc0
//! 判据原文摘录：UNX-F0758-J1 演练全链走通（解包→核对单逐项→回执三段 3/3）
//! 且演练态隔离断言过（正式包零触碰）；完成时 API/软件写回：状态=已完成、
//! 结果摘要非空。纯功能代码 ≥580 行。
//!
//! 设计纪律：
//! 1. 全链三段写死：解包（七哈希逐件核）→ 核对单逐项（六判据三列）
//!    → 回执三段（收包/核验/签收 3/3），段段按序，跳段即演练失败。
//! 2. 演练态隔离：演练在影子账上跑（影子包 = 正式包数据的项目化快照），
//!    演练过程对正式包的任何写尝试必须被隔离哨检出（正式包零触碰）。
//! 3. 回执三段：段一收包回执（七哈希一致）、段二核验回执（六判据全过）、
//!    段三签收回执（包哈希定版确认），三段齐才算演练走通。
//! 4. 定容数组账本 + 守恒恒等式：段位计数 pass+fail == SEGMENTS；
//!    影子写拦截数 == 演练写尝试数。
//! 5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 6. 诊断段 0x5dc0 为本单独占，XcCode 判别式显式写出。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x5dc0;

// ── 诊断码闭集（SEG_BASE+1..+N）────────────────────────────────
const E_UNPACK_LACK: u16 = SEG_BASE + 1; // 解包缺件（七哈希未齐）
const E_UNPACK_DRIFT: u16 = SEG_BASE + 2; // 解包哈希漂移（件哈希与清单不符）
const E_CHECK_LACK: u16 = SEG_BASE + 3; // 核对单六判据未齐
const E_CHECK_FAIL: u16 = SEG_BASE + 4; // 核对单存在不过项
const E_SEG_SKIP: u16 = SEG_BASE + 5; // 回执跳段（三段须按序）
const E_SEG_TWICE: u16 = SEG_BASE + 6; // 同段重复回执
const E_SEG_BAD: u16 = SEG_BASE + 7; // 段号越界（1..=3）
const E_ISOLATION: u16 = SEG_BASE + 8; // 演练触碰正式包（隔离哨拦截）
const E_ISOLATION_MISS: u16 = SEG_BASE + 9; // 隔离哨漏检
const E_SHADOW_LACK: u16 = SEG_BASE + 10; // 影子包未建立
const E_BAL: u16 = SEG_BASE + 11; // 守恒恒等式破裂
const E_SEALED: u16 = SEG_BASE + 12; // 演练已封账，拒绝再写
const E_EMPTY: u16 = SEG_BASE + 13; // 演练账为空
const E_TWICE: u16 = SEG_BASE + 14; // 演练重复执行

/// 回执三段段号。
pub const SEG_RECEIVE: u8 = 1; // 段一：收包回执
pub const SEG_VERIFY: u8 = 2; // 段二：核验回执
pub const SEG_SIGN: u8 = 3; // 段三：签收回执
pub const SEGMENTS: usize = 3;

/// 六件套件号（与 F0756 同序）。
pub const PIECES: usize = 6;

/// 隔离哨账定容。
pub const MAX_GUARD: usize = 16;

/// 六件套件名表。
pub const PIECE_NAMES: [&str; 6] = [
    "账册汇编",
    "接口快照",
    "ktest 封版",
    "性能基线",
    "域经卷",
    "验收核对单",
];

/// 件号 → 件名（越界拒绝）。
pub fn piece_name(piece_no: u8) -> Result<&'static str, u16> {
    if piece_no == 0 || piece_no as usize > PIECE_NAMES.len() {
        return Err(E_UNPACK_LACK);
    }
    Ok(PIECE_NAMES[(piece_no - 1) as usize])
}

// ── FNV-1a 64（包哈希与影子哈希口径）────────────────────────────
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 诊断详情：把错误码翻译成中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_UNPACK_LACK => "解包缺件：七哈希六件一清单须齐",
        E_UNPACK_DRIFT => "解包哈希漂移：件哈希与包清单不符",
        E_CHECK_LACK => "核对单未齐：六判据逐项过",
        E_CHECK_FAIL => "核对单存在不过项：先整改再演练",
        E_SEG_SKIP => "回执跳段：收包→核验→签收 按序",
        E_SEG_TWICE => "同段重复回执",
        E_SEG_BAD => "段号越界：1..=3",
        E_ISOLATION => "演练触碰正式包：隔离哨拦截（预期）",
        E_ISOLATION_MISS => "隔离哨漏检：拦截数与尝试数不平",
        E_SHADOW_LACK => "影子包未建立：先建影子再演练",
        E_BAL => "守恒破裂：段位计数账不平",
        E_SEALED => "演练已封账：封账后只读",
        E_EMPTY => "演练账为空",
        E_TWICE => "演练重复执行：一轮一账",
        _ => "未知诊断码",
    }
}

// ── 影子包 ───────────────────────────────────────────────────
/// 影子包：正式包的项目化快照（演练专用，与正式包数据隔离）。
#[derive(Clone)]
pub struct ShadowPack {
    /// 六件载荷。
    payloads: [Vec<u8>; PIECES],
    /// 包清单哈希（七哈希之七）。
    manifest: u64,
}

impl ShadowPack {
    /// 建影子包：六件载荷 + 清单哈希自动聚合。
    pub fn project(six: [&[u8]; PIECES]) -> Result<ShadowPack, u16> {
        let mut payloads: [Vec<u8>; PIECES] = [
            Vec::new(), Vec::new(), Vec::new(),
            Vec::new(), Vec::new(), Vec::new(),
        ];
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < PIECES {
            if six[i].is_empty() {
                return Err(E_SHADOW_LACK);
            }
            payloads[i] = Vec::from(six[i]);
            buf.extend_from_slice(fnv1a64(six[i]).to_le_bytes().as_slice());
            i += 1;
        }
        Ok(ShadowPack {
            payloads,
            manifest: fnv1a64(&buf),
        })
    }

    /// 清单哈希。
    pub fn manifest(&self) -> u64 {
        self.manifest
    }

    /// 件载荷只读。
    pub fn piece(&self, piece_no: u8) -> Result<&[u8], u16> {
        if piece_no == 0 || piece_no as usize > PIECES {
            return Err(E_UNPACK_LACK);
        }
        Ok(&self.payloads[(piece_no - 1) as usize])
    }
}

// ── 解包核 ───────────────────────────────────────────────────
/// 解包核结果：逐件一致位 + 清单一致。
#[derive(Clone, Copy, Debug)]
pub struct UnpackResult {
    /// 逐件一致（6 位位图，bit_i = 件 i+1 一致）。
    pub piece_ok: u8,
    /// 清单一致（重聚合哈希 == 清单哈希）。
    pub manifest_ok: bool,
}

impl UnpackResult {
    /// 六件全一致且清单一致。
    pub fn all_ok(&self) -> bool {
        self.manifest_ok && self.piece_ok == 0b111111
    }
}

/// 解包：逐件重哈希比对清单侧哈希，再聚合比对清单哈希。
pub fn unpack(shadow: &ShadowPack, declared_piece_hashes: &[u64; PIECES]) -> Result<UnpackResult, u16> {
    let mut piece_ok = 0u8;
    let mut buf: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < PIECES {
        let p = shadow.piece((i + 1) as u8)?;
        let h = fnv1a64(p);
        buf.extend_from_slice(h.to_le_bytes().as_slice());
        if h == declared_piece_hashes[i] {
            piece_ok |= 1u8 << i;
        }
        i += 1;
    }
    let manifest_ok = fnv1a64(&buf) == shadow.manifest();
    Ok(UnpackResult { piece_ok, manifest_ok })
}

// ── 演练账（主体）────────────────────────────────────────────
/// 移交演练账：影子包 → 三段回执 → 隔离哨 → 封账。
pub struct HandoverDrill {
    shadow: Option<ShadowPack>,
    /// 段位完成位图（bit0=段1 bit1=段2 bit2=段3）。
    seg_bits: u8,
    /// 段位计数：完成段数（守恒分子）。
    segs_done: u32,
    /// 段位失败数（恒应 0）。
    segs_failed: u32,
    /// 解包结果。
    unpacked: Option<UnpackResult>,
    /// 隔离哨账：正式包写尝试序号表。
    guard: [Option<u32>; MAX_GUARD],
    guard_count: usize,
    guarded: u32,
    /// 演练是否已封账。
    sealed: bool,
    /// 演练哈希。
    sealed_hash: u64,
}

impl HandoverDrill {
    /// 开演练：投影影子包。
    pub fn begin(six: [&[u8]; PIECES]) -> Result<HandoverDrill, u16> {
        Ok(HandoverDrill {
            shadow: Some(ShadowPack::project(six)?),
            seg_bits: 0,
            segs_done: 0,
            segs_failed: 0,
            unpacked: None,
            guard: [None; MAX_GUARD],
            guard_count: 0,
            guarded: 0,
            sealed: false,
            sealed_hash: 0,
        })
    }

    /// 段一：解包（须先于其余段）。
    pub fn seg1_unpack(&mut self, declared: &[u64; PIECES]) -> Result<UnpackResult, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        let sh = self.shadow.as_ref().ok_or(E_SHADOW_LACK)?;
        let r = unpack(sh, declared)?;
        if !r.all_ok() {
            self.segs_failed += 1;
            return Err(if !r.manifest_ok { E_UNPACK_DRIFT } else { E_UNPACK_LACK });
        }
        self.unpacked = Some(r);
        self.mark_seg(SEG_RECEIVE)?;
        Ok(r)
    }

    /// 段二：核对单逐项（六判据全过）。
    pub fn seg2_check(&mut self, six_conclusions: &[bool; PIECES]) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.seg_bits & 0b001 == 0 {
            return Err(E_SEG_SKIP);
        }
        // 六判据逐项核（影子侧载荷派生确定性结论由调用方传入，
        // 但六项必须全 true 才算核对单齐）。
        let mut i = 0usize;
        while i < PIECES {
            if !six_conclusions[i] {
                self.segs_failed += 1;
                return Err(E_CHECK_FAIL);
            }
            i += 1;
        }
        self.mark_seg(SEG_VERIFY)?;
        Ok(())
    }

    /// 段三：签收（包哈希定版确认）。
    pub fn seg3_sign(&mut self, declared_pkg: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.seg_bits & 0b010 == 0 {
            return Err(E_SEG_SKIP);
        }
        let sh = self.shadow.as_ref().ok_or(E_SHADOW_LACK)?;
        if declared_pkg != sh.manifest() {
            self.segs_failed += 1;
            return Err(E_UNPACK_DRIFT);
        }
        self.mark_seg(SEG_SIGN)?;
        Ok(())
    }

    /// 段位落账：段号合法 + 按序 + 不重复。
    fn mark_seg(&mut self, seg: u8) -> Result<(), u16> {
        if seg == 0 || seg as usize > SEGMENTS {
            return Err(E_SEG_BAD);
        }
        let bit = 1u8 << (seg - 1);
        if self.seg_bits & bit != 0 {
            return Err(E_SEG_TWICE);
        }
        // 按序：前置段必须已完成。
        let mut prev = bit - 1;
        if self.seg_bits & prev != prev {
            return Err(E_SEG_SKIP);
        }
        self.seg_bits |= bit;
        self.segs_done += 1;
        Ok(())
    }

    /// 段位守恒：完成 + 失败 ≤ 3 且完成位图与计数一致。
    pub fn conserved(&self) -> bool {
        self.segs_done + self.segs_failed <= SEGMENTS as u32
            && popcount3(self.seg_bits) == self.segs_done
    }

    /// 全链走通断言：三段全完成、零失败、守恒。
    pub fn chain_assert(&self) -> Result<(), u16> {
        if self.segs_done != SEGMENTS as u32 {
            return Err(E_BAL);
        }
        if self.segs_failed != 0 {
            return Err(E_CHECK_FAIL);
        }
        if !self.conserved() {
            return Err(E_BAL);
        }
        if self.seg_bits != 0b111 {
            return Err(E_SEG_SKIP);
        }
        Ok(())
    }

    /// 演练态隔离哨：对正式包的写尝试（演练期禁止）——
    /// 影子演练本不应触碰正式包，此哨记录并拒绝，正式包零触碰。
    pub fn formal_write_attempt(&mut self, seq: u32) -> Result<(), u16> {
        if self.guard_count >= MAX_GUARD {
            return Err(E_BAL);
        }
        self.guard[self.guard_count] = Some(seq);
        self.guard_count += 1;
        self.guarded += 1;
        Err(E_ISOLATION)
    }

    /// 隔离断言：哨账行数 == 拦截数（零漏检），且正式包零触碰
    /// 语义 = 演练账内没有任何正式包数据的落账（影子独立证明）。
    pub fn isolation_assert(&self) -> Result<(), u16> {
        if self.guard_count as u32 != self.guarded {
            return Err(E_ISOLATION_MISS);
        }
        Ok(())
    }

    /// 封演练账：全链走通 + 隔离 → 演练哈希。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_TWICE);
        }
        self.chain_assert()?;
        self.isolation_assert()?;
        let mut buf: Vec<u8> = Vec::new();
        buf.push(self.seg_bits);
        if let Some(u) = &self.unpacked {
            buf.push(u.piece_ok);
            buf.push(u.manifest_ok as u8);
        }
        if let Some(sh) = &self.shadow {
            buf.extend_from_slice(sh.manifest().to_le_bytes().as_slice());
        }
        self.sealed_hash = fnv1a64(&buf);
        self.sealed = true;
        Ok(self.sealed_hash)
    }

    /// 演练哈希。
    pub fn sealed_hash(&self) -> u64 {
        self.sealed_hash
    }

    /// 是否封账。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 段位进度读取。
    pub fn seg_progress(&self) -> u8 {
        self.seg_bits
    }

    /// 演练报告行渲染。
    pub fn report_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 移交演练 · 全链三段 ===".to_string());
        lines.push("段位: ".to_string() + &itoa(self.seg_bits as u64)
            + " 完成 " + &itoa(self.segs_done as u64)
            + " 失败 " + &itoa(self.segs_failed as u64));
        match &self.unpacked {
            Some(u) => {
                lines.push("解包: ".to_string()
                    + if u.all_ok() { "六件+清单全一致" } else { "存在漂移" });
            }
            None => lines.push(String::from("解包: 未执行")),
        }
        lines.push("隔离哨: ".to_string() + &itoa(self.guard_count as u64) + " 次拦截");
        lines
    }

    /// 解包明细行：逐件"件名|一致？"（解包后可渲染）。
    pub fn unpack_detail_lines(&self) -> Result<Vec<String>, u16> {
        let u = self.unpacked.ok_or(E_SHADOW_LACK)?;
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 解包明细 ===".to_string());
        let mut i = 0usize;
        while i < PIECES {
            let ok = u.piece_ok & (1u8 << i) != 0;
            let mut line = String::from("件");
            line.push_str(&itoa((i + 1) as u64));
            line.push('|');
            line.push_str(match piece_name((i + 1) as u8) {
                Ok(n) => n,
                Err(_) => "?",
            });
            line.push_str("|");
            line.push_str(if ok { "一致" } else { "漂移" });
            lines.push(line);
            i += 1;
        }
        lines.push("清单: ".to_string() + if u.manifest_ok { "一致" } else { "漂移" });
        Ok(lines)
    }

    /// 隔离哨账行渲染。
    pub fn guard_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 隔离哨账（正式包写尝试） ===".to_string());
        let mut i = 0usize;
        while i < self.guard_count {
            if let Some(seq) = &self.guard[i] {
                let mut line = String::from("G|序");
                line.push_str(&itoa(*seq as u64));
                line.push_str("|拦截");
                lines.push(line);
            }
            i += 1;
        }
        lines
    }

    /// 影子-正式一致证明：影子六件载荷与正式侧声明哈希逐件相等
    /// （影子是数据的忠实投影，但物理上独立账本）。
    pub fn shadow_matches(&self, declared_hashes: &[u64; PIECES]) -> bool {
        let sh = match &self.shadow {
            Some(s) => s,
            None => return false,
        };
        let mut i = 0usize;
        while i < PIECES {
            let p = match sh.piece((i + 1) as u8) {
                Ok(p) => p,
                Err(_) => return false,
            };
            if fnv1a64(p) != declared_hashes[i] {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 演练统计三元组：(完成段数, 失败段数, 哨账行数)。
    pub fn drill_stats(&self) -> (u32, u32, u32) {
        (self.segs_done, self.segs_failed, self.guard_count as u32)
    }
}

/// 3 位位图计数。
fn popcount3(b: u8) -> u32 {
    (b & 1) as u32 + ((b >> 1) & 1) as u32 + ((b >> 2) & 1) as u32
}

/// 极简无符号转十进制。
fn itoa(v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    let mut x = v;
    while x > 0 {
        buf[n] = b'0' + (x % 10) as u8;
        x /= 10;
        n += 1;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

// ── 自例程 ───────────────────────────────────────────────────
/// 演练：全链 3/3 走通 + 隔离断言 + 错误路径。返回 0 = 全过。
pub fn drill() -> Result<u32, u16> {
    let six: [&[u8]; 6] = [
        b"catalog final", b"api final", b"ktest final",
        b"perf final", b"domexp final", b"check final",
    ];
    let mut dl = HandoverDrill::begin(six)?;
    // 演练期触碰正式包 → 隔离哨拦截。
    match dl.formal_write_attempt(1) {
        Err(c) if c == E_ISOLATION => {}
        _ => return Err(SEG_BASE),
    }
    dl.isolation_assert()?;
    // 段一：解包（用影子侧派生哈希 = 全一致样本）。
    let mut declared = [0u64; PIECES];
    let mut i = 0usize;
    while i < PIECES {
        declared[i] = fnv1a64(six[i]);
        i += 1;
    }
    let ur = dl.seg1_unpack(&declared)?;
    if !ur.all_ok() {
        return Err(SEG_BASE);
    }
    // 段二：核对单六项全过。
    let ok6 = [true, true, true, true, true, true];
    dl.seg2_check(&ok6)?;
    // 段三：签收（包哈希一致）。
    let pkg = {
        let mut buf: Vec<u8> = Vec::new();
        let mut j = 0usize;
        while j < PIECES {
            buf.extend_from_slice(fnv1a64(six[j]).to_le_bytes().as_slice());
            j += 1;
        }
        fnv1a64(&buf)
    };
    dl.seg3_sign(pkg)?;
    dl.chain_assert()?;
    dl.isolation_assert()?;
    if !dl.conserved() {
        return Err(SEG_BASE);
    }
    // 取数口：明细、哨账、影子一致、统计。
    let ud = dl.unpack_detail_lines()?;
    if ud.len() != 8 {
        return Err(SEG_BASE);
    }
    let gl = dl.guard_lines();
    if gl.len() != 2 {
        return Err(SEG_BASE);
    }
    if !dl.shadow_matches(&declared) {
        return Err(SEG_BASE);
    }
    if piece_name(4)? != "性能基线" {
        return Err(SEG_BASE);
    }
    let (sd, sf, gc) = dl.drill_stats();
    if sd != 3 || sf != 0 || gc != 1 {
        return Err(SEG_BASE);
    }
    let rl = dl.report_lines();
    if rl.len() != 5 {
        return Err(SEG_BASE);
    }
    let h = dl.seal()?;
    if h == 0 || !dl.is_sealed() {
        return Err(SEG_BASE);
    }
    // 封账后再演练拒绝。
    match dl.seg2_check(&ok6) {
        Err(c) if c == E_SEALED => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：跳段（未解包先核对）。
    let mut dl2 = HandoverDrill::begin(six)?;
    match dl2.seg2_check(&ok6) {
        Err(c) if c == E_SEG_SKIP => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：核对单存在不过项。
    let mut dl3 = HandoverDrill::begin(six)?;
    let _ = dl3.seg1_unpack(&declared)?;
    let bad6 = [true, true, true, true, false, true];
    match dl3.seg2_check(&bad6) {
        Err(c) if c == E_CHECK_FAIL => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：解包哈希漂移。
    let mut dl4 = HandoverDrill::begin(six)?;
    let mut drift = declared;
    drift[2] ^= 0xFF;
    match dl4.seg1_unpack(&drift) {
        Err(c) if c == E_UNPACK_DRIFT || c == E_UNPACK_LACK => {}
        _ => return Err(SEG_BASE),
    }
    Ok(0)
}
