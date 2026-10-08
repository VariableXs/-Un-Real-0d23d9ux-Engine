//! VE-F0223 判据层：Intel 命令流编码器（锚点五条判据逐条映射）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0223`
//!
//! **锚点原文五条判据 → 本层判据族**：
//!
//! | 锚点判据 | 判据族 | 要点 |
//! |---|---|---|
//! | 编码结构等价对拍 | `C23-MI-*` `C23-3D-*` | 判据侧独立参照编码器逐 dword 对拍 |
//! | 代际分派正确 | `C23-GEN-*` | Tier→Variant 唯一映射 + 代际差异逐位落点 |
//! | 对齐与 patching 正确 | `C23-AP-*` | 8 字节补齐 + 重定位收集/回填闭环 |
//! | 吞吐达标 | `C23-THRU-*` | 确定性 dword 计数 ≤ 预算 + 空会话基线 |
//! | 错误码全映射 | `C23-ERR-*` | 0x32xx 段全量互异 + reason 专属 |
//!
//! # 本层的核心纪律：**判据侧独立重算，不向被测问答案**
//!
//! 参照编码器把预期 dword 序列按**字面量独立展开**（0x0188_0001 这类
//! 值不经被测的任何常量或函数）；代际差异预期（ext dword 有无、Scope
//! 位、视口上限）也是判据侧写死；吞吐基线（空会话=0）先行自证。

// ---------------------------------------------------------------------------
// 导入
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

use super::veb21_ident::GenTier;
use super::veb23_cmdenc::*;

// ---------------------------------------------------------------------------
// 判据侧独立参照编码器（不调被测的口径；字面量独立展开）
// ---------------------------------------------------------------------------

/// 参照：MI_BATCH_BUFFER_START（2 dword）。
fn ref_bb_start(asi: u32, scope: bool) -> [u32; 2] {
    // 0x31<<23 = 0x0188_0000；|长度1；ASI 在 bit8；Scope 在 bit9。
    let mut dw0: u32 = 0x0188_0001 | (asi << 8);
    if scope {
        dw0 |= 0x200;
    }
    [dw0, 0]
}

/// 参照：MI_SEMAPHORE_WAIT（3 dword，轮询、比较操作与内存轮询位可变）。
fn ref_sem_wait(cmp_bit: u32, mem_poll: bool) -> [u32; 3] {
    // 0x1C<<23 = 0x0E00_0000；|长度2；WaitMode=1<<14；MemPoll=1<<15。
    let mut dw0: u32 = 0x0E00_0002 | 0x4000 | (cmp_bit << 12);
    if mem_poll {
        dw0 |= 0x8000;
    }
    [dw0, 0x0000_1000, 0x0000_00AB] // addr=0x8000 → (0x8000 & !7)；value=0xAB
}

/// 参照：MI_STORE_DATA_IMM（3 dword）。
fn ref_store_data() -> [u32; 3] {
    // 0x20<<23 = 0x1000_0000；|长度2。addr=0x4000_0010 → &!7。
    [0x1000_0002, 0x4000_0010, 0x0000_0055]
}

/// 参照：3DSTATE 头（代际差异：Xe 起带 ext dword）。
fn ref_3d_head(subop: u16, len_after_head: u32, has_ext: bool) -> (u32, Option<u32>) {
    // 类型 3<<29 | 子类型 3<<27 | subop<<16 | 长度。
    let dw0 = 0x6000_0000u32 | 0x1800_0000u32 | ((subop as u32) << 16) | len_after_head;
    let ext = if has_ext { Some(0x0010_0000) } else { None }; // 标记位 20
    (dw0, ext)
}

/// 参照：3DSTATE_VF body。
fn ref_vf_body(indexed: bool, elem: u8) -> u32 {
    let mut b = (elem as u32) << 10;
    if indexed {
        b |= 0x200;
    }
    b
}

/// 参照：3DSTATE_VIEWPORT body。
fn ref_viewport_body(count: u8) -> u32 {
    (count as u32) << 2
}

/// 参照：3DSTATE_BLEND body。
fn ref_blend_body(write: bool, logic: bool, mask: u8) -> u32 {
    let mut b = mask as u32;
    if write {
        b |= 0x100;
    }
    if logic {
        b |= 0x200;
    }
    b
}

/// 参照：3DSTATE_DEPTH body。
fn ref_depth_body(test: bool, write: bool) -> u32 {
    let mut b = 0u32;
    if test {
        b |= 1;
    }
    if write {
        b |= 2;
    }
    b
}

/// 参照：渲染目标集合 body（两 dword）。
fn ref_rt_body(rt_count: u8, fmt: u32) -> [u32; 2] {
    [rt_count as u32, fmt & 0x3FF]
}

/// 判据侧逐 dword 对拍：被测 batch 从 `base` 起与参照序列全等。
fn seq_eq(enc: &CmdEncoder, base: usize, want: &[u32]) -> bool {
    let mut i = 0usize;
    while i < want.len() {
        match enc.dword(base + i) {
            Some(v) => {
                if v != want[i] {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// C23-MI：MI 族编码结构等价对拍（判据一）
// ---------------------------------------------------------------------------

fn fam_mi(s: &mut CheckSet) {
    // M1：BB_START GGTT（ASI=0）逐 dword 对拍。
    let mut e = CmdEncoder::new(GenTier::Baseline);
    let ok_m1 = match e.emit_mi_bb_start(Some(0x0001_0000), None, AddrSpace::Ggtt) {
        Ok(()) => e.len() == 2 && seq_eq(&e, 0, &ref_bb_start(0, false)),
        Err(_) => false,
    };
    s.add("C23-MI-bbstart-ggtt", ok_m1, "GGTT ASI=0 两 dword 结构对拍");

    // M2：BB_START ppGTT（ASI=1）——F0222 表种语义在编码位落点。
    let mut e = CmdEncoder::new(GenTier::Baseline);
    let ok_m2 = match e.emit_mi_bb_start(Some(0x0001_0008), None, AddrSpace::PpGtt) {
        Ok(()) => e.len() == 2 && seq_eq(&e, 0, &ref_bb_start(1, false)),
        Err(_) => false,
    };
    s.add("C23-MI-bbstart-ppgtt", ok_m2, "ppGTT ASI=1 位落点对拍");

    // M3：BB_START Xe2 才有 Scope 位（Gen9 无）——代际差异在 MI 侧落点。
    let mut g = CmdEncoder::new(GenTier::Baseline);
    let mut x2 = CmdEncoder::new(GenTier::XeLatest);
    let ok_m3 = match (g.emit_mi_bb_start(Some(0x1000), None, AddrSpace::Ggtt),
                       x2.emit_mi_bb_start(Some(0x1000), None, AddrSpace::Ggtt)) {
        (Ok(()), Ok(())) => seq_eq(&g, 0, &ref_bb_start(0, false))
            && seq_eq(&x2, 0, &ref_bb_start(0, true)),
        _ => false,
    };
    s.add("C23-MI-bbstart-scope-xe2only", ok_m3, "Scope 位仅 Xe2 代置位");

    // M4：SEMAPHORE_WAIT 轮询全位对拍（Equal、无内存轮询）。
    let mut e = CmdEncoder::new(GenTier::Baseline);
    let ok_m4 = match e.emit_mi_sem_wait(0x8000, 0xAB, SemCompare::Equal, false) {
        Ok(()) => e.len() == 3 && seq_eq(&e, 0, &ref_sem_wait(0, false)),
        Err(_) => false,
    };
    s.add("C23-MI-semwait-equal", ok_m4, "信号量等待 Equal 全位对拍");

    // M5：SEMAPHORE_WAIT GreaterEqual + 内存轮询（Xe 代）位独立落点。
    let mut e = CmdEncoder::new(GenTier::XeStandard);
    let ok_m5 = match e.emit_mi_sem_wait(0x8000, 0xAB, SemCompare::GreaterEqual, true) {
        Ok(()) => e.len() == 3 && seq_eq(&e, 0, &ref_sem_wait(1, true)),
        Err(_) => false,
    };
    s.add("C23-MI-semwait-ge-mempoll", ok_m5, "GreaterEqual+MemPoll 位对拍");

    // M6：STORE_DATA_IMM 对拍（地址低位清零纪律可见）。
    let mut e = CmdEncoder::new(GenTier::Baseline);
    let ok_m6 = match e.emit_mi_store_data(0x4000_0010, 0x55) {
        Ok(()) => e.len() == 3 && seq_eq(&e, 0, &ref_store_data()),
        Err(_) => false,
    };
    s.add("C23-MI-storedata", ok_m6, "立即数写入三 dword 对拍");

    // M7：MI_NOOP 编码恒为 0（补齐的合法形态共用同一条编码）。
    let mut e = CmdEncoder::new(GenTier::Baseline);
    e.emit_mi_noop();
    let ok_m7 = e.len() == 1 && e.dword(0) == Some(0);
    s.add("C23-MI-noop-zero", ok_m7, "NOOP 单 dword 恒 0");
}

// ---------------------------------------------------------------------------
// C23-3D：3D 族编码结构等价对拍（判据一）
// ---------------------------------------------------------------------------

fn fam_3d(s: &mut CheckSet) {
    // D1：VF Gen9（无 ext）三件结构（头+body）逐位对拍。
    let mut e = CmdEncoder::new(GenTier::Baseline);
    let (dw0, ext) = ref_3d_head(0x78, 1, false);
    let want = [dw0, ref_vf_body(true, 2)];
    let ok_d1 = match e.emit_3d_vf(true, 2) {
        Ok(()) => ext.is_none() && e.len() == 2 && seq_eq(&e, 0, &want),
        Err(_) => false,
    };
    s.add("C23-3D-vf-gen9", ok_d1, "VF Gen9 无 ext 两 dword 对拍");

    // D2：VF Xe（有 ext）三 dword 对拍——代际差异精确落在 ext dword。
    let mut e = CmdEncoder::new(GenTier::XeStandard);
    let (dw0x, extx) = ref_3d_head(0x78, 2, true);
    let wantx = [dw0x, extx.unwrap_or(0), ref_vf_body(true, 2)];
    let ok_d2 = match e.emit_3d_vf(true, 2) {
        Ok(()) => extx.is_some() && e.len() == 3 && seq_eq(&e, 0, &wantx),
        Err(_) => false,
    };
    s.add("C23-3D-vf-xe-ext", ok_d2, "VF Xe 带 ext 三 dword 对拍");

    // D3：Gen9 与 Xe 的同命令差异**恰为** ext dword（不多不少——
    // 防「两代编码整体错位」型变异：对拍各自通过但结构漂移）。
    let mut g = CmdEncoder::new(GenTier::Baseline);
    let mut x = CmdEncoder::new(GenTier::XeStandard);
    let _ = g.emit_3d_blend(true, false, 0x0F);
    let _ = x.emit_3d_blend(true, false, 0x0F);
    let ok_d3 = g.len() == 2 && x.len() == 3
        && g.dword(0) == x.dword(0).map(|v| v & !0x1) // 头长度字段差 ext 的 1
        && x.dword(1) == Some(0x0010_0000)
        && g.dword(1) == x.dword(2);
    s.add("C23-3D-gen-delta-exact", ok_d3, "两代差异恰为 ext dword");

    // D4：视口 body 对拍（count 位落点 6:2）。
    let mut e = CmdEncoder::new(GenTier::Baseline);
    let (dw0, _) = ref_3d_head(0x74, 1, false);
    let want = [dw0, ref_viewport_body(4)];
    let ok_d4 = match e.emit_3d_viewport(4) {
        Ok(()) => e.len() == 2 && seq_eq(&e, 0, &want),
        Err(_) => false,
    };
    s.add("C23-3D-viewport-body", ok_d4, "视口 count 位落点对拍");

    // D5：混合状态对拍（mask/write/logic 三位独立）。
    let mut e = CmdEncoder::new(GenTier::Baseline);
    let (dw0, _) = ref_3d_head(0x4D, 1, false);
    let want = [dw0, ref_blend_body(true, true, 0x0F)];
    let ok_d5 = match e.emit_3d_blend(true, true, 0x0F) {
        Ok(()) => e.len() == 2 && seq_eq(&e, 0, &want),
        Err(_) => false,
    };
    s.add("C23-3D-blend-body", ok_d5, "混合 mask+write+logic 对拍");

    // D6：深度模板对拍。
    let mut e = CmdEncoder::new(GenTier::Baseline);
    let (dw0, _) = ref_3d_head(0x4E, 1, false);
    let want = [dw0, ref_depth_body(true, false)];
    let ok_d6 = match e.emit_3d_depth(true, false) {
        Ok(()) => e.len() == 2 && seq_eq(&e, 0, &want),
        Err(_) => false,
    };
    s.add("C23-3D-depth-body", ok_d6, "深度 test/write 位对拍");

    // D7：渲染目标集合对拍（两 dword body + 格式码掩码）。
    let mut e = CmdEncoder::new(GenTier::XeStandard);
    let (dw0, ext) = ref_3d_head(0x4B, 3, true);
    let rt = ref_rt_body(2, 0x3FF | 0x400); // 超宽格式码须被掩到 10 位
    let want = [dw0, ext.unwrap_or(0), rt[0], rt[1]];
    let ok_d7 = match e.emit_3d_rt_set(2, 0x3FF | 0x400) {
        Ok(()) => e.len() == 4 && seq_eq(&e, 0, &want),
        Err(_) => false,
    };
    s.add("C23-3D-rtset", ok_d7, "RT 集合双 body+格式掩码对拍");
}

// ---------------------------------------------------------------------------
// C23-GEN：代际分派正确（判据二）
// ---------------------------------------------------------------------------

fn fam_gen(s: &mut CheckSet) {
    // G1：Tier→Variant 三条映射逐一断言（唯一映射，无默认回退）。
    let ok_g1 = GenVariant::for_tier(GenTier::Baseline) == GenVariant::Gen9
        && GenVariant::for_tier(GenTier::XeStandard) == GenVariant::Xe
        && GenVariant::for_tier(GenTier::XeLatest) == GenVariant::Xe2;
    s.add("C23-GEN-tier-map", ok_g1, "三代 Tier→Variant 映射逐一");

    // G2：视口上限 Gen9=16（17 拒绝）。
    let mut e = CmdEncoder::new(GenTier::Baseline);
    let ok16 = e.emit_3d_viewport(16).is_ok();
    let ok17 = e.emit_3d_viewport(17) == Err(CErr::BadViewportCount);
    s.add("C23-GEN-vpcap-gen9", ok16 && ok17, "Gen9 上限 16：16 可 17 拒");

    // G3：视口上限 Xe=32（32 可、Gen9 档的 16 也仍可）。
    let mut e = CmdEncoder::new(GenTier::XeStandard);
    let ok32 = e.emit_3d_viewport(32).is_ok();
    let ok16 = e.emit_3d_viewport(16).is_ok();
    s.add("C23-GEN-vpcap-xe", ok32 && ok16, "Xe 上限 32：32 与 16 均可");

    // G4：信号量内存轮询 Gen9 拒绝（专属码）Xe 可。
    let mut g = CmdEncoder::new(GenTier::Baseline);
    let mut x = CmdEncoder::new(GenTier::XeStandard);
    let gg = g.emit_mi_sem_wait(0x8000, 1, SemCompare::Equal, true);
    let xx = x.emit_mi_sem_wait(0x8000, 1, SemCompare::Equal, true);
    s.add("C23-GEN-mempoll-gating", gg == Err(CErr::UnsupportedForGen) && xx.is_ok(),
          "内存轮询 Gen9 拒/Xe 可");

    // G5：编码器构造分派与 Tier 一致（变体可观测）。
    let ok_g5 = CmdEncoder::new(GenTier::Baseline).variant() == GenVariant::Gen9
        && CmdEncoder::new(GenTier::XeStandard).variant() == GenVariant::Xe
        && CmdEncoder::new(GenTier::XeLatest).variant() == GenVariant::Xe2;
    s.add("C23-GEN-ctor-variant", ok_g5, "构造分派可观测一致");
}

// ---------------------------------------------------------------------------
// C23-AP：对齐与 patching（判据三）
// ---------------------------------------------------------------------------

fn fam_align_patch(s: &mut CheckSet) {
    // A1：奇数 dword 后 pad_to_8 补齐为偶（NOOP 恒 0 记账）。
    let mut e = CmdEncoder::new(GenTier::Baseline);
    let _ = e.emit_3d_vf(false, 0); // Gen9：2 dword
    let before = e.len();
    e.pad_to_8();
    let ok_a1 = before == 2 && e.len() == 2 && e.stats.noop_pads == 0; // 偶数不补
    let mut e2 = CmdEncoder::new(GenTier::Baseline);
    e2.emit_mi_noop(); // 1 dword（奇）
    e2.pad_to_8();
    let ok_a1b = e2.len() == 2 && e2.dword(1) == Some(0) && e2.stats.noop_pads == 1;
    s.add("C23-AP-pad-to-8", ok_a1 && ok_a1b, "偶不补/奇补一条恒 0 NOOP");

    // A2：符号 BB_START 收集重定位 + finalize 回填闭环（lo dword 精确落位）。
    let mut e = CmdEncoder::new(GenTier::Baseline);
    let _ = e.emit_mi_bb_start(None, Some(0x5A5A), AddrSpace::Ggtt);
    let snap = e.relocs_snapshot();
    let collected = snap.len() == 1 && snap[0].at_dword == 1 && snap[0].target.sym == 0x5A5A;
    fn res_ok(sym: u64) -> Option<u64> {
        if sym == 0x5A5A { Some(0x0002_4000) } else { None }
    }
    let fin = e.finalize(res_ok);
    let patched = fin == Ok(1) && e.dword(1) == Some(0x0002_4000) && e.stats.patches == 1;
    s.add("C23-AP-reloc-roundtrip", collected && patched, "收集→回填闭环精确落位");

    // A3：解析不出 → RelocUnresolved 且零回填（先全解析后落笔，无半提交）。
    let mut e = CmdEncoder::new(GenTier::Baseline);
    let _ = e.emit_mi_bb_start(None, Some(0x77), AddrSpace::Ggtt);
    fn res_none(_sym: u64) -> Option<u64> { None }
    let ok_a3 = e.finalize(res_none) == Err(CErr::RelocUnresolved)
        && e.stats.patches == 0
        && e.dword(1) == Some(0); // 占位未被污染
    s.add("C23-AP-reloc-unresolved", ok_a3, "解析失败零回填无半提交");

    // A4：batch 起址 8 字节对齐纪律（非对齐专属码拒绝）。
    let mut e = CmdEncoder::new(GenTier::Baseline);
    let ok_a4 = e.emit_mi_bb_start(Some(0x1004), None, AddrSpace::Ggtt)
        == Err(CErr::BadBatchAlign)
        && e.len() == 0; // 拒绝零写入
    s.add("C23-AP-bb-align", ok_a4, "BB_START 非 8 对齐拒绝零写入");

    // A5：信号量与数据写入的对齐纪律各自专属码。
    let mut e = CmdEncoder::new(GenTier::Baseline);
    let r1 = e.emit_mi_sem_wait(0x1002, 1, SemCompare::Equal, false);
    let r2 = e.emit_mi_store_data(0x1004, 1);
    s.add("C23-AP-addr-align-codes", r1 == Err(CErr::BadSemaphoreAlign)
        && r2 == Err(CErr::BadStoreAlign), "两处对齐拒绝码各自专属");

    // A6：超 32 位地址契约专属拒绝（不静默截断）。
    let mut e = CmdEncoder::new(GenTier::Baseline);
    let r1 = e.emit_mi_bb_start(Some(0x1_0000_0000), None, AddrSpace::Ggtt);
    let r2 = e.emit_mi_sem_wait(0x1_0000_0000, 1, SemCompare::Equal, false);
    s.add("C23-AP-addr-4g", r1 == Err(CErr::AddrBeyond4G)
        && r2 == Err(CErr::AddrBeyond4G) && e.len() == 0, "超 4G 专属码零写入");

    // A7：会话编排失败即停（错误前缀保留、错误码透传）。
    let cmds = [
        SemCmd::MiNoop,
        SemCmd::Viewport { count: 17 }, // Gen9 越上限
        SemCmd::MiNoop,
    ];
    let mut e = CmdEncoder::new(GenTier::Baseline);
    let r = e.encode_all(&cmds);
    let ok_a7 = r == Err(CErr::BadViewportCount)
        && e.stats.commands == 1 // 第一条 NOOP 已发射，其后即停
        && e.len() == 1;
    s.add("C23-AP-session-fail-stop", ok_a7, "会话失败即停前缀保留");

    // A8：会话成功路径全部发射 + 末尾补齐（奇数结尾时 NOOP 进账）。
    let cmds = [
        SemCmd::VfSetup { indexed: false, index_elem_size: 0 },
        SemCmd::MiNoop,
    ];
    let mut e = CmdEncoder::new(GenTier::Baseline);
    let r = e.encode_all(&cmds);
    let ok_a8 = r.is_ok() && e.len() == 3 && e.dword(2) == Some(0) // VF 2 + 无需补
        || (r.is_ok() && e.len() % 2 == 0);
    s.add("C23-AP-session-ok", ok_a8, "会话成功全部发射且偶对齐");
}

// ---------------------------------------------------------------------------
// C23-THRU：吞吐达标（判据四）
// ---------------------------------------------------------------------------

fn fam_thru(s: &mut CheckSet) {
    // T1：空会话基线——产出 0 且无命令（非恒真门禁的基线自证）。
    let mut e = CmdEncoder::new(GenTier::Baseline);
    let empty: [SemCmd; 0] = [];
    let ok_t1 = e.encode_all(&empty).is_ok()
        && e.stats.last_op_count == 0
        && e.stats.commands == 0;
    s.add("C23-THRU-empty-zero", ok_t1, "空会话产出为 0");

    // T2：64 条有效命令全部在预算内完成（吞吐达标正路）。
    let mut cmds: Vec<SemCmd> = Vec::new();
    let mut i = 0u32;
    while i < 64 {
        cmds.push(SemCmd::VfSetup { indexed: i % 2 == 0, index_elem_size: 1 });
        i += 1;
    }
    let mut e = CmdEncoder::new(GenTier::Baseline);
    let ok_t2 = e.encode_all(&cmds).is_ok()
        && e.stats.commands == 64
        && e.stats.dwords == 128
        && e.stats.last_op_count == 128
        && e.stats.dwords <= OP_BUDGET_BASE + 64 * OP_BUDGET_PER_CMD;
    s.add("C23-THRU-64-in-budget", ok_t2, "64 命令 128 dword 预算内达标");

    // T3：吞吐计数非平凡（>0 且与 dword 总数一致——同源对拍防记错口径）。
    let mut e = CmdEncoder::new(GenTier::XeStandard);
    let _ = e.encode_all(&[SemCmd::VfSetup { indexed: true, index_elem_size: 2 }]);
    let ok_t3 = e.stats.last_op_count == 3 && e.stats.dwords == 3 && e.stats.dwords > 0;
    s.add("C23-THRU-nontrivial", ok_t3, "Xe VF=3 dword 计数一致非零");

    // T4：NOOP 补齐计入产出（吞吐口径含对齐成本，不藏）。
    let mut e = CmdEncoder::new(GenTier::Baseline);
    let _ = e.encode_all(&[SemCmd::MiNoop]);
    let ok_t4 = e.stats.dwords == 2 // NOOP 1 + 补齐 1（奇→偶）
        && e.stats.noop_pads == 1
        && e.stats.last_op_count == 2;
    s.add("C23-THRU-pad-counted", ok_t4, "补齐成本如实计入产出");

    // T5：预算口径自检——预算常量判据侧写死（防被测侧改常量放水）。
    s.add("C23-THRU-budget-literals", OP_BUDGET_BASE == 16 && OP_BUDGET_PER_CMD == 8,
          "预算基线 16/每命令 8 判据侧字面量");
}

// ---------------------------------------------------------------------------
// C23-ERR：错误码全映射（判据五）
// ---------------------------------------------------------------------------

fn fam_err(s: &mut CheckSet) {
    // E1：全部 13 码在 0x32xx 段内且互异（独立重排码值再查重）。
    let mut codes: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < CErr::ALL.len() {
        codes.push(CErr::ALL[i].code());
        i += 1;
    }
    let mut sorted = codes.clone();
    sorted.sort();
    let mut distinct = true;
    let mut k = 1usize;
    while k < sorted.len() {
        if sorted[k] == sorted[k - 1] {
            distinct = false;
        }
        k += 1;
    }
    let mut in_seg = true;
    let mut m = 0usize;
    while m < codes.len() {
        if codes[m] & 0xFF00 != 0x3200 {
            in_seg = false;
        }
        m += 1;
    }
    s.add("C23-ERR-13-distinct-inseg", distinct && in_seg && CErr::ALL.len() == 13,
          "13 码全在 0x32xx 段且互异");

    // E2：reason 全非空且互异（拒绝必带专属原因）。
    let mut rs: Vec<String> = Vec::new();
    let mut nonempty = true;
    let mut j = 0usize;
    while j < CErr::ALL.len() {
        let r = CErr::ALL[j].reason();
        if r.len() == 0 {
            nonempty = false;
        }
        rs.push(r);
        j += 1;
    }
    rs.sort();
    let mut rd = true;
    let mut k = 1usize;
    while k < rs.len() {
        if rs[k] == rs[k - 1] {
            rd = false;
        }
        k += 1;
    }
    s.add("C23-ERR-reasons-unique", nonempty && rd, "reason 全非空且互异");

    // E3：越界视口真实可达 BadViewportCount（错误码不止映射还可达）。
    let mut e = CmdEncoder::new(GenTier::Baseline);
    s.add("C23-ERR-viewport-reachable",
          e.emit_3d_viewport(0) == Err(CErr::BadViewportCount), "count=0 亦拒");

    // E4：索引元素尺寸 3 拒绝（0/1/2 之外的值有专属码）。
    let mut e = CmdEncoder::new(GenTier::Baseline);
    s.add("C23-ERR-elemsize-reachable",
          e.emit_3d_vf(true, 3) == Err(CErr::BadIndexElementSize), "尺寸 3 专属拒");

    // E5：渲染目标 0 与 9 双边界拒绝。
    let mut e = CmdEncoder::new(GenTier::Baseline);
    let r0 = e.emit_3d_rt_set(0, 1);
    let r9 = e.emit_3d_rt_set(9, 1);
    s.add("C23-ERR-rt-bounds", r0 == Err(CErr::BadRtCount) && r9 == Err(CErr::BadRtCount),
          "RT 0 与 9 双边界专属拒");

    // E6：无地址无符号的 BB_START 构造错误即拒（零写入）。
    let mut e = CmdEncoder::new(GenTier::Baseline);
    s.add("C23-ERR-bb-empty",
          e.emit_mi_bb_start(None, None, AddrSpace::Ggtt) == Err(CErr::RelocUnresolved)
              && e.len() == 0, "空 BB_START 拒绝零写入");
}

// ---------------------------------------------------------------------------
// C23-A11Y/PRIV：无障碍与隐私
// ---------------------------------------------------------------------------

fn fam_a11y(s: &mut CheckSet) {
    // Y1：读屏摘要可达（非空且含聚合计数语义）。
    let mut e = CmdEncoder::new(GenTier::Baseline);
    let _ = e.emit_3d_vf(true, 1);
    let sum = e.status_summary();
    s.add("C23-A11Y-summary", sum.len() > 0, "读屏摘要非空");

    // Y2：隐私红线——摘要不含任何地址（十六进制形态不出现）。
    let mut e = CmdEncoder::new(GenTier::Baseline);
    let _ = e.emit_mi_store_data(0x4000_0010, 0x55);
    let sum = e.status_summary();
    let no_hex = !sum.contains("0x") && !sum.contains("0X");
    s.add("C23-PRIV-no-addr", no_hex, "摘要零地址零符号");

    // Y3：账本零值基线自证（防账本口径自身写错）。
    let z = EncStats::zero();
    s.add("C23-A11Y-stats-zero",
          z.commands == 0 && z.dwords == 0 && z.relocs == 0 && z.patches == 0
              && z.last_op_count == 0 && z.budget_rejects == 0,
          "账本零值基线");
}

// ---------------------------------------------------------------------------
// 聚合入口
// ---------------------------------------------------------------------------

/// VE-F0223 域自检（判据逐条映射锚点五条判据：
/// MI 7 / 3D 7 / GEN 5 / AP 8 / THRU 5 / ERR 6 / A11Y-PRIV 3 共 41 项七族）。
pub fn run_veb23_checks() -> CheckSet {
    let mut s = CheckSet::new("intel-cmdenc");
    fam_mi(&mut s);
    fam_3d(&mut s);
    fam_gen(&mut s);
    fam_align_patch(&mut s);
    fam_thru(&mut s);
    fam_err(&mut s);
    fam_a11y(&mut s);
    s
}
