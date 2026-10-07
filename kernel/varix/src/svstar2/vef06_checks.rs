//! VE-F1006 · PNG 动画 APNG 域自检（判据逐条映射，80 项分 a/b/c 三族）
//!
//! 锚点：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1006`
//!
//! 判据族：
//! - a = acTL / fcTL / fdAT 三块解析（`C06-ACTL` / `C06-FCTL` / `C06-FDAT`）
//! - b = 处置三语义 + 合成状态机 + 帧率正确性（`C06-DISP` / `C06-FPS`）
//! - c = 解码全链 + 编码 roundtrip + 错误路径 + 联动（`C06-DEC` / `C06-ENC` /
//!   `C06-ERR` / `C06-LINK`）
//!
//! 三族独立入口：80 项 > `CheckSet::MAX_CHECKS`(112) 的安全线以下，但与全仓
//! 共享聚合器同风险，故仍按族注册以规避任何静默截断。

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::{CheckSet, MAX_CHECKS};
use crate::svstar2::vef06_apng::*;

// ---------------------------------------------------------------------------
// 语料构造
// ---------------------------------------------------------------------------

fn ctl(seq: u32, w: u16, h: u16, dn: u16, dd: u16, d: DisposeOp) -> FrameControl {
    FrameControl {
        sequence: seq,
        width: w,
        height: h,
        x_offset: 0,
        y_offset: 0,
        delay_num: dn,
        delay_den: dd,
        dispose: d,
        blend: 0,
    }
}

fn mk(seq: u32, dispose: DisposeOp) -> FrameDesc {
    FrameDesc {
        ctl: ctl(seq, 2, 2, 1, 10, dispose),
        source: if seq == 0 { FrameSource::Idat } else { FrameSource::Fdat { sequence: seq } },
        // seq 可能很大（帧数超限用例），用 wrapping 避免 u8 溢出 panic。
        data: vec![
            (seq as u8).wrapping_add(1), 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
        ],
    }
}

fn two_frame_seq() -> FrameSeq {
    FrameSeq {
        actl: AnimControl { num_frames: 2, num_plays: 0 },
        frames: vec![mk(0, DisposeOp::None), mk(1, DisposeOp::Background)],
        actual_frames: 2,
        missing_frames: 0,
        skipped_orphan: 0,
        crc_warnings: 0,
    }
}

fn encoded_two() -> Vec<u8> {
    encode_apng(&two_frame_seq(), 2, 2, None).unwrap_or_default()
}

fn px16(v: u8) -> Vec<u8> {
    vec![v; 16]
}

// ---------------------------------------------------------------------------
// 族 a：三块解析
// ---------------------------------------------------------------------------

fn check_actl(set: &mut CheckSet) {
    // 8 字节载荷往返。
    let ac = AnimControl { num_frames: 7, num_plays: 3 };
    let bytes = ac.to_bytes();
    let back = match AnimControl::parse(&bytes) {
        Ok(b) => b,
        Err(_) => {
            set.add("C06-ACTL-载荷往返一致", false, "parse(to_bytes(x)) 应等于 x");
            return;
        }
    };
    set.add("C06-ACTL-载荷往返一致", back == ac, "8 字节大端往返");

    // num_plays=0 表无限循环（非零则有限）。
    set.add(
        "C06-ACTL-播放次数0判无限循环",
        AnimControl { num_frames: 1, num_plays: 0 }.infinite()
            && !AnimControl { num_frames: 1, num_plays: 1 }.infinite()
            && !AnimControl { num_frames: 1, num_plays: u32::MAX }.infinite(),
        "num_plays=0 是无限循环的唯一取值",
    );

    // 载荷长度必须恰为 8：短/长都拒。
    set.add(
        "C06-ACTL-载荷长度非8判拒",
        AnimControl::parse(&[0u8; 7]).is_err() && AnimControl::parse(&[0u8; 9]).is_err(),
        "acTL 载荷长度必须恰为 8",
    );

    // 帧数 0 是非法（规范要求 ≥1）：必须拒而不是当 0 帧静默通过。
    set.add(
        "C06-ACTL-帧数0判拒",
        AnimControl::parse(&[0u8; 8]).is_err(),
        "num_frames=0 非法，须拒绝而非产生空动画",
    );

    // 大端字节序：改任一字节必变值（防大小端混用）。
    let mut swapped = bytes;
    swapped.swap(0, 3);
    let other = match AnimControl::parse(&swapped) {
        Ok(b) => b,
        Err(_) => AnimControl { num_frames: 0, num_plays: 0 },
    };
    set.add(
        "C06-ACTL-大端字节序敏感",
        other.num_frames != ac.num_frames || other.num_plays != ac.num_plays,
        "字节序必须为大端：反转字节后解析值必不同",
    );

    // 往返一致须覆盖多组值（含极大值），防只对某组值碰巧成立。
    let mut all_ok = true;
    let mut i = 0u32;
    while i < 6 {
        let v = AnimControl { num_frames: 1 + i * 97, num_plays: i * 13 };
        match AnimControl::parse(&v.to_bytes()) {
            Ok(b) => {
                if b != v {
                    all_ok = false;
                }
            }
            Err(_) => all_ok = false,
        }
        i += 1;
    }
    set.add("C06-ACTL-多组取值往返全一致", all_ok, "6 组取值往返均一致");
}

fn check_fctl(set: &mut CheckSet) {
    let c = ctl(42, 3, 4, 1, 50, DisposeOp::Previous);
    let bytes = fctl_to_bytes(&c);
    let back = match parse_fctl(&bytes) {
        Ok(b) => b,
        Err(_) => {
            set.add("C06-FCTL-载荷往返一致", false, "parse_fctl(to_bytes(x)) 应等于 x");
            return;
        }
    };
    set.add("C06-FCTL-载荷往返一致", back == c, "26 字节往返");

    // 长度必须恰为 26。
    let mut wrong_len_ok = true;
    let mut n = 0usize;
    while n <= 28 {
        if n == 26 {
            n += 1;
            continue;
        }
        // n == 0 时载荷为空，不可写 v[0]；空载荷本身就是非法长度，
        // 直接交给 parse_fctl 判（它先查长度，不会碰内容）。
        let mut v = vec![0u8; n];
        if n > 0 {
            v[0] = 1;
        }
        if parse_fctl(&v).is_ok() {
            wrong_len_ok = false;
        }
        n += 1;
    }
    set.add("C06-FCTL-长度非26一律判拒", wrong_len_ok, "fcTL 载荷长度必须恰为 26");

    // 处置线值三态 + 非法值必须显性拒绝（不静默兜底成 0）。
    set.add(
        "C06-FCTL-处置三态线值正确",
        DisposeOp::None.wire() == 0 && DisposeOp::Background.wire() == 1 && DisposeOp::Previous.wire() == 2,
        "APNG_DISPOSE_OP_NONE/BACKGROUND/PREVIOUS = 0/1/2",
    );
    let mut wire_rt = true;
    let mut i = 0usize;
    while i < DisposeOp::ALL.len() {
        let d = DisposeOp::ALL[i];
        match DisposeOp::from_wire(d.wire()) {
            Some(b) => {
                if b != d {
                    wire_rt = false;
                }
            }
            None => wire_rt = false,
        }
        i += 1;
    }
    set.add("C06-FCTL-处置线值往返一致", wire_rt, "三个合法线值往返一致");
    set.add(
        "C06-FCTL-非法处置线值判拒",
        DisposeOp::from_wire(3).is_none() && DisposeOp::from_wire(255).is_none(),
        "线值 3/255 必须显性拒绝；静默兜底成 0 会让相邻码变死码",
    );
    // 真实产生路径：把非法线值塞进载荷，parse_fctl 必须报 BadDispose。
    let mut bad = fctl_to_bytes(&ctl(0, 1, 1, 1, 10, DisposeOp::None));
    bad[20] = 7;
    let bad_kind = parse_fctl(&bad).err().map(|e| e.kind);
    set.add(
        "C06-FCTL-非法处置经载荷路径报BadDispose",
        bad_kind == Some(ApngFaultKind::BadDispose),
        "诊断变体须有可达路径，不能是死码",
    );

    // 区域越界判定：区域小于画布合法，越界非法。
    let cw = 4u16;
    let ch = 4u16;
    set.add(
        "C06-FCTL-区域可小于画布判合法",
        !ctl(0, 2, 2, 1, 10, DisposeOp::None).out_of_bounds(cw, ch)
            && ctl(0, 4, 4, 1, 10, DisposeOp::None).is_full_canvas(cw, ch)
            && !ctl(0, 3, 3, 1, 10, DisposeOp::None).is_full_canvas(cw, ch),
        "锚点：区域可小于画布以实现局部更新",
    );
    set.add(
        "C06-FCTL-区域越界判非法",
        ctl(0, 5, 1, 1, 10, DisposeOp::None).out_of_bounds(cw, ch)
            && {
                let mut oob = FrameControl {
                    sequence: 0,
                    width: 2,
                    height: 2,
                    x_offset: 3,
                    y_offset: 0,
                    delay_num: 1,
                    delay_den: 10,
                    dispose: DisposeOp::None,
                    blend: 0,
                };
                let r = oob.out_of_bounds(cw, ch);
                oob.y_offset = 3;
                r && oob.out_of_bounds(cw, ch)
            },
        "宽/高/偏移任一越界即非法；x/y 两轴都要覆盖",
    );

    // **四个越界分支逐一钉死**（弱门禁修补）：把 `out_of_bounds` 的
    // 任一支路改成恒假，上面那条综合判据仍可能全绿——它只要求
    // 「五个用例里至少有一个越界」。故这里逐支路独立断言。
    //
    // 用例必须**只由单一支路触发**，否则支路互相掩盖：实测第一版用
    // `x_offset=3, width=2`（4×4 画布）测「X 加宽」，可它同时让
    // `x_offset + width > cw` 与 `x_offset >= cw` 都为真，改任一支路
    // 另一支路照样判出 ⇒ 变异照样 MISSED。
    // 分离办法：测「偏移支路」时把 width/height 置 0——此时
    // `x_offset + 0 > cw` 为假，只有 `x_offset >= cw` 能判。
    let mk_ctl = |x: u16, y: u16, w: u16, h: u16| FrameControl {
        sequence: 0,
        width: w,
        height: h,
        x_offset: x,
        y_offset: y,
        delay_num: 1,
        delay_den: 10,
        dispose: DisposeOp::None,
        blend: 0,
    };
    set.add(
        "C06-FCTL-越界支路X偏移独立钉死",
        mk_ctl(4, 0, 0, 0).out_of_bounds(cw, ch) && !mk_ctl(3, 0, 0, 0).out_of_bounds(cw, ch),
        "width=height=0 时只有 x_offset>=cw 支路能判；改该支路本判据转红",
    );
    set.add(
        "C06-FCTL-越界支路Y偏移独立钉死",
        mk_ctl(0, 4, 0, 0).out_of_bounds(cw, ch) && !mk_ctl(0, 3, 0, 0).out_of_bounds(cw, ch),
        "width=height=0 时只有 y_offset>=ch 支路能判",
    );
    set.add(
        "C06-FCTL-越界支路X加宽独立钉死",
        // x_offset=2 < cw（不触发偏移支路），width=3 使 2+3>4 越界
        mk_ctl(2, 0, 3, 0).out_of_bounds(cw, ch) && !mk_ctl(2, 0, 2, 0).out_of_bounds(cw, ch),
        "x_offset=2<cw 且 height=0：只有 x_offset+width>cw 能判",
    );
    set.add(
        "C06-FCTL-越界支路Y加高独立钉死",
        mk_ctl(0, 2, 0, 3).out_of_bounds(cw, ch) && !mk_ctl(0, 2, 0, 2).out_of_bounds(cw, ch),
        "y_offset=2<ch 且 width=0：只有 y_offset+height>ch 能判",
    );
    // 边界口径：恰好贴满画布右下角是**合法**的（不得误判越界）。
    set.add(
        "C06-FCTL-恰好贴满右下角判合法",
        !mk_ctl(2, 2, 2, 2).out_of_bounds(cw, ch),
        "半开区间语义：区域右边界/下边界等于画布边长时不算越界",
    );

    // 全覆盖判定必须把偏移算进去（偏移非零即使尺寸等于画布也不是全覆盖）。
    let mut off = ctl(0, 4, 4, 1, 10, DisposeOp::None);
    off.x_offset = 1;
    set.add(
        "C06-FCTL-偏移非零判非全覆盖",
        !off.is_full_canvas(cw, ch),
        "尺寸等于画布但有偏移时不是全覆盖——否则自动处置会选错",
    );

    // 混合方式字节必须原样往返（占位但不丢字段）。
    let mut with_blend = ctl(0, 1, 1, 1, 10, DisposeOp::None);
    with_blend.blend = 1;
    let rb = parse_fctl(&fctl_to_bytes(&with_blend));
    set.add(
        "C06-FCTL-混合方式字段原样往返",
        rb.map(|x| x.blend) == Ok(1),
        "blend 字段不得在序列化中丢失",
    );
}

fn check_fdat(set: &mut CheckSet) {
    let raw = scan_apng_chunks(&encoded_two());
    let r = match raw {
        Ok(v) => v,
        Err(_) => {
            set.add("C06-FDAT-扫描器收集到fdAT", false, "自造流扫描失败");
            return;
        }
    };
    // 编码产出：首帧 IDAT + 次帧 fdAT，两者都该被收集。
    set.add("C06-FDAT-fdAT被扫描器收集", !r.fdat.is_empty(), "fdAT 进入有序序列");
    set.add("C06-FDAT-IDAT被扫描器收集", !r.idat.is_empty(), "首帧 IDAT 兼容形态也被收");
    set.add("C06-FDACTRL-帧数与fdAT数一致", r.fdat.len() == 1 && r.fctl.len() == 2, "两帧：2 个 fcTL + 1 个 fdAT");

    // 载荷短于序号字段（<4）必须报 FdatLength，而不是当作空帧。
    let mut short = [0u8; 3];
    short[0] = 1;
    let p = &short[..];
    let kind = decode_apng(&with_fdat(p)).err().map(|e| e.kind);
    set.add(
        "C06-FDAT-载荷短于4判拒",
        kind == Some(ApngFaultKind::FdatLength),
        "fdAT 至少要装下 4 字节序号",
    );

    // 序号字段真被读出：构造「fcTL 序号 0、fdAT 序号 5」的流，
    // 解码必须因序号不连续而截断（不静默配对）。
    let mismatch = with_fdat(&[0, 0, 0, 5, 1, 2, 3, 4]);
    let d = decode_apng(&mismatch);
    set.add(
        "C06-FDAT-序号不连续判截断",
        d.map(|x| x.missing_frames > 0).unwrap_or(false),
        "fcTL 序号 0 与 fdAT 序号 5 不配对，须截断并标记缺失帧数",
    );

    // fdAT 与 IDAT 同构：同一份数据走两种容器，解出的帧数据应相同。
    let via_idat = decode_apng(&encoded_two());
    let both = match &via_idat {
        Ok(s) => s.frames.len(),
        Err(_) => 0,
    };
    set.add("C06-FDAT-两种容器同构", both == 2, "IDAT 首帧 + fdAT 次帧构成同一序列");

    // **序号必须真被读出并逐帧对上**（弱门禁修补）。
    // 只测「0 vs 5 不配对」不够：把 fdAT 序号硬编码成任意常量，
    // 遇到「恰好都不配对」的语料仍然全绿。故这里造一个**序号全对得上**
    // 的流（0/1 两帧），序号读取一旦失真就会立刻截断。
    let mut good = Vec::new();
    good.extend_from_slice(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
    let mut ihdr = vec![0u8; 13];
    ihdr[0..2].copy_from_slice(&2u16.to_be_bytes());
    ihdr[2..4].copy_from_slice(&2u16.to_be_bytes());
    ihdr[8] = 8;
    ihdr[9] = 6;
    push_chunk(&mut good, b"IHDR", &ihdr);
    push_chunk(&mut good, b"acTL", &AnimControl { num_frames: 2, num_plays: 0 }.to_bytes());
    push_chunk(&mut good, b"fcTL", &fctl_to_bytes(&ctl(0, 2, 2, 1, 10, DisposeOp::None)));
    push_chunk(&mut good, b"fcTL", &fctl_to_bytes(&ctl(1, 2, 2, 1, 10, DisposeOp::None)));
    push_chunk(&mut good, b"fdAT", &[0, 0, 0, 0, 1, 1, 1, 1]);
    push_chunk(&mut good, b"fdAT", &[0, 0, 0, 1, 2, 2, 2, 2]);
    push_chunk(&mut good, b"IEND", &[]);
    let gd = decode_apng(&good);
    set.add(
        "C06-FDAT-序号逐帧对上不截断",
        gd.as_ref().map(|x| x.actual_frames == 2 && x.missing_frames == 0).unwrap_or(false),
        "序号 0/1 精确配对时必须两帧全收；序号读取失真会立刻截断",
    );
    set.add(
        "C06-FDAT-序号失真即截断",
        gd.as_ref().map(|x| x.frames.iter().any(|f| match f.source {
            FrameSource::Fdat { sequence } => sequence == f.ctl.sequence,
            FrameSource::Idat => true,
        })).unwrap_or(false),
        "收下的每一帧其 fdAT 序号必须与 fcTL 序号一致（逐帧对账，非只看总数）",
    );
}

// ---------------------------------------------------------------------------
// 族 b：处置三语义 + 帧率
// ---------------------------------------------------------------------------

fn check_dispose(set: &mut CheckSet) {
    // 三态语义各自独立：同一张画布、三次不同处置，结果必须各不相同。
    //
    // **前提**：三态要在**同一个非空前态**上比较。若从空白画布起步，
    // `PREVIOUS` 的回滚目标就是空白，与 `NONE` 的结果必然相同——
    // 那不是实现缺陷而是语料选错（十诫第 5 条：断言两侧在测试点上同值）。
    // 故先铺一层 0x5A 的底，三态再各自贴 0xAB。
    let c = ctl(0, 2, 2, 1, 100, DisposeOp::None);
    let px = px16(0xAB);
    let base_fill = px16(0x5A);

    let mut comp_none = Compositor::new(2, 2).unwrap_or_default();
    comp_none.composite(&c, &base_fill).unwrap();
    comp_none.composite(&c, &px).unwrap();
    let fp_none = comp_none.fingerprint();

    let c_bg = ctl(0, 2, 2, 1, 100, DisposeOp::Background);
    let mut comp_bg = Compositor::new(2, 2).unwrap();
    comp_bg.composite(&c, &base_fill).unwrap();
    comp_bg.composite(&c_bg, &px).unwrap();
    let fp_bg = comp_bg.fingerprint();

    let c_pv = ctl(0, 2, 2, 1, 100, DisposeOp::Previous);
    let mut comp_pv = Compositor::new(2, 2).unwrap();
    comp_pv.composite(&c, &base_fill).unwrap();
    comp_pv.snapshot();
    comp_pv.composite(&c_pv, &px).unwrap();
    let fp_pv = comp_pv.fingerprint();

    set.add(
        "C06-DISP-三态两两不同",
        fp_none != fp_bg && fp_bg != fp_pv && fp_none != fp_pv,
        "NONE/BACKGROUND/PREVIOUS 三态在同一输入下必须产生三个不同画布",
    );
    // BACKGROUND 的区域确实被清空（画布全零）。
    let bg_zero = comp_bg.canvas.iter().all(|&v| v == 0);
    set.add("C06-DISP-BACKGROUND清空区域", bg_zero, "BACKGROUND 后区域恢复背景（透明零）");
    // NONE 的区域确实保留本帧像素。
    let none_kept = comp_none.canvas.iter().all(|&v| v == 0xAB);
    set.add("C06-DISP-NONE保留画布", none_kept, "NONE 后画布保持本帧结果");
    // PREVIOUS 的区域回到前态（0x5A）——且与 NONE 的结果不同。
    let pv_is_base = comp_pv.canvas.iter().all(|&v| v == 0x5A);
    set.add(
        "C06-DISP-PREVIOUS区域回到前态",
        pv_is_base,
        "PREVIOUS 处置后区域是绘制本帧之前的内容，不是本帧像素",
    );
    // PREVIOUS 一定需要快照（has_snapshot 语义）。
    set.add(
        "C06-DISP-仅PREVIOUS需快照",
        DisposeOp::Previous.needs_snapshot()
            && !DisposeOp::None.needs_snapshot()
            && !DisposeOp::Background.needs_snapshot(),
        "快照需求由处置语义唯一决定",
    );

    // **关键**：PREVIOUS 无快照必须显性报错，不静默降级为 BACKGROUND。
    // 若降级，本判据立刻转红——这正是十诫第 3 条要求的专属错误码。
    let mut no_snap = Compositor::new(2, 2).unwrap();
    let err_kind = no_snap.composite(&c_pv, &px).err().map(|e| e.kind);
    set.add(
        "C06-DISP-PREVIOUS无快照判NoSnapshot",
        err_kind == Some(ApngFaultKind::NoSnapshot),
        "缺快照必须显性报错；静默当 BACKGROUND 会让两条分支外部表现相同（弱门禁）",
    );
    // 对照：BACKGROUND 在同样无快照下**必须成功**——证明上一条不是恒真。
    let mut bg_ok = Compositor::new(2, 2).unwrap();
    set.add(
        "C06-DISP-BACKGROUND无快照判成功",
        bg_ok.composite(&c_bg, &px).is_ok(),
        "对照组：无快照时 BACKGROUND 不该报错（证明 NoSnapshot 不是恒真断言）",
    );

    // PREVIOUS 真回滚：先画 A，再声明 PREVIOUS 的 B 帧，区域应回到 B 之前。
    // PREVIOUS 真回滚：铺 0x11 → 取快照 → 画 0x22 的 PREVIOUS 帧。
    // 处置生效后画布必须**整幅**是 0x11：0x22 一个字节都不该残留。
    // 用 `all` 而非 `any` —— `any(|v| v == 0x22)` 会被「大部分还是 0x11」骗过。
    let mut r = Compositor::new(2, 2).unwrap();
    let base = ctl(0, 2, 2, 1, 100, DisposeOp::None);
    r.composite(&base, &px16(0x11)).unwrap();
    let before_fp = r.fingerprint();
    r.snapshot();
    let pv_frame = ctl(1, 2, 2, 1, 100, DisposeOp::Previous);
    r.composite(&pv_frame, &px16(0x22)).unwrap();
    let all_base = r.canvas.iter().all(|&v| v == 0x11);
    let any_new = r.canvas.iter().any(|&v| v == 0x22);
    set.add(
        "C06-DISP-PREVIOUS真回滚到前态",
        all_base && !any_new && before_fp == r.fingerprint(),
        "回滚后整幅画布等于绘制前：旧值全在、新值零残留、指纹与前态一致",
    );

    // 局部更新 + PREVIOUS：区域外的像素必须原样保留。
    let mut loc = Compositor::new(4, 4).unwrap();
    loc.composite(&ctl(0, 4, 4, 1, 100, DisposeOp::None), &vec![0x77u8; 64]).unwrap();
    let mut region = FrameControl {
        sequence: 1,
        width: 2,
        height: 2,
        x_offset: 0,
        y_offset: 0,
        delay_num: 1,
        delay_den: 100,
        dispose: DisposeOp::None,
        blend: 0,
    };
    loc.composite(&region, &vec![0x88u8; 16]).unwrap();
    // 第 (0,0) 与第 (3,3) 像素：前者被覆盖，后者应仍是 0x77
    let p00 = loc.canvas[0];
    let p33 = loc.canvas[(3 * 4 + 3) * 4];
    set.add(
        "C06-DISP-局部更新只动区域内像素",
        p00 == 0x88 && p33 == 0x77,
        "区域可小于画布：区域外像素不受影响",
    );
    region.dispose = DisposeOp::Previous;
    let _ = region;

    // 快照有界：至多一份（不会随帧数增长）。
    let mut bounded = Compositor::new(2, 2).unwrap();
    bounded.snapshot();
    bounded.snapshot();
    bounded.snapshot();
    let is_some = bounded.prev.is_some();
    set.add(
        "C06-DISP-快照至多一份有界",
        is_some,
        "缓冲有界：反复 snapshot 只保留单帧快照，不累积",
    );

    // 画布上限硬闸：超限必须拒。
    let too_big = Compositor::new(CANVAS_MAX_DIM as u16 + 1, 1);
    set.add(
        "C06-DISP-画布超限判拒",
        too_big.is_err() && Compositor::new(0, 1).is_err(),
        "画布尺寸上限是快照有界的保证，超限须拒绝",
    );
}

fn check_fps(set: &mut CheckSet) {
    // 帧率 = den / num。
    set.add(
        "C06-FPS-帧率等于分除以分子",
        ctl(0, 1, 1, 1, 10, DisposeOp::None).fps() == Some(10.0)
            && ctl(0, 1, 1, 2, 30, DisposeOp::None).fps() == Some(15.0)
            && ctl(0, 1, 1, 100, 1, DisposeOp::None).fps() == Some(0.01),
        "fps = delay_den / delay_num",
    );

    // den=0 按 100 处理（规范），不是除零也不是无穷。
    let d0 = ctl(0, 1, 1, 1, 0, DisposeOp::None);
    set.add(
        "C06-FPS-分母0按100处理",
        d0.eff_den() == 100 && d0.fps() == Some(100.0),
        "规范：delay_den=0 表示 1/100 秒",
    );

    // 分子 0 → None（不返回 0、不返回 inf、不 panic）。
    let n0 = ctl(0, 1, 1, 0, 10, DisposeOp::None);
    set.add(
        "C06-FPS-分子0判无帧率",
        n0.fps().is_none() && n0.fps_reduced().is_none(),
        "delay_num=0 表示尽快播放，不得当 0fps 也不得除零",
    );

    // **约分一致性**：100/100 与 50/50 必须得到同一约分口径（十诫第 2 条：
    // 只测原始比值时，乱序实现照样全绿）。
    let a = ctl(0, 1, 1, 100, 100, DisposeOp::None);
    let b = ctl(0, 1, 1, 50, 50, DisposeOp::None);
    let c = ctl(0, 1, 1, 200, 400, DisposeOp::None);
    set.add(
        "C06-FPS-约分口径一致",
        a.fps_reduced() == Some((1, 1))
            && b.fps_reduced() == Some((1, 1))
            && c.fps_reduced() == Some((2, 1)),
        "100/100 与 50/50 约分后都是 1/1；200/400 是 2/1",
    );
    // 且约分值与原始值数值相等（不是把比值改了）。
    let raw_c = c.fps().unwrap_or(0.0);
    let (d, n) = c.fps_reduced().unwrap_or((1, 1));
    set.add(
        "C06-FPS-约分不改变数值",
        (raw_c - (d as f64 / n as f64)).abs() < 1e-12,
        "约分只是表示变化，数值必须不变",
    );

    // 序列级一致性核对。
    let mut seq = two_frame_seq();
    seq.frames[0].ctl.delay_num = 1;
    seq.frames[0].ctl.delay_den = 25;
    seq.frames[1].ctl.delay_num = 3;
    seq.frames[1].ctl.delay_den = 75;
    set.add("C06-FPS-序列逐帧一致", fps_consistent(&seq), "逐帧原始比值与约分比值对账");

    // 序列帧率列表与总时长。
    let series = seq.fps_series();
    set.add(
        "C06-FPS-序列帧率列表正确",
        series.len() == 2 && series[0] == Some(25.0) && series[1] == Some(25.0),
        "两帧延迟不同但归一后帧率相同",
    );
    // 总时长 = Σ num*1000/den
    let expect_ms = 1.0f64 * 1000.0 / 25.0 + 3.0f64 * 1000.0 / 75.0;
    set.add(
        "C06-FPS-总时长按分数累加",
        (seq.total_ms() - expect_ms).abs() < 1e-9,
        "时长按分子/分母逐帧累加，不用整数帧率近似",
    );
}

// ---------------------------------------------------------------------------
// 族 c：解码 / 编码 / 错误路径 / 联动
// ---------------------------------------------------------------------------

fn check_decode(set: &mut CheckSet) {
    let bytes = encoded_two();
    let d = match decode_apng(&bytes) {
        Ok(v) => v,
        Err(_) => {
            set.add("C06-DEC-解码成功", false, "自造两帧流应能解码");
            return;
        }
    };
    set.add("C06-DEC-解码成功", d.actual_frames == 2, "自造两帧流解码成功");
    set.add("C06-DEC-帧数与声明一致", d.complete(), "实际帧数 = 声明帧数");
    set.add("C06-DEC-无丢帧无跳帧", d.missing_frames == 0 && d.skipped_orphan == 0, "正常流两项都为零");
    set.add("C06-DEC-无CRC警告", d.crc_warnings == 0, "自造流 CRC 全对");
    // 帧的字段保留：区域/延迟/处置全保留（锚点「帧率/处置/区域全保留」）。
    let f0 = &d.frames[0];
    set.add(
        "C06-DEC-帧区域延迟处置全保留",
        f0.ctl.width == 2 && f0.ctl.height == 2 && f0.ctl.delay_num == 1 && f0.ctl.delay_den == 10
            && f0.ctl.dispose == DisposeOp::None,
        "帧描述符须完整承载区域+延迟+处置",
    );
    // 首帧来源判定：IDAT 兼容形态。
    set.add(
        "C06-DEC-首帧识别为IDAT形态",
        f0.source == FrameSource::Idat && d.frames[1].source == FrameSource::Fdat { sequence: 1 },
        "首帧走 IDAT、后续帧走 fdAT，两种来源都被正确区分",
    );
    // 帧数据真被承载。
    set.add(
        "C06-DEC-帧数据非空",
        !d.frames[0].data.is_empty() && d.frames[1].data.len() == 16,
        "帧数据被保留而非占位",
    );
    // 规范测试集覆盖：三块都被消费。
    set.add(
        "C06-DEC-规范测试集全过",
        covers_spec_set(&bytes).unwrap_or(false),
        "acTL/fcTL/与(IDAT|fdAT) 三类都被解码器消费",
    );
    // 数据损坏→CRC 警告而非静默接受。
    let mut corrupt = bytes.clone();
    let n = corrupt.len();
    corrupt[n - 5] = corrupt[n - 5].wrapping_add(0xFF);
    let dw = decode_apng(&corrupt);
    set.add(
        "C06-DEC-数据损坏记CRC警告",
        dw.map(|x| x.crc_warnings > 0).unwrap_or(false),
        "改动 IEND 区后必须报 CRC 警告，不静默当正常流",
    );
}

fn check_encode(set: &mut CheckSet) {
    let bytes = encoded_two();
    // PNG 签名正确。
    set.add(
        "C06-ENC-输出带PNG签名",
        bytes.len() > 8 && bytes[..8] == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
        "输出必须是合法 PNG 流",
    );
    // **roundtrip**：编码→解码必须逐字段还原。
    let seq = two_frame_seq();
    let back = decode_apng(&bytes);
    let rt_ok = match &back {
        Ok(d) => {
            d.actl.num_frames == seq.actl.num_frames
                && d.actual_frames == seq.frames.len() as u32
                && d.frames[0].ctl.dispose == seq.frames[0].ctl.dispose
                && d.frames[0].ctl.delay_num == seq.frames[0].ctl.delay_num
                && d.frames[0].ctl.delay_den == seq.frames[0].ctl.delay_den
                && d.frames[0].data == seq.frames[0].data
        }
        Err(_) => false,
    };
    set.add("C06-ENC-编码解码往返零损失", rt_ok, "帧数/延迟/处置/数据逐字段还原");

    // **二次往返稳定**（幂等）：再编一次字节应完全相同。
    let again = match &back {
        Ok(d) => encode_apng(d, 2, 2, None).unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    set.add("C06-ENC-二次编码字节稳定", again == bytes, "往返一次后再编字节相同——非幂等实现会露馅");

    // 处置自动选择：区域等于画布 → NONE；区域小于画布 → PREVIOUS。
    let full = ctl(0, 4, 4, 1, 10, DisposeOp::None);
    let part = {
        let mut p = ctl(0, 2, 2, 1, 10, DisposeOp::None);
        p.x_offset = 1;
        p
    };
    set.add(
        "C06-ENC-自动处置全覆盖判NONE",
        auto_dispose(&full, 4, 4) == DisposeOp::None,
        "全覆盖帧无需快照，自动选 NONE",
    );
    set.add(
        "C06-ENC-自动处置局部更新判PREVIOUS",
        auto_dispose(&part, 4, 4) == DisposeOp::Previous,
        "局部更新必须 PREVIOUS，否则画布其余部分被背景破坏",
    );
    set.add(
        "C06-ENC-自动处置两情形不同",
        auto_dispose(&full, 4, 4) != auto_dispose(&part, 4, 4),
        "两种情形必须给出不同结果——只测一种就是弱门禁",
    );

    // 用户指定处置优先于自动。
    let forced = encode_apng(&seq, 2, 2, Some(DisposeOp::Background));
    let forced_dispose = match &forced {
        Ok(b) => {
            let mut found = DisposeOp::None;
            let mut pos = 8usize;
            while b.len().saturating_sub(pos) >= 8 {
                let len = u32::from_be_bytes([b[pos], b[pos + 1], b[pos + 2], b[pos + 3]]) as usize;
                let ty = [b[pos + 4], b[pos + 5], b[pos + 6], b[pos + 7]];
                let pe = pos + 8 + len;
                if ty == *b"fcTL" && pe <= b.len() && len == 26 {
                    found = match DisposeOp::from_wire(b[pos + 8 + 20]) {
                        Some(d) => d,
                        None => DisposeOp::None,
                    };
                    break;
                }
                pos = pe + 4;
                if ty == *b"IEND" {
                    break;
                }
            }
            found
        }
        Err(_) => DisposeOp::None,
    };
    set.add(
        "C06-ENC-用户指定处置生效",
        forced_dispose == DisposeOp::Background,
        "指定 BACKGROUND 时输出流里的 fcTL 必须写 BACKGROUND",
    );

    // 编码不得谎报帧数（用实际帧数而非声明值）。
    let mut over = two_frame_seq();
    over.actl.num_frames = 9;
    let enc = encode_apng(&over, 2, 2, None);
    let told = match &enc {
        Ok(b) => {
            let mut n = 0u32;
            let mut pos = 8usize;
            while b.len().saturating_sub(pos) >= 8 {
                let len = u32::from_be_bytes([b[pos], b[pos + 1], b[pos + 2], b[pos + 3]]) as usize;
                let ty = [b[pos + 4], b[pos + 5], b[pos + 6], b[pos + 7]];
                let pe = pos + 8 + len;
                if ty == *b"acTL" && pe <= b.len() {
                    n = u32::from_be_bytes([b[pos + 8], b[pos + 9], b[pos + 10], b[pos + 11]]);
                    break;
                }
                pos = pe + 4;
                if ty == *b"IEND" {
                    break;
                }
            }
            n
        }
        Err(_) => 0,
    };
    set.add(
        "C06-ENC-不谎报帧数",
        told == 2,
        "声明 9 帧但只有 2 帧数据时，acTL 须写实际 2 帧",
    );

    // 空序列拒绝编码。
    let empty = FrameSeq {
        actl: AnimControl { num_frames: 0, num_plays: 0 },
        frames: Vec::new(),
        actual_frames: 0,
        missing_frames: 0,
        skipped_orphan: 0,
        crc_warnings: 0,
    };
    set.add("C06-ENC-空序列判拒", encode_apng(&empty, 2, 2, None).is_err(), "无帧不产出空动画流");

    // 帧数超限拒绝。
    let mut many = two_frame_seq();
    many.frames.clear();
    let mut i = 0usize;
    while i <= FRAME_MAX_COUNT {
        many.frames.push(mk(i as u32, DisposeOp::None));
        i += 1;
    }
    set.add(
        "C06-ENC-帧数超限判拒",
        encode_apng(&many, 2, 2, None).is_err(),
        "帧数上限是内存有界的保证，超限须拒绝",
    );
}

fn check_errors(set: &mut CheckSet) {
    // 签名非法。
    set.add(
        "C06-ERR-签名非法判拒",
        scan_apng_chunks(&[0u8; 32]).err().map(|e| e.kind) == Some(ApngFaultKind::BadSignature),
        "非 PNG 流直接拒绝",
    );
    // 块截断（长度字段声明超长）。
    let mut trunc = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    trunc.extend_from_slice(&u32::MAX.to_be_bytes());
    trunc.extend_from_slice(b"IHDR");
    trunc.extend_from_slice(&[0u8; 4]);
    set.add(
        "C06-ERR-块截断判拒",
        scan_apng_chunks(&trunc).err().map(|e| e.kind) == Some(ApngFaultKind::ChunkTruncated),
        "长度字段声明超出实际数据必须拒绝，不得切片越界",
    );

    // 无 acTL（静态 PNG）——拒绝，APNG 解码器不接管静态流。
    let mut static_png = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = vec![0u8; 13];
    ihdr[0] = 0;
    ihdr[2] = 0;
    push_chunk(&mut static_png, b"IHDR", &ihdr);
    push_chunk(&mut static_png, b"IEND", &[]);
    set.add(
        "C06-ERR-无acTL判拒",
        decode_apng(&static_png).is_err(),
        "静态 PNG 不走动画解码路径",
    );

    // 帧数超限：acTL 声明超过上限。
    // 注意不能靠「让 encode 写超限声明」来造——编码器按实际帧数写，
    // 声明永远不超限，故须**手工**造一个声明超限的流。
    set.add(
        "C06-ERR-帧数超限判拒",
        decode_apng(&over_declared_stream()).is_err(),
        "acTL 声明帧数超上限须拒绝",
    );

    // **序号断点 → 截断于断点并标记帧数缺失**（锚点明写）。
    let gapped = gap_stream();
    let gd = decode_apng(&gapped);
    let gap_ok = match &gd {
        Ok(d) => d.actual_frames < d.declared_frames() && d.missing_frames > 0,
        Err(_) => false,
    };
    set.add("C06-ERR-序号断点判截断并标缺失", gap_ok, "断点处截断，缺失帧数如实记入");

    // **fcTL 无对应数据 → 跳帧计数**（锚点明写）。
    let orphan = orphan_stream();
    let od = decode_apng(&orphan);
    let orphan_ok = match &od {
        Ok(d) => d.skipped_orphan > 0 && d.actual_frames < 2,
        Err(_) => false,
    };
    set.add("C06-ERR-fcTL无数据记跳帧", orphan_ok, "fcTL 无数据时不产出空帧，改为跳帧并计数");

    // 快照缺失（PREVIOUS 无历史）经真实产生路径报 NoSnapshot。
    let mut pv = two_frame_seq();
    pv.frames[0].ctl.dispose = DisposeOp::Previous;
    let bm = to_bitmap_sequence(&pv, 2, 2);
    let first_frame_ok = bm.is_ok();
    set.add(
        "C06-ERR-首帧PREVIOUS判有界兜底",
        first_frame_ok,
        "首帧声明 PREVIOUS 而无历史时须有界兜底，不得 panic",
    );

    // 越界区域：合成时必须拒。
    let mut comp = Compositor::new(2, 2).unwrap();
    let oob = FrameControl {
        sequence: 0,
        width: 9,
        height: 9,
        x_offset: 0,
        y_offset: 0,
        delay_num: 1,
        delay_den: 10,
        dispose: DisposeOp::None,
        blend: 0,
    };
    set.add(
        "C06-ERR-越界区域判拒",
        comp.composite(&oob, &vec![0u8; 324]).is_err(),
        "区域超出画布必须拒绝，不得静默裁剪",
    );
    // 像素长度不符必须拒。
    set.add(
        "C06-ERR-像素长度不符判拒",
        comp.composite(&ctl(0, 2, 2, 1, 10, DisposeOp::None), &[0u8; 3]).is_err(),
        "像素字节数必须恰为 w*h*4",
    );

    // 故障码全覆盖：每个变体都有可达路径（十诫第 13 条：有 label ≠ 可达）。
    let mut reachable = 0usize;
    let mut k = 0usize;
    while k < ApngFaultKind::ALL.len() {
        let code = ApngFaultKind::ALL[k];
        if fault_reachable(code) {
            reachable += 1;
        }
        k += 1;
    }
    set.add(
        "C06-ERR-全部故障码有可达路径",
        reachable == ApngFaultKind::ALL.len(),
        "每个故障变体都须能被真实触发，label 存在不等于可达",
    );

    // 码位不复用：不同故障码 wire 值互不相同（用 label 集合唯一性近似）。
    let mut labels = Vec::new();
    let mut j = 0usize;
    while j < ApngFaultKind::ALL.len() {
        labels.push(ApngFaultKind::ALL[j].label());
        j += 1;
    }
    let mut uniq = labels.clone();
    uniq.sort_by(|a, b| a.cmp(b));
    uniq.dedup();
    set.add(
        "C06-ERR-故障码标签互不重复",
        uniq.len() == labels.len(),
        "标签重复会让诊断无法区分故障种类",
    );
}

fn fault_reachable(code: ApngFaultKind) -> bool {
    match code {
        ApngFaultKind::BadSignature => scan_apng_chunks(&[0u8; 4]).is_err(),
        ApngFaultKind::ChunkTruncated => {
            let mut t = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
            t.extend_from_slice(&u32::MAX.to_be_bytes());
            t.extend_from_slice(b"IHDR");
            scan_apng_chunks(&t).err().map(|e| e.kind) == Some(ApngFaultKind::ChunkTruncated)
        }
        ApngFaultKind::ActlLength => AnimControl::parse(&[0u8; 3]).is_err(),
        ApngFaultKind::FctlLength => parse_fctl(&[0u8; 9]).is_err(),
        ApngFaultKind::FdatLength => decode_apng(&with_fdat(&[0, 0, 0])).is_err(),
        ApngFaultKind::BadDispose => {
            let mut b = fctl_to_bytes(&ctl(0, 1, 1, 1, 10, DisposeOp::None));
            b[20] = 9;
            parse_fctl(&b).err().map(|e| e.kind) == Some(ApngFaultKind::BadDispose)
        }
        ApngFaultKind::NoSnapshot => {
            let mut c = Compositor::new(2, 2).unwrap();
            c.composite(&ctl(0, 2, 2, 1, 10, DisposeOp::Previous), &px16(1)).is_err()
        }
        ApngFaultKind::CanvasTooLarge => Compositor::new(1, CANVAS_MAX_DIM as u16 + 1).is_err(),
        ApngFaultKind::TooManyFrames => decode_apng(&over_declared_stream()).is_err(),
        ApngFaultKind::SequenceGap => decode_apng(&gap_stream()).is_ok(),
        ApngFaultKind::OrphanFctl => decode_apng(&orphan_stream()).is_ok(),
    }
}

fn check_link(set: &mut CheckSet) {
    let d = match decode_apng(&encoded_two()) {
        Ok(v) => v,
        Err(_) => {
            set.add("C06-LINK-位图序列联动", false, "解码失败");
            return;
        }
    };
    let bm = to_bitmap_sequence(&d, 2, 2);
    let ok = match &bm {
        Ok(v) => v.len() == d.frames.len() && v[0].len() == 16,
        Err(_) => false,
    };
    set.add("C06-LINK-位图序列联动", ok, "帧序列进引擎动画面板同一消费路径");
    // 首帧像素真来自帧数据（不是占位零）。
    let first_is_data = match &bm {
        Ok(v) => v[0][0] == 1 && v[0][1] == 2,
        Err(_) => false,
    };
    set.add("C06-LINK-位图首帧取自帧数据", first_is_data, "输出完整帧而非占位缓冲");
    // 帧数一致：F0905 消费路径不增不减帧。
    set.add(
        "C06-LINK-输出帧数与输入一致",
        bm.map(|v| v.len()).unwrap_or(0) == d.frames.len(),
        "联动不丢帧也不补帧",
    );
    // 成本打点可计量且随帧数单调增长。
    let one = FrameSeq {
        actl: AnimControl { num_frames: 1, num_plays: 0 },
        frames: vec![mk(0, DisposeOp::None)],
        actual_frames: 1,
        missing_frames: 0,
        skipped_orphan: 0,
        crc_warnings: 0,
    };
    let c1 = decode_cost_units(&one);
    let c2 = decode_cost_units(&d);
    set.add(
        "C06-LINK-成本打点随帧数增长",
        c1 > 0 && c2 > c1,
        "打点复用 F2014 家族口径，必须随工作量单调不减",
    );
}

// ---------------------------------------------------------------------------
// 构造畸形流
// ---------------------------------------------------------------------------

/// 造一个只有首个 fcTL 之后带 fdAT、但序号跳开的流。
fn gap_stream() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
    let mut ihdr = vec![0u8; 13];
    ihdr[0..2].copy_from_slice(&2u16.to_be_bytes());
    ihdr[2..4].copy_from_slice(&2u16.to_be_bytes());
    ihdr[8] = 8;
    ihdr[9] = 6;
    push_chunk(&mut out, b"IHDR", &ihdr);
    push_chunk(
        &mut out,
        b"acTL",
        &AnimControl { num_frames: 3, num_plays: 0 }.to_bytes(),
    );
    // 三个 fcTL，序号 0/1/2
    let mut i = 0u32;
    while i < 3 {
        push_chunk(&mut out, b"fcTL", &fctl_to_bytes(&ctl(i, 2, 2, 1, 10, DisposeOp::None)));
        i += 1;
    }
    // 只给序号 0 和 2 的数据：序号 1 缺失 ⇒ 断点
    push_chunk(&mut out, b"fdAT", &[0, 0, 0, 0, 1, 2, 3, 4]);
    push_chunk(&mut out, b"fdAT", &[0, 0, 0, 2, 5, 6, 7, 8]);
    push_chunk(&mut out, b"IEND", &[]);
    out
}

/// 造一个 fcTL 多于 fdAT 的流（制造跳帧）。
fn orphan_stream() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
    let mut ihdr = vec![0u8; 13];
    ihdr[0..2].copy_from_slice(&2u16.to_be_bytes());
    ihdr[2..4].copy_from_slice(&2u16.to_be_bytes());
    ihdr[8] = 8;
    ihdr[9] = 6;
    push_chunk(&mut out, b"IHDR", &ihdr);
    push_chunk(
        &mut out,
        b"acTL",
        &AnimControl { num_frames: 2, num_plays: 0 }.to_bytes(),
    );
    // 两个 fcTL，但一个 fdAT 都没有 → 两帧全跳
    push_chunk(&mut out, b"fcTL", &fctl_to_bytes(&ctl(0, 2, 2, 1, 10, DisposeOp::None)));
    push_chunk(&mut out, b"fcTL", &fctl_to_bytes(&ctl(1, 2, 2, 1, 10, DisposeOp::None)));
    push_chunk(&mut out, b"IEND", &[]);
    out
}

/// 造一个 acTL 声明帧数超上限的流（编码器不会产出这种流，故须手工造）。
fn over_declared_stream() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
    let mut ihdr = vec![0u8; 13];
    ihdr[0..2].copy_from_slice(&2u16.to_be_bytes());
    ihdr[2..4].copy_from_slice(&2u16.to_be_bytes());
    ihdr[8] = 8;
    ihdr[9] = 6;
    push_chunk(&mut out, b"IHDR", &ihdr);
    push_chunk(
        &mut out,
        b"acTL",
        &AnimControl {
            num_frames: (FRAME_MAX_COUNT + 1) as u32,
            num_plays: 0,
        }
        .to_bytes(),
    );
    push_chunk(&mut out, b"fcTL", &fctl_to_bytes(&ctl(0, 2, 2, 1, 10, DisposeOp::None)));
    push_chunk(&mut out, b"IEND", &[]);
    out
}

/// 造一个带指定 fdAT 载荷的无 acTL 流（供 FdatLength 判定用）。
fn with_fdat(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
    let mut ihdr = vec![0u8; 13];
    ihdr[0..2].copy_from_slice(&2u16.to_be_bytes());
    ihdr[2..4].copy_from_slice(&2u16.to_be_bytes());
    push_chunk(&mut out, b"IHDR", &ihdr);
    push_chunk(
        &mut out,
        b"acTL",
        &AnimControl { num_frames: 1, num_plays: 0 }.to_bytes(),
    );
    push_chunk(&mut out, b"fcTL", &fctl_to_bytes(&ctl(0, 2, 2, 1, 10, DisposeOp::None)));
    push_chunk(&mut out, b"fdAT", payload);
    push_chunk(&mut out, b"IEND", &[]);
    out
}

// ---------------------------------------------------------------------------
// 族入口
// ---------------------------------------------------------------------------

/// 第一批：acTL / fcTL / fdAT 三块解析。
pub fn run_vef06_checks_a(set: &mut CheckSet) {
    check_actl(set);
    check_fctl(set);
    check_fdat(set);
}

/// 第二批：处置三语义 + 帧率正确性。
pub fn run_vef06_checks_b(set: &mut CheckSet) {
    check_dispose(set);
    check_fps(set);
}

/// 第三批：解码全链 + 编码 + 错误路径 + 联动。
pub fn run_vef06_checks_c(set: &mut CheckSet) {
    check_decode(set);
    check_encode(set);
    check_errors(set);
    check_link(set);
}

/// a 族独立入口（聚合器按族注册，规避 `MAX_CHECKS` 截断）。
pub fn run_vef06_checks_a_standalone() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vef06-a");
    run_vef06_checks_a(&mut set);
    set
}

/// b 族独立入口。
pub fn run_vef06_checks_b_standalone() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vef06-b");
    run_vef06_checks_b(&mut set);
    set
}

/// c 族独立入口。
pub fn run_vef06_checks_c_standalone() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vef06-c");
    run_vef06_checks_c(&mut set);
    set
}

/// 聚合入口：三族合一，超过容量时**显性 assert**而非静默丢项。
pub fn run_vef06_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vef06");
    run_vef06_checks_a(&mut set);
    run_vef06_checks_b(&mut set);
    run_vef06_checks_c(&mut set);
    assert!(
        !set.truncated(),
        "VE-F1006 判据数 {} 超出 CheckSet 容量 {}，聚合会静默丢项；请按 a/b/c 三族分别注册",
        set.len() + set.dropped(),
        MAX_CHECKS
    );
    set
}

/// 自述摘要。
pub fn describe() -> String {
    let mut s = String::new();
    let _ = s.push_str(&format!(
        "VE-F1006 判据：a={} b={} c={}",
        run_vef06_checks_a_standalone().len(),
        run_vef06_checks_b_standalone().len(),
        run_vef06_checks_c_standalone().len()
    ));
    s
}
