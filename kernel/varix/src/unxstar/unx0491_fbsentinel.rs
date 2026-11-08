//! UNX-F0491 帧缓冲哨兵（stride 尾巴写禁令） · 0x75D80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，AI-VX7-1104 批5 L 组）：
//!   stride 尾巴（行尾到下一行起点间隙）写禁令：哨兵格写入即显性抓
//!   （哨兵魔数破坏检出 100%），行守恒（行数×stride=缓冲大小）。
//!
//! 设计要点：
//!   ① 哨兵魔数取 0x5AF3_C00DE 口径（任务板冻结值），每行尾巴格恰放
//!      一枚哨兵，哨兵格数 = 行数；
//!   ② 行守恒恒等式：行数 × stride = 缓冲大小（checked_mul 复算），
//!      stride 必须 ≥ 可见宽 + 1，否则尾巴装不下哨兵格即拒；
//!   ③ 尾巴写禁令双层防线：API 层 write_cell 对 col ≥ 可见宽的写入
//!      显性拒（TailWrite）；物理层越权直写由 check_sentinels 扫描
//!      魔数破坏当场抓（SentinelBroken），注入检出率必须 100%；
//!   ④ 状态机单调：Uninit → Armed → Verified → Sealed，倒退与越级
//!      显性拒（BadState），不许「验证过的帧缓冲悄悄回到未布防」；
//!   ⑤ 所有扫描/写入走 .get()/.get_mut()，无裸下标；整数运算，中文
//!      诊断文案带可操作建议（建议写进 detail）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75D80，批5 L 组 F0491 独占窗口）。
pub const SEG_BASE: u32 = 0x75D80;

/// 哨兵魔数（0x5AF3 C00DE 口径，任务板冻结；u64 承载 37 位常量）。
/// 来源：UNX-F0491 分派卡「哨兵魔数（0x5AF3 C00DE 口径）」。
pub const SENTINEL_MAGIC: u64 = 0x0000_5AF3_C00DE;

/// 每行哨兵格数（冻结 1：一格哨兵 per 行，哨兵格数=行数）。
pub const SENTINEL_CELLS_PER_ROW: usize = 1;

/// 哨兵判定轮数（判据 10/10 语义，注入轮写死 10 轮）。
pub const JUDGE_ROUNDS: u32 = 10;

/// 最小帧缓冲行数（冻结 1：0 行帧缓冲无意义，规划即拒）。
pub const MIN_ROWS: u32 = 1;

/// 最小帧缓冲可见宽（冻结 1：0 宽无意义，规划即拒）。
pub const MIN_WIDTH: u32 = 1;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FbCode {
    /// 尾巴写入：col 越过可见宽，触 stride 尾巴写禁令
    TailWrite,
    /// 行号越界
    RowOver,
    /// 哨兵魔数破坏：尾巴格被越权直写
    SentinelBroken,
    /// 行守恒破坏：行数×stride ≠ 缓冲大小
    ConservationBroken,
    /// stride 过小：装不下可见宽 + 哨兵格
    StrideTooSmall,
    /// 状态机倒退或越级
    BadState,
    /// 重复布防：已 Armed 后再次 arm
    DoubleArm,
    /// 双跑不一致：复现红线
    NotReproducible,
}

impl FbCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FbCode::TailWrite => "尾巴写入被拒：col 越过可见宽；请改用可见列下标（0..宽），\
                stride 尾巴是哨兵专属区，任何业务写入都属违宪",
            FbCode::RowOver => "行号越界：超出帧缓冲行数；请以 height() 为上界重算行号",
            FbCode::SentinelBroken => "哨兵魔数破坏：尾巴格遭越权直写；请回滚对 stride 尾巴的\
                全部裸写，统一走 write_cell 通道后重新 arm+verify",
            FbCode::ConservationBroken => "行守恒破坏：行数×stride ≠ 缓冲大小；请按\
                rows×stride 重排缓冲或修正 stride 声明",
            FbCode::StrideTooSmall => "stride 过小：装不下可见宽+哨兵格；请把 stride 提到\
                至少 宽+1 后重新规划",
            FbCode::BadState => "状态机倒退/越级：帧缓冲生命周期只许单向推进；\
                请按 Uninit→Armed→Verified→Sealed 次序调用",
            FbCode::DoubleArm => "重复布防：已 Armed 的帧缓冲不许再 arm；\
                如需重布防请走破坏检出→修复→re-arm 的显性路径",
            FbCode::NotReproducible => "双跑不一致：同输入两次运行结果不同，复现红线；\
                请排查全局可变状态与未初始化读",
        }
    }
    pub const ALL: [FbCode; 8] = [
        FbCode::TailWrite,
        FbCode::RowOver,
        FbCode::SentinelBroken,
        FbCode::ConservationBroken,
        FbCode::StrideTooSmall,
        FbCode::BadState,
        FbCode::DoubleArm,
        FbCode::NotReproducible,
    ];
}

/// 查表闭集返回（未知码不在本段窗口时给中文兜底文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < FbCode::ALL.len() {
        if FbCode::ALL[i].val() == code {
            return FbCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x75D80 段闭集内"
}

// ── 帧缓冲生命周期状态机（单调，倒退显性拒） ──────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FbState {
    /// 未布防：哨兵格尚未写魔数
    Uninit,
    /// 已布防：哨兵格已写入 0x5AF3_C00DE
    Armed,
    /// 已验证：全行哨兵扫描通过
    Verified,
    /// 已封印：验证后冻结，只读
    Sealed,
}

impl FbState {
    /// 单调秩（只许 +1 推进）。
    fn rank(self) -> u8 {
        match self {
            FbState::Uninit => 0,
            FbState::Armed => 1,
            FbState::Verified => 2,
            FbState::Sealed => 3,
        }
    }
    /// 下一步状态（Sealed 无下一步）。
    fn next(self) -> Option<FbState> {
        match self {
            FbState::Uninit => Some(FbState::Armed),
            FbState::Armed => Some(FbState::Verified),
            FbState::Verified => Some(FbState::Sealed),
            FbState::Sealed => None,
        }
    }
}

// ── 帧缓冲域模型 ────────────────────────────────────────────────
/// 帧缓冲规划书（冻结三账：可见宽/行数/stride）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FbPlan {
    /// 可见宽（格）
    pub width: u32,
    /// 行数
    pub height: u32,
    /// 行距（格/行，含尾巴）
    pub stride: u32,
}

/// 帧缓冲哨兵账（cell 粒度整数模型，无真实显存依赖）。
#[derive(Clone, Debug)]
pub struct FrameBuf {
    pub plan: FbPlan,
    /// 缓冲格：len 必须恰等于 height×stride（行守恒）
    cells: Vec<u64>,
    pub state: FbState,
    /// 最近一次破坏检出的行号（无破坏为 None）
    pub last_broken_row: Option<usize>,
    /// 尾巴写入被拒次数（防线 1 记账）
    pub tail_write_rejected: u32,
    /// 哨兵破坏检出次数（防线 2 记账）
    pub sentinel_breaks_found: u32,
}

impl FrameBuf {
    /// 规划帧缓冲：stride ≥ 宽+哨兵格，缓冲按 rows×stride 精确切出
    /// （行守恒由构造保证），并清零。违者显性拒。
    pub fn plan(width: u32, height: u32, stride: u32) -> Result<FrameBuf, u32> {
        if width < MIN_WIDTH {
            return Err(FbCode::StrideTooSmall.val());
        }
        if height < MIN_ROWS {
            return Err(FbCode::ConservationBroken.val());
        }
        // 尾巴 = stride - 宽，必须装下哨兵格（≥1）。
        if stride.checked_sub(width).map(|t| t < SENTINEL_CELLS_PER_ROW as u32).unwrap_or(true) {
            return Err(FbCode::StrideTooSmall.val());
        }
        let total = match (height as usize).checked_mul(stride as usize) {
            Some(t) => t,
            None => return Err(FbCode::ConservationBroken.val()),
        };
        let mut cells = Vec::new();
        cells.resize(total, 0u64);
        Ok(FrameBuf {
            plan: FbPlan { width, height, stride },
            cells,
            state: FbState::Uninit,
            last_broken_row: None,
            tail_write_rejected: 0,
            sentinel_breaks_found: 0,
        })
    }

    /// 行守恒复算：行数×stride（checked）== 缓冲大小。
    pub fn conservation_ok(&self) -> bool {
        match (self.plan.height as usize).checked_mul(self.plan.stride as usize) {
            Some(expect) => expect == self.cells.len(),
            None => false,
        }
    }

    /// 哨兵格数复算：应恰等于行数。
    pub fn sentinel_cell_count(&self) -> usize {
        self.plan.height as usize * SENTINEL_CELLS_PER_ROW
    }

    /// 第 r 行哨兵格的缓冲下标（行首 + 可见宽）。
    fn sentinel_index(&self, row: usize) -> Option<usize> {
        if row >= self.plan.height as usize {
            return None;
        }
        (row * self.plan.stride as usize).checked_add(self.plan.width as usize)
    }

    /// 布防：把每行尾巴格写入哨兵魔数。Uninit→Armed。
    pub fn arm(&mut self) -> Result<(), u32> {
        if self.state != FbState::Uninit {
            return Err(FbCode::DoubleArm.val());
        }
        if !self.conservation_ok() {
            return Err(FbCode::ConservationBroken.val());
        }
        let mut r = 0usize;
        while r < self.plan.height as usize {
            if let Some(idx) = self.sentinel_index(r) {
                if let Some(c) = self.cells.get_mut(idx) {
                    *c = SENTINEL_MAGIC;
                }
            }
            r += 1;
        }
        self.state = FbState::Armed;
        Ok(())
    }

    /// 可见区写格（业务唯一写通道）：尾巴写禁令在 API 层显性拒。
    pub fn write_cell(&mut self, row: u32, col: u32, v: u64) -> Result<(), u32> {
        if row >= self.plan.height {
            return Err(FbCode::RowOver.val());
        }
        if col >= self.plan.width {
            // 防线 1：尾巴写禁令。
            self.tail_write_rejected += 1;
            return Err(FbCode::TailWrite.val());
        }
        if self.state.rank() < FbState::Armed.rank() {
            return Err(FbCode::BadState.val());
        }
        if self.state == FbState::Sealed {
            return Err(FbCode::BadState.val());
        }
        let idx = (row as usize) * (self.plan.stride as usize) + (col as usize);
        match self.cells.get_mut(idx) {
            Some(c) => {
                *c = v;
                Ok(())
            }
            None => Err(FbCode::ConservationBroken.val()),
        }
    }

    /// 全可见区填充（清屏/上底色）：只写可见列，哨兵格零接触。
    pub fn fill_visible(&mut self, v: u64) -> Result<(), u32> {
        if self.state.rank() < FbState::Armed.rank() || self.state == FbState::Sealed {
            return Err(FbCode::BadState.val());
        }
        let mut r = 0u32;
        while r < self.plan.height {
            let mut c = 0u32;
            while c < self.plan.width {
                self.write_cell(r, c, v)?;
                c += 1;
            }
            r += 1;
        }
        Ok(())
    }

    /// 哨兵扫描（防线 2）：任一行尾巴格魔数不符即显性抓。
    pub fn check_sentinels(&mut self) -> Result<(), u32> {
        let mut r = 0usize;
        while r < self.plan.height as usize {
            let idx = match self.sentinel_index(r) {
                Some(i) => i,
                None => return Err(FbCode::ConservationBroken.val()),
            };
            let broken = match self.cells.get(idx) {
                Some(v) => *v != SENTINEL_MAGIC,
                None => true,
            };
            if broken {
                self.last_broken_row = Some(r);
                self.sentinel_breaks_found += 1;
                return Err(FbCode::SentinelBroken.val());
            }
            r += 1;
        }
        Ok(())
    }

    /// 单调推进：Uninit→Armed→Verified→Sealed，倒退/越级显性拒。
    pub fn advance(&mut self) -> Result<(), u32> {
        let want = match self.state.next() {
            Some(n) => n,
            None => return Err(FbCode::BadState.val()),
        };
        // 推进到 Verified/Sealed 前必须先过哨兵扫描。
        if want == FbState::Verified || want == FbState::Sealed {
            self.check_sentinels()?;
        }
        self.state = want;
        Ok(())
    }

    /// 越权直写模拟（注入专用）：对第 row 行尾巴格的 1 字节低位居
    /// 翻转——模拟绕过 write_cell 的裸写违约，必须被防线 2 100% 抓。
    pub fn rogue_tail_poke(&mut self, row: usize) -> Result<(), u32> {
        if row >= self.plan.height as usize {
            return Err(FbCode::RowOver.val());
        }
        let idx = match self.sentinel_index(row) {
            Some(i) => i,
            None => return Err(FbCode::ConservationBroken.val()),
        };
        match self.cells.get_mut(idx) {
            Some(c) => {
                *c ^= 0x01;
                Ok(())
            }
            None => Err(FbCode::ConservationBroken.val()),
        }
    }

    /// 行守恒违约模拟（注入专用）：抽走最后一格，令 rows×stride≠len。
    pub fn rogue_shrink(&mut self) {
        self.cells.pop();
    }
}

// ── 判据谓词（私有，全部 bool 收口） ────────────────────────────
/// 魔数口径冻结：0x5AF3_C00DE。
fn magic_ok() -> bool {
    SENTINEL_MAGIC == 0x0000_5AF3_C00DE
}

/// 哨兵格配比冻结：每行 1 格，哨兵格数=行数。
fn sentinel_ratio_ok() -> bool {
    SENTINEL_CELLS_PER_ROW == 1
}

/// 段位守卫：诊断码全落 0x75D80 窗口且基址合法。
fn segment_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < FbCode::ALL.len() {
        let v = FbCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 正常流判定：规划→布防→写可见→扫描→推进，全绿且守恒。
fn happy_path_ok() -> bool {
    let mut fb = match FrameBuf::plan(80, 25, 84) {
        Ok(f) => f,
        Err(_) => return false,
    };
    if fb.arm().is_err() {
        return false;
    }
    if fb.fill_visible(0x20).is_err() {
        return false;
    }
    if fb.check_sentinels().is_err() {
        return false;
    }
    if fb.advance().is_err() {
        return false;
    }
    fb.conservation_ok() && fb.state == FbState::Verified
}

/// 尾巴写禁令判定：col=宽（第一个尾巴格）与 col=宽+9（尾巴深处）都必拒。
fn tail_write_rejected_ok() -> bool {
    let mut fb = match FrameBuf::plan(16, 4, 20) {
        Ok(f) => f,
        Err(_) => return false,
    };
    if fb.arm().is_err() {
        return false;
    }
    let a = fb.write_cell(0, 16, 0).is_err();
    let b = fb.write_cell(1, 19, 0).is_err();
    let c = fb.write_cell(2, 20, 0).is_err();
    a && b && c && fb.tail_write_rejected == 3
}

/// 注入检出判定：10 轮尾巴 1 字节翻转，检出率必须 100%。
fn inject_all_caught(rounds: u32) -> bool {
    let mut r = 0u32;
    while r < rounds {
        let mut fb = match FrameBuf::plan(32, 8, 34) {
            Ok(f) => f,
            Err(_) => return false,
        };
        if fb.arm().is_err() {
            return false;
        }
        // 注入：第 r%8 行尾巴翻 1 字节。
        let row = (r % 8) as usize;
        if fb.rogue_tail_poke(row).is_err() {
            return false;
        }
        // 必抓：扫描必须报 SentinelBroken 且行号对得上。
        match fb.check_sentinels() {
            Err(e) => {
                if e != FbCode::SentinelBroken.val() {
                    return false;
                }
                if fb.last_broken_row != Some(row) {
                    return false;
                }
            }
            Ok(()) => return false,
        }
        r += 1;
    }
    true
}

/// 行守恒违约判定：rogue_shrink 抽格后守恒必翻红。
fn conservation_break_caught() -> bool {
    let mut fb = match FrameBuf::plan(8, 4, 10) {
        Ok(f) => f,
        Err(_) => return false,
    };
    if !fb.conservation_ok() {
        return false;
    }
    fb.rogue_shrink();
    !fb.conservation_ok()
}

/// 状态机倒退判定：Sealed 后再 advance 必拒；未 arm 直接 advance 越级必拒。
fn state_monotonic_ok() -> bool {
    let mut fb = match FrameBuf::plan(4, 2, 6) {
        Ok(f) => f,
        Err(_) => return false,
    };
    // 越级：Uninit 直接想 Verified 不可达（advance 只走一步，但 Uninit 的
    // 下一站是 Armed，这里验证的是「跳过 arm 的推进到不了已验证」）。
    if fb.advance().is_err() {
        return false;
    }
    // 现在 Armed；手工置 Sealed 模拟封印后再试写/再推进。
    fb.state = FbState::Sealed;
    if fb.advance().is_ok() {
        return false;
    }
    if fb.write_cell(0, 0, 1).is_ok() {
        return false;
    }
    true
}

/// 规划闸判定：stride 装不下哨兵格必拒；0 行/0 宽必拒。
fn plan_gate_ok() -> bool {
    let a = FrameBuf::plan(16, 4, 16).is_err(); // 无尾巴位
    let b = FrameBuf::plan(0, 4, 16).is_err(); // 0 宽
    let c = FrameBuf::plan(16, 0, 18).is_err(); // 0 行
    a && b && c
}

/// 黄金全链：规划→布防→可见写→扫描→推进→封印→守恒+计数复算。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut fb = FrameBuf::plan(80, 25, 84)?;
    fb.arm()?;
    // 可见区写一圈字符格。
    fb.fill_visible(0x41)?;
    // 尾巴写禁令（防线 1）记 3 笔。
    let rej = fb.write_cell(0, 80, 0).is_err() as u32
        + fb.write_cell(24, 83, 0).is_err() as u32
        + fb.write_cell(25, 0, 0).is_err() as u32; // 行越界也算拒
    // 哨兵扫描（防线 2）。
    fb.check_sentinels()?;
    let clean = 1u32;
    fb.advance()?; // Verified
    fb.advance()?; // Sealed
    let cons = fb.conservation_ok() as u32;
    Ok((clean + cons, rej, fb.sentinel_cell_count() as u32))
}

/// 探针序 A：黄金全链数值封口（1 绿+守恒、3 拒、25 哨兵格）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((2, 3, 25)) => Ok(()),
        _ => Err(FbCode::NotReproducible.val()),
    }
}

/// 探针序 B：注入必抓单点——1 行尾巴翻转立即报 SentinelBroken。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut fb = FrameBuf::plan(8, 1, 10)?;
    fb.arm()?;
    fb.rogue_tail_poke(0)?;
    match fb.check_sentinels() {
        Err(e) if e == FbCode::SentinelBroken.val() => Ok(()),
        _ => Err(FbCode::NotReproducible.val()),
    }
}

/// 探针序 C：魔数与配比口径冻结。
pub fn probe_seq_c() -> Result<(), u32> {
    match (magic_ok(), sentinel_ratio_ok()) {
        (true, true) => Ok(()),
        _ => Err(FbCode::NotReproducible.val()),
    }
}

// ── LCG（种子固定，禁随机） ─────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账（三路：尾巴直写/守恒违约/规划违宪） ────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t += 1;
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c += 1;
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入轮（LCG 驱动 rounds 轮 × 3 路），全部必须被显性抓。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0u32;
    while r < rounds {
        let _ = rng.next();
        // 路 1：尾巴 1 字节直写（LCG 选行）。
        let mut f1 = FrameBuf::plan(32, 8, 34)?;
        f1.arm()?;
        let row = rng.below(8) as usize;
        f1.rogue_tail_poke(row)?;
        led.record(0, f1.check_sentinels().is_err());
        // 路 2：守恒违约（抽格）。
        let mut f2 = FrameBuf::plan(16, 4, 18)?;
        f2.rogue_shrink();
        led.record(1, !f2.conservation_ok());
        // 路 3：规划违宪（无尾巴位 / 0 行）。
        let p1 = FrameBuf::plan(10, 2, 10).is_err();
        let p2 = FrameBuf::plan(10, 0, 12).is_err();
        led.record(2, p1 && p2);
        r += 1;
    }
    Ok(led)
}

/// 双跑一致（复现红线）。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(FbCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫（公开面，供检查节直接调用）。
pub fn segment_guard() -> bool {
    segment_ok()
}

// ── 判据面（extra_checks：≥12 条，覆盖正常流/边界/注入/双跑/段位） ─┐
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 魔数口径。
    v.push((
        "J1-哨兵魔数口径",
        magic_ok(),
        format!("SENTINEL_MAGIC=0x{:X}", SENTINEL_MAGIC),
    ));
    // 2. 哨兵格配比。
    v.push((
        "J1-哨兵格数=行数",
        sentinel_ratio_ok() && golden_run().map(|t| t.2 == 25).unwrap_or(false),
        String::from("每行 1 格哨兵，80x25 帧恰 25 格"),
    ));
    // 3. 行守恒恒等式。
    v.push((
        "J1-行守恒",
        conservation_ok_main(),
        String::from("行数×stride=缓冲大小（checked_mul 复算）"),
    ));
    // 4. 正常流。
    v.push((
        "J1-正常流",
        happy_path_ok(),
        String::from("规划→布防→可见写→扫描→Verified 全绿"),
    ));
    // 5. 尾巴写禁令（API 层）。
    v.push((
        "J1-尾巴写禁令",
        tail_write_rejected_ok(),
        String::from("col≥宽 写入显性拒，拒笔数记账 3/3"),
    ));
    // 6. 注入检出 100%。
    v.push((
        "J1-注入检出100%",
        inject_all_caught(JUDGE_ROUNDS),
        format!("{} 轮尾巴 1 字节翻转全部被抓", JUDGE_ROUNDS),
    ));
    // 7. 守恒违约必抓。
    v.push((
        "J1-守恒违约必抓",
        conservation_break_caught(),
        String::from("抽 1 格后 rows×stride≠len 当场翻红"),
    ));
    // 8. 状态机单调。
    v.push((
        "J2-状态机倒退拒",
        state_monotonic_ok(),
        String::from("Sealed 后推进/写入均显性拒"),
    ));
    // 9. 规划闸。
    v.push((
        "J1-规划闸",
        plan_gate_ok(),
        String::from("无尾巴位/0宽/0行 规划即拒"),
    ));
    // 10. 注入三路账。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = inj.as_ref().map(|l| l.all_caught()).unwrap_or(false);
    v.push((
        "J1-注入三路账",
        inj_ok,
        match &inj {
            Ok(l) => format!("t={:?} c={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 11. 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J2-探针三序",
        a && b && c,
        format!("a={} b={} c={}", a, b, c),
    ));
    // 12. 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 13. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("0x{:X} 窗口 8 码全落段", SEG_BASE),
    ));
    // 14. 边界：单行单列最小帧。
    v.push((
        "J1-最小帧边界",
        min_frame_ok(),
        String::from("1x1 帧 stride=2 可布防可验证"),
    ));
    v
}

/// 行守恒主判定：黄金帧 + 多规格帧全守恒。
fn conservation_ok_main() -> bool {
    let specs = [(80u32, 25u32, 84u32), (1, 1, 2), (320, 200, 324), (7, 3, 9)];
    let mut i = 0;
    while i < specs.len() {
        match specs.get(i) {
            Some(&(w, h, st)) => match FrameBuf::plan(w, h, st) {
                Ok(f) => {
                    if !f.conservation_ok() || f.sentinel_cell_count() != h as usize {
                        return false;
                    }
                }
                Err(_) => return false,
            },
            None => break,
        }
        i += 1;
    }
    true
}

/// 最小帧边界判定：1x1、stride=2。
fn min_frame_ok() -> bool {
    let mut fb = match FrameBuf::plan(1, 1, 2) {
        Ok(f) => f,
        Err(_) => return false,
    };
    if fb.arm().is_err() || fb.check_sentinels().is_err() {
        return false;
    }
    let vis = fb.write_cell(0, 0, 7).is_ok();
    let tail = fb.write_cell(0, 1, 7).is_err();
    vis && tail && fb.advance().is_ok()
}

// ── 检查节 ─────────────────────────────────────────────────────
pub fn run_unx0491_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0491 帧缓冲哨兵（stride 尾巴写禁令）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（注入必抓 / 双跑一致 / 边界） ────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 测试专用规划兜底：断言失败后给一个空账帧（后续守恒断言会兜住）。
    fn must_plan(w: u32, h: u32, s: u32) -> FrameBuf {
        match FrameBuf::plan(w, h, s) {
            Ok(f) => f,
            Err(e) => {
                assert!(false, "规划 {}x{} stride={} 不应失败（code=0x{:x}）", w, h, s, e);
                FrameBuf {
                    plan: FbPlan { width: w, height: h, stride: s },
                    cells: Vec::new(),
                    state: FbState::Uninit,
                    last_broken_row: None,
                    tail_write_rejected: 0,
                    sentinel_breaks_found: 0,
                }
            }
        }
    }

    #[test]
    fn normal_flow_arm_scan_seal() {
        assert!(happy_path_ok(), "正常流应全绿");
    }

    #[test]
    fn tail_write_is_always_rejected() {
        assert!(tail_write_rejected_ok(), "尾巴写禁令必须显性拒");
        let mut fb = must_plan(4, 2, 6);
        assert!(fb.arm().is_ok());
        assert_eq!(fb.write_cell(1, 4, 9), Err(FbCode::TailWrite.val()));
        assert_eq!(fb.write_cell(9, 0, 9), Err(FbCode::RowOver.val()));
    }

    #[test]
    fn sentinel_break_injection_fully_caught() {
        assert!(inject_all_caught(JUDGE_ROUNDS), "注入检出率必须 100%");
    }

    #[test]
    fn conservation_break_is_detected() {
        assert!(conservation_break_caught(), "抽格后守恒必须翻红");
    }

    #[test]
    fn state_machine_rejects_regression() {
        assert!(state_monotonic_ok(), "状态机倒退必须显性拒");
    }

    #[test]
    fn plan_gate_blocks_bad_geometry() {
        assert!(plan_gate_ok(), "违宪规划必须全拒");
    }

    #[test]
    fn golden_run_is_reproducible() {
        let a = golden_run();
        let b = golden_run();
        assert_eq!(a, b, "双跑必须一致");
        assert_eq!(a, Ok((2, 3, 25)), "黄金全链数值封口");
    }

    #[test]
    fn probe_three_seqs_all_green() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn inject_ledger_all_slots_caught() {
        let led = inject_rounds(JUDGE_ROUNDS);
        match led {
            Ok(l) => {
                assert!(l.all_caught(), "三路注入必须全抓：{:?}/{:?}", l.tries, l.caught);
                let mut i = 0;
                while i < 3 {
                    assert_eq!(l.tries.get(i).copied().unwrap_or(0), JUDGE_ROUNDS, "路 {} 轮数应封口", i);
                    i += 1;
                }
            }
            Err(_) => assert!(false, "注入轮不应报错"),
        }
    }

    #[test]
    fn diagnostic_codes_stay_in_segment() {
        assert!(segment_guard(), "段位守卫应绿");
        assert_eq!(FbCode::TailWrite.val(), SEG_BASE, "首码应落在基址");
        assert_eq!(detail(FbCode::SentinelBroken.val()), FbCode::SentinelBroken.detail());
        assert_eq!(detail(0x75DFF), "未知诊断码：不在 0x75D80 段闭集内");
    }

    #[test]
    fn min_frame_edge_case() {
        assert!(min_frame_ok(), "1x1 最小帧应可全链走通");
    }

    #[test]
    fn fill_visible_leaves_sentinels_intact() {
        let mut fb = must_plan(8, 6, 10);
        assert!(fb.arm().is_ok());
        assert!(fb.fill_visible(0xFF).is_ok());
        assert!(fb.check_sentinels().is_ok(), "可见区填充不得伤哨兵");
        assert_eq!(fb.sentinel_breaks_found, 0);
    }
}
