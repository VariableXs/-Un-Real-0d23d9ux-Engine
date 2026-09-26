
// ---------------------------------------------------------------------------
// F019 · 深化批次四：进程外 COM/DCOM 拒绝面（差异表 F132 公开）
//
// 主册依据（G-A-19【功能定义】）：「进程外 COM/DCOM 明确不承诺（差异表 F132
// 公开）」——不承诺 ≠ 静默失败：激活上下文带 LOCAL/REMOTE_SERVER 位时给
/// 结构化拒绝，短语直指差异表（三要素之「为什么+下一步」）。
// ---------------------------------------------------------------------------

/// 激活上下文位（winbase.h CLSCTX 钉值——本域消费的三个）。
pub const CLSCTX_INPROC_SERVER: u32 = 0x1;
pub const CLSCTX_LOCAL_SERVER: u32 = 0x4;
pub const CLSCTX_REMOTE_SERVER: u32 = 0x10;

/// 进程外拒绝短语（F132 差异表入口——非裸错误码）。
pub const OUT_OF_PROC_DIFF_SHEET: &str = "进程外 COM/DCOM 不在 VARIX 承诺面，详见差异表 F132";

/// 激活请求裁决：仅进程内（INPROC_SERVER/HANDLER 位族）放行；带 LOCAL/
/// REMOTE_SERVER 位 → 结构化拒绝（差异表短语），不假装成功。
pub fn activate_context(clsctx: u32) -> Result<(), &'static str> {
    if clsctx & CLSCTX_LOCAL_SERVER != 0 || clsctx & CLSCTX_REMOTE_SERVER != 0 {
        return Err(OUT_OF_PROC_DIFF_SHEET);
    }
    if clsctx & CLSCTX_INPROC_SERVER == 0 && clsctx == 0 {
        return Err("激活上下文未声明任何服务器类别");
    }
    Ok(())
}

/// F019 深化批次四自检。
pub fn run_comloc_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F019-comloc-deep3");
    // 1) 进程内放行；本地/远程服务器拒绝且短语指向差异表 F132（非裸码）。
    cs.add(
        "out_of_proc_refused_to_diff_sheet",
        activate_context(CLSCTX_INPROC_SERVER).is_ok()
            && activate_context(CLSCTX_LOCAL_SERVER) == Err(OUT_OF_PROC_DIFF_SHEET)
            && activate_context(CLSCTX_LOCAL_SERVER | CLSCTX_INPROC_SERVER)
                == Err(OUT_OF_PROC_DIFF_SHEET)
            && activate_context(CLSCTX_REMOTE_SERVER).is_err(),
        "",
    );
    // 2) 钉值锚（winbase.h 原值——一处一事实）。
    cs.add(
        "clsctx_pins",
        CLSCTX_INPROC_SERVER == 0x1 && CLSCTX_LOCAL_SERVER == 0x4 && CLSCTX_REMOTE_SERVER == 0x10,
        "",
    );
    // 3) 零上下文如实拒（不猜缺省类别——诚实边界）。
    cs.add("clsctx_zero_rejected", activate_context(0).is_err(), "");
    cs
}
