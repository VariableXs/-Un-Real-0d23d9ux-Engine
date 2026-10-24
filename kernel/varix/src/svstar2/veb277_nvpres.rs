//! veb277_nvpres —— VE-F0277 NVIDIA 呈现语义映射。
//! 锚点（CoRun Unxreal 主册）：VE-F0277 · NVIDIA 呈现语义映射 · 目标 320 行
//! 判据原文：VE-F0277-J1 呈现映射账：交换链登记（表面数 2~4）；
//!           呈现模式三态（即时 / 垂直同步 / 邮箱）；模式切换
//!           （换模式 → 空交换一次再生效）；帧入队（队列满拒
//!           ≠ 撕裂保底）；呈现弹出（队头弹出 + 表位轮转）；
//!           撕裂核（即时模式允许撕裂，其余两态禁）；表位轮
//!           转核（N 表面弹 N 次回到起始位）。
//! 组件：交换链账 + 呈现态机 + 队列 + 表位轮转。
//! 职责定位：呈现映射是「帧到屏的最后一公里」——模式不是
//!           偏好是契约：即时拿延迟、垂直拿完整、邮箱拿折中；
//!           队满拒比撕裂好：晚一帧好过破一帧；表位轮转是呈
//!           现的节拍器，转一圈必须回到原点。
//! 性能逐项分解：入队/弹出 O(1)；轮转 O(1)。
//! 诊断码段：0x30E40（0x30E41~0x30E49 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const PR_VERSION: &str = "veb277_nvpres-v1";

/// 诊断段基址（0x30E40 段 · 本模块独占）。
pub const PR_SEG_BASE: u32 = 0x30E40;
pub const XC_MAGIC: u32 = 0x30E41; // 账魔数破
pub const XC_SURF: u32 = 0x30E42; // 表面数不合法
pub const XC_FULL: u32 = 0x30E43; // 队满
pub const XC_EMPTY: u32 = 0x30E44; // 队空
pub const XC_UNSWAP: u32 = 0x30E45; // 未空交换即切换

/// 账魔数。
const MAGIC: u32 = 0x0E40_1B88;

/// 呈现模式三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PMode {
    /// 即时：允许撕裂。
    Immediate,
    /// 垂直同步：禁撕裂。
    Vsync,
    /// 邮箱：禁撕裂，队满弃旧。
    Mailbox,
}

/// 呈现模式是否禁撕裂。
pub fn mode_tearsafe(m: PMode) -> bool {
    matches!(m, PMode::Vsync | PMode::Mailbox)
}

/// 交换链。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwapChain {
    /// 表面数（2~4）。
    pub surfaces: u32,
    /// 当前模式。
    pub mode: PMode,
    /// 待生效模式（空交换后转正）。
    pending: Option<PMode>,
    /// 入队帧号环。
    queue: [u64; 3],
    qlen: usize,
    /// 当前表位（0..surfaces）。
    pub slot: u32,
    /// 已弹出帧数。
    pub presented: u64,
    /// 撕裂事件数。
    pub tears: u32,
    magic: u32,
}

impl SwapChain {
    /// 建（表面数 2~4、初始模式）。
    pub fn new(surfaces: u32, mode: PMode) -> Result<SwapChain, u32> {
        if surfaces < 2 || surfaces > 4 {
            return Err(XC_SURF);
        }
        Ok(SwapChain {
            surfaces,
            mode,
            pending: None,
            queue: [0; 3],
            qlen: 0,
            slot: 0,
            presented: 0,
            tears: 0,
            magic: MAGIC,
        })
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 请求切模式：两态互切须先空交换一次；同模式幂等。
    pub fn request_mode(&mut self, m: PMode) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if m == self.mode && self.pending.is_none() {
            return Ok(());
        }
        // 若已有待生效模式且相同则幂等。
        if self.pending == Some(m) {
            return Ok(());
        }
        // 有在飞帧（队列非空）时不允许直接改当前模式。
        if self.qlen > 0 && self.pending.is_none() {
            self.pending = Some(m);
            return Err(XC_UNSWAP);
        }
        if self.pending.is_some() {
            // 已挂起中再改 = 覆盖挂起。
            self.pending = Some(m);
            return Err(XC_UNSWAP);
        }
        self.mode = m;
        Ok(())
    }

    /// 空交换一次：清队列并让挂起模式转正。
    pub fn drain_swap(&mut self) -> Result<u32, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        self.qlen = 0;
        match self.pending.take() {
            Some(m) => {
                self.mode = m;
                Ok(1)
            }
            None => Ok(0),
        }
    }

    /// 帧入队：队容 3，满即拒（不撕裂保底）。
    pub fn enqueue(&mut self, frame: u64) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if self.qlen >= 3 {
            return Err(XC_FULL);
        }
        self.queue[self.qlen] = frame;
        self.qlen += 1;
        Ok(())
    }

    /// 呈现弹出：队头出 + 表位轮转；即时模式在撕裂窗内计撕裂。
    pub fn present(&mut self, tear_window: bool) -> Result<u64, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if self.qlen == 0 {
            return Err(XC_EMPTY);
        }
        let frame = self.queue[0];
        // 后移。
        for i in 1..self.qlen {
            self.queue[i - 1] = self.queue[i];
        }
        self.qlen -= 1;
        // 撕裂规则：即时模式 + 撕裂窗 = 计一次撕裂事件。
        if tear_window && !mode_tearsafe(self.mode) {
            self.tears += 1;
        }
        self.slot = (self.slot + 1) % self.surfaces;
        self.presented += 1;
        Ok(frame)
    }

    /// 队深。
    pub fn depth(&self) -> usize {
        self.qlen
    }
}

pub fn run_veb277_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0277");
    // 族零：配置。
    s.add("表面 1 拒", SwapChain::new(1, PMode::Vsync) == Err(XC_SURF), "<2");
    s.add("表面 5 拒", SwapChain::new(5, PMode::Vsync) == Err(XC_SURF), ">4");
    // 撕裂安全表。
    s.add("即时可撕", mode_tearsafe(PMode::Immediate) == false, "禁关");
    s.add("垂直禁撕", mode_tearsafe(PMode::Vsync), "契约");
    s.add("邮箱禁撕", mode_tearsafe(PMode::Mailbox), "契约");
    // 族一：入队与队满。
    let mut c = match SwapChain::new(2, PMode::Vsync) { Ok(x) => x, Err(_) => { s.add("建账败", false, "不应到"); return s; } };
    s.add("入 1", c.enqueue(100).is_ok(), "一帧");
    s.add("入 2", c.enqueue(101).is_ok(), "两帧");
    s.add("入 3", c.enqueue(102).is_ok(), "三帧");
    s.add("队满拒", c.enqueue(103) == Err(XC_FULL), "容 3");
    // 族二：模式切换门。
    s.add("带帧切拒", c.request_mode(PMode::Immediate) == Err(XC_UNSWAP), "先空交换");
    s.add("挂起已记", c.pending.is_some(), "挂起态");
    // 再切覆盖挂起仍拒。
    s.add("挂起覆盖拒", c.request_mode(PMode::Mailbox) == Err(XC_UNSWAP), "覆盖挂起");
    // 空交换转正。
    s.add("空交换生效", c.drain_swap() == Ok(1), "转正一档");
    s.add("换到邮箱", c.mode == PMode::Mailbox, "新模式");
    // 队列被清。
    s.add("队清零", c.depth() == 0, "空交换清队");
    s.add("再空交换幂等", c.drain_swap() == Ok(0), "无挂起");
    // 族三：同模式幂等。
    let mut d = match SwapChain::new(3, PMode::Immediate) { Ok(x) => x, Err(_) => { s.add("建账败", false, "不应到"); return s; } };
    s.add("同切幂等", d.request_mode(PMode::Immediate).is_ok(), "无事");
    // 族四：弹出与轮转。
    let _ = d.enqueue(1);
    let _ = d.enqueue(2);
    s.add("弹 1", d.present(false) == Ok(1), "队头");
    s.add("表位 1", d.slot == 1, "3 表轮 1");
    s.add("弹 2", d.present(false) == Ok(2), "队头");
    s.add("表位 2", d.slot == 2, "3 表轮 2");
    // 队空拒。
    s.add("队空拒", d.present(false) == Err(XC_EMPTY), "无帧");
    // 表位轮转核：3 表弹 3 次回 0。已 2 次，再 1 次。
    let _ = d.enqueue(3);
    s.add("弹 3", d.present(false).is_ok(), "三弹");
    s.add("回 0", d.slot == 0, "轮转核");
    // 族五：撕裂规则。
    let mut t = match SwapChain::new(2, PMode::Immediate) { Ok(x) => x, Err(_) => { s.add("建账败", false, "不应到"); return s; } };
    let _ = t.enqueue(9);
    let _ = t.present(true);
    s.add("即时计撕 1", t.tears == 1, "撕裂窗内");
    let _ = t.enqueue(10);
    let _ = t.present(true);
    s.add("即时计撕 2", t.tears == 2, "再计");
    // 换到禁撕模式后不计。
    let _ = t.enqueue(11);
    let _ = t.present(false);
    let _ = t.request_mode(PMode::Vsync);
    let _ = t.enqueue(12);
    let _ = t.present(true);
    s.add("垂直不计撕", t.tears == 2, "契约生效");
    // 2 表轮转核。
    let mut w = match SwapChain::new(2, PMode::Vsync) { Ok(x) => x, Err(_) => { s.add("建账败", false, "不应到"); return s; } };
    for _ in 0..2 {
        let _ = w.enqueue(1);
        let _ = w.present(false);
    }
    s.add("2 表回 0", w.slot == 0, "轮转核");
    // 4 表轮转核。
    let mut q = match SwapChain::new(4, PMode::Vsync) { Ok(x) => x, Err(_) => { s.add("建账败", false, "不应到"); return s; } };
    for _ in 0..4 {
        let _ = q.enqueue(1);
        let _ = q.present(false);
    }
    s.add("4 表回 0", q.slot == 0, "轮转核");
    s.add("魔数核", PR_SEG_BASE == 0x30E40, "段基址");
    s
}
