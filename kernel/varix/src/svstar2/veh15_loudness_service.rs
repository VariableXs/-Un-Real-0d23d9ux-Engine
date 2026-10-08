//! VE-F1414 · 引擎级响度归一服务（VE-H 域 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1414`
//!
//! **判据（锚点原文六条）**：批量扫描、预设表、指纹缓存、增量、报告、判据。
//!
//! # 职责
//!
//! 批量响度分析：媒体库全量扫描的 LUFS / True Peak 结果，
//! 归一预设（R128 −23 LUFS 广播 / 流媒体 −14 LUFS / 自定义），
//! 分析缓存（文件指纹→结果，不重复扫描），增量扫描
//! （新增/变更文件才入队），全库响度报告导出。
//!
//! # 工程量构成（锚点原文）
//!
//! 批量扫描调度 100 行＋预设表 60 行＋指纹缓存 80 行＋增量 70 行＋报告 50 行。
//!
//! ---
//!
//! ## 设计要点一：缓存键**必须含算法版本**（锚点「版本入键纪律」）
//!
//! 锚点：「缓存键（哈希+分析算法版本（算法升级失效重扫——版本入键纪律）」。
//! 只用文件哈希做键是**最常见的假优化**：算法从 A 升到 B（测量口径
//! 改了，比如门限从 −70 LU 提到 −60 LU），哈希没变 ⇒ 命中旧缓存 ⇒
//! 全库返回**旧口径**的 LUFS，而界面上显示的是新算法名。这类缺陷
//! 极隐蔽：所有断言都"通过"，只是结果整体错了。
//! 故 [`CacheKey`] 含 [`ANALYSIS_ALGO_VERSION`]，版本不同 ⇒ 键不同 ⇒
//! 自然失效重扫，**不靠"记得清缓存"这种约定**。
//!
//! ## 设计要点二：缓存是**纯函数记忆**，不是可变表
//!
//! 锚点：「分析缓存（文件指纹→结果缓存——不重复扫描）」。缓存的
//! 价值在于「同文件不重扫」，而**只有纯记忆才保证这点**：若缓存表
//! 存的是"上一轮看到的东西"（可变状态），那么扫描顺序一变、
//! 中途加一个文件，命中行为就会漂移。故本实现里缓存是
//! [`LoudnessCache`] 的**只读查询 + 显式插入**，命中判定纯由键决定，
//! 与扫描历史无关。
//!
//! ## 设计要点三：增量扫描只处理**变更集**，全量只在首次/升级
//!
//! 锚点：「增量（库变更事件（新增/修改文件才分析——增量扫描
//! （全量扫描只发生在首次/算法升级）」。关键纪律有两条：
//! 一是**修改文件必须被识别**（只认新增会漏掉改过的文件——而改过的
//! 文件正是最需要重新测量的）；二是**全量有且只有两个触发条件**
//! （首次扫描、算法升级），不是"扫描队列空了"就算全量。
//! 故 [`ScanReason`] 显式枚举三个原因，判据直接断原因可区分。
//!
//! ## 设计要点四：预设表**数据驱动且可扩展**，但越界值必须拒收
//!
//! 锚点：「预设表（R128（−23 LUFS 广播）/流媒体（−14 LUFS）/
//! 自定义——预设表开放（平台目标随生态更新（数据驱动）」。
//! 「数据驱动」意味着**新增平台不改代码**（进表即可）；但开放表
//! 同时意味着**坏数据能进来**，故 [`TargetTable::upsert`] 必须
//! 拒收非法目标（NaN / 超出合理 LUFS 区间 / True Peak > 0 dBTP
//! 这种物理不可能值），而不是照单全收。
//!
//! ## 设计要点五：报告必须**带口径与版本**，离开模块仍自解释
//!
//! 锚点：「报告导出（全库响度归一报告——剪辑与管理用）」。
//! 报告会被导出到引擎之外（剪辑软件、项目管理表），若不带
//! 「预设 id + 算法版本 + 采样统计口径」，收报告的人无法判断
//! 两份报告能不能比。故 [`LoudnessReport`] 带 [`ReportMeta`]，
//! 且导出文本首行即口径头。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 锚点「文件哈希」——指纹字节数（取 128 bit，够长且与常见库一致）。
pub const FINGERPRINT_BYTES: usize = 16;

/// 锚点「分析算法版本」——**入缓存键**（设计要点一）。
///
/// 任何影响测量口径的改动都必须 bump：门限、加窗、静音门、
/// 采样率折算方式……否则旧缓存会以新算法的名义返回旧口径的值。
pub const ANALYSIS_ALGO_VERSION: u32 = 3;

/// 锚点「True Peak」——以 dBTP 计量，上界为 0（超过 0 即数字削波）。
pub const TRUE_PEAK_MAX_DBTP: f32 = 0.0;

/// True Peak 合理下界（−40 dBTP 以下基本是静音，低于此值视为异常）。
pub const TRUE_PEAK_MIN_DBTP: f32 = -40.0;

/// 响度目标合理区间（LUFS）。低于 −70 基本无声，高于 −5 必然削波。
pub const LOUDNESS_MIN_LUFS: f32 = -70.0;
pub const LOUDNESS_MAX_LUFS: f32 = -5.0;

/// 增益应用上下限（dB）——超过 +24 dB 会把底噪一起抬起来。
pub const GAIN_MAX_DB: f32 = 24.0;
pub const GAIN_MIN_DB: f32 = -24.0;

/// 门限（LUFS）——低于此值按静音处理，不参与平均。
pub const SILENCE_GATE_LUFS: f32 = -70.0;

// ---------------------------------------------------------------------------
// 二、预设表
// ---------------------------------------------------------------------------

/// 归一预设（锚点「R128 / 流媒体平台目标表 / 自定义」三分类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresetKind {
    /// R128 广播。
    R128,
    /// 流媒体平台。
    Streaming,
    /// 自定义。
    Custom,
}

impl PresetKind {
    pub fn name(self) -> &'static str {
        match self {
            PresetKind::R128 => "r128",
            PresetKind::Streaming => "streaming",
            PresetKind::Custom => "custom",
        }
    }
}

/// 一条归一目标（响度目标 + True Peak 上限）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoudnessTarget {
    pub target_lufs: f32,
    pub ceiling_dbtp: f32,
}

impl LoudnessTarget {
    pub const fn new(target_lufs: f32, ceiling_dbtp: f32) -> LoudnessTarget {
        LoudnessTarget { target_lufs, ceiling_dbtp }
    }

    /// 锚点 R128：−23 LUFS / −1 dBTP。
    pub const R128: LoudnessTarget = LoudnessTarget::new(-23.0, -1.0);
    /// 锚点流媒体：−14 LUFS / −1 dBTP。
    pub const STREAMING: LoudnessTarget = LoudnessTarget::new(-14.0, -1.0);

    /// 目标是否在合法域内（设计要点四：开放表必须拒收坏数据）。
    pub fn sane(self) -> bool {
        let l = self.target_lufs;
        let p = self.ceiling_dbtp;
        if l.is_nan() || p.is_nan() {
            return false;
        }
        if l < LOUDNESS_MIN_LUFS || l > LOUDNESS_MAX_LUFS {
            return false;
        }
        if p < TRUE_PEAK_MIN_DBTP || p > TRUE_PEAK_MAX_DBTP {
            return false;
        }
        true
    }
}

/// 预设表（**数据驱动**，新增平台只进表不改代码）。
#[derive(Clone, Debug, PartialEq)]
pub struct TargetTable {
    /// 按 id 排序的条目（`id` 是平台标识，如 `r128` / `streaming`）。
    rows: Vec<TableRow>,
}

/// 预设表一行。
#[derive(Clone, Debug, PartialEq)]
pub struct TableRow {
    pub id: String,
    pub kind: PresetKind,
    pub target: LoudnessTarget,
}

/// 预设表拒收原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableError {
    /// 目标值越界/非有限。
    TargetNotSane,
    /// id 重复。
    IdDuplicate,
    /// id 为空。
    IdEmpty,
}

impl TableError {
    pub fn name(self) -> &'static str {
        match self {
            TableError::TargetNotSane => "target_not_sane",
            TableError::IdDuplicate => "id_duplicate",
            TableError::IdEmpty => "id_empty",
        }
    }
}

impl TargetTable {
    /// 内置两行（锚点明列的 R128 与流媒体）。
    pub fn builtin() -> TargetTable {
        let mut rows: Vec<TableRow> = Vec::new();
        rows.push(TableRow {
            id: String::from("r128"),
            kind: PresetKind::R128,
            target: LoudnessTarget::R128,
        });
        rows.push(TableRow {
            id: String::from("streaming"),
            kind: PresetKind::Streaming,
            target: LoudnessTarget::STREAMING,
        });
        TargetTable { rows }
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn rows(&self) -> &[TableRow] {
        &self.rows
    }

    /// O(1)-ish 查表（线性但表极小；锚点未要求 O(1)，此处不为性能牺牲可读性）。
    pub fn get(&self, id: &str) -> Option<LoudnessTarget> {
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].id == id {
                return Some(self.rows[i].target);
            }
            i += 1;
        }
        None
    }

    /// 数据驱动入口：新增平台**只进表**，不改代码（设计要点四）。
    pub fn upsert(&mut self, row: TableRow) -> Result<(), TableError> {
        if row.id.is_empty() {
            return Err(TableError::IdEmpty);
        }
        if !row.target.sane() {
            return Err(TableError::TargetNotSane);
        }
        // 拒收重复 id：同名两行会让 `get` 只命中第一行，
        // 而调用方以为改的是生效那一行 ⇒ 静默失效。
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].id == row.id {
                return Err(TableError::IdDuplicate);
            }
            i += 1;
        }
        self.rows.push(row);
        // 按 id 排序：查表与导出的确定性依赖它。
        self.rows.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(())
    }

    /// 导出口径头（预设清单，报告须自解释）。
    pub fn dump(&self) -> String {
        let mut s = String::from("presets:");
        let mut i = 0usize;
        while i < self.rows.len() {
            s.push_str(&self.rows[i].id);
            s.push('=');
            s.push_str(&self.rows[i].target.target_lufs.to_string());
            s.push('/');
            s.push_str(&self.rows[i].target.ceiling_dbtp.to_string());
            if i + 1 < self.rows.len() {
                s.push(',');
            }
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 三、指纹与缓存
// ---------------------------------------------------------------------------

/// 文件指纹（128 bit，O(1) 比对）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fingerprint(pub [u8; FINGERPRINT_BYTES]);

impl Fingerprint {
    pub const fn zero() -> Fingerprint {
        Fingerprint([0; FINGERPRINT_BYTES])
    }

    /// 构造（截断到 16 字节，长输入取前 16）。
    pub fn from_slice(b: &[u8]) -> Fingerprint {
        let mut out = [0u8; FINGERPRINT_BYTES];
        let mut i = 0usize;
        while i < FINGERPRINT_BYTES && i < b.len() {
            out[i] = b[i];
            i += 1;
        }
        Fingerprint(out)
    }

    pub fn is_zero(&self) -> bool {
        let mut i = 0usize;
        while i < FINGERPRINT_BYTES {
            if self.0[i] != 0 {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 缓存键 = 指纹 + **算法版本**（设计要点一：版本入键纪律）。
///
/// 单独暴露 [`CacheKey::fingerprint_only`] 是为了**测试反例**：
/// 若某实现只按指纹命中，改版本后仍能查到旧结果——
/// 判据用反例证明「版本确实参与键」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheKey {
    pub fingerprint: Fingerprint,
    pub algo_version: u32,
}

impl CacheKey {
    pub const fn new(fingerprint: Fingerprint, algo_version: u32) -> CacheKey {
        CacheKey { fingerprint, algo_version }
    }

    /// **错误口径**：仅按指纹（仅供反例/测试，勿用于生产命中判定）。
    pub const fn fingerprint_only(fingerprint: Fingerprint) -> CacheKey {
        CacheKey { fingerprint, algo_version: 0 }
    }

    /// 键的展示形（十六进制，带版本前缀）。
    pub fn render(&self) -> String {
        let mut s = String::from("v");
        s.push_str(&self.algo_version.to_string());
        s.push(':');
        let mut i = 0usize;
        while i < FINGERPRINT_BYTES {
            let b = self.fingerprint.0[i];
            s.push_str(&format!("{:02x}", b));
            i += 1;
        }
        s
    }
}

/// 单文件分析结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoudnessReading {
    /// 集成响度（LUFS）。
    pub integrated_lufs: f32,
    /// True Peak（dBTP）。
    pub true_peak_dbtp: f32,
    /// 参与门限积分的块数（`0` 表示整片静音）。
    pub gated_blocks: u32,
}

impl LoudnessReading {
    pub fn new(integrated_lufs: f32, true_peak_dbtp: f32, gated_blocks: u32) -> LoudnessReading {
        LoudnessReading { integrated_lufs, true_peak_dbtp, gated_blocks }
    }

    /// 是否整片静音（门限下无块参与 ⇒ LUFS 无定义，取下界口径）。
    pub fn is_silent(&self) -> bool {
        self.gated_blocks == 0
    }
}

/// 分析缓存（**纯函数记忆**，设计要点二）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LoudnessCache {
    entries: Vec<(CacheKey, LoudnessReading)>,
    hits: u32,
    misses: u32,
}

impl LoudnessCache {
    pub fn new() -> LoudnessCache {
        LoudnessCache { entries: Vec::new(), hits: 0, misses: 0 }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn hits(&self) -> u32 {
        self.hits
    }

    pub fn misses(&self) -> u32 {
        self.misses
    }

    /// O(条目数) 查表；表极小且命中判定**纯由键决定**（与插入历史无关）。
    pub fn lookup(&self, key: CacheKey) -> Option<LoudnessReading> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].0 == key {
                return Some(self.entries[i].1);
            }
            i += 1;
        }
        None
    }

    pub fn insert(&mut self, key: CacheKey, reading: LoudnessReading) {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].0 == key {
                self.entries[i].1 = reading;
                return;
            }
            i += 1;
        }
        self.entries.push((key, reading));
    }
}

// ---------------------------------------------------------------------------
// 四、增益计算
// ---------------------------------------------------------------------------

/// 增益应用结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GainResult {
    /// 响度增益（dB）。
    pub gain_db: f32,
    /// True Peak 限幅增益（dB），非正。
    pub ceiling_trim_db: f32,
    /// 是否触发限幅。
    pub limited: bool,
    /// 是否整片静音（无增益可算）。
    pub silent: bool,
}

/// 按目标计算增益（响度对齐 + True Peak 限幅）。
///
/// 两段**分开**而非合并成一个数：响度对齐决定"听起来多响"，
/// 限幅决定"会不会削波"，二者处置不同（前者调混音，后者调限幅器）。
/// 合并成一个 gain 会让报告说不清到底做了哪件事。
pub fn compute_gain(reading: &LoudnessReading, target: &LoudnessTarget) -> GainResult {
    if reading.is_silent() {
        // 静音不做增益：把 −70 抬到 −14 会把底噪放大 56 dB。
        return GainResult { gain_db: 0.0, ceiling_trim_db: 0.0, limited: false, silent: true };
    }
    let raw = target.target_lufs - reading.integrated_lufs;
    let gain_db = clamp_gain(raw);
    // 限幅后峰值（dBTP）。
    let after = reading.true_peak_dbtp + gain_db;
    let trim_db = if after > target.ceiling_dbtp {
        target.ceiling_dbtp - after
    } else {
        0.0
    };
    GainResult {
        gain_db,
        ceiling_trim_db: trim_db,
        limited: trim_db < 0.0,
        silent: false,
    }
}

fn clamp_gain(g: f32) -> f32 {
    if g.is_nan() {
        return 0.0;
    }
    if g > GAIN_MAX_DB {
        return GAIN_MAX_DB;
    }
    if g < GAIN_MIN_DB {
        return GAIN_MIN_DB;
    }
    g
}

// ---------------------------------------------------------------------------
// 五、媒体库条目与增量
// ---------------------------------------------------------------------------

/// 媒体库一个文件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryFile {
    pub path: String,
    pub fingerprint: Fingerprint,
}

impl LibraryFile {
    pub fn new(path: &str, fingerprint: Fingerprint) -> LibraryFile {
        LibraryFile { path: String::from(path), fingerprint }
    }
}

/// 扫描原因（设计要点三：全量**有且只有**两个触发条件）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanReason {
    /// 首次全量。
    FullFirst,
    /// 算法升级后全量失效重扫。
    FullAlgoBump,
    /// 增量（新增/修改文件）。
    Incremental,
}

impl ScanReason {
    pub fn is_full(self) -> bool {
        matches!(self, ScanReason::FullFirst | ScanReason::FullAlgoBump)
    }

    pub fn name(self) -> &'static str {
        match self {
            ScanReason::FullFirst => "full_first",
            ScanReason::FullAlgoBump => "full_algo_bump",
            ScanReason::Incremental => "incremental",
        }
    }
}

/// 库变更事件（锚点「库变更事件（新增/修改文件入扫描队列」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LibEvent {
    /// 新增文件。
    Added,
    /// 修改文件（**必须与新增同等对待**——设计要点三）。
    Modified,
    /// 删除文件（不需分析，但影响报告口径）。
    Removed,
}

impl LibEvent {
    /// 是否需要入分析队列。删除**不入队**（没有内容可测），
    /// 但它要清缓存——见 [`LoudnessService::apply_event`]。
    pub fn needs_analysis(self) -> bool {
        matches!(self, LibEvent::Added | LibEvent::Modified)
    }

    pub fn name(self) -> &'static str {
        match self {
            LibEvent::Added => "added",
            LibEvent::Modified => "modified",
            LibEvent::Removed => "removed",
        }
    }
}

/// 扫描队列项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanTask {
    pub path: String,
    pub fingerprint: Fingerprint,
}

/// 一轮扫描的结果。
#[derive(Clone, Debug, PartialEq)]
pub struct ScanReport {
    pub reason: ScanReason,
    /// 本轮送入分析的文件数。
    pub analyzed: u32,
    /// 本轮命中缓存跳过的文件数。
    pub cache_hits: u32,
    /// 本轮待测文件总数。
    pub queued: u32,
    /// 各文件读数（按 path 序）。
    pub readings: Vec<(String, LoudnessReading)>,
}

// ---------------------------------------------------------------------------
// 六、服务本体
// ---------------------------------------------------------------------------

/// 引擎级响度归一服务。
#[derive(Clone, Debug, PartialEq)]
pub struct LoudnessService {
    pub table: TargetTable,
    pub cache: LoudnessCache,
    /// 媒体库当前清单。
    pub library: Vec<LibraryFile>,
    /// 最近一轮全量时的算法版本（用于判定是否需升级全量）。
    last_full_version: u32,
    /// 是否已做过首次全量。
    ever_full: bool,
}

/// 分析器：由调用方注入的测量函数（真测量在解码侧，本服务只调度）。
///
/// 用闭包而非trait 对象：内核面无 dyn 分配，且本域只需要"给定指纹
/// 返回读数"这一个动作。
pub type MeasureFn<'a> = &'a dyn Fn(&LibraryFile) -> LoudnessReading;

impl LoudnessService {
    pub fn new(table: TargetTable) -> LoudnessService {
        LoudnessService {
            table,
            cache: LoudnessCache::new(),
            library: Vec::new(),
            last_full_version: 0,
            ever_full: false,
        }
    }

    /// 构造：内置预设表。
    pub fn with_builtin_table() -> LoudnessService {
        LoudnessService::new(TargetTable::builtin())
    }

    pub fn library_len(&self) -> usize {
        self.library.len()
    }

    pub fn last_full_version(&self) -> u32 {
        self.last_full_version
    }

    pub fn ever_full(&self) -> bool {
        self.ever_full
    }

    /// 规划一轮扫描：返回扫描原因与**待实测任务集**。
    ///
    /// **调度决策在此，不在调用方**（锚点「批量扫描调度」100 行的落点）：
    /// 调用方只说"我要扫"，由服务判定走全量还是增量——否则每个调用点
    /// 都要重写一遍"首次/升级/增量"三分支，必然出现某个调用点漏判。
    ///
    /// 队列**只含真正需要实测的文件**（缓存命中项不进队列），
    /// 故"免除的文件数"由 [`plan_scan_ex`] 一并返回——
    /// 若让 `run_scan` 靠遍历队列来统计命中数，命中项根本没进队列，
    /// `cache_hits` 将恒为0，"缓存省了多少"这条报表数据就丢了。
    pub fn plan_scan(&self, algo_version: u32) -> (ScanReason, Vec<ScanTask>) {
        let (r, q, _) = self.plan_scan_ex(algo_version);
        (r, q)
    }

    /// 同 [`plan_scan`]，并额外返回「因缓存命中而免除的文件数」。
    pub fn plan_scan_ex(&self, algo_version: u32) -> (ScanReason, Vec<ScanTask>, u32) {
        let reason = if !self.ever_full {
            ScanReason::FullFirst
        } else if self.last_full_version != algo_version {
            ScanReason::FullAlgoBump
        } else {
            ScanReason::Incremental
        };
        let mut tasks: Vec<ScanTask> = Vec::new();
        let mut skipped_by_cache: u32 = 0;
        let mut i = 0usize;
        while i < self.library.len() {
            let key = CacheKey::new(self.library[i].fingerprint, algo_version);
            if reason.is_full() || self.cache.lookup(key).is_none() {
                // 全量时**不看缓存**（算法升级/首次就是来重测的）；
                // 增量时只排未命中项。
                tasks.push(ScanTask {
                    path: self.library[i].path.clone(),
                    fingerprint: self.library[i].fingerprint,
                });
            } else {
                skipped_by_cache = skipped_by_cache.saturating_add(1);
            }
            i += 1;
        }
        (reason, tasks, skipped_by_cache)
    }

    /// 执行一轮扫描（`measure` 提供真实测量）。
    pub fn run_scan(
        &mut self,
        algo_version: u32,
        measure: MeasureFn<'_>,
    ) -> ScanReport {
        let (reason, tasks, skipped) = self.plan_scan_ex(algo_version);
        let queued = tasks.len() as u32;
        let mut analyzed = 0u32;
        let mut cache_hits = skipped;
        let mut readings: Vec<(String, LoudnessReading)> = Vec::new();

        let mut i = 0usize;
        while i < tasks.len() {
            let t = &tasks[i];
            let key = CacheKey::new(t.fingerprint, algo_version);
            // 队列里理论上都是未命中项；仍做一次查表兜底——
            // 万一同一批里有两个文件指纹相同，第二个会命中第一个的结果，
            // 此时**不该**再调测量（那正是缓存要省下的重复扫描）。
            if let Some(cached) = self.cache.lookup(key) {
                cache_hits = cache_hits.saturating_add(1);
                readings.push((t.path.clone(), cached));
                i += 1;
                continue;
            }
            // 未命中 ⇒ 真测（这才是"缓存避免重复扫描"的兑现点）。
            let f = LibraryFile::new(&t.path, t.fingerprint);
            let r = measure(&f);
            self.cache.insert(key, r);
            analyzed = analyzed.saturating_add(1);
            readings.push((t.path.clone(), r));
            i += 1;
        }

        if reason.is_full() {
            self.last_full_version = algo_version;
            self.ever_full = true;
        }

        // 命中项也要进读数表，否则报告会漏掉被缓存的文件。
        if cache_hits > 0 && reason.is_full() == false {
            let mut j = 0usize;
            while j < self.library.len() {
                let key = CacheKey::new(self.library[j].fingerprint, algo_version);
                if let Some(r) = self.cache.lookup(key) {
                    let already = readings.iter().any(|(p, _)| *p == self.library[j].path);
                    if !already {
                        readings.push((self.library[j].path.clone(), r));
                    }
                }
                j += 1;
            }
        }

        ScanReport { reason, analyzed, cache_hits, queued, readings }
    }

    /// 库变更事件入队（锚点「库变更事件（新增/修改文件入扫描队列」）。
    ///
    /// 语义要点：
    /// - 新增/修改 ⇒ 进库；**修改要作废旧指纹缓存**（改过的文件
    ///   换了内容，新指纹本就不命中；但同指纹的旧读数必须清，
    ///   否则「改内容但哈希未变」的极端情形会拿到旧值）。
    /// - 删除 ⇒ 出库并清缓存，且**不入分析队列**。
    pub fn apply_event(&mut self, event: LibEvent, file: &LibraryFile, algo_version: u32) {
        match event {
            LibEvent::Added | LibEvent::Modified => {
                // 同路径替换 ⇒ 先移除旧行再插入，保持清单无重复路径。
                self.remove_path(&file.path);
                self.library.push(file.clone());
                if event == LibEvent::Modified {
                    self.evict(file.fingerprint, algo_version);
                }
            }
            LibEvent::Removed => {
                self.remove_path(&file.path);
                self.evict(file.fingerprint, algo_version);
            }
        }
    }

    fn remove_path(&mut self, path: &str) {
        let mut i = 0usize;
        while i < self.library.len() {
            if self.library[i].path == path {
                self.library.remove(i);
                return;
            }
            i += 1;
        }
    }

    fn evict(&mut self, fp: Fingerprint, algo_version: u32) {
        let key = CacheKey::new(fp, algo_version);
        let mut i = 0usize;
        while i < self.cache.entries.len() {
            if self.cache.entries[i].0 == key {
                self.cache.entries.remove(i);
                return;
            }
            i += 1;
        }
    }

    /// 缓存命中查询（计数进出，报表用）。
    pub fn probe_cache(&mut self, fp: Fingerprint, algo_version: u32) -> Option<LoudnessReading> {
        let key = CacheKey::new(fp, algo_version);
        let r = self.cache.lookup(key);
        if r.is_some() {
            self.cache.hits = self.cache.hits.saturating_add(1);
        } else {
            self.cache.misses = self.cache.misses.saturating_add(1);
        }
        r
    }
}

// ---------------------------------------------------------------------------
// 七、报告
// ---------------------------------------------------------------------------

/// 报告元信息（设计要点五：报告须自解释）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportMeta {
    pub preset_id: String,
    pub algo_version: u32,
    pub total_files: u32,
    pub silent_files: u32,
}

impl ReportMeta {
    pub fn new(preset_id: &str, algo_version: u32, total: u32, silent: u32) -> ReportMeta {
        ReportMeta {
            preset_id: String::from(preset_id),
            algo_version,
            total_files: total,
            silent_files: silent,
        }
    }
}

/// 一个文件的报告条目。
#[derive(Clone, Debug, PartialEq)]
pub struct ReportEntry {
    pub path: String,
    pub integrated_lufs: f32,
    pub true_peak_dbtp: f32,
    pub gain_db: f32,
    pub limited: bool,
    pub silent: bool,
}

/// 全库响度报告。
#[derive(Clone, Debug, PartialEq)]
pub struct LoudnessReport {
    pub meta: ReportMeta,
    pub entries: Vec<ReportEntry>,
}

impl LoudnessReport {
    /// 导出为自解释文本（首行即口径头）。
    pub fn export(&self) -> String {
        let mut s = String::from("# loudness report preset=");
        s.push_str(&self.meta.preset_id);
        s.push_str(" algo=v");
        s.push_str(&self.meta.algo_version.to_string());
        s.push_str(" gate=");
        s.push_str(&SILENCE_GATE_LUFS.to_string());
        s.push_str(" files=");
        s.push_str(&self.meta.total_files.to_string());
        s.push_str(" silent=");
        s.push_str(&self.meta.silent_files.to_string());
        let mut i = 0usize;
        while i < self.entries.len() {
            s.push_str("\n");
            s.push_str(&self.entries[i].path);
            s.push('|');
            s.push_str(&self.entries[i].integrated_lufs.to_string());
            s.push('|');
            s.push_str(&self.entries[i].true_peak_dbtp.to_string());
            s.push('|');
            s.push_str(&self.entries[i].gain_db.to_string());
            if self.entries[i].limited {
                s.push_str("|LIMIT");
            }
            if self.entries[i].silent {
                s.push_str("|SILENT");
            }
            i += 1;
        }
        s
    }

    /// 正文行数（不含口径头）。
    pub fn body_lines(&self) -> usize {
        self.entries.len()
    }

    /// 超标条目数（True Peak 触限幅）——治理清单用。
    pub fn limited_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].limited {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }
}

/// 按预设生成全库报告。
///
/// `preset_id` 查不到时返回 `None` 而不是"用默认值悄悄算"——
/// 报告的每一行都建立在预设之上，预设错了报告整张作废。
pub fn build_report(
    readings: &[(String, LoudnessReading)],
    table: &TargetTable,
    preset_id: &str,
    algo_version: u32,
) -> Option<LoudnessReport> {
    let target = table.get(preset_id)?;
    let mut entries: Vec<ReportEntry> = Vec::new();
    let mut silent = 0u32;
    let mut i = 0usize;
    while i < readings.len() {
        let r = &readings[i].1;
        let g = compute_gain(r, &target);
        if g.silent {
            silent = silent.saturating_add(1);
        }
        entries.push(ReportEntry {
            path: readings[i].0.clone(),
            integrated_lufs: r.integrated_lufs,
            true_peak_dbtp: r.true_peak_dbtp,
            gain_db: g.gain_db,
            limited: g.limited,
            silent: g.silent,
        });
        i += 1;
    }
    Some(LoudnessReport {
        meta: ReportMeta::new(preset_id, algo_version, readings.len() as u32, silent),
        entries,
    })
}

// ---------------------------------------------------------------------------
// 八、诊断输出（读屏可达）
// ---------------------------------------------------------------------------

/// 诊断行（中英双语聚合）。
pub fn diagnostic_lines(svc: &LoudnessService, last: &ScanReport) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();

    let mut s1 = String::from("响度归一/loudness normalize: 库文件/library ");
    s1.push_str(&svc.library_len().to_string());
    s1.push_str(" 预设数/presets ");
    s1.push_str(&svc.table.len().to_string());
    s1.push_str(" 算法版本/algo v");
    s1.push_str(&ANALYSIS_ALGO_VERSION.to_string());
    v.push(s1);

    let mut s2 = String::from("  上轮扫描/last scan 原因/reason ");
    s2.push_str(last.reason.name());
    s2.push_str(" 排队/queued ");
    s2.push_str(&last.queued.to_string());
    s2.push_str(" 实测/analyzed ");
    s2.push_str(&last.analyzed.to_string());
    s2.push_str(" 缓存命中/cache-hits ");
    s2.push_str(&last.cache_hits.to_string());
    v.push(s2);

    let mut s3 = String::from("  缓存/cache 条目/entries ");
    s3.push_str(&svc.cache.len().to_string());
    s3.push_str(" 命中/hits ");
    s3.push_str(&svc.cache.hits().to_string());
    s3.push_str(" 未命中/misses ");
    s3.push_str(&svc.cache.misses().to_string());
    s3.push_str(" （键含算法版本，升级即失效重扫/key carries algo version）");
    v.push(s3);

    v
}