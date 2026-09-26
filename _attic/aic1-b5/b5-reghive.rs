
// ---------------------------------------------------------------------------
// F009 · 深化批次五：蜂巢键前缀枚举（树视图的数据源）
//
// 主册依据（G-A-09【交互设计】）：「设置中心显示『查看注册表内容』入口
// （只读十六进制/键树视图）」——树视图按前缀枚举子键：Hive 全量记录上的
// 前缀过滤（零堆面：枚举结果写入调用方缓冲行表）。
// ---------------------------------------------------------------------------

/// 按前缀枚举键（键树视图：key 以 `prefix\` 起头或恰等于 prefix 的记录全收）。
/// 结果以 `key=value字节序` 行写入 buf，行间 '\n'。返回 (写入字节, 命中数)。
pub fn hive_enumerate_prefix(hive: &Hive, prefix: &str, buf: &mut [u8]) -> (usize, usize) {
    let mut out = 0usize;
    let mut hits = 0usize;
    let total = hive.record_count();
    for i in 0..total {
        let rec = match hive.record_at(i) {
            Some(r) => r,
            None => continue,
        };
        let k = rec.key_str();
        let matched = k == prefix || (k.len() > prefix.len() && k.starts_with(prefix) && k.as_bytes()[prefix.len()] == b'\\');
        if !matched {
            continue;
        }
        hits += 1;
        if hits > 1 && out < buf.len() {
            buf[out] = b'\n';
            out += 1;
        }
        for &b in k.as_bytes() {
            if out >= buf.len() {
                return (out, hits);
            }
            buf[out] = b;
            out += 1;
        }
        for &b in b" = " {
            if out >= buf.len() {
                return (out, hits);
            }
            buf[out] = b;
            out += 1;
        }
        let vb = rec.val_bytes();
        for (j, &b) in vb.iter().enumerate() {
            if j >= 16 || out >= buf.len() {
                break; // 值预览 16 字节（查看器行内预览口径）
            }
            if out >= buf.len() {
                return (out, hits);
            }
            buf[out] = b;
            out += 1;
        }
    }
    (out, hits)
}

/// F009 深化批次五自检。
pub fn run_reghive_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F009-reghive-deep4");
    // 1) 前缀枚举：Software\MyApp 命中其直接键树（模板内构造三键）。
    let mut hive = Hive::new(SAMPLE_TEMPLATE);
    hive.set("Software\\MyApp\\Theme", b"dark");
    hive.set("Software\\MyApp\\Recent\\F1", b"a");
    hive.set("Software\\VARIX\\Platform", b"STAR I");
    let mut buf = [0u8; 256];
    let (n, hits) = hive_enumerate_prefix(&hive, "Software\\MyApp", &mut buf);
    let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
    cs.add(
        "hive_prefix_enum_direct",
        hits == 2 && text.contains("Software\\MyApp\\Theme") && text.contains("Software\\MyApp\\Recent\\F1"),
        "",
    );
    // 2) 深层前缀只命中子树：Software\MyApp\Recent 只中一条。
    let (n2, hits2) = hive_enumerate_prefix(&hive, "Software\\MyApp\\Recent", &mut buf);
    let t2 = core::str::from_utf8(&buf[..n2]).unwrap_or("");
    cs.add(
        "hive_prefix_enum_deep",
        hits2 == 1 && t2.contains("Recent\\F1") && !t2.contains("Theme"),
        "",
    );
    // 3) 无前缀同名键不被误收（前缀必须整段或 `\` 边界——Software\My 不中）。
    let (_, hits3) = hive_enumerate_prefix(&hive, "Software\\My", &mut buf);
    cs.add("hive_prefix_enum_boundary", hits3 == 0, "");
    cs
}
