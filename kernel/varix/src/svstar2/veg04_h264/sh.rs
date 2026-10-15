//! VE-F1204 · H.264 解码器（VE-G 域 · G01 视频解码组 · L2/L3）
//!
//! **VE-F1204 · H.264 解码器 · §6 切片头**
//!
//! 由 `veg04_h264.rs` 按职责拆分而来（L5 子模块拆分，行为保持）。
//! 全部符号经 `veg04_h264` 的 `pub use` 再导出，对外路径不变。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::core::{DiagBag, DiagCode, Failure, H264Limits, R};
use super::nal::{BitReader, NalHeader, SliceType};
use super::ps::{Pps, Sps, TruncErr, TruncSome};

// §6 切片头（表 7-6/7-7 的解码相关字段）
// ════════════════════════════════════════════════════════════════════════════

/// 长期参考帧索引标记（DPB 标记语义的最小可执行形态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RefMarking {
    /// 滑动窗口（`SlidingWindow`）：解码顺序最旧的短周期帧被标记为「不用」。
    SlidingWindow,
    /// 自适应（`Adaptive`）：按 MMCO 命令显式标记。
    Adaptive { mmco: u8, long_term_frame_idx: u32 },
}

/// 切片头（解码相关字段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SliceHeader {
    pub first_mb_in_slice: u32,
    pub slice_type: SliceType,
    pub pic_parameter_set_id: u32,
    pub frame_num: u32,
    pub field_pic_flag: u8,
    pub bottom_field_flag: u8,
    pub idr_pic_id: u32,
    pub pic_order_cnt_lsb: u32,
    pub delta_pic_order_cnt_bottom: i32,
    pub redundant_pic_cnt: u32,
    pub direct_spatial_mv_pred_flag: u8,
    pub num_ref_idx_l0_active_minus1: u32,
    pub num_ref_idx_l1_active_minus1: u32,
    /// `dec_ref_pic_marking` 的语义最小化结果（只记 `adaptive`/IDR 两种形态）。
    pub long_term_reference_flag: u8,
    pub adaptive_ref_pic_marking_mode_flag: u8,
    pub mmco_count: u8,
    pub cabac_init_idc: u32,
    pub slice_qp_delta: i32,
    pub disable_deblocking_filter_idc: u32,
    pub slice_alpha_c0_offset_div2: i32,
    pub slice_beta_offset_div2: i32,
    pub slice_group_change_cycle: u32,
}

impl SliceHeader {
    /// 实际使用的 L0 参考列表长度。
    pub const fn l0_len(&self) -> u32 {
        self.num_ref_idx_l0_active_minus1 + 1
    }

    /// 实际使用的 L1 参考列表长度（B 切片）。
    pub const fn l1_len(&self) -> u32 {
        self.num_ref_idx_l1_active_minus1 + 1
    }

    /// DPB 标记语义。
    pub const fn ref_marking(&self) -> RefMarking {
        if self.adaptive_ref_pic_marking_mode_flag != 0 {
            RefMarking::Adaptive { mmco: self.mmco_count, long_term_frame_idx: 0 }
        } else {
            RefMarking::SlidingWindow
        }
    }
}

/// 解析切片头（输入为解防竞争后的 RBSP）。
///
/// 依赖已登记的 SPS/PPS（`sps` / `pps`）——这是规范要求的前置条件，不是可选优化。
/// 切片头的前缀区（规范 7.4.3.1 的 `first_mb_in_slice` .. `redundant_pic_cnt`）。
///
/// 只读，不依赖帧间/参考维度的信息：把「这块位序固定、逐字段读一段」的
/// 前缀从主解析里搬出来，主解析只留下与切片类型相关的分支。
struct SlicePrefix {
    frame_num: u32,
    field_pic_flag: u8,
    bottom_field_flag: u8,
    idr_pic_id: u32,
    pic_order_cnt_lsb: u32,
    delta_pic_order_cnt_bottom: i32,
    redundant_pic_cnt: u32,
}

fn parse_slice_prefix(
    r: &mut BitReader,
    sps: &Sps,
    pps: &Pps,
    nal: &NalHeader,
    limits: &H264Limits,
) -> R<SlicePrefix> {
    // 色度平面 id（仅 separate_colour_plane_flag=1）。
    if sps.separate_colour_plane_flag == 1 {
        r.read_bits(2).trunc_some("colour_plane_id")?;
    }

    let frame_num = r.read_bits(sps.log2_max_frame_num()).trunc_some("frame_num")?;

    let (field_pic_flag, bottom_field_flag) = if sps.frame_mbs_only_flag == 0 {
        let f = r.read_bit().trunc_some("field_pic_flag")?;
        let bf = if f == 1 { r.read_bit().trunc_some("bottom_field_flag")? } else { 0 };
        (f, bf)
    } else {
        (0, 0)
    };

    let idr_pic_id = if nal.is_idr_slice() {
        r.read_ue(limits).trunc_err("idr_pic_id")?
    } else {
        0
    };

    // POC
    let mut pic_order_cnt_lsb: u32 = 0;
    let mut delta_pic_order_cnt_bottom: i32 = 0;
    match sps.pic_order_cnt_type {
        0 => {
            if nal.is_idr_slice() {
                pic_order_cnt_lsb = 0;
            } else {
                pic_order_cnt_lsb = r
                    .read_bits(sps.log2_max_pic_order_cnt_lsb())
                    .trunc_some("pic_order_cnt_lsb")?;
            }
            if pps.bottom_field_pic_order_in_frame_present_flag == 1 && !nal.is_idr_slice() {
                delta_pic_order_cnt_bottom =
                    r.read_se(limits).trunc_err("delta_pic_order_cnt_bottom")?;
            }
        }
        1 => {
            if sps.delta_pic_order_always_zero_flag == 0 {
                delta_pic_order_cnt_bottom =
                    r.read_se(limits).trunc_err("delta_pic_order_cnt_bottom")?;
                if pps.bottom_field_pic_order_in_frame_present_flag == 1 && !nal.is_idr_slice() {
                    delta_pic_order_cnt_bottom +=
                        r.read_se(limits).trunc_err("delta_pic_order_cnt[1]")?;
                }
            }
        }
        _ => {}
    }

    let redundant_pic_cnt = if pps.redundant_pic_cnt_present_flag == 1 {
        r.read_ue(limits).trunc_err("redundant_pic_cnt")?
    } else {
        0
    };

    Ok(SlicePrefix {
        frame_num,
        field_pic_flag,
        bottom_field_flag,
        idr_pic_id,
        pic_order_cnt_lsb,
        delta_pic_order_cnt_bottom,
        redundant_pic_cnt,
    })
}

/// 参考列表改写循环（规范 7.4.3.3.2）的**单向**版本。
///
/// L0 与 L1 两条分支只差「读的是哪个 flag、计数写回哪个变量」，位序
/// 语义完全相同。合成一个函数后两侧只会实现一次，避免一边改了闸门
/// 另一边漏改（存量实现里这类「对称两边不同步」是最难对拍出来的缺陷）。
/// 返回该方向的改写条数。
fn skip_ref_list_modification(
    r: &mut BitReader,
    limits: &H264Limits,
) -> R<u32> {
    let flag = r.read_bit().trunc_some("ref_pic_list_modification_flag")?;
    if flag != 1 {
        return Ok(0);
    }
    // 本模块记录改写条数但**不改写参考列表顺序**——那是完整的 8.2.4.2
    // 过程，属 F1204 上游的「参考列表构造」子域。缺口在此点名。
    let mut count = 0u32;
    loop {
        let op = r.read_ue(limits).trunc_err("modification_of_pic_nums_idc")?;
        if op == 3 {
            break;
        }
        count += 1;
        if count > limits.max_ref_list_len as u32 {
            return Err(Failure::new(
                DiagCode::RefIndexOutOfRange,
                format!("参考列表改写条数超过上限 {}", limits.max_ref_list_len),
                "参考列表改写循环炸弹；拒绝解析（F1122 计数纪律）",
            ));
        }
        r.read_ue(limits).trunc_err("abs_diff_pic_num_minus1")?;
    }
    Ok(count)
}

/// `dec_ref_pic_marking`（规范 7.4.3.3.4 / 7.4.3.3.5）。
///
/// IDR 与非 IDR 是两条不同的位序支路，且非 IDR 的 MMCO 循环要逐操作码
/// 分派；这段逻辑独立于切片头其余部分，单独成函数后才能按自己的
/// 循环计数维度测试。
struct DecRefMarking {
    long_term_reference_flag: u8,
    adaptive_ref_pic_marking_mode_flag: u8,
    mmco_count: u8,
}

/// MMCO（`memory_management_control_operation`）循环的逐操作码分派。
/// 返回条数；操作码越出规范 [0,6] 显性拒绝（含 0——落在 `_` 臂，与原实现一致）。
fn parse_mmco_loop(r: &mut BitReader, limits: &H264Limits) -> R<u8> {
    let mut mmco_count: u8 = 0;
    loop {
        let op = r
            .read_ue(limits)
            .trunc_err("memory_management_control_operation")?;
        match op {
            1 | 3 => {
                r.read_ue(limits).trunc_err("difference_of_pic_nums_minus1")?;
                if op == 3 {
                    r.read_ue(limits).trunc_err("long_term_frame_idx")?;
                }
            }
            2 => {
                r.read_ue(limits).trunc_err("long_term_pic_num")?;
            }
            4 => {
                r.read_ue(limits)
                    .trunc_err("max_long_term_frame_idx_plus1")?;
            }
            5 => {}
            6 => {
                r.read_ue(limits).trunc_err("long_term_frame_idx")?;
            }
            _ => {
                return Err(Failure::new(
                    DiagCode::SpsGeometryInconsistent,
                    format!("MMCO 操作码 {} 超出规范 [0,6]", op),
                    "规范 7.4.3.3 只定义 0..6；其他值非法",
                ))
            }
        }
        mmco_count += 1;
        if op == 0 || mmco_count > 8 {
            return Ok(mmco_count);
        }
    }
}

fn parse_dec_ref_pic_marking(
    r: &mut BitReader,
    nal: &NalHeader,
    limits: &H264Limits,
) -> R<DecRefMarking> {
    let mut long_term_reference_flag: u8 = 0;
    let mut adaptive_ref_pic_marking_mode_flag: u8 = 0;
    let mut mmco_count: u8 = 0;
    if !nal.is_reference() {
        return Ok(DecRefMarking {
            long_term_reference_flag,
            adaptive_ref_pic_marking_mode_flag,
            mmco_count,
        });
    }
    if nal.is_idr_slice() {
        r.read_bit().trunc_some("no_output_of_prior_pics_flag")?;
        long_term_reference_flag = r.read_bit().trunc_some("long_term_reference_flag")?;
    } else {
        adaptive_ref_pic_marking_mode_flag =
            r.read_bit().trunc_some("adaptive_ref_pic_marking_mode_flag")?;
        if adaptive_ref_pic_marking_mode_flag == 1 {
            mmco_count = parse_mmco_loop(r, limits)?;
        }
    }
    Ok(DecRefMarking {
        long_term_reference_flag,
        adaptive_ref_pic_marking_mode_flag,
        mmco_count,
    })
}

/// 去块滤波参数（规范 7.4.3.1 末尾三个字段 + 合法性闸门）。
struct DeblockHeader {
    disable_deblocking_filter_idc: u32,
    slice_alpha_c0_offset_div2: i32,
    slice_beta_offset_div2: i32,
}

fn parse_slice_deblock(
    r: &mut BitReader,
    pps: &Pps,
    limits: &H264Limits,
) -> R<DeblockHeader> {
    let mut disable_deblocking_filter_idc: u32 = 0;
    let mut slice_alpha_c0_offset_div2: i32 = 0;
    let mut slice_beta_offset_div2: i32 = 0;
    if pps.deblocking_in_slice_header() {
        disable_deblocking_filter_idc = r
            .read_ue(limits)
            .trunc_err("disable_deblocking_filter_idc")?;
        if disable_deblocking_filter_idc != 1 {
            slice_alpha_c0_offset_div2 = r
                .read_se(limits)
                .trunc_err("slice_alpha_c0_offset_div2")?;
            slice_beta_offset_div2 = r.read_se(limits).trunc_err("slice_beta_offset_div2")?;
        }
        // idc 超出 [0,2] 即非法（规范 7.4.3.1）。
        if disable_deblocking_filter_idc > 2 {
            return Err(Failure::new(
                DiagCode::SpsGeometryInconsistent,
                format!("disable_deblocking_filter_idc {} 超出 [0,2]", disable_deblocking_filter_idc),
                "规范 7.4.3.1 只允许 0/1/2；其他值非法",
            ));
        }
    }
    Ok(DeblockHeader {
        disable_deblocking_filter_idc,
        slice_alpha_c0_offset_div2,
        slice_beta_offset_div2,
    })
}

fn parse_slice_group_change_cycle(r: &mut BitReader, pps: &Pps) -> R<u32> {
    if pps.num_slice_groups_minus1 > 0
        && pps.slice_group_map_type >= 3
        && pps.slice_group_map_type <= 5
    {
        let bits = ((pps.num_slice_groups_minus1 + 1).max(2) as u64)
            .checked_next_power_of_two()
            .map_or(0, |v| v.trailing_zeros());
        r.read_bits(bits).trunc_some("slice_group_change_cycle")
    } else {
        Ok(0)
    }
}

/// 活跃参考索引（规范 7.4.3.1 的 `num_ref_idx_active_override_flag` 段）。
struct ActiveRefIdx {
    l0: u32,
    l1: u32,
}

fn parse_active_ref_idx(
    r: &mut BitReader,
    slice_type: SliceType,
    pps: &Pps,
    limits: &H264Limits,
) -> R<ActiveRefIdx> {
    let (mut l0, mut l1) = (
        pps.num_ref_idx_l0_default_active_minus1,
        pps.num_ref_idx_l1_default_active_minus1,
    );
    if !slice_type.has_inter() {
        return Ok(ActiveRefIdx { l0, l1 });
    }
    let override_flag = r.read_bit().trunc_some("num_ref_idx_active_override_flag")?;
    if override_flag != 1 {
        return Ok(ActiveRefIdx { l0, l1 });
    }
    l0 = r.read_ue(limits).trunc_err("num_ref_idx_l0_active_minus1")?;
    // **规范 7.4.3 的范围闸门**：该字段限`0..=31`，
    // 且受 `limits.max_ref_list_len` 约束。
    // 缺了这两道检查，越界值会被静默接受——
    // 上层拿它当「参考表长度」去索引，索引越界读到的
    // 是别的DPB 槽位的像素，且全程无任何报错。
    if l0 > 31 || l0 >= limits.max_ref_list_len {
        return Err(Failure::new(
            DiagCode::RefIndexOutOfRange,
            format!(
                "num_ref_idx_l0_active_minus1 = {} 越界（规范限 0..=31，上限 {}）",
                l0, limits.max_ref_list_len
            ),
            "恶意参考表长度会让后续 ref_idx 索引越界读到别的 DPB 槽位；必须在参数层拒绝",
        ));
    }
    if slice_type == SliceType::B {
        l1 = r.read_ue(limits).trunc_err("num_ref_idx_l1_active_minus1")?;
        if l1 > 31 || l1 >= limits.max_ref_list_len {
            return Err(Failure::new(
                DiagCode::RefIndexOutOfRange,
                format!(
                    "num_ref_idx_l1_active_minus1 = {} 越界（规范限 0..=31，上限 {}）",
                    l1, limits.max_ref_list_len
                ),
                "同上（L1 方向）",
            ));
        }
    }
    Ok(ActiveRefIdx { l0, l1 })
}

/// 解析切片头（规范 7.4.3.1）。
///
/// 本函数只编排各子段的读取顺序：字段逐一对应规范表格，每个子段
/// 由同名子解析函数负责（前缀 / 参考列表长度 / 参考列表改写 /
/// 权重表跳过 / 参考标记 / QP / 去块参数 / slice group）。
/// 对外签名与调用方不变。
pub fn parse_slice_header(
    rbsp: &[u8],
    sps: &Sps,
    pps: &Pps,
    nal: &NalHeader,
    limits: &H264Limits,
    bag: &mut DiagBag,
) -> R<SliceHeader> {
    let mut r = BitReader::new(rbsp);
    let first_mb_in_slice = r.read_ue(limits).trunc_err("first_mb_in_slice")?;
    let slice_type_raw = r.read_ue(limits).trunc_err("slice_type")?;
    let slice_type = SliceType::from_raw(slice_type_raw).ok_or_else(|| {
        Failure::new(
            DiagCode::SliceTypeUnsupported,
            format!("slice_type {} 超出规范 [0,9]", slice_type_raw),
            "规范 7.3.3.1：slice_type 取值 0..9；其他值非法",
        )
    })?;
    let pic_parameter_set_id = r.read_ue(limits).trunc_err("pic_parameter_set_id")?;

    if pic_parameter_set_id != pps.pic_parameter_set_id {
        return Err(Failure::new(
            DiagCode::SpsPpsRefMissing,
            format!(
                "切片头声明 PPS {}，上下文 PPS 为 {}",
                pic_parameter_set_id, pps.pic_parameter_set_id
            ),
            "切片头与上下文的 PPS 不一致；拒绝解析（继续会读错熵引擎与 QP）",
        ));
    }

    let prefix = parse_slice_prefix(&mut r, sps, pps, nal, limits)?;

    let direct_spatial_mv_pred_flag = if slice_type == SliceType::B {
        r.read_bit().trunc_some("direct_spatial_mv_pred_flag")?
    } else {
        0
    };

    let active = parse_active_ref_idx(&mut r, slice_type, pps, limits)?;
    let l0 = active.l0;
    let l1 = active.l1;

    let list_mod_l0 = if slice_type.has_inter() {
        skip_ref_list_modification(&mut r, limits)?
    } else {
        0
    };
    let list_mod_l1 = if slice_type == SliceType::B {
        skip_ref_list_modification(&mut r, limits)?
    } else {
        0
    };

    if slice_type == SliceType::B {
        // pred_weight_table：本模块消费其比特位（避免错位）但不建权重表。
        // 权重表语义属残差加权预测，属F1222 编码器侧的对称面。
        if pps.weighted_bipred_idc == 1 {
            skip_pred_weight_table(&mut r, l0 + 1, l1 + 1, limits)?;
        }
    } else if slice_type == SliceType::P || slice_type == SliceType::Sp {
        if pps.weighted_pred_flag == 1 {
            skip_pred_weight_table(&mut r, l0 + 1, 1, limits)?;
        }
    }

    let marking = parse_dec_ref_pic_marking(&mut r, nal, limits)?;

    // cabac_init_idc（**仅当该 PPS 启用 CABAC 且切片非 I**）
    let cabac_init_idc = if pps.entropy_coding_mode_flag == 1 && slice_type != SliceType::I {
        r.read_ue(limits).trunc_err("cabac_init_idc")?
    } else {
        0
    };

    let slice_qp_delta = r.read_se(limits).trunc_err("slice_qp_delta")?;

    if slice_type == SliceType::Sp {
        r.read_bit().trunc_some("sp_for_switch_flag")?;
        r.read_se(limits).trunc_err("slice_qs_delta")?;
    }

    let deblock = parse_slice_deblock(&mut r, pps, limits)?;

    let slice_group_change_cycle = parse_slice_group_change_cycle(&mut r, pps)?;

    // I/Si 切片不应有参考列表长度覆盖；若切片类型声称帧间但引用数为 0，诊断。
    if slice_type.has_inter() && (l0 == 0 && slice_type != SliceType::B) {
        bag.push(
            DiagCode::RefIndexOutOfRange,
            format!("{} 切片的 L0 参考数为 0", slice_type.name()),
            "帧间切片必须有至少 1 个参考；解析继续但该切片不能解出帧间宏块",
        );
    }

    Ok(SliceHeader {
        first_mb_in_slice,
        slice_type,
        pic_parameter_set_id,
        frame_num: prefix.frame_num,
        field_pic_flag: prefix.field_pic_flag,
        bottom_field_flag: prefix.bottom_field_flag,
        idr_pic_id: prefix.idr_pic_id,
        pic_order_cnt_lsb: prefix.pic_order_cnt_lsb,
        delta_pic_order_cnt_bottom: prefix.delta_pic_order_cnt_bottom,
        redundant_pic_cnt: prefix.redundant_pic_cnt,
        direct_spatial_mv_pred_flag,
        num_ref_idx_l0_active_minus1: l0,
        num_ref_idx_l1_active_minus1: l1,
        long_term_reference_flag: marking.long_term_reference_flag,
        adaptive_ref_pic_marking_mode_flag: marking.adaptive_ref_pic_marking_mode_flag,
        mmco_count: marking.mmco_count,
        cabac_init_idc,
        slice_qp_delta,
        disable_deblocking_filter_idc: deblock.disable_deblocking_filter_idc,
        slice_alpha_c0_offset_div2: deblock.slice_alpha_c0_offset_div2,
        slice_beta_offset_div2: deblock.slice_beta_offset_div2,
        slice_group_change_cycle,
    })
}

/// 跳过 `pred_weight_table`（表 7-3 的权重表；本模块不建表，只精确消费比特）。
fn skip_pred_weight_table(r: &mut BitReader, l0: u32, l1: u32, limits: &H264Limits) -> R<()> {
    let chroma = 32i32;
    r.read_ue(limits).trunc_err("luma_log2_weight_denom")?;
    r.read_ue(limits).trunc_err("chroma_log2_weight_denom")?;
    for _ in 0..l0 {
        r.read_se(limits).trunc_err("luma_weight_l0")?;
        r.read_se(limits).trunc_err("luma_offset_l0")?;
    }
    for _ in 0..chroma {
        r.read_se(limits).trunc_err("chroma_weight_l0")?;
        r.read_se(limits).trunc_err("chroma_offset_l0")?;
    }
    for _ in 0..l1 {
        r.read_se(limits).trunc_err("luma_weight_l1")?;
        r.read_se(limits).trunc_err("luma_offset_l1")?;
    }
    for _ in 0..chroma {
        r.read_se(limits).trunc_err("chroma_weight_l1")?;
        r.read_se(limits).trunc_err("chroma_offset_l1")?;
    }
    Ok(())
}

// ════════════════════════════════════════════════════════════════════════════
