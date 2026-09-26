
// ---------------------------------------------------------------------------
// F002 · 深化批次五：节特征 → 内存保护页属性映射（装载语义的真映射面）
//
// 主册依据（G-A-02【功能定义】）：「节映射零拷贝（文件页缓存直接映射，写时
// 复制保护）」+【用户故事】「控制台窗口弹出、输出正常」——.text 可执行不可
// 写是装载安全语义的根基：节名特征 → 保护属性映射（R/W/X 三位），未知节名
// 如实给缺省 RW 并登记（不猜）。
// ---------------------------------------------------------------------------

/// 内存保护属性位（R/W/X 三位——与 F021 VirtualAlloc 六保护位的子集对齐）。
pub const PROT_R: u8 = 0b100;
pub const PROT_W: u8 = 0b010;
pub const PROT_X: u8 = 0b001;

/// 按节名映射保护属性（大小写不敏感；特征名取 Windows 惯用节名）。
pub fn section_protect(name: &str) -> u8 {
    let n = name.to_ascii_lowercase();
    match n.as_str() {
        ".text" | ".code" => PROT_R | PROT_X,              // 代码：可读可执行
        ".rdata" | ".rodata" => PROT_R,                     // 只读数据
        ".data" | ".bss" => PROT_R | PROT_W,                // 可写数据
        ".idata" | ".edata" => PROT_R,                      // 导入/导出表：只读
        ".rsrc" | ".reloc" | ".pdata" => PROT_R,            // 资源/重定位：只读
        ".tls" => PROT_R | PROT_W,                          // 线程局部：可写
        _ => PROT_R | PROT_W,                               // 未知节：缺省 RW + 登记面
    }
}

/// 未知节名检测（映射缺省值的同时要知道「这是缺省」——诊断面）。
pub fn section_protect_is_default(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    !matches!(
        n.as_str(),
        ".text" | ".code" | ".rdata" | ".rodata" | ".data" | ".bss" | ".idata" | ".edata"
            | ".rsrc" | ".reloc" | ".pdata" | ".tls"
    )
}

/// F002 深化批次五自检。
pub fn run_peblend_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F002-peblend-deep4");
    // 1) 关键三面：.text = R|X（可执行不可写——装载安全根基）；.data = R|W；
    //    .rdata = R。
    cs.add(
        "section_protect_core_three",
        section_protect(".text") == PROT_R | PROT_X
            && section_protect(".data") == PROT_R | PROT_W
            && section_protect(".rdata") == PROT_R
            && section_protect(".CODE") == PROT_R | PROT_X,
        "",
    );
    // 2) 表/资源面只读：.idata/.edata/.rsrc/.reloc/.pdata = R（不可写——
    //    装载后改表 = 越权路径）。
    cs.add(
        "section_protect_readonly_tables",
        section_protect(".idata") == PROT_R
            && section_protect(".edata") == PROT_R
            && section_protect(".rsrc") == PROT_R
            && section_protect(".reloc") == PROT_R,
        "",
    );
    // 3) 未知节缺省 RW 且「缺省」身份可见（.bss/.tls 在已知表；.mydata 不在）。
    cs.add(
        "section_protect_default_visible",
        section_protect(".bss") == PROT_R | PROT_W
            && section_protect(".tls") == PROT_R | PROT_W
            && section_protect_is_default(".mydata")
            && !section_protect_is_default(".text"),
        "",
    );
    cs
}
