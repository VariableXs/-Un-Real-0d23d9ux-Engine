//! unx48302_cfgio — UNX-F48302 配置导入导出（开放格式+CRC+版本头，往返零丢失）（诊断段 0xC6A0）。
//!
//! 判据原文：
//!   J1 往返 ×10 保真、无关键必填拒绝导入；判据锚定 体验十四 开放性。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 开放格式帧：帧 = 头(魔数4B + 格式版本u16 + 段计数u16 + CRC32 u32)
//!      + 载荷(四段各8B + 树版本u64 + 备用8B，共48B)，全长 60B 定长——格式
//!      公开、逐字段有账，任何外部工具按本注释即可解帧（开放性即无黑箱）。
//!   2. CRC32 覆盖载荷：导入先验 CRC 再解字段，载荷任一位翻转必红；
//!      帧头魔数/格式版本不符、帧长截断、段计数不为四段，一律拒收且
//!      逐路诊断码分立（错误必须说清是哪一路坏）。
//!   3. 关键必填闭集（无关键必填拒绝导入）：会话令牌 0、ABI 版本≠1、
//!      页大小 0、心跳窗 0——四类缺一即拒，绝不半截导入（半截导入
//!      等于把损坏当事实源落库）。
//!   4. 往返 ×10 保真（J1）：10 轮 LCG 驱动在闭集内变更四段配置树，
//!      导出→导入→逐段逐字段逐位比对含版本——零丢失才绿；导出为纯
//!      函数，同树两次导出逐字节一致（指纹可复跑）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，确定性 LCG，
//!      drill() 返回 Ok(hits)。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC6A0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_TRUNC: u16 = SEG_BASE + 1;    // 帧长截断/不足定长
const E_MAGIC: u16 = SEG_BASE + 2;    // 魔数不符
const E_FMTVER: u16 = SEG_BASE + 3;   // 格式版本不受支持
const E_CRC: u16 = SEG_BASE + 4;      // CRC 校验失败
const E_REQUIRED: u16 = SEG_BASE + 5; // 关键必填缺失
const E_SECTION: u16 = SEG_BASE + 6;  // 段计数不符
const E_LEDGER: u16 = SEG_BASE + 7;   // 账目不平
const E_EMPTY_SUM: u16 = SEG_BASE + 8;// 写回摘要为空

/// 帧格式常量（开放格式契约）。
pub const FMT_VER: u16 = 1;
pub const SECTION_COUNT: u16 = 4;
pub const HEADER_LEN: usize = 12;
pub const PAYLOAD_LEN: usize = 48;
pub const FRAME_LEN: usize = HEADER_LEN + PAYLOAD_LEN;

/// 魔数 "VXCF"（Varix Config eXchange Frame）。
pub const MAGIC: [u8; 4] = [0x56, 0x58, 0x43, 0x46];

/// 方案 id 闭集（与配置树同闭集）。
pub const SCHEME_MAX: u8 = 2;
pub const JITTER_MIN_MS: u32 = 5;
pub const JITTER_MAX_MS: u32 = 60;
pub const COOL_MAX_MS: u32 = 200;

/// 四段配置树（与 unx48301 单一事实源同构，本面持值拷贝）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CfgTree {
    pub fhw_mode: bool,
    pub punct_cn: bool,
    pub page_size: u8,
    pub abi_ver: u32,
    pub session_token: u64,
    pub heartbeat_window: u32,
    pub scheme_id: u8,
    pub s2t_on: bool,
    pub fuzzy_on: bool,
    pub jitter_ms: u32,
    pub cool_ms: u32,
    pub caps_led_sync: bool,
    pub version: u64,
}

impl CfgTree {
    /// 声明默认树。
    pub fn default_tree() -> CfgTree {
        CfgTree {
            fhw_mode: false,
            punct_cn: true,
            page_size: 5,
            abi_ver: 1,
            session_token: 0xC6A0_C6A1,
            heartbeat_window: 3,
            scheme_id: 0,
            s2t_on: false,
            fuzzy_on: true,
            jitter_ms: 20,
            cool_ms: 40,
            caps_led_sync: true,
            version: 0,
        }
    }

    /// 关键必填闭集校验（导入导出共用，缺一即拒）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.page_size == 0 || self.page_size > 9 {
            return Err(E_REQUIRED);
        }
        if self.abi_ver != 1 {
            return Err(E_REQUIRED);
        }
        if self.session_token == 0 {
            return Err(E_REQUIRED);
        }
        if self.heartbeat_window == 0 || self.heartbeat_window > 10 {
            return Err(E_REQUIRED);
        }
        if self.scheme_id > SCHEME_MAX {
            return Err(E_REQUIRED);
        }
        if self.jitter_ms < JITTER_MIN_MS || self.jitter_ms > JITTER_MAX_MS {
            return Err(E_REQUIRED);
        }
        if self.cool_ms > COOL_MAX_MS || self.cool_ms < self.jitter_ms {
            return Err(E_REQUIRED);
        }
        Ok(())
    }
}

// ── 字节序原语（小端，纯整数算术）────────────────────────
fn put_u16(buf: &mut [u8], off: usize, v: u16) {
    buf[off] = (v & 0xFF) as u8;
    buf[off + 1] = (v >> 8) as u8;
}
fn put_u32(buf: &mut [u8], off: usize, v: u32) {
    buf[off] = (v & 0xFF) as u8;
    buf[off + 1] = (v >> 8) as u8;
    buf[off + 2] = (v >> 16) as u8;
    buf[off + 3] = (v >> 24) as u8;
}
fn put_u64(buf: &mut [u8], off: usize, v: u64) {
    let mut i = 0usize;
    while i < 8 {
        buf[off + i] = ((v >> (i * 8)) & 0xFF) as u8;
        i += 1;
    }
}
fn get_u16(buf: &[u8], off: usize) -> u16 {
    buf[off] as u16 | ((buf[off + 1] as u16) << 8)
}
fn get_u32(buf: &[u8], off: usize) -> u32 {
    buf[off] as u32
        | ((buf[off + 1] as u32) << 8)
        | ((buf[off + 2] as u32) << 16)
        | ((buf[off + 3] as u32) << 24)
}
fn get_u64(buf: &[u8], off: usize) -> u64 {
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < 8 {
        v |= (buf[off + i] as u64) << (i * 8);
        i += 1;
    }
    v
}

/// CRC32（poly 0xEDB88320，逐位反射，无查表——纯算术可审计）。
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    let mut i = 0usize;
    while i < data.len() {
        crc ^= data[i] as u32;
        let mut b = 0u32;
        while b < 8 {
            let lsb = crc & 1;
            crc >>= 1;
            if lsb == 1 {
                crc ^= 0xEDB8_8320;
            }
            b += 1;
        }
        i += 1;
    }
    !crc
}

/// XOR 折叠指纹（帧/树等价复核用，防漂移留痕）。
pub fn fingerprint(data: &[u8]) -> u64 {
    let mut fp: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut i = 0usize;
    while i < data.len() {
        fp ^= data[i] as u64;
        fp = fp.rotate_left(7);
        i += 1;
    }
    fp
}

/// 配置导入导出面：唯一出径 export、唯一入径 import、双账本。
pub struct CfgIo {
    pub export_total: u32,
    pub import_total: u32,
    pub reject_total: u32,
    pub roundtrip_total: u32,
}

impl CfgIo {
    pub fn new() -> CfgIo {
        CfgIo { export_total: 0, import_total: 0, reject_total: 0, roundtrip_total: 0 }
    }

    /// 出径：树 → 开放格式帧。树过不了关键必填闭集则拒出。
    /// 纯函数：同树两次导出逐字节一致。
    pub fn export(&mut self, tree: &CfgTree) -> Result<Vec<u8>, u16> {
        tree.validate()?;
        let mut frame = Vec::new();
        let mut i = 0usize;
        while i < FRAME_LEN {
            frame.push(0);
            i += 1;
        }
        // ── 帧头 ──
        frame[0] = MAGIC[0];
        frame[1] = MAGIC[1];
        frame[2] = MAGIC[2];
        frame[3] = MAGIC[3];
        put_u16(&mut frame, 4, FMT_VER);
        put_u16(&mut frame, 6, SECTION_COUNT);
        // CRC 位先置零，载荷齐后再折。
        put_u32(&mut frame, 8, 0);
        // ── 载荷：S0 引擎 ──
        let mut eng: u32 = 0;
        if tree.fhw_mode {
            eng |= 1;
        }
        if tree.punct_cn {
            eng |= 1 << 1;
        }
        eng |= (tree.page_size as u32) << 8;
        put_u32(&mut frame, HEADER_LEN, eng);
        put_u32(&mut frame, HEADER_LEN + 4, 0x5A5A_A5A5);
        // ── S1 桥 ──
        put_u16(&mut frame, HEADER_LEN + 8, tree.abi_ver as u16);
        put_u16(&mut frame, HEADER_LEN + 10, tree.heartbeat_window as u16);
        put_u32(&mut frame, HEADER_LEN + 12, tree.session_token as u32);
        put_u32(&mut frame, HEADER_LEN + 16, (tree.session_token >> 32) as u32);
        // ── S2 方案 ──
        let mut sch: u32 = tree.scheme_id as u32;
        if tree.s2t_on {
            sch |= 1 << 8;
        }
        if tree.fuzzy_on {
            sch |= 1 << 9;
        }
        put_u32(&mut frame, HEADER_LEN + 20, sch);
        put_u32(&mut frame, HEADER_LEN + 24, 0x5A5A_A5A5);
        // ── S3 切换 ──
        put_u16(&mut frame, HEADER_LEN + 28, tree.jitter_ms as u16);
        put_u16(&mut frame, HEADER_LEN + 30, tree.cool_ms as u16);
        let mut swb: u32 = 0;
        if tree.caps_led_sync {
            swb |= 1;
        }
        put_u32(&mut frame, HEADER_LEN + 32, swb);
        put_u32(&mut frame, HEADER_LEN + 36, 0x5A5A_A5A5);
        // ── S4 树版本 + 备用 ──
        put_u64(&mut frame, HEADER_LEN + 40, tree.version);
        put_u32(&mut frame, HEADER_LEN + 48, 0x5A5A_A5A5);
        // ── CRC 覆盖载荷 ──
        let crc = crc32(&frame[HEADER_LEN..FRAME_LEN]);
        put_u32(&mut frame, 8, crc);
        self.export_total += 1;
        Ok(frame)
    }

    /// 入径：帧 → 树。五路前拒（截断/魔数/版本/段数/CRC）+
    /// 关键必填闭集——任一路不过整帧拒收，绝不半截导入。
    pub fn import(&mut self, frame: &[u8]) -> Result<CfgTree, u16> {
        if frame.len() < FRAME_LEN {
            self.reject_total += 1;
            return Err(E_TRUNC);
        }
        if frame[0] != MAGIC[0] || frame[1] != MAGIC[1]
            || frame[2] != MAGIC[2] || frame[3] != MAGIC[3] {
            self.reject_total += 1;
            return Err(E_MAGIC);
        }
        if get_u16(frame, 4) != FMT_VER {
            self.reject_total += 1;
            return Err(E_FMTVER);
        }
        if get_u16(frame, 6) != SECTION_COUNT {
            self.reject_total += 1;
            return Err(E_SECTION);
        }
        let want = get_u32(frame, 8);
        let got = crc32(&frame[HEADER_LEN..FRAME_LEN]);
        if want != got {
            self.reject_total += 1;
            return Err(E_CRC);
        }
        // ── 解字段 ──
        let eng = get_u32(frame, HEADER_LEN);
        let mut tree = CfgTree::default_tree();
        tree.fhw_mode = eng & 1 != 0;
        tree.punct_cn = eng & (1 << 1) != 0;
        tree.page_size = ((eng >> 8) & 0xFF) as u8;
        tree.abi_ver = get_u16(frame, HEADER_LEN + 8) as u32;
        tree.heartbeat_window = get_u16(frame, HEADER_LEN + 10) as u32;
        tree.session_token =
            get_u32(frame, HEADER_LEN + 12) as u64 | ((get_u32(frame, HEADER_LEN + 16) as u64) << 32);
        let sch = get_u32(frame, HEADER_LEN + 20);
        tree.scheme_id = (sch & 0xFF) as u8;
        tree.s2t_on = sch & (1 << 8) != 0;
        tree.fuzzy_on = sch & (1 << 9) != 0;
        tree.jitter_ms = get_u16(frame, HEADER_LEN + 28) as u32;
        tree.cool_ms = get_u16(frame, HEADER_LEN + 30) as u32;
        tree.caps_led_sync = get_u32(frame, HEADER_LEN + 32) & 1 != 0;
        tree.version = get_u64(frame, HEADER_LEN + 40);
        tree.validate()?;
        self.import_total += 1;
        Ok(tree)
    }

    /// 往返一轮（J1 主体）：导出→导入→逐字段逐位比对（含版本）。
    /// 出入各一次，账目守恒（导出数==导入数+拒收数）。
    pub fn roundtrip(&mut self, tree: &CfgTree) -> Result<CfgTree, u16> {
        let frame = self.export(tree)?;
        if frame.len() != FRAME_LEN {
            return Err(E_TRUNC);
        }
        let back = self.import(&frame)?;
        if back != *tree {
            return Err(E_CRC); // 逐位不一致即保真失败
        }
        self.roundtrip_total += 1;
        Ok(back)
    }

    /// 账目守恒审计：导入数+拒收数==导出数，往返数≤导入数。
    pub fn audit(&self) -> Result<(), u16> {
        if self.export_total != self.import_total + self.reject_total {
            return Err(E_LEDGER);
        }
        if self.roundtrip_total > self.import_total {
            return Err(E_LEDGER);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.roundtrip_total == 0 {
            return Err(E_LEDGER);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 确定性 LCG（fuzz/变异唯一熵源，零种子禁用：种子 0 直接拒收）。
pub struct Lcg {
    x: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Result<Lcg, u16> {
        if seed == 0 {
            return Err(E_REQUIRED); // 零种子禁用
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

/// LCG 驱动的闭集内树变异（第 round 轮，确定性可复跑）。
pub fn mutate(lcg: &mut Lcg, round: u32, base: &CfgTree) -> CfgTree {
    let mut t = *base;
    t.version = round as u64 + 1;
    t.page_size = (lcg.next_below(9) + 1) as u8;
    t.heartbeat_window = (lcg.next_below(10) + 1) as u32;
    t.scheme_id = lcg.next_below(3) as u8;
    t.jitter_ms = (JITTER_MIN_MS as u64 + lcg.next_below((JITTER_MAX_MS - JITTER_MIN_MS + 1) as u64)) as u32;
    let mut c = lcg.next_below(COOL_MAX_MS as u64 - t.jitter_ms as u64 + 1) as u32;
    c += t.jitter_ms;
    t.cool_ms = c;
    t.fhw_mode = lcg.next_below(2) == 0;
    t.punct_cn = lcg.next_below(2) == 0;
    t.s2t_on = lcg.next_below(2) == 0;
    t.fuzzy_on = lcg.next_below(2) == 0;
    t.caps_led_sync = lcg.next_below(2) == 0;
    t
}

/// 自例程判据：逐条落实 J1（往返 ×10 保真、无关键必填拒绝导入），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 往返 ×10 保真：LCG 驱动闭集内变异 10 轮，导出→导入→逐位
    //    一致（含版本），帧长恒 60B，账目守恒。
    let mut io = CfgIo::new();
    let mut lcg = Lcg::new(0xC6A0)?;
    let mut r = 0u32;
    while r < 10 {
        let tree = mutate(&mut lcg, r, &CfgTree::default_tree());
        let back = io.roundtrip(&tree)?;
        if back != tree {
            return Err(E_CRC);
        }
        if back.version != tree.version {
            return Err(E_LEDGER);
        }
        r += 1;
    }
    if io.roundtrip_total != 10 {
        return Err(E_LEDGER);
    }
    io.audit()?;
    hits += 1;

    // R2 默认树与全开树往返：两端树形（缺省/全开全切换）也逐位
    //    保真，帧长恒定 60B、头四字节为魔数、格式版本=1。
    let mut io = CfgIo::new();
    let mut t = CfgTree::default_tree();
    let f1 = io.export(&t)?;
    if f1.len() != FRAME_LEN {
        return Err(E_TRUNC);
    }
    if f1[0] != MAGIC[0] || f1[1] != MAGIC[1] || f1[2] != MAGIC[2] || f1[3] != MAGIC[3] {
        return Err(E_MAGIC);
    }
    let b1 = io.import(&f1)?;
    if b1 != t {
        return Err(E_CRC);
    }
    t.fhw_mode = true;
    t.punct_cn = true;
    t.s2t_on = true;
    t.scheme_id = SCHEME_MAX;
    t.caps_led_sync = false;
    t.version = 7;
    let f2 = io.export(&t)?;
    let b2 = io.import(&f2)?;
    if b2 != t || b2.version != 7 {
        return Err(E_CRC);
    }
    hits += 1;

    // R3 五路错误注入：截断/坏魔数/坏格式版本/坏 CRC/段计数——
    //    五路诊断码分立且互不串路。
    let mut io = CfgIo::new();
    let t = CfgTree::default_tree();
    let good = io.export(&t)?;
    let mut trunc = good.clone();
    trunc.pop();
    if io.import(&trunc) != Err(E_TRUNC) {
        return Err(E_TRUNC);
    }
    let mut badmagic = good.clone();
    badmagic[0] ^= 0xFF;
    if io.import(&badmagic) != Err(E_MAGIC) {
        return Err(E_MAGIC);
    }
    let mut badver = good.clone();
    badver[4] = 2; // 格式版本 2 → 版本路
    if io.import(&badver) != Err(E_FMTVER) {
        return Err(E_FMTVER);
    }
    let mut badcrc = good.clone();
    badcrc[4] = 0; // 格式版本 0 → 仍走版本路（版本前置于 CRC）
    if io.import(&badcrc) != Err(E_FMTVER) {
        return Err(E_FMTVER);
    }
    let mut badcrc2 = good.clone();
    badcrc2[HEADER_LEN] ^= 0x01; // 载荷位翻转 → CRC 路
    if io.import(&badcrc2) != Err(E_CRC) {
        return Err(E_CRC);
    }
    let mut badsec = good.clone();
    badsec[6] = 3;
    if io.import(&badsec) != Err(E_SECTION) {
        return Err(E_SECTION);
    }
    if io.import(&good).is_err() {
        return Err(E_CRC); // 原帧必须仍可导入
    }
    hits += 1;

    // R4 CRC 防漂移：载荷 5 个不同位逐帧翻转全部 E_CRC；帧头
    //    CRC 字段本身被改同样 E_CRC。
    let mut io = CfgIo::new();
    let t = CfgTree::default_tree();
    let good = io.export(&t)?;
    let probes: [usize; 5] = [HEADER_LEN, HEADER_LEN + 9, HEADER_LEN + 21, HEADER_LEN + 30, HEADER_LEN + 40];
    let mut p = 0usize;
    while p < probes.len() {
        let mut f = good.clone();
        f[probes[p]] ^= 0x80;
        if io.import(&f) != Err(E_CRC) {
            return Err(E_CRC);
        }
        p += 1;
    }
    let mut fcrc = good.clone();
    fcrc[8] ^= 0x01;
    if io.import(&fcrc) != Err(E_CRC) {
        return Err(E_CRC);
    }
    hits += 1;

    // R5 关键必填逐项拒绝（无关键必填拒绝导入）：令牌 0、ABI≠1、
    //    页大小 0、心跳窗 0——四路各拒一次且计数留痕，无半截树。
    let mut io = CfgIo::new();
    let mut t = CfgTree::default_tree();
    t.session_token = 0;
    if io.export(&t) != Err(E_REQUIRED) {
        return Err(E_REQUIRED);
    }
    let mut t = CfgTree::default_tree();
    t.abi_ver = 0;
    if io.export(&t) != Err(E_REQUIRED) {
        return Err(E_REQUIRED);
    }
    let mut t = CfgTree::default_tree();
    t.page_size = 0;
    if io.export(&t) != Err(E_REQUIRED) {
        return Err(E_REQUIRED);
    }
    let mut t = CfgTree::default_tree();
    t.heartbeat_window = 0;
    if io.export(&t) != Err(E_REQUIRED) {
        return Err(E_REQUIRED);
    }
    if io.import_total != 0 || io.export_total != 0 {
        return Err(E_LEDGER); // 拒出不产帧、更无导入
    }
    // 入径侧同一红线上探：直接改载荷字段令页大小为 0，CRC 重折
    //    后仍必走必填路（证明必填校验在 CRC 之后仍独立生效）。
    let mut io2 = CfgIo::new();
    let good = io2.export(&CfgTree::default_tree())?;
    let mut f = good.clone();
    f[HEADER_LEN] = 0x00; // 页大小字节清零
    let crc = crc32(&f[HEADER_LEN..FRAME_LEN]);
    f[8] = (crc & 0xFF) as u8;
    f[9] = ((crc >> 8) & 0xFF) as u8;
    f[10] = ((crc >> 16) & 0xFF) as u8;
    f[11] = ((crc >> 24) & 0xFF) as u8;
    if io2.import(&f) != Err(E_REQUIRED) {
        return Err(E_REQUIRED);
    }
    hits += 1;

    // R6 导出纯函数：同树两次导出逐字节一致；十轮复跑指纹逐轮
    //    相同（确定性，可复跑）。
    let mut io = CfgIo::new();
    let mut lcg = Lcg::new(0xC6A1)?;
    let mut r = 0u32;
    while r < 10 {
        let tree = mutate(&mut lcg, r, &CfgTree::default_tree());
        let f1 = io.export(&tree)?;
        let f2 = io.export(&tree)?;
        if f1 != f2 {
            return Err(E_CRC);
        }
        let fp1 = fingerprint(&f1);
        let f3 = io.export(&tree)?;
        if fingerprint(&f3) != fp1 {
            return Err(E_CRC);
        }
        r += 1;
    }
    hits += 1;

    // R7 拒收账目与可红：各路拒收计数逐次留痕（每帧一导一拒，
    //    守恒可对账）；伪造账目（导出≠导入+拒收）必红可演练。
    let mut io = CfgIo::new();
    let t = CfgTree::default_tree();
    let good = io.export(&t)?;
    let mut f = good.clone();
    f[1] ^= 0xFF;
    io.import(&f).is_err();
    let good2 = io.export(&t)?;
    let mut f2 = good2.clone();
    f2[1] ^= 0xFF;
    io.import(&f2).is_err();
    let good3 = io.export(&t)?;
    let mut f3 = good3.clone();
    f3.pop();
    io.import(&f3).is_err();
    if io.reject_total != 3 {
        return Err(E_LEDGER);
    }
    io.import(&good)?;
    // 守恒：4 导出 == 1 成功导入 + 3 拒收。
    if io.export_total != io.import_total + io.reject_total {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R8 往返第二轮复跑与写回（判据 J2）：同 LCG 种子重跑 ×10，
    //    终轮同树同帧指纹一致（可复跑）；末尾空摘要拒收、非空写
    //    回过；主账审计守恒（复跑比对走独立 io 面不污染主账）。
    let mut io = CfgIo::new();
    let mut lcg = Lcg::new(0xC6A0)?;
    let mut final_tree = CfgTree::default_tree();
    let mut r = 0u32;
    while r < 10 {
        let tree = mutate(&mut lcg, r, &CfgTree::default_tree());
        let back = io.roundtrip(&tree)?;
        if back != tree {
            return Err(E_CRC);
        }
        if r == 9 {
            final_tree = back;
        }
        r += 1;
    }
    // 复跑一轮：同种子重推至第 9 轮，树逐位一致；帧指纹经独立
    //    io 面复核（导出纯函数 + 确定性变异双证）。
    let mut lcg2 = Lcg::new(0xC6A0)?;
    let mut r2 = 0u32;
    let mut replay_tree = CfgTree::default_tree();
    while r2 < 10 {
        replay_tree = mutate(&mut lcg2, r2, &CfgTree::default_tree());
        r2 += 1;
    }
    if replay_tree != final_tree {
        return Err(E_CRC);
    }
    let mut io2 = CfgIo::new();
    let f_re = io2.export(&replay_tree)?;
    let f_or = io2.export(&final_tree)?;
    if fingerprint(&f_re) != fingerprint(&f_or) {
        return Err(E_CRC);
    }
    io.audit()?;
    if io.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    io.writeback(true, 0x52)?;
    hits += 1;

    Ok(hits)
}
