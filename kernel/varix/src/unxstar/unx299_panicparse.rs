//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0299 · panic 转储对端工具（构建侧解析器）· 诊断段 0x3d50
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定）：
//!   UNX-F0299-J1 解析器还原三件套结构（帧/日志/上下文），
//!   与转储黄金样本一致。
//! 难点分析：
//!   panic 转储是「唯一一次机会」的数据——写它的那台机器已经死了，
//!   解析器必须在不完整、被截断、被翻转比特的输入上给出可定位的错误，
//!   而不是崩溃或静默给出半真半假的结果。三件套（帧/日志/上下文）
//!   各自有不变量：帧栈链 rbp 单调可回溯、日志 seq 严格递增零跳号、
//!   上下文键闭集不重复。任何一处破坏都必须有独立诊断码。
//! 纪律约束：
//!   - 解析器纯只读：输入 &[u8]，零副作用，零 panic 面；
//!   - 每条记录独立 CRC，头部独立 CRC——坏一条不连坐整卷；
//!   - 错误码闭集：每个码附带「发生了什么 + 建议动作」；
//!   - 黄金样本确定性：同输入两次构建逐位相同。
//! 成本记账：解析 O(n)；CRC O(n)；对拍 O(记录数)。
//! ═══════════════════════════════════════════════════════════════════════

#![allow(dead_code)]

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（全仓普查 0x3d50 空闲后占段）。
pub const SEG_BASE: u16 = 0x3d50;

// ─────────────────────────────────────────────────────────────────────
// 一、诊断码（闭集：SEG_BASE + 序号）
// ─────────────────────────────────────────────────────────────────────

/// 解析错误码。显式判别式，code() 直达。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseErrCode {
    /// 魔数不对：这不是 Varix 转储卷。
    BadMagic = 1,
    /// 版本不认识：比本解析器新。
    BadVersion = 2,
    /// 头部 CRC 坏：卷在写头部时被截断或损坏。
    HeaderCrc = 3,
    /// 卷在记录中途被截断。
    Truncated = 4,
    /// 某条记录 CRC 坏。
    RecordCrc = 5,
    /// 记录类型不认识（比本解析器新的记录类型）。
    UnknownType = 6,
    /// 日志 seq 跳号：有日志在转储前丢了。
    SeqBroken = 7,
    /// 记录长度声明与 TLV 边界矛盾。
    BadLength = 8,
    /// 上下文键越出闭集。
    BadContextKey = 9,
}

impl ParseErrCode {
    /// u16 码值（SEG_BASE + 序号）。
    pub fn code(self) -> u16 {
        SEG_BASE + (self as u16)
    }

    /// 人话 + 可操作建议。
    pub fn detail(self) -> &'static str {
        match self {
            ParseErrCode::BadMagic =>
                "魔数不是 VDMP——拿错文件了。建议确认导出通道没有把非转储卷混进来。",
            ParseErrCode::BadVersion =>
                "版本号高于本解析器。建议升级解析器或用写入端同版本工具回读。",
            ParseErrCode::HeaderCrc =>
                "头部 CRC 不符——头部写入被打断。建议取同帧号的重试卷。",
            ParseErrCode::Truncated =>
                "卷在记录中途截断。建议按已还原前缀使用，截断点已给出。",
            ParseErrCode::RecordCrc =>
                "记录 CRC 不符——该条记录丢弃并入坏账，其余记录继续。",
            ParseErrCode::UnknownType =>
                "记录类型未登记。建议升级解析器；该条按未知账保留原始字节。",
            ParseErrCode::SeqBroken =>
                "日志 seq 跳号——转储窗口内有日志丢失。建议核对环形缓冲丢弃账。",
            ParseErrCode::BadLength =>
                "记录长度声明越界。建议整卷按损坏归档，不要部分信任。",
            ParseErrCode::BadContextKey =>
                "上下文键越出闭集——写入端混入了未登记键。建议登记新键后重解。",
        }
    }
}

// ─────────────────────────────────────────────────────────────────────
// 二、CRC32（软件实现，无查表依赖，可 const 化思想逐字节推进）
// ─────────────────────────────────────────────────────────────────────

/// CRC32（IEEE 多项式 0xEDB88320，反射形式）。
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

// ─────────────────────────────────────────────────────────────────────
// 三、卷格式常量与三件套记录
// ─────────────────────────────────────────────────────────────────────

/// 魔数 "VDMP"。
pub const MAGIC: [u8; 4] = [0x56, 0x44, 0x4D, 0x50];
/// 当前格式版本（只升不降；解析器接受 <= 自身版本）。
pub const FORMAT_VERSION: u16 = 1;
/// 头部长度：magic4 + version2 + record_count4 + header_crc4 = 14。
pub const HEADER_LEN: usize = 14;
/// 记录头长度：type1 + len2 + crc4 = 7。
pub const REC_HEADER_LEN: usize = 7;

/// 记录类型（闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecType {
    /// 帧记录：回溯栈帧。
    Frame = 1,
    /// 日志记录：panic 前最后写出的日志。
    Log = 2,
    /// 上下文记录：键值对现场。
    Context = 3,
}

impl RecType {
    fn from_u8(v: u8) -> Option<RecType> {
        match v {
            1 => Some(RecType::Frame),
            2 => Some(RecType::Log),
            3 => Some(RecType::Context),
            _ => None,
        }
    }
}

/// 上下文键闭集（0..=7，越出即 BadContextKey）。
pub const CONTEXT_KEY_CLOSED: [u8; 8] = [0, 1, 2, 3, 4, 5, 6, 7];

/// 帧记录（栈回溯一帧）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameRec {
    /// 返回地址。
    pub rip: u64,
    /// 栈指针。
    pub rsp: u64,
    /// 帧指针（用于链回溯）。
    pub rbp: u64,
    /// 代码段选择子。
    pub cs: u16,
    /// 标志位。
    pub rflags: u64,
    /// 中断向量（0 表示非中断入口）。
    pub vec: u8,
    /// 错误码（无则 0 并置 has_err=false）。
    pub err: u64,
    /// 是否带错误码。
    pub has_err: bool,
}

/// 日志记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRec {
    /// 全局序号（严格递增零跳号）。
    pub seq: u32,
    /// 级别（0=debug 1=info 2=warn 3=error）。
    pub level: u8,
    /// 消息正文（UTF-8 语义按字节保留）。
    pub msg: Vec<u8>,
}

/// 上下文记录。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CtxRec {
    /// 键（闭集 0..=7）。
    pub key: u8,
    /// 值。
    pub value: u64,
}

/// 解析结果三件套。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDump {
    /// 帧链（从最深到最浅）。
    pub frames: Vec<FrameRec>,
    /// 日志行。
    pub logs: Vec<LogRec>,
    /// 上下文键值。
    pub ctx: Vec<CtxRec>,
    /// 丢弃的坏记录账（偏移, 码）。
    pub bad_ledger: Vec<(usize, ParseErrCode)>,
    /// 截断点（None = 完整卷）。
    pub truncated_at: Option<usize>,
}

impl ParsedDump {
    /// 三件套完整且零坏账。
    pub fn clean(&self) -> bool {
        self.bad_ledger.is_empty() && self.truncated_at.is_none()
    }

    /// rbp 链回溯合法：每一帧 rbp 要么大于自身 rsp，要么为 0（链尾）。
    pub fn frame_chain_sane(&self) -> bool {
        self.frames.iter().all(|f| f.rbp == 0 || f.rbp > f.rsp)
    }

    /// 日志 seq 严格递增零跳号（跳号是解析期发现的坏账，不在此重复）。
    pub fn logs_seq_strict(&self) -> bool {
        self.logs.windows(2).all(|w| w[1].seq > w[0].seq)
    }
}

// ─────────────────────────────────────────────────────────────────────
// 四、写入端（构建黄金样本 + 序列化）
// ─────────────────────────────────────────────────────────────────────

/// 记录写入器。
pub struct DumpBuilder {
    /// 记录字节流（不含头部）。
    body: Vec<u8>,
    /// 日志 seq 游标。
    next_seq: u32,
}

impl DumpBuilder {
    /// 新建空卷。
    pub fn new() -> DumpBuilder {
        DumpBuilder { body: Vec::new(), next_seq: 0 }
    }

    fn push_rec(&mut self, ty: RecType, payload: &[u8]) {
        let crc = crc32(payload);
        self.body.push(ty as u8);
        self.body.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        self.body.extend_from_slice(&crc.to_le_bytes());
        self.body.extend_from_slice(payload);
    }

    /// 压帧。
    pub fn frame(&mut self, f: &FrameRec) {
        let mut p = Vec::new();
        p.extend_from_slice(&f.rip.to_le_bytes());
        p.extend_from_slice(&f.rsp.to_le_bytes());
        p.extend_from_slice(&f.rbp.to_le_bytes());
        p.extend_from_slice(&f.cs.to_le_bytes());
        p.extend_from_slice(&f.rflags.to_le_bytes());
        p.push(f.vec);
        let flags: u8 = if f.has_err { 1 } else { 0 };
        p.push(flags);
        if f.has_err {
            p.extend_from_slice(&f.err.to_le_bytes());
        }
        self.push_rec(RecType::Frame, &p);
    }

    /// 压日志（seq 自动分配，调用方不许自带）。
    pub fn log(&mut self, level: u8, msg: &[u8]) {
        let seq = self.next_seq;
        self.next_seq += 1;
        let mut p = Vec::new();
        p.extend_from_slice(&seq.to_le_bytes());
        p.push(level);
        p.extend_from_slice(&(msg.len() as u16).to_le_bytes());
        p.extend_from_slice(msg);
        self.push_rec(RecType::Log, &p);
    }

    /// 压上下文键值。
    pub fn ctx(&mut self, key: u8, value: u64) {
        let mut p = Vec::new();
        p.push(key);
        p.extend_from_slice(&value.to_le_bytes());
        self.push_rec(RecType::Context, &p);
    }

    /// 收卷：拼头部（count + 头部 CRC）。
    pub fn finish(self) -> Vec<u8> {
        let count = {
            // 记录数 = 扫描 body 数记录头
            let mut n = 0usize;
            let mut i = 0usize;
            while i + REC_HEADER_LEN <= self.body.len() {
                let len = u16::from_le_bytes([
                    self.body[i + 1], self.body[i + 2],
                ]) as usize;
                n += 1;
                i += REC_HEADER_LEN + len;
            }
            n
        };
        let mut head = Vec::new();
        head.extend_from_slice(&MAGIC);
        head.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        head.extend_from_slice(&(count as u32).to_le_bytes());
        let hc = crc32(&head);
        head.extend_from_slice(&hc.to_le_bytes());
        let mut out = head;
        out.extend_from_slice(&self.body);
        out
    }
}

impl Default for DumpBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────
// 五、读取端（解析器本体）
// ─────────────────────────────────────────────────────────────────────

/// 解析一个字节数卷。纯只读，坏记录降级入账不连坐。
pub fn parse(bytes: &[u8]) -> Result<ParsedDump, (ParseErrCode, usize)> {
    // 头部
    if bytes.len() < HEADER_LEN {
        return if bytes.len() >= 4 && bytes[0..4] != MAGIC {
            Err((ParseErrCode::BadMagic, 0))
        } else {
            Err((ParseErrCode::Truncated, bytes.len()))
        };
    }
    if bytes[0..4] != MAGIC {
        return Err((ParseErrCode::BadMagic, 0));
    }
    let ver = u16::from_le_bytes([bytes[4], bytes[5]]);
    if ver > FORMAT_VERSION {
        return Err((ParseErrCode::BadVersion, 4));
    }
    let count = u32::from_le_bytes([bytes[6], bytes[7], bytes[8], bytes[9]]) as usize;
    let head_crc = u32::from_le_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]);
    if crc32(&bytes[0..10]) != head_crc {
        return Err((ParseErrCode::HeaderCrc, 10));
    }

    let mut out = ParsedDump {
        frames: Vec::new(),
        logs: Vec::new(),
        ctx: Vec::new(),
        bad_ledger: Vec::new(),
        truncated_at: None,
    };

    let mut i = HEADER_LEN;
    let mut last_seq: Option<u32> = None;
    for _ in 0..count {
        // 记录头
        if i + REC_HEADER_LEN > bytes.len() {
            out.truncated_at = Some(i);
            return Ok(out);
        }
        let ty_raw = bytes[i];
        let len = u16::from_le_bytes([bytes[i + 1], bytes[i + 2]]) as usize;
        let rec_crc = u32::from_le_bytes([
            bytes[i + 3], bytes[i + 4], bytes[i + 5], bytes[i + 6],
        ]);
        let payload_at = i + REC_HEADER_LEN;
        if payload_at + len > bytes.len() {
            out.truncated_at = Some(payload_at);
            return Ok(out);
        }
        let payload = &bytes[payload_at..payload_at + len];
        let ty = match RecType::from_u8(ty_raw) {
            Some(t) => t,
            None => {
                out.bad_ledger.push((i, ParseErrCode::UnknownType));
                i = payload_at + len;
                continue;
            }
        };
        if crc32(payload) != rec_crc {
            out.bad_ledger.push((i, ParseErrCode::RecordCrc));
            i = payload_at + len;
            continue;
        }
        // 分类型解包
        match ty {
            RecType::Frame => {
                // 载荷布局：rip8 rsp8 rbp8 cs2 rflags8 vec1 flags1 [err8]
                if len < 36 || (payload[35] == 1 && len < 44) {
                    out.bad_ledger.push((i, ParseErrCode::BadLength));
                } else {
                    let g8 = |o: usize| u64::from_le_bytes([
                        payload[o], payload[o + 1], payload[o + 2], payload[o + 3],
                        payload[o + 4], payload[o + 5], payload[o + 6], payload[o + 7],
                    ]);
                    let has_err = payload[35] == 1;
                    out.frames.push(FrameRec {
                        rip: g8(0),
                        rsp: g8(8),
                        rbp: g8(16),
                        cs: (payload[24] as u16) | ((payload[25] as u16) << 8),
                        rflags: g8(26),
                        vec: payload[34],
                        err: if has_err { g8(36) } else { 0 },
                        has_err,
                    });
                }
            }
            RecType::Log => {
                if len < 7 {
                    out.bad_ledger.push((i, ParseErrCode::BadLength));
                } else {
                    let seq = u32::from_le_bytes([
                        payload[0], payload[1], payload[2], payload[3],
                    ]);
                    let level = payload[4];
                    let mlen = (payload[5] as usize) | ((payload[6] as usize) << 8);
                    if 7 + mlen > len {
                        out.bad_ledger.push((i, ParseErrCode::BadLength));
                    } else if let Some(ls) = last_seq {
                        if seq <= ls {
                            out.bad_ledger.push((i, ParseErrCode::SeqBroken));
                        } else {
                            last_seq = Some(seq);
                            out.logs.push(LogRec {
                                seq,
                                level,
                                msg: payload[7..7 + mlen].to_vec(),
                            });
                        }
                    } else {
                        last_seq = Some(seq);
                        out.logs.push(LogRec {
                            seq,
                            level,
                            msg: payload[7..7 + mlen].to_vec(),
                        });
                    }
                }
            }
            RecType::Context => {
                if len != 9 {
                    out.bad_ledger.push((i, ParseErrCode::BadLength));
                } else {
                    let key = payload[0];
                    if !CONTEXT_KEY_CLOSED.contains(&key) {
                        out.bad_ledger.push((i, ParseErrCode::BadContextKey));
                    } else {
                        let v = u64::from_le_bytes([
                            payload[1], payload[2], payload[3], payload[4],
                            payload[5], payload[6], payload[7], payload[8],
                        ]);
                        out.ctx.push(CtxRec { key, value: v });
                    }
                }
            }
        }
        i = payload_at + len;
    }
    Ok(out)
}

// ─────────────────────────────────────────────────────────────────────
// 六、黄金样本（确定性构建）
// ─────────────────────────────────────────────────────────────────────

/// 黄金样本帧链（3 帧，rbp 链单调）。
pub fn golden_frames() -> [FrameRec; 3] {
    [
        FrameRec {
            rip: 0xFFFF_8000_1000_0010,
            rsp: 0xFFFF_8000_0000_7F00,
            rbp: 0xFFFF_8000_0000_7F40,
            cs: 0x08,
            rflags: 0x0000_0246,
            vec: 0x0E,
            err: 0x0000_0002,
            has_err: true,
        },
        FrameRec {
            rip: 0xFFFF_8000_1002_0A40,
            rsp: 0xFFFF_8000_0000_7F50,
            rbp: 0xFFFF_8000_0000_7F80,
            cs: 0x08,
            rflags: 0x0000_0246,
            vec: 0,
            err: 0,
            has_err: false,
        },
        FrameRec {
            rip: 0xFFFF_8000_1003_FF00,
            rsp: 0xFFFF_8000_0000_7F90,
            rbp: 0,
            cs: 0x08,
            rflags: 0x0000_0246,
            vec: 0,
            err: 0,
            has_err: false,
        },
    ]
}

/// 黄金样本日志行。
pub fn golden_logs() -> [(&'static str, u8); 4] {
    [
        ("sched: tick 1024 budget rebalanced", 0),
        ("gpu: frame 512 submitted", 1),
        ("vm: page fault resolved at 0x7F00", 2),
        ("panic: cannot continue, dumping", 3),
    ]
}

/// 黄金样本上下文。
pub fn golden_ctx() -> [CtxRec; 5] {
    [
        CtxRec { key: 0, value: 1024 },      // tick
        CtxRec { key: 1, value: 512 },       // frame
        CtxRec { key: 2, value: 0x7F00 },    // fault addr
        CtxRec { key: 3, value: 4 },         // cpu count
        CtxRec { key: 4, value: 0x3D50 },    // dump seg
    ]
}

/// 构建黄金样本字节卷（确定性：同输入两次逐位相同）。
pub fn build_golden() -> Vec<u8> {
    let mut b = DumpBuilder::new();
    for f in golden_frames().iter() {
        b.frame(f);
    }
    for (m, lv) in golden_logs().iter() {
        b.log(*lv, m.as_bytes());
    }
    for c in golden_ctx().iter() {
        b.ctx(c.key, c.value);
    }
    b.finish()
}

// ─────────────────────────────────────────────────────────────────────
// 六·五、人读报告（解析结果 → 逐行文本，构建侧排查用）
// ─────────────────────────────────────────────────────────────────────

/// 渲染一卷解析结果为人读报告。纯函数：同输入同输出。
pub fn render_report(p: &ParsedDump) -> String {
    let mut s = String::new();
    s.push_str("== Varix panic 转储报告 ==\n");
    let status = if p.clean() { "完整" } else { "受损" };
    s.push_str(&format!("卷状态：{}\n", status));
    s.push_str(&format!("帧链：{} 帧\n", p.frames.len()));
    for (i, f) in p.frames.iter().enumerate() {
        s.push_str(&format!(
            "  #{:02} rip={:#018x} rbp={:#018x} vec={}{}\n",
            i,
            f.rip,
            f.rbp,
            f.vec,
            if f.has_err { " (带错误码)" } else { "" },
        ));
    }
    s.push_str(&format!("日志：{} 行\n", p.logs.len()));
    for l in p.logs.iter() {
        s.push_str(&format!(
            "  [{}] L{} {}\n",
            l.seq,
            l.level,
            core::str::from_utf8(&l.msg).unwrap_or("<非UTF8，按字节保留>"),
        ));
    }
    s.push_str(&format!("上下文：{} 键\n", p.ctx.len()));
    for c in p.ctx.iter() {
        s.push_str(&format!("  key={} value={:#x}\n", c.key, c.value));
    }
    if !p.bad_ledger.is_empty() {
        s.push_str(&format!("坏账：{} 条\n", p.bad_ledger.len()));
        for (off, code) in p.bad_ledger.iter() {
            s.push_str(&format!("  @{:#x} 码 {:#06x}：{}\n", off, code.code(), code.detail()));
        }
    }
    if let Some(at) = p.truncated_at {
        s.push_str(&format!("截断点：@{:#x}\n", at));
    }
    s
}

// ─────────────────────────────────────────────────────────────────────
// 七、判据入口
// ─────────────────────────────────────────────────────────────────────

/// UNX-F0299 判据：解析器还原三件套与黄金样本一致。
pub fn run_unx299_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0299 panic 转储对端解析器");
    let golden = build_golden();

    // J1 主判据：黄金样本完整解析
    let parsed = match parse(&golden) {
        Ok(p) => p,
        Err((c, at)) => {
            cs.add("黄金样本可解析", false, ParseErrCode::BadMagic.detail());
            let _ = (c, at);
            return cs;
        }
    };
    cs.add("J1 卷完整解析", parsed.clean(), "零坏账零截断");
    cs.add("三件套-帧数=3", parsed.frames.len() == 3, "帧还原");
    cs.add("三件套-日志=4", parsed.logs.len() == 4, "日志还原");
    cs.add("三件套-上下文=5", parsed.ctx.len() == 5, "上下文还原");
    cs.add("帧内容逐位一致", parsed.frames.as_slice() == golden_frames().as_slice(), "帧对拍");
    let logs_match = parsed
        .logs
        .iter()
        .zip(golden_logs().iter())
        .all(|(l, (m, lv))| l.msg.as_slice() == m.as_bytes() && l.level == *lv);
    cs.add("日志内容一致", logs_match, "日志对拍");
    let ctx_match = parsed
        .ctx
        .iter()
        .zip(golden_ctx().iter())
        .all(|(a, b)| a == b);
    cs.add("上下文一致", ctx_match, "上下文对拍");
    cs.add("日志 seq 严格递增", parsed.logs_seq_strict(), "0,1,2,3");
    cs.add("rbp 链合法", parsed.frame_chain_sane(), "单调回溯");

    // 确定性：两次构建逐位相同
    cs.add("黄金样本确定性", build_golden() == golden, "两次构建逐位同");

    // 往返：解析后再序列化 → 逐位同（解析器信息无损）
    let mut rb = DumpBuilder::new();
    for f in parsed.frames.iter() {
        rb.frame(f);
    }
    for l in parsed.logs.iter() {
        rb.log(l.level, &l.msg);
    }
    for c in parsed.ctx.iter() {
        rb.ctx(c.key, c.value);
    }
    let roundtrip = rb.finish();
    cs.add("往返序列化无损", roundtrip == golden, "parse→build 逐位同");

    // ── 注入核：五路破坏全部必须被抓，且码位准确 ──

    // 注入 1：魔数坏 → BadMagic
    let mut bad = golden.clone();
    bad[0] = 0x00;
    cs.add(
        "注入1 魔数坏被抓",
        matches!(parse(&bad), Err((ParseErrCode::BadMagic, 0))),
        ParseErrCode::BadMagic.detail(),
    );

    // 注入 2：版本超前瞻 → BadVersion
    let mut bad = golden.clone();
    bad[4] = 0xFF;
    bad[5] = 0xFF;
    cs.add(
        "注入2 版本超前被抓",
        matches!(parse(&bad), Err((ParseErrCode::BadVersion, 4))),
        ParseErrCode::BadVersion.detail(),
    );

    // 注入 3：头部字段翻转（count 改大）→ HeaderCrc
    let mut bad = golden.clone();
    bad[9] ^= 0x01;
    cs.add(
        "注入3 头CRC不符被抓",
        matches!(parse(&bad), Err((ParseErrCode::HeaderCrc, 10))),
        ParseErrCode::HeaderCrc.detail(),
    );

    // 注入 4：截断到中段 → truncated_at 落账且前缀三件套仍可信
    // 第一帧记录 = 头7 + 载荷44 = 51；截断在第一帧记录之后
    let cut = HEADER_LEN + REC_HEADER_LEN + 44;
    let pre = parse(&golden[..cut]);
    match pre {
        Ok(p) => {
            cs.add("注入4 截断被标记", p.truncated_at.is_some(), "截断点入账");
            cs.add("注入4 前缀帧可信", p.frames.len() == 1 && p.bad_ledger.is_empty(), "前缀不连坐");
        }
        Err(_) => cs.add("注入4 截断被标记", false, "截断误判为硬错"),
    }

    // 注入 5：日志载荷翻一比特 → RecordCrc 入坏账，其余记录继续
    // 第一帧记录 51 字节（7+44）；日志载荷 = seq4+level1+len2+正文35 = 42
    // 第一条日志记录头在 14+51=65，载荷起点 65+7=72
    let mut bad = golden.clone();
    bad[72] ^= 0x01;
    match parse(&bad) {
        Ok(p) => {
            let hit = p.bad_ledger.iter().any(|(_, c)| *c == ParseErrCode::RecordCrc);
            cs.add("注入5 载荷翻转入坏账", hit, "RecordCrc 定位准确");
            cs.add("注入5 其余日志不连坐", p.logs.len() == 3, "坏一保三");
        }
        Err(_) => cs.add("注入5 载荷翻转入坏账", false, "软错被升级为硬错"),
    }

    // 注入 6：日志 seq 重复（构造降序双日志）→ SeqBroken
    let mut dup = DumpBuilder::new();
    dup.log(1, b"first");
    dup.log(1, b"second"); // seq 复用 0 —— 解析器按 <= 判跳号
    let dup_vol = {
        // 直接手工构造 seq=1, seq=0 的卷（写入端自动分配做不到降序）
        let mut b = DumpBuilder::new();
        b.log(1, b"first");
        b.finish()
    };
    let _ = dup;
    // 在卷字节里把第二条日志的 seq 从 1 改回 0，同时重算该记录 CRC 使其只犯 seq 错
    // 简化路径：用解析器语义直接验证「seq 不增 → SeqBroken 入账」
    let mut bad = dup_vol.clone();
    // 第二条日志记录：定位需要扫描；用暴力方式——找 Log 类型字节后紧跟 len 的位置
    let mut pos = HEADER_LEN;
    let mut found = 0usize;
    while pos + REC_HEADER_LEN <= bad.len() {
        let len = u16::from_le_bytes([bad[pos + 1], bad[pos + 2]]) as usize;
        if bad[pos] == RecType::Log as u8 {
            found += 1;
            if found == 2 {
                let p0 = pos + REC_HEADER_LEN;
                // seq 置 0（与第一条重复），并重算 CRC 保持 CRC 门通过
                bad[p0] = 0;
                bad[p0 + 1] = 0;
                bad[p0 + 2] = 0;
                bad[p0 + 3] = 0;
                let crc = crc32(&bad[p0..p0 + len]);
                bad[pos + 3..pos + 7].copy_from_slice(&crc.to_le_bytes());
                break;
            }
        }
        pos += REC_HEADER_LEN + len;
    }
    match parse(&bad) {
        Ok(p) => {
            let hit = p.bad_ledger.iter().any(|(_, c)| *c == ParseErrCode::SeqBroken);
            cs.add("注入6 seq重复被抓", hit, "SeqBroken 语义准确");
        }
        Err(_) => cs.add("注入6 seq重复被抓", false, "软错误升级"),
    }

    // 人读报告：确定性 + 关键行在场
    let rpt1 = render_report(&parsed);
    let rpt2 = render_report(&parsed);
    cs.add("报告确定性", rpt1 == rpt2, "同输入同输出");
    cs.add("报告帧行在场", rpt1.contains("#00"), "帧编号");
    cs.add("报告日志行在场", rpt1.contains("[0] L0"), "seq+级别");
    cs.add("报告状态完整", rpt1.contains("完整"), "零坏账判定");
    cs.add("报告受损态可辨", render_report(&{
        let mut bad = golden.clone();
        bad[72] ^= 0x01;
        match parse(&bad) {
            Ok(p) => p,
            Err(_) => parsed.clone(),
        }
    }).contains("受损"), "坏账可见");

    // 诊断码闭集：九码互相不同且落在段内
    let codes = [
        ParseErrCode::BadMagic, ParseErrCode::BadVersion, ParseErrCode::HeaderCrc,
        ParseErrCode::Truncated, ParseErrCode::RecordCrc, ParseErrCode::UnknownType,
        ParseErrCode::SeqBroken, ParseErrCode::BadLength, ParseErrCode::BadContextKey,
    ];
    let mut uniq = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i].code() == codes[j].code() {
                uniq = false;
            }
        }
    }
    cs.add("诊断码九码互异", uniq, "闭集无重码");
    cs.add("诊断码全部带建议", codes.iter().all(|c| !c.detail().is_empty()), "detail 非空");

    cs
}
