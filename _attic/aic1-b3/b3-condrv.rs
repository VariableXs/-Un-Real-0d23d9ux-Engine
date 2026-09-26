
// ---------------------------------------------------------------------------
// F012 · 深化批次三：SGR 256 色索引展开面（xterm 全规则色值）+ CTRL_CLOSE_EVENT
// 剩余秒显示
//
// 主册依据（G-A-12【设计细节】）：「VT 序列支持含 256 色（SGR 38/48）与真彩」
// ——参数面解析（38;5;N 消费）由既有 VtParser 承载，本段给出索引 → RGB 展开
// （渲染面消费）；「CTRL_CLOSE_EVENT 宽限 5 秒后强杀（倒计时显示）」——既有
// request_close/tick 承载宽限状态机，本段补倒计时显示值（用户看见的剩余秒）。
// ---------------------------------------------------------------------------

/// xterm 256 色索引 → RGB（标准规则：0-15 基准色表；16-231 6×6×6 立方
/// （阶梯 0/95/135/175/215/255）；232-255 灰阶 8 步进起 8）。
pub fn palette256_rgb(index: u8) -> [u8; 3] {
    const BASE: [[u8; 3]; 16] = [
        [0, 0, 0],
        [128, 0, 0],
        [0, 128, 0],
        [128, 128, 0],
        [0, 0, 128],
        [128, 0, 128],
        [0, 128, 128],
        [192, 192, 192],
        [128, 128, 128],
        [255, 0, 0],
        [0, 255, 0],
        [255, 255, 0],
        [0, 0, 255],
        [255, 0, 255],
        [0, 255, 255],
        [255, 255, 255],
    ];
    let i = index as usize;
    if i < 16 {
        return BASE[i];
    }
    if i < 232 {
        const STEP: [u8; 6] = [0, 95, 135, 175, 215, 255];
        let n = i - 16;
        [STEP[n / 36], STEP[(n % 36) / 6], STEP[n % 6]]
    } else {
        let v = (index - 232) * 10 + 8;
        [v, v, v]
    }
}

/// 倒计时显示值（用户看见的剩余整秒——向上取整：请求瞬间显示 5，走完 5s 归 0
/// 后由既有 tick 强杀）。
pub fn grace_remaining_secs(close_requested_ms: u64, now_ms: u64, total_ms: u64) -> u32 {
    let elapsed = now_ms.saturating_sub(close_requested_ms);
    if elapsed >= total_ms {
        return 0;
    }
    let remain_ms = total_ms - elapsed;
    ((remain_ms + 999) / 1000) as u32
}

/// F012 深化批次三自检。
pub fn run_condrv_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F012-condrv-deep2");
    // 1) 256 色锚点：基准 1 = 暗红；索引 196 = 纯红（5,0,0 阶梯）；231 = 立方
    //    白（5,5,5）；232 = 8 号灰；255 = 238 号灰。
    cs.add(
        "palette256_anchors",
        palette256_rgb(1) == [128, 0, 0]
            && palette256_rgb(196) == [255, 0, 0]
            && palette256_rgb(231) == [255, 255, 255]
            && palette256_rgb(232) == [8, 8, 8]
            && palette256_rgb(255) == [238, 238, 238],
        "",
    );
    // 2) 立方全覆盖：16..231 逐索引都在 6 阶梯值域内（0/95/135/175/215/255）。
    let mut cube_ok = true;
    for i in 16u32..232 {
        let [r, g, b] = palette256_rgb(i as u8);
        for v in [r, g, b] {
            cube_ok &= matches!(v, 0 | 95 | 135 | 175 | 215 | 255);
        }
    }
    cs.add("palette256_cube_lattice", cube_ok, "");
    // 3) 倒计时：请求瞬间 5s；4999ms 时剩 1s；5000ms 归 0（强杀由既有 tick 承接）。
    cs.add(
        "grace_countdown_display",
        grace_remaining_secs(10_000, 10_000, 5_000) == 5
            && grace_remaining_secs(10_000, 14_999, 5_000) == 1
            && grace_remaining_secs(10_000, 15_000, 5_000) == 0,
        "",
    );
    cs
}
