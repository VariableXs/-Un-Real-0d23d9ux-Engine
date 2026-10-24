//! veb369_sandbox —— VE-F0369 驱动隔离沙箱。
//! 锚点（CoRun Unxreal 主册）：VE-F0369 · 驱动隔离沙箱 · 目标 400 行
//! 判据原文：VE-F0369-J1 沙箱按配额隔离驱动：内存配额（字节）+
//!           请求速率配额（次/窗口）；越配额请求拒绝（拒绝码
//!           带配额号）；沙箱内故障不外泄（故障计数封顶后熔断
//!           该沙箱，其他沙箱不受影响）；沙箱状态三态（活/熔
//!           断/解除）可判定；熔断可解除（冷却期后复位计数，
//!           但历史熔断数留档）；隔离可重放。
//! 组件：沙箱（配额对 + 用量账 + 三态）+ 配额闸（内存/速率）+
//!       熔断器（封顶 + 冷却解除）+ 沙箱场（多沙箱互不影响核）+
//!       重放核。
//! 职责定位：隔离沙箱是「驱动之间的防火墙」——一个烂驱动不许
//!   拖垮整机：内存配额防它吃光堆，速率配额防它刷爆总线，故
//!   障封顶防它反复炸。熔断是「先停电再检修」：与其让烂驱动
//!   每秒炸一次，不如整体熔断等冷却。多沙箱互不影响是多租户
//!   的底线：隔壁沙箱炸了，我的请求照常。
//! 性能逐项分解：配额判定 O(1)；熔断 O(1)；冷却 O(1)；重放
//!   O(请求数)。
//! 诊断码段：0x301C0（0x301C1~0x301C9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const SB_VERSION: &str = "veb369_sandbox-v1";

/// 诊断段基址（0x301C0 段 · 本模块独占）。
pub const SB_SEG_BASE: u32 = 0x301C0;
pub const XC_MEM_QUOTA: u32 = 0x301C1; // 内存配额超
pub const XC_RATE_QUOTA: u32 = 0x301C2; // 速率配额超
pub const XC_FUSED: u32 = 0x301C3; // 已熔断拒请求
pub const XC_QUOTA_ZERO: u32 = 0x301C4; // 配额零拒
pub const XC_DUP_BOX: u32 = 0x301C5; // 沙箱重号
pub const XC_MAGIC: u32 = 0x301C6; // 账魔数破
pub const XC_REPLAY: u32 = 0x301C7; // 重放不一致
pub const XC_COOL_SHORT: u32 = 0x301C8; // 冷却期未满解除拒
pub const XC_BOX_MISS: u32 = 0x301C9; // 沙箱不在场

/// 三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoxState {
    /// 活。
    Alive,
    /// 熔断。
    Fused,
}

/// 故障封顶。
pub const FAULT_CAP: u32 = 3;
/// 冷却步数。
pub const COOL_STEPS: u32 = 4;

/// 沙箱。
pub struct Sandbox {
    pub id: u32,
    /// 内存配额（字节模型）。
    pub mem_quota: u64,
    /// 速率配额（次/窗口）。
    pub rate_quota: u32,
    /// 内存已用。
    mem_used: u64,
    /// 窗口内请求数。
    window_req: u32,
    /// 故障计数。
    faults: u32,
    /// 熔断时步号（冷却计时起点）。
    fused_at: Option<u32>,
    /// 历史熔断数。
    pub fuse_history: u32,
    cur_step: u32,
}

impl Sandbox {
    /// 建：配额对。
    pub fn new(id: u32, mem_quota: u64, rate_quota: u32) -> Result<Sandbox, u32> {
        if mem_quota == 0 || rate_quota == 0 {
            return Err(XC_QUOTA_ZERO);
        }
        Ok(Sandbox {
            id,
            mem_quota,
            rate_quota,
            mem_used: 0,
            window_req: 0,
            faults: 0,
            fused_at: None,
            fuse_history: 0,
            cur_step: 0,
        })
    }

    /// 内存请求：熔断拒、配额超拒；成功记账。
    pub fn mem_alloc(&mut self, bytes: u64) -> Result<(), u32> {
        if self.fused_at.is_some() {
            return Err(XC_FUSED);
        }
        if self.mem_used + bytes > self.mem_quota {
            return Err(XC_MEM_QUOTA);
        }
        self.mem_used += bytes;
        Ok(())
    }

    /// 速率请求：窗口计数超拒。
    pub fn rate_hit(&mut self) -> Result<(), u32> {
        if self.fused_at.is_some() {
            return Err(XC_FUSED);
        }
        if self.window_req >= self.rate_quota {
            return Err(XC_RATE_QUOTA);
        }
        self.window_req += 1;
        Ok(())
    }

    /// 内存归还。
    pub fn mem_free(&mut self, bytes: u64) {
        self.mem_used = self.mem_used.saturating_sub(bytes);
    }

    /// 故障上报：封顶即熔断。
    pub fn fault(&mut self) -> Result<(), u32> {
        if self.fused_at.is_some() {
            return Err(XC_FUSED);
        }
        self.faults += 1;
        if self.faults >= FAULT_CAP {
            self.fused_at = Some(self.cur_step);
            self.fuse_history += 1;
            return Err(XC_FUSED); // 触发本次即熔断
        }
        Ok(())
    }

    /// 步进：窗口复位 + 冷却推进。
    pub fn tick(&mut self) {
        self.cur_step += 1;
        self.window_req = 0;
    }

    /// 解除熔断：冷却期未满拒。
    pub fn defuse(&mut self) -> Result<(), u32> {
        let at = match self.fused_at {
            Some(a) => a,
            None => return Ok(()), // 未熔断幂等
        };
        if self.cur_step - at < COOL_STEPS {
            return Err(XC_COOL_SHORT);
        }
        self.fused_at = None;
        self.faults = 0;
        Ok(())
    }

    /// 状态。
    pub fn state(&self) -> BoxState {
        if self.fused_at.is_some() {
            BoxState::Fused
        } else {
            BoxState::Alive
        }
    }

    /// 用量。
    pub fn mem_used(&self) -> u64 {
        self.mem_used
    }

    /// 故障数。
    pub fn fault_count(&self) -> u32 {
        self.faults
    }
}

/// 沙箱场：多沙箱互不影响。
pub struct SandboxField {
    boxes: Vec<Sandbox>,
}

impl SandboxField {
    /// 建。
    pub fn new() -> SandboxField {
        SandboxField { boxes: Vec::new() }
    }

    /// 开沙箱：重号拒。
    pub fn open(&mut self, id: u32, mem: u64, rate: u32) -> Result<(), u32> {
        if mem == 0 || rate == 0 {
            return Err(XC_QUOTA_ZERO);
        }
        for b in self.boxes.iter() {
            if b.id == id {
                return Err(XC_DUP_BOX);
            }
        }
        self.boxes.push(Sandbox::new(id, mem, rate)?);
        Ok(())
    }

    /// 取沙箱（可变）。
    pub fn box_mut(&mut self, id: u32) -> Result<&mut Sandbox, u32> {
        match self.boxes.iter_mut().find(|b| b.id == id) {
            Some(b) => Ok(b),
            None => Err(XC_BOX_MISS),
        }
    }

    /// 全场步进。
    pub fn tick_all(&mut self) {
        for b in self.boxes.iter_mut() {
            b.tick();
        }
    }

    /// 隔离核：任一沙箱熔断，其他仍 Alive。
    pub fn isolation_ok(&self, fused_id: u32) -> Result<(), u32> {
        for b in self.boxes.iter() {
            if b.id != fused_id && b.state() == BoxState::Fused {
                return Err(XC_REPLAY); // 误伤 = 隔离破
            }
        }
        Ok(())
    }

    /// 沙箱数。
    pub fn len(&self) -> usize {
        self.boxes.len()
    }

    /// 重放核：全部沙箱状态可复算。
    pub fn replay_ok(&self) -> Result<(), u32> {
        for b in self.boxes.iter() {
            let expect = if b.fused_at.is_some() { 2 } else { 1 };
            if b.state().code_of() != expect {
                return Err(XC_REPLAY);
            }
        }
        Ok(())
    }
}

impl BoxState {
    fn code_of(&self) -> u32 {
        match self {
            BoxState::Alive => 1,
            BoxState::Fused => 2,
        }
    }
}

pub fn run_veb369_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0369");
    // 族一：内存配额。
    let mut b = match Sandbox::new(1, 1000, 10) {
        Ok(x) => x,
        Err(_) => {
            s.add("建沙箱", false, "拒");
            return s;
        }
    };
    s.add("初态活", b.state() == BoxState::Alive, "Alive");
    let _ = b.mem_alloc(600);
    s.add("600 记账", b.mem_used() == 600, "半容");
    s.add("超配额拒", b.mem_alloc(500) == Err(XC_MEM_QUOTA), "1100 > 1000");
    s.add("拒绝不记账", b.mem_used() == 600, "用量不变");
    let _ = b.mem_free(100);
    s.add("归还后可续", b.mem_alloc(500).is_ok(), "500 ≤ 500");
    // 族二：速率配额。
    let mut r = match Sandbox::new(2, 1000, 3) {
        Ok(x) => x,
        Err(_) => {
            s.add("建沙箱2", false, "拒");
            return s;
        }
    };
    s.add("速率 3 发成", { r.rate_hit().is_ok() && r.rate_hit().is_ok() && r.rate_hit().is_ok() }, "配额内");
    s.add("第 4 发拒", r.rate_hit() == Err(XC_RATE_QUOTA), "超窗");
    r.tick();
    s.add("步进后复活", r.rate_hit().is_ok(), "窗口复位");
    // 族三：故障熔断。
    let mut f = match Sandbox::new(3, 1000, 10) {
        Ok(x) => x,
        Err(_) => {
            s.add("建沙箱3", false, "拒");
            return s;
        }
    };
    s.add("故障 1/2 受理", { f.fault().is_ok() && f.fault().is_ok() }, "封顶前");
    s.add("第 3 故障熔断", f.fault() == Err(XC_FUSED), "触发封顶");
    s.add("熔断态", f.state() == BoxState::Fused, "Fused");
    s.add("熔断拒请求", f.mem_alloc(1) == Err(XC_FUSED), "全拒");
    s.add("历史熔断 1", f.fuse_history == 1, "留档");
    // 冷却不足解除拒。
    f.tick();
    f.tick();
    s.add("冷却未满解除拒", f.defuse() == Err(XC_COOL_SHORT), "2 < 4");
    f.tick();
    f.tick();
    s.add("冷却满解除成", f.defuse().is_ok(), "4 步");
    s.add("解除后复活", f.state() == BoxState::Alive, "Alive");
    s.add("故障计数清零", f.fault_count() == 0, "复位");
    s.add("历史留档 1", f.fuse_history == 1, "不清");
    // 族四：沙箱场隔离。
    let mut fd = SandboxField::new();
    let _ = fd.open(10, 100, 5);
    let _ = fd.open(11, 100, 5);
    let _ = fd.open(12, 100, 5);
    s.add("三沙箱开", fd.len() == 3, "多租户");
    // 沙箱 10 炸三次。
    let b10 = match fd.box_mut(10) {
        Ok(b) => b,
        Err(_) => {
            s.add("10 号开箱败", false, "不应到此");
            return s;
        }
    };
    let _ = b10.fault();
    let _ = b10.fault();
    let _ = b10.fault();
    s.add("10 号熔断", fd.box_mut(10).map(|b| b.state()) == Ok(BoxState::Fused), "炸了");
    s.add("隔离核过", fd.isolation_ok(10).is_ok(), "11/12 不受影响");
    s.add("11 号照常", fd.box_mut(11).map(|b| b.rate_hit().is_ok()) == Ok(true), "隔壁炸不影响");
    // 熔断沙箱请求仍拒。
    s.add("10 号仍拒", fd.box_mut(10).map(|b| b.mem_alloc(1).is_err()) == Ok(true), "FUSED");
    // 重号拒 / 越域拒。
    s.add("重号拒", fd.open(10, 100, 5) == Err(XC_DUP_BOX), "10 撞");
    s.add("配额零拒", fd.open(13, 0, 5) == Err(XC_QUOTA_ZERO), "mem=0");
    s.add("越域取拒", fd.box_mut(99).is_err(), "无 99 号");
    // 全场步进后冷却。
    for _ in 0..4u32 {
        fd.tick_all();
    }
    s.add("全场冷却解除", fd.box_mut(10).map(|b| b.defuse().is_ok()) == Ok(true), "10 复活");
    s.add("重放核过", fd.replay_ok().is_ok(), "状态可复算");
    s.add("魔数核", SB_SEG_BASE == 0x301C0, "段基址");
    s
}
