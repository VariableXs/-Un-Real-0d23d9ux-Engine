
// ---------------------------------------------------------------------------
// F006 · 深化批次三：跨设备颜色深度转换（不静默降色深）+ 屏上 DC 直绘省拷贝
// 记账
//
// 主册依据（G-A-06【状态与异常】）：「BitBlt 跨设备格式转换按源格式如实转换
// （不静默降色深）」；【设计细节】「DC 概念映射为『合成器上下文句柄』，屏上
// DC 直接绘即所见（省一次拷贝）」。GdiState/DeviceContext/ROP_TABLE 既有面
// （一处一事实），本段只补转换语义与省拷贝观测。
// ---------------------------------------------------------------------------

/// 颜色深度转换语义：目标深于源（升档）恒可；目标浅于源（降档）必须显式
/// 请求——否则如实拒绝（不静默降色深，调用方决定 UI）。
pub fn convert_color_depth(src_bpp: u32, dst_bpp: u32, explicit_downgrade: bool) -> Result<u32, &'static str> {
    if dst_bpp < src_bpp && !explicit_downgrade {
        return Err("color depth downgrade refused; source depth preserved");
    }
    if dst_bpp == 0 || src_bpp == 0 {
        return Err("invalid color depth");
    }
    Ok(dst_bpp)
}

/// 屏上 DC 直绘省拷贝记账（主册【设计细节】：屏上 DC 直接绘即所见——相对
/// 内存 DC 的一次合成器拷贝被省去，记账面可观测）。
#[derive(Clone, Copy, Debug)]
pub struct DirectDrawLedger {
    /// 屏上 DC 绘制次数。
    pub screen_dc_draws: u32,
    /// 省去的拷贝次数（屏上直绘每笔省一次——两计数恒等，恒等式即判据）。
    pub copies_saved: u32,
}

impl DirectDrawLedger {
    pub const fn new() -> DirectDrawLedger {
        DirectDrawLedger { screen_dc_draws: 0, copies_saved: 0 }
    }

    pub fn note_screen_draw(&mut self) {
        self.screen_dc_draws += 1;
        self.copies_saved += 1;
    }
}

/// F006 深化批次三自检。
pub fn run_gdiface_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F006-gdiface-deep2");
    // 1) 降档必须显式：32→16 无请求 = 拒绝；显式请求 = 放行；16→32 升档恒可；
    //    零深度 = 非法。
    cs.add(
        "color_depth_no_silent_downgrade",
        convert_color_depth(32, 16, false).is_err()
            && convert_color_depth(32, 16, true) == Ok(16)
            && convert_color_depth(16, 32, false) == Ok(32)
            && convert_color_depth(0, 16, false).is_err(),
        "",
    );
    // 2) 省拷贝恒等式：N 笔屏上直绘 → 省去 N 次拷贝（两计数逐笔恒等）。
    let mut ledger = DirectDrawLedger::new();
    for _ in 0..7 {
        ledger.note_screen_draw();
    }
    cs.add(
        "direct_draw_copies_saved_identity",
        ledger.screen_dc_draws == 7 && ledger.copies_saved == 7,
        "",
    );
    // 3) 高频 ROP 表对账不变（16 码既有面锚——深化不破坏既有判据）。
    cs.add(
        "rop_table_size_anchor",
        ROP_TABLE.len() == HIGH_FREQ_ROP_COUNT,
        "",
    );
    cs
}
