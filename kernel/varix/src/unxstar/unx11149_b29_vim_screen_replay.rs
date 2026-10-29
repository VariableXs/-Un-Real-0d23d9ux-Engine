//! UNX-F11149 B29 清账（vim 全屏联测判据终重放）（0x1f310 段）
//!
//! 纯功能模型：八个全屏联测场景各自按「进入 → 动作 → 落定」三步跑一遍，
//! 共二十四个终态。每步的终态只由（场景, 步号）唯一确定——模式指示器与
//! 脏位必须与规则表逐条相符，否则即为联测失真。
//!
//! 第二本账是「全链账」：联测脚手架十个环节（播种 → 起进程 → 喂键 →
//! 抓屏 → 验光标 → 验模式 → 验消息 → 验脏位 → 复原 → 收尾）必须按序
//! 各记一次，且环摘要按前环递推。链上一环缺失或摘要断链，全链账即不齐。
//!
//! 规则面：
//!   - 进入步（0）与落定步（2）一律回到普通模式；
//!   - 动作步（1）的模式由场景决定：插入 / 可视 / 命令行 / 普通；
//!   - 脏位只由场景决定，插入与粘贴两场景落脏，其余不落脏；
//!   - 模式转移只允许规则表列出的边，越界即失真。
//!
//! 拒止面：场景号或步号越界、重复登记、例序倒挂、期望模式越界、
//! 未登记即观测、缺测、不符、链账断环、冻结后再动账。

use crate::alloc::format;
use crate::checks::CheckSet;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f310;
/// 场景号或步号越界
pub const VM_ESCENE: u32 = SEG_BASE + 0x01;
/// 重复登记
pub const VM_EDUP: u32 = SEG_BASE + 0x02;
/// 例序倒挂
pub const VM_EORDER: u32 = SEG_BASE + 0x03;
/// 期望模式越界
pub const VM_ENOEXP: u32 = SEG_BASE + 0x04;
/// 未登记即观测
pub const VM_EUNREG: u32 = SEG_BASE + 0x05;
/// 未测即重放
pub const VM_ESHORT: u32 = SEG_BASE + 0x06;
/// 实测与期望不符
pub const VM_EMISMATCH: u32 = SEG_BASE + 0x07;
/// 链账断环或摘要不递推
pub const VM_ECHAIN: u32 = SEG_BASE + 0x08;
/// 已封盘
pub const VM_EFROZEN: u32 = SEG_BASE + 0x09;
/// 模式转移越界
pub const VM_EMODE: u32 = SEG_BASE + 0x0A;

/// 场景数。
pub const SCENES: usize = 8;
/// 场景名。
pub const SCENE_NAMES: [&str; SCENES] = [
    "open_file",
    "insert_text",
    "normal_motion",
    "visual_yank",
    "put_after",
    "search_jump",
    "split_window",
    "quit_all",
];
/// 模式数。
pub const MODES: usize = 4;
/// 模式名。
pub const MODE_NAMES: [&str; MODES] = ["normal", "insert", "visual", "cmdline"];
/// 普通模式。
pub const M_NORMAL: u8 = 0;
/// 插入模式。
pub const M_INSERT: u8 = 1;
/// 可视模式。
pub const M_VISUAL: u8 = 2;
/// 命令行模式。
pub const M_CMDLINE: u8 = 3;
/// 每场景步数。
pub const STEPS_PER_SCENE: usize = 3;
/// 终态总数。
pub const STEPS_TOTAL: usize = SCENES * STEPS_PER_SCENE;
/// 链环节数。
pub const LINKS: usize = 10;
/// 链环名。
pub const LINK_NAMES: [&str; LINKS] = [
    "seed_repo",
    "spawn_vim",
    "feed_keys",
    "grab_screen",
    "assert_cursor",
    "assert_mode",
    "assert_message",
    "assert_dirty",
    "restore_buf",
    "teardown",
];
/// 视口行数。
pub const ROWS: usize = 24;
/// 视口列数。
pub const COLS: usize = 80;
/// 模式码上界。
pub const MODE_MAX: u8 = 3;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 动作步（1）的模式：由场景决定。
pub fn action_mode(scene: u8) -> u8 {
    match scene {
        1 => M_INSERT,
        3 => M_VISUAL,
        5 | 6 | 7 => M_CMDLINE,
        _ => M_NORMAL,
    }
}

/// 场景落脏与否。
pub fn scene_dirty(scene: u8) -> bool {
    scene == 1 || scene == 4
}

/// 规则纯函数：由（场景, 步号）给出应有终态模式。
pub fn rule_mode(scene: u8, step: u8) -> u8 {
    if scene as usize >= SCENES || step as usize >= STEPS_PER_SCENE {
        return MODE_MAX.wrapping_add(1);
    }
    match step {
        0 | 2 => M_NORMAL,
        _ => action_mode(scene),
    }
}

/// 模式转移是否合法：普通模式出发可达四种；其余模式只能回普通模式。
pub fn transition_ok(from: u8, to: u8) -> bool {
    if from as usize >= MODES || to as usize >= MODES {
        return false;
    }
    if from == M_NORMAL {
        return true;
    }
    to == M_NORMAL
}

/// FNV-1a 单步。
pub fn fnv_step(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 4 {
        x ^= ((v >> (i * 8)) & 0xFF) as u64;
        x = x.wrapping_mul(FNV_PRIME);
        i = i.wrapping_add(1);
    }
    x
}

/// 单个终态样本。
#[derive(Clone, Copy, Debug)]
pub struct StepCase {
    /// 场景号。
    pub scene: u8,
    /// 步号（0 进入 / 1 动作 / 2 落定）。
    pub step: u8,
    /// 实测模式。
    pub mode: u8,
    /// 规则模式。
    pub expected: u8,
    /// 实测脏位。
    pub dirty: bool,
    /// 规则脏位。
    pub expected_dirty: bool,
    /// 是否已登记。
    pub live: bool,
    /// 是否已观测。
    pub seen: bool,
}

impl StepCase {
    /// 空样本。
    pub const fn blank() -> Self {
        StepCase {
            scene: 0,
            step: 0,
            mode: 0,
            expected: 0,
            dirty: false,
            expected_dirty: false,
            live: false,
            seen: false,
        }
    }

    /// 该步是否踩在场景的动作步上。
    pub fn is_action(&self) -> bool {
        self.step == 1
    }

    /// 实测与规则是否双面相符。
    pub fn agrees(&self) -> bool {
        self.mode == self.expected && self.dirty == self.expected_dirty
    }
}

/// 链环样本。
#[derive(Clone, Copy, Debug)]
pub struct LinkCase {
    /// 环号。
    pub idx: u8,
    /// 环节载荷（环名下标异或环号与视口格数）。
    pub payload: u32,
    /// 环摘要（前环递推）。
    pub digest: u64,
    /// 是否已登记。
    pub live: bool,
    /// 是否已观测。
    pub seen: bool,
}

impl LinkCase {
    /// 空样本。
    pub const fn blank() -> Self {
        LinkCase {
            idx: 0,
            payload: 0,
            digest: 0,
            live: false,
            seen: false,
        }
    }
}

/// vim 全屏联测账。
pub struct VimRun {
    base: u64,
    steps: [StepCase; STEPS_TOTAL],
    links: [LinkCase; LINKS],
    cursor: usize,
    closed: usize,
    registered: u32,
    observed: u32,
    rejects: u32,
    frozen: bool,
}

impl VimRun {
    /// 新账本。
    pub const fn new() -> Self {
        VimRun {
            base: SEG_BASE as u64,
            steps: [StepCase::blank(); STEPS_TOTAL],
            links: [LinkCase::blank(); LINKS],
            cursor: 0,
            closed: 0,
            registered: 0,
            observed: 0,
            rejects: 0,
            frozen: false,
        }
    }

    /// 记一次拒止并回传码。
    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 已登记终态数。
    pub fn registered(&self) -> u32 {
        self.registered
    }

    /// 已观测终态数。
    pub fn observed(&self) -> u32 {
        self.observed
    }

    /// 已闭环数。
    pub fn closed(&self) -> u32 {
        self.closed as u32
    }

    /// 拒止计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否封盘。
    pub fn frozen(&self) -> bool {
        self.frozen
    }

    /// 取终态。
    pub fn step(&self, id: usize) -> Option<&StepCase> {
        self.steps.get(id)
    }

    /// 取链环。
    pub fn link_at(&self, idx: usize) -> Option<&LinkCase> {
        self.links.get(idx)
    }

    /// 链头摘要：已闭环的最后一环，或账本基底。
    pub fn chain_head(&self) -> u64 {
        if self.closed == 0 {
            return self.base;
        }
        match self.links.get(self.closed - 1) {
            Some(l) => l.digest,
            None => self.base,
        }
    }

    /// 登记一个终态：期望模式与期望脏位由规则推得。
    pub fn register(&mut self, id: usize, scene: u8, step: u8, mode: u8) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(VM_EFROZEN));
        }
        if id >= STEPS_TOTAL || scene as usize >= SCENES || step as usize >= STEPS_PER_SCENE {
            return Err(self.fail(VM_ESCENE));
        }
        if mode as usize >= MODES {
            return Err(self.fail(VM_ENOEXP));
        }
        let live = match self.steps.get(id) {
            Some(c) => c.live,
            None => return Err(self.fail(VM_EUNREG)),
        };
        if live {
            return Err(self.fail(VM_EDUP));
        }
        if id != self.cursor {
            return Err(self.fail(VM_EORDER));
        }
        let expected = rule_mode(scene, step);
        let expected_dirty = scene_dirty(scene);
        let c = match self.steps.get_mut(id) {
            Some(c) => c,
            None => return Err(self.fail(VM_EUNREG)),
        };
        *c = StepCase {
            scene,
            step,
            mode,
            expected,
            dirty: expected_dirty,
            expected_dirty,
            live: true,
            seen: false,
        };
        self.cursor = self.cursor.wrapping_add(1);
        self.registered = self.registered.wrapping_add(1);
        Ok(())
    }

    /// 观测一个终态的模式与脏位。
    pub fn observe(&mut self, id: usize, mode: u8, dirty: bool) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(VM_EFROZEN));
        }
        if id >= STEPS_TOTAL {
            return Err(self.fail(VM_ESCENE));
        }
        if mode as usize >= MODES {
            return Err(self.fail(VM_ENOEXP));
        }
        let c = match self.steps.get_mut(id) {
            Some(c) => c,
            None => return Err(self.fail(VM_EUNREG)),
        };
        if !c.live {
            return Err(self.fail(VM_EUNREG));
        }
        if c.seen {
            return Err(self.fail(VM_EDUP));
        }
        c.seen = true;
        c.mode = mode;
        c.dirty = dirty;
        self.observed = self.observed.wrapping_add(1);
        if !c.agrees() {
            return Err(self.fail(VM_EMISMATCH));
        }
        Ok(())
    }
}

/// 链账与账面度量。
impl VimRun {
    /// 追加一环：环号必须等于已闭环数，摘要按前一环递推。
    pub fn link(&mut self, idx: usize, payload: u32) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(VM_EFROZEN));
        }
        if idx >= LINKS {
            return Err(self.fail(VM_ECHAIN));
        }
        let live = match self.links.get(idx) {
            Some(l) => l.live,
            None => return Err(self.fail(VM_EUNREG)),
        };
        if live {
            return Err(self.fail(VM_EDUP));
        }
        if idx != self.closed {
            return Err(self.fail(VM_EORDER));
        }
        let head = self.chain_head();
        let digest = fnv_step(fnv_step(head, payload), idx as u32);
        let l = match self.links.get_mut(idx) {
            Some(l) => l,
            None => return Err(self.fail(VM_EUNREG)),
        };
        *l = LinkCase {
            idx: idx as u8,
            payload,
            digest,
            live: true,
            seen: false,
        };
        self.closed = self.closed.wrapping_add(1);
        Ok(digest)
    }

    /// 观测一环的抓取摘要。
    pub fn observe_link(&mut self, idx: usize, digest: u64) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(VM_EFROZEN));
        }
        if idx >= LINKS {
            return Err(self.fail(VM_ECHAIN));
        }
        let l = match self.links.get_mut(idx) {
            Some(l) => l,
            None => return Err(self.fail(VM_EUNREG)),
        };
        if !l.live {
            return Err(self.fail(VM_EUNREG));
        }
        if l.seen {
            return Err(self.fail(VM_EDUP));
        }
        l.seen = true;
        if l.digest != digest {
            return Err(self.fail(VM_ECHAIN));
        }
        Ok(())
    }

    /// 全链逐环重算：任一环摘要与递推不符即断链。
    pub fn link_chain_ok(&self) -> bool {
        let mut head = self.base;
        let mut i = 0usize;
        while i < LINKS {
            let l = match self.links.get(i) {
                Some(l) => l,
                None => return false,
            };
            if !l.live || !l.seen {
                return false;
            }
            if l.idx as usize != i {
                return false;
            }
            head = fnv_step(fnv_step(head, l.payload), i as u32);
            if l.digest != head {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 模式面：每个规则模式出现多少步。
    pub fn mode_face(&self) -> [u32; MODES] {
        let mut face = [0u32; MODES];
        let mut i = 0usize;
        while i < STEPS_TOTAL {
            if let Some(c) = self.steps.get(i) {
                if c.live {
                    if let Some(slot) = face.get_mut(c.expected as usize) {
                        *slot = slot.wrapping_add(1);
                    }
                }
            }
            i = i.wrapping_add(1);
        }
        face
    }

    /// 场景面：每场景已登记几步。
    pub fn scene_face(&self) -> [u32; SCENES] {
        let mut face = [0u32; SCENES];
        let mut i = 0usize;
        while i < STEPS_TOTAL {
            if let Some(c) = self.steps.get(i) {
                if c.live {
                    if let Some(slot) = face.get_mut(c.scene as usize) {
                        *slot = slot.wrapping_add(1);
                    }
                }
            }
            i = i.wrapping_add(1);
        }
        face
    }

    /// 落脏步数（按规则面）。
    pub fn dirty_steps(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < STEPS_TOTAL {
            if let Some(c) = self.steps.get(i) {
                if c.live && c.expected_dirty {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 转移越界步数：步 0 从普通模式出发，其后从同场景上一步出发。
    pub fn transition_faults(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < STEPS_TOTAL {
            if let Some(c) = self.steps.get(i) {
                if c.live {
                    let from = if c.step == 0 {
                        M_NORMAL
                    } else {
                        match self.steps.get(i - 1) {
                            Some(p) => p.mode,
                            None => M_NORMAL,
                        }
                    };
                    if !transition_ok(from, c.mode) {
                        n = n.wrapping_add(1);
                    }
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 摘要：先混二十四个终态，再混十个链环。
    pub fn fold(&self) -> u64 {
        let mut h = self.base;
        let mut i = 0usize;
        while i < STEPS_TOTAL {
            if let Some(c) = self.steps.get(i) {
                h = fnv_step(h, c.scene as u32);
                h = fnv_step(h, c.step as u32);
                h = fnv_step(h, c.mode as u32);
                h = fnv_step(h, c.expected as u32);
                let flags = (c.dirty as u32)
                    | ((c.expected_dirty as u32) << 1)
                    | ((c.live as u32) << 2)
                    | ((c.seen as u32) << 3);
                h = fnv_step(h, flags);
            }
            i = i.wrapping_add(1);
        }
        let mut k = 0usize;
        while k < LINKS {
            if let Some(l) = self.links.get(k) {
                h = fnv_step(h, l.payload);
                h = fnv_step(h, (l.digest & 0xFFFF_FFFF) as u32);
                h = fnv_step(h, (l.digest >> 32) as u32);
                h = fnv_step(h, (l.live as u32) | ((l.seen as u32) << 1));
            }
            k = k.wrapping_add(1);
        }
        h
    }

    /// 终重放：终态全登记全观测全相符、链账十环闭合、
    /// 模式四类齐备、转移零越界。
    pub fn replay(&self) -> Result<u64, u32> {
        let mut i = 0usize;
        while i < STEPS_TOTAL {
            let c = match self.steps.get(i) {
                Some(c) => c,
                None => return Err(VM_EUNREG),
            };
            if !c.live {
                return Err(VM_EUNREG);
            }
            if !c.seen {
                return Err(VM_ESHORT);
            }
            if !c.agrees() {
                return Err(VM_EMISMATCH);
            }
            i = i.wrapping_add(1);
        }
        if self.closed != LINKS || !self.link_chain_ok() {
            return Err(VM_ECHAIN);
        }
        let face = self.mode_face();
        let mut m = 0usize;
        while m < MODES {
            if face[m] == 0 {
                return Err(VM_EMODE);
            }
            m = m.wrapping_add(1);
        }
        if self.transition_faults() != 0 {
            return Err(VM_EMODE);
        }
        Ok(self.fold())
    }

    /// 封盘。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(VM_EFROZEN));
        }
        let d = match self.replay() {
            Ok(d) => d,
            Err(e) => return Err(self.fail(e)),
        };
        self.frozen = true;
        Ok(d)
    }
}

/// 例号 → (场景, 步号, 终态模式)：三数全由规则表推得。
pub fn spec_of(id: usize) -> (u8, u8, u8) {
    let scene = ((id / STEPS_PER_SCENE) % SCENES) as u8;
    let step = (id % STEPS_PER_SCENE) as u8;
    (scene, step, rule_mode(scene, step))
}

/// 链环载荷：由环号与视口格数推出，保证每环各不相同。
pub fn link_payload(idx: usize) -> u32 {
    let grid = (ROWS as u32).wrapping_mul(COLS as u32);
    (idx as u32)
        .wrapping_mul(0x9E37_79B9)
        .wrapping_add(grid)
        ^ ((idx as u32) << 8)
}

/// 建一本完整账：二十四终态 + 十环链账。
pub fn build_run() -> VimRun {
    let mut r = VimRun::new();
    let mut i = 0usize;
    while i < STEPS_TOTAL {
        let (scene, step, mode) = spec_of(i);
        r.register(i, scene, step, mode).ok();
        r.observe(i, mode, scene_dirty(scene)).ok();
        i = i.wrapping_add(1);
    }
    let mut k = 0usize;
    while k < LINKS {
        if let Ok(d) = r.link(k, link_payload(k)) {
            r.observe_link(k, d).ok();
        }
        k = k.wrapping_add(1);
    }
    r
}

/// 四向注入：终态倒序、重复观测、链环跳号、封盘后再登记。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut a = VimRun::new();
    out[0] = a.register(1, 0, 1, M_NORMAL) == Err(VM_EORDER);
    let mut b = build_run();
    out[1] = b.observe(0, M_NORMAL, false) == Err(VM_EDUP);
    let mut c = VimRun::new();
    out[2] = c.link(2, link_payload(2)) == Err(VM_EORDER);
    let mut d = build_run();
    let _ = d.freeze();
    out[3] = d.register(0, 0, 0, M_NORMAL) == Err(VM_EFROZEN);
    out
}

/// 判据集：V1–V13（联测 10 条 + 链账 10 条对应项 + 拒止面）。
pub fn run_unx11149_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11149");
    let r = build_run();

    s.add(
        "V1 账面建齐：二十四终态、十环链账、零拒止",
        r.registered() == STEPS_TOTAL as u32
            && r.observed() == STEPS_TOTAL as u32
            && r.closed() == LINKS as u32
            && r.rejects() == 0,
        "24 终态 / 10 链环 / 拒止 0",
    );

    let d = r.fold();
    s.add(
        "V2 终重放通过且摘要等于 fold",
        r.replay() == Ok(d),
        "replay == fold",
    );

    let mut agree = 0u32;
    let mut i = 0usize;
    while i < STEPS_TOTAL {
        if let Some(c) = r.step(i) {
            if c.agrees() {
                agree = agree.wrapping_add(1);
            }
        }
        i = i.wrapping_add(1);
    }
    s.add(
        "V3 规则一致：二十四终态模式与脏位全相符",
        agree == STEPS_TOTAL as u32,
        "24/24 相符",
    );

    let face = r.mode_face();
    s.add_dyn(
        "V4 模式面四类齐备（普通 19 / 插入 1 / 可视 1 / 命令行 3）",
        face == [19, 1, 1, 3],
        &format!(
            "模式面 n={} i={} v={} c={}",
            face[0], face[1], face[2], face[3]
        ),
    );

    let sf = r.scene_face();
    let mut scenes_ok = true;
    let mut k = 0usize;
    while k < SCENES {
        if sf[k] != STEPS_PER_SCENE as u32 {
            scenes_ok = false;
        }
        k = k.wrapping_add(1);
    }
    s.add(
        "V5 场景面八场景各三步",
        scenes_ok,
        "8 × 3 = 24",
    );

    s.add(
        "V6 脏位面：插入与粘贴两场景共六步落脏",
        r.dirty_steps() == 6 && scene_dirty(1) && scene_dirty(4) && !scene_dirty(0),
        "脏步 6",
    );

    s.add(
        "V7 模式转移零越界（插入/可视/命令行只回普通）",
        r.transition_faults() == 0
            && transition_ok(M_NORMAL, M_INSERT)
            && !transition_ok(M_INSERT, M_VISUAL)
            && transition_ok(M_CMDLINE, M_NORMAL),
        "转移 24/24 合法",
    );

    s.add(
        "V8 全链账十环闭合、顺序与环名表对齐",
        r.link_chain_ok() && r.closed() == LINKS as u32 && LINK_NAMES.len() == LINKS,
        "链账 10/10",
    );

    let mut chain = VimRun::new();
    let mut ok_seq = true;
    let mut k2 = 0usize;
    while k2 < LINKS {
        match chain.link(k2, link_payload(k2)) {
            Ok(dg) => {
                if chain.observe_link(k2, dg).is_err() {
                    ok_seq = false;
                }
            }
            Err(_) => ok_seq = false,
        }
        k2 = k2.wrapping_add(1);
    }
    s.add(
        "V9 链摘要逐环递推（重算十环全对）",
        ok_seq && chain.link_chain_ok() && chain.chain_head() != chain.base,
        "十环摘要递推",
    );

    let mut bad = VimRun::new();
    let mut tamper = false;
    if let Ok(dg) = bad.link(0, link_payload(0)) {
        tamper = bad.observe_link(0, dg ^ 1) == Err(VM_ECHAIN);
    }
    s.add(
        "V10 断链可检出（错摘要落 VM_ECHAIN）",
        tamper,
        "摘要错位 → ECHAIN",
    );

    let mut o = VimRun::new();
    let order = o.register(3, 1, 0, M_NORMAL) == Err(VM_EORDER);
    let first = o.register(0, 0, 0, M_NORMAL) == Ok(());
    let mut dup = build_run();
    let dup_ok = dup.observe(0, M_NORMAL, false) == Err(VM_EDUP);
    s.add(
        "V11 例序倒挂被拒、重复观测被拒",
        order && first && dup_ok,
        "EORDER / EDUP",
    );

    let mut short = VimRun::new();
    let _ = short.register(0, 0, 0, M_NORMAL);
    let mut unreg = VimRun::new();
    s.add(
        "V12 未测即重放 ESHORT、未登记即观测 EUNREG",
        short.replay() == Err(VM_ESHORT) && unreg.observe(0, M_NORMAL, false) == Err(VM_EUNREG),
        "ESHORT / EUNREG",
    );

    let inj = inject_four();
    let mut all = true;
    let mut q = 0usize;
    while q < 4 {
        if !inj[q] {
            all = false;
        }
        q = q.wrapping_add(1);
    }
    let mut fz = build_run();
    let once = fz.freeze().is_ok() && fz.freeze() == Err(VM_EFROZEN);
    s.add(
        "V13 四向注入全真、封盘一次性",
        all && once,
        "注入 4/4 + 封盘",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_builds_clean() {
        let r = build_run();
        assert_eq!(r.registered(), STEPS_TOTAL as u32);
        assert_eq!(r.observed(), STEPS_TOTAL as u32);
        assert_eq!(r.closed(), LINKS as u32);
        assert_eq!(r.rejects(), 0);
        assert_eq!(r.replay(), Ok(r.fold()));
    }

    #[test]
    fn mode_and_dirty_faces() {
        let r = build_run();
        assert_eq!(r.mode_face(), [19, 1, 1, 3]);
        assert_eq!(r.dirty_steps(), 6);
        let sf = r.scene_face();
        for v in sf.iter() {
            assert_eq!(*v, STEPS_PER_SCENE as u32);
        }
        assert_eq!(r.transition_faults(), 0);
    }

    #[test]
    fn rule_edges() {
        assert_eq!(rule_mode(1, 1), M_INSERT);
        assert_eq!(rule_mode(3, 1), M_VISUAL);
        assert_eq!(rule_mode(7, 1), M_CMDLINE);
        assert_eq!(rule_mode(0, 2), M_NORMAL);
        assert_eq!(rule_mode(SCENES as u8, 0), MODE_MAX + 1);
        assert!(!transition_ok(M_INSERT, M_VISUAL));
        assert!(transition_ok(M_VISUAL, M_NORMAL));
        assert!(!transition_ok(M_NORMAL, MODE_MAX + 1));
    }

    #[test]
    fn refusal_gates() {
        let mut r = VimRun::new();
        assert_eq!(r.register(1, 0, 1, M_NORMAL), Err(VM_EORDER));
        assert_eq!(r.register(0, SCENES as u8, 0, M_NORMAL), Err(VM_ESCENE));
        assert_eq!(r.register(0, 0, 0, MODE_MAX + 1), Err(VM_ENOEXP));
        assert!(r.register(0, 0, 0, M_NORMAL).is_ok());
        assert_eq!(r.register(0, 0, 0, M_NORMAL), Err(VM_EDUP));
        assert_eq!(r.link(1, 0), Err(VM_EORDER));
        assert_eq!(r.link(LINKS, 0), Err(VM_ECHAIN));
    }

    #[test]
    fn mismatch_detected() {
        let mut r = VimRun::new();
        assert!(r.register(0, 1, 0, M_NORMAL).is_ok());
        assert_eq!(r.observe(0, M_NORMAL, false), Err(VM_EMISMATCH));
        assert_eq!(r.observe(0, M_INSERT, true), Err(VM_EDUP));
        let mut p = VimRun::new();
        assert!(p.register(0, 0, 0, M_NORMAL).is_ok());
        assert_eq!(p.replay(), Err(VM_ESHORT));
    }

    #[test]
    fn broken_chain_detected() {
        let mut r = VimRun::new();
        let d = r.link(0, link_payload(0)).expect("first link");
        assert_eq!(r.observe_link(0, d ^ 0xFF), Err(VM_ECHAIN));
        let mut s = VimRun::new();
        for k in 0..LINKS {
            let dg = s.link(k, link_payload(k)).expect("link");
            assert!(s.observe_link(k, dg).is_ok());
        }
        assert!(s.link_chain_ok());
        assert_ne!(s.chain_head(), s.base);
    }

    #[test]
    fn freeze_once_and_checks_green() {
        let mut r = build_run();
        assert!(r.freeze().is_ok());
        assert_eq!(r.freeze(), Err(VM_EFROZEN));
        let s = run_unx11149_checks();
        assert!(s.all_passed(), "{:?}", s);
    }
}
