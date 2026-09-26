
// ---------------------------------------------------------------------------
// F010 · 深化批次五：镜像区读态三态判定（可见可读不可写——合并视图细则）
//
// 主册依据（G-A-10【设计细节】）：「合并视图的『系统镜像次之』指镜像内只读
// 文件可见可读不可写」——三态判定面：镜像区内文件 VisibleReadableNotWritable，
// 沙盒/用户区可写（走既有重定向），不存在的路径如实 Unknown。
// ---------------------------------------------------------------------------

/// 镜像区读态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MirrorReadState {
    /// 镜像区：可见可读，写一律拒（落重定向/拒绝由既有写面裁决）。
    VisibleReadableNotWritable,
    /// 非镜像区（沙盒/用户直通白名单/共享区）：可写语义由既有 classify/write 面
    /// 裁决——本面不越权下结论。
    OutsideMirror,
}

/// 三态判定（镜像前缀与 EscapeLedger::judge_mirror_write 同表——一处一事实）。
pub fn mirror_read_state(path: &str) -> MirrorReadState {
    const MIRROR_PREFIXES: [&str; 3] =
        ["c:\\windows", "c:\\program files", "c:\\program files (x86)"];
    let p = path.to_ascii_lowercase();
    if MIRROR_PREFIXES.iter().any(|m| p.starts_with(m)) {
        MirrorReadState::VisibleReadableNotWritable
    } else {
        MirrorReadState::OutsideMirror
    }
}

/// F010 深化批次五自检。
pub fn run_fsredir_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F010-fsredir-deep4");
    // 1) 镜像区三面：Windows / Program Files / x86 → 可见可读不可写。
    cs.add(
        "mirror_read_state_system_areas",
        mirror_read_state("C:\\Windows\\notepad.exe") == MirrorReadState::VisibleReadableNotWritable
            && mirror_read_state("c:\\Program Files\\app\\a.exe")
                == MirrorReadState::VisibleReadableNotWritable
            && mirror_read_state("C:\\Program Files (x86)\\old\\b.dll")
                == MirrorReadState::VisibleReadableNotWritable,
        "",
    );
    // 2) 非镜像区不下结论（沙盒/用户区写语义归既有面——不越权）。
    cs.add(
        "mirror_read_state_outside_honest",
        mirror_read_state("C:\\Users\\doc\\report.txt") == MirrorReadState::OutsideMirror
            && mirror_read_state("~\\AppSandbox\\app\\cfg.ini") == MirrorReadState::OutsideMirror,
        "",
    );
    // 3) 大小写不敏感（镜像前缀判定与写面同表同判——一处一事实）。
    cs.add(
        "mirror_read_state_case_insensitive",
        mirror_read_state("c:\\WiNdOwS\\x") == MirrorReadState::VisibleReadableNotWritable,
        "",
    );
    cs
}
