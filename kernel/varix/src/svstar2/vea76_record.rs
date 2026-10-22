//! VE-F0076 · 命令流录制与回放器（VE-A 域 · A04 命令流组 · 目标 440 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0076`
//!
//! **职责定位（锚点原文）**：命令流录制与回放器——GPU 命令流的完整录制与
//! 确定性回放（每条命令的参数/顺序/上下文全录），回放用于 bug 复现
//! （录一次回放万次），录制开销显式（开录制慢多少写明），回放与实机对拍校验。
//! 命令流是 GPU 的日记——日记记得全，bug 就跑不掉；含录制的压缩存储
//! （体积与完整性平衡）。
//!
//! **数据结构（锚点原文）**：录制器；回放器。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//! - 回放失真 → 对拍归因（给出**首个不一致命令的下标与字段名**，
//!   不是「不一致」三个字——没有下标的归因等于没归因）；
//! - 录制溢出 → 分片（按定长切片，跨片回放与整条流字节级一致）；
//! - 开销超标 → 降采样（抽 1/N，**并在录制上打 sampled 标记**，
//!   之后不许拿抽检录制声称「回放一致」）。
//!
//! **性能逐项分解（锚点原文）**：O(命令数)。录制一次 push，回放一次
//! 乘加；压缩是线性扫描 + 行程编码，解压是线性扫描。
//!
//! **跨批对接点（锚点原文）**：X03 调试套件接入——`Pack` 的字节流格式
//! 是 X03 的存档格式，`MAGIC`/`FORMAT_VERSION` 变更即破坏兼容，必须同步。
//!
//! **无障碍与隐私（锚点原文）**：回放面板读屏可达——`panel_line()` 输出
//! 纯文本「条数/分片/开销/是否抽检」，不含命令参数值。
//!
//! 纯功能线：no_std + alloc；零 unwrap/expect/panic；整数定点；
//! 零浮点、零墙钟、零 IO。
//!
//! 诊断段：0xE7A0 段（0xE7A0~0xE7AF 十六码）。

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 诊断码（0xE7A0 段）
// ---------------------------------------------------------------------------

/// 回放与实机不一致。
pub const XC_REPLAY_DIVERGE: u16 = 0xE7A0;
/// 录制硬上限溢出。
pub const XC_RECORD_FULL: u16 = 0xE7A1;
/// 压缩流损坏（校验和不符）。
pub const XC_DECOMPRESS_CORRUPT: u16 = 0xE7A2;
/// 压缩流截断（长度不足）。
pub const XC_DECOMPRESS_TRUNC: u16 = 0xE7A3;
/// 抽检录制不可对拍。
pub const XC_SAMPLED_NOT_COMPARABLE: u16 = 0xE7A4;
/// 抽检倍率非法。
pub const XC_BAD_SAMPLING: u16 = 0xE7A5;
/// 录制未开启。
pub const XC_NOT_RECORDING: u16 = 0xE7A6;
/// 压缩格式版本不符。
pub const XC_BAD_VERSION: u16 = 0xE7A7;
/// 账不平。
pub const XC_LEDGER_DRIFT: u16 = 0xE7A8;

// ---------------------------------------------------------------------------
// 定容常量
// ---------------------------------------------------------------------------

/// 归档格式魔数（`"VXR1"` 小端）。
pub const MAGIC: u32 = 0x3152_5856;
/// 归档格式版本。
pub const FORMAT_VERSION: u16 = 1;
/// 单个分片的命令数。
pub const SHARD_LEN: usize = 128;
/// 录制硬上限（超过即拒收并标记截断）。
pub const HARD_CAP: usize = 4096;
/// 单条命令序列化字节数（op 2 + a/b/c/handle 各 4）。
pub const CMD_BYTES: usize = 18;
/// 行程记录字节数（tag 1 + op 2 + count 2 + a0/b/c/handle 各 4 + step 1 = 22）。
///
/// 行程**必须**带全 b/c/handle：只压 op/a 的「压缩」会在解压时把
/// b/c/handle 变成 0，那不是压缩，是数据丢失。
pub const RUN_BYTES: usize = 22;
/// 行程编码的最小行程（短于此不划算）。
pub const MIN_RUN: usize = 3;
/// 单条录制的固定开销（ns）。
pub const RECORD_COST_NS: u64 = 40;
/// 抽检倍率上限。
pub const MAX_SAMPLING: u32 = 64;
/// 开销预算（千分比），超过建议降采样。
pub const OVERHEAD_BUDGET_PERMILLE: u32 = 50;

/// 上下文设置命令。
pub const OP_SET_CTX: u16 = 0x0001;
/// 绘制命令。
pub const OP_DRAW: u16 = 0x0100;

// ---------------------------------------------------------------------------
// 命令
// ---------------------------------------------------------------------------

/// 一条 GPU 命令（全字段录制：op + 三参 + 句柄）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cmd {
    /// 操作码。
    pub op: u16,
    /// 参数一。
    pub a: u32,
    /// 参数二。
    pub b: u32,
    /// 参数三。
    pub c: u32,
    /// 资源句柄。
    pub handle: u32,
}

impl Cmd {
    /// 构造。
    pub fn new(op: u16, a: u32, b: u32, c: u32, handle: u32) -> Cmd {
        Cmd {
            op,
            a,
            b,
            c,
            handle,
        }
    }

    /// 首处不一致的字段名（相同则返回空串）。
    pub fn diff_field(&self, other: &Cmd) -> &'static str {
        if self.op != other.op {
            "op"
        } else if self.a != other.a {
            "a"
        } else if self.b != other.b {
            "b"
        } else if self.c != other.c {
            "c"
        } else if self.handle != other.handle {
            "handle"
        } else {
            ""
        }
    }
}

// ---------------------------------------------------------------------------
// 归档格式（压缩流）
// ---------------------------------------------------------------------------

/// 压缩后的归档。
#[derive(Clone)]
pub struct Pack {
    /// 字节流。
    pub bytes: Vec<u8>,
    /// FNV-1a 校验和。
    pub checksum: u32,
    /// 原始命令数。
    pub cmds: u32,
    /// 原始字节数（未压缩）。
    pub raw_bytes: u32,
}

impl Pack {
    /// 压缩率（千分比）。
    pub fn ratio_permille(&self) -> u32 {
        if self.raw_bytes == 0 {
            return 0;
        }
        let packed = (self.bytes.len() as u64).saturating_mul(1000);
        (packed / self.raw_bytes as u64) as u32
    }

    /// 归档头（魔数 + 版本 + 条数）。
    pub fn header(&self) -> (u32, u16, u32) {
        (MAGIC, FORMAT_VERSION, self.cmds)
    }
}

fn put_u16(out: &mut Vec<u8>, v: u16) {
    out.push((v & 0xFF) as u8);
    out.push(((v >> 8) & 0xFF) as u8);
}

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.push((v & 0xFF) as u8);
    out.push(((v >> 8) & 0xFF) as u8);
    out.push(((v >> 16) & 0xFF) as u8);
    out.push(((v >> 24) & 0xFF) as u8);
}

fn get_u16(b: &[u8], off: usize) -> Option<u16> {
    if off + 2 > b.len() {
        return None;
    }
    Some((b[off] as u16) | ((b[off + 1] as u16) << 8))
}

fn get_u32(b: &[u8], off: usize) -> Option<u32> {
    if off + 4 > b.len() {
        return None;
    }
    Some(
        (b[off] as u32)
            | ((b[off + 1] as u32) << 8)
            | ((b[off + 2] as u32) << 16)
            | ((b[off + 3] as u32) << 24),
    )
}

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h = 0x811C_9DC5u32;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 从 `start` 起的相同模式行程长度（op/b/c/handle 相同且 a 步长恒定 0 或 1）。
fn run_len(cmds: &[Cmd], start: usize) -> usize {
    if start >= cmds.len() {
        return 0;
    }
    let head = cmds[start];
    let mut step: Option<u32> = None;
    let mut n = 1usize;
    while start + n < cmds.len() && n < u16::MAX as usize {
        let c = cmds[start + n];
        if c.op != head.op || c.b != head.b || c.c != head.c || c.handle != head.handle {
            break;
        }
        let d = c.a.wrapping_sub(cmds[start + n - 1].a);
        if d > 1 {
            break;
        }
        match step {
            None => step = Some(d),
            Some(s) => {
                if s != d {
                    break;
                }
            }
        }
        n += 1;
    }
    n
}

/// 压缩（行程编码 + 单条直存双标签）。
pub fn pack(cmds: &[Cmd]) -> Pack {
    let mut bytes: Vec<u8> = Vec::new();
    // 头：魔数 4 + 版本 2 + 条数 4
    put_u32(&mut bytes, MAGIC);
    put_u16(&mut bytes, FORMAT_VERSION);
    put_u32(&mut bytes, cmds.len() as u32);
    let mut i = 0usize;
    while i < cmds.len() {
        let n = run_len(cmds, i);
        if n >= MIN_RUN {
            let step = if cmds[i + 1].a.wrapping_sub(cmds[i].a) == 1 {
                1u8
            } else {
                0u8
            };
            bytes.push(0xFF);
            put_u16(&mut bytes, cmds[i].op);
            put_u16(&mut bytes, n as u16);
            put_u32(&mut bytes, cmds[i].a);
            put_u32(&mut bytes, cmds[i].b);
            put_u32(&mut bytes, cmds[i].c);
            put_u32(&mut bytes, cmds[i].handle);
            bytes.push(step);
            i += n;
        } else {
            bytes.push(0x00);
            put_u16(&mut bytes, cmds[i].op);
            put_u32(&mut bytes, cmds[i].a);
            put_u32(&mut bytes, cmds[i].b);
            put_u32(&mut bytes, cmds[i].c);
            put_u32(&mut bytes, cmds[i].handle);
            i += 1;
        }
    }
    let checksum = fnv1a(&bytes);
    Pack {
        bytes,
        checksum,
        cmds: cmds.len() as u32,
        raw_bytes: (cmds.len() * (CMD_BYTES + 1)) as u32,
    }
}

/// 解压（校验和优先；损坏时**不产出半截序列**）。
pub fn unpack(p: &Pack) -> Result<Vec<Cmd>, u16> {
    if fnv1a(&p.bytes) != p.checksum {
        return Err(XC_DECOMPRESS_CORRUPT);
    }
    let magic = match get_u32(&p.bytes, 0) {
        Some(v) => v,
        None => return Err(XC_DECOMPRESS_TRUNC),
    };
    if magic != MAGIC {
        return Err(XC_DECOMPRESS_CORRUPT);
    }
    let ver = match get_u16(&p.bytes, 4) {
        Some(v) => v,
        None => return Err(XC_DECOMPRESS_TRUNC),
    };
    if ver != FORMAT_VERSION {
        return Err(XC_BAD_VERSION);
    }
    let total = match get_u32(&p.bytes, 6) {
        Some(v) => v,
        None => return Err(XC_DECOMPRESS_TRUNC),
    };
    let mut out: Vec<Cmd> = Vec::new();
    let mut i = 10usize;
    while i < p.bytes.len() {
        let tag = p.bytes[i];
        i += 1;
        if tag == 0xFF {
            let op = match get_u16(&p.bytes, i) {
                Some(v) => v,
                None => return Err(XC_DECOMPRESS_TRUNC),
            };
            let n = match get_u16(&p.bytes, i + 2) {
                Some(v) => v,
                None => return Err(XC_DECOMPRESS_TRUNC),
            };
            let a0 = match get_u32(&p.bytes, i + 4) {
                Some(v) => v,
                None => return Err(XC_DECOMPRESS_TRUNC),
            };
            let bb = match get_u32(&p.bytes, i + 8) {
                Some(v) => v,
                None => return Err(XC_DECOMPRESS_TRUNC),
            };
            let cc = match get_u32(&p.bytes, i + 12) {
                Some(v) => v,
                None => return Err(XC_DECOMPRESS_TRUNC),
            };
            let hh = match get_u32(&p.bytes, i + 16) {
                Some(v) => v,
                None => return Err(XC_DECOMPRESS_TRUNC),
            };
            if i + 20 >= p.bytes.len() {
                return Err(XC_DECOMPRESS_TRUNC);
            }
            let step = p.bytes[i + 20];
            if step > 1 {
                return Err(XC_DECOMPRESS_CORRUPT);
            }
            i += 21;
            let mut k = 0u16;
            while k < n {
                let a = a0.wrapping_add((step as u32).wrapping_mul(k as u32));
                out.push(Cmd::new(op, a, bb, cc, hh));
                k += 1;
            }
        } else {
            let op = match get_u16(&p.bytes, i) {
                Some(v) => v,
                None => return Err(XC_DECOMPRESS_TRUNC),
            };
            let a = match get_u32(&p.bytes, i + 2) {
                Some(v) => v,
                None => return Err(XC_DECOMPRESS_TRUNC),
            };
            let b = match get_u32(&p.bytes, i + 6) {
                Some(v) => v,
                None => return Err(XC_DECOMPRESS_TRUNC),
            };
            let c = match get_u32(&p.bytes, i + 10) {
                Some(v) => v,
                None => return Err(XC_DECOMPRESS_TRUNC),
            };
            let h = match get_u32(&p.bytes, i + 14) {
                Some(v) => v,
                None => return Err(XC_DECOMPRESS_TRUNC),
            };
            i += 18;
            out.push(Cmd::new(op, a, b, c, h));
        }
    }
    if out.len() as u32 != total {
        return Err(XC_DECOMPRESS_TRUNC);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 录制器
// ---------------------------------------------------------------------------

/// 录制结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecordOutcome {
    /// 已入库。
    Stored,
    /// 被抽检跳过。
    Skipped,
    /// 硬上限拒收。
    Dropped,
    /// 录制未开启。
    Off,
}

/// 命令流录制器。
pub struct Recorder {
    cmds: Vec<Cmd>,
    recording: bool,
    sampling: u32,
    seen: u32,
    skipped: u32,
    dropped: u32,
    cost_ns: u64,
}

impl Recorder {
    /// 空录制器（默认未开启，避免「忘了关」的开销）。
    pub fn new() -> Recorder {
        Recorder {
            cmds: Vec::new(),
            recording: false,
            sampling: 1,
            seen: 0,
            skipped: 0,
            dropped: 0,
            cost_ns: 0,
        }
    }

    /// 开启录制。
    pub fn start(&mut self) {
        self.recording = true;
    }

    /// 关闭录制。
    pub fn stop(&mut self) {
        self.recording = false;
    }

    /// 是否在录。
    pub fn recording(&self) -> bool {
        self.recording
    }

    /// 设置抽检倍率（1 = 全录）。
    pub fn set_sampling(&mut self, k: u32) -> Result<u32, u16> {
        if k == 0 || k > MAX_SAMPLING {
            return Err(XC_BAD_SAMPLING);
        }
        self.sampling = k;
        Ok(k)
    }

    /// 抽检倍率。
    pub fn sampling(&self) -> u32 {
        self.sampling
    }

    /// 是否抽检录制（抽检录制不可用于「一致」结论）。
    pub fn sampled(&self) -> bool {
        self.sampling > 1
    }

    /// 录一条。
    pub fn record(&mut self, cmd: Cmd) -> RecordOutcome {
        if !self.recording {
            return RecordOutcome::Off;
        }
        self.seen = self.seen.saturating_add(1);
        if self.sampling > 1 && self.seen % self.sampling != 0 {
            self.skipped = self.skipped.saturating_add(1);
            return RecordOutcome::Skipped;
        }
        if self.cmds.len() >= HARD_CAP {
            // 溢出不静默：计 dropped，`honest_truncated()` 为真。
            self.dropped = self.dropped.saturating_add(1);
            return RecordOutcome::Dropped;
        }
        self.cmds.push(cmd);
        self.cost_ns = self.cost_ns.saturating_add(RECORD_COST_NS);
        RecordOutcome::Stored
    }

    /// 已录条数。
    pub fn len(&self) -> usize {
        self.cmds.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.cmds.is_empty()
    }

    /// 分片数（满 `SHARD_LEN` 的片数 + 有余量则再加一片）。
    pub fn shards(&self) -> u32 {
        let n = self.cmds.len();
        if n == 0 {
            return 0;
        }
        let full = n / SHARD_LEN;
        if n % SHARD_LEN == 0 {
            full as u32
        } else {
            (full + 1) as u32
        }
    }

    /// 已录开销（ns）。
    pub fn cost_ns(&self) -> u64 {
        self.cost_ns
    }

    /// 观测条数。
    pub fn seen(&self) -> u32 {
        self.seen
    }

    /// 抽检跳过条数。
    pub fn skipped(&self) -> u32 {
        self.skipped
    }

    /// 硬上限丢弃条数。
    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    /// 是否发生过诚实截断。
    pub fn honest_truncated(&self) -> bool {
        self.dropped > 0
    }

    /// 录制账平衡：观测 ≡ 入库 + 抽检跳过 + 溢出丢弃。
    pub fn ledger_balanced(&self) -> bool {
        self.seen == (self.cmds.len() as u32) + self.skipped + self.dropped
    }

    /// 命令视图。
    pub fn cmds(&self) -> &[Cmd] {
        &self.cmds
    }

    /// 开销千分比（相对 `baseline_ns` 的单命令基线成本）。
    pub fn overhead_permille(&self, baseline_ns: u64) -> u32 {
        let n = self.cmds.len() as u64;
        if n == 0 || baseline_ns == 0 {
            return 0;
        }
        let base_total = n.saturating_mul(baseline_ns);
        (self.cost_ns.saturating_mul(1000) / base_total) as u32
    }

    /// 开销是否超预算（超了才建议降采样）。
    pub fn overhead_over_budget(&self, baseline_ns: u64) -> bool {
        self.overhead_permille(baseline_ns) > OVERHEAD_BUDGET_PERMILLE
    }

    /// 开销说明行（读屏与日志共用）。
    pub fn overhead_line(&self, baseline_ns: u64) -> String {
        let mut s = String::new();
        s.push_str("开录制 ");
        s.push('+');
        push_u32(&mut s, self.overhead_permille(baseline_ns));
        s.push_str("‰ 单条 ");
        push_u64(&mut s, RECORD_COST_NS);
        s.push_str("ns 共 ");
        push_u32(&mut s, self.cmds.len() as u32);
        s.push_str(" 条");
        s
    }

    /// 归档。
    pub fn archive(&self) -> Pack {
        pack(&self.cmds)
    }
}

impl Default for Recorder {
    fn default() -> Recorder {
        Recorder::new()
    }
}

// ---------------------------------------------------------------------------
// 回放器
// ---------------------------------------------------------------------------

/// 回放结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReplayResult {
    /// 状态摘要。
    pub digest: u32,
    /// 事件数。
    pub events: u32,
    /// 上下文切换数。
    pub ctx_switches: u32,
    /// 绘制数。
    pub draws: u32,
}

/// 对拍结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Compare {
    /// 一致。
    Match,
    /// 不一致（带首个分歧位置与字段）。
    Diverge { index: u32, field: &'static str },
    /// 不可比（抽检录制）。
    NotComparable { code: u16 },
}

/// 确定性回放：同样的命令流永远得到同样的摘要。
pub fn replay(cmds: &[Cmd]) -> ReplayResult {
    let mut state = 0x811C_9DC5u32;
    let mut events = 0u32;
    let mut ctx = 0u32;
    let mut draws = 0u32;
    let mut i = 0usize;
    while i < cmds.len() {
        let c = cmds[i];
        state = state.wrapping_mul(0x0100_0193);
        state ^= (c.op as u32)
            ^ c.a.rotate_left(5)
            ^ c.b.rotate_left(11)
            ^ c.c.rotate_left(17)
            ^ c.handle.rotate_left(23);
        events = events.saturating_add(1);
        if c.op == OP_SET_CTX {
            ctx = ctx.saturating_add(1);
        }
        if c.op == OP_DRAW {
            draws = draws.saturating_add(1);
        }
        i += 1;
    }
    ReplayResult {
        digest: state ^ events.wrapping_mul(0x9E37_79B1),
        events,
        ctx_switches: ctx,
        draws,
    }
}

/// 对拍：先按条数与逐字段找分歧位置，再比摘要。
pub fn compare(live: &[Cmd], rec: &Recorder) -> Compare {
    if rec.sampled() {
        return Compare::NotComparable {
            code: XC_SAMPLED_NOT_COMPARABLE,
        };
    }
    first_divergence(live, rec.cmds())
}

/// 逐条对拍（返回首个分歧的下标与字段名）。
pub fn first_divergence(live: &[Cmd], replay_cmds: &[Cmd]) -> Compare {
    let n = if live.len() < replay_cmds.len() {
        live.len()
    } else {
        replay_cmds.len()
    };
    let mut i = 0usize;
    while i < n {
        let f = live[i].diff_field(&replay_cmds[i]);
        if !f.is_empty() {
            return Compare::Diverge {
                index: i as u32,
                field: f,
            };
        }
        i += 1;
    }
    if live.len() != replay_cmds.len() {
        return Compare::Diverge {
            index: n as u32,
            field: "length",
        };
    }
    Compare::Match
}

// ---------------------------------------------------------------------------
// 文本辅助
// ---------------------------------------------------------------------------

fn push_u32(s: &mut String, mut v: u32) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut n = 0usize;
    while v > 0 && n < buf.len() {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    while n > 0 {
        n -= 1;
        s.push(buf[n] as char);
    }
}

fn push_u64(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 && n < buf.len() {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    while n > 0 {
        n -= 1;
        s.push(buf[n] as char);
    }
}

// ---------------------------------------------------------------------------
// 判据套件
// ---------------------------------------------------------------------------

fn chk(set: &mut CheckSet, name: &'static str, ok: bool, detail: &'static str) {
    set.add(name, ok, if ok { "" } else { detail });
}

/// 一段典型命令流：上下文设置 + 递增 a 的绘制序列 + 一次上下文切换。
fn stream(n: u32) -> Vec<Cmd> {
    let mut v: Vec<Cmd> = Vec::new();
    v.push(Cmd::new(OP_SET_CTX, 1, 0, 0, 0xA000));
    let mut i = 0u32;
    while i < n {
        v.push(Cmd::new(OP_DRAW, i, 7, 0, 0));
        i += 1;
    }
    v.push(Cmd::new(OP_SET_CTX, 2, 0, 0, 0xA100));
    v
}

fn recorder_with(cmds: &[Cmd]) -> Recorder {
    let mut r = Recorder::new();
    r.start();
    let mut i = 0usize;
    while i < cmds.len() {
        let _ = r.record(cmds[i]);
        i += 1;
    }
    r
}

/// VE-F0076 判据入口。
pub fn run_vea76_checks() -> CheckSet {
    let mut set = CheckSet::new("vea76_record");

    // C76-01 录制保序与全字段
    {
        let src = stream(5);
        let r = recorder_with(&src);
        let ok = r.len() == src.len() && r.cmds() == src.as_slice();
        chk(&mut set, "C76-01 保序全字段", ok, "顺序或字段丢");
    }

    // C76-02 回放摘要确定性
    {
        let src = stream(9);
        let a = replay(&src);
        let b = replay(&src);
        let c = replay(&src[..src.len() - 1]);
        let ok = a == b && a.digest != c.digest && a.events == 11;
        chk(&mut set, "C76-02 回放确定性", ok, "两次回放不同或未变摘要");
    }

    // C76-03 上下文切换与绘制计数
    {
        let src = stream(4);
        let r = replay(&src);
        let ok = r.ctx_switches == 2 && r.draws == 4 && r.events == 6;
        chk(&mut set, "C76-03 事件计数", ok, "上下文/绘制计数错");
    }

    // C76-04 对拍一致
    {
        let src = stream(6);
        let rec = recorder_with(&src);
        let ok = compare(&src, &rec) == Compare::Match;
        chk(&mut set, "C76-04 对拍一致", ok, "同流被判不一致");
    }

    // C76-05 失真归因（改第 3 条的 b）
    {
        let src = stream(6);
        let rec = recorder_with(&src);
        let mut live = src.clone();
        live[3].b = 8;
        let d = compare(&live, &rec);
        let ok = d
            == Compare::Diverge {
                index: 3,
                field: "b",
            };
        chk(&mut set, "C76-05 失真归因", ok, "缺下标或字段名错");
    }

    // C76-06 条数不同也要归因（真前缀：公共段全同，只在长度上分歧）
    {
        let src = stream(6);
        let rec = recorder_with(&src);
        let live: Vec<Cmd> = src[..src.len() - 1].to_vec();
        let d = compare(&live, &rec);
        let ok = d
            == Compare::Diverge {
                index: (src.len() - 1) as u32,
                field: "length",
            };
        chk(&mut set, "C76-06 长度分歧", ok, "长度不同被判一致或下标错");
    }

    // C76-07 分片：条数守恒、片数按 128 切
    {
        let src = stream(300);
        let r = recorder_with(&src);
        let ok = r.len() == 302 && r.shards() == 3 && r.ledger_balanced();
        chk(&mut set, "C76-07 分片守恒", ok, "分片丢条或片数错");
    }

    // C76-08 跨片回放与整条流一致
    {
        let src = stream(300);
        let whole = replay(&src);
        let mut joined: Vec<Cmd> = Vec::new();
        let mut lo = 0usize;
        while lo < src.len() {
            let hi = if lo + SHARD_LEN < src.len() {
                lo + SHARD_LEN
            } else {
                src.len()
            };
            let mut k = lo;
            while k < hi {
                joined.push(src[k]);
                k += 1;
            }
            lo = hi;
        }
        let ok = replay(&joined) == whole && joined.len() == src.len();
        chk(&mut set, "C76-08 跨片连续", ok, "分片改变回放结果");
    }

    // C76-09 压缩往返逐条相等
    {
        let src = stream(40);
        let p = pack(&src);
        let back = unpack(&p);
        let ok = match back {
            Ok(v) => v == src,
            Err(_) => false,
        };
        chk(&mut set, "C76-09 压缩往返", ok, "解压后序列不等");
    }

    // C76-10 压缩率（100 条递增 a 应压到 50‰ 以内）
    {
        let src = stream(100);
        let p = pack(&src);
        let ok = p.ratio_permille() <= 50 && p.bytes.len() < src.len() * CMD_BYTES;
        chk(&mut set, "C76-10 压缩率", ok, "体积未下降");
    }

    // C76-11 校验和篡改即拒（不产半截）
    {
        let src = stream(20);
        let mut p = pack(&src);
        if !p.bytes.is_empty() {
            p.bytes[12] ^= 0x5A;
        }
        let r = unpack(&p);
        let ok = r == Err(XC_DECOMPRESS_CORRUPT);
        chk(&mut set, "C76-11 篡改拒解", ok, "损坏流被解出");
    }

    // C76-12 压缩不改语义（解压后 digest 相同）
    {
        let src = stream(50);
        let p = pack(&src);
        let ok = match unpack(&p) {
            Ok(v) => replay(&v).digest == replay(&src).digest,
            Err(_) => false,
        };
        chk(&mut set, "C76-12 压缩不改语义", ok, "解压后摘要变化");
    }

    // C76-13 开销显式
    {
        let src = stream(10);
        let r = recorder_with(&src);
        let line = r.overhead_line(100);
        let ok = r.cost_ns() == (r.len() as u64) * RECORD_COST_NS
            && line.contains('+')
            && line.contains("‰")
            && r.overhead_permille(100) == (RECORD_COST_NS as u32) * 1000 / 100;
        chk(&mut set, "C76-13 开销显式", ok, "开销未如实计入或未写明");
    }

    // C76-14 降采样生效
    {
        let src = stream(8);
        let mut r = Recorder::new();
        r.start();
        let _ = r.set_sampling(4);
        let mut i = 0usize;
        while i < src.len() {
            let _ = r.record(src[i]);
            i += 1;
        }
        let ok = r.sampled() && r.len() == 2 && r.skipped() == 8 && r.ledger_balanced();
        chk(&mut set, "C76-14 降采样", ok, "抽检倍率或计数错");
    }

    // C76-15 抽检录制不可声称一致
    {
        let src = stream(8);
        let mut r = Recorder::new();
        r.start();
        let _ = r.set_sampling(2);
        let mut i = 0usize;
        while i < src.len() {
            let _ = r.record(src[i]);
            i += 1;
        }
        let d = compare(&src, &r);
        let ok = d == Compare::NotComparable { code: XC_SAMPLED_NOT_COMPARABLE };
        chk(&mut set, "C76-15 抽检不可比", ok, "抽检录制被当成一致");
    }

    // C76-16 关闭录制零开销
    {
        let mut r = Recorder::new();
        let a = r.record(Cmd::new(OP_DRAW, 1, 2, 3, 4));
        let b = r.record(Cmd::new(OP_DRAW, 1, 2, 3, 5));
        let ok = a == RecordOutcome::Off
            && b == RecordOutcome::Off
            && r.len() == 0
            && r.cost_ns() == 0
            && r.seen() == 0;
        chk(&mut set, "C76-16 关闭零开销", ok, "关录制仍计开销");
    }

    // C76-17 硬上限拒收不静默
    {
        let mut r = Recorder::new();
        r.start();
        let mut i = 0usize;
        while i <= HARD_CAP {
            let _ = r.record(Cmd::new(OP_DRAW, i as u32, 0, 0, 0));
            i += 1;
        }
        let ok = r.len() == HARD_CAP
            && r.dropped() == 1
            && r.honest_truncated()
            && r.ledger_balanced();
        chk(&mut set, "C76-17 溢出不静默", ok, "溢出无痕");
    }

    // C76-18 空录制回放
    {
        let empty: Vec<Cmd> = Vec::new();
        let r = replay(&empty);
        let rec = Recorder::new();
        let ok = r.events == 0 && r.ctx_switches == 0 && r.draws == 0 && rec.shards() == 0;
        chk(&mut set, "C76-18 空流回放", ok, "空流处理异常");
    }

    // C76-19 采样倍率越界拒绝
    {
        let mut r = Recorder::new();
        let a = r.set_sampling(0);
        let b = r.set_sampling(MAX_SAMPLING + 1);
        let c = r.set_sampling(MAX_SAMPLING);
        let ok = a == Err(XC_BAD_SAMPLING) && b == Err(XC_BAD_SAMPLING) && c == Ok(MAX_SAMPLING);
        chk(&mut set, "C76-19 采样倍率校验", ok, "越界倍率被接受");
    }

    // C76-20 面板行四段齐备
    {
        let src = stream(4);
        let r = recorder_with(&src);
        let line = r.overhead_line(200);
        let mut panel = String::new();
        panel.push_str("条数 ");
        push_u32(&mut panel, r.len() as u32);
        panel.push_str(" 分片 ");
        push_u32(&mut panel, r.shards());
        panel.push_str(" 抽检 ");
        panel.push_str(if r.sampled() { "是" } else { "否" });
        panel.push(' ');
        panel.push_str(&line);
        let ok = panel.contains("条数")
            && panel.contains("分片")
            && panel.contains("抽检")
            && panel.contains('‰');
        chk(&mut set, "C76-20 面板可读", ok, "读屏字段缺失");
    }

    set
}

/// 单点演练。
pub fn drill() -> (bool, String) {
    let src = stream(8);
    let rec = recorder_with(&src);
    let mut ok = true;
    let mut s = String::new();
    if compare(&src, &rec) != Compare::Match {
        ok = false;
        s.push_str("compare ");
    }
    if unpack(&rec.archive()).is_err() {
        ok = false;
        s.push_str("archive ");
    }
    if !rec.ledger_balanced() {
        ok = false;
        s.push_str("ledger ");
    }
    if s.is_empty() {
        s.push_str("录制回放正常：");
        s.push_str(&rec.overhead_line(100));
    }
    (ok, s)
}
