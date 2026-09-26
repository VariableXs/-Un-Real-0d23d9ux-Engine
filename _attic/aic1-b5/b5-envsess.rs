
// ---------------------------------------------------------------------------
// F011 · 深化批次五：会话快照差异三分类（诊断面——还原点/F121 联动的读侧）
//
// 主册依据（G-A-11【状态与异常】）：「改完正在运行的程序不受影响（会话边界
// 语义）」——会话快照前后对比：新增/修改/删除三分类（诊断中心「环境变量
// 变更史」的数据源）。
// ---------------------------------------------------------------------------

/// 差异分类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VarDiff {
    Added,
    Modified,
    Removed,
}

/// 会话快照 diff（两份 EnvSnapshot 逐变量比对——容量内变量）。
/// 返回 (新增数, 修改数, 删除数)。
pub fn snapshot_diff(before: &EnvSnapshot, after: &EnvSnapshot) -> (usize, usize, usize) {
    let mut added = 0usize;
    let mut modified = 0usize;
    let mut removed = 0usize;
    // after 视角：新增/修改
    for i in 0..after.len() {
        if let Some((name, val, _)) = after.slot_at(i) {
            match before.get(name) {
                None => added += 1,
                Some(old) => {
                    if old != val {
                        modified += 1;
                    }
                }
            }
        }
    }
    // before 视角：删除（after 中不存在）
    for i in 0..before.len() {
        if let Some((name, _, _)) = before.slot_at(i) {
            if after.get(name).is_none() {
                removed += 1;
            }
        }
    }
    (added, modified, removed)
}

/// F011 深化批次五自检。
pub fn run_envsess_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F011-envsess-deep4");
    // 1) 三分类齐：改 PATH（修改）+ 加 GOPATH（新增）+ 删 TEMP（删除）。
    let mut t1 = EnvTable::new();
    t1.set("PATH", b"C:\\bin", Scope::System);
    t1.set("TEMP", b"C:\\tmp", Scope::System);
    let snap1 = t1.snapshot();
    let mut t2 = EnvTable::new();
    t2.set("PATH", b"C:\\bin;C:\\go", Scope::System);
    t2.set("GOPATH", b"C:\\go", Scope::User);
    let snap2 = t2.snapshot();
    let (a, m, r) = snapshot_diff(&snap1, &snap2);
    cs.add(
        "snapshot_diff_three_classes",
        (a, m, r) == (1, 1, 1),
        "",
    );
    // 2) 无变更 = 三零（幂等快照 diff——诊断面不造噪声）。
    let (a2, m2, r2) = snapshot_diff(&snap1, &snap1);
    cs.add("snapshot_diff_no_change", (a2, m2, r2) == (0, 0, 0), "");
    // 3) 值同义大小写不敏感语义下仍按字节比对（diff 忠于字节——不做「看似
    //    相同」的合并）。
    let mut t3 = EnvTable::new();
    t3.set("PATH", b"C:\\BIN", Scope::System);
    let snap3 = t3.snapshot();
    let (_, m3, _) = snapshot_diff(&snap1, &snap3);
    cs.add("snapshot_diff_byte_faithful", m3 == 1, "");
    cs
}
