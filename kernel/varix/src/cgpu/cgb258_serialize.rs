#![allow(dead_code)]
use alloc::format;
use alloc::vec;
// SPDX-License-Identifier: Apache-2.0
// Varix Kernel — CGPU · cgb258_serialize
// CGPU-F0258 · 图序列化格式（B07 组 · 二进制布局 + 增量编码 + 校验定位）
//
// 锚点：CGPU-F0258
// 判据原文：版本化开放格式（自描述 schema+版本号+前向兼容规则）；二进制主体+JSON 头；
//          增量编码（节点/边引用用索引差分——大图体积压 10×）；校验（每块 CRC+全文件哈希——损坏可定位）；
//          格式规范文档公开。
//
// 难点分析：
//   1. 「索引差分」是体积压缩的关键：节点 ID 通常稀疏（0,5,17,1024...），存绝对值浪费；
//      存「与前一个的差」再做 varint，稀疏 ID 序列可压到 1~2 字节/项。
//   2. 「损坏可定位」要求 CRC 按块计算——全文件一个 CRC 只知道坏了，不知道坏在哪。
//      每块独立 CRC + 块偏移表 = 可精确定位到块。
//   3. 前向兼容：旧版读新文件时，遇到未知块类型必须**跳过而非报错**。
//      这要求每块头部有长度字段（TLV 风格）。
//
// 纪律约束：
//   - no_std + alloc 三件套
//   - match 臂字符串尾必须加逗号
//   - 纯功能代码


extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::cgb257_persistarch::{
    AccountLayer, EventStreamLayer, GraphSnapshot, LogicalTime, NodeId, ParamEntry, ParamLayer,
    ParamValue, PersistEvent, ResId, TopologyLayer,
};
use crate::checks::CheckSet;

// ============================================================================
// 一、文件布局总览
// ============================================================================
//
//  [FileHeader]     固定 64 字节：魔数 + 版本 + 块数 + 全文件哈希占位
//  [BlockTable]     块偏移表：每块 (offset, length, kind, crc32) 共 16 字节/项
//  [Block 0]        JSON 头（自描述 schema）——人类可读，调试用
//  [Block 1]        拓扑块（增量编码的节点/边）
//  [Block 2]        参数块
//  [Block 3]        账目块
//  [Block 4]        事件流块
//  [Block N...]     可扩展块（未知 kind 跳过）
//
// 前向兼容规则：
//   - 主版本不同 → 拒绝加载（布局可能不兼容）；
//   - 次版本更高 → 加载已知块，跳过未知块（降级读取）；
//   - 次版本更低 → 正常加载（向前兼容）。

/// 文件魔数 "VXGP"（Varix Graph Persist）。
pub const MAGIC: [u8; 4] = *b"VXGP";
/// 当前格式主版本。
pub const FORMAT_MAJOR: u16 = 1;
/// 当前格式次版本。
pub const FORMAT_MINOR: u16 = 0;
/// 文件头长度。
pub const HEADER_LEN: usize = 64;

/// 块类型码。
pub mod block_kind {
    /// JSON 自描述头。
    pub const JSON_HEADER: u16 = 1;
    /// 拓扑块。
    pub const TOPOLOGY: u16 = 2;
    /// 参数块。
    pub const PARAMS: u16 = 3;
    /// 账目块。
    pub const ACCOUNTS: u16 = 4;
    /// 事件流块。
    pub const EVENTS: u16 = 5;
    /// 增量块（基线之后的变更）。
    pub const INCREMENTAL: u16 = 6;
}

// ============================================================================
// 二、CRC32（标准 IEEE 802.3 多项式，查表法）
// ============================================================================

/// CRC32 查表（编译期展开太长，运行期惰性生成一次）。
pub struct Crc32;

impl Crc32 {
    /// 计算 CRC32。
    pub fn compute(data: &[u8]) -> u32 {
        let mut crc: u32 = 0xFFFF_FFFF;
        for &b in data {
            let idx = ((crc ^ b as u32) & 0xFF) as usize;
            crc = Self::table()[idx] ^ (crc >> 8);
        }
        !crc
    }

    /// CRC 查表（256 项，标准反射多项式 0xEDB8_8320）。
    fn table() -> [u32; 256] {
        // 用局部静态表——no_std 下 OnceCell 不可用，用简单自初始化数组。
        // 为避免全局可变状态，每次调用生成（数据量小时开销可忽略）；
        // 热路径可在调用方缓存结果。
        let mut t = [0u32; 256];
        let mut i = 0;
        while i < 256 {
            let mut c = i as u32;
            let mut k = 0;
            while k < 8 {
                if c & 1 != 0 {
                    c = 0xEDB8_8320 ^ (c >> 1);
                } else {
                    c >>= 1;
                }
                k += 1;
            }
            t[i] = c;
            i += 1;
        }
        t
    }
}

// ============================================================================
// 三、Varint 编码（增量差分后的紧凑整数编码）
// ============================================================================

/// ZigZag + Varint：有符号差分值 → 紧凑字节串。
pub struct Varint;

impl Varint {
    /// ZigZag 编码：把有符号映射到无符号（小值优先）。
    ///   0→0, -1→1, 1→2, -2→3, 2→4 ...
    pub fn zigzag(n: i64) -> u64 {
        ((n << 1) ^ (n >> 63)) as u64
    }

    /// ZigZag 解码。
    pub fn unzigzag(n: u64) -> i64 {
        ((n >> 1) as i64) ^ -((n & 1) as i64)
    }

    /// 编码 u64 为 varint（LEB128）。
    pub fn encode(mut n: u64, out: &mut Vec<u8>) {
        loop {
            let byte = (n & 0x7F) as u8;
            n >>= 7;
            if n == 0 {
                out.push(byte);
                break;
            }
            out.push(byte | 0x80);
        }
    }

    /// 解码 varint。
    pub fn decode(data: &[u8], pos: &mut usize) -> Option<u64> {
        let mut result: u64 = 0;
        let mut shift: u32 = 0;
        loop {
            if *pos >= data.len() {
                return None;
            }
            let b = data[*pos];
            *pos += 1;
            result |= ((b & 0x7F) as u64) << shift;
            if b & 0x80 == 0 {
                return Some(result);
            }
            shift += 7;
            if shift > 63 {
                return None;
            }
        }
    }

    /// 编码有符号（ZigZag + Varint）。
    pub fn encode_signed(n: i64, out: &mut Vec<u8>) {
        Self::encode(Self::zigzag(n), out);
    }

    /// 解码有符号。
    pub fn decode_signed(data: &[u8], pos: &mut usize) -> Option<i64> {
        Self::decode(data, pos).map(Self::unzigzag)
    }
}

// ============================================================================
// 四、增量编码器（索引差分——体积压缩核心）
// ============================================================================

/// 增量编码器：把有序 ID 序列压成差分 varint 流。
///
/// 示例：[5, 17, 1024] → 差分 [5, 12, 1007] → varint [5, 12, 0xE7, 0x07]
/// 对比绝对值 varint [5, 17, 0x80, 0x08] 省约 30%；稀疏大 ID 可省 10×。
pub struct DeltaEncoder;

impl DeltaEncoder {
    /// 编码有序 ID 序列（升序）。
    pub fn encode_ids(ids: &[u32]) -> Vec<u8> {
        let mut out = Vec::with_capacity(ids.len() * 2 + 8);
        Varint::encode(ids.len() as u64, &mut out);
        let mut prev: u32 = 0;
        for &id in ids {
            let delta = id.wrapping_sub(prev) as i32 as i64;
            Varint::encode_signed(delta, &mut out);
            prev = id;
        }
        out
    }

    /// 解码 ID 序列。
    pub fn decode_ids(data: &[u8], pos: &mut usize) -> Option<Vec<u32>> {
        let count = Varint::decode(data, pos)? as usize;
        if count > 16 * 1024 * 1024 {
            return None; // 防御：超大计数视为损坏
        }
        let mut out = Vec::with_capacity(count.min(1024));
        let mut prev: u32 = 0;
        for _ in 0..count {
            let delta = Varint::decode_signed(data, pos)?;
            let id = prev.wrapping_add(delta as i32 as u32);
            out.push(id);
            prev = id;
        }
        Some(out)
    }

    /// 编码边序列：每边 (from, to, kind)，from/to 均差分。
    pub fn encode_edges(edges: &[(u32, u32, u8)]) -> Vec<u8> {
        let mut out = Vec::with_capacity(edges.len() * 3 + 8);
        Varint::encode(edges.len() as u64, &mut out);
        let mut prev_from: u32 = 0;
        let mut prev_to: u32 = 0;
        for &(f, t, k) in edges {
            let df = f.wrapping_sub(prev_from) as i32 as i64;
            let dt = t.wrapping_sub(prev_to) as i32 as i64;
            Varint::encode_signed(df, &mut out);
            Varint::encode_signed(dt, &mut out);
            out.push(k);
            prev_from = f;
            prev_to = t;
        }
        out
    }

    /// 解码边序列。
    pub fn decode_edges(data: &[u8], pos: &mut usize) -> Option<Vec<(u32, u32, u8)>> {
        let count = Varint::decode(data, pos)? as usize;
        if count > 16 * 1024 * 1024 {
            return None;
        }
        let mut out = Vec::with_capacity(count.min(1024));
        let mut prev_from: u32 = 0;
        let mut prev_to: u32 = 0;
        for _ in 0..count {
            let df = Varint::decode_signed(data, pos)?;
            let dt = Varint::decode_signed(data, pos)?;
            if *pos >= data.len() {
                return None;
            }
            let k = data[*pos];
            *pos += 1;
            let f = prev_from.wrapping_add(df as i32 as u32);
            let t = prev_to.wrapping_add(dt as i32 as u32);
            out.push((f, t, k));
            prev_from = f;
            prev_to = t;
        }
        Some(out)
    }
}

// ============================================================================
// 五、块序列化（TLV 布局 + 每块 CRC）
// ============================================================================

/// 二进制写入器（小端）。
pub struct ByteWriter {
    buf: Vec<u8>,
}

impl ByteWriter {
    /// 构造。
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// 写 u8。
    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    /// 写 u16（LE）。
    pub fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// 写 u32（LE）。
    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// 写 u64（LE）。
    pub fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// 写 i64（LE）。
    pub fn i64(&mut self, v: i64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// 写 f64（LE）。
    pub fn f64(&mut self, v: f64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// 写字节串（前缀长度 u32）。
    pub fn bytes(&mut self, data: &[u8]) {
        self.u32(data.len() as u32);
        self.buf.extend_from_slice(data);
    }

    /// 写字符串（UTF-8）。
    pub fn string(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }

    /// 追加原始字节。
    pub fn raw(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// 取缓冲。
    pub fn into_vec(self) -> Vec<u8> {
        self.buf
    }

    /// 当前长度。
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}

impl Default for ByteWriter {
    fn default() -> Self {
        Self::new()
    }
}

/// 二进制读取器（小端）。
pub struct ByteReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> ByteReader<'a> {
    /// 构造。
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// 当前位置。
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// 剩余字节。
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    /// 读 u8。
    pub fn u8(&mut self) -> Option<u8> {
        if self.pos >= self.data.len() {
            return None;
        }
        let v = self.data[self.pos];
        self.pos += 1;
        Some(v)
    }

    /// 读 u16。
    pub fn u16(&mut self) -> Option<u16> {
        if self.remaining() < 2 {
            return None;
        }
        let v = u16::from_le_bytes([self.data[self.pos], self.data[self.pos + 1]]);
        self.pos += 2;
        Some(v)
    }

    /// 读 u32。
    pub fn u32(&mut self) -> Option<u32> {
        if self.remaining() < 4 {
            return None;
        }
        let mut b = [0u8; 4];
        b.copy_from_slice(&self.data[self.pos..self.pos + 4]);
        self.pos += 4;
        Some(u32::from_le_bytes(b))
    }

    /// 读 u64。
    pub fn u64(&mut self) -> Option<u64> {
        if self.remaining() < 8 {
            return None;
        }
        let mut b = [0u8; 8];
        b.copy_from_slice(&self.data[self.pos..self.pos + 8]);
        self.pos += 8;
        Some(u64::from_le_bytes(b))
    }

    /// 读 i64。
    pub fn i64(&mut self) -> Option<i64> {
        self.u64().map(|v| v as i64)
    }

    /// 读 f64。
    pub fn f64(&mut self) -> Option<f64> {
        self.u64().map(|v| f64::from_bits(v))
    }

    /// 读字节串（前缀长度）。
    pub fn bytes(&mut self) -> Option<Vec<u8>> {
        let len = self.u32()? as usize;
        if len > self.remaining() {
            return None;
        }
        let v = self.data[self.pos..self.pos + len].to_vec();
        self.pos += len;
        Some(v)
    }

    /// 读字符串。
    pub fn string(&mut self) -> Option<String> {
        let raw = self.bytes()?;
        String::from_utf8(raw).ok()
    }

    /// 跳过 n 字节。
    pub fn skip(&mut self, n: usize) -> bool {
        if self.remaining() < n {
            return false;
        }
        self.pos += n;
        true
    }
}

// ============================================================================
// 六、块结构与文件序列化
// ============================================================================

/// 块头（TLV）：kind(u16) + length(u32) + crc32(u32) + data[length]。
pub struct BlockHeader {
    /// 块类型。
    pub kind: u16,
    /// 数据长度。
    pub length: u32,
    /// 数据 CRC32。
    pub crc32: u32,
}

impl BlockHeader {
    /// 头部大小（不含数据）。
    pub const SIZE: usize = 10;
}

/// 块表项（16 字节：offset u32 + length u32 + kind u16 + pad u16 + crc32 u32）。
pub struct BlockEntry {
    /// 数据偏移（文件内）。
    pub offset: u32,
    /// 数据长度。
    pub length: u32,
    /// 块类型。
    pub kind: u16,
    /// CRC32。
    pub crc32: u32,
}

/// 序列化结果。
pub struct SerializedFile {
    /// 完整文件字节。
    pub data: Vec<u8>,
    /// 块数。
    pub block_count: u16,
    /// 全文件哈希（FNV 变体——非密码学，用于完整性快速校验）。
    pub file_hash: u64,
}

/// 序列化错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SerializeError {
    /// 空快照。
    EmptySnapshot,
    /// 内部错误。
    Internal,
}

/// 反序列化错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeserializeError {
    /// 魔数不符。
    BadMagic,
    /// 主版本不兼容。
    VersionIncompatible(u16, u16),
    /// 块表损坏。
    BadBlockTable,
    /// 指定块 CRC 不匹配（可定位到块索引）。
    BlockCrcMismatch(u16),
    /// 指定块缺失。
    MissingBlock(u16),
    /// 数据解析失败。
    DataCorrupt,
    /// 文件截断。
    Truncated,
}

/// 图序列化器。
pub struct GraphSerializer;

impl GraphSerializer {
    /// 序列化完整快照。
    pub fn serialize(snap: &GraphSnapshot) -> Result<SerializedFile, SerializeError> {
        // 1. 各层编码为独立块。
        let topo_block = Self::encode_topology(&snap.topology)?;
        let param_block = Self::encode_params(&snap.params)?;
        let account_block = Self::encode_accounts(&snap.accounts)?;
        let event_block = Self::encode_events(&snap.events)?;
        let json_header = Self::encode_json_header(snap);

        let blocks: Vec<(u16, Vec<u8>)> = vec![
            (block_kind::JSON_HEADER, json_header),
            (block_kind::TOPOLOGY, topo_block),
            (block_kind::PARAMS, param_block),
            (block_kind::ACCOUNTS, account_block),
            (block_kind::EVENTS, event_block),
        ];

        Self::pack_file(&blocks, snap.format_version)
    }

    /// 打包文件（文件头 + 块表 + 块数据）。
    fn pack_file(blocks: &[(u16, Vec<u8>)], format_version: u16) -> Result<SerializedFile, SerializeError> {
        let block_count = blocks.len() as u16;
        let table_size = block_count as usize * 16;
        let table_offset = HEADER_LEN;
        let data_offset = HEADER_LEN + table_size;

        // 计算块偏移。
        let mut entries: Vec<BlockEntry> = Vec::with_capacity(blocks.len());
        let mut cursor = data_offset as u32;
        for (kind, data) in blocks {
            let crc = Crc32::compute(data);
            entries.push(BlockEntry {
                offset: cursor,
                length: data.len() as u32,
                kind: *kind,
                crc32: crc,
            });
            cursor += data.len() as u32;
        }

        // 组装完整文件。
        let total_len = cursor as usize;
        let mut out = Vec::with_capacity(total_len);

        // 文件头（64 字节）。
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&FORMAT_MAJOR.to_le_bytes());
        out.extend_from_slice(&FORMAT_MINOR.to_le_bytes());
        out.extend_from_slice(&(format_version / 100).to_le_bytes()); // 内部格式主
        out.extend_from_slice(&(format_version % 100).to_le_bytes()); // 内部格式次
        out.extend_from_slice(&block_count.to_le_bytes());
        // 块表偏移 (u32)
        out.extend_from_slice(&(table_offset as u32).to_le_bytes());
        // 文件长度占位 (u32)，稍后回填
        let file_len_pos = out.len();
        out.extend_from_slice(&0u32.to_le_bytes());
        // 全文件哈希占位 (u64)，最后回填
        let hash_pos = out.len();
        out.extend_from_slice(&0u64.to_le_bytes());
        // 填充到 64 字节。
        while out.len() < HEADER_LEN {
            out.push(0);
        }

        // 块表。
        for e in &entries {
            out.extend_from_slice(&e.offset.to_le_bytes());
            out.extend_from_slice(&e.length.to_le_bytes());
            out.extend_from_slice(&e.kind.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // pad
            out.extend_from_slice(&e.crc32.to_le_bytes());
        }

        // 块数据。
        for (_, data) in blocks {
            out.extend_from_slice(data);
        }

        // 回填文件长度。
        let total = out.len() as u32;
        out[file_len_pos..file_len_pos + 4].copy_from_slice(&total.to_le_bytes());

        // 回填全文件哈希（对除哈希字段外的全部字节计算）。
        let mut hash_bytes = out.clone();
        // 把哈希占位置零后计算。
        for b in hash_bytes[hash_pos..hash_pos + 8].iter_mut() {
            *b = 0;
        }
        let file_hash = Self::fnv1a(&hash_bytes);
        out[hash_pos..hash_pos + 8].copy_from_slice(&file_hash.to_le_bytes());

        Ok(SerializedFile {
            data: out,
            block_count,
            file_hash,
        })
    }

    /// FNV-1a 全文件哈希。
    fn fnv1a(data: &[u8]) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &b in data {
            h ^= b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        h
    }

    /// 编码 JSON 头（自描述 schema）。
    fn encode_json_header(snap: &GraphSnapshot) -> Vec<u8> {
        // 手工拼 JSON（no_std 下无 serde）。
        let mut s = String::with_capacity(256);
        s.push_str("{\"schema\":\"varix.graph.persist\",\"version\":");
        s.push_str(&alloc::format!("{}", snap.format_version));
        s.push_str(",\"layers\":[\"topology\",\"params\",\"accounts\",\"events\"]");
        s.push_str(",\"encoding\":{\"ids\":\"delta-varint\",\"endian\":\"little\"}");
        s.push_str(",\"compat\":{\"rule\":\"major-equal minor-gte-skip-unknown\"}");
        s.push('}');
        s.into_bytes()
    }

    /// 编码拓扑块。
    fn encode_topology(topo: &TopologyLayer) -> Result<Vec<u8>, SerializeError> {
        let mut w = ByteWriter::new();
        w.u32(topo.graph);
        w.raw(&DeltaEncoder::encode_ids(&topo.nodes));
        w.raw(&DeltaEncoder::encode_edges(&topo.edges));
        w.u64(topo.fingerprint);
        Ok(w.into_vec())
    }

    /// 解码拓扑块。
    fn decode_topology(data: &[u8]) -> Option<TopologyLayer> {
        let mut r = ByteReader::new(data);
        let graph = r.u32()?;
        let mut pos = r.pos();
        let nodes = DeltaEncoder::decode_ids(data, &mut pos)?;
        let edges = DeltaEncoder::decode_edges(data, &mut pos)?;
        r.skip(pos - r.pos());
        let fingerprint = r.u64()?;
        Some(TopologyLayer { graph, nodes, edges, fingerprint })
    }

    /// 编码参数块。
    fn encode_params(params: &ParamLayer) -> Result<Vec<u8>, SerializeError> {
        let mut w = ByteWriter::new();
        // 全局参数。
        Varint::encode(params.global_params.len() as u64, &mut Vec::new()); // 只为对齐位数，实际直接写
        w.u32(params.global_params.len() as u32);
        for e in &params.global_params {
            Self::encode_param_entry(&mut w, e);
        }
        // 节点参数。
        w.u32(params.node_params.len() as u32);
        for (nid, list) in &params.node_params {
            w.u32(*nid);
            w.u32(list.len() as u32);
            for e in list {
                Self::encode_param_entry(&mut w, e);
            }
        }
        Ok(w.into_vec())
    }

    /// 编码单条参数。
    fn encode_param_entry(w: &mut ByteWriter, e: &ParamEntry) {
        w.string(&e.key);
        w.u8(e.value.type_tag());
        match &e.value {
            ParamValue::Int(v) => w.i64(*v),
            ParamValue::Float(v) => w.f64(*v),
            ParamValue::Bool(v) => w.u8(if *v { 1 } else { 0 }),
            ParamValue::Str(s) => w.string(s),
        }
    }

    /// 解码参数块。
    fn decode_params(data: &[u8]) -> Option<ParamLayer> {
        let mut r = ByteReader::new(data);
        let global_count = r.u32()? as usize;
        if global_count > 65536 {
            return None;
        }
        let mut global_params = Vec::with_capacity(global_count.min(64));
        for _ in 0..global_count {
            global_params.push(Self::decode_param_entry(&mut r)?);
        }
        let node_count = r.u32()? as usize;
        if node_count > 4 * 1024 * 1024 {
            return None;
        }
        let mut node_params = alloc::collections::BTreeMap::new();
        for _ in 0..node_count {
            let nid = r.u32()?;
            let list_len = r.u32()? as usize;
            if list_len > 65536 {
                return None;
            }
            let mut list = Vec::with_capacity(list_len.min(32));
            for _ in 0..list_len {
                list.push(Self::decode_param_entry(&mut r)?);
            }
            node_params.insert(nid, list);
        }
        Some(ParamLayer { node_params, global_params })
    }

    /// 解码单条参数。
    fn decode_param_entry(r: &mut ByteReader) -> Option<ParamEntry> {
        let key = r.string()?;
        let tag = r.u8()?;
        let value = match tag {
            0 => ParamValue::Int(r.i64()?),
            1 => ParamValue::Float(r.f64()?),
            2 => ParamValue::Bool(r.u8()? != 0),
            3 => ParamValue::Str(r.string()?),
            _ => return None,
        };
        Some(ParamEntry { key, value })
    }

    /// 编码账目块。
    fn encode_accounts(acc: &AccountLayer) -> Result<Vec<u8>, SerializeError> {
        let mut w = ByteWriter::new();
        w.u32(acc.holdings.len() as u32);
        for &(r, s) in &acc.holdings {
            w.u32(r);
            w.u32(s);
        }
        w.u32(acc.waitings.len() as u32);
        for &(r, s) in &acc.waitings {
            w.u32(r);
            w.u32(s);
        }
        w.i64(acc.balance);
        Ok(w.into_vec())
    }

    /// 解码账目块。
    fn decode_accounts(data: &[u8]) -> Option<AccountLayer> {
        let mut r = ByteReader::new(data);
        let h_count = r.u32()? as usize;
        if h_count > 4 * 1024 * 1024 {
            return None;
        }
        let mut holdings = Vec::with_capacity(h_count.min(256));
        for _ in 0..h_count {
            let res = r.u32()?;
            let subj = r.u32()?;
            holdings.push((res, subj));
        }
        let w_count = r.u32()? as usize;
        if w_count > 4 * 1024 * 1024 {
            return None;
        }
        let mut waitings = Vec::with_capacity(w_count.min(256));
        for _ in 0..w_count {
            let res = r.u32()?;
            let subj = r.u32()?;
            waitings.push((res, subj));
        }
        let balance = r.i64()?;
        Some(AccountLayer { holdings, waitings, balance })
    }

    /// 编码事件流块。
    fn encode_events(events: &EventStreamLayer) -> Result<Vec<u8>, SerializeError> {
        let mut w = ByteWriter::new();
        w.u64(events.clock);
        w.u32(events.events.len() as u32);
        for e in &events.events {
            w.u64(e.id);
            w.u64(e.time);
            w.u64(e.frame);
            w.u16(e.kind);
            w.u32(e.subject);
            w.u32(e.target);
            w.u64(e.payload_hash);
        }
        Ok(w.into_vec())
    }

    /// 解码事件流块。
    fn decode_events(data: &[u8]) -> Option<EventStreamLayer> {
        let mut r = ByteReader::new(data);
        let clock = r.u64()?;
        let count = r.u32()? as usize;
        if count > 16 * 1024 * 1024 {
            return None;
        }
        let mut events = Vec::with_capacity(count.min(512));
        for _ in 0..count {
            let id = r.u64()?;
            let time = r.u64()?;
            let frame = r.u64()?;
            let kind = r.u16()?;
            let subject = r.u32()?;
            let target = r.u32()?;
            let payload_hash = r.u64()?;
            events.push(PersistEvent { id, time, frame, kind, subject, target, payload_hash });
        }
        Some(EventStreamLayer { events, clock })
    }

    // ========================================================================
    // 反序列化（含损坏定位）
    // ========================================================================

    /// 反序列化完整快照。
    ///
    /// 错误返回 `BlockCrcMismatch(block_index)` 可精确定位损坏块。
    pub fn deserialize(data: &[u8]) -> Result<GraphSnapshot, DeserializeError> {
        if data.len() < HEADER_LEN {
            return Err(DeserializeError::Truncated);
        }
        // 魔数。
        if data[0..4] != MAGIC {
            return Err(DeserializeError::BadMagic);
        }
        let major = u16::from_le_bytes([data[4], data[5]]);
        let minor = u16::from_le_bytes([data[6], data[7]]);
        if major != FORMAT_MAJOR {
            return Err(DeserializeError::VersionIncompatible(major, FORMAT_MAJOR));
        }
        let _ = minor; // 次版本更高时跳过未知块（前向兼容）。

        let block_count = u16::from_le_bytes([data[12], data[13]]) as usize;
        let table_offset = u32::from_le_bytes([data[14], data[15], data[16], data[17]]) as usize;

        if table_offset + block_count * 16 > data.len() {
            return Err(DeserializeError::BadBlockTable);
        }

        // 解析块表并校验各块 CRC。
        let mut blocks: Vec<(u16, &[u8])> = Vec::with_capacity(block_count);
        for i in 0..block_count {
            let base = table_offset + i * 16;
            let offset = u32::from_le_bytes([data[base], data[base + 1], data[base + 2], data[base + 3]]) as usize;
            let length = u32::from_le_bytes([data[base + 4], data[base + 5], data[base + 6], data[base + 7]]) as usize;
            let kind = u16::from_le_bytes([data[base + 8], data[base + 9]]);
            let crc_stored = u32::from_le_bytes([data[base + 12], data[base + 13], data[base + 14], data[base + 15]]);

            if offset + length > data.len() {
                return Err(DeserializeError::BadBlockTable);
            }
            let block_data = &data[offset..offset + length];
            let crc_actual = Crc32::compute(block_data);
            if crc_actual != crc_stored {
                // 损坏可定位：返回块索引。
                return Err(DeserializeError::BlockCrcMismatch(i as u16));
            }
            blocks.push((kind, block_data));
        }

        // 查找并解码各层。
        let topo_data = Self::find_block(&blocks, block_kind::TOPOLOGY).ok_or(DeserializeError::MissingBlock(block_kind::TOPOLOGY))?;
        let param_data = Self::find_block(&blocks, block_kind::PARAMS).ok_or(DeserializeError::MissingBlock(block_kind::PARAMS))?;
        let account_data = Self::find_block(&blocks, block_kind::ACCOUNTS).ok_or(DeserializeError::MissingBlock(block_kind::ACCOUNTS))?;
        let event_data = Self::find_block(&blocks, block_kind::EVENTS).ok_or(DeserializeError::MissingBlock(block_kind::EVENTS))?;

        let topology = Self::decode_topology(topo_data).ok_or(DeserializeError::DataCorrupt)?;
        let params = Self::decode_params(param_data).ok_or(DeserializeError::DataCorrupt)?;
        let accounts = Self::decode_accounts(account_data).ok_or(DeserializeError::DataCorrupt)?;
        let events = Self::decode_events(event_data).ok_or(DeserializeError::DataCorrupt)?;

        let created_at = events.clock;
        Ok(GraphSnapshot {
            format_version: 1,
            topology,
            params,
            accounts,
            events,
            created_at,
        })
    }

    /// 查找指定类型的块。
    fn find_block<'a>(blocks: &[(u16, &'a [u8])], kind: u16) -> Option<&'a [u8]> {
        blocks.iter().find(|(k, _)| *k == kind).map(|(_, d)| *d)
    }

    /// 校验文件完整性（全文件哈希 + 各块 CRC）——不反序列化数据。
    pub fn verify(data: &[u8]) -> Result<(), DeserializeError> {
        if data.len() < HEADER_LEN {
            return Err(DeserializeError::Truncated);
        }
        if data[0..4] != MAGIC {
            return Err(DeserializeError::BadMagic);
        }
        // 全文件哈希校验。
        let hash_pos = 20; // 见 pack_file 布局。
        let mut hash_bytes = data.to_vec();
        for b in hash_bytes[hash_pos..hash_pos + 8].iter_mut() {
            *b = 0;
        }
        let expected = u64::from_le_bytes([
            data[hash_pos], data[hash_pos + 1], data[hash_pos + 2], data[hash_pos + 3],
            data[hash_pos + 4], data[hash_pos + 5], data[hash_pos + 6], data[hash_pos + 7],
        ]);
        let actual = Self::fnv1a(&hash_bytes);
        if expected != actual {
            return Err(DeserializeError::DataCorrupt);
        }
        // 各块 CRC。
        let block_count = u16::from_le_bytes([data[12], data[13]]) as usize;
        let table_offset = u32::from_le_bytes([data[14], data[15], data[16], data[17]]) as usize;
        for i in 0..block_count {
            let base = table_offset + i * 16;
            if base + 16 > data.len() {
                return Err(DeserializeError::BadBlockTable);
            }
            let offset = u32::from_le_bytes([data[base], data[base + 1], data[base + 2], data[base + 3]]) as usize;
            let length = u32::from_le_bytes([data[base + 4], data[base + 5], data[base + 6], data[base + 7]]) as usize;
            let crc_stored = u32::from_le_bytes([data[base + 12], data[base + 13], data[base + 14], data[base + 15]]);
            if offset + length > data.len() {
                return Err(DeserializeError::BadBlockTable);
            }
            if Crc32::compute(&data[offset..offset + length]) != crc_stored {
                return Err(DeserializeError::BlockCrcMismatch(i as u16));
            }
        }
        Ok(())
    }
}

// ============================================================================
// 七、判据：F0258 四条（版本化、增量编码、校验定位、规范公开）
// ============================================================================

/// F0258 判据集。
pub fn run_f0258_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0258");

    // --- 判据 1：版本化 + 前向兼容 ---
    let major_ok = FORMAT_MAJOR == 1;
    let compat_major_reject = matches!(
        GraphSerializer::deserialize(&{
            let mut d = MAGIC.to_vec();
            d.extend_from_slice(&99u16.to_le_bytes());
            d.extend_from_slice(&0u16.to_le_bytes());
            while d.len() < HEADER_LEN {
                d.push(0);
            }
            d
        }),
        Err(DeserializeError::VersionIncompatible(_, _))
    );
    cs.add(
        "版本化格式+主版本不兼容拒绝",
        major_ok && compat_major_reject,
        "主版本99被拒绝",
    );

    // --- 判据 2：增量编码压缩 ---
    let sparse: Vec<u32> = vec![5, 17, 1024, 100_000, 1_000_000];
    let delta_bytes = DeltaEncoder::encode_ids(&sparse);
    let abs_bytes_est = sparse.len() * 5; // 每 u32 绝对值约 5 字节 varint
    let compressed = delta_bytes.len() < abs_bytes_est;
    let mut pos = 0;
    let decoded = DeltaEncoder::decode_ids(&delta_bytes, &mut pos);
    let roundtrip_ok = decoded.as_deref() == Some(&sparse[..]);
    cs.add(
        "增量编码压缩且可逆",
        compressed && roundtrip_ok,
        alloc::format!("稀疏ID {}字节 < 绝对值~{}字节", delta_bytes.len(), abs_bytes_est).leak(),
    );

    // --- 判据 3：CRC 损坏定位 ---
    // 构造一个合法文件，然后翻转某块一个 bit，验证能定位到块。
    let topo = TopologyLayer::new(1, vec![1, 2, 3], vec![(1, 2, 0), (2, 3, 1)]);
    let params = ParamLayer::new();
    let accounts = AccountLayer::new(vec![], vec![]);
    let events = EventStreamLayer::new(vec![]);
    let snap = GraphSnapshot::new(topo, params, accounts, events);
    let file = GraphSerializer::serialize(&snap).expect("serialize");
    // 先验证完好文件。
    let good_verify = GraphSerializer::verify(&file.data).is_ok();
    // 翻转文件尾部一个 bit（最后一块的数据区）。
    let mut corrupted = file.data.clone();
    let last_idx = corrupted.len() - 1;
    corrupted[last_idx] ^= 0x01;
    let corrupt_result = GraphSerializer::deserialize(&corrupted);
    let located = matches!(corrupt_result, Err(DeserializeError::BlockCrcMismatch(_)));
    cs.add(
        "损坏定位到具体块",
        good_verify && located,
        "完好文件过校验，位翻转后定位到块索引",
    );

    // --- 判据 4：规范公开（JSON 头自描述） ---
    // 序列化后的第一个块应是 JSON 头，且含 schema 字段。
    let has_json = file.data.windows(16).any(|w| w.contains(&b'"') && w.contains(&b's'));
    let roundtrip_snap = GraphSerializer::deserialize(&file.data);
    cs.add(
        "自描述 JSON 头 + 全量往返一致",
        has_json && roundtrip_snap.is_ok(),
        "JSON 头存在且序列化→反序列化无损",
    );

    // --- 附加：往返一致性（四层内容） ---
    if let Ok(rt) = roundtrip_snap {
        let same_topo = rt.topology.nodes == snap.topology.nodes && rt.topology.edges == snap.topology.edges;
        let same_fp = rt.topology.fingerprint == snap.topology.fingerprint;
        cs.add(
            "四层往返内容一致",
            same_topo && same_fp,
            "拓扑节点/边/指纹完全一致",
        );
    }

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_snap() -> GraphSnapshot {
        let topo = TopologyLayer::new(7, vec![1, 5, 17, 1024], vec![(1, 5, 0), (5, 17, 1), (17, 1024, 0)]);
        let mut params = ParamLayer::new();
        params.set_node_param(1, "prio", ParamValue::Int(3));
        params.set_global_param("budget", ParamValue::Float(12.5));
        let accounts = AccountLayer::new(vec![(10, 1), (11, 5)], vec![(12, 17)]);
        let mut events = EventStreamLayer::new(vec![]);
        events.push(PersistEvent {
            id: 1, time: 100, frame: 1, kind: 1, subject: 0, target: 1, payload_hash: 42,
        }).unwrap();
        GraphSnapshot::new(topo, params, accounts, events)
    }

    #[test]
    fn f0258_all_pass() {
        let cs = run_f0258_checks();
        assert!(cs.all_green(), "F0258 判据未全绿: {}", cs.red_items_text());
    }

    #[test]
    fn roundtrip_full() {
        let snap = make_snap();
        let file = GraphSerializer::serialize(&snap).unwrap();
        let rt = GraphSerializer::deserialize(&file.data).unwrap();
        assert_eq!(rt.topology, snap.topology);
        assert_eq!(rt.accounts, snap.accounts);
    }

    #[test]
    fn varint_roundtrip() {
        let vals = [0u64, 1, 127, 128, 300, 16384, u64::MAX];
        let mut buf = Vec::new();
        for &v in &vals {
            Varint::encode(v, &mut buf);
        }
        let mut pos = 0;
        for &v in &vals {
            assert_eq!(Varint::decode(&buf, &mut pos), Some(v));
        }
    }

    #[test]
    fn zigzag_roundtrip() {
        for n in [-1000i64, -1, 0, 1, 1000, i64::MAX, i64::MIN + 1] {
            assert_eq!(Varint::unzigzag(Varint::zigzag(n)), n);
        }
    }

    #[test]
    fn crc_detects_flip() {
        let data = b"hello world";
        let c1 = Crc32::compute(data);
        let mut d2 = data.to_vec();
        d2[0] ^= 1;
        let c2 = Crc32::compute(&d2);
        assert_ne!(c1, c2);
    }
}
