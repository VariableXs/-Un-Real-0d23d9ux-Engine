
// ---------------------------------------------------------------------------
// F012 · 深化批次四：会话导出存文本（F096 联动）
//
// 主册依据（G-A-12【数据与存储】）：「会话导出功能（F096）可存文本」——
/// 导出是回看缓冲的有界落盘面：行序保真、截断如实计数（不冒充完整导出）。
// ---------------------------------------------------------------------------

/// 导出统计（bytes/lines/truncated——诊断页三件）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportStats {
    pub bytes: usize,
    pub lines: usize,
    /// 因缓冲不足未能导出的行数（如实计数）。
    pub truncated_lines: u32,
}

/// 回看缓冲导出：逐行写入（行间 '\n'），缓冲满即停并如实计数剩余。
pub fn export_session_lines(lines: &[&[u8]], out: &mut [u8]) -> ExportStats {
    let mut st = ExportStats { bytes: 0, lines: 0, truncated_lines: 0 };
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            if st.bytes < out.len() {
                out[st.bytes] = b'\n';
                st.bytes += 1;
            } else {
                st.truncated_lines += 1;
                continue;
            }
        }
        let room = out.len() - st.bytes;
        if line.len() <= room {
            out[st.bytes..st.bytes + line.len()].copy_from_slice(line);
            st.bytes += line.len();
            st.lines += 1;
        } else {
            // 整行放不下 → 行级截断（部分行不冒充完整行——该行计入截断）。
            out[st.bytes..out.len()].copy_from_slice(&line[..room]);
            st.bytes = out.len();
            st.truncated_lines += (lines.len() - i) as u32;
            return st;
        }
    }
    st
}

/// F012 深化批次四自检。
pub fn run_condrv_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F012-condrv-deep3");
    // 1) 全量导出：三行保真，行序一致，统计三件齐。
    let lines: [&[u8]; 3] = [b"build: ok", b"test: 214 passed", b"exit 0"];
    let mut buf = [0u8; 256];
    let st = export_session_lines(&lines, &mut buf);
    let text = core::str::from_utf8(&buf[..st.bytes]).unwrap_or("");
    cs.add(
        "session_export_full_fidelity",
        st.lines == 3
            && st.truncated_lines == 0
            && st.bytes == 9 + 1 + 16 + 1 + 6
            && text.starts_with("build: ok\ntest: 214 passed\nexit 0"),
        "",
    );
    // 2) 有界导出：小缓冲行级截断如实计数（放不下的行数全计入，不静默丢）。
    let mut small = [0u8; 12];
    let st2 = export_session_lines(&lines, &mut small);
    cs.add(
        "session_export_truncation_honest",
        st2.lines == 1 && st2.truncated_lines == 2 && st2.bytes == 12,
        "",
    );
    // 3) 空会话导出 = 零字节零行（不写幽灵换行）。
    let mut tiny = [0u8; 8];
    let st3 = export_session_lines(&[], &mut tiny);
    cs.add("session_export_empty", st3.bytes == 0 && st3.lines == 0, "");
    cs
}
