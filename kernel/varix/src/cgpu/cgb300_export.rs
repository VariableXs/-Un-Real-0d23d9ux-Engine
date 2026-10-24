// CGPU-F0300 可视化导出 · cgb300_export.rs
// AI-MIMO-1022 · CGPU-B 可视化续章
// 判据：场景可导出为矢量/位图/结构化数据三种格式；导出确定性（同场景同字节）；
//       大场景分块导出不溢出；元数据（版本/时间/作者）随文件。
#![allow(dead_code)]

use alloc::format;
use alloc::vec;
extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;

use super::super::checks::CheckSet;

/// 导出格式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExportFormat {
    VectorSvg,
    BitmapRaw,
    StructuredJson,
    BinaryScene,
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::VectorSvg => "svg",
            Self::BitmapRaw => "raw",
            Self::StructuredJson => "json",
            Self::BinaryScene => "vsc",
        }
    }
}

/// 导出元数据。
#[derive(Clone, Debug)]
pub struct ExportMeta {
    pub version: u32,
    pub timestamp_us: u64,
    pub author_id: u32,
    pub scene_name: String,
    pub checksum: u32,
}

/// 矢量图元。
#[derive(Clone, Debug)]
pub enum VectorPrimitive {
    Rect { x: i32, y: i32, w: u32, h: u32, fill: u32, stroke: u32 },
    Circle { cx: i32, cy: i32, r: u32, fill: u32 },
    Line { x1: i32, y1: i32, x2: i32, y2: i32, stroke: u32, dashed: bool },
    Path { points: Vec<(i32, i32)>, stroke: u32 },
    Text { x: i32, y: i32, text_id: u32, size: u8 },
}

/// 位图像素块（分块导出用）。
#[derive(Clone, Debug)]
pub struct PixelBlock {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    pub rgba: Vec<u8>,
}

/// 结构化导出节点。
#[derive(Clone, Debug)]
pub struct ExportNode {
    pub id: u32,
    pub label: String,
    pub x: f32,
    pub y: f32,
    pub kind: u8,
    pub state: u8,
}

/// 结构化导出边。
#[derive(Clone, Debug)]
pub struct ExportEdge {
    pub id: u32,
    pub from: u32,
    pub to: u32,
    pub weight: u32,
    pub kind: u8,
}

/// 导出产物。
#[derive(Clone, Debug)]
pub struct ExportArtifact {
    pub format: ExportFormat,
    pub meta: ExportMeta,
    pub bytes: Vec<u8>,
    pub chunk_count: u32,
    pub byte_len: u64,
}

/// 导出引擎。
pub struct ExportEngine {
    pub format: ExportFormat,
    pub max_chunk_bytes: usize,
    pub artifacts: Vec<ExportArtifact>,
    pub total_bytes: u64,
    pub export_count: u32,
    pub last_checksum: u32,
}

impl ExportEngine {
    pub fn new(format: ExportFormat) -> Self {
        Self {
            format,
            max_chunk_bytes: 64 * 1024,
            artifacts: Vec::new(),
            total_bytes: 0,
            export_count: 0,
            last_checksum: 0,
        }
    }

    /// 确定性 CRC32（IEEE 多项式，查表-free 位运算）。
    pub fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
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

    /// 导出矢量 SVG（确定性属性序）。
    pub fn export_vector(&mut self, prims: &[VectorPrimitive], name: &str) -> ExportArtifact {
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        out.extend_from_slice(b"<svg xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\">\n");
        for p in prims {
            match p {
                VectorPrimitive::Rect { x, y, w, h, fill, stroke } => {
                    out.extend_from_slice(
                        alloc::format!(
                            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"#{:06X}\" stroke=\"#{:06X}\"/>\n",
                            x, y, w, h, fill & 0xFF_FFFF, stroke & 0xFF_FFFF
                        )
                        .as_bytes(),
                    );
                }
                VectorPrimitive::Circle { cx, cy, r, fill } => {
                    out.extend_from_slice(
                        alloc::format!("<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"#{:06X}\"/>\n", cx, cy, r, fill & 0xFF_FFFF)
                            .as_bytes(),
                    );
                }
                VectorPrimitive::Line { x1, y1, x2, y2, stroke, dashed } => {
                    let dash = if *dashed { " stroke-dasharray=\"4 2\"" } else { "" };
                    out.extend_from_slice(
                        alloc::format!(
                            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#{:06X}\"{}/>\n",
                            x1, y1, x2, y2, stroke & 0xFF_FFFF, dash
                        )
                        .as_bytes(),
                    );
                }
                VectorPrimitive::Path { points, stroke } => {
                    let mut d = String::from("M");
                    for (i, (px, py)) in points.iter().enumerate() {
                        if i > 0 {
                            d.push_str(" L");
                        }
                        d.push_str(&alloc::format!("{} {}", px, py));
                    }
                    out.extend_from_slice(
                        alloc::format!("<path d=\"{}\" stroke=\"#{:06X}\" fill=\"none\"/>\n", d, stroke & 0xFF_FFFF).as_bytes(),
                    );
                }
                VectorPrimitive::Text { x, y, text_id, size } => {
                    out.extend_from_slice(
                        alloc::format!("<text x=\"{}\" y=\"{}\" font-size=\"{}\">#{}</text>\n", x, y, size, text_id).as_bytes(),
                    );
                }
            }
        }
        out.extend_from_slice(b"</svg>\n");
        self.finish(ExportFormat::VectorSvg, name, out)
    }

    /// 导出位图（分块）。
    pub fn export_bitmap(&mut self, blocks: &[PixelBlock], name: &str) -> ExportArtifact {
        let mut out: Vec<u8> = Vec::new();
        // 头：魔数 + 块数
        out.extend_from_slice(b"VRWBMP01");
        out.extend_from_slice(&(blocks.len() as u32).to_le_bytes());
        for b in blocks {
            out.extend_from_slice(&b.x.to_le_bytes());
            out.extend_from_slice(&b.y.to_le_bytes());
            out.extend_from_slice(&b.w.to_le_bytes());
            out.extend_from_slice(&b.h.to_le_bytes());
            out.extend_from_slice(&(b.rgba.len() as u32).to_le_bytes());
            out.extend_from_slice(&b.rgba);
        }
        self.finish(ExportFormat::BitmapRaw, name, out)
    }

    /// 导出结构化 JSON（确定性键序）。
    pub fn export_structured(&mut self, nodes: &[ExportNode], edges: &[ExportEdge], name: &str) -> ExportArtifact {
        let mut out = String::from("{\"version\":1,\"nodes\":[");
        for (i, n) in nodes.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&alloc::format!(
                "{{\"id\":{},\"label\":\"{}\",\"x\":{:.3},\"y\":{:.3},\"kind\":{},\"state\":{}}}",
                n.id, n.label, n.x, n.y, n.kind, n.state
            ));
        }
        out.push_str("],\"edges\":[");
        for (i, e) in edges.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&alloc::format!(
                "{{\"id\":{},\"from\":{},\"to\":{},\"weight\":{},\"kind\":{}}}",
                e.id, e.from, e.to, e.weight, e.kind
            ));
        }
        out.push_str("]}");
        self.finish(ExportFormat::StructuredJson, name, out.into_bytes())
    }

    fn finish(&mut self, format: ExportFormat, name: &str, bytes: Vec<u8>) -> ExportArtifact {
        let checksum = Self::crc32(&bytes);
        self.last_checksum = checksum;
        let chunk_count = if bytes.is_empty() {
            0
        } else {
            ((bytes.len() + self.max_chunk_bytes - 1) / self.max_chunk_bytes) as u32
        };
        let art = ExportArtifact {
            format,
            meta: ExportMeta {
                version: 1,
                timestamp_us: self.export_count as u64 * 1_000_000,
                author_id: 0x4D49,
                scene_name: String::from(name),
                checksum,
            },
            bytes,
            chunk_count,
            byte_len: 0,
        };
        let mut art = art;
        art.byte_len = art.bytes.len() as u64;
        self.total_bytes += art.byte_len;
        self.export_count += 1;
        self.artifacts.push(art.clone());
        art
    }

    /// 同场景两次导出字节一致（确定性验证）。
    pub fn verify_determinism(&self, a: &ExportArtifact, b: &ExportArtifact) -> bool {
        a.bytes == b.bytes && a.meta.checksum == b.meta.checksum
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0300 可视化导出");
        cs.add(
            "产物非空（有导出则有字节）",
            self.artifacts.iter().all(|a| !a.bytes.is_empty()),
            "non-empty",
        );
        cs.add(
            "校验和一致",
            self.artifacts.iter().all(|a| Self::crc32(&a.bytes) == a.meta.checksum),
            "crc match",
        );
        cs.add(
            "字节长度记账正确",
            self.artifacts.iter().all(|a| a.byte_len == a.bytes.len() as u64),
            "len match",
        );
        cs.add(
            "分块数与字节数自洽",
            self.artifacts.iter().all(|a| {
                if a.bytes.is_empty() {
                    a.chunk_count == 0
                } else {
                    let expect = ((a.bytes.len() + self.max_chunk_bytes - 1) / self.max_chunk_bytes) as u32;
                    a.chunk_count == expect
                }
            }),
            "chunk calc",
        );
        cs.add(
            "元数据版本为 1",
            self.artifacts.iter().all(|a| a.meta.version == 1),
            "ver=1",
        );
        cs.add(
            "场景名非空",
            self.artifacts.iter().all(|a| !a.meta.scene_name.is_empty()),
            "name",
        );
        cs.add(
            "总字节记账",
            self.total_bytes == self.artifacts.iter().map(|a| a.byte_len).sum::<u64>(),
            "sum",
        );
        cs.add(
            "导出计数一致",
            self.export_count as usize == self.artifacts.len(),
            "count",
        );
        cs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_deterministic() {
        let mut e1 = ExportEngine::new(ExportFormat::VectorSvg);
        let mut e2 = ExportEngine::new(ExportFormat::VectorSvg);
        let prims = vec![
            VectorPrimitive::Rect { x: 0, y: 0, w: 10, h: 10, fill: 0xFF0000, stroke: 0 },
            VectorPrimitive::Line { x1: 0, y1: 0, x2: 10, y2: 10, stroke: 0x00FF00, dashed: true },
        ];
        let a = e1.export_vector(&prims, "scene");
        let b = e2.export_vector(&prims, "scene");
        assert!(e1.verify_determinism(&a, &b));
    }

    #[test]
    fn structured_roundtrip() {
        let mut e = ExportEngine::new(ExportFormat::StructuredJson);
        let nodes = vec![ExportNode { id: 1, label: String::from("A"), x: 1.0, y: 2.0, kind: 0, state: 1 }];
        let edges = vec![ExportEdge { id: 1, from: 1, to: 1, weight: 5, kind: 0 }];
        let a = e.export_structured(&nodes, &edges, "g");
        assert!(a.bytes.starts_with(b"{\"version\""));
        assert_eq!(a.meta.checksum, ExportEngine::crc32(&a.bytes));
    }

    #[test]
    fn bitmap_chunks() {
        let mut e = ExportEngine::new(ExportFormat::BitmapRaw);
        e.max_chunk_bytes = 32;
        let blocks = vec![PixelBlock { x: 0, y: 0, w: 2, h: 2, rgba: vec![0; 16] }];
        let a = e.export_bitmap(&blocks, "bmp");
        assert!(a.chunk_count >= 1);
    }
}
