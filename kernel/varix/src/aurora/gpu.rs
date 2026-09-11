//! AURORA-1000 AI-04 · GPU 驱动与硬件加速（A076~A100，W1）
//!
//! 三阶梯图形（软→虚→真）+ 真实 GPU 命令流。全部为纯逻辑建模：
//! 命令流编码器（固定容量环形命令缓冲）、命令包格式（NOP/DRAW/FLIP/WAIT）、
//! 命令解码回放校验、显存分配器（首适应 + 释放合并 + 泄漏检测）、
//! 三阶梯降级链（真 GPU→虚 GPU→软渲染的状态机与能力探测）、
//! 着色器字节码校验器、批处理提交（flush 边界）、错误恢复（超时复位状态机）、
//! 每阶梯合成加速策略判定。
//!
//! no_std / 仅 core：固定容量数组 + usize 计数，无分配、无 unsafe、无宏、无泛型魔法。
//! 真实硬件/寄存器访问只留接口位，命令流与分配器可在宿主机测试全绿验证。

use crate::checks::CheckSet;

// ===========================================================================
// 共享类型
// ===========================================================================

/// 三阶梯图形等级：真实 GPU / 虚拟 GPU / 软件光栅。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuTier {
    Real,
    Virtual,
    Soft,
}

impl GpuTier {
    pub fn name(self) -> &'static str {
        match self {
            GpuTier::Real => "real",
            GpuTier::Virtual => "virtual",
            GpuTier::Soft => "soft",
        }
    }
}

// ===========================================================================
// A076 真实 GPU 驱动框架
// ===========================================================================

/// 真实 GPU 设备模型：能力位 + 厂商 + 推荐等级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuDevice {
    pub real_present: bool,
    pub virtual_present: bool,
    pub software_present: bool,
    pub vendor_id: u16,
    pub name: &'static str,
}

impl GpuDevice {
    /// 该等级在当前设备模型上是否可用。
    pub fn tier_present(self, tier: GpuTier) -> bool {
        match tier {
            GpuTier::Real => self.real_present,
            GpuTier::Virtual => self.virtual_present,
            GpuTier::Soft => self.software_present,
        }
    }

    /// 能力探测后推荐的起始等级（优先真实 GPU）。
    pub fn recommended_tier(self) -> GpuTier {
        if self.real_present {
            GpuTier::Real
        } else if self.virtual_present {
            GpuTier::Virtual
        } else {
            GpuTier::Soft
        }
    }
}

/// 探测 GPU 设备能力（纯建模：软件光栅始终可用，虚拟与真实依模型标记）。
pub fn detect_gpu_device() -> GpuDevice {
    GpuDevice {
        real_present: true,
        virtual_present: true,
        software_present: true,
        vendor_id: 0x1234,
        name: "aurora-gpu-model",
    }
}

// ===========================================================================
// A077 命令缓冲提交 + 命令包格式
// ===========================================================================

pub const CMD_NOP: u32 = 0;
pub const CMD_DRAW: u32 = 1;
pub const CMD_FLIP: u32 = 2;
pub const CMD_WAIT: u32 = 3;

pub const CMD_RING_CAP: usize = 256;

/// 解码后的命令。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuCommand {
    Nop,
    Draw { vertices: u16, tex: u8 },
    Flip { vsync: bool },
    Wait { ms: u32 },
}

/// 取命令字低 4 位 opcode。
pub fn opcode_of(word: u32) -> u32 {
    word & 0xF
}

/// 编码 NOP。
pub fn encode_nop() -> u32 {
    CMD_NOP
}

/// 编码 DRAW：opcode=1，顶点数占 bits[4..20)，纹理槽占 bits[20..28)。
pub fn encode_draw(vertices: u16, tex: u8) -> u32 {
    CMD_DRAW | ((vertices as u32) << 4) | ((tex as u32) << 20)
}

/// 编码 FLIP：opcode=2，vsync 占 bit4。
pub fn encode_flip(vsync: bool) -> u32 {
    CMD_FLIP | ((vsync as u32) << 4)
}

/// 编码 WAIT：opcode=3，超时(ms)占 bits[4..32)。
pub fn encode_wait(ms: u32) -> u32 {
    CMD_WAIT | ((ms & 0x0FFF_FFFF) << 4)
}

/// 解码命令字；未知 opcode 安全降级为 Nop（绝不越界/panic）。
pub fn decode_command(word: u32) -> GpuCommand {
    match opcode_of(word) {
        CMD_NOP => GpuCommand::Nop,
        CMD_DRAW => GpuCommand::Draw {
            vertices: ((word >> 4) & 0xFFFF) as u16,
            tex: ((word >> 20) & 0xFF) as u8,
        },
        CMD_FLIP => GpuCommand::Flip {
            vsync: (word >> 4) & 1 == 1,
        },
        CMD_WAIT => GpuCommand::Wait {
            ms: (word >> 4) & 0x0FFF_FFFF,
        },
        _ => GpuCommand::Nop,
    }
}

/// 将解码结果重新编码（用于回放校验的往返一致性）。
pub fn reencode(cmd: GpuCommand) -> u32 {
    match cmd {
        GpuCommand::Nop => encode_nop(),
        GpuCommand::Draw { vertices, tex } => encode_draw(vertices, tex),
        GpuCommand::Flip { vsync } => encode_flip(vsync),
        GpuCommand::Wait { ms } => encode_wait(ms),
    }
}

/// 命令字是否为合法 opcode（0..3）。
pub fn is_valid_command_word(word: u32) -> bool {
    opcode_of(word) <= 3
}

/// 固定容量环形命令缓冲：写指针/读指针/长度，满判空。
#[derive(Clone, Copy, Debug)]
pub struct CommandRing {
    buf: [u32; CMD_RING_CAP],
    write: usize,
    read: usize,
    len: usize,
}

impl CommandRing {
    pub const fn new() -> CommandRing {
        CommandRing {
            buf: [0u32; CMD_RING_CAP],
            write: 0,
            read: 0,
            len: 0,
        }
    }

    /// 写入一个命令字；满则返回 false。
    pub fn push(&mut self, word: u32) -> bool {
        if self.len >= CMD_RING_CAP {
            return false;
        }
        self.buf[self.write] = word;
        self.write = (self.write + 1) % CMD_RING_CAP;
        self.len += 1;
        true
    }

    /// 弹出一个命令字；空则返回 None。
    pub fn pop(&mut self) -> Option<u32> {
        if self.len == 0 {
            return None;
        }
        let w = self.buf[self.read];
        self.read = (self.read + 1) % CMD_RING_CAP;
        self.len -= 1;
        Some(w)
    }

    pub fn len(self) -> usize {
        self.len
    }

    pub fn is_full(self) -> bool {
        self.len >= CMD_RING_CAP
    }

    pub fn is_empty(self) -> bool {
        self.len == 0
    }
}

/// 命令流解码回放校验：每个字经「解码→重编码」必须还原，否则判损。
pub fn replay_validate(words: &[u32]) -> bool {
    let mut i = 0;
    while i < words.len() {
        if reencode(decode_command(words[i])) != words[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 便捷构造：A077 的对外构造器。
pub fn command_ring_new() -> CommandRing {
    CommandRing::new()
}

// ===========================================================================
// A078 着色器编译（字节码校验器）
// ===========================================================================

pub const SHADER_MAGIC: [u8; 4] = *b"AVXS";
pub const SHADER_VERSION: u16 = 1;
pub const SHADER_MAX_INSTRS: u16 = 4096;
/// 每条指令固定 4 字节。
pub const SHADER_INSTR_BYTES: usize = 4;

/// 着色器校验结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shader {
    pub magic_ok: bool,
    pub version_ok: bool,
    pub instrs: u16,
    pub valid: bool,
    pub bytecode_len: usize,
}

/// 校验着色器字节码：魔数 / 版本 / 指令数界限 / 长度一致。
pub fn validate_shader(bytes: &[u8]) -> Shader {
    let mut s = Shader {
        magic_ok: false,
        version_ok: false,
        instrs: 0,
        valid: false,
        bytecode_len: bytes.len(),
    };
    if bytes.len() < 8 {
        return s;
    }
    let mut magic = [0u8; 4];
    magic.copy_from_slice(&bytes[0..4]);
    s.magic_ok = magic == SHADER_MAGIC;
    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    let instrs = u16::from_le_bytes([bytes[6], bytes[7]]);
    s.version_ok = version == SHADER_VERSION;
    s.instrs = instrs;
    let expected = 8usize + (instrs as usize) * SHADER_INSTR_BYTES;
    s.valid =
        s.magic_ok && s.version_ok && instrs <= SHADER_MAX_INSTRS && bytes.len() == expected;
    s
}

/// 简单字节码哈希（用作着色器句柄），纯加权和。
pub fn shader_hash(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 编译着色器：校验通过才返回句柄。
pub fn compile_shader(bytes: &[u8]) -> Option<u32> {
    if validate_shader(bytes).valid {
        Some(shader_hash(bytes))
    } else {
        None
    }
}

// ===========================================================================
// A079 显存分配器（首适应 + 释放合并 + 泄漏检测）
// ===========================================================================

pub const VRAM_MAX_BLOCKS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockState {
    Free,
    Used,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VramBlock {
    pub state: BlockState,
    pub offset: u32,
    pub size: u32,
    pub tag: u8,
}

/// 固定池显存分配器：按偏移有序的块表，首适应分配、相邻空闲合并、分配/释放计数配对检测泄漏。
#[derive(Clone, Copy, Debug)]
pub struct VramAllocator {
    blocks: [VramBlock; VRAM_MAX_BLOCKS],
    count: usize,
    used_bytes: u32,
    alloc_count: u32,
    free_count: u32,
    total_bytes: u32,
}

impl VramAllocator {
    pub const fn new(total_bytes: u32) -> VramAllocator {
        let mut a = VramAllocator {
            blocks: [VramBlock {
                state: BlockState::Free,
                offset: 0,
                size: 0,
                tag: 0,
            }; VRAM_MAX_BLOCKS],
            count: 1,
            used_bytes: 0,
            alloc_count: 0,
            free_count: 0,
            total_bytes,
        };
        a.blocks[0] = VramBlock {
            state: BlockState::Free,
            offset: 0,
            size: total_bytes,
            tag: 0,
        };
        a
    }

    /// 首适应分配：返回分配块偏移；池满或不足返回 None。
    pub fn alloc(&mut self, size: u32, tag: u8) -> Option<u32> {
        if size == 0 {
            return None;
        }
        let mut i = 0;
        while i < self.count {
            let b = self.blocks[i];
            if b.state == BlockState::Free && b.size >= size {
                let used_off = b.offset;
                if b.size == size {
                    self.blocks[i].state = BlockState::Used;
                    self.blocks[i].tag = tag;
                } else {
                    if self.count >= VRAM_MAX_BLOCKS {
                        return None;
                    }
                    // 在 i 处插入已用块，原空闲块向右收缩。
                    let mut j = self.count;
                    while j > i {
                        self.blocks[j] = self.blocks[j - 1];
                        j -= 1;
                    }
                    self.blocks[i] = VramBlock {
                        state: BlockState::Used,
                        offset: used_off,
                        size,
                        tag,
                    };
                    self.blocks[i + 1].offset = used_off + size;
                    self.blocks[i + 1].size = b.size - size;
                    self.count += 1;
                }
                self.alloc_count += 1;
                self.used_bytes = self.used_bytes.saturating_add(size);
                return Some(used_off);
            }
            i += 1;
        }
        None
    }

    /// 释放某偏移的已用块；双释放/未知偏移返回 false（不重复计数）。
    pub fn free(&mut self, offset: u32) -> bool {
        let mut idx = None;
        let mut i = 0;
        while i < self.count {
            if self.blocks[i].offset == offset && self.blocks[i].state == BlockState::Used {
                idx = Some(i);
                break;
            }
            i += 1;
        }
        let i = match idx {
            Some(x) => x,
            None => return false,
        };
        self.used_bytes = self.used_bytes.saturating_sub(self.blocks[i].size);
        self.blocks[i].state = BlockState::Free;
        self.blocks[i].tag = 0;
        self.free_count += 1;
        self.merge_free();
        true
    }

    /// 合并相邻空闲块（偏移连续）。
    fn merge_free(&mut self) {
        let mut i = 0;
        while i + 1 < self.count {
            let cur = self.blocks[i];
            let nxt = self.blocks[i + 1];
            if cur.state == BlockState::Free
                && nxt.state == BlockState::Free
                && cur.offset + cur.size == nxt.offset
            {
                self.blocks[i].size = cur.size + nxt.size;
                let mut j = i + 1;
                while j + 1 < self.count {
                    self.blocks[j] = self.blocks[j + 1];
                    j += 1;
                }
                self.count -= 1;
            } else {
                i += 1;
            }
        }
    }

    pub fn block_count(self) -> usize {
        self.count
    }

    pub fn used_bytes(self) -> u32 {
        self.used_bytes
    }

    /// 泄漏检测：分配计数与释放计数必须配对且已用字节归零。
    pub fn has_leak(self) -> bool {
        self.alloc_count != self.free_count || self.used_bytes != 0
    }
}

/// A079 对外构造器。
pub fn vram_new(total_bytes: u32) -> VramAllocator {
    VramAllocator::new(total_bytes)
}

// ===========================================================================
// A080 纹理上传
// ===========================================================================

pub const MAX_TEX_DIM: u32 = 16384;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextureHandle {
    pub id: u32,
    pub w: u16,
    pub h: u16,
    pub fmt: u8,
}

fn tex_hash(w: u16, h: u16, fmt: u8) -> u32 {
    ((w as u32) << 16) ^ ((h as u32) << 2) ^ (fmt as u32)
}

/// 上传纹理：维度与格式边界校验，返回句柄。
pub fn upload_texture(w: u16, h: u16, fmt: u8) -> Option<TextureHandle> {
    if w == 0 || h == 0 || (w as u32) > MAX_TEX_DIM || (h as u32) > MAX_TEX_DIM {
        return None;
    }
    Some(TextureHandle {
        id: tex_hash(w, h, fmt),
        w,
        h,
        fmt,
    })
}

// ===========================================================================
// A081 顶点/索引缓冲
// ===========================================================================

pub const MAX_BUFFER_BYTES: u32 = 256 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferKind {
    Vertex,
    Index,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferHandle {
    pub id: u32,
    pub kind: BufferKind,
    pub bytes: u32,
}

fn buf_hash(kind: BufferKind, bytes: u32) -> u32 {
    ((kind as u32) << 28) ^ bytes
}

/// 创建顶点缓冲。
pub fn create_vertex_buffer(bytes: u32) -> Option<BufferHandle> {
    if bytes == 0 || bytes > MAX_BUFFER_BYTES {
        return None;
    }
    Some(BufferHandle {
        id: buf_hash(BufferKind::Vertex, bytes),
        kind: BufferKind::Vertex,
        bytes,
    })
}

/// 创建索引缓冲。
pub fn create_index_buffer(bytes: u32) -> Option<BufferHandle> {
    if bytes == 0 || bytes > MAX_BUFFER_BYTES {
        return None;
    }
    Some(BufferHandle {
        id: buf_hash(BufferKind::Index, bytes),
        kind: BufferKind::Index,
        bytes,
    })
}

// ===========================================================================
// A082 GPU 调度（批处理提交 / flush 边界）
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuScheduler {
    pub pending_words: usize,
    pub flushed_batches: usize,
    pub flush_threshold: usize,
}

impl GpuScheduler {
    pub const fn new(flush_threshold: usize) -> GpuScheduler {
        GpuScheduler {
            pending_words: 0,
            flushed_batches: 0,
            flush_threshold: if flush_threshold == 0 { 1 } else { flush_threshold },
        }
    }

    /// 入队 `n` 个命令字；越过 flush 边界时提交一批并返回 true。
    pub fn enqueue(&mut self, n: usize) -> bool {
        self.pending_words = self.pending_words.saturating_add(n);
        if self.pending_words >= self.flush_threshold {
            self.pending_words = 0;
            self.flushed_batches += 1;
            true
        } else {
            false
        }
    }

    pub fn pending(self) -> usize {
        self.pending_words
    }
}

/// A082 对外构造器。
pub fn scheduler_new(flush_threshold: usize) -> GpuScheduler {
    GpuScheduler::new(flush_threshold)
}

// ===========================================================================
// A083 合成器硬件加速
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompositorAccel {
    pub can_blit: bool,
    pub can_blur: bool,
    pub can_shadow: bool,
    pub can_scale: bool,
}

/// 合成器硬件能力：真实 GPU 全开，虚拟 GPU 仅 blit/scale，软件全软。
pub fn compositor_capabilities(tier: GpuTier) -> CompositorAccel {
    match tier {
        GpuTier::Real => CompositorAccel {
            can_blit: true,
            can_blur: true,
            can_shadow: true,
            can_scale: true,
        },
        GpuTier::Virtual => CompositorAccel {
            can_blit: true,
            can_blur: false,
            can_shadow: false,
            can_scale: true,
        },
        GpuTier::Soft => CompositorAccel {
            can_blit: false,
            can_blur: false,
            can_shadow: false,
            can_scale: false,
        },
    }
}

// ===========================================================================
// A084 视频解码硬件加速
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoCodec {
    H264,
    VP9,
    AV1,
}

/// 视频解码硬件加速能力矩阵。
pub fn video_decode_accel(tier: GpuTier, codec: VideoCodec) -> bool {
    match tier {
        GpuTier::Real => true,
        GpuTier::Virtual => matches_vp9_h264(codec),
        GpuTier::Soft => false,
    }
}

fn matches_vp9_h264(codec: VideoCodec) -> bool {
    codec == VideoCodec::H264 || codec == VideoCodec::VP9
}

// ===========================================================================
// A085 2D 加速接口
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Accel2dOp {
    Fill,
    Blit,
    Stretch,
}

/// 2D 加速接口能力：真实全开，虚拟仅 Fill/Blit，软件无。
pub fn accel2d(tier: GpuTier, op: Accel2dOp) -> bool {
    match tier {
        GpuTier::Real => true,
        GpuTier::Virtual => op == Accel2dOp::Fill || op == Accel2dOp::Blit,
        GpuTier::Soft => false,
    }
}

// ===========================================================================
// A086 虚拟 GPU 设备
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VirtualGpu {
    pub backend: &'static str,
    pub vram_mb: u32,
    pub queues: u8,
}

/// 探测虚拟 GPU（paravirt 后端模型）。
pub fn detect_virtual_gpu() -> Option<VirtualGpu> {
    Some(VirtualGpu {
        backend: "virtio-gpu",
        vram_mb: 256,
        queues: 4,
    })
}

// ===========================================================================
// A087 软件光栅回退
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoftRaster {
    pub threads: u8,
    pub max_dim: u32,
}

/// 软件光栅回退能力（始终可用）。
pub fn software_raster_capable() -> SoftRaster {
    SoftRaster {
        threads: 4,
        max_dim: 16384,
    }
}

// ===========================================================================
// A088 GPU 功耗管理
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuPowerState {
    pub clock_mhz: u32,
    pub milliwatts: u32,
}

/// 按负载(0..1000)与等级给出 GPU 时钟/功耗步进。
pub fn gpu_power_step(tier: GpuTier, load_permille: u16) -> GpuPowerState {
    match tier {
        GpuTier::Real => {
            let clock = 200u32 + (load_permille as u32) * 2;
            GpuPowerState {
                clock_mhz: clock,
                milliwatts: clock / 10 + 50,
            }
        }
        GpuTier::Virtual => {
            let clock = 400u32 + (load_permille as u32);
            GpuPowerState {
                clock_mhz: clock,
                milliwatts: clock / 8,
            }
        }
        GpuTier::Soft => GpuPowerState {
            clock_mhz: 200,
            milliwatts: 300,
        },
    }
}

// ===========================================================================
// A089 GPU 崩溃隔离（超时复位状态机）
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetState {
    Healthy,
    Detecting,
    Recovering,
    Recovered,
}

#[derive(Clone, Copy, Debug)]
pub struct ResetFsm {
    pub state: ResetState,
    pub ticks: u32,
    pub deadline_ms: u32,
}

impl ResetFsm {
    pub const fn new(deadline_ms: u32) -> ResetFsm {
        ResetFsm {
            state: ResetState::Healthy,
            ticks: 0,
            deadline_ms: if deadline_ms == 0 { 1 } else { deadline_ms },
        }
    }

    /// 推进状态机：`hung` 触发探测，超时进入恢复，最后回到健康。
    pub fn reset_tick(&mut self, ms: u32, hung: bool) -> ResetState {
        match self.state {
            ResetState::Healthy => {
                if hung {
                    self.state = ResetState::Detecting;
                    self.ticks = 0;
                }
            }
            ResetState::Detecting => {
                self.ticks = self.ticks.saturating_add(ms);
                if self.ticks >= self.deadline_ms {
                    self.state = ResetState::Recovering;
                    self.ticks = 0;
                }
            }
            ResetState::Recovering => {
                self.state = ResetState::Recovered;
            }
            ResetState::Recovered => {
                self.state = ResetState::Healthy;
            }
        }
        self.state
    }
}

/// A089 对外构造器。
pub fn reset_fsm_new(deadline_ms: u32) -> ResetFsm {
    ResetFsm::new(deadline_ms)
}

// ===========================================================================
// A090 GPU 直通安全
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuPassthrough {
    pub iommu: bool,
    pub dma_isolated: bool,
}

/// GPU 直通安全性：需 IOMMU 且 DMA 隔离均开启。
pub fn passthrough_safe(p: GpuPassthrough) -> bool {
    p.iommu && p.dma_isolated
}

// ===========================================================================
// A091 GPU 性能基准
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuBenchmark {
    pub score: u32,
    pub fps: u16,
}

/// 各等级性能基准（真实 > 虚拟 > 软件）。
pub fn benchmark(tier: GpuTier) -> GpuBenchmark {
    match tier {
        GpuTier::Real => GpuBenchmark { score: 1000, fps: 240 },
        GpuTier::Virtual => GpuBenchmark { score: 400, fps: 120 },
        GpuTier::Soft => GpuBenchmark { score: 60, fps: 30 },
    }
}

// ===========================================================================
// A092 GPU 兼容矩阵
// ===========================================================================

pub const KNOWN_VENDORS: [u16; 8] = [0x10DE, 0x1022, 0x8086, 0x1002, 0x1A86, 0x1234, 0x15AD, 0x1AF4];

/// 厂商兼容矩阵：已知厂商走真实 GPU，未知走软件回退。
pub fn compat_tier(vendor_id: u16) -> GpuTier {
    let mut i = 0;
    while i < KNOWN_VENDORS.len() {
        if KNOWN_VENDORS[i] == vendor_id {
            return GpuTier::Real;
        }
        i += 1;
    }
    GpuTier::Soft
}

// ===========================================================================
// A093 GPU 降级链（三阶梯状态机 + 能力探测）
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DegradeChain {
    pub tier: GpuTier,
    pub reason: u8,
}

impl DegradeChain {
    pub const fn new() -> DegradeChain {
        DegradeChain {
            tier: GpuTier::Real,
            reason: 0,
        }
    }

    /// 探测失败时下降一级（真→虚→软）。
    pub fn step(&mut self, reason: u8) {
        self.tier = degrade_step(self.tier);
        self.reason = reason;
    }
}

/// 下降一级。
pub fn degrade_step(tier: GpuTier) -> GpuTier {
    match tier {
        GpuTier::Real => GpuTier::Virtual,
        GpuTier::Virtual => GpuTier::Soft,
        GpuTier::Soft => GpuTier::Soft,
    }
}

/// 能力探测：模型下三等级均可用。
pub fn capability_probe(tier: GpuTier) -> bool {
    match tier {
        GpuTier::Real => true,
        GpuTier::Virtual => true,
        GpuTier::Soft => true,
    }
}

// ===========================================================================
// A094 GPU 自检（能力探测快照）
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuProbe {
    pub real: bool,
    pub virtual_gpu: bool,
    pub software: bool,
    pub recommended: GpuTier,
}

/// 汇总 GPU 能力探测。
pub fn gpu_probe() -> GpuProbe {
    let d = detect_gpu_device();
    GpuProbe {
        real: d.real_present,
        virtual_gpu: d.virtual_present,
        software: d.software_present,
        recommended: d.recommended_tier(),
    }
}

// ===========================================================================
// A095 GPU 性能预算
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuPerfBudget {
    pub frame_ms: u32,
    pub mem_mb: u32,
}

/// 是否在帧时间 + 显存预算内。
pub fn within_budget(b: GpuPerfBudget, used_ms: u32, used_mb: u32) -> bool {
    used_ms <= b.frame_ms && used_mb <= b.mem_mb
}

// ===========================================================================
// A096 GPU 可观测（遥测快照）
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuTelemetry {
    pub submissions: u32,
    pub dropped: u32,
    pub last_tier: GpuTier,
}

/// 生成一次遥测快照。
pub fn telemetry_snapshot(submissions: u32, dropped: u32, tier: GpuTier) -> GpuTelemetry {
    GpuTelemetry {
        submissions,
        dropped,
        last_tier: tier,
    }
}

// ===========================================================================
// A097 GPU 模糊测试（解码/着色器永不 panic）
// ===========================================================================

/// 喂任意命令字：解码安全（返回合法变体，绝不越界/panic）。
pub fn fuzz_decode_safe(word: u32) -> bool {
    let _ = decode_command(word);
    true
}

/// 喂任意着色器字节：校验安全（绝不越界/panic）。
pub fn fuzz_shader_safe(bytes: &[u8]) -> bool {
    let _ = validate_shader(bytes);
    true
}

// ===========================================================================
// A098 GPU 文档（单一文档串）
// ===========================================================================

/// 域文档串（供内核自检与帮助系统引用）。
pub fn gpu_doc() -> &'static str {
    "AURORA-1000 AI-04 GPU driver & hardware acceleration: real/virtual/soft tiers, \
     command ring, VRAM allocator, shader validator, degrade chain, compositor accel."
}

// ===========================================================================
// A099 每阶梯合成加速策略判定
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompositeOp {
    Blit,
    Blur,
    Shadow,
    Scale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccelDecision {
    Hardware,
    Partial,
    Software,
}

/// 每阶梯合成加速策略：真实=全硬件，虚拟=blit/scale 部分硬件其余软件，软件=全软件。
pub fn tier_compositor_strategy(tier: GpuTier, op: CompositeOp) -> AccelDecision {
    match tier {
        GpuTier::Real => AccelDecision::Hardware,
        GpuTier::Virtual => match op {
            CompositeOp::Blit | CompositeOp::Scale => AccelDecision::Partial,
            CompositeOp::Blur | CompositeOp::Shadow => AccelDecision::Software,
        },
        GpuTier::Soft => AccelDecision::Software,
    }
}

// ===========================================================================
// A100 GPU 驱动与硬件加速域自检收口
// ===========================================================================

/// 域自检：25 项（A076~A100）全真，参与内核闭环。
pub fn run_gpu_checks() -> CheckSet {
    let mut set = CheckSet::new("aurora-gpu");

    // A076 真实 GPU 驱动框架
    let dev = detect_gpu_device();
    set.add(
        "A076 real-gpu framework",
        dev.tier_present(GpuTier::Soft) && dev.recommended_tier() == GpuTier::Real,
        "device present + recommended tier",
    );

    // A077 命令缓冲 + 包格式 + 编码/解码 + 回放
    let mut ring = CommandRing::new();
    let mut ok_push = true;
    let mut i = 0u32;
    while i < 256 {
        if !ring.push(encode_draw(i as u16, (i & 0xFF) as u8)) {
            ok_push = false;
        }
        i += 1;
    }
    let full = ring.is_full() && !ring.push(encode_nop());
    let mut ok_pop = true;
    let mut ok_rt = true;
    while !ring.is_empty() {
        match ring.pop() {
            Some(w) => {
                if reencode(decode_command(w)) != w {
                    ok_rt = false;
                }
            }
            None => ok_pop = false,
        }
    }
    let words = [encode_nop(), encode_draw(10, 2), encode_flip(true), encode_wait(120)];
    set.add(
        "A077 command ring + encode/decode + replay",
        ok_push && full && ok_pop && ok_rt && replay_validate(&words) && ring.is_empty(),
        "ring full/empty + round-trip",
    );

    // A078 着色器校验器
    let mut good = [0u8; 16];
    good[0..4].copy_from_slice(&SHADER_MAGIC);
    good[4..6].copy_from_slice(&SHADER_VERSION.to_le_bytes());
    good[6..8].copy_from_slice(&2u16.to_le_bytes());
    let sv = validate_shader(&good);
    let mut bad = [0u8; 16];
    bad[0..4].copy_from_slice(b"XXXX");
    let bv = validate_shader(&bad);
    set.add(
        "A078 shader validator",
        sv.valid && !bv.valid && sv.instrs == 2 && compile_shader(&good).is_some() && compile_shader(&bad).is_none(),
        "magic/version/instr bounds",
    );

    // A079 显存分配器
    let mut va = VramAllocator::new(1024);
    let a = va.alloc(256, 1);
    let b = va.alloc(256, 2);
    let c = va.alloc(256, 3);
    let freed = a.is_some()
        && b.is_some()
        && c.is_some()
        && va.free(a.unwrap())
        && va.free(b.unwrap())
        && va.free(c.unwrap());
    let double_free = va.free(0) == false; // 已释放 → 拒绝，不重复计数
    let merged = va.block_count() == 1 && va.used_bytes() == 0;
    set.add(
        "A079 vram allocator",
        freed && double_free && merged && !va.has_leak() && va.alloc_count == 3 && va.free_count == 3,
        "first-fit + merge + leak pair",
    );

    // A080 纹理上传
    let t = upload_texture(64, 64, 1);
    let bad_tex = upload_texture(0, 64, 1);
    let over_tex = upload_texture(20000, 64, 1);
    set.add(
        "A080 texture upload",
        t.is_some() && bad_tex.is_none() && over_tex.is_none() && t.unwrap().w == 64,
        "dims + limits",
    );

    // A081 顶点/索引缓冲
    let v = create_vertex_buffer(1024);
    let ix = create_index_buffer(512);
    let z = create_vertex_buffer(0);
    let big = create_index_buffer(MAX_BUFFER_BYTES + 1);
    set.add(
        "A081 vertex/index buffer",
        v.is_some() && ix.is_some() && z.is_none() && big.is_none(),
        "alloc + bounds",
    );

    // A082 GPU 调度 / flush 边界
    let mut s = GpuScheduler::new(4);
    let f1 = s.enqueue(4);
    let f2 = s.enqueue(2);
    let f3 = s.enqueue(2);
    set.add(
        "A082 gpu scheduler",
        f1 && !f2 && f3 && s.flushed_batches == 2 && s.pending() == 0,
        "flush boundary",
    );

    // A083 合成器硬件加速
    let rc = compositor_capabilities(GpuTier::Real);
    let sc = compositor_capabilities(GpuTier::Soft);
    set.add(
        "A083 compositor hw",
        rc.can_blur && rc.can_shadow && !sc.can_blur && !sc.can_shadow,
        "per-tier caps",
    );

    // A084 视频解码硬件加速
    set.add(
        "A084 video decode",
        video_decode_accel(GpuTier::Real, VideoCodec::AV1)
            && !video_decode_accel(GpuTier::Soft, VideoCodec::H264),
        "codec matrix",
    );

    // A085 2D 加速接口
    set.add(
        "A085 2d accel",
        accel2d(GpuTier::Real, Accel2dOp::Fill) && !accel2d(GpuTier::Soft, Accel2dOp::Fill),
        "2d ops",
    );

    // A086 虚拟 GPU 设备
    let vg = detect_virtual_gpu();
    set.add(
        "A086 virtual gpu",
        vg.is_some() && vg.unwrap().vram_mb > 0 && vg.unwrap().queues >= 1,
        "paravirt device",
    );

    // A087 软件光栅回退
    let sr = software_raster_capable();
    set.add(
        "A087 soft raster",
        sr.max_dim > 0 && sr.threads >= 1,
        "fallback present",
    );

    // A088 GPU 功耗管理
    let hi = gpu_power_step(GpuTier::Real, 1000);
    let lo = gpu_power_step(GpuTier::Real, 0);
    let soft = gpu_power_step(GpuTier::Soft, 1000);
    set.add(
        "A088 gpu power",
        hi.milliwatts > lo.milliwatts && soft.clock_mhz < hi.clock_mhz,
        "load scaling",
    );

    // A089 GPU 崩溃隔离（超时复位状态机）
    let mut fsm = ResetFsm::new(100);
    let s1 = fsm.reset_tick(10, true);
    let s2 = fsm.reset_tick(100, true);
    let s3 = fsm.reset_tick(10, false);
    let s4 = fsm.reset_tick(10, false);
    set.add(
        "A089 crash isolation",
        s1 == ResetState::Detecting
            && s2 == ResetState::Recovering
            && s3 == ResetState::Recovered
            && s4 == ResetState::Healthy,
        "reset fsm",
    );

    // A090 GPU 直通安全
    let safe = GpuPassthrough { iommu: true, dma_isolated: true };
    let unsafe_p = GpuPassthrough { iommu: false, dma_isolated: true };
    set.add(
        "A090 passthrough safety",
        passthrough_safe(safe) && !passthrough_safe(unsafe_p),
        "iommu + dma isolation",
    );

    // A091 GPU 性能基准
    set.add(
        "A091 benchmark",
        benchmark(GpuTier::Real).score > benchmark(GpuTier::Soft).score
            && benchmark(GpuTier::Virtual).fps > benchmark(GpuTier::Soft).fps,
        "tier scores",
    );

    // A092 GPU 兼容矩阵
    set.add(
        "A092 compat matrix",
        compat_tier(0x10DE) == GpuTier::Real && compat_tier(0xFFFF) == GpuTier::Soft,
        "vendor ids",
    );

    // A093 GPU 降级链
    let real_ok = capability_probe(GpuTier::Real);
    let step1 = if real_ok { GpuTier::Real } else { degrade_step(GpuTier::Real) };
    let d1 = degrade_step(GpuTier::Real);
    let d2 = degrade_step(d1);
    set.add(
        "A093 degrade chain",
        real_ok && step1 == GpuTier::Real && d1 == GpuTier::Virtual && d2 == GpuTier::Soft,
        "real->virtual->soft",
    );

    // A094 GPU 自检（能力探测）
    let p = gpu_probe();
    set.add(
        "A094 gpu probe",
        p.software && p.real && p.recommended == GpuTier::Real,
        "capability probe",
    );

    // A095 GPU 性能预算
    let b = GpuPerfBudget { frame_ms: 16, mem_mb: 256 };
    set.add(
        "A095 perf budget",
        within_budget(b, 10, 100) && !within_budget(b, 20, 100) && !within_budget(b, 10, 300),
        "frame + mem",
    );

    // A096 GPU 可观测
    let tel = telemetry_snapshot(42, 1, GpuTier::Real);
    set.add(
        "A096 telemetry",
        tel.submissions == 42 && tel.dropped == 1 && tel.last_tier == GpuTier::Real,
        "observability",
    );

    // A097 GPU 模糊测试
    let df = fuzz_decode_safe(0xFFFF_FFFF) && fuzz_decode_safe(0);
    let sf = fuzz_shader_safe(&[0u8; 3]) && fuzz_shader_safe(&[]);
    set.add("A097 fuzz safety", df && sf, "decoder/shader never panic");

    // A098 GPU 文档
    set.add("A098 docs", gpu_doc().len() > 0, "doc string present");

    // A099 每阶梯合成加速策略
    set.add(
        "A099 tier compositor",
        tier_compositor_strategy(GpuTier::Real, CompositeOp::Blur) == AccelDecision::Hardware
            && tier_compositor_strategy(GpuTier::Soft, CompositeOp::Blur) == AccelDecision::Software
            && tier_compositor_strategy(GpuTier::Virtual, CompositeOp::Blur) == AccelDecision::Software,
        "per-tier accel",
    );

    // A100 域自检收口
    let total = set.len() + 1;
    set.add(
        "A100 domain self-check closure",
        total >= 25 && set.all_passed(),
        ">=25 checks green",
    );

    set
}

// ===========================================================================
// 单元测试
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a076_device_detect() {
        let d = detect_gpu_device();
        assert!(d.tier_present(GpuTier::Soft));
        assert!(d.tier_present(GpuTier::Real));
        assert_eq!(d.recommended_tier(), GpuTier::Real);
        assert_eq!(d.tier_present(GpuTier::Virtual), true);
    }

    #[test]
    fn a077_ring_full_and_empty() {
        let mut r = CommandRing::new();
        assert!(r.is_empty());
        for _ in 0..CMD_RING_CAP {
            assert!(r.push(encode_nop()));
        }
        assert!(r.is_full());
        assert!(!r.push(encode_nop())); // 满判空
        assert_eq!(r.len(), CMD_RING_CAP);
        while !r.is_empty() {
            assert!(r.pop().is_some());
        }
        assert!(r.is_empty());
        assert!(r.pop().is_none());
    }

    #[test]
    fn a077_command_encode_decode_roundtrip() {
        let cases = [
            encode_nop(),
            encode_draw(0, 0),
            encode_draw(65535, 255),
            encode_flip(true),
            encode_flip(false),
            encode_wait(0),
            encode_wait(123456),
        ];
        for &w in cases.iter() {
            assert_eq!(reencode(decode_command(w)), w);
        }
        // 未知 opcode 安全降级为 Nop
        assert_eq!(decode_command(0xF), GpuCommand::Nop);
        assert!(!is_valid_command_word(0xF));
    }

    #[test]
    fn a077_replay_validate() {
        let good = [encode_nop(), encode_draw(7, 3), encode_flip(true), encode_wait(50)];
        assert!(replay_validate(&good));
        let bad = [encode_draw(1, 1), 0xF << 4]; // 未知 opcode 字
        assert!(!replay_validate(&bad));
    }

    #[test]
    fn a078_shader_validate_ok() {
        let mut b = [0u8; 8 + 4 * 3];
        b[0..4].copy_from_slice(&SHADER_MAGIC);
        b[4..6].copy_from_slice(&SHADER_VERSION.to_le_bytes());
        b[6..8].copy_from_slice(&3u16.to_le_bytes());
        let s = validate_shader(&b);
        assert!(s.magic_ok && s.version_ok && s.valid && s.instrs == 3);
        assert_eq!(compile_shader(&b), Some(shader_hash(&b)));
    }

    #[test]
    fn a078_shader_validate_bad_magic() {
        let mut b = [0u8; 16];
        b[0..4].copy_from_slice(b"XXXX");
        b[4..6].copy_from_slice(&SHADER_VERSION.to_le_bytes());
        b[6..8].copy_from_slice(&2u16.to_le_bytes());
        assert!(!validate_shader(&b).valid);
        assert!(compile_shader(&b).is_none());
    }

    #[test]
    fn a078_shader_validate_too_many() {
        let mut b = [0u8; 8 + 4 * 10];
        b[0..4].copy_from_slice(&SHADER_MAGIC);
        b[4..6].copy_from_slice(&SHADER_VERSION.to_le_bytes());
        // 声明 5000 条指令，超过 SHADER_MAX_INSTRS=4096
        b[6..8].copy_from_slice(&5000u16.to_le_bytes());
        assert!(!validate_shader(&b).valid);
        // 长度不一致也应判否
        let mut short = [0u8; 10];
        short[0..4].copy_from_slice(&SHADER_MAGIC);
        short[4..6].copy_from_slice(&SHADER_VERSION.to_le_bytes());
        short[6..8].copy_from_slice(&3u16.to_le_bytes());
        assert!(!validate_shader(&short).valid);
        // 太短直接拒绝
        assert!(!validate_shader(&[0u8; 4]).valid);
    }

    #[test]
    fn a079_vram_alloc_free_merge() {
        let mut va = VramAllocator::new(1024);
        let a = va.alloc(256, 1).unwrap();
        let b = va.alloc(256, 2).unwrap();
        let c = va.alloc(256, 3).unwrap();
        assert_eq!(a, 0);
        assert_eq!(b, 256);
        assert_eq!(c, 512);
        assert_eq!(va.used_bytes(), 768);
        assert!(va.free(a));
        assert!(va.free(b));
        assert!(va.free(c));
        // 全部释放后相邻合并为单个块
        assert_eq!(va.block_count(), 1);
        assert_eq!(va.used_bytes(), 0);
        assert!(!va.has_leak());
        assert_eq!(va.alloc_count, 3);
        assert_eq!(va.free_count, 3);
    }

    #[test]
    fn a079_vram_double_free_and_unknown() {
        let mut va = VramAllocator::new(512);
        let off = va.alloc(128, 1).unwrap();
        assert!(va.free(off));
        // 已释放再释放 → 拒绝，不重复计数
        assert!(!va.free(off));
        // 未知偏移 → 拒绝
        assert!(!va.free(9999));
        assert_eq!(va.free_count, 1);
        assert!(!va.has_leak());
    }

    #[test]
    fn a079_vram_pool_exhausted() {
        let mut va = VramAllocator::new(128);
        assert!(va.alloc(128, 1).is_some());
        assert!(va.alloc(1, 2).is_none()); // 池耗尽
        // 非零、但未分配请求的 0 尺寸
        assert!(va.alloc(0, 3).is_none());
    }

    #[test]
    fn a079_vram_leak_detection() {
        let mut va = VramAllocator::new(256);
        let _ = va.alloc(64, 1);
        // 不释放 → 泄漏
        assert!(va.has_leak());
        assert_ne!(va.alloc_count, va.free_count);
    }

    #[test]
    fn a080_texture_upload() {
        let t = upload_texture(64, 48, 2).unwrap();
        assert_eq!(t.w, 64);
        assert_eq!(t.h, 48);
        assert!(upload_texture(0, 48, 2).is_none());
        assert!(upload_texture(64, 0, 2).is_none());
        assert!(upload_texture(20000, 64, 2).is_none());
    }

    #[test]
    fn a081_vertex_index_buffer() {
        assert!(create_vertex_buffer(1024).is_some());
        assert!(create_index_buffer(2048).is_some());
        assert!(create_vertex_buffer(0).is_none());
        assert!(create_index_buffer(MAX_BUFFER_BYTES + 1).is_none());
    }

    #[test]
    fn a082_scheduler_flush() {
        let mut s = GpuScheduler::new(4);
        assert!(s.enqueue(4)); // 命中边界 → flush
        assert_eq!(s.flushed_batches, 1);
        assert_eq!(s.pending(), 0);
        assert!(!s.enqueue(2));
        assert!(s.enqueue(2)); // 累计 4 → flush
        assert_eq!(s.flushed_batches, 2);
        // 阈值 0 被规整为 1
        let mut s2 = GpuScheduler::new(0);
        assert!(s2.enqueue(1));
    }

    #[test]
    fn a083_compositor_caps() {
        let r = compositor_capabilities(GpuTier::Real);
        assert!(r.can_blit && r.can_blur && r.can_shadow && r.can_scale);
        let v = compositor_capabilities(GpuTier::Virtual);
        assert!(v.can_blit && v.can_scale && !v.can_blur && !v.can_shadow);
        let f = compositor_capabilities(GpuTier::Soft);
        assert!(!f.can_blit && !f.can_blur && !f.can_shadow && !f.can_scale);
    }

    #[test]
    fn a084_video_decode() {
        assert!(video_decode_accel(GpuTier::Real, VideoCodec::AV1));
        assert!(video_decode_accel(GpuTier::Virtual, VideoCodec::H264));
        assert!(video_decode_accel(GpuTier::Virtual, VideoCodec::VP9));
        assert!(!video_decode_accel(GpuTier::Virtual, VideoCodec::AV1));
        assert!(!video_decode_accel(GpuTier::Soft, VideoCodec::H264));
    }

    #[test]
    fn a085_accel2d() {
        assert!(accel2d(GpuTier::Real, Accel2dOp::Stretch));
        assert!(accel2d(GpuTier::Virtual, Accel2dOp::Fill));
        assert!(!accel2d(GpuTier::Virtual, Accel2dOp::Stretch));
        assert!(!accel2d(GpuTier::Soft, Accel2dOp::Fill));
    }

    #[test]
    fn a086_virtual_gpu() {
        let v = detect_virtual_gpu().unwrap();
        assert_eq!(v.backend, "virtio-gpu");
        assert!(v.vram_mb > 0);
        assert!(v.queues >= 1);
    }

    #[test]
    fn a087_soft_raster() {
        let s = software_raster_capable();
        assert!(s.threads >= 1);
        assert!(s.max_dim > 0);
    }

    #[test]
    fn a088_power_step() {
        let hi = gpu_power_step(GpuTier::Real, 1000);
        let lo = gpu_power_step(GpuTier::Real, 0);
        assert!(hi.milliwatts > lo.milliwatts);
        assert!(hi.clock_mhz > lo.clock_mhz);
        let soft = gpu_power_step(GpuTier::Soft, 1000);
        assert!(soft.clock_mhz < hi.clock_mhz);
        assert_eq!(gpu_power_step(GpuTier::Soft, 0).clock_mhz, 200);
    }

    #[test]
    fn a089_reset_fsm() {
        let mut fsm = ResetFsm::new(100);
        assert_eq!(fsm.state, ResetState::Healthy);
        assert_eq!(fsm.reset_tick(10, true), ResetState::Detecting);
        assert_eq!(fsm.reset_tick(100, true), ResetState::Recovering);
        assert_eq!(fsm.reset_tick(10, false), ResetState::Recovered);
        assert_eq!(fsm.reset_tick(10, false), ResetState::Healthy);
        // 未挂起不进入探测
        let mut f2 = ResetFsm::new(50);
        assert_eq!(f2.reset_tick(10, false), ResetState::Healthy);
    }

    #[test]
    fn a090_passthrough() {
        assert!(passthrough_safe(GpuPassthrough { iommu: true, dma_isolated: true }));
        assert!(!passthrough_safe(GpuPassthrough { iommu: false, dma_isolated: true }));
        assert!(!passthrough_safe(GpuPassthrough { iommu: true, dma_isolated: false }));
    }

    #[test]
    fn a091_benchmark() {
        assert!(benchmark(GpuTier::Real).score > benchmark(GpuTier::Virtual).score);
        assert!(benchmark(GpuTier::Virtual).score > benchmark(GpuTier::Soft).score);
        assert!(benchmark(GpuTier::Real).fps > benchmark(GpuTier::Soft).fps);
    }

    #[test]
    fn a092_compat() {
        assert_eq!(compat_tier(0x10DE), GpuTier::Real); // NVIDIA
        assert_eq!(compat_tier(0x8086), GpuTier::Real); // Intel
        assert_eq!(compat_tier(0x15AD), GpuTier::Real); // VMware
        assert_eq!(compat_tier(0xFFFF), GpuTier::Soft); // 未知
        assert_eq!(compat_tier(0x0000), GpuTier::Soft);
    }

    #[test]
    fn a093_degrade_chain() {
        assert!(capability_probe(GpuTier::Real));
        let mut chain = DegradeChain::new();
        assert_eq!(chain.tier, GpuTier::Real);
        chain.step(1);
        assert_eq!(chain.tier, GpuTier::Virtual);
        chain.step(2);
        assert_eq!(chain.tier, GpuTier::Soft);
        // 已到软件不再下降
        chain.step(3);
        assert_eq!(chain.tier, GpuTier::Soft);
        // 自由函数式降级
        assert_eq!(degrade_step(GpuTier::Real), GpuTier::Virtual);
        assert_eq!(degrade_step(GpuTier::Virtual), GpuTier::Soft);
    }

    #[test]
    fn a094_probe() {
        let p = gpu_probe();
        assert!(p.real && p.virtual_gpu && p.software);
        assert_eq!(p.recommended, GpuTier::Real);
    }

    #[test]
    fn a095_budget() {
        let b = GpuPerfBudget { frame_ms: 16, mem_mb: 256 };
        assert!(within_budget(b, 10, 100));
        assert!(!within_budget(b, 17, 100)); // 超帧时
        assert!(!within_budget(b, 10, 257)); // 超显存
    }

    #[test]
    fn a096_telemetry() {
        let t = telemetry_snapshot(100, 5, GpuTier::Virtual);
        assert_eq!(t.submissions, 100);
        assert_eq!(t.dropped, 5);
        assert_eq!(t.last_tier, GpuTier::Virtual);
    }

    #[test]
    fn a097_fuzz_decode() {
        // 任意位模式不得 panic，且总是返回合法变体
        for w in [0u32, 0xFFFF_FFFF, 0x1234_5678, 0x0A0B_0C0D] {
            let c = decode_command(w);
            match c {
                GpuCommand::Nop | GpuCommand::Draw { .. } | GpuCommand::Flip { .. } | GpuCommand::Wait { .. } => {}
            }
            assert!(fuzz_decode_safe(w));
        }
        assert!(fuzz_shader_safe(&[]));
        assert!(fuzz_shader_safe(&[1, 2, 3, 4, 5, 6, 7, 8, 9]));
    }

    #[test]
    fn a098_doc() {
        let d = gpu_doc();
        assert!(d.len() > 0);
        assert!(d.contains("AURORA-1000"));
    }

    #[test]
    fn a099_tier_compositor() {
        assert_eq!(
            tier_compositor_strategy(GpuTier::Real, CompositeOp::Blur),
            AccelDecision::Hardware
        );
        assert_eq!(
            tier_compositor_strategy(GpuTier::Virtual, CompositeOp::Blit),
            AccelDecision::Partial
        );
        assert_eq!(
            tier_compositor_strategy(GpuTier::Virtual, CompositeOp::Scale),
            AccelDecision::Partial
        );
        assert_eq!(
            tier_compositor_strategy(GpuTier::Virtual, CompositeOp::Blur),
            AccelDecision::Software
        );
        assert_eq!(
            tier_compositor_strategy(GpuTier::Soft, CompositeOp::Blit),
            AccelDecision::Software
        );
    }

    #[test]
    fn a100_run_gpu_checks() {
        let set = run_gpu_checks();
        let (passed, failed) = set.tally();
        assert!(set.all_passed(), "failed checks: {} / {}", failed, passed + failed);
        assert!(set.len() >= 25);
        assert_eq!(failed, 0);
    }
}
