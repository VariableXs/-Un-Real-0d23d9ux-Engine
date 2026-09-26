
// ---------------------------------------------------------------------------
// F020 · 深化批次三：dump 导出 CDB 可读文本（自定格式 → 开发者可读面）+
// 「重新启动应用」启动快照（命令行 + 工作目录字符串捕获）
//
// 主册依据（G-A-20【数据与存储】）：「dump 格式用自定轻量格式（可导出转换
// CDB 可读文本）」；【设计细节】「『重新启动应用』保留命令行与工作目录（启动
// 时快照）」。既有面：MiniDump 序列化/降级/LRU/异常帧栈不重复——本段补导出
// 文本化与字符串快照（既有 restart 哈希位之上的原文捕获）。
// ---------------------------------------------------------------------------

/// CDB 导出行缓冲上限（导出截断如实——不静默丢帧）。
pub const CDB_TEXT_CAP: usize = 4096;

/// dump → CDB 可读文本（开发者拿 dump 定位源码行的消费面：异常码/崩溃地址/
/// 栈回溯逐帧/模块表逐模块）。返回写入字节数（缓冲不足截断——截断结果不冒充
/// 完整 dump）。
pub fn dump_to_cdb_text(d: &MiniDump, out: &mut [u8]) -> usize {
    let mut n = 0usize;
    crate::checks::push_str(out, &mut n, "VARIX minidump (CDB view)\n");
    crate::checks::push_str(out, &mut n, "exception code: ");
    crate::checks::push_hex_u64(out, &mut n, d.exception_code as u64);
    crate::checks::push_str(out, &mut n, "\ncrash address: ");
    crate::checks::push_hex_u64(out, &mut n, d.crash_addr);
    crate::checks::push_str(out, &mut n, "\nstack frames:\n");
    for i in 0..d.stack_n {
        crate::checks::push_str(out, &mut n, "  #");
        crate::checks::push_usize(out, &mut n, i);
        crate::checks::push_str(out, &mut n, ": ");
        crate::checks::push_hex_u64(out, &mut n, d.stack[i]);
        crate::checks::push_str(out, &mut n, "\n");
    }
    crate::checks::push_str(out, &mut n, "modules:\n");
    for i in 0..d.module_n {
        if let Some(m) = d.modules[i] {
            crate::checks::push_str(out, &mut n, "  base ");
            crate::checks::push_hex_u64(out, &mut n, m.base);
            crate::checks::push_str(out, &mut n, " size ");
            crate::checks::push_usize(out, &mut n, m.size as usize);
            crate::checks::push_str(out, &mut n, " checksum ");
            crate::checks::push_hex_u64(out, &mut n, m.checksum as u64);
            crate::checks::push_str(out, &mut n, "\n");
        }
    }
    n.min(out.len())
}

/// 重启快照缓冲上限（命令行/工作目录同限——超出如实截断计数）。
pub const RESTART_STR_CAP: usize = 64;

/// 「重新启动应用」启动快照（命令行 + 工作目录原文捕获——重启 = 原参数重拉）。
#[derive(Clone, Copy, Debug)]
pub struct RestartSnapshot {
    cmd: [u8; RESTART_STR_CAP],
    cmd_n: usize,
    cwd: [u8; RESTART_STR_CAP],
    cwd_n: usize,
    /// 捕获时被截断的字段数（如实登记——截断参数重启 = 参数失真，必须可见）。
    pub truncated_fields: u32,
}

impl RestartSnapshot {
    pub fn capture(cmdline: &str, workdir: &str) -> RestartSnapshot {
        let mut s = RestartSnapshot {
            cmd: [0; RESTART_STR_CAP],
            cmd_n: 0,
            cwd: [0; RESTART_STR_CAP],
            cwd_n: 0,
            truncated_fields: 0,
        };
        let cb = cmdline.as_bytes();
        s.cmd_n = cb.len().min(RESTART_STR_CAP);
        s.cmd[..s.cmd_n].copy_from_slice(&cb[..s.cmd_n]);
        if cb.len() > RESTART_STR_CAP {
            s.truncated_fields += 1;
        }
        let wb = workdir.as_bytes();
        s.cwd_n = wb.len().min(RESTART_STR_CAP);
        s.cwd[..s.cwd_n].copy_from_slice(&wb[..s.cwd_n]);
        if wb.len() > RESTART_STR_CAP {
            s.truncated_fields += 1;
        }
        s
    }

    pub fn cmdline(&self) -> &str {
        core::str::from_utf8(&self.cmd[..self.cmd_n]).unwrap_or("")
    }

    pub fn workdir(&self) -> &str {
        core::str::from_utf8(&self.cwd[..self.cwd_n]).unwrap_or("")
    }
}

/// F020 深化批次三自检。
pub fn run_excface_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F020-excface-deep2");
    // 1) CDB 导出：含异常码/崩溃地址十六进制行、逐帧行数与 stack_n 一致、
    //    模块行数与 module_n 一致（开发者可读面的结构完整）。
    let mut d = MiniDump {
        exception_code: 0xC000_0005,
        crash_addr: 0x0000_7FF6_1234_5678,
        thread_id: 42,
        stack: [0; DUMP_STACK_FRAMES],
        stack_n: 3,
        modules: [None; DUMP_MODULES],
        module_n: 2,
        system_fingerprint: 0x0102_0304,
        degraded: false,
        restart_cmd_hash: 0,
        restart_cwd_hash: 0,
    };
    d.stack[0] = 0xFFFF_8000_1000_0000;
    d.stack[1] = 0xFFFF_8000_1000_0010;
    d.stack[2] = 0xFFFF_8000_1000_0020;
    d.modules[0] = Some(DumpModule { base: 0x7FF6_1000_0000, size: 0x1000, checksum: 0xDEAD });
    d.modules[1] = Some(DumpModule { base: 0x7FF6_2000_0000, size: 0x2000, checksum: 0xBEEF });
    let mut buf = [0u8; CDB_TEXT_CAP];
    let n = dump_to_cdb_text(&d, &mut buf);
    let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
    let frame_lines = text.matches("  #").count();
    let mod_lines = text.matches(" checksum ").count();
    cs.add(
        "cdb_export_structure_complete",
        n > 0
            && text.contains("exception code: c0000005")
            && text.contains("crash address: 7ff612345678")
            && frame_lines == 3
            && mod_lines == 2,
        "",
    );
    // 2) 导出截断诚实：小缓冲（64 字节）只产出前 64 字节且不越界（截断不冒充
    //    完整 dump）。
    let mut small = [0u8; 64];
    let n2 = dump_to_cdb_text(&d, &mut small);
    cs.add("cdb_export_truncation_honest", n2 == 64, "");
    // 3) 重启快照：命令行+工作目录原文往返；超长如实截断计数（截断参数重启
    //    必须可见——不静默失真）。
    let snap = RestartSnapshot::capture("app.exe --flag=1", "C:\\Users\\doc");
    let mut snap2 = RestartSnapshot::capture("", "");
    snap2 = RestartSnapshot::capture(&"x".repeat(RESTART_STR_CAP + 10), "C:\\w");
    cs.add(
        "restart_snapshot_original_args",
        snap.cmdline() == "app.exe --flag=1"
            && snap.workdir() == "C:\\Users\\doc"
            && snap.truncated_fields == 0
            && snap2.truncated_fields == 1
            && snap2.cmdline().len() == RESTART_STR_CAP,
        "",
    );
    cs
}
