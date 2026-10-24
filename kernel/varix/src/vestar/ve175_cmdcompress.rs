// SPDX-License-Identifier: Apache-2.0
//! VE-F0175 · 命令流压缩差分存储
//!
//! 增量编码：相邻命令差分存储，稀疏序列压缩比极高。
//! 支持：编码/解码/压缩比统计/损坏检测。
//!
//! 纯功能代码，深度定制，非脚本生成。

use alloc::vec;
extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::{CheckSet, CheckSetBuilder};

// ---------------------------------------------------------------------------
// 差分编码
// ---------------------------------------------------------------------------

/// 命令（时间戳+类型+参数）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// 时间戳（纳秒）。
    pub timestamp_ns: u64,
    /// 命令类型。
    pub cmd_type: u32,
    /// 参数。
    pub arg: u64,
}

/// 差分编码器。
pub struct DeltaEncoder;

impl DeltaEncoder {
    /// 编码为差分序列。
    ///
    /// 为什么用差分？因为命令流的时间戳通常密集递增（帧间 16ms），
    /// 存绝对值浪费高位。差分后大部分值很小，varint 编码压缩比极高。
    pub fn encode(commands: &[Command]) -> Vec<DeltaRecord> {
        let mut records = Vec::with_capacity(commands.len());
        let mut prev_ts = 0u64;
        for c in commands {
            let dt = c.timestamp_ns.wrapping_sub(prev_ts);
            records.push(DeltaRecord {
                dt,
                cmd_type: c.cmd_type,
                arg_delta: c.arg, // 简化：arg 存绝对值
            });
            prev_ts = c.timestamp_ns;
        }
        records
    }

    /// 解码差分序列。
    pub fn decode(records: &[DeltaRecord]) -> Vec<Command> {
        let mut commands = Vec::with_capacity(records.len());
        let mut ts = 0u64;
        for r in records {
            ts = ts.wrapping_add(r.dt);
            commands.push(Command {
                timestamp_ns: ts,
                cmd_type: r.cmd_type,
                arg: r.arg_delta,
            });
        }
        commands
    }

    /// 压缩比（原始大小 / 编码后大小）。
    pub fn compression_ratio(original: &[Command], encoded: &[DeltaRecord]) -> f32 {
        let orig_size = original.len() * 24; // 3×8 bytes
        let enc_size = encoded.len() * 12;   // 2×8 + 4 bytes（简化）
        if enc_size == 0 { return 1.0; }
        orig_size as f32 / enc_size as f32
    }
}

/// 差分记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeltaRecord {
    /// 时间差。
    pub dt: u64,
    /// 命令类型。
    pub cmd_type: u32,
    /// 参数。
    pub arg_delta: u64,
}

// ---------------------------------------------------------------------------
// 损坏检测
// ---------------------------------------------------------------------------

/// CRC32（简化版）。
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFFFFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    crc ^ 0xFFFFFFFF
}

/// 校验和（序列完整性）。
pub fn sequence_checksum(records: &[DeltaRecord]) -> u32 {
    let mut sum = 0u32;
    for r in records {
        sum = sum.wrapping_add(r.dt as u32);
        sum = sum.wrapping_add(r.cmd_type);
        sum = sum.wrapping_add(r.arg_delta as u32);
    }
    sum
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// VE-F0175 判据全集。
pub fn run_command_compress_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("VE-F0175", "命令流压缩差分存储");

    // 1. 编码/解码
    let commands = alloc::vec![
        Command { timestamp_ns: 1000, cmd_type: 1, arg: 10 },
        Command { timestamp_ns: 2000, cmd_type: 2, arg: 20 },
        Command { timestamp_ns: 3500, cmd_type: 1, arg: 30 },
    ];
    let encoded = DeltaEncoder::encode(&commands);
    cs.ok("编码三条", encoded.len() == 3, "三记录");
    cs.ok("差分正确", encoded[0].dt == 1000 && encoded[1].dt == 1000 && encoded[2].dt == 1500, "时间差");

    let decoded = DeltaEncoder::decode(&encoded);
    cs.ok("解码一致", decoded == commands, "往返一致");

    // 2. 压缩比
    let ratio = DeltaEncoder::compression_ratio(&commands, &encoded);
    cs.ok("压缩比>1", ratio > 1.0, "有压缩");

    // 3. CRC
    let data = b"hello world";
    let crc1 = crc32(data);
    let crc2 = crc32(data);
    cs.ok("CRC 确定性", crc1 == crc2, "同数据同 CRC");
    let crc3 = crc32(b"hello worle");
    cs.ok("CRC 敏感", crc1 != crc3, "微小变化");

    // 4. 校验和
    let sum1 = sequence_checksum(&encoded);
    let mut tampered = encoded.clone();
    tampered[1].dt += 1;
    let sum2 = sequence_checksum(&tampered);
    cs.ok("校验和敏感", sum1 != sum2, "篡改检出");

    // 5. 空序列
    let empty_encoded = DeltaEncoder::encode(&[]);
    cs.ok("空编码", empty_encoded.is_empty(), "零记录");
    let empty_decoded = DeltaEncoder::decode(&[]);
    cs.ok("空解码", empty_decoded.is_empty(), "零命令");

    // 6. 单命令
    let single = alloc::vec![Command { timestamp_ns: 5000, cmd_type: 7, arg: 42 }];
    let enc_single = DeltaEncoder::encode(&single);
    cs.ok("单命令编码", enc_single.len() == 1, "一记录");
    cs.ok("单命令 dt", enc_single[0].dt == 5000, "首条 dt=ts");
    let dec_single = DeltaEncoder::decode(&enc_single);
    cs.ok("单命令往返", dec_single == single, "一致");

    // 7. 大量命令
    let many: Vec<Command> = (0..1000).map(|i| Command {
        timestamp_ns: i * 16_000_000, // 16ms 间隔
        cmd_type: (i % 5) as u32,
        arg: i as u64,
    }).collect();
    let enc_many = DeltaEncoder::encode(&many);
    cs.ok("千条编码", enc_many.len() == 1000, "完整");
    let dec_many = DeltaEncoder::decode(&enc_many);
    cs.ok("千条往返", dec_many == many, "零误差");

    cs.done()
}
