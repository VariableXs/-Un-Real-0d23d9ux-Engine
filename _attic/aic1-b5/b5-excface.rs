
// ---------------------------------------------------------------------------
// F020 · 深化批次五：崩溃卡片一行摘要（三要素的用户面出口）
//
// 主册依据（G-A-20【交互设计】）：「VARIX 弹『程序已停止工作』卡片（可重启/
// 可关闭/可查看详情）」——卡片首行 = 三要素紧凑摘要（由 MiniDump 渲染：
// 异常类型人话 + 崩溃模块线索 + 下一步短语）。
// ---------------------------------------------------------------------------

/// 异常码 → 人话归因（六类注入样本同源——批次一判据的六类）。
pub fn exception_kind_human(code: u32) -> &'static str {
    match code {
        0xC000_0094 => "除零异常",
        0xC000_0005 => "非法内存访问",
        0xC000_00FD => "栈溢出",
        0xE06D_7363 => "未处理 C++ 异常",
        0xC000_0602 => "SEH 吞噬类异常",
        0xC000_0409 => "VEH/栈完整性异常",
        _ => "未识别异常",
    }
}

/// 一行摘要渲染（`程序已停止工作：{归因}（崩溃地址 {hex}）— 可重启或查看详情`）。
/// 返回写入字节数（缓冲不足截断）。
pub fn dump_summary_line(d: &MiniDump, out: &mut [u8]) -> usize {
    let mut n = 0usize;
    crate::checks::push_str(out, &mut n, "程序已停止工作：");
    crate::checks::push_str(out, &mut n, exception_kind_human(d.exception_code));
    crate::checks::push_str(out, &mut n, "（崩溃地址 ");
    crate::checks::push_hex_u64(out, &mut n, d.crash_addr);
    crate::checks::push_str(out, &mut n, "）— 可重启或查看详情");
    n.min(out.len())
}

/// F020 深化批次五自检。
pub fn run_excface_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F020-excface-deep4");
    // 1) 六类异常码人话归因全非「未识别」（六类判据的摘要面延伸）。
    let mut all_human = true;
    for code in [
        0xC000_0094u32,
        0xC000_0005,
        0xC000_00FD,
        0xE06D_7363,
        0xC000_0602,
        0xC000_0409,
    ] {
        all_human &= exception_kind_human(code) != "未识别异常";
    }
    let mut d = MiniDump {
        exception_code: 0xC000_0005,
        crash_addr: 0x7FF6_1234_5678,
        thread_id: 1,
        stack: [0; DUMP_STACK_FRAMES],
        stack_n: 0,
        modules: [None; DUMP_MODULES],
        module_n: 0,
        system_fingerprint: 0,
        degraded: false,
        restart_cmd_hash: 0,
        restart_cwd_hash: 0,
    };
    let mut buf = [0u8; 256];
    let n = dump_summary_line(&d, &mut buf);
    let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
    cs.add(
        "dump_summary_three_parts",
        all_human
            && text.starts_with("程序已停止工作：非法内存访问")
            && text.contains("7ff612345678")
            && text.ends_with("可重启或查看详情"),
        "",
    );
    // 2) 未知异常码如实「未识别」（不冒充归因）+ 降级 dump 照常出摘要。
    d.exception_code = 0x1234_5678;
    d.degraded = true;
    let n2 = dump_summary_line(&d, &mut buf);
    let t2 = core::str::from_utf8(&buf[..n2]).unwrap_or("");
    cs.add(
        "dump_summary_unknown_and_degraded",
        t2.contains("未识别异常") && d.degraded,
        "",
    );
    // 3) 小缓冲截断（不冒充完整行）。
    let mut small = [0u8; 8];
    let n3 = dump_summary_line(&d, &mut small);
    cs.add("dump_summary_truncation_honest", n3 == 8, "");
    cs
}
