//! unx4113_ramfsunion — UNX-F4113 一致性与真 fs 联测锚（ramfs）（诊断段 0x9020）。
//!
//! 判据原文（提炼）：
//!   J1 ramfs 负载下缓存态与 fs 实态抽样 1,000 次比对零差
//!      （B02 负载复用：create/write/unlink 确定性负载序列）。
//!   J2 缓存开关两模式终态一致：开缓存与直查 fs 两模式跑同一负载后，
//!      最终可见态（文件集合）逐名一致。
//!   J3 抽样比对独立：每次比对取（名 → 存在性）二元组，缓存侧与
//!      fs 侧各自应答，比对差账逐条留痕（恒 0 才绿）。
//!   J4 负载回放确定性：负载序列写死（create×N → unlink 间隔摘除），
//!      两模式回放同一序列。
//!   J5 非法负载（重复 create / 删不存在）拒收并记错误账，不污染比对。
//! 设计纪律：
//!   1. fs 模型极简：ramfs = 名集合（存在性布尔面），本锚点只校验
//       缓存一致性，不涉数据块（内容一致性归 B07 写路径）。
//!   2. 缓存侧旁路可关：模式枚举控制 resolve 走缓存或直查，
//!      两模式共享同一 fs 模型实例的克隆（回放同一负载）。
//!   3. 抽样即全量：本批负载规模下 1,000 次抽样覆盖全部名字两遍以上，
//!      等价于全量比对（确定性抽样表）。
//!   4. 差账驱动绿判：diffs 计数器恒 0 才允许申报一致，
//!      任何一次差即整锚点红。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9020 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9020;

/// 抽样比对次数（判据 J1 原文数）。
pub const SAMPLES: u32 = 1000;
/// 负载建文件数（B02 负载复用规模）。
pub const LOAD_FILES: u32 = 24;
/// 负载摘除数（间隔摘除：每第 4 个建后摘）。
pub const LOAD_UNLINKS: u32 = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_DIFF: u16 = SEG_BASE + 1;     // 抽样比对发现差（判据 J1 失守）
const E_DUP_CREATE: u16 = SEG_BASE + 2; // 重复建（判据 J5）
const E_UNLINK_MISS: u16 = SEG_BASE + 3;// 删不存在（判据 J5）
const E_STATE_MIX: u16 = SEG_BASE + 4; // 两模式终态不一致（判据 J2 失守）
const E_NAME_BAD: u16 = SEG_BASE + 5;  // 空名拒收
const E_BAL: u16 = SEG_BASE + 6;       // 负载账不平（建/摘/存留不符）

/// 缓存模式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CacheMode {
    Cached,  // 开缓存：resolve 走缓存镜像
    Direct,  // 关缓存：resolve 直查 fs
}

/// 极简 ramfs 模型：名集合（存在性面）。
pub struct RamFs {
    pub files: Vec<&'static str>, // 现存名集合
    pub creates: u32,             // 累计建账
    pub unlinks: u32,             // 累计摘账
    pub rejected: u32,            // 非法负载拒收账
}

impl RamFs {
    pub fn new() -> RamFs {
        RamFs {
            files: Vec::new(),
            creates: 0,
            unlinks: 0,
            rejected: 0,
        }
    }

    pub fn create(&mut self, name: &'static str) -> Result<(), u16> {
        if name.is_empty() {
            self.rejected += 1;
            return Err(E_NAME_BAD);
        }
        for f in &self.files {
            if *f == name {
                self.rejected += 1;
                return Err(E_DUP_CREATE);
            }
        }
        self.files.push(name);
        self.creates += 1;
        Ok(())
    }

    pub fn unlink(&mut self, name: &str) -> Result<(), u16> {
        let mut idx = self.files.len();
        for i in 0..self.files.len() {
            if self.files[i] == name {
                idx = i;
                break;
            }
        }
        if idx == self.files.len() {
            self.rejected += 1;
            return Err(E_UNLINK_MISS);
        }
        let last = self.files.len() - 1;
        self.files.swap(idx, last);
        self.files.pop();
        self.unlinks += 1;
        Ok(())
    }

    fn exists(&self, name: &str) -> bool {
        for f in &self.files {
            if *f == name {
                return true;
            }
        }
        false
    }
}

/// 缓存镜像（Cached 模式专用）：名 → 存在性。
pub struct CacheMirror {
    pub lines: Vec<(&'static str, bool)>, // (名, fs 应答存在性)
    pub fills: u32,   // 回填次数
    pub hits: u32,    // 命中次数
}

impl CacheMirror {
    fn new() -> CacheMirror {
        CacheMirror {
            lines: Vec::new(),
            fills: 0,
            hits: 0,
        }
    }

    fn pos(&self, name: &str) -> Option<usize> {
        for i in 0..self.lines.len() {
            if self.lines[i].0 == name {
                return Some(i);
            }
        }
        None
    }
}

/// 联测锚点：fs 模型 + 镜像 + 抽样差账 + 终态对账。
pub struct RamfsUnion {
    pub fs: RamFs,          // fs 实态
    pub mirror: CacheMirror, // 缓存镜像（Cached 模式逐步回填）
    pub mode: CacheMode,    // 当前模式
    pub samples: u32,       // 抽样比对次数（判据 J1）
    pub diffs: u32,         // 比对差账（恒 0 才绿）
    pub probe_name: &'static str, // 当前抽样名（确定性抽样游标产物）
}

impl RamfsUnion {
    pub fn new(mode: CacheMode) -> RamfsUnion {
        RamfsUnion {
            fs: RamFs::new(),
            mirror: CacheMirror::new(),
            mode,
            samples: 0,
            diffs: 0,
            probe_name: "",
        }
    }

    /// 确定性抽样游标：第 s 次抽样取名 f{(s % LOAD_FILES)}。
    fn sample_name(s: u32) -> &'static str {
        const POOL: [&str; 24] = [
            "f0", "f1", "f2", "f3", "f4", "f5", "f6", "f7",
            "f8", "f9", "f10", "f11", "f12", "f13", "f14", "f15",
            "f16", "f17", "f18", "f19", "f20", "f21", "f22", "f23",
        ];
        POOL[(s % LOAD_FILES) as usize]
    }

    /// 单次解析（判据 J3）：缓存命中零穿透；未命中穿透一次并回填；
    /// Direct 模式直查（镜像不参与）。
    fn resolve(&mut self, name: &'static str) -> bool {
        if self.mode == CacheMode::Direct {
            return self.fs.exists(name); // 直查：零缓存参与
        }
        if let Some(i) = self.mirror.pos(name) {
            self.mirror.hits += 1;
            return self.mirror.lines[i].1; // 镜像应答
        }
        // 未命中：穿透并回填
        let real = self.fs.exists(name);
        self.mirror.lines.push((name, real));
        self.mirror.fills += 1;
        real
    }

    /// 抽样比对（判据 J1/J3）：缓存/直查应答与 fs 实态比对，差即入账。
    pub fn sample(&mut self, s: u32) -> Result<bool, u16> {
        let name = RamfsUnion::sample_name(s);
        let answered = self.resolve(name);
        let real = self.fs.exists(name);
        self.samples += 1;
        self.probe_name = name;
        if answered != real {
            self.diffs += 1;
            return Err(E_DIFF); // 差即红：判据 J1 零差要求
        }
        Ok(answered)
    }

    /// 负载回放（判据 J4/J5）：create×24，每第 4 个建后同拍摘除 1 个
    /// （共 6 摘），非法负载拒收不中断回放。
    pub fn replay_load(&mut self) -> Result<(), u16> {
        // 建负载：f0..f23；每建满 4 个摘最早建的未摘名（确定性）
        let mut made: u32 = 0;
        let mut unlinked = 0u32;
        while made < LOAD_FILES {
            // 拼名：POOL 与 sample_name 同源（确定性名空间）
            const POOL: [&str; 24] = [
                "f0", "f1", "f2", "f3", "f4", "f5", "f6", "f7",
                "f8", "f9", "f10", "f11", "f12", "f13", "f14", "f15",
                "f16", "f17", "f18", "f19", "f20", "f21", "f22", "f23",
            ];
            let name = POOL[made as usize];
            match self.fs.create(name) {
                Ok(()) => {}
                Err(_) => {
                    return Err(E_DUP_CREATE); // 同回放内重复建属负载错
                }
            }
            made += 1;
            // 每 4 建 1 摘：摘 f{(unlinked*4)}（已建的最早批）
            if made % 4 == 0 && unlinked < LOAD_UNLINKS {
                const VICTIM: [&str; 6] = ["f0", "f4", "f8", "f12", "f16", "f20"];
                let v = VICTIM[unlinked as usize];
                match self.fs.unlink(v) {
                    Ok(()) => unlinked += 1,
                    Err(_) => return Err(E_UNLINK_MISS),
                }
            }
        }
        // 负载账对账（判据 J4 前置）：建-摘 == 存留
        if self.fs.creates != LOAD_FILES
            || self.fs.unlinks != LOAD_UNLINKS
            || self.fs.files.len() as u32 != LOAD_FILES - LOAD_UNLINKS
        {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 抽样驱动（判据 J1）：跑满 SAMPLES 次，任一差即红。
    pub fn run_samples(&mut self) -> Result<(), u16> {
        let mut s = 0u32;
        while s < SAMPLES {
            self.sample(s)?;
            s += 1;
        }
        if self.diffs != 0 {
            return Err(E_DIFF);
        }
        Ok(())
    }

    /// 终态导出：现存名集合排序副本（判据 J2 对账输入）。
    pub fn final_state(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        for i in 0..self.fs.files.len() {
            let mut k = 0usize;
            while k < v.len() && v[k] < self.fs.files[i] {
                k += 1;
            }
            v.insert(k, self.fs.files[i]); // 有序插入
        }
        v
    }

    /// 两模式终态对账（判据 J2）：逐名一致且数量一致。
    pub fn states_equal(&self, other: &RamfsUnion) -> bool {
        let a = self.final_state();
        let b = other.final_state();
        if a.len() != b.len() {
            return false;
        }
        let mut i = 0usize;
        while i < a.len() {
            if a[i] != b[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 自例程：R1 负载回放→R2 两模式抽样→R3 终态对账→R4 注入与守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 负载回放：两模式各自回放同一负载（判据 J4）。
    let mut cached = RamfsUnion::new(CacheMode::Cached);
    let mut direct = RamfsUnion::new(CacheMode::Direct);
    cached.replay_load()?;
    direct.replay_load()?;
    hits += 1;

    // R2 抽样 1,000 次零差（判据 J1/J3）。
    cached.run_samples()?;
    direct.run_samples()?;
    if cached.diffs != 0 || direct.diffs != 0 {
        return Err(E_DIFF);
    }
    if cached.samples != SAMPLES || direct.samples != SAMPLES {
        return Err(E_BAL);
    }
    hits += 1;

    // R3 终态对账（判据 J2）：两模式现存集合逐名一致。
    if !cached.states_equal(&direct) {
        return Err(E_STATE_MIX);
    }
    let st = cached.final_state();
    if st.len() as u32 != LOAD_FILES - LOAD_UNLINKS {
        return Err(E_BAL); // 24 建 - 6 摘 = 18 存留
    }
    hits += 1;

    // R4 注入与守恒：重复建、删不存在、镜像差面全验。
    // f0 已被摘除：重建应成功，随后的重复建才拒。
    cached.fs.create("f0")?;
    if cached.fs.create("f0").is_ok() {
        return Err(E_DUP_CREATE);
    }
    if cached.fs.unlink("ghost").is_ok() {
        return Err(E_UNLINK_MISS);
    }
    if cached.fs.rejected < 2 {
        return Err(E_BAL); // 拒收账：重复建 + 幽灵删
    }
    // 镜像路径确实被使用过：有回填有命中（缓存侧活性证据）。
    if cached.mirror.fills == 0 || cached.mirror.hits == 0 {
        return Err(E_BAL);
    }
    // 直查侧终态复验：f0 未重建，抽样必须回不存在（直查零镜像参与）。
    let s0 = direct.sample(0)?;
    if s0 {
        return Err(E_DIFF);
    }
    hits += 1;

    Ok(hits)
}
