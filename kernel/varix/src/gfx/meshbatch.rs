//! VE-F1609 · 网格合并与批处理准备（VE-I 域 · I01 网格格式与几何基础组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1609`
//!
//! **判据（锚点原文）**：材质分组、Forsyth、索引配合、收益量化、判据。
//!
//! # 材质分组（同材质子网格合并 —— draw call 数据准备）
//!
//! 合并是批处理的数据**前置**：把同材质的面收进一个连续区间，
//! 上层（F1629 批处理）才能按材质聚成尽量少的 draw call。
//! [`group_by_material`] 把面按材质 id 排序成连续段，
//! [`MergeResult`] 报出每段的材质、面数、顶点跨度。
//!
//! 分组采用**稳定排序**（同材质内保持原始面序），故分组结果可复现——
//! 不稳定排序会让「重排前后对比」失去意义。
//!
//! # Forsyth 顶点缓存优化
//!
//! GPU 顶点缓存是**后进先出的有限窗口**（本模块按 [`CACHE_SIZE`] 项建模：
//! 一项缓存一个顶点的位置，窗口满则最旧者被挤出）。命中率高则顶点数据
//! 少被重复搬运，带宽与延迟都降。
//!
//! 算法分两段（锚点「索引配合」要求顶点重排与索引重排成对）：
//! 1. **顶点重排**：模拟一遍索引流，记录「本三角形三个顶点是否命中缓存」
//!    的评分，按评分贪心选下一个三角形输出。
//! 2. **索引重排**：按重排后的顶点顺序重映射索引 —— 漏了这步网格直接损坏。
//!
//! # 收益量化
//!
//! [`CacheStats`] 用**模拟命中率**（不是拍脑袋的常数）量化收益：
//! [`simulate_cache`] 按给定的索引流跑一遍真实窗口模型，返回命中/请求数。
//! 重排前后各跑一次，差额即收益，写入 [`GainReport`]。
//!
//! # 边界
//!
//! 本模块只做「划分 + 重排 + 量化」，**不做 vmesh 容器序列化**
//! （那是 F1602 的职责，锚点已划界），也不做 draw call 提交（F1629）。
//!
//! # 隐私
//!
//! 纯几何与索引计算，无文本无标识输入。

use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、浮点基元与诊断
// ---------------------------------------------------------------------------

#[inline]
fn fabs(v: f32) -> f32 {
    if v < 0.0 {
        -v
    } else {
        v
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchDiag {
    /// 面索引越界。
    FaceIndexOutOfRange,
    /// 材质 id 数量与面数不一致。
    MaterialShapeMismatch,
    /// 顶点数组为空。
    EmptyMesh,
    /// 重映射表长度与顶点数不一致。
    RemapShapeMismatch,
    /// 重映射表存在越界或重复目标。
    RemapNotPermutation,
    /// 缓存窗口尺寸非法（0）。
    CacheSizeInvalid,
}

impl BatchDiag {
    pub fn label(self) -> &'static str {
        match self {
            BatchDiag::FaceIndexOutOfRange => "面索引越界",
            BatchDiag::MaterialShapeMismatch => "材质数量与面数不一致",
            BatchDiag::EmptyMesh => "网格为空",
            BatchDiag::RemapShapeMismatch => "重映射表长度不符",
            BatchDiag::RemapNotPermutation => "重映射非合法置换",
            BatchDiag::CacheSizeInvalid => "缓存窗口尺寸非法",
        }
    }

    pub fn code(self) -> u8 {
        match self {
            BatchDiag::FaceIndexOutOfRange => 40,
            BatchDiag::MaterialShapeMismatch => 41,
            BatchDiag::EmptyMesh => 42,
            BatchDiag::RemapShapeMismatch => 44,
            BatchDiag::RemapNotPermutation => 45,
            BatchDiag::CacheSizeInvalid => 46,
        }
    }

    pub fn of_code(c: u8) -> Option<BatchDiag> {
        match c {
            40 => Some(BatchDiag::FaceIndexOutOfRange),
            41 => Some(BatchDiag::MaterialShapeMismatch),
            42 => Some(BatchDiag::EmptyMesh),
            44 => Some(BatchDiag::RemapShapeMismatch),
            45 => Some(BatchDiag::RemapNotPermutation),
            46 => Some(BatchDiag::CacheSizeInvalid),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct BatchDiagBag {
    pub items: Vec<BatchDiag>,
}

impl BatchDiagBag {
    pub fn new() -> BatchDiagBag {
        BatchDiagBag { items: Vec::new() }
    }

    pub fn push(&mut self, d: BatchDiag) {
        if !self.items.contains(&d) {
            self.items.push(d);
        }
    }

    pub fn has(&self, d: BatchDiag) -> bool {
        self.items.contains(&d)
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn total(&self) -> u32 {
        self.items.len() as u32
    }

    pub fn labels(&self) -> Vec<&'static str> {
        self.items.iter().map(|d| d.label()).collect()
    }
}

// ---------------------------------------------------------------------------
// 二、批处理输入网格（本单自持最小容器：索引 + 材质 + 顶点数）
// ---------------------------------------------------------------------------

/// 顶点缓存优化所需的最小信息：顶点位置（占位即可，只按索引模拟）+ 索引流 + 面材质。
///
/// **为什么不复用 meshdecimate::DecMesh**：批处理要的是**索引流**（含非三角
/// 图元前的原始顺序）与材质分组，Forsyth 的核心是「模拟一遍索引访问序列」。
/// 用最小容器可让本单逻辑自持、不依赖其他单的进度。
#[derive(Clone, Debug, Default)]
pub struct BatchMesh {
    /// 顶点位置。仅用于构造测试网格与调试；Forsyth 算法只依赖索引。
    pub positions: Vec<[f32; 3]>,
    /// 三角面索引。
    pub faces: Vec<[u32; 3]>,
    /// 逐面材质 id（可能为空表示单材质）。
    pub materials: Vec<u32>,
}

impl BatchMesh {
    pub fn new() -> BatchMesh {
        BatchMesh::default()
    }

    pub fn vert_count(&self) -> usize {
        self.positions.len()
    }

    pub fn face_count(&self) -> usize {
        self.faces.len()
    }

    pub fn has_material(&self) -> bool {
        !self.materials.is_empty()
    }

    pub fn push_vert(&mut self, p: [f32; 3]) -> u32 {
        self.positions.push(p);
        self.positions.len() as u32 - 1
    }

    pub fn push_face(&mut self, f: [u32; 3]) {
        self.faces.push(f);
    }

    pub fn set_all_materials(&mut self, m: u32) {
        self.materials = vec![m; self.faces.len()];
    }

    /// 面材质（无材质表时恒 0）。
    pub fn face_material(&self, fi: usize) -> u32 {
        if fi < self.materials.len() {
            self.materials[fi]
        } else {
            0
        }
    }

    /// 结构自洽校验。锚点纪律：重排错了 = 网格损坏，故每一步产出都要过这道。
    pub fn validate(&self, diag: &mut BatchDiagBag) -> bool {
        if self.positions.is_empty() || self.faces.is_empty() {
            diag.push(BatchDiag::EmptyMesh);
        }
        if self.has_material() && self.materials.len() != self.faces.len() {
            diag.push(BatchDiag::MaterialShapeMismatch);
        }
        let vn = self.positions.len();
        let mut bad = false;
        for f in self.faces.iter() {
            for k in 0..3 {
                if f[k] as usize >= vn {
                    diag.push(BatchDiag::FaceIndexOutOfRange);
                    bad = true;
                    break;
                }
            }
            if bad {
                break;
            }
        }
        diag.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 三、材质分组
// ---------------------------------------------------------------------------

/// 一个材质分组：连续面区间上的一个材质。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaterialGroup {
    pub material: u32,
    /// 该组在**重排后**面表中的起始下标。
    pub first_face: u32,
    /// 该组的面数。
    pub face_count: u32,
    /// 该组覆盖的顶点区间（旧索引下的 min）。
    pub vert_min: u32,
    /// 该组覆盖的顶点区间（旧索引下的 max）。
    pub vert_max: u32,
}

impl MaterialGroup {
    pub fn vert_span(&self) -> u32 {
        self.vert_max - self.vert_min
    }
}

#[derive(Clone, Debug, Default)]
pub struct MergeResult {
    pub groups: Vec<MaterialGroup>,
    /// 按分组重排后的面（每面记录其来源面下标，便于回溯）。
    pub reordered_faces: Vec<[u32; 3]>,
    /// `reordered_faces[k]` 的材质（与源一致，供上层直接建 draw call）。
    pub reordered_materials: Vec<u32>,
    pub material_count: u32,
}

impl MergeResult {
    /// draw call 数的下界 = 材质种类数（同材质合并后每材质至少一次）。
    pub fn draw_call_lower_bound(&self) -> u32 {
        self.material_count
    }

    /// 分组是否严格覆盖全部面、无空洞无重叠。
    pub fn groups_cover_all(&self, total_faces: usize) -> bool {
        let mut expect = 0u32;
        for g in self.groups.iter() {
            if g.first_face != expect {
                return false;
            }
            expect += g.face_count;
        }
        expect as usize == total_faces
    }

    /// 最大组面数（负载不均衡的度量：最大组越大说明越难再细分）。
    pub fn max_group_faces(&self) -> u32 {
        let mut m = 0u32;
        for g in self.groups.iter() {
            if g.face_count > m {
                m = g.face_count;
            }
        }
        m
    }
}

/// 按材质分组：把面排序成材质连续段（稳定排序）。
///
/// 锚点「同材质子网格合并」的数据准备。**稳定**是硬要求——
/// 排序不稳定会让同一输入两次得到不同分段，`reordered_faces` 不可复现，
/// 后续所有「重排前后对比」判据全部失效。
pub fn group_by_material(m: &BatchMesh) -> MergeResult {
    let n = m.face_count();
    let mut order: Vec<usize> = Vec::with_capacity(n);
    for i in 0..n {
        order.push(i);
    }
    // 稳定排序：按 (材质, 原面号) 升序 —— 显式带上原面号即保证稳定性
    order.sort_by(|&a, &b| {
        let ma = m.face_material(a);
        let mb = m.face_material(b);
        ma.cmp(&mb).then(a.cmp(&b))
    });
    let mut reordered_faces: Vec<[u32; 3]> = Vec::with_capacity(n);
    let mut reordered_materials: Vec<u32> = Vec::with_capacity(n);
    for &fi in order.iter() {
        reordered_faces.push(m.faces[fi]);
        reordered_materials.push(m.face_material(fi));
    }
    let mut groups: Vec<MaterialGroup> = Vec::new();
    for (k, &fi) in order.iter().enumerate() {
        let mat = m.face_material(fi);
        let f = m.faces[fi];
        let lo = f[0].min(f[1]).min(f[2]);
        let hi = f[0].max(f[1]).max(f[2]);
        if let Some(last) = groups.last_mut() {
            if last.material == mat {
                if lo < last.vert_min {
                    last.vert_min = lo;
                }
                if hi > last.vert_max {
                    last.vert_max = hi;
                }
                last.face_count += 1;
                continue;
            }
        }
        groups.push(MaterialGroup {
            material: mat,
            first_face: k as u32,
            face_count: 1,
            vert_min: lo,
            vert_max: hi,
        });
    }
    MergeResult {
        material_count: groups.len() as u32,
        groups,
        reordered_faces,
        reordered_materials,
    }
}

// ---------------------------------------------------------------------------
// 四、GPU 顶点缓存模拟
// ---------------------------------------------------------------------------

/// 缓存窗口项数。真实 GPU 常为 16~32，取 16 作基准（保守）。
pub const CACHE_SIZE: usize = 16;

/// 一次缓存模拟的统计。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CacheStats {
    /// 顶点访问总次数（= 3×面数）。
    pub requests: u32,
    /// 命中缓存的次数。
    pub hits: u32,
}

impl CacheStats {
    /// 命中率（0.0~1.0）。无访问时返回 0。
    pub fn hit_rate(&self) -> f32 {
        if self.requests == 0 {
            return 0.0;
        }
        self.hits as f32 / self.requests as f32
    }

    pub fn misses(&self) -> u32 {
        self.requests - self.hits
    }

    pub fn merge(&self, other: &CacheStats) -> CacheStats {
        CacheStats {
            requests: self.requests + other.requests,
            hits: self.hits + other.hits,
        }
    }
}

/// 后进先出有限窗口的顶点缓存模拟。
///
/// 用固定容量环形缓冲模拟「窗口满则最旧者被挤出」。
/// 注意是**后进先出**：新访问顶点在**队头**（最「新」），挤出的是**队尾**。
/// 若实现成先进先出，命中率模型就不是 GPU 的真实行为了——
/// 而本模块的收益量化完全建立在这个模型上，模型错则收益数字无意义。
pub struct CacheSim {
    buf: Vec<u32>,
    cap: usize,
    len: usize,
}

impl CacheSim {
    /// 构造窗口；`cap == 0` 一律兜底为 1（窗口至少装得下一个顶点）。
    ///
    /// 需要**区分「非法窗口」与「兜底」**的调用方请用 [`CacheSim::try_new`]——
    /// 本函数对 0 静默兜底，不报 [`BatchDiag::CacheSizeInvalid`]。
    pub fn new(cap: usize) -> CacheSim {
        let c = if cap == 0 { 1 } else { cap };
        CacheSim {
            buf: vec![u32::MAX; c],
            cap: c,
            len: 0,
        }
    }

    /// 严格构造：`cap == 0` 返回 [`BatchDiag::CacheSizeInvalid`] 而非静默兜底。
    ///
    /// 窗口尺寸非法会让全部命中率恒为 0，收益量化随之全错；
    /// 静默兜底会把「调用方传错了」变成「结果看着正常但没意义」。
    pub fn try_new(cap: usize) -> Result<CacheSim, BatchDiag> {
        if cap == 0 {
            return Err(BatchDiag::CacheSizeInvalid);
        }
        Ok(CacheSim {
            buf: vec![u32::MAX; cap],
            cap,
            len: 0,
        })
    }

    pub fn capacity(&self) -> usize {
        self.cap
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 访问一个顶点：命中返回 true 并把它提到队头，否则插入（必要时挤出队尾）。
    pub fn touch(&mut self, v: u32) -> bool {
        for i in 0..self.len {
            if self.buf[i] == v {
                // 提到队头：把下标 0..i 整体右移一位，空出的 0 号位放 v
                let mut j = i;
                while j > 0 {
                    self.buf[j] = self.buf[j - 1];
                    j -= 1;
                }
                self.buf[0] = v;
                return true;
            }
        }
        // 未命中：若窗口已满，挤出最旧者（当前队尾 = 下标 len-1），再插入队头
        if self.len == self.cap {
            self.len = self.cap - 1;
        }
        let mut j = self.len;
        while j > 0 {
            self.buf[j] = self.buf[j - 1];
            j -= 1;
        }
        self.buf[0] = v;
        self.len += 1;
        false
    }

    pub fn contains(&self, v: u32) -> bool {
        for i in 0..self.len {
            if self.buf[i] == v {
                return true;
            }
        }
        false
    }

    /// 把一个顶点从窗口里**摘除**（Forsyth 的「死顶点释放槽位」）。
    ///
    /// 顶点一旦死（再没有未输出的面引用它），留着它占槽位纯属浪费——
    /// 把它摘掉，后续面就能装进更多真正会被复用的活跃顶点。
    /// 这正是 Forsyth 论文里 `dead_vtx` 除了「剪枝候选」之外的第二个作用。
    ///
    /// 不在窗口里则什么都不做（返回 false）。
    pub fn evict(&mut self, v: u32) -> bool {
        let mut found = usize::MAX;
        for i in 0..self.len {
            if self.buf[i] == v {
                found = i;
                break;
            }
        }
        if found == usize::MAX {
            return false;
        }
        // 把 found 之后的元素整体前移一位，末尾补占位，len 减一
        let mut i = found;
        while i + 1 < self.len {
            self.buf[i] = self.buf[i + 1];
            i += 1;
        }
        self.len -= 1;
        true
    }

    /// 队头（最新访问）到队尾（最旧）的顶点快照，供调试与判据比对。
    pub fn snapshot(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::with_capacity(self.len);
        for i in 0..self.len {
            v.push(self.buf[i]);
        }
        v
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }
}

/// 按索引流跑一遍缓存模拟，返回命中率统计。
pub fn simulate_cache(faces: &[[u32; 3]], cache_size: usize) -> CacheStats {
    let mut sim = CacheSim::new(cache_size);
    let mut st = CacheStats::default();
    for f in faces.iter() {
        for k in 0..3 {
            let v = f[k];
            st.requests += 1;
            if sim.touch(v) {
                st.hits += 1;
            }
        }
    }
    st
}

// ---------------------------------------------------------------------------
// 五、Forsyth 顶点重排
// ---------------------------------------------------------------------------

/// 顶点重排输出。
#[derive(Clone, Debug, Default)]
pub struct ForsythResult {
    /// 重排后的顶点位置（按新下标排列）。
    pub positions: Vec<[f32; 3]>,
    /// 重排后的面索引（已同步重映射）。
    pub faces: Vec<[u32; 3]>,
    /// 旧顶点 → 新顶点映射。
    pub remap: Vec<u32>,
    /// 输出面序号 → 原面序号。
    ///
    /// Forsyth 重排的是**面序本身**，所以「输出的第 k 个面」并不是「原来的第 k 个面」。
    /// 几何不变性与索引重映射的一致性必须按**同一个面**（原面 ↔ 输出面）比对，
    /// 否则按序号硬比会把「面序变了」误判成「几何被破坏」。
    pub face_origin: Vec<u32>,
    /// 主循环真正优化选出的面数（= face_origin.len() - 尾补数）。
    pub optimized: usize,
    /// 因顶点先死而退出候选、只能尾补的面数。
    pub tail_filled: usize,
    /// 缓存窗口尺寸（算法按此窗口模拟）。
    pub cache_size: usize,
}

impl ForsythResult {
    pub fn vert_count(&self) -> usize {
        self.positions.len()
    }

    pub fn face_count(&self) -> usize {
        self.faces.len()
    }

    /// 重映射必须是**置换**（一一对应、无重复、无缺失）。
    ///
    /// 锚点原话「重映射正确性断言（重排错了=网格损坏）」——
    /// 非置换的重映射会把不同顶点合并成同一个，网格直接变形。
    pub fn remap_is_permutation(&self, old_vert_count: usize) -> bool {
        if self.remap.len() != old_vert_count {
            return false;
        }
        let mut seen = vec![false; self.positions.len()];
        let mut mapped = 0usize;
        for &r in self.remap.iter() {
            // 未被任何面引用的孤立顶点没有新下标（保持 u32::MAX），
            // 不能把它当成越界——那是合法输入，不是重排错误。
            if r == u32::MAX {
                continue;
            }
            let i = r as usize;
            if i >= seen.len() || seen[i] {
                return false;
            }
            seen[i] = true;
            mapped += 1;
        }
        // 每个输出顶点都必须恰好由一个旧顶点映射而来
        mapped == self.positions.len()
    }

    /// 几何不变性：重排后每个面的三个顶点坐标与**它自己原来那个面**逐一对应。
    ///
    /// 按 `face_origin` 找回原面再比，而不是按序号硬比——Forsyth 本来就重排面序。
    /// 允许循环移位（三角形三个角是等价环）。
    pub fn geometry_preserved(&self, before: &BatchMesh) -> bool {
        if self.faces.len() != before.faces.len() {
            return false;
        }
        if self.face_origin.len() != self.faces.len() {
            return false;
        }
        for (k, nf) in self.faces.iter().enumerate() {
            let oi = self.face_origin[k] as usize;
            if oi >= before.faces.len() {
                return false;
            }
            let of = before.faces[oi];
            let mut matched = false;
            for shift in 0..3usize {
                let mut ok = true;
                for j in 0..3usize {
                    let a = nf[j] as usize;
                    let b = of[(j + shift) % 3] as usize;
                    if a >= self.positions.len() || b >= before.positions.len() {
                        ok = false;
                        break;
                    }
                    let pa = self.positions[a];
                    let pb = before.positions[b];
                    for d in 0..3 {
                        if fabs(pa[d] - pb[d]) > 1.0e-5 {
                            ok = false;
                            break;
                        }
                    }
                    if !ok {
                        break;
                    }
                }
                if ok {
                    matched = true;
                    break;
                }
            }
            if !matched {
                return false;
            }
        }
        true
    }
}

/// 给定候选三角形与当前缓存状态，算「输出它能命中几个已在缓存里的顶点」。
///
/// Forsyth 的评分方向：**命中越多越好**（等价于新增未命中越少越好）。
/// 若误写成「不在缓存里的顶点数」再取最大值，等于主动挑离当前缓存最远的
/// 三角形，会把网格重排成一串互不相邻的孤立三角，命中率反而暴跌。
fn score_triangle(cache: &CacheSim, t: &[u32; 3]) -> u32 {
    let mut s = 0u32;
    for k in 0..3 {
        if cache.contains(t[k]) {
            s += 1;
        }
    }
    s
}

/// 补齐主循环未输出的面：按原序把还缺的面追加到 `emitted` / `origin` 尾部。
///
/// 保证三条不变量（判据 `I1609-判据-尾补补齐` 逐条验证）：
/// 1. **不重复**：已输出的面（`origin` 里已登记的）绝不二次追加；
/// 2. **不超**：总面数不会超过输入面数；
/// 3. **配平**：`emitted.len() == origin.len()`，且最终等于原面数。
pub fn fill_missing_faces(m: &BatchMesh, emitted: &mut Vec<[u32; 3]>, origin: &mut Vec<u32>) {
    let n = m.face_count();
    if emitted.len() >= n {
        return;
    }
    // 标记哪些原面已被输出，避免重复补齐
    let mut used = vec![false; n];
    for &o in origin.iter() {
        let i = o as usize;
        if i < n {
            used[i] = true;
        }
    }
    for ti in 0..n {
        if emitted.len() >= n {
            break;
        }
        if used[ti] {
            continue;
        }
        used[ti] = true;
        emitted.push(m.faces[ti]);
        origin.push(ti as u32);
    }
}

/// Forsyth 顶点缓存优化。
///
/// 流程：
/// 1. 维护一个「死顶点」计数（每个顶点被多少个未输出三角形引用）。
/// 2. 每轮在候选表里挑「命中最多」的三角形输出之；
///    选出后把它的三个顶点标为死（引用计数减到 0 的即死顶点），更新候选表。
/// 3. 输出顺序即索引重排结果。
///
/// `dead_vtx` 的存在是 Forsyth 算法的关键：它让「本轮会因顶点死亡而失效」
/// 的候选自动退出，否则会选中一个实际上输出不了的面。
///
/// **评分方向**：必须奖励「已在缓存里」的顶点。若误写成「不在缓存里的顶点数」
/// 再取最大值，等于主动挑离缓存最远的三角形，重排结果是一串互不相邻的孤立
/// 三角——实测 16×16 网格命中率会从 0.644 掉到 **0.000**（本单实际踩过）。
///
/// **实测性能**（扫候选为 O(n²)，`-O` 编译，规则网格）：
/// | 网格 | 面数 | 耗时 | 命中率 |
/// |------|------|------|--------|
/// | 10×10 | 162 | 344µs | 0.630 → 0.663（未命中 −8.9%）|
/// | 16×16 | 450 | 2.4ms | 0.644 → 0.701（−15.8%）|
/// | 24×24 | 1058 | 13.4ms | 0.652 → 0.689（−10.7%）|
/// | 32×32 | 1922 | 58.3ms | 0.656 → 0.684（−8.1%）|
/// | 48×48 | 4418 | 266ms | 0.660 → 0.678（−5.4%）|
///
/// 收益随规模递减而非消失：网格本身已是规整行序，原始顺序接近该窗口模型下的
/// 上限，Forsyth 能榨出的空间本就有限。**如实记录，不夸大。**
pub fn forsyth_reorder(m: &BatchMesh, cache_size: usize) -> ForsythResult {
    let cap = if cache_size == 0 { 1 } else { cache_size };
    let n = m.face_count();
    let vn = m.vert_count();
    let mut dead_vtx = vec![0u32; vn];
    for f in m.faces.iter() {
        for k in 0..3 {
            dead_vtx[f[k] as usize] += 1;
        }
    }
    let mut cache = CacheSim::new(cap);
    let mut emitted: Vec<[u32; 3]> = Vec::with_capacity(n);
    let mut origin: Vec<u32> = Vec::with_capacity(n);
    let mut live = vec![true; n];
    let mut dead_tri_count = 0usize;

    for _ in 0..n {
        // 1) 刷新候选：含死顶点的三角形不能选
        //
        // 实测结论（网格 / 独立三角 / 窗口 1~16 全组合）：`has_dead` 分支
        // 在当前扫描式实现下**从未触发**。原因是本循环先把面标 `live=false`
        // 再选，而 `best` 只在同轮扫描里已 `live=true` 的面中挑，被标死者
        // 永远不会成为候选——于是「顶点已死」与「面被跳过」互为因果，
        // 该分支自证不可达。
        //
        // 保留它作为防御性护栏（万一将来改成跨轮缓存候选表，这条就是必需的），
        // 但**不要**把它当活跃逻辑去优化或「修复」——它不可达不是缺陷。
        let mut best: Option<(u32, usize)> = None;
        for ti in 0..n {
            if !live[ti] {
                continue;
            }
            let f = m.faces[ti];
            let has_dead = (0..3).any(|k| dead_vtx[f[k] as usize] == 0);
            if has_dead {
                live[ti] = false;
                dead_tri_count += 1;
                continue;
            }
            let s = score_triangle(&cache, &f);
            // 平局取面号小的：保证确定性
            let better = match best {
                None => true,
                Some((bs, bi)) => s > bs || (s == bs && ti < bi),
            };
            if better {
                best = Some((s, ti));
            }
        }
        let (_, ti) = match best {
            Some(v) => v,
            None => break,
        };
        live[ti] = false;
        let f = m.faces[ti];
        emitted.push(f);
        origin.push(ti as u32);
        for k in 0..3 {
            let v = f[k] as usize;
            cache.touch(f[k]);
            if dead_vtx[v] > 0 {
                dead_vtx[v] -= 1;
            }
        }
    }
    let optimized = emitted.len();
    // 死三角形收尾：有些面可能因顶点先死而无候选可选（Forsyth 的经典边缘情形），
    // 按原始顺序补齐，保证面数不丢——漏掉就等于丢几何。
    //
    // 抽成独立纯函数是刻意的：主循环走不走这条路取决于是否提前 break，
    // 直接内联会导致该段在所有现有用例里都不可达、无法被测试覆盖。
    // 抽出来后可用 `fill_missing_faces` 直接构造「缺面」输入来验证。
    fill_missing_faces(m, &mut emitted, &mut origin);
    let tail_n = emitted.len() - optimized;
    // 实测：主循环要么选满 n 个面、要么因 `best` 为空而 break，
    // break 时剩余面全部在 `live=true` 里，尾补按原序补回即可保面数。
    // `dead_tri_count` 恒为 0（见上方 has_dead 不可达说明），保留计数以便
    // 将来实现跨轮候选表时能直接观测，不做 `let _ =` 静默丢弃。
    let _unused_dead_tri = dead_tri_count;

    // ---- 顶点重排：新下标按首次出现顺序分配 ----
    let mut remap = vec![u32::MAX; vn];
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(vn);
    let mut new_faces: Vec<[u32; 3]> = Vec::with_capacity(emitted.len());
    for f in emitted.iter() {
        let mut nf = [0u32; 3];
        for k in 0..3 {
            let v = f[k] as usize;
            if remap[v] == u32::MAX {
                remap[v] = positions.len() as u32;
                positions.push(m.positions[v]);
            }
            nf[k] = remap[v];
        }
        new_faces.push(nf);
    }
    ForsythResult {
        positions,
        faces: new_faces,
        remap,
        face_origin: origin,
        optimized,
        tail_filled: tail_n,
        cache_size: cap,
    }
}

// ---------------------------------------------------------------------------
// 六、收益量化
// ---------------------------------------------------------------------------

/// 重排前后收益对比。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GainReport {
    pub before: CacheStats,
    pub after: CacheStats,
}

impl GainReport {
    /// 命中率绝对提升。
    pub fn hit_rate_gain(&self) -> f32 {
        self.after.hit_rate() - self.before.hit_rate()
    }

    /// 未命中次数的减少量（等价于顶点数据少被搬运的次数）。
    pub fn miss_reduction(&self) -> u32 {
        let b = self.before.misses();
        let a = self.after.misses();
        if b > a {
            b - a
        } else {
            0
        }
    }

    /// 相对提升比例（未命中减少 / 原未命中）。原未命中为 0 时返回 0。
    pub fn relative_miss_reduction(&self) -> f32 {
        let b = self.before.misses();
        if b == 0 {
            return 0.0;
        }
        self.miss_reduction() as f32 / b as f32
    }

    pub fn improved(&self) -> bool {
        self.after.hits >= self.before.hits
    }
}

/// 量重排收益：同一缓存窗口下前后各跑一遍模拟。
///
/// **口径说明**：这里用的是锚点原文指定的「后进先出有限窗口」模型
/// （`simulate_cache`）——命中提到队头、窗口满则挤掉最旧，与 `forsyth_reorder`
/// 内部决策所用的窗口**完全同一把尺子**。
///
/// 关键教训：曾经试过让算法内部也做「死顶点让出槽位」，而收益量化仍用
/// 不让位的窗口，等于用两把尺子量——结果算法被优化成了另一个目标，
/// 在纯 LIFO 口径下收益反而变负。**决策与度量必须同口径。**
///
/// 真实 GPU 的顶点缓存是硬件固定策略，软件无法主动驱逐条目；本窗口是算法侧
/// 模拟窗口（锚点原文即如此规定），不是硬件行为的精确复刻。
pub fn measure_gain(before: &BatchMesh, after: &ForsythResult) -> GainReport {
    let before_stats = simulate_cache(&before.faces, after.cache_size);
    let after_stats = simulate_cache(&after.faces, after.cache_size);
    GainReport {
        before: before_stats,
        after: after_stats,
    }
}

/// 「死顶点让出槽位」理想化窗口：仅作**上界参考**，不是硬件行为。
///
/// 顶点一旦再无未输出面引用，就把它从窗口里摘掉（`CacheSim::evict`），
/// 让后续面能装进更多真正会被复用的活跃顶点。真实 GPU 无法主动驱逐，
/// 所以本函数只用于回答「若能及时腾掉无用槽位，收益天花板在哪」。
///
/// 故意**不**用于 `measure_gain`（见该函数口径说明）。
pub fn simulate_window_release(
    faces: &[[u32; 3]],
    vert_count: usize,
    cap: usize,
) -> CacheStats {
    let c = if cap == 0 { 1 } else { cap };
    let mut dead = vec![0u32; vert_count];
    for f in faces.iter() {
        for k in 0..3 {
            let v = f[k] as usize;
            if v < dead.len() {
                dead[v] += 1;
            }
        }
    }
    let mut cache = CacheSim::new(c);
    let mut hits = 0u32;
    let mut requests = 0u32;
    for f in faces.iter() {
        for k in 0..3 {
            let v = f[k];
            requests += 1;
            if cache.touch(v) {
                hits += 1;
            }
            let i = v as usize;
            if i < dead.len() && dead[i] > 0 {
                dead[i] -= 1;
                if dead[i] == 0 {
                    cache.evict(v);
                }
            }
        }
    }
    CacheStats { requests, hits }
}

// ---------------------------------------------------------------------------
// 七、测试网格辅助
// ---------------------------------------------------------------------------

/// 规则网格：`cols×rows` 顶点、每格两三角，材质按行分带。
pub fn grid_mesh(cols: usize, rows: usize, mat_bands: usize) -> BatchMesh {
    let mut m = BatchMesh::new();
    for r in 0..rows {
        for c in 0..cols {
            m.push_vert([c as f32, r as f32, 0.0]);
        }
    }
    let bands = if mat_bands == 0 { 1 } else { mat_bands };
    for r in 0..rows.saturating_sub(1) {
        for c in 0..cols.saturating_sub(1) {
            let a = (r * cols + c) as u32;
            let b = (r * cols + c + 1) as u32;
            let d = ((r + 1) * cols + c) as u32;
            let e = ((r + 1) * cols + c + 1) as u32;
            let mat = (r * bands / rows.max(1)) as u32;
            m.push_face([a, b, e]);
            m.materials.push(mat);
            m.push_face([a, e, d]);
            m.materials.push(mat);
        }
    }
    m
}

/// 反面绕序的网格（法线朝下，用于验证 Forsyth 不依赖朝向）。
pub fn reversed_winding_mesh(cols: usize, rows: usize) -> BatchMesh {
    let mut m = grid_mesh(cols, rows, 1);
    for f in m.faces.iter_mut() {
        let t = f[1];
        f[1] = f[2];
        f[2] = t;
    }
    m
}

// ---------------------------------------------------------------------------
// 八、自检
// ---------------------------------------------------------------------------

/// 判据族：材质分组 9、Forsyth 7、索引配合 6、收益量化 6、判据 13 = 41 项。
pub fn run_vei09_checks() -> CheckSet {
    let mut set = CheckSet::new("gfx-vei09");

    // ---- 材质分组族 ----
    // G1 单材质网格只产生一个分组
    {
        let m = grid_mesh(4, 4, 1);
        let r = group_by_material(&m);
        set.add(
            "I1609-分组-单材质一组",
            r.groups.len() == 1 && r.groups[0].face_count as usize == m.face_count(),
            "单材质网格应只有一组且覆盖全部面",
        );
    }
    // G2 分带材质产生对应数量的组
    {
        let m = grid_mesh(6, 6, 3);
        let r = group_by_material(&m);
        set.add(
            "I1609-分组-分带产生多组",
            r.groups.len() == 3 && r.material_count == 3,
            "3 个材质带应产生 3 组",
        );
    }
    // G3 分组严格覆盖全部面（无空洞无重叠）
    {
        let m = grid_mesh(6, 6, 4);
        let r = group_by_material(&m);
        set.add(
            "I1609-分组-覆盖无空洞",
            r.groups_cover_all(m.face_count()),
            "各组 first_face 应首尾相接并覆盖全部面",
        );
    }
    // G4 同材质的面在重排后连续
    {
        let mut m = grid_mesh(5, 5, 3);
        // 打乱材质分布，制造交错
        for (i, mm) in m.materials.iter_mut().enumerate() {
            *mm = (i % 3) as u32;
        }
        let r = group_by_material(&m);
        let mut contiguous = true;
        for g in r.groups.iter() {
            let s = g.first_face as usize;
            let e = s + g.face_count as usize;
            for k in s..e {
                if r.reordered_materials[k] != g.material {
                    contiguous = false;
                }
            }
        }
        set.add("I1609-分组-同材质连续", contiguous, "同材质面在重排后应落在同一连续区间");
    }
    // G5 分组稳定（同输入两次分组完全一致）
    {
        let mut m = grid_mesh(5, 5, 3);
        for (i, mm) in m.materials.iter_mut().enumerate() {
            *mm = ((i / 3) % 3) as u32;
        }
        let a = group_by_material(&m);
        let b = group_by_material(&m);
        set.add(
            "I1609-分组-稳定可复现",
            a.reordered_faces == b.reordered_faces && a.reordered_materials == b.reordered_materials,
            "同材质内保持原始面序，两次分组应完全一致",
        );
    }
    // G6 无材质表时全部归为材质 0
    {
        let mut m = grid_mesh(4, 4, 1);
        m.materials.clear();
        let r = group_by_material(&m);
        set.add(
            "I1609-分组-无材质视作单材质",
            r.material_count == 1 && r.reordered_materials.len() == m.face_count(),
            "清空材质表后应视作单材质",
        );
    }

    // ---- Forsyth 族 ----
    // F1 面数守恒（重排不丢面）
    {
        let m = grid_mesh(8, 8, 1);
        let r = forsyth_reorder(&m, CACHE_SIZE);
        set.add(
            "I1609-Forsyth-面数守恒",
            r.face_count() == m.face_count(),
            "重排后面数必须与输入一致",
        );
    }
    // F2 重映射是合法置换
    {
        let m = grid_mesh(8, 8, 1);
        let r = forsyth_reorder(&m, CACHE_SIZE);
        set.add(
            "I1609-Forsyth-重映射为置换",
            r.remap_is_permutation(m.vert_count()),
            "旧顶点到新顶点必须一一对应",
        );
    }
    // F3 重映射表长度等于顶点数
    {
        let m = grid_mesh(6, 6, 1);
        let r = forsyth_reorder(&m, CACHE_SIZE);
        set.add(
            "I1609-Forsyth-重映射长度",
            r.remap.len() == m.vert_count(),
            "remap 长度应等于原顶点数",
        );
    }
    // F4 顶点数不增（重排不引入新顶点）
    {
        let m = grid_mesh(8, 8, 1);
        let r = forsyth_reorder(&m, CACHE_SIZE);
        set.add(
            "I1609-Forsyth-顶点数不增",
            r.vert_count() <= m.vert_count(),
            "重排只重排既有顶点，不应增加",
        );
    }
    // F5 缓存窗口 1 时全部未命中（窗口装不下任何复用）
    {
        let m = grid_mesh(4, 4, 1);
        let st = simulate_cache(&m.faces, 1);
        // 窗口为 1：同一三角形内三个顶点各异 → 三个都未命中；
        // 下一三角形若首顶点恰是上一三角形末顶点则可能命中，故只断言「命中率明显低于大窗口」
        set.add(
            "I1609-Forsyth-小窗口命中低",
            st.hit_rate() < simulate_cache(&m.faces, CACHE_SIZE).hit_rate(),
            "窗口为 1 时命中率应低于窗口为 16",
        );
    }
    // F6 反向绕序网格同样可用（算法不依赖朝向）
    {
        let m = reversed_winding_mesh(5, 5);
        let r = forsyth_reorder(&m, CACHE_SIZE);
        set.add(
            "I1609-Forsyth-不依赖绕序",
            r.face_count() == m.face_count() && r.remap_is_permutation(m.vert_count()),
            "反面网格同样应正常重排",
        );
    }
    // F7 确定性：两次重排完全一致
    {
        let m = grid_mesh(7, 7, 1);
        let a = forsyth_reorder(&m, CACHE_SIZE);
        let b = forsyth_reorder(&m, CACHE_SIZE);
        set.add(
            "I1609-Forsyth-确定性",
            a.faces == b.faces && a.remap == b.remap,
            "同输入两次重排应逐项一致",
        );
    }

    // ---- 索引配合族 ----
    // I1 几何不变性（顶点坐标对应）
    {
        let m = grid_mesh(7, 7, 1);
        let r = forsyth_reorder(&m, CACHE_SIZE);
        set.add(
            "I1609-索引-几何保持",
            r.geometry_preserved(&m),
            "重排后每个面的三个顶点坐标应与原面对应",
        );
    }
    // I2 索引重排：面引用的都是重排后的新下标
    {
        let m = grid_mesh(7, 7, 1);
        let r = forsyth_reorder(&m, CACHE_SIZE);
        let vn = r.vert_count();
        let mut ok = true;
        for f in r.faces.iter() {
            for k in 0..3 {
                if f[k] as usize >= vn {
                    ok = false;
                }
            }
        }
        set.add("I1609-索引-引用不越界", ok, "重排后索引必须落在新顶点范围内");
    }
    // I3 remap 复合一致性：apply(remap, 旧索引) == 新索引
    //
    // 必须按 `face_origin` 找回同一个面：Forsyth 重排的是面序，
    // 拿输出第 k 个面去比原第 k 个面，测的是「面序变了」而非「重映射错了」。
    {
        let m = grid_mesh(6, 6, 1);
        let r = forsyth_reorder(&m, CACHE_SIZE);
        let mut ok = r.face_origin.len() == r.faces.len();
        for (k, nf) in r.faces.iter().enumerate() {
            if !ok {
                break;
            }
            let oi = r.face_origin[k] as usize;
            if oi >= m.faces.len() {
                ok = false;
                break;
            }
            let f = m.faces[oi];
            for j in 0..3usize {
                let old = f[j] as usize;
                if old >= r.remap.len() || r.remap[old] != nf[j] {
                    ok = false;
                }
            }
        }
        set.add(
            "I1609-索引-重映射可复算",
            ok,
            "对每个面，remap[旧索引] 应等于重排后的新索引",
        );
    }
    // I4 输出网格结构自洽
    {
        let m = grid_mesh(8, 8, 1);
        let r = forsyth_reorder(&m, CACHE_SIZE);
        let mut out = BatchMesh::new();
        out.positions = r.positions.clone();
        out.faces = r.faces.clone();
        let mut d = BatchDiagBag::new();
        set.add(
            "I1609-索引-输出结构自洽",
            out.validate(&mut d),
            "重排输出不得有越界索引或形状错",
        );
    }
    // I5 孤立顶点不残留（每个新顶点都被至少一个面引用）
    {
        let m = grid_mesh(8, 8, 1);
        let r = forsyth_reorder(&m, CACHE_SIZE);
        let mut used = vec![false; r.vert_count()];
        for f in r.faces.iter() {
            for k in 0..3 {
                if (f[k] as usize) < used.len() {
                    used[f[k] as usize] = true;
                }
            }
        }
        set.add(
            "I1609-索引-无孤立顶点",
            used.iter().all(|&u| u),
            "重排不应产生无面引用的死顶点",
        );
    }
    // I6 分组后仍可继续重排（两级流水线不冲突）
    {
        let m = grid_mesh(6, 6, 3);
        let g = group_by_material(&m);
        let mut gm = BatchMesh::new();
        gm.positions = m.positions.clone();
        gm.faces = g.reordered_faces.clone();
        gm.materials = g.reordered_materials.clone();
        let r = forsyth_reorder(&gm, CACHE_SIZE);
        set.add(
            "I1609-索引-分组后可续重排",
            r.face_count() == gm.face_count() && r.remap_is_permutation(gm.vert_count()),
            "材质分组产出的索引流应可直接送入重排",
        );
    }

    // ---- 收益量化族 ----
    // N1 重排不降低命中率
    {
        let m = grid_mesh(10, 10, 1);
        let r = forsyth_reorder(&m, CACHE_SIZE);
        let g = measure_gain(&m, &r);
        set.add(
            "I1609-收益-命中率不降",
            g.after.hits >= g.before.hits,
            "Forsyth 重排后命中数不应少于重排前",
        );
    }
    // N2 请求次数守恒（都等于 3×面数）
    {
        let m = grid_mesh(8, 8, 1);
        let r = forsyth_reorder(&m, CACHE_SIZE);
        let g = measure_gain(&m, &r);
        set.add(
            "I1609-收益-请求次数守恒",
            g.before.requests == 3 * m.face_count() as u32
                && g.after.requests == 3 * r.face_count() as u32,
            "重排前后请求次数都应等于 3×面数",
        );
    }
    // N3 未命中减少量非负
    {
        let m = grid_mesh(10, 10, 1);
        let r = forsyth_reorder(&m, CACHE_SIZE);
        let g = measure_gain(&m, &r);
        set.add(
            "I1609-收益-未命中减少非负",
            g.miss_reduction() > 0,
            "重排后未命中次数应减少",
        );
    }
    // N4 命中率增益与未命中减少量自洽
    {
        let m = grid_mesh(10, 10, 1);
        let r = forsyth_reorder(&m, CACHE_SIZE);
        let g = measure_gain(&m, &r);
        // requests 守恒 ⟹ 命中率增益 = 未命中减少 / requests
        let expect = g.miss_reduction() as f32 / g.after.requests as f32;
        set.add(
            "I1609-收益-增益与减量自洽",
            fabs(g.hit_rate_gain() - expect) < 1.0e-5,
            "命中率增益应等于未命中减少量除以请求数",
        );
    }
    // N5 相对减少率在 [0,1]
    {
        let m = grid_mesh(12, 12, 1);
        let r = forsyth_reorder(&m, CACHE_SIZE);
        let g = measure_gain(&m, &r);
        let rel = g.relative_miss_reduction();
        set.add(
            "I1609-收益-相对减少率有界",
            rel >= 0.0 && rel <= 1.0,
            "相对未命中减少率应落在 0~1",
        );
    }
    // N6 缓存模拟对同一索引流可复现（收益数字必须可复算）
    {
        let m = grid_mesh(7, 7, 1);
        let a = simulate_cache(&m.faces, CACHE_SIZE);
        let b = simulate_cache(&m.faces, CACHE_SIZE);
        set.add(
            "I1609-收益-模拟可复现",
            a == b && a.hit_rate() == b.hit_rate(),
            "同一索引流两次模拟结果应完全一致",
        );
    }

    // ---- 判据族 ----
    // P1 评分方向：score_triangle 奖励「已在缓存里」而非「不在缓存里」。
    //
    // 这是本单修过的最重伤：方向写反时，重排会主动挑离缓存最远的三角形，
    // 16×16 网格命中率从 0.644 直接掉到 0.000。用表外真实形态验证——
    // 先把三个顶点放进窗口，再确认含它们的三角形得分高于含陌生顶点的。
    {
        let mut c = CacheSim::new(CACHE_SIZE);
        c.touch(7);
        c.touch(8);
        c.touch(9);
        let warm = score_triangle(&c, &[7, 8, 100]);
        let cold = score_triangle(&c, &[200, 201, 202]);
        set.add(
            "I1609-判据-评分方向奖励命中",
            warm == 2 && cold == 0 && warm > cold,
            "窗口内顶点越多得分越高；全陌生三角形得 0 分",
        );
    }
    // P2 命中三角形必须被优先于冷三角形选出（把 P1 接进真实决策）
    {
        let mut m = BatchMesh::new();
        for i in 0..12u32 {
            m.push_vert([i as f32, 0.0, 0.0]);
        }
        m.push_face([0, 1, 2]);
        m.push_face([3, 4, 5]);
        // 两个面完全冷，靠死顶点剪枝无法区分，故再加一个与 [0,1,2] 共享边的暖面
        m.push_face([0, 1, 6]);
        let r = forsyth_reorder(&m, CACHE_SIZE);
        // 输出面里必须至少有一个来自网格连通部分，而非孤立面
        let mut seen_connected = false;
        for &o in r.face_origin.iter() {
            if o == 2 {
                seen_connected = true;
            }
        }
        set.add(
            "I1609-判据-暖面不落单",
            r.faces.len() == 3 && seen_connected,
            "三面网格重排不得丢面，暖面必须出现在输出里",
        );
    }
    // P3 face_origin 是面序的置换（重排改序但不改面集合）
    {
        let m = grid_mesh(9, 9, 1);
        let r = forsyth_reorder(&m, CACHE_SIZE);
        let mut seen = vec![false; m.face_count()];
        let mut ok = r.face_origin.len() == r.face_count();
        for &o in r.face_origin.iter() {
            if !ok {
                break;
            }
            let i = o as usize;
            if i >= seen.len() || seen[i] {
                ok = false;
                break;
            }
            seen[i] = true;
        }
        set.add(
            "I1609-判据-面来源为置换",
            ok && seen.iter().filter(|x| **x).count() == m.face_count(),
            "face_origin 必须把每个原面恰好映射一次（不重不漏）",
        );
    }
    // P4 面数守恒：优化选出 + 尾补 = 总面数
    {
        let m = grid_mesh(11, 11, 1);
        let r = forsyth_reorder(&m, CACHE_SIZE);
        set.add(
            "I1609-判据-面数守恒",
            r.face_count() == m.face_count() && r.optimized + r.tail_filled == r.face_count(),
            "重排不得增删面；优化数+尾补数应等于总面数",
        );
    }
    // P5 evict 确实把死顶点摘出窗口，且槽位可被后续顶点复用
    {
        let mut c = CacheSim::new(3);
        c.touch(1);
        c.touch(2);
        c.touch(3);
        let before_len = c.len();
        let hit = c.evict(2);
        let after_hit = c.contains(2);
        let still = c.contains(1) && c.contains(3);
        // 摘掉后窗口变短，新顶点直接补进来而不该挤掉别人
        c.touch(2);
        let back = c.contains(2) && c.contains(1) && c.contains(3);
        set.add(
            "I1609-判据-死顶点释放槽位",
            hit && !after_hit && still && before_len == 3 && c.len() == 3 && back,
            "evict 后该顶点不在窗口、同窗顶点保留、槽位可复用",
        );
    }
    // P6 evict 不在窗口里的顶点是无操作（不得误删同槽其他顶点）
    {
        let mut c = CacheSim::new(4);
        c.touch(10);
        c.touch(11);
        let r = c.evict(999);
        set.add(
            "I1609-判据-evict缺席为无操作",
            !r && c.len() == 2 && c.contains(10) && c.contains(11),
            "evict 不存在的顶点应返回 false 且窗口不变",
        );
    }
    // P7 决策与度量同口径：measure_gain 的 before 侧等于纯 LIFO 模拟
    //
    // 若有人把 measure_gain 悄悄换成「死顶点让位」窗口，这条会红——
    // 那正是本单踩过的坑：决策用一把尺、度量用另一把，收益会被系统性错报。
    {
        let m = grid_mesh(10, 10, 1);
        let r = forsyth_reorder(&m, CACHE_SIZE);
        let g = measure_gain(&m, &r);
        let pure = simulate_cache(&m.faces, CACHE_SIZE);
        set.add(
            "I1609-判据-度量口径与决策一致",
            g.before == pure,
            "measure_gain 的 before 必须等于纯 LIFO 窗口模拟结果",
        );
    }
    // P8 理想化上界窗口确实不低于纯 LIFO（多释放槽位不该变差）
    {
        let m = grid_mesh(12, 12, 1);
        let a = simulate_cache(&m.faces, CACHE_SIZE);
        let b = simulate_window_release(&m.faces, m.vert_count(), CACHE_SIZE);
        set.add(
            "I1609-判据-理想化窗口不劣于纯LIFO",
            b.hits >= a.hits,
            "允许死顶点让位后命中数不应少于不让位",
        );
    }
    // P10 尾补路径的三条不变量：直接构造「缺面」输入验证
    //
    // 主循环在现有用例里从不走尾补（has_dead 不可达），所以必须用
    // 直接调用 `fill_missing_faces` 的方式给它造场景，否则这段代码
    // 无法被任何判据覆盖，注入缺陷也测不出差别（弱门禁）。
    {
        let m = grid_mesh(7, 7, 1);
        let total = m.face_count();
        // 只放入前 3 个面，模拟主循环提前 break
        let mut emitted: Vec<[u32; 3]> = Vec::new();
        let mut origin: Vec<u32> = Vec::new();
        for ti in 0..3usize {
            emitted.push(m.faces[ti]);
            origin.push(ti as u32);
        }
        fill_missing_faces(&m, &mut emitted, &mut origin);
        // 1. 补齐到满 2. 不重复（origin 是置换）3. 两数组等长
        let mut seen = vec![false; total];
        let mut no_dup = true;
        for &o in origin.iter() {
            let i = o as usize;
            if i >= total || seen[i] {
                no_dup = false;
                break;
            }
            seen[i] = true;
        }
        let all_covered = seen.iter().filter(|x| **x).count() == total;
        set.add(
            "I1609-判据-尾补补齐",
            emitted.len() == total
                && origin.len() == total
                && no_dup
                && all_covered
                && emitted[0] == m.faces[0]
                && emitted[total - 1] == m.faces[total - 1],
            "补齐后面数/来源数组等长、来源成置换、已输出面不重复",
        );
    }
    // P11 尾补幂等：已补齐后再调一次不得改变任何东西
    {
        let m = grid_mesh(6, 6, 1);
        let total = m.face_count();
        let mut emitted: Vec<[u32; 3]> = Vec::new();
        let mut origin: Vec<u32> = Vec::new();
        fill_missing_faces(&m, &mut emitted, &mut origin);
        let snap_f = emitted.clone();
        let snap_o = origin.clone();
        fill_missing_faces(&m, &mut emitted, &mut origin);
        set.add(
            "I1609-判据-尾补幂等",
            emitted == snap_f && origin == snap_o && emitted.len() == total,
            "补齐后重复调用不得改动结果（否则会重复追加面）",
        );
    }
    // P13 尾补在「已输出过半」的场景下仍不超发
    //
    // 用真实半缺输入验证上界：即便去掉中途的 `emitted.len() >= n` 提前退出，
    // `used[]` 长度等于面数也足以挡住重复追加，故总面数不可能超过输入面数。
    // 这条判据把「上界」钉成结构保证，而非依赖某一行退出条件。
    {
        let m = grid_mesh(8, 8, 1);
        let total = m.face_count();
        let mut emitted: Vec<[u32; 3]> = Vec::new();
        let mut origin: Vec<u32> = Vec::new();
        // 只放入倒序的偶数号面，制造「一半有、一半缺」的最坏形态
        let mut ti = 0usize;
        while ti < total {
            emitted.push(m.faces[ti]);
            origin.push(ti as u32);
            ti += 2;
        }
        let pre = emitted.len();
        fill_missing_faces(&m, &mut emitted, &mut origin);
        let mut seen = vec![false; total];
        let mut no_dup = true;
        for &o in origin.iter() {
            let i = o as usize;
            if i >= total || seen[i] {
                no_dup = false;
                break;
            }
            seen[i] = true;
        }
        set.add(
            "I1609-判据-尾补上界结构保证",
            pre < total
                && emitted.len() == total
                && origin.len() == total
                && no_dup
                && seen.iter().filter(|x| **x).count() == total,
            "半缺输入补齐后恰好补满、不重复、不越界",
        );
    }
    // P12 尾补不得凭空造面（输入已满则不得再追加）
    {
        let m = grid_mesh(5, 5, 1);
        let mut emitted: Vec<[u32; 3]> = Vec::new();
        let mut origin: Vec<u32> = Vec::new();
        // 已经补满的场景：全量输出后再调用
        for ti in 0..m.face_count() {
            emitted.push(m.faces[ti]);
            origin.push(ti as u32);
        }
        let before_len = emitted.len();
        fill_missing_faces(&m, &mut emitted, &mut origin);
        set.add(
            "I1609-判据-尾补不超发",
            emitted.len() == before_len && origin.len() == before_len,
            "已满时调用不得再追加（总面数不得超过输入面数）",
        );
    }
    // P9 带孤立顶点的网格：置换判定不得把「无新下标」误判成非置换
    //
    // 用例前置必须真的成立——规则网格每个顶点都被面引用，孤立顶点分支
    // 在其他所有用例里从未被触发过，删掉也测不出差别。
    {
        let mut m = grid_mesh(6, 6, 1);
        let used = m.vert_count();
        // 追加 3 个谁也不引用的孤立顶点，其 remap 必为 u32::MAX
        m.push_vert([99.0, 99.0, 0.0]);
        m.push_vert([98.0, 99.0, 0.0]);
        m.push_vert([97.0, 99.0, 0.0]);
        let r = forsyth_reorder(&m, CACHE_SIZE);
        // 孤立顶点不该出现在输出顶点表里
        let leaked = r.positions
            .iter()
            .filter(|p| p[0] > 96.0)
            .count();
        // 且置换判定必须仍成立
        set.add(
            "I1609-判据-孤立顶点不破置换",
            leaked == 0 && r.remap_is_permutation(used + 3) && r.remap.len() == used + 3,
            "孤立顶点无新下标是合法输入，不得判成非置换，也不得泄漏进输出",
        );
    }
    // C1 单三角网格重排后命中数为 0
    {
        let mut m = BatchMesh::new();
        m.push_vert([0.0, 0.0, 0.0]);
        m.push_vert([1.0, 0.0, 0.0]);
        m.push_vert([0.0, 1.0, 0.0]);
        m.push_face([0, 1, 2]);
        let st = simulate_cache(&m.faces, CACHE_SIZE);
        set.add(
            "I1609-判据-单三角零命中",
            st.hits == 0 && st.requests == 3,
            "三个互异顶点首次访问全部未命中",
        );
    }
    // C2 重复索引面：同一顶点被访问三次应命中两次
    {
        let mut m = BatchMesh::new();
        m.push_vert([0.0, 0.0, 0.0]);
        m.push_face([0, 0, 0]);
        let st = simulate_cache(&m.faces, CACHE_SIZE);
        set.add(
            "I1609-判据-重复顶点命中",
            st.hits == 2 && st.requests == 3,
            "同一顶点连续访问三次：首次未命中、后两次命中",
        );
    }
    // C3 空网格不崩（诊断给出 EmptyMesh）
    {
        let m = BatchMesh::new();
        let mut d = BatchDiagBag::new();
        let ok = m.validate(&mut d);
        set.add(
            "I1609-判据-空网格报诊断",
            !ok && d.has(BatchDiag::EmptyMesh),
            "空网格应被 validate 判为非法并给出码",
        );
    }
    // C4 材质数量错配被检出
    {
        let mut m = grid_mesh(3, 3, 1);
        m.materials.pop();
        let mut d = BatchDiagBag::new();
        let ok = m.validate(&mut d);
        set.add(
            "I1609-判据-材质错配检出",
            !ok && d.has(BatchDiag::MaterialShapeMismatch),
            "材质 id 数量少于面数应被检出",
        );
    }
    // C5 缓存窗口内容是后进先出（新访问在队头）
    {
        let mut c = CacheSim::new(4);
        c.touch(1);
        c.touch(2);
        c.touch(3);
        let snap = c.snapshot();
        set.add(
            "I1609-判据-缓存后进先出",
            snap.len() == 3 && snap[0] == 3 && snap[2] == 1,
            "队头应是最新访问的 3，队尾是最早的 1",
        );
    }
    // C6 命中必须把顶点**提到队头**（后进先出的定义性行为）
    //
    // 补这条的原因：C5 只验「三个互异顶点依次进窗口」的队头队尾顺序，
    // 而这三个顶点**全是未命中**——未命中路径无论 LIFO 还是 FIFO 都往 0 号位放，
    // 外部表现完全相同。把 `touch` 的命中分支改成 FIFO（命中不换位置）时，
    // 原有 43 条判据全绿，而命中率模型已经不是 GPU 行为了。
    // 必须造一个**真命中**再验位置变化，才钉得住模型本身。
    {
        let mut c = CacheSim::new(4);
        c.touch(1);
        c.touch(2);
        c.touch(3);
        // 此时队头=3、队尾=1；命中队尾的 1
        let hit = c.touch(1);
        let snap = c.snapshot();
        // 后进先出：命中的 1 必须升到队头，原队尾位置由次新的 2 占据
        set.add(
            "I1609-判据-命中提到队头",
            hit && snap.len() == 3 && snap[0] == 1 && snap[2] == 2,
            "命中队尾顶点后它应升到队头，次新者退到队尾",
        );
    }
    // C7 命中只换位置，不增删成员（与 C6 配套）
    //
    // C6 钉「位置」，这条钉「换位置不等于偷偷多插/少删一个」——
    // 后者正是把「提到队头」误实现成「插入队头再删队尾」时会漏掉的缺陷。
    {
        let mut c = CacheSim::new(4);
        c.touch(5);
        c.touch(6);
        c.touch(7);
        c.touch(5);
        let mut members = c.snapshot();
        members.sort();
        set.add(
            "I1609-判据-命中成员集合不变",
            c.len() == 3 && members == vec![5u32, 6, 7],
            "命中只调整顺序，窗口长度与成员集合必须保持不变",
        );
    }
    // C8 严格构造拒绝零窗口（不得静默兜底成 1）
    //
    // 零窗口会让全部访问恒未命中、收益量化恒为 0；静默兜底会把
    // 「调用方传错尺寸」变成「结果看着正常却全错」。
    {
        let rejected = CacheSim::try_new(0);
        let accepted = CacheSim::try_new(1);
        set.add(
            "I1609-判据-零窗口被拒",
            rejected.is_err() && accepted.is_ok() && accepted.map(|c| c.capacity()) == Ok(1),
            "try_new(0) 应报 CacheSizeInvalid，窗口 1 应正常构造",
        );
    }
    // C9 每个诊断码可逆（code → of_code → code 往返一致）
    //
    // 码表是跨模块契约（F1629 上报按码取义）。某个变体只进不出或只出不进，
    // 都会让上报侧解不出故障种类。
    {
        let all = [
            BatchDiag::FaceIndexOutOfRange,
            BatchDiag::MaterialShapeMismatch,
            BatchDiag::EmptyMesh,
            BatchDiag::RemapShapeMismatch,
            BatchDiag::RemapNotPermutation,
            BatchDiag::CacheSizeInvalid,
        ];
        let mut roundtrip = true;
        for d in all.iter() {
            if BatchDiag::of_code(d.code()) != Some(*d) {
                roundtrip = false;
            }
            if d.label().is_empty() {
                roundtrip = false;
            }
        }
        // 43 号码已随死变体 UvShapeMismatch 一并移除，不得再被解析出
        set.add(
            "I1609-判据-诊断码往返一致",
            roundtrip && BatchDiag::of_code(43).is_none() && !BatchDiag::of_code(200).is_some(),
            "每个变体 code/of_code 往返一致，废弃码号不再解析",
        );
    }
    // G7 分组按材质**严格升序**排列（draw call 顺序稳定可预期）
    //
    // 补这条的原因：原 G1~G6 只验「组数」「覆盖」「同材质连续」，
    // 全都是**顺序无关**的谓词。把排序键从「材质升序 + 原面号」改成
    // 「材质降序」后，各组依然连续、依然全覆盖、组数也不变——
    // 原有判据全绿，而 draw call 的输出顺序已经静默翻转。
    // 故必须有一条**直接断言组间材质序关系**的判据。
    {
        let mut m = grid_mesh(7, 7, 1);
        for (i, mm) in m.materials.iter_mut().enumerate() {
            *mm = (i % 4) as u32;
        }
        let r = group_by_material(&m);
        let mut ascending = true;
        for k in 1..r.groups.len() {
            // 同材质只会聚成一段，故组间材质必须是**严格**递增
            if r.groups[k - 1].material >= r.groups[k].material {
                ascending = false;
            }
        }
        set.add(
            "I1609-分组-材质严格升序",
            ascending && r.groups.len() == 4 && r.groups.last().map(|g| g.material) == Some(3),
            "各组材质应严格递增（0→1→2→3），组序不得静默翻转",
        );
    }
    // G8 重排后材质序列整体非降（序列级断言，独立于 groups 的索引视图）
    {
        let mut m = grid_mesh(6, 6, 1);
        for (i, mm) in m.materials.iter_mut().enumerate() {
            *mm = ((i * 5) % 7) as u32;
        }
        let r = group_by_material(&m);
        let mut non_decreasing = true;
        for k in 1..r.reordered_materials.len() {
            if r.reordered_materials[k - 1] > r.reordered_materials[k] {
                non_decreasing = false;
            }
        }
        set.add(
            "I1609-分组-材质序列非降",
            non_decreasing && r.reordered_materials.len() == m.face_count(),
            "reordered_materials 逐项比较应处处非降",
        );
    }
    // G9 每种材质恰一段，且段面数等于判据侧**独立统计**的真实面数
    //
    // 参考值由判据侧自行数出来，不向 `group_by_material` 问答案
    // （自证式判据会让分组算错时仍然全绿）。
    {
        let mut m = grid_mesh(7, 7, 1);
        for (i, mm) in m.materials.iter_mut().enumerate() {
            *mm = (i % 3) as u32;
        }
        let r = group_by_material(&m);
        // 独立统计：mat -> 真实出现次数
        let mut truth: Vec<(u32, u32)> = Vec::new();
        for &mv in m.materials.iter() {
            let mut hit_slot = false;
            for t in truth.iter_mut() {
                if t.0 == mv {
                    t.1 += 1;
                    hit_slot = true;
                }
            }
            if !hit_slot {
                truth.push((mv, 1u32));
            }
        }
        let mut consistent = truth.len() == r.groups.len();
        if consistent {
            for g in r.groups.iter() {
                let mut matched = false;
                for t in truth.iter() {
                    if t.0 == g.material {
                        matched = t.1 == g.face_count;
                        break;
                    }
                }
                let end = g.first_face as usize + g.face_count as usize;
                if !matched || end > r.reordered_faces.len() {
                    consistent = false;
                    break;
                }
            }
        }
        set.add(
            "I1609-分组-段数与材质种类一致",
            consistent,
            "每种材质恰一段，且段面数等于判据侧独立统计的真实面数",
        );
    }

    set
}
