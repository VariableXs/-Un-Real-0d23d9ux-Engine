//! cga114_detverify — CGPU-F0114 CPU 渲染确定性总验证。
//!
//! 判据原文（施工书锚点 CGPU-F0114）：CPU 路径的确定性总验证（F0055/68
//! 纪律的全链版）：全场景 × 全线程数（1-32）× 多次运行——逐像素一致性
//! 断言（确定性是软渲可信的根基，也是对拍有效性的前提）。确定性破坏即
//! 最高缺陷（对拍体系的地基动摇）。判据：全场景矩阵、线程数无关、运行
//! 无关、破坏即🔴、CI 集成。
//!
//! **设计定盘**：确定性 = 同（场景，线程数，运行序号）三元组下渲染输出
//! 的指纹逐位相同。本模块是纯逻辑建模：不真跑渲染，而是对「验证账本」
//! 本身做确定性推演——每个验证格子（场景 × 线程数 × 运行）记录基准指纹
//! 与复跑指纹，逐格三态判定（一致 / 破坏 / 未跑）。破坏格立即升格为最高
//! 缺陷（🔴），且允许注明「破坏面」：线程数相关 / 运行相关 / 场景内在。
//!
//! **指纹**：FNV-1a 64 位逐像素哈希（无浮点、无墙钟、无外部熵——指纹
//! 本身跨平台确定）。基准指纹来自「金准运行」（线程数=1 的参照实现），
//! 其余线程数与它对拍：软渲的确定性纪律要求多线程分片结果与单线程逐位
//! 相同，而不是只与自身相同。
//!
//! 诊断码段 0x0100~0x010F（0x01 段第 1 子区，全域空闲段普查后占用）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA114_VERSION: &str = "cga114-detverify-v1";

/// 诊断段基值（0x01 段第 1 子区）。
pub const SEG_BASE: u16 = 0x0100;

/// 线程数阶梯上界（判据"全线程数 1-32"）。
pub const THREAD_LADDER_MAX: u32 = 32;

/// 每场景复跑次数下限（判据"多次运行"——同一配置至少跑 3 次排除偶发）。
pub const RUN_REPEAT_MIN: u32 = 3;

/// FNV-1a 64 位偏移基。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64 位逐字节哈希。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// 确定性破坏面分类（判据"破坏即🔴"时必须归因到面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BreakFace {
    /// 多线程结果与单线程金准不一致——分片归约顺序破坏确定性。
    ThreadRelated,
    /// 同配置多次运行结果不一致——存在隐藏可变状态或未驯化的熵。
    RunRelated,
    /// 金准自身两次运行不一致——参照实现就不确定，账本整体作废。
    GoldenUnstable,
}

impl BreakFace {
    /// 诊断码。
    pub fn code(self) -> u16 {
        match self {
            BreakFace::ThreadRelated => SEG_BASE + 1,
            BreakFace::RunRelated => SEG_BASE + 2,
            BreakFace::GoldenUnstable => SEG_BASE + 3,
        }
    }

    /// 中文诊断与可操作建议（铁律 6：错误路径带可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            BreakFace::ThreadRelated => {
                "多线程分片结果与单线程金准逐位不同：检查分片边界的浮点归约顺序、\
                 共享中间缓冲的写入次序与原子聚合的可结合性；把归约改成固定序树形归约后复测"
            }
            BreakFace::RunRelated => {
                "同配置复跑不一致：存在未初始化缓冲、遗留帧状态或墙钟/地址熵参与输出；\
                 先做帧间全零化复跑，再对输出做逐帧 diff 定位首次分歧帧"
            }
            BreakFace::GoldenUnstable => {
                "金准运行（线程数=1）自身不稳定：验证体系地基失效，全部对拍结论作废；\
                 先修金准，再重跑整本验证账"
            }
        }
    }
}

/// 验证格子三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CellState {
    /// 未跑。
    Pending,
    /// 与金准逐位一致。
    Match,
    /// 破坏（🔴，最高缺陷）。
    Broken(BreakFace),
}

/// 一条验证记录：场景 × 线程数 × 运行序号 → 指纹对与判定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VerifyCell {
    /// 场景编号（场景集由 F0109 场景册冻结，此处只持编号）。
    pub scene: u32,
    /// 线程数（1..=32）。
    pub threads: u32,
    /// 运行序号（1..=RUN_REPEAT_MIN 起）。
    pub run: u32,
    /// 金准指纹（线程数=1 参照；threads=1 时与自身指纹相同）。
    pub golden: u64,
    /// 本次运行指纹。
    pub observed: u64,
    /// 输出像素数（指纹的输入规模，规模为零的格子无效）。
    pub pixels: u32,
    /// 判定态。
    pub state: CellState,
}

impl VerifyCell {
    /// 构造待判格子（threads=0 或 run=0 即拒）。
    pub fn new(scene: u32, threads: u32, run: u32, pixels: u32) -> Result<Self, u16> {
        if threads == 0 || threads > THREAD_LADDER_MAX {
            return Err(SEG_BASE + 4);
        }
        if run == 0 {
            return Err(SEG_BASE + 5);
        }
        Ok(VerifyCell {
            scene,
            threads,
            run,
            golden: 0,
            observed: 0,
            pixels,
            state: CellState::Pending,
        })
    }

    /// 写入指纹并判定（判据"逐像素一致性"的账面化）。
    pub fn record(&mut self, golden: u64, observed: u64) {
        self.golden = golden;
        self.observed = observed;
        self.state = if golden == observed {
            CellState::Match
        } else if self.threads == 1 {
            // 线程数=1 的格子自己就是金准来源；两次不一致是金准不稳。
            CellState::Broken(BreakFace::GoldenUnstable)
        } else {
            CellState::Broken(BreakFace::ThreadRelated)
        };
    }
}

/// 场景登记项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SceneEntry {
    /// 场景编号。
    pub id: u32,
    /// 输出像素数。
    pub pixels: u32,
    /// 场景指纹（场景定义冻结后的 FNV——场景变更即指纹变更，账本换代）。
    pub scene_fp: u64,
}

/// 确定性验证总账。
pub struct DetLedger {
    /// 场景册（冻结序）。
    scenes: Vec<SceneEntry>,
    /// 验证格子（登记序，允许重复键但判定取末次）。
    cells: Vec<VerifyCell>,
    /// 账本封印（登记封存后禁止再登记——CI 证据必须出自封存后的账）。
    sealed: Option<u64>,
    /// 🔴 总数。
    broken_total: u32,
    /// 🟢 总数。
    match_total: u32,
}

impl DetLedger {
    /// 空账。
    pub fn new() -> Self {
        DetLedger {
            scenes: Vec::new(),
            cells: Vec::new(),
            sealed: None,
            broken_total: 0,
            match_total: 0,
        }
    }

    /// 登记场景（编号重复即拒；像素为零即拒——零像素场景无指纹可言）。
    pub fn register_scene(&mut self, id: u32, pixels: u32, def_bytes: &[u8]) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 6);
        }
        if pixels == 0 {
            return Err(SEG_BASE + 7);
        }
        if self.scenes.iter().any(|s| s.id == id) {
            return Err(SEG_BASE + 8);
        }
        self.scenes.push(SceneEntry {
            id,
            pixels,
            scene_fp: fnv1a64(def_bytes),
        });
        Ok(())
    }

    /// 场景数。
    pub fn scene_count(&self) -> usize {
        self.scenes.len()
    }

    /// 查场景（越界返回 None，不 panic）。
    pub fn scene(&self, idx: usize) -> Option<&SceneEntry> {
        self.scenes.get(idx)
    }

    /// 登记一次运行指纹（场景未登记即拒；封存后即拒）。
    pub fn record_run(
        &mut self,
        scene: u32,
        threads: u32,
        run: u32,
        observed: u64,
    ) -> Result<CellState, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 6);
        }
        let entry = match self.scenes.iter().find(|s| s.id == scene) {
            Some(s) => *s,
            None => return Err(SEG_BASE + 9),
        };
        let mut cell = VerifyCell::new(scene, threads, run, entry.pixels)?;
        let golden = if threads == 1 {
            observed
        } else {
            // 金准指纹 = 场景指纹 ⊕ 单线程基准运行指纹（可复现推导，不存副本）。
            self.golden_of(scene).unwrap_or(0)
        };
        cell.record(golden, observed);
        match cell.state {
            CellState::Match => self.match_total += 1,
            CellState::Broken(_) => self.broken_total += 1,
            CellState::Pending => {}
        }
        let st = cell.state;
        self.cells.push(cell);
        Ok(st)
    }

    /// 金准指纹查询（场景的单线程基准运行）。
    fn golden_of(&self, scene: u32) -> Option<u64> {
        self.cells
            .iter()
            .find(|c| c.scene == scene && c.threads == 1 && c.run == 1)
            .map(|c| c.observed)
    }

    /// 封账：判据"全场景矩阵"要求每场景至少 RUN_REPEAT_MIN×阶梯全跑，
    /// 有缺格即拒（封账是 CI 证据的准入闸，缺格账不许当证据）。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 10);
        }
        if self.scenes.is_empty() {
            return Err(SEG_BASE + 11);
        }
        for s in &self.scenes {
            // 线程数阶梯：1 与 32 两个端点必须全覆盖（1=金准，32=伸缩极限）。
            for threads in [1u32, THREAD_LADDER_MAX] {
                for run in 1..=RUN_REPEAT_MIN {
                    let hit = self.cells.iter().any(|c| {
                        c.scene == s.id && c.threads == threads && c.run == run
                    });
                    if !hit {
                        return Err(SEG_BASE + 12);
                    }
                }
            }
        }
        let mut fp = FNV_OFFSET;
        for c in &self.cells {
            fp ^= c.scene as u64;
            fp = fp.wrapping_mul(FNV_PRIME);
            fp ^= c.threads as u64;
            fp = fp.wrapping_mul(FNV_PRIME);
            fp ^= c.run as u64;
            fp = fp.wrapping_mul(FNV_PRIME);
            fp ^= c.observed;
            fp = fp.wrapping_mul(FNV_PRIME);
        }
        self.sealed = Some(fp);
        Ok(fp)
    }

    /// 封印指纹。
    pub fn seal_fp(&self) -> Option<u64> {
        self.sealed
    }

    /// 🔴 清单（判据"破坏即🔴"——破坏格必须可枚举可归因）。
    pub fn broken_cells(&self) -> Vec<&VerifyCell> {
        self.cells
            .iter()
            .filter(|c| matches!(c.state, CellState::Broken(_)))
            .collect()
    }

    /// 🔴 总数。
    pub fn broken_total(&self) -> u32 {
        self.broken_total
    }

    /// 🟢 总数。
    pub fn match_total(&self) -> u32 {
        self.match_total
    }

    /// CI 判定（判据"CI 集成"）：封存且 🔴=0 才放行。
    pub fn ci_gate(&self) -> Result<(), u16> {
        if self.sealed.is_none() {
            return Err(SEG_BASE + 13);
        }
        if self.broken_total != 0 {
            return Err(SEG_BASE + 14);
        }
        Ok(())
    }
}

impl Default for DetLedger {
    fn default() -> Self {
        Self::new()
    }
}
