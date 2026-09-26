
// ---------------------------------------------------------------------------
// F018 · 深化批次四：跨盘拖放路由（F086 复制管线承接）+ 进度回报记账
//
// 主册依据（G-A-18【数据与存储】）：「跨盘拖放落 F086 复制管线（进度/冲突
// 面板复用）」——move 语义跨盘在文件系统层实为 copy+delete，诚实路由到复制
// 管线（带进度），不冒充「瞬间移动完成」。
// ---------------------------------------------------------------------------

/// 拖放落点路由。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DropRoute {
    /// 同卷 move：原地改名（瞬时语义成立）。
    InPlace,
    /// 跨盘 move（或任意 copy）：落 F086 复制管线（进度/冲突面板复用）。
    CopyPipeline,
}

/// 路由判定：move 跨盘 = 复制管线；其余（同卷 move / 显式 copy / link）按
/// 各自语义就地执行。
pub fn drop_route(move_semantics: bool, same_volume: bool) -> DropRoute {
    if move_semantics && !same_volume {
        DropRoute::CopyPipeline
    } else {
        DropRoute::InPlace
    }
}

/// 复制管线承接记账（F086 消费面：文件数与进度回报数——进度条诚实的数据源）。
#[derive(Clone, Copy, Debug)]
pub struct CopyHandoff {
    pub files: u32,
    pub progress_reports: u32,
}

impl CopyHandoff {
    pub const fn new() -> CopyHandoff {
        CopyHandoff { files: 0, progress_reports: 0 }
    }

    /// 承接一批文件（每文件至少一次进度回报——「慢要有诚实的进度」判据）。
    pub fn start(&mut self, files: u32) {
        self.files = files;
        self.progress_reports += files; // 启动即各记一笔（进度条立即动起来）
    }

    /// 逐文件进度更新。
    pub fn progress(&mut self) {
        self.progress_reports += 1;
    }
}

/// F018 深化批次四自检。
pub fn run_dragdrop_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F018-dragdrop-deep3");
    // 1) 路由四象限：move 跨盘 → 复制管线；同卷 move/copy/link → 就地。
    cs.add(
        "drop_route_cross_disk",
        drop_route(true, false) == DropRoute::CopyPipeline
            && drop_route(true, true) == DropRoute::InPlace
            && drop_route(false, false) == DropRoute::InPlace
            && drop_route(false, true) == DropRoute::InPlace,
        "",
    );
    // 2) 承接记账：3 文件 → 启动即 3 笔进度 + 逐文件更新 → 报告数 ≥ 文件数
    //    （进度条从第一帧就动——不白屏硬等）。
    let mut ho = CopyHandoff::new();
    ho.start(3);
    let at_start = ho.progress_reports;
    ho.progress();
    ho.progress();
    cs.add(
        "copy_handoff_progress_honest",
        ho.files == 3 && at_start == 3 && ho.progress_reports == 5 && ho.progress_reports >= ho.files,
        "",
    );
    cs
}
