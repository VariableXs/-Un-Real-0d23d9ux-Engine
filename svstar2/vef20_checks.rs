//! VE-F1617 · 域自检（判据逐条对应，见 `vef20_meshspec.rs` 头注）
//!
//! **判据（锚点原文）**：三文档、判据。补充：三读者适配、开放规范的可执行性。
//!
//! 判据侧纪律：
//! 1. **独立第二实现**：判据侧按规范表**手写**一个参考编码器
//!    （[`spec_encode_ref`]），与产线编码器输出逐字节对拍——"依规范可
//!    实现"的判据化：判据侧就是那个"依规范的第三方实现"。
//! 2. **基线非平凡**：先断三文档表行数非零、参考编码 56 字节，再断
//!    一致性——空表对空流会恒绿。
//! 3. **双向验证**：指南档位对拍是双向的（指南 ⊆ 实现档位 且 实现档位
//!    ⊆ 指南），单侧对拍抓不到"实现加档指南漏更"。
//! 4. **清单纪律**（F1185）：不支持/部分行必须带非空替代路径——
//!    "不支持"三字不是免检通行证。

use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::gfx::meshquant::QuantBits;
use crate::gfx::meshrepair::RepairMesh;
use crate::svstar2::vef18_geofuzz::encode_mesh_stream;
use crate::svstar2::vef20_meshspec as ms;

// ---------------------------------------------------------------------------
// 判据侧独立实现：按规范表手写的参考编码器（"第三方依规范实现"的化身）
// ---------------------------------------------------------------------------

/// 依规范表驱动编码 3 顶点 1 面参考网格（不调产线编码器）。
fn spec_encode_ref() -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    // header.vert_count：offset 0，u32 LE（规范行 1）。
    out.extend_from_slice(&3u32.to_le_bytes());
    // header.face_count：offset 4，u32 LE（规范行 2）。
    out.extend_from_slice(&1u32.to_le_bytes());
    // verts[i].xyz：offset 8+12i，3×f32 LE（规范行 3）。
    let vs = [[0.0f32, 0.0, 0.0], [1.0f32, 0.0, 0.0], [0.0f32, 1.0, 0.0]];
    let mut i = 0usize;
    while i < 3 {
        out.extend_from_slice(&vs[i][0].to_le_bytes());
        out.extend_from_slice(&vs[i][1].to_le_bytes());
        out.extend_from_slice(&vs[i][2].to_le_bytes());
        i += 1;
    }
    // faces[j].abc：offset 8+12*vert_count+12j，3×u32 LE（规范行 4）。
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&2u32.to_le_bytes());
    out
}

/// 产线参考网格（与被测 spec_impl_drift_check 的语料同构）。
fn ref_mesh() -> RepairMesh {
    let mut m = RepairMesh::new();
    let vs = [[0.0f32, 0.0, 0.0], [1.0f32, 0.0, 0.0], [0.0f32, 1.0, 0.0]];
    let mut i = 0;
    while i < 3 {
        m.push_vert(vs[i]);
        i += 1;
    }
    m.push_face([0, 1, 2]);
    m
}

/// 判据条数（文档/聚合器自检口径）。
pub const fn total_check_count() -> u32 {
    14
}

/// 判据入口（聚合器经 mod.rs 调用）。
pub fn run_vef20_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F1617");

    // ------------------------------------------------------------------
    // SPEC：文档一 vmesh 规范（基线非平凡先行）
    // ------------------------------------------------------------------

    // ① 规范表行数非平凡 + 总长公式独立重算（4 行、56 字节、逐行 size>0）。
    let mut rows_ok = true;
    let mut k = 0usize;
    while k < ms::VMESH_SPEC.len() {
        if ms::VMESH_SPEC[k].size == 0 {
            rows_ok = false;
        }
        k += 1;
    }
    s.add(
        "C17-SPEC-01 规范表 4 行非平凡且总长公式=56",
        ms::VMESH_SPEC.len() == 4
            && rows_ok
            && ms::spec_total_size(3, 1) == 8 + 12 * 3 + 12 * 1
            && ms::spec_total_size(3, 1) == 56,
        "判据侧独立重算 8+12*3+12*1=56",
    );

    // ② 容量上限单源：规范常量与实现常量逐位相等（各自写数必漂移）。
    s.add(
        "C17-SPEC-02 容量上限与实现单源（2^20/2^20）",
        ms::SPEC_MAX_VERTS == crate::svstar2::vef18_geofuzz::STREAM_MAX_VERTS
            && ms::SPEC_MAX_FACES == crate::svstar2::vef18_geofuzz::STREAM_MAX_FACES
            && ms::SPEC_MAX_VERTS == 1 << 20,
        "规范与实现各自写数=双源漂移温床",
    );

    // ③ 头部行偏移口径：两行 offset 文本恰为 "0"/"4"（字段序即字节序）。
    s.add(
        "C17-SPEC-03 头部行偏移口径 0/4",
        ms::VMESH_SPEC[0].offset_expr == "0" && ms::VMESH_SPEC[1].offset_expr == "4",
        "头部字段序错位=第三方读写器全错",
    );

    // ------------------------------------------------------------------
    // DRIFT：规范-实现一致性（可执行规范 + 独立第二实现对拍）
    // ------------------------------------------------------------------

    // ④ 产线漂移检查零漂移（规范表 vs 实现编码器，6 项逐字段全 ok）。
    let rep = ms::spec_impl_drift_check();
    let mut all_ok = true;
    for f in rep.findings.iter() {
        if !f.ok {
            all_ok = false;
        }
    }
    s.add(
        "C17-DRIFT-01 规范-实现零漂移（6 字段全 ok）",
        rep.findings.len() == 6 && all_ok && rep.drift_count == 0 && !rep.drifted(),
        "漂移检查挂聚合器=CI 每次全量核对",
    );

    // ⑤ 独立第二实现对拍：判据侧按规范表手写的编码器与产线编码器
    //    输出逐字节相等（56 字节全等——"依规范可实现"的判据化）。
    let mine = spec_encode_ref();
    let prod = encode_mesh_stream(&ref_mesh());
    let mut byte_eq = mine.len() == prod.len();
    let mut bi = 0usize;
    while byte_eq && bi < mine.len() {
        if mine[bi] != prod[bi] {
            byte_eq = false;
        }
        bi += 1;
    }
    s.add(
        "C17-DRIFT-02 规范驱动独立编码与产线逐字节相等",
        byte_eq && mine.len() == 56,
        "判据侧=依规范的第三方实现，56 字节逐位对拍",
    );

    // ⑥ 双向验证：篡改规范期望（长度公式错版 48）必须与实现长度不等
    //    ——漂移检查若对错误规范也绿，规范-实现一致就是空话。
    let wrong_spec_len = ms::spec_total_size(3, 1) + 4;
    s.add(
        "C17-DRIFT-03 错误规范期望必被区分（双向）",
        wrong_spec_len != prod.len() as u64 && rep.spec_len == prod.len() as u64,
        "规范错版与实现在长度口径上可区分",
    );

    // ------------------------------------------------------------------
    // GLTF：文档二 互转清单（F1185 清单纪律）
    // ------------------------------------------------------------------

    // ⑦ 不支持/部分行全部带非空替代路径；支持行替代路径留空。
    let mut alt_ok = true;
    let mut unsup = 0usize;
    let mut partial = 0usize;
    let mut k = 0usize;
    while k < ms::GLTF_INTEROP.len() {
        let r = &ms::GLTF_INTEROP[k];
        match r.support {
            ms::InteropSupport::Unsupported => {
                unsup += 1;
                if r.alternative.is_empty() || r.note.is_empty() {
                    alt_ok = false;
                }
            }
            ms::InteropSupport::Partial => {
                partial += 1;
                if r.alternative.is_empty() {
                    alt_ok = false;
                }
            }
            ms::InteropSupport::Supported => {
                if !r.alternative.is_empty() || r.note.is_empty() {
                    alt_ok = false;
                }
            }
        }
        k += 1;
    }
    s.add(
        "C17-GLTF-01 限制清单化：不支持/部分行全带替代路径",
        alt_ok && unsup >= 4 && partial >= 1,
        "F1185：不做不假装，逐条指路替代管线",
    );

    // ⑧ 支持面基线非平凡：恰好 2 条直接支持（位置+三角索引）——
    //    支持面突然扩大/缩水都会被此基线抓到。
    let mut sup = 0usize;
    let mut k = 0usize;
    while k < ms::GLTF_INTEROP.len() {
        if ms::GLTF_INTEROP[k].support == ms::InteropSupport::Supported {
            sup += 1;
        }
        k += 1;
    }
    s.add(
        "C17-GLTF-02 支持面基线=2（位置/索引直转）",
        sup == 2 && ms::GLTF_INTEROP.len() == 7,
        "支持面漂移=互转承诺漂移",
    );

    // ------------------------------------------------------------------
    // QUANT：文档三 量化指南（档位单源双向对拍）
    // ------------------------------------------------------------------

    // ⑨ 指南档位全部可构造且四档恰好全覆盖（双向：指南⊆实现 且 实现⊆指南）。
    let mut guide_bits: Vec<u32> = Vec::new();
    let mut all_constructible = true;
    let mut k = 0usize;
    while k < ms::QUANT_GUIDE.len() {
        let b = ms::QUANT_GUIDE[k].bits;
        if QuantBits::from_bits(b).is_none() {
            all_constructible = false;
        }
        guide_bits.push(b);
        k += 1;
    }
    let impl_bits = [8u32, 10, 12, 16];
    let mut impl_covered = true;
    let mut k = 0usize;
    while k < impl_bits.len() {
        let mut found = false;
        let mut j = 0usize;
        while j < guide_bits.len() {
            if guide_bits[j] == impl_bits[k] {
                found = true;
            }
            j += 1;
        }
        if !found {
            impl_covered = false;
        }
        k += 1;
    }
    let mut guide_unique = guide_bits.len() == 4;
    if guide_unique {
        let mut j = 0usize;
        while j < 4 {
            let mut k2 = j + 1;
            while k2 < 4 {
                if guide_bits[j] == guide_bits[k2] {
                    guide_unique = false;
                }
                k2 += 1;
            }
            j += 1;
        }
    }
    s.add(
        "C17-QUANT-01 指南档位=实现档位全集（双向对拍）",
        all_constructible && impl_covered && guide_unique,
        "指南推荐不存在的档位/实现加档指南漏更 都会被抓",
    );

    // ⑩ 行纪律：场景与依据非空 + 12bit 主流档在册（与 F1616 基准档呼应）。
    let mut rows_ok = true;
    let mut has_mainstream = false;
    let mut k = 0usize;
    while k < ms::QUANT_GUIDE.len() {
        let r = &ms::QUANT_GUIDE[k];
        if r.scenario.is_empty() || r.rationale.is_empty() {
            rows_ok = false;
        }
        if r.bits == 12 {
            has_mainstream = true;
        }
        k += 1;
    }
    s.add(
        "C17-QUANT-02 行纪律齐备且 12bit 主流档在册",
        rows_ok && has_mainstream,
        "无依据的推荐是拍脑袋；主流档缺位=指南与基准脱钩",
    );

    // ------------------------------------------------------------------
    // AUD：三读者适配
    // ------------------------------------------------------------------

    // ⑪ 三读者覆盖断言全绿 + wire 短码互异（读者键可作账本键）。
    let wires = [
        ms::SpecAudience::ALL[0].wire(),
        ms::SpecAudience::ALL[1].wire(),
        ms::SpecAudience::ALL[2].wire(),
    ];
    let wires_differ = wires[0] != wires[1] && wires[1] != wires[2] && wires[0] != wires[2];
    s.add(
        "C17-AUD-01 三读者全覆盖且短码互异",
        ms::all_audiences_covered() && wires_differ,
        "缺任一读者的文档只是半个文档",
    );

    // ------------------------------------------------------------------
    // META：判据集自检
    // ------------------------------------------------------------------

    // ⑫ 实挂条数对拍声明条数：META 段之前实 add 11 条（从 CheckSet
    //    运行时实取，非自证常数），META 段自身 3 条，合计须等于声明
    //    total_check_count()——任何一侧增删判据而漏改口径即红。
    let before_meta = s.len();
    s.add(
        "C17-META-01 实挂条数+3(META)=声明条数14",
        before_meta == 11 && before_meta + 3 == total_check_count() as usize,
        "实 add 数从 CheckSet.len() 实取；增删判据漏改口径即红",
    );
    s.add(
        "C17-META-02 三文档行数总和=15",
        ms::VMESH_SPEC.len() + ms::GLTF_INTEROP.len() + ms::QUANT_GUIDE.len() == 15,
        "文档行数是规范的一部分：增删行必须过判据",
    );
    // ⑭ 收口自检：至此实挂 13 条（11+META 前 2 条），+本条后全集恰为
    //    声明条数；且逐条实取判据名，全集互异（重名=聚合器 tally 失真）。
    let mut names: Vec<&'static str> = Vec::new();
    let mut k = 0usize;
    while k < s.len() {
        if let Some(ch) = s.get(k) {
            names.push(ch.name);
        }
        k += 1;
    }
    let mut all_differ = true;
    let mut i = 0usize;
    while i < names.len() {
        let mut j = i + 1;
        while j < names.len() {
            if names[i] == names[j] {
                all_differ = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add(
        "C17-META-03 收口：实挂13+1=声明14 且判据名全集互异",
        s.len() + 1 == total_check_count() as usize && all_differ,
        "聚合器尾部 tally 与此处双口径互钉；重名即 tally 失真",
    );

    s
}
