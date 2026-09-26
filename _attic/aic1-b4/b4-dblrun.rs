
// ---------------------------------------------------------------------------
// F001 · 深化批次四：装载服务进程独立（崩溃互不传染——隔离记账面）
//
// 主册依据（G-A-01【设计细节】）：「装载服务进程独立于合成器与资源管理器
// （崩溃互不传染）」——独立性是承诺不是巧合：邻面崩溃必须记账可见且**不得**
// 影响装载管线判定（F175 隔离语义在本服务的落点）。
// ---------------------------------------------------------------------------

/// 邻居组件（装载服务独立性的两根支柱）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NeighborComponent {
    /// 合成器（渲染面）。
    Composer,
    /// 资源管理器（入口面）。
    Explorer,
}

/// 服务隔离记账（崩溃互不传染的观测面）。
#[derive(Clone, Copy, Debug)]
pub struct ServiceIsolation {
    pub composer_crashes: u32,
    pub explorer_crashes: u32,
    /// 装载服务自身存活标记。
    pub loader_alive: bool,
    /// 邻面崩溃时**正在运行**的已启动窗口数（不得被连带回收——计数恒不减）。
    pub running_windows: u32,
}

impl ServiceIsolation {
    pub const fn new() -> ServiceIsolation {
        ServiceIsolation { composer_crashes: 0, explorer_crashes: 0, loader_alive: true, running_windows: 0 }
    }

    /// 邻面崩溃登记（计数可见——异常零静默）；装载服务不跟随死亡。
    pub fn note_neighbor_crash(&mut self, which: NeighborComponent) {
        match which {
            NeighborComponent::Composer => self.composer_crashes += 1,
            NeighborComponent::Explorer => self.explorer_crashes += 1,
        }
    }

    /// 隔离判据：服务存活，且邻面崩溃**不构成**装载判据失效（verdict 面独立）。
    pub fn containment_intact(&self) -> bool {
        self.loader_alive
    }
}

/// F001 深化批次四自检。
pub fn run_dblrun_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F001-dblrun-deep3");
    // 1) 邻面崩溃计数分面可见；隔离判定不受邻面崩溃影响（服务仍存活）。
    let mut iso = ServiceIsolation::new();
    iso.note_neighbor_crash(NeighborComponent::Composer);
    iso.note_neighbor_crash(NeighborComponent::Composer);
    iso.note_neighbor_crash(NeighborComponent::Explorer);
    cs.add(
        "service_isolation_crash_containment",
        iso.composer_crashes == 2
            && iso.explorer_crashes == 1
            && iso.containment_intact(),
        "",
    );
    // 2) 正在运行的窗口不因邻面崩溃被连带回收（计数恒不减——F175 语义锚）。
    let mut iso2 = ServiceIsolation::new();
    iso2.running_windows = 5;
    iso2.note_neighbor_crash(NeighborComponent::Composer);
    cs.add(
        "running_windows_preserved_on_neighbor_crash",
        iso2.running_windows == 5 && iso2.containment_intact(),
        "",
    );
    // 3) 服务自身死亡 = 隔离判定如实红（不装作还活着）。
    let mut iso3 = ServiceIsolation::new();
    iso3.loader_alive = false;
    cs.add("loader_down_honest", !iso3.containment_intact(), "");
    cs
}
