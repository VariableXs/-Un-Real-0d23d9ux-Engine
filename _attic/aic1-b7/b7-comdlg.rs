
// ---------------------------------------------------------------------------
// F008 · 深化批次七：OFN 多选文件名缓冲（双 NUL 终止约定——lpstrFile 的
// 多选返回形态）
//
// 主册依据（G-A-08【功能定义】）：「GetOpenFileNameW + IFileDialog 双接口」
// ——OFN_ALLOWMULTISELECT 时 lpstrFile 返回形态：`目录\0文件1\0文件2\0\0`
// （双 NUL 终止）。打包/解包双向核（程序完全认识返回值的根基）。
// ---------------------------------------------------------------------------

/// 打包多选返回（dir + 逐文件 NUL 分隔 + 双 NUL 终止）。缓冲不足如实截断。
pub fn ofn_pack_multiselect(dir: &str, files: &[&str], buf: &mut [u8]) -> usize {
    let mut n = 0usize;
    let mut put = |b: u8, buf: &mut [u8], n: &mut usize| -> bool {
        if *n < buf.len() {
            buf[*n] = b;
            *n += 1;
            true
        } else {
            false
        }
    };
    for &b in dir.as_bytes() {
        if !put(b, buf, &mut n) {
            return n;
        }
    }
    if !put(0, buf, &mut n) {
        return n;
    }
    for f in files {
        for &b in f.as_bytes() {
            if !put(b, buf, &mut n) {
                return n;
            }
        }
        if !put(0, buf, &mut n) {
            return n;
        }
    }
    if !put(0, buf, &mut n) {
        return n;
    }
    n
}

/// 解包计数（校验双 NUL 终止 + 逐名非空——结构不符 None；单选形态（首段
/// 含完整路径、无第二段）计 1）。
pub fn ofn_unpack_count(data: &[u8]) -> Option<usize> {
    let mut count = 0usize;
    let mut i = 0usize;
    let mut has_content = false;
    loop {
        let start = i;
        while i < data.len() && data[i] != 0 {
            i += 1;
        }
        if i >= data.len() {
            return None; // 无终止 NUL——结构不符
        }
        if i == start {
            break; // 空段 = 终止
        }
        count += 1;
        has_content = true;
        i += 1;
    }
    if has_content {
        Some(count)
    } else {
        None
    }
}

/// F008 深化批次七自检。
pub fn run_comdlg_deep6_checks() -> CheckSet {
    let mut cs = CheckSet::new("F008-comdlg-deep6");
    // 1) 多选打包：目录 + 两文件 + 双 NUL；解包计数 2。
    let mut buf = [0u8; 128];
    let n = ofn_pack_multiselect("C:\\Downloads", &["a.txt", "b.bin"], &mut buf);
    let count = ofn_unpack_count(&buf[..n]);
    let ends_double_nul = buf[n - 2] == 0 && buf[n - 1] == 0;
    cs.add(
        "ofn_multiselect_pack_count",
        count == Some(2) && ends_double_nul,
        "",
    );
    // 2) 单选形态：单段含路径（无第二段）→ 计 1。
    let mut single = alloc::vec::Vec::new();
    single.extend_from_slice(b"C:\\a.txt");
    single.push(0);
    single.push(0);
    cs.add("ofn_single_select_count_one", ofn_unpack_count(&single) == Some(1), "");
    // 3) 无终止 NUL 如实 None（截断缓冲不冒充完整返回）。
    let mut broken = buf[..n - 1].to_vec();
    let _ = &broken;
    cs.add(
        "ofn_unpack_malformed_rejected",
        ofn_unpack_count(&[0x41, 0x00, 0x42, 0x00]).is_none(), // 无终止双 NUL
        "",
    );
    cs
}
