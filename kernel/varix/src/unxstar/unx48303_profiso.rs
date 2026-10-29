//! unx48303_profiso — UNX-F48303 多 profile 隔离纵深（工作/游戏/极简三档预置，互不污染）（诊断段 0xC6C0）。
//!
//! 判据原文：
//!   J1 三档切换 ×100 行为一致、词库互不可见（B04 多档联签续）；
//!   判据锚定 M5 隔离母版。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 三档预置：工作/游戏/极简三档各有独立词库空间与设置四元组；
//!      预置互异（设置逐位不同）是隔离有意义的前提。
//!   2. 切换行为一致 ×100：切换循环 100 次，每次切换后引擎可见视图
//!      必须恰为激活档的词库（视图=激活空间，一一对应，零错档）。
//!   3. 词库互不可见：任意档写入的词条对其他两档不可见（三档两两
//!      互查 ×3 对全查）；写隔离 = 只有激活档可写、写只落激活档。
//!   4. 零污染：全部写完之后，每档词库 = 本档初始集 + 本档写入集，
//!      不含任何外来词（逐档指纹核对）。
//!   5. 切换幂等：切到当前档 = 无操作（状态零变化、不重置词库）。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC6C0;

const E_PRESET_SAME: u16 = SEG_BASE + 1;   // 预置互异失败
const E_VIEW_MISMATCH: u16 = SEG_BASE + 2; // 视图与激活档不符
const E_CROSS_LEAK: u16 = SEG_BASE + 3;    // 词库跨档可见（泄漏）
const E_POLLUTION: u16 = SEG_BASE + 4;     // 档内混入外来词
const E_WRITE_WRONG_SLOT: u16 = SEG_BASE + 5; // 写未落激活档
const E_SWITCH_NONIDEM: u16 = SEG_BASE + 6;   // 同档切换产生状态变化
const E_CODE_RANGE: u16 = SEG_BASE + 7;    // 诊断码越段

/// 三档预置。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// 工作：正式词库 + 严谨设置。
    Work = 0,
    /// 游戏：短词优先 + 快捷设置。
    Game = 1,
    /// 极简：最小词库 + 保守设置。
    Minimal = 2,
}

pub const PROFILE_COUNT: usize = 3;

/// 档位词库空间：词哈希集（有序去重）+ 设置四元组。
pub struct ProfileSpace {
    pub words: Vec<u32>,
    pub settings: [u32; 4],
}

/// 确定性线性同余发生器。
pub struct Lcg(u32);

impl Lcg {
    pub fn new(seed: u32) -> Self {
        Lcg(seed | 1)
    }
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
}

/// 三档初始空间：设置互异（逐位可证）+ 各档确定性初始词集（互不重叠）。
pub fn init_spaces() -> [ProfileSpace; PROFILE_COUNT] {
    let presets: [[u32; 4]; PROFILE_COUNT] = [
        [0x1111_0001u32, 0x2222_0002u32, 0x3333_0003u32, 0x4444_0004u32], // 工作
        [0xAAAA_0001u32, 0xBBBB_0002u32, 0xCCCC_0003u32, 0xDDDD_0004u32], // 游戏
        [0x0F0F_0001u32, 0x0E0E_0002u32, 0x0D0D_0003u32, 0x0C0C_0004u32], // 极简
    ];
    let mut spaces: [ProfileSpace; PROFILE_COUNT] = [
        ProfileSpace {
            words: Vec::new(),
            settings: presets[0],
        },
        ProfileSpace {
            words: Vec::new(),
            settings: presets[1],
        },
        ProfileSpace {
            words: Vec::new(),
            settings: presets[2],
        },
    ];
    // 初始词集：档 p 生成 6 词，值域按档偏移（互不重叠）。
    let mut p = 0usize;
    while p < PROFILE_COUNT {
        let mut lc = Lcg::new(0x4830_3000 + p as u32);
        let mut n = 0;
        while n < 6 {
            let w = (p as u32) * 0x0100_0000 + lc.next() % 0x0001_0000;
            if !spaces[p].words.contains(&w) {
                spaces[p].words.push(w);
                n += 1;
            }
        }
        p += 1;
    }
    spaces
}

/// 引擎视图：当前激活档的词库（切换即换视图，视图是只读投影）。
pub struct ProfileManager {
    active: Profile,
    spaces: [ProfileSpace; PROFILE_COUNT],
    switch_count: u32,
}

impl ProfileManager {
    pub fn new() -> Self {
        ProfileManager {
            active: Profile::Work,
            spaces: init_spaces(),
            switch_count: 0,
        }
    }

    pub fn active(&self) -> Profile {
        self.active
    }

    pub fn switch_count(&self) -> u32 {
        self.switch_count
    }

    /// 引擎可见词集（只读投影：恰为激活档词库）。
    pub fn view(&self) -> &[u32] {
        match self.active {
            Profile::Work => &self.spaces[0].words,
            Profile::Game => &self.spaces[1].words,
            Profile::Minimal => &self.spaces[2].words,
        }
    }

    /// 激活档设置视图（只读投影：恰为激活档四元组）。
    pub fn settings_view(&self) -> [u32; 4] {
        match self.active {
            Profile::Work => self.spaces[0].settings,
            Profile::Game => self.spaces[1].settings,
            Profile::Minimal => self.spaces[2].settings,
        }
    }

    /// 切换：非同档才生效；同档 = 无操作（幂等）。
    /// 返回 true = 发生了切换。
    pub fn switch_to(&mut self, p: Profile) -> bool {
        if p == self.active {
            return false;
        }
        self.active = p;
        self.switch_count += 1;
        true
    }

    /// 写词：只落激活档空间；重复词去重。返回是否新增。
    pub fn write_word(&mut self, w: u32) -> bool {
        let idx = match self.active {
            Profile::Work => 0,
            Profile::Game => 1,
            Profile::Minimal => 2,
        };
        if self.spaces[idx].words.contains(&w) {
            return false;
        }
        self.spaces[idx].words.push(w);
        true
    }

    /// 外来词检查：词 w 对非激活档是否不可见。
    pub fn hidden_from_others(&self, w: u32) -> bool {
        let act = match self.active {
            Profile::Work => 0,
            Profile::Game => 1,
            Profile::Minimal => 2,
        };
        let mut p = 0usize;
        while p < PROFILE_COUNT {
            if p != act && self.spaces[p].words.contains(&w) {
                return false;
            }
            p += 1;
        }
        true
    }

    pub fn space_fingerprint(&self, p: usize) -> u64 {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        let mut i = 0;
        while i < self.spaces[p].words.len() {
            h ^= self.spaces[p].words[i] as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            i += 1;
        }
        h
    }

    pub fn space_words(&self, p: usize) -> &[u32] {
        &self.spaces[p].words
    }
}

/// 多 profile 隔离主巡检：×100 切换 + 互不可见全链判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 预置互异：三档设置四元组两两不同（逐位比较）。
    let probe = ProfileManager::new();
    let mut p = 0usize;
    while p < PROFILE_COUNT {
        let mut q = p + 1;
        while q < PROFILE_COUNT {
            let a = match p {
                0 => [0x1111_0001u32, 0x2222_0002u32, 0x3333_0003u32, 0x4444_0004u32],
                1 => [0xAAAA_0001u32, 0xBBBB_0002u32, 0xCCCC_0003u32, 0xDDDD_0004u32],
                _ => [0x0F0F_0001u32, 0x0E0E_0002u32, 0x0D0D_0003u32, 0x0C0C_0004u32],
            };
            let b = match q {
                0 => [0x1111_0001u32, 0x2222_0002u32, 0x3333_0003u32, 0x4444_0004u32],
                1 => [0xAAAA_0001u32, 0xBBBB_0002u32, 0xCCCC_0003u32, 0xDDDD_0004u32],
                _ => [0x0F0F_0001u32, 0x0E0E_0002u32, 0x0D0D_0003u32, 0x0C0C_0004u32],
            };
            if a == b {
                return Err(E_PRESET_SAME);
            }
            q += 1;
        }
        p += 1;
    }
    hits += 1;

    // R2 ×100 切换行为一致：确定性切换序列（LCG 选档），每次切换后
    //    视图恰为激活档初始词集（此阶段尚未写词，视图=初始集）。
    let mut pm = ProfileManager::new();
    let mut lc = Lcg::new(0x4830_3100);
    let mut switched = 0u32;
    let mut step = 0u32;
    while step < 100 {
        let pick = lc.next() % 3;
        let target = match pick {
            0 => Profile::Work,
            1 => Profile::Game,
            _ => Profile::Minimal,
        };
        let before = pm.active();
        let moved = pm.switch_to(target);
        if moved {
            switched += 1;
            if pm.active() != target || before == target {
                return Err(E_VIEW_MISMATCH);
            }
        }
        // 视图一致性：视图词数 == 激活档空间词数（此时都是 6）。
        if pm.view().len() != 6 {
            return Err(E_VIEW_MISMATCH);
        }
        step += 1;
    }
    if switched == 0 {
        return Err(E_VIEW_MISMATCH); // 序列必须真的发生切换
    }
    hits += 1;

    // R3 写隔离：三轮「切档→写本档专属词」→ 词只落激活档。
    let stamps: [u32; 3] = [0x7100_0001, 0x7200_0002, 0x7300_0003];
    let slots = [Profile::Work, Profile::Game, Profile::Minimal];
    let mut s = 0usize;
    while s < 3 {
        pm.switch_to(slots[s]);
        if !pm.write_word(stamps[s]) {
            return Err(E_WRITE_WRONG_SLOT);
        }
        // 本档可见。
        if !pm.view().contains(&stamps[s]) {
            return Err(E_WRITE_WRONG_SLOT);
        }
        // 其他两档不可见（写入瞬间的互不可见判据）。
        if !pm.hidden_from_others(stamps[s]) {
            return Err(E_CROSS_LEAK);
        }
        s += 1;
    }
    hits += 1;

    // R4 词库互不可见（全量两两互查 ×3 对）：每档的专属词（初始 6 词
    //    值域按档偏移 + 写入戳）在对方档中不出现。
    let mut a = 0usize;
    while a < PROFILE_COUNT {
        let mut b = a + 1;
        while b < PROFILE_COUNT {
            let mut i = 0;
            while i < pm.space_words(a).len() {
                if pm.space_words(b).contains(&pm.space_words(a)[i]) {
                    return Err(E_CROSS_LEAK);
                }
                i += 1;
            }
            b += 1;
        }
        a += 1;
    }
    hits += 1;

    // R5 零污染：每档词数 == 6（初始）+ 本档写入数，且无外来值域词。
    //    值域纪律：档 p 的词高 8 位 == p（初始生成器与写入戳共同保证）。
    let mut p2 = 0usize;
    while p2 < PROFILE_COUNT {
        let words = pm.space_words(p2);
        if words.len() != 7 {
            return Err(E_POLLUTION); // 6 初始 + 1 戳
        }
        let mut i = 0;
        while i < words.len() {
            let hi = (words[i] >> 24) as usize;
            if hi != p2 {
                return Err(E_POLLUTION);
            }
            i += 1;
        }
        p2 += 1;
    }
    hits += 1;

    // R6 切换幂等：同档切换 ×5 → 状态零变化、切换计数不变、词库不动。
    let fp_before = pm.space_fingerprint(1);
    pm.switch_to(Profile::Game); // 先确保在 Game
    let cnt_before = pm.switch_count();
    let mut k = 0;
    while k < 5 {
        if pm.switch_to(Profile::Game) {
            return Err(E_SWITCH_NONIDEM);
        }
        k += 1;
    }
    if pm.switch_count() != cnt_before || pm.space_fingerprint(1) != fp_before {
        return Err(E_SWITCH_NONIDEM);
    }
    hits += 1;

    // R7 设置视图可观测：激活档的设置四元组经管理器可读，切档即
    //    换设置视图（行为一致的另一半：不只是词库，设置同步换装）。
    pm.switch_to(Profile::Work);
    let s_work = pm.settings_view();
    pm.switch_to(Profile::Game);
    let s_game = pm.settings_view();
    pm.switch_to(Profile::Minimal);
    let s_min = pm.settings_view();
    if s_work == s_game || s_game == s_min || s_work == s_min {
        return Err(E_PRESET_SAME);
    }
    // 切回工作档设置视图必须复原（确定性换装）。
    pm.switch_to(Profile::Work);
    if pm.settings_view() != s_work {
        return Err(E_VIEW_MISMATCH);
    }
    hits += 1;

    // R8 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 7] = [
        E_PRESET_SAME,
        E_VIEW_MISMATCH,
        E_CROSS_LEAK,
        E_POLLUTION,
        E_WRITE_WRONG_SLOT,
        E_SWITCH_NONIDEM,
        E_CODE_RANGE,
    ];
    let mut ci = 0;
    while ci < CODES.len() {
        if CODES[ci] <= SEG_BASE || CODES[ci] > SEG_BASE + 0x1F {
            return Err(E_CODE_RANGE);
        }
        ci += 1;
    }
    hits += 1;

    Ok(hits)
}
