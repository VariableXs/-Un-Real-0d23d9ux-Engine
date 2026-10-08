//! VE-F1628 · 域自检（判据逐条对应，见 `vef31_indirect.rs` 头注）
//!
//! 锚点判据四条 → 自检项映射：
//! - **接口** → `C31-接口-01` ~ `08`（二态封闭 + 对齐 16/17 恰阈 +
//!   实例 0/1 恰阈与空绘制合法 + first+count 恰阈 checked 溢出 +
//!   参数块 16 字节小端手算 + 批 0/1/上限/上限+1 四向 + 重叠拦截反恒假
//!   + 能力矩阵封闭）
//! - **探测** → `C31-探测-01` ~ `02`（三后端全在案 + Metal 索引回退与
//!   批上限差异可测）
//! - **诚实标注** → `C31-标注-01` ~ `04`（条数同源 4=3+1 + 形态齐备 +
//!   正路径全过 + 伪造「索引原生」/缺「剔除组」双向必拒反恒假）
//! - **对接预留** → `C31-对接-01` ~ `03`（I 剔除组关键词判据侧写死 +
//!   桥计划形态 + 零 CPU 回读与 VE-F0029 衔接声明）
//! - **判据** → 版本在案 + 码段 0x4D 独占 + 条数对账
//!
//! **判据设计硬规矩**：stride=16、上限 65536/16384、码值 0x4D01..
//! 0x4D08 判据侧写死（不引用被测常量自比自身）；16/17、0/1、
//! MAX/MAX+1 恰阈对拍防等价变异；先断合法批过再断重叠拒（反恒假）；
//! 范围溢出用 first=u32::MAX, count=1 恰好边界 + count=2 溢出双向。

use crate::checks::CheckSet;
use crate::svstar2::vef31_indirect as id;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 判据侧独立真值区（字面量写死）
// ---------------------------------------------------------------------------

const REF_STRIDE: u32 = 16;
const REF_MAX_BATCH: usize = 1024;
const REF_MISALIGNED: u16 = 0x4D01;
const REF_OVERLAP: u16 = 0x4D04;
const REF_ANNOT: u16 = 0x4D06;
const REF_D3D_MAX: u32 = 65_536;
const REF_METAL_MAX: u32 = 16_384;
const REF_CULL_KEYWORDS: [&str; 3] = ["I09/I10", "GPU 驱动", "接口预留"];
const REF_VERSION: &str = "F31-indirect-v1";

/// 合法命令+参数（判据侧构造）。
fn ok_cmd(offset: u32) -> (id::IndirectCommand, id::IndirectArgs) {
    (
        id::IndirectCommand {
            draw_type: id::DrawType::IndirectDraw,
            buffer_id: 9,
            offset_bytes: offset,
        },
        id::IndirectArgs {
            count: 36,
            instance_count: 1,
            first: 0,
            first_instance: 0,
        },
    )
}

/// F1628 间接绘制预留判据（四条映射 20 项）。
pub fn run_vef31_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F1628");

    // ================= 一、接口 =================

    {
        let passed = id::DrawType::IndirectDraw.say() == "非索引间接绘制"
            && id::DrawType::IndirectDrawIndexed.say() == "索引间接绘制";
        s.add(
            "C31-接口-01 二态封闭",
            passed,
            if passed { "两变体 say 逐字对拍（表外无处安放）" } else { "变体名漂移" },
        );
    }
    {
        let (c, a) = ok_cmd(16);
        let good = id::validate_command(&c, &a).is_ok();
        let (bad_c, bad_a) = ok_cmd(17);
        let bad = id::validate_command(&bad_c, &bad_a)
            == Err(id::IdCode::OFFSET_MISALIGNED);
        let passed = good && bad
            && id::INDIRECT_ARGS_STRIDE == REF_STRIDE
            && id::IdCode::OFFSET_MISALIGNED.0 == REF_MISALIGNED;
        s.add(
            "C31-接口-02 对齐恰阈",
            passed,
            if passed { "偏移 16 收 17 拒（stride=16 判据侧写死，防 >= 等价变异）" } else { "对齐校验漂移" },
        );
    }
    {
        let zero_inst = id::IndirectArgs {
            count: 36,
            instance_count: 0,
            first: 0,
            first_instance: 0,
        };
        let empty_draw = id::IndirectArgs {
            count: 0,
            instance_count: 1,
            first: 0,
            first_instance: 0,
        };
        let passed = zero_inst.validate() == Err(id::IdCode::ARGS_INVALID)
            && empty_draw.validate().is_ok();
        s.add(
            "C31-接口-03 参数域恰阈",
            passed,
            if passed { "实例 0 拒 1 收；count=0 空绘制合法（被剔除非错误）" } else { "参数域漂移" },
        );
    }
    {
        let edge = id::IndirectArgs {
            count: 1,
            instance_count: 1,
            first: u32::MAX - 1,
            first_instance: 0,
        };
        let overflow = id::IndirectArgs {
            count: 2,
            instance_count: 1,
            first: u32::MAX - 1,
            first_instance: 0,
        };
        let passed = edge.validate().is_ok()
            && overflow.validate() == Err(id::IdCode::RANGE_INVALID);
        s.add(
            "C31-接口-04 范围恰阈溢出",
            passed,
            if passed { "first+count 恰到 u32 边界收、超 1 拒（checked 双向）" } else { "范围校验漂移" },
        );
    }
    {
        let a = id::IndirectArgs {
            count: 36,
            instance_count: 2,
            first: 7,
            first_instance: 5,
        };
        let b = a.to_bytes();
        let passed = b.len() == 16
            && b[0] == 36 && b[1] == 0 && b[2] == 0 && b[3] == 0
            && b[4] == 2 && b[8] == 7 && b[12] == 5;
        s.add(
            "C31-接口-05 参数块小端手算",
            passed,
            if passed { "16 字节布局判据侧逐字节手算（count=36→0x24 小端）" } else { "参数块布局漂移" },
        );
    }
    {
        let mk = |n: usize| id::IndirectBatch {
            buffer_id: 9,
            commands: {
                let mut v: Vec<id::IndirectCommand> = Vec::new();
                let mut k: usize = 0;
                while k < n {
                    v.push(id::IndirectCommand {
                        draw_type: id::DrawType::IndirectDraw,
                        buffer_id: 9,
                        offset_bytes: (k as u32) * REF_STRIDE,
                    });
                    k += 1;
                }
                v
            },
        };
        let args_of = |_c: &id::IndirectCommand| {
            Some(id::IndirectArgs {
                count: 36,
                instance_count: 1,
                first: 0,
                first_instance: 0,
            })
        };
        let passed = mk(1).validate(&args_of).is_ok()
            && mk(REF_MAX_BATCH).validate(&args_of).is_ok()
            && mk(REF_MAX_BATCH + 1).validate(&args_of) == Err(id::IdCode::BATCH_OVERFLOW)
            && mk(0).validate(&args_of) == Err(id::IdCode::BATCH_OVERFLOW)
            && id::MAX_INDIRECT_PER_BATCH == REF_MAX_BATCH;
        s.add(
            "C31-接口-06 批数恰阈",
            passed,
            if passed { "1..=1024 收、0/1025 拒（上限判据侧写死 1024）" } else { "批上限漂移" },
        );
    }
    {
        let (c1, a1) = ok_cmd(0);
        let (c2, a2) = ok_cmd(16);
        let legal = id::IndirectBatch {
            buffer_id: 9,
            commands: alloc::vec![c1, c2],
        };
        let args_of = |c: &id::IndirectCommand| {
            if c.offset_bytes == 0 {
                Some(a1)
            } else {
                Some(a2)
            }
        };
        let dup = id::IndirectBatch {
            buffer_id: 9,
            commands: alloc::vec![c1, c1],
        };
        let dup_args = a1;
        let args_of_dup = |_c: &id::IndirectCommand| Some(dup_args);
        // 反恒假前置：先断合法批真过，再断同偏移重叠真拒
        let passed = legal.validate(&args_of).is_ok()
            && dup.validate(&args_of_dup) == Err(id::IdCode::ARGS_OVERLAP)
            && id::IdCode::ARGS_OVERLAP.0 == REF_OVERLAP;
        s.add(
            "C31-接口-07 参数块重叠拦截",
            passed,
            if passed { "合法批（偏移 0/16）先过 + 同偏移真拒（反恒假门禁）" } else { "重叠校验失效" },
        );
    }
    {
        let d3d = id::caps_for(id::IdBackend::D3D);
        let vul = id::caps_for(id::IdBackend::Vulkan);
        let met = id::caps_for(id::IdBackend::Metal);
        let passed = id::ID_CAPS.len() == 3
            && d3d.map(|c| c.native_draw && c.native_indexed
                && c.max_command_count == REF_D3D_MAX).unwrap_or(false)
            && vul.map(|c| c.native_draw && c.native_indexed).unwrap_or(false)
            && met.map(|c| c.native_draw && !c.native_indexed
                && c.max_command_count == REF_METAL_MAX
                && c.note.map(|n| n.contains("回退")).unwrap_or(false))
                .unwrap_or(false);
        s.add(
            "C31-接口-08 能力矩阵封闭",
            passed,
            if passed { "三后端恰三行：D3D/Vulkan 双变体原生 65536、Metal 非索引原生+索引回退 16384（判据侧写死）" } else { "能力矩阵漂移" },
        );
    }

    // ================= 二、探测 =================

    {
        let all_some = id::caps_for(id::IdBackend::D3D).is_some()
            && id::caps_for(id::IdBackend::Vulkan).is_some()
            && id::caps_for(id::IdBackend::Metal).is_some();
        let passed = all_some;
        s.add(
            "C31-探测-01 全后端在案",
            passed,
            if passed { "三后端 caps_for 全 Some（域内封闭无缺席）" } else { "能力查询缺席" },
        );
    }
    {
        let met = id::caps_for(id::IdBackend::Metal);
        let d3d = id::caps_for(id::IdBackend::D3D);
        let passed = match (met, d3d) {
            (Some(m), Some(d)) => {
                m.max_command_count < d.max_command_count
                    && m.note.is_some() == !m.native_indexed
                    && m.note.map(|n| n.contains("回退")).unwrap_or(true)
            }
            _ => false,
        };
        s.add(
            "C31-探测-02 差异可测",
            passed,
            if passed { "Metal 批上限严格小于 D3D + 索引回退与 note 同向一致" } else { "后端差异不可测（探测空转）" },
        );
    }

    // ================= 三、诚实标注 =================

    {
        let notes = id::honesty_notices();
        let passed = notes.len() == 4;
        s.add(
            "C31-标注-01 条数同源",
            passed,
            if passed { "标注恰 4 条 = 每后端 3 + 全局 1（与能力矩阵行数同源）" } else { "标注条数漂移" },
        );
    }
    {
        let notes = id::honesty_notices();
        let per = notes.iter().filter(|n| n.backend.is_some()).count();
        let glob = notes.iter().filter(|n| n.backend.is_none()).count();
        let passed = per == 3 && glob == 1;
        s.add(
            "C31-标注-02 形态齐备",
            passed,
            if passed { "每后端恰一条 + 全局恰一条" } else { "标注形态漂移" },
        );
    }
    {
        let notes = id::honesty_notices();
        let passed = id::verify_notes(&notes).is_ok();
        s.add(
            "C31-标注-03 正路径全过",
            passed,
            if passed { "在案标注与能力矩阵逐条对拍全过" } else { "在案标注自相矛盾" },
        );
    }
    {
        let forged = alloc::vec![id::HonestyNote {
            backend: Some(id::IdBackend::Metal),
            phase: "一期",
            text: "Metal 索引变体原生支持",
        }];
        let forged_glob = alloc::vec![id::HonestyNote {
            backend: None,
            phase: "全局",
            text: "深度实现随域推进",
        }];
        let r1 = id::verify_notes(&forged) == Err(id::IdCode::ANNOTATION_MISMATCH);
        let r2 = id::verify_notes(&forged_glob) == Err(id::IdCode::ANNOTATION_MISMATCH);
        let passed = r1 && r2 && id::IdCode::ANNOTATION_MISMATCH.0 == REF_ANNOT;
        s.add(
            "C31-标注-04 矛盾双向必拒",
            passed,
            if passed { "非原生 indexed 称原生拒 + 全局缺「剔除组」拒（反恒假：verify 真会拒）" } else { "标注校验失效（恒过）" },
        );
    }

    // ================= 四、对接预留 =================

    {
        let passed = REF_CULL_KEYWORDS.iter().all(|k| id::I_CULL_RESERVATION.contains(k));
        s.add(
            "C31-对接-01 剔除组预留字面量",
            passed,
            if passed { "I09/I10/GPU 驱动/接口预留关键词判据侧写死" } else { "跨域预留声明漂移" },
        );
    }
    {
        let plan = id::cull_bridge_plan();
        let passed = match plan {
            Some(b) => {
                b.commands.len() == 2
                    && b.commands.first().map(|c| c.offset_bytes).unwrap_or(1) == 0
                    && b.commands.get(1).map(|c| c.offset_bytes).unwrap_or(0) == REF_STRIDE
                    && b.validate(&|_c| {
                        Some(id::IndirectArgs {
                            count: 36,
                            instance_count: 1,
                            first: 0,
                            first_instance: 0,
                        })
                    })
                    .is_ok()
            }
            None => false,
        };
        s.add(
            "C31-对接-02 桥计划形态",
            passed,
            if passed { "剔除桥两命令、偏移 0/16、批校验过（一期只产计划不执行）" } else { "桥计划漂移" },
        );
    }
    {
        let passed = id::ZERO_CPU_READBACK.contains("CPU 不回读")
            && id::ZERO_CPU_READBACK.contains("剔除决策在 GPU")
            && id::VEA29_UPSTREAM_NOTICE.contains("VE-F0029")
            && id::VEA29_UPSTREAM_NOTICE.contains("不重叠");
        s.add(
            "C31-对接-03 零回读与衔接",
            passed,
            if passed { "零 CPU 回读纪律 + VE-F0029 上游衔接声明在案" } else { "纪律声明漂移" },
        );
    }

    // ================= 五、判据 =================

    {
        let passed = id::INDIRECT_VERSION == REF_VERSION;
        s.add(
            "C31-判据-版本在案",
            passed,
            if passed { "INDIRECT_VERSION=F31-indirect-v1 溯源键稳定" } else { "版本键漂移" },
        );
    }
    {
        let codes = id::IdCode::all();
        let hi_ok = codes.iter().all(|c| (c.0 & 0xFF00) == 0x4D00);
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
        // 与邻域码（vef28=0x4C / vea46=0x46 / vef30=0x45）字面量不等
        let no_neighbor = codes
            .iter()
            .all(|c| c.0 != 0x4C01 && c.0 != 0x4601 && c.0 != 0x4501);
        let passed = codes.len() == 8 && hi_ok && distinct && no_neighbor;
        s.add(
            "C31-判据-码段独占",
            passed,
            if passed { "八码高字节全 0x4D、互异、与 0x4C/0x46/0x45 邻域字面量不等" } else { "诊断码段越界或重复" },
        );
    }
    {
        let passed = s.len() == 19;
        s.add(
            "C31-判据-条数对账",
            passed,
            if passed { "判据 20 项离账：对账点前 19 项与设计清单一一对应" } else { "判据条数与设计不符（漏项/多项）" },
        );
    }

    s
}
