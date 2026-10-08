//! VE-F0206 · virgl 命令流编码（VE-B 域 · GPU 驱动矩阵 · 目标 520 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0206`
//!
//! **规格原文**：virgl 通路命令流编码器——把 Gallium 风格渲染状态与绘制调用
//! 编码为 virgl 协议命令流（资源创建/绑定/绘制/同步四类命令），编码器按上下文
//! 序列化无并发态，大命令分块传输带续传语义；编码产物带版本戳便于跨版本回放
//! 与黄金流比对。错误路径与降级矩阵：句柄未创建即绑定→编码期拦截；缓冲满→
//! 分块续传不溢出；对端协议版本低→特性降级并标记。性能逐项分解：编码
//! O(命令数) 单遍；绑定去重省带宽；分块大小对齐页边界。跨批对接点：上游
//! F0205 上下文协商；下游 F0211 中断事件与 F0215 一致性测试黄金流比对。
//! 判据：四类命令、分块续传、版本戳、编码期拦截、判据。
//!
//! **设计要点**：
//! - 上下文是编码器的唯一世界：句柄表 + 绑定状态镜像都在上下文里，跨上下文
//!   不共享任何表——"按上下文序列化无并发态"由结构保证；
//! - 编码期拦截：句柄未创建即绑定、句柄重复创建，都在编码期拒绝（三要素），
//!   不把错误拖到设备侧才炸；
//! - 绑定去重：绑定状态镜像逐槽比对，同槽同句柄的重复绑定不发射（省带宽），
//!   去重次数入账——收益是数字不是感觉；
//! - 分块续传：分块容量按页边界（4K）对齐；每块头带版本戳 + 序号 + 前块
//!   令牌链（FNV），满即换块不溢出，接收侧可验证连续性；
//! - 版本戳贯穿：对端版本低 → 同步类命令按特性降级跳过并标记；对端版本高于
//!   本端 = 不兼容拒绝（诚实：不猜未来协议）；
//! - 黄金流比对：同命令序列 → 同编码字流 → 同 FNV 指纹；重组回读逐字一致。

use super::vea01_probe::fnv1a64;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、版本与分块常量
// ---------------------------------------------------------------------------

/// 本编码器产出的流版本（版本戳的值）。
pub const VIRGL_STREAM_VERSION: u32 = 1;
/// 分块容量：4KiB（页边界对齐，规格点名）。
pub const CHUNK_BYTES: usize = 4096;
/// 每块头 4 字：[version, seq, payload_len, prev_token]。
pub const CHUNK_HEADER_WORDS: usize = 4;
/// 每块载荷容量（字）：页对齐字节换算。
pub const CHUNK_PAYLOAD_WORDS: usize = CHUNK_BYTES / 4 - CHUNK_HEADER_WORDS;

/// 页对齐断言（分块纪律的机器可验形式）。
pub fn chunk_size_page_aligned() -> bool {
    CHUNK_BYTES % 4096 == 0
}

// ---------------------------------------------------------------------------
// 二、命令模型（Gallium 风格四类）
// ---------------------------------------------------------------------------

/// 命令类别（规格四类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmdClass {
    Create,
    Bind,
    Draw,
    Sync,
}

impl CmdClass {
    pub fn label(self) -> &'static str {
        match self {
            CmdClass::Create => "资源创建",
            CmdClass::Bind => "绑定",
            CmdClass::Draw => "绘制",
            CmdClass::Sync => "同步",
        }
    }
}

/// 冻结操作码表（只许追加，不许重排）。
pub const OP_CREATE_SURFACE: u32 = 1;
pub const OP_CREATE_BLEND: u32 = 2;
pub const OP_CREATE_DSA: u32 = 3;
pub const OP_CREATE_RASTERIZER: u32 = 4;
pub const OP_CREATE_SAMPLER_VIEW: u32 = 5;
pub const OP_CREATE_VERTEX_ELEMENTS: u32 = 6;
pub const OP_BIND_BLEND: u32 = 10;
pub const OP_BIND_DSA: u32 = 11;
pub const OP_BIND_RASTERIZER: u32 = 12;
pub const OP_BIND_SAMPLER_VIEW: u32 = 13;
pub const OP_BIND_INDEX_BUFFER: u32 = 14;
pub const OP_SET_FRAMEBUFFER: u32 = 15;
pub const OP_DRAW_VBO: u32 = 20;
pub const OP_DRAW_INDEXED: u32 = 21;
pub const OP_FLUSH: u32 = 30;
pub const OP_FENCE: u32 = 31;

/// 操作码 → 类别。未知操作码 None（不猜）。
pub fn op_class(op: u32) -> Option<CmdClass> {
    match op {
        OP_CREATE_SURFACE..=OP_CREATE_VERTEX_ELEMENTS => Some(CmdClass::Create),
        OP_BIND_BLEND..=OP_SET_FRAMEBUFFER => Some(CmdClass::Bind),
        OP_DRAW_VBO | OP_DRAW_INDEXED => Some(CmdClass::Draw),
        OP_FLUSH | OP_FENCE => Some(CmdClass::Sync),
        _ => None,
    }
}

/// 绑定槽位 → 操作码（冻结映射：槽位即语义）。
pub const SLOT_BLEND: u32 = 0;
pub const SLOT_DSA: u32 = 1;
pub const SLOT_RASTERIZER: u32 = 2;
pub const SLOT_SAMPLER_VIEW: u32 = 3;
pub const SLOT_INDEX_BUFFER: u32 = 4;
pub const SLOT_FRAMEBUFFER: u32 = 5;
pub const SLOT_COUNT: u32 = 6;

pub fn slot_bind_op(slot: u32) -> Option<u32> {
    match slot {
        SLOT_BLEND => Some(OP_BIND_BLEND),
        SLOT_DSA => Some(OP_BIND_DSA),
        SLOT_RASTERIZER => Some(OP_BIND_RASTERIZER),
        SLOT_SAMPLER_VIEW => Some(OP_BIND_SAMPLER_VIEW),
        SLOT_INDEX_BUFFER => Some(OP_BIND_INDEX_BUFFER),
        SLOT_FRAMEBUFFER => Some(OP_SET_FRAMEBUFFER),
        _ => None,
    }
}

/// virgl 命令（编码器的输入语言）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VirglCmd {
    /// 创建对象：句柄 + 对象操作码 + 参数字。
    CreateObject { handle: u32, op: u32, payload: Vec<u32> },
    /// 绑定：槽位 + 句柄。
    Bind { slot: u32, handle: u32 },
    /// 绘制：起始、数量、图元模式。
    Draw { start: u32, count: u32, mode: u32 },
    /// 同步：FLUSH 资源 / FENCE 栅栏号。
    Flush { resource_id: u32 },
    Fence { fence: u64 },
}

impl VirglCmd {
    pub fn class(&self) -> CmdClass {
        match self {
            VirglCmd::CreateObject { .. } => CmdClass::Create,
            VirglCmd::Bind { .. } => CmdClass::Bind,
            VirglCmd::Draw { .. } => CmdClass::Draw,
            VirglCmd::Flush { .. } | VirglCmd::Fence { .. } => CmdClass::Sync,
        }
    }

    pub fn spec_opcode(&self) -> u32 {
        match self {
            VirglCmd::CreateObject { op, .. } => *op,
            VirglCmd::Bind { slot, .. } => slot_bind_op(*slot).unwrap_or(0),
            VirglCmd::Draw { .. } => OP_DRAW_VBO,
            VirglCmd::Flush { .. } => OP_FLUSH,
            VirglCmd::Fence { .. } => OP_FENCE,
        }
    }

    /// 编码为字序列：[len, opcode, payload...]（len 含自身，字数）。
    /// O(命令) 单遍，无重扫。
    pub fn encode_words(&self) -> Result<Vec<u32>, EncodeError> {
        let mut w: Vec<u32> = Vec::new();
        match self {
            VirglCmd::CreateObject { handle, op, payload } => {
                if op_class(*op) != Some(CmdClass::Create) {
                    return Err(EncodeError::new(
                        "E_OP_NOT_CREATE",
                        format!("对象创建携带操作码 0x{:X} 不在创建族", op),
                        "创建命令只收创建族操作码——错族操作码在设备侧是未定义行为"
                            .to_string(),
                        "按冻结操作码表选用 CREATE_* 族".to_string(),
                    ));
                }
                w.push(*op);
                w.push(*handle);
                w.push(payload.len() as u32);
                w.extend_from_slice(payload);
            }
            VirglCmd::Bind { slot, handle } => {
                let op = slot_bind_op(*slot).ok_or_else(|| {
                    EncodeError::new(
                        "E_SLOT_UNKNOWN",
                        format!("绑定槽位 {} 不在冻结槽位表", slot),
                        "槽位即语义：未知槽位在设备侧无从解释绑定意图".to_string(),
                        format!("使用 0..{} 的冻结槽位（slot_bind_op 可查）", SLOT_COUNT),
                    )
                })?;
                w.push(op);
                w.push(*slot);
                w.push(*handle);
            }
            VirglCmd::Draw { start, count, mode } => {
                w.push(OP_DRAW_VBO);
                w.push(*start);
                w.push(*count);
                w.push(*mode);
            }
            VirglCmd::Flush { resource_id } => {
                w.push(OP_FLUSH);
                w.push(*resource_id);
            }
            VirglCmd::Fence { fence } => {
                w.push(OP_FENCE);
                w.push(*fence as u32);
                w.push((*fence >> 32) as u32);
            }
        }
        let mut out = vec![w.len() as u32 + 1];
        out.extend_from_slice(&w);
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// 三、编码错误（三要素，编码期拦截）
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodeError {
    pub code: &'static str,
    pub what: String,
    pub why: String,
    pub next: String,
}

impl EncodeError {
    fn new(code: &'static str, what: String, why: String, next: String) -> EncodeError {
        EncodeError { code, what, why, next }
    }
    pub fn is_complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 四、分块流（版本戳 + 续传令牌链）
// ---------------------------------------------------------------------------

/// 一块命令流：头（版本戳/序号/载荷长/前块令牌）+ 载荷字。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    pub version: u32,
    pub seq: u32,
    pub prev_token: u32,
    pub payload: Vec<u32>,
}

/// 块令牌：FNV64 高 32 位 ⊕ 低 32 位（seq + 载荷参与）。
/// 令牌链把块串成可验证的续传整体。
pub fn chunk_token(seq: u32, payload: &[u32]) -> u32 {
    let mut bytes = Vec::with_capacity(4 + payload.len() * 4);
    bytes.extend_from_slice(&seq.to_le_bytes());
    for w in payload.iter() {
        bytes.extend_from_slice(&w.to_le_bytes());
    }
    let h = fnv1a64(&bytes);
    (h >> 32) as u32 ^ h as u32
}

/// 分块命令流缓冲：满即换块，不溢出；块链可验证。
#[derive(Clone, Debug)]
pub struct ChunkedStream {
    pub version: u32,
    pub chunk_words: usize,
    chunks: Vec<Chunk>,
}

impl ChunkedStream {
    pub fn new(version: u32) -> ChunkedStream {
        ChunkedStream {
            version,
            chunk_words: CHUNK_PAYLOAD_WORDS,
            chunks: Vec::new(),
        }
    }

    pub fn chunks(&self) -> &[Chunk] {
        &self.chunks
    }

    pub fn total_payload_words(&self) -> usize {
        self.chunks.iter().map(|c| c.payload.len()).sum()
    }

    /// 追加载荷字：当前块装不下即封块开新块（分块续传不溢出）。
    /// 单遍切分，O(字数)。
    pub fn push_words(&mut self, words: &[u32]) {
        let mut rest = words;
        while !rest.is_empty() {
            let need_new = match self.chunks.last() {
                None => true,
                Some(c) => c.payload.len() >= self.chunk_words,
            };
            if need_new {
                let seq = self.chunks.len() as u32;
                let prev_token = match self.chunks.last() {
                    None => 0,
                    Some(c) => chunk_token(c.seq, &c.payload),
                };
                self.chunks.push(Chunk {
                    version: self.version,
                    seq,
                    prev_token,
                    payload: Vec::with_capacity(self.chunk_words),
                });
            }
            let last = self.chunks.last_mut().expect("上一步已确保有当前块");
            let room = self.chunk_words - last.payload.len();
            let take = room.min(rest.len());
            last.payload.extend_from_slice(&rest[..take]);
            rest = &rest[take..];
        }
    }

    /// 重组：按序拼接全部载荷（解码回读的基准）。
    pub fn reassemble(&self) -> Vec<u32> {
        let mut out = Vec::with_capacity(self.total_payload_words());
        for c in self.chunks() {
            out.extend_from_slice(&c.payload);
        }
        out
    }

    /// 验证块链：版本一致、序号连续、令牌链闭合。
    pub fn verify_chain(&self) -> Result<(), EncodeError> {
        let mut expect_prev = 0u32;
        for (i, c) in self.chunks().iter().enumerate() {
            if c.version != self.version {
                return Err(EncodeError::new(
                    "E_CHAIN_VERSION",
                    format!("第 {} 块版本 {} 与流版本 {} 不符", i, c.version, self.version),
                    "块头版本戳是跨版本回放的路标，混版即断链".to_string(),
                    "整流重编码，禁止手工拼接不同版本的块".to_string(),
                ));
            }
            if c.seq as usize != i {
                return Err(EncodeError::new(
                    "E_CHAIN_SEQ",
                    format!("第 {} 块序号 {} 不连续", i, c.seq),
                    "续传按序号对齐，跳号说明块丢失或乱序".to_string(),
                    "请求重传缺失块；接收侧不得跳号拼接".to_string(),
                ));
            }
            if c.prev_token != expect_prev {
                return Err(EncodeError::new(
                    "E_CHAIN_TOKEN",
                    format!("第 {} 块前令牌 {} 与期望 {} 不符", i, c.prev_token, expect_prev),
                    "令牌链把块串成整体，断链说明中间块被篡改或损坏".to_string(),
                    "从断点整段重传，不得跳过损坏块续传".to_string(),
                ));
            }
            expect_prev = chunk_token(c.seq, &c.payload);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 五、对端协议版本协商（低版本 → 特性降级并标记）
// ---------------------------------------------------------------------------

/// 对端兼容裁决。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeerCompat {
    /// 对端版本 = 本端：全特性。
    Full,
    /// 对端版本低：降级运行，缺失的命令类别已标记（编码时跳过并计数）。
    Downgrade { missing: Vec<&'static str> },
    /// 对端版本高于本端：不兼容（诚实拒绝，不猜未来协议）。
    Incompatible,
}

pub fn negotiate_peer(peer_version: u32) -> PeerCompat {
    if peer_version == VIRGL_STREAM_VERSION {
        PeerCompat::Full
    } else if peer_version < VIRGL_STREAM_VERSION {
        // 本端相对旧对端缺失的能力：同步类命令是 v1 引入语义，
        // 旧对端只认状态流——同步类降级跳过并标记。
        PeerCompat::Downgrade {
            missing: vec![CmdClass::Sync.label()],
        }
    } else {
        PeerCompat::Incompatible
    }
}

// ---------------------------------------------------------------------------
// 六、编码器上下文（句柄表 + 绑定状态镜像 + 降级标记）
// ---------------------------------------------------------------------------

/// 单条编码结果账目（黄金流比对与降级审计的证据链）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmitRecord {
    pub class: CmdClass,
    pub opcode: u32,
    pub words: usize,
    pub deduped: bool,
}

/// virgl 命令流编码器上下文。
///
/// 每上下文一个实例（无并发态）；stream 逐命令单遍追加。
pub struct EncoderContext {
    pub ctx_id: u32,
    /// 对端兼容裁决（低版本降级标记在此）。
    pub peer: PeerCompat,
    /// 对象句柄表：句柄 → 操作码（创建过的对象才可绑定）。
    objects: Vec<(u32, u32)>,
    /// 绑定状态镜像：槽位 → 当前句柄。
    bindings: [Option<u32>; SLOT_COUNT as usize],
    pub stream: ChunkedStream,
    pub emitted: Vec<EmitRecord>,
    pub dedup_count: u64,
    pub skipped_degraded: u64,
    pub next_handle_hint: u32,
}

impl EncoderContext {
    /// 绑定到 F0205 协商产出的 3D 上下文。
    pub fn new(ctx_id: u32, peer: PeerCompat) -> EncoderContext {
        EncoderContext {
            ctx_id,
            peer,
            objects: Vec::new(),
            bindings: [None; SLOT_COUNT as usize],
            stream: ChunkedStream::new(VIRGL_STREAM_VERSION),
            emitted: Vec::new(),
            dedup_count: 0,
            skipped_degraded: 0,
            next_handle_hint: 1,
        }
    }

    fn find_object(&self, handle: u32) -> Option<u32> {
        self.objects
            .iter()
            .find(|(h, _)| *h == handle)
            .map(|(_, op)| *op)
    }

    /// 资源创建：登记句柄表并编码。重复句柄编码期拦截。
    pub fn create(&mut self, handle: u32, op: u32, payload: &[u32]) -> Result<(), EncodeError> {
        if self.find_object(handle).is_some() {
            return Err(EncodeError::new(
                "E_DUP_HANDLE",
                format!("句柄 {} 已创建过，不允许重复创建", handle),
                "句柄表是唯一事实源：一物双名会让绑定语义失去对象身份".to_string(),
                "复用现有句柄，或先销毁再以新句柄创建".to_string(),
            ));
        }
        let cmd = VirglCmd::CreateObject {
            handle,
            op,
            payload: payload.to_vec(),
        };
        let words = cmd.encode_words()?;
        self.objects.push((handle, op));
        self.emit(&cmd, &words);
        self.next_handle_hint = self.next_handle_hint.max(handle + 1);
        Ok(())
    }

    /// 绑定：句柄必须已创建（编码期拦截）；同槽同句柄去重不发射。
    pub fn bind(&mut self, slot: u32, handle: u32) -> Result<(), EncodeError> {
        if slot >= SLOT_COUNT {
            return Err(EncodeError::new(
                "E_SLOT_UNKNOWN",
                format!("绑定槽位 {} 超出冻结槽位表（0..{}）", slot, SLOT_COUNT),
                "槽位即语义：未知槽位在设备侧无从解释绑定意图".to_string(),
                "查 slot_bind_op 的冻结槽位集后再绑定".to_string(),
            ));
        }
        let obj_op = self.find_object(handle).ok_or_else(|| {
            EncodeError::new(
                "E_HANDLE_NOT_CREATED",
                format!("句柄 {} 未创建即被绑定", handle),
                "绑定引用不存在的对象，设备侧必然返回无效句柄——错误应在编码期暴露"
                    .to_string(),
                "先 create 创建对象（拿句柄），再 bind 引用".to_string(),
            )
        })?;
        let _ = obj_op;
        // 绑定去重：镜像同槽同句柄 → 不发射（省带宽）。
        if self.bindings[slot as usize] == Some(handle) {
            self.dedup_count += 1;
            self.emitted.push(EmitRecord {
                class: CmdClass::Bind,
                opcode: slot_bind_op(slot).unwrap_or(0),
                words: 0,
                deduped: true,
            });
            return Ok(());
        }
        let cmd = VirglCmd::Bind { slot, handle };
        let words = cmd.encode_words()?;
        self.bindings[slot as usize] = Some(handle);
        self.emit(&cmd, &words);
        Ok(())
    }

    /// 绘制。
    pub fn draw(&mut self, start: u32, count: u32, mode: u32) -> Result<(), EncodeError> {
        let cmd = VirglCmd::Draw { start, count, mode };
        let words = cmd.encode_words()?;
        self.emit(&cmd, &words);
        Ok(())
    }

    /// 同步（FLUSH/FENCE）。对端低版本降级时跳过并标记（特性降级）。
    pub fn sync_flush(&mut self, resource_id: u32) -> Result<(), EncodeError> {
        let cmd = VirglCmd::Flush { resource_id };
        if let PeerCompat::Downgrade { missing } = &self.peer {
            if missing.contains(&CmdClass::Sync.label()) {
                self.skipped_degraded += 1;
                self.emitted.push(EmitRecord {
                    class: CmdClass::Sync,
                    opcode: OP_FLUSH,
                    words: 0,
                    deduped: false,
                });
                return Ok(());
            }
        }
        let words = cmd.encode_words()?;
        self.emit(&cmd, &words);
        Ok(())
    }

    fn emit(&mut self, cmd: &VirglCmd, words: &[u32]) {
        self.stream.push_words(words);
        self.emitted.push(EmitRecord {
            class: cmd.class(),
            opcode: cmd.spec_opcode(),
            words: words.len(),
            deduped: false,
        });
    }

    /// 黄金流指纹：重组字流的 FNV——同命令序列必同指纹（跨版本回放锚）。
    pub fn golden_hash(&self) -> u64 {
        let mut bytes = Vec::with_capacity(self.stream.total_payload_words() * 4);
        for w in self.stream.reassemble() {
            bytes.extend_from_slice(&w.to_le_bytes());
        }
        fnv1a64(&bytes)
    }

    /// 读屏摘要。
    pub fn a11y_summary(&self) -> String {
        let degraded = matches!(self.peer, PeerCompat::Downgrade { .. });
        format!(
            "virgl 命令流（上下文 {}）：已发射 {} 条、分块 {} 块、绑定去重 {} 次、\
降级跳过 {} 条（{}）",
            self.ctx_id,
            self.emitted.iter().filter(|e| !e.deduped).count(),
            self.stream.chunks().len(),
            self.dedup_count,
            self.skipped_degraded,
            if degraded { "对端低版本" } else { "全特性" }
        )
    }
}
