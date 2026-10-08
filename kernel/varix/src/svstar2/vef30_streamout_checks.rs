//! VE-F1627 · 域自检（判据逐条对应，见 `vef30_streamout.rs` 头注）
//!
//! 锚点判据四条 → 自检项映射：
//! - **接口预留** → `C30-接口-01` ~ `08`（阶段二态封闭 + 目标对齐/形状/
//!   分量/槽/目标数五个恰阈对拍 + 区间重叠拦截反恒假 + 能力矩阵封闭）
//! - **对接标注** → `C30-对接-01` ~ `03`（L 粒子预留字面量关键词判据侧
//!   写死 + 粒子桥布局 stride=28 手算 + 程序化声明非空）
//! - **诚实标注** → `C30-标注-01` ~ `04`（条数同源 4=3+1 + 每后端恰一条
//!   + 全过正路径 + 伪造缺「降级」双向必拒反恒假）
//! - **确定性** → `C30-确定-01` ~ `03`（契约三要素字面量 + 两次发射逐字
//!   节一致且基线非空反恒真 + 顶点序敏感可测面）
//! - **判据** → 版本在案 + 码段 0x45 独占（高字节全等 + 与邻域码不等）
//!   + 条数离账
//!
//! **判据设计硬规矩**：stride=28 与码值 0x4501..0x4508 判据侧写死
//! （不引用被测常量自比自身）；恰阈边界 4/5、0/1、4/5 目标数对拍防
//! `>`⇄`>=` 等价变异；先断合法管线过再断重叠拒（反恒假门禁）；先断
//! 基线输出非空再断一致（恒真门禁防护）。

use crate::checks::CheckSet;
use crate::svstar2::vef30_streamout as so;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 判据侧独立真值区（字面量写死）
// ---------------------------------------------------------------------------

const REF_MISALIGNED: u16 = 0x4501;
const REF_OVERFLOW: u16 = 0x4503;
const REF_ANNOT: u16 = 0x4507;
const REF_PARTICLE_STRIDE: u32 = 28;
const REF_MAX_TARGETS: usize = 4;
const REF_L_KEYWORDS: [&str; 3] = ["L 域粒子系统", "ParticleBridge", "跨域预留声明"];
const REF_PROC_KEYWORDS: [&str; 2] = ["程序化生成", "一期接口预留"];
const REF_DET_KEYWORDS: [&str; 3] = ["相同输入同输出", "固定顶点序", "绑定序即写序"];
const REF_VERSION: &str = "F30-streamout-v1";

/// 合法目标（判据侧构造）。
fn ok_target(offset: u32) -> so::SoTarget {
    so::SoTarget {
        buffer_id: 7,
        offset_bytes: offset,
        byte_stride: 28,
        max_vertices: 16,
    }
}

/// F1627 顶点流输出预留判据（四条映射 21 项）。
pub fn run_vef30_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F1627");

    // ================= 一、接口预留 =================

    {
        let passed = so::SoStage::VertexFeedback.say() == "顶点级 transform feedback 写回"
            && so::SoStage::GeometryStream.say() == "GS 流输出写回";
        s.add(
            "C30-接口-01 阶段二态封闭",
            passed,
            if passed { "两阶段 say 逐字对拍（表外无处安放）" } else { "阶段名漂移" },
        );
    }
    {
        let good = so::validate_target(&ok_target(4)).is_ok();
        let bad = so::validate_target(&ok_target(5))
            == Err(so::SoCode::TARGET_MISALIGNED);
        let passed = good && bad
            && so::SoCode::TARGET_MISALIGNED.0 == REF_MISALIGNED;
        s.add(
            "C30-接口-02 目标对齐恰阈",
            passed,
            if passed { "offset 4 收 5 拒（4/5 边界对拍防 >= 等价变异）" } else { "对齐校验漂移" },
        );
    }
    {
        let mut t = ok_target(0);
        let stride_bad = {
            let mut x = t;
            x.byte_stride = 6;
            so::validate_target(&x) == Err(so::SoCode::TARGET_SHAPE_INVALID)
        };
        t.byte_stride = 8;
        let stride_ok = so::validate_target(&t).is_ok();
        let mut z = ok_target(0);
        z.max_vertices = 0;
        let zero_bad = so::validate_target(&z) == Err(so::SoCode::TARGET_SHAPE_INVALID);
        let passed = stride_bad && stride_ok && zero_bad;
        s.add(
            "C30-接口-03 形状恰阈对拍",
            passed,
            if passed { "步幅 8 收 6 拒 + 容量 0 拒（双向边界）" } else { "形状校验漂移" },
        );
    }
    {
        let mk = |c: u8| so::SoLayout {
            vars: alloc::vec![so::SoVar {
                name: "v",
                slot: 0,
                components: c,
            }],
        };
        let passed = mk(1).validate().is_ok()
            && mk(4).validate().is_ok()
            && mk(0).validate() == Err(so::SoCode::COMPONENT_RANGE)
            && mk(5).validate() == Err(so::SoCode::COMPONENT_RANGE);
        s.add(
            "C30-接口-04 分量域恰阈",
            passed,
            if passed { "分量 1..=4 收、0/5 拒（闭开区间双向）" } else { "分量域漂移" },
        );
    }
    {
        let dup = so::SoLayout {
            vars: alloc::vec![
                so::SoVar { name: "a", slot: 0, components: 4 },
                so::SoVar { name: "b", slot: 0, components: 3 },
            ],
        };
        let name_dup = so::SoLayout {
            vars: alloc::vec![
                so::SoVar { name: "a", slot: 0, components: 4 },
                so::SoVar { name: "a", slot: 1, components: 3 },
            ],
        };
        let passed = dup.validate() == Err(so::SoCode::VAR_SLOT_DUP)
            && name_dup.validate() == Err(so::SoCode::VAR_SLOT_DUP);
        s.add(
            "C30-接口-05 槽与名唯一",
            passed,
            if passed { "槽重复与名重复双向拒" } else { "唯一性校验漂移" },
        );
    }
    {
        let mk_pipe = |n: usize| so::SoPipeline {
            stage: so::SoStage::VertexFeedback,
            layout: so::SoLayout {
                vars: alloc::vec![so::SoVar { name: "v", slot: 0, components: 4 }],
            },
            targets: {
                let mut v: Vec<so::SoTarget> = Vec::new();
                let mut k: u32 = 0;
                while (k as usize) < n {
                    v.push(ok_target(k * 4096));
                    k += 1;
                }
                v
            },
            restart_enabled: false,
        };
        let passed = mk_pipe(1).validate().is_ok()
            && mk_pipe(REF_MAX_TARGETS).validate().is_ok()
            && mk_pipe(REF_MAX_TARGETS + 1).validate() == Err(so::SoCode::TARGET_OVERFLOW)
            && so::MAX_SO_TARGETS == REF_MAX_TARGETS
            && mk_pipe(0).validate() == Err(so::SoCode::TARGET_OVERFLOW);
        s.add(
            "C30-接口-06 目标数恰阈",
            passed,
            if passed { "1..=4 收、0/5 拒（上限判据侧写死 4）" } else { "目标数上限漂移" },
        );
    }
    {
        let legal = so::SoPipeline {
            stage: so::SoStage::VertexFeedback,
            layout: so::SoLayout {
                vars: alloc::vec![so::SoVar { name: "v", slot: 0, components: 4 }],
            },
            targets: alloc::vec![ok_target(0), ok_target(4096)],
            restart_enabled: false,
        };
        let overlap = so::SoPipeline {
            stage: so::SoStage::VertexFeedback,
            layout: legal.layout.clone(),
            targets: alloc::vec![ok_target(0), ok_target(16)],
            restart_enabled: false,
        };
        // 反恒假前置：先断合法管线真过，再断重叠真拒
        let passed = legal.validate().is_ok()
            && overlap.validate() == Err(so::SoCode::TARGET_OVERFLOW)
            && so::SoCode::TARGET_OVERFLOW.0 == REF_OVERFLOW;
        s.add(
            "C30-接口-07 区间重叠拦截",
            passed,
            if passed { "合法先过 + 重叠真拒（反恒假门禁：偏移 0 与 16 在步幅 28 下相交）" } else { "重叠校验失效" },
        );
    }
    {
        let d3d = so::caps_for(so::SoBackend::D3D);
        let vul = so::caps_for(so::SoBackend::Vulkan);
        let met = so::caps_for(so::SoBackend::Metal);
        let passed = so::SO_CAPS.len() == 3
            && d3d.map(|c| c.native && c.max_targets == 4).unwrap_or(false)
            && vul.map(|c| c.native && c.max_targets == 4).unwrap_or(false)
            && met.map(|c| !c.native && c.max_targets == 0
                && c.alternate_path.map(|p| p.contains("存储缓冲")).unwrap_or(false))
                .unwrap_or(false)
            && d3d.map(|c| c.alternate_path.is_none()).unwrap_or(false);
        s.add(
            "C30-接口-08 能力矩阵封闭",
            passed,
            if passed { "三后端恰三行：D3D/Vulkan 原生 4 目标、Metal 非原生 0 目标且降级路径在案" } else { "能力矩阵漂移" },
        );
    }

    // ================= 二、对接标注 =================

    {
        let text = so::L_PARTICLE_RESERVATION;
        let passed = REF_L_KEYWORDS.iter().all(|k| text.contains(k));
        s.add(
            "C30-对接-01 L 域预留字面量",
            passed,
            if passed { "L 域粒子系统/ParticleBridge/跨域预留声明三关键词判据侧写死" } else { "跨域预留声明漂移" },
        );
    }
    {
        let plan = so::particle_bridge_plan();
        let passed = match plan {
            Some((l, stage)) => {
                l.vars.len() == 2
                    && l.vars.first().map(|v| v.components).unwrap_or(0) == 4
                    && l.vars.get(1).map(|v| v.components).unwrap_or(0) == 3
                    && l.stride_bytes() == REF_PARTICLE_STRIDE
                    && stage == so::SoStage::VertexFeedback
            }
            None => false,
        };
        s.add(
            "C30-对接-02 粒子桥计划",
            passed,
            if passed { "位置 4 + 速度 3 分量、stride=(4+3)*4=28 判据侧手算写死" } else { "粒子桥布局漂移" },
        );
    }
    {
        let passed = REF_PROC_KEYWORDS.iter().all(|k| so::PROCEDURAL_NOTICE.contains(k));
        s.add(
            "C30-对接-03 程序化声明",
            passed,
            if passed { "程序化生成/一期接口预留关键词在案" } else { "程序化声明漂移" },
        );
    }

    // ================= 三、诚实标注 =================

    {
        let notes = so::honesty_notices();
        let passed = notes.len() == 4;
        s.add(
            "C30-标注-01 条数同源",
            passed,
            if passed { "标注恰 4 条 = 每后端 3 + 全局 1（与能力矩阵行数同源）" } else { "标注条数漂移" },
        );
    }
    {
        let notes = so::honesty_notices();
        let per = notes
            .iter()
            .filter(|n| n.backend.is_some())
            .count();
        let glob = notes
            .iter()
            .filter(|n| n.backend.is_none())
            .count();
        let passed = per == 3 && glob == 1;
        s.add(
            "C30-标注-02 形态齐备",
            passed,
            if passed { "每后端恰一条 + 全局恰一条" } else { "标注形态漂移" },
        );
    }
    {
        let notes = so::honesty_notices();
        let passed = so::verify_notes(&notes).is_ok();
        s.add(
            "C30-标注-03 正路径全过",
            passed,
            if passed { "在案标注与能力矩阵逐条对拍全过" } else { "在案标注自相矛盾" },
        );
    }
    {
        let forged = alloc::vec![so::HonestyNote {
            backend: Some(so::SoBackend::Metal),
            phase: "一期",
            text: "Metal 原生流输出已就绪",
        }];
        let forged_glob = alloc::vec![so::HonestyNote {
            backend: None,
            phase: "全局",
            text: "深度实现随域推进",
        }];
        let r1 = so::verify_notes(&forged) == Err(so::SoCode::ANNOTATION_MISMATCH);
        let r2 = so::verify_notes(&forged_glob) == Err(so::SoCode::ANNOTATION_MISMATCH);
        let passed = r1 && r2 && so::SoCode::ANNOTATION_MISMATCH.0 == REF_ANNOT;
        s.add(
            "C30-标注-04 矛盾双向必拒",
            passed,
            if passed { "非原生后端缺「降级」拒 + 全局缺「L 域」拒（反恒假：verify 真会拒）" } else { "标注校验失效（恒过）" },
        );
    }

    // ================= 四、确定性 =================

    {
        let passed = REF_DET_KEYWORDS.iter().all(|k| so::DETERMINISM_CONTRACT.contains(k));
        s.add(
            "C30-确定-01 契约字面量",
            passed,
            if passed { "相同输入同输出/固定顶点序/绑定序即写序三要素在案" } else { "确定性契约漂移" },
        );
    }
    {
        let plan = so::particle_bridge_plan();
        let passed = match plan {
            Some((l, _)) => {
                let verts: [[i32; 4]; 3] = [[1, 2, 3, 4], [5, 6, 7, 8], [-1, 0, 7, 255]];
                let nonempty = {
                    let mut o: Vec<u8> = Vec::new();
                    so::reference_emit(&l, &verts, &mut o);
                    o.len() == REF_PARTICLE_STRIDE as usize * 3
                };
                so::emit_twice_equal(&l, &verts) && nonempty
            }
            None => false,
        };
        s.add(
            "C30-确定-02 两次发射一致",
            passed,
            if passed { "两次逐字节一致 + 基线字节数=28*3 非空（反恒真前置）" } else { "确定性破坏或基线平凡" },
        );
    }
    {
        let plan = so::particle_bridge_plan();
        let passed = match plan {
            Some((l, _)) => {
                let a: [[i32; 4]; 2] = [[1, 0, 0, 0], [2, 0, 0, 0]];
                let b: [[i32; 4]; 2] = [[2, 0, 0, 0], [1, 0, 0, 0]];
                let mut oa: Vec<u8> = Vec::new();
                let mut ob: Vec<u8> = Vec::new();
                so::reference_emit(&l, &a, &mut oa);
                so::reference_emit(&l, &b, &mut ob);
                oa != ob && !oa.is_empty()
            }
            None => false,
        };
        s.add(
            "C30-确定-03 顶点序敏感",
            passed,
            if passed { "相同布局不同顶点序输出必不同（「固定顶点序」的可测面）" } else { "输出对顶点序不敏感（确定性契约空转）" },
        );
    }

    // ================= 五、判据 =================

    {
        let passed = so::STREAMOUT_VERSION == REF_VERSION;
        s.add(
            "C30-判据-版本在案",
            passed,
            if passed { "STREAMOUT_VERSION=F30-streamout-v1 溯源键稳定" } else { "版本键漂移" },
        );
    }
    {
        let codes = so::SoCode::all();
        let hi_ok = codes.iter().all(|c| (c.0 & 0xFF00) == 0x4500);
        let mut distinct = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if let (Some(a), Some(b)) = (codes.get(i), codes.get(j)) {
                    if a.0 == b.0 {
                        distinct = false;
                    }
                }
            }
        }
        // 与邻域码（vef26=0x43xx / vef27=0x44xx / vef28=0x4Cxx）字面量不等
        let no_neighbor = codes.iter().all(|c| {
            c.0 != 0x4301 && c.0 != 0x4401 && c.0 != 0x4C01
        });
        let passed = codes.len() == 8 && hi_ok && distinct && no_neighbor;
        s.add(
            "C30-判据-码段独占",
            passed,
            if passed { "八码高字节全 0x45、互异、与 0x43/0x44/0x4C 邻域字面量不等" } else { "诊断码段越界或重复" },
        );
    }
    {
        let passed = s.len() == 20;
        s.add(
            "C30-判据-条数对账",
            passed,
            if passed { "判据 21 项离账：对账点前 20 项与设计清单一一对应" } else { "判据条数与设计不符（漏项/多项）" },
        );
    }

    s
}
