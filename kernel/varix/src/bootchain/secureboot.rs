//! UNREAL-X AI-01 · 族0008 安全启动仪式（X00176~X00200）与
//! 族0010 固件风格定制（X00226~X00250）。
//!
//! secureboot.rs 落点：安全启动信任链可视化数据结构（节点 + 有向链 +
//! 逐环校验），供设置页渲染「信任链剧场」。fwpersona 落点：固件皮配置
//! （配色/徽标/进度样式/静默），越界钳制、默认档 = 现状。
//! 纯逻辑 + 固定容量数组，no_std 兼容。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 族0008 安全启动仪式：信任链
// ---------------------------------------------------------------------------

/// 信任链最大节点数（固件→shim→引导器→内核→模块 → 预留扩展）。
pub const TRUST_NODE_MAX: usize = 8;
/// 摘要长度（字节）。
pub const DIGEST_LEN: usize = 8;

/// 信任节点状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrustState {
    /// 尚未度量（链路未走到）。
    Pending,
    /// 度量通过。
    Verified,
    /// 签名/摘要不符（链路在此断裂）。
    Broken,
    /// 无签名但策略允许（relaxed 档）。
    TrustedUnsigned,
}

/// 信任链节点：名字 + 期望摘要 + 实际摘要 + 状态。
#[derive(Clone, Copy, Debug)]
pub struct TrustNode {
    pub name: [u8; 16],
    pub name_len: usize,
    pub expect: [u8; DIGEST_LEN],
    pub actual: [u8; DIGEST_LEN],
    pub state: TrustState,
}

impl TrustNode {
    const fn empty() -> TrustNode {
        TrustNode {
            name: [0u8; 16],
            name_len: 0,
            expect: [0u8; DIGEST_LEN],
            actual: [0u8; DIGEST_LEN],
            state: TrustState::Pending,
        }
    }
}

/// 安全启动策略档位：≥5 档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SecureBootMode {
    /// 关闭（现状默认）：度量照做，不阻断。
    Off,
    /// 审计：断裂只记录不阻断。
    Audit,
    /// 宽松：允许无签名，禁断裂。
    Relaxed,
    /// 严格：全链 Verified 才放行。
    Strict,
    /// 锁定：断裂即进入恢复环境（联动族0004）。
    Locked,
}

impl SecureBootMode {
    pub fn from_index(i: u32) -> SecureBootMode {
        match i {
            0 => SecureBootMode::Off,
            1 => SecureBootMode::Audit,
            2 => SecureBootMode::Relaxed,
            3 => SecureBootMode::Strict,
            _ => SecureBootMode::Locked,
        }
    }

    /// 该模式下某节点状态是否允许链路继续。
    pub fn allows(self, st: TrustState) -> bool {
        match self {
            SecureBootMode::Off => true,
            SecureBootMode::Audit => true,
            SecureBootMode::Relaxed => st != TrustState::Broken,
            SecureBootMode::Strict | SecureBootMode::Locked => st == TrustState::Verified,
        }
    }
}

/// 信任链可视化数据：节点数组 + 断裂位置。
#[derive(Clone, Copy, Debug)]
pub struct TrustChain {
    pub nodes: [TrustNode; TRUST_NODE_MAX],
    pub count: usize,
    pub mode: SecureBootMode,
    /// 第一处断裂下标；无断裂 = None。
    pub first_break: Option<usize>,
    /// 被钳制的非法操作次数。
    pub clamped: u32,
}

fn digest(name: &[u8]) -> [u8; DIGEST_LEN] {
    let h = crate::bootchain::hash_bytes(name);
    let mut d = [0u8; DIGEST_LEN];
    for (i, b) in d.iter_mut().enumerate() {
        *b = ((h >> ((i % 4) * 8)) as u8) ^ crate::bootchain::freeze32(h.wrapping_add(i as u32)) as u8;
    }
    d
}

impl TrustChain {
    pub const fn new(mode: SecureBootMode) -> TrustChain {
        TrustChain {
            nodes: [TrustNode::empty(); TRUST_NODE_MAX],
            count: 0,
            mode,
            first_break: None,
            clamped: 0,
        }
    }

    /// 登记一个节点（名字 + 期望摘要自动从名字派生，演示环境可复现）。
    pub fn enroll(&mut self, name: &[u8]) -> bool {
        if name.is_empty() || self.count >= TRUST_NODE_MAX {
            self.clamped += 1;
            return false;
        }
        let i = self.count;
        let n = name.len().min(16);
        let node = &mut self.nodes[i];
        node.name[..n].copy_from_slice(&name[..n]);
        node.name_len = n;
        node.expect = digest(name);
        node.actual = node.expect;
        node.state = TrustState::Pending;
        self.count += 1;
        true
    }

    /// 度量：实际摘要与期望一致 → Verified；不一致 → Broken 并更新断裂点。
    pub fn measure(&mut self, index: usize, actual: &[u8; DIGEST_LEN]) -> bool {
        if index >= self.count {
            self.clamped += 1;
            return false;
        }
        let node = &mut self.nodes[index];
        node.actual = *actual;
        node.state = if actual == &node.expect {
            TrustState::Verified
        } else {
            TrustState::Broken
        };
        if node.state == TrustState::Broken && self.first_break.is_none() {
            self.first_break = Some(index);
        }
        true
    }

    /// 逐环校验：返回链路是否整体放行（在当前模式下）。
    pub fn admit(&self) -> bool {
        (0..self.count).all(|i| self.mode.allows(self.nodes[i].state))
    }

    /// 可视化行：设置页逐环渲染用（`环序` + 状态符号）。
    pub fn render_marks(&self) -> [u8; TRUST_NODE_MAX] {
        let mut out = [b'-'; TRUST_NODE_MAX];
        for i in 0..self.count {
            out[i] = match self.nodes[i].state {
                TrustState::Pending => b'-',
                TrustState::Verified => b'V',
                TrustState::Broken => b'X',
                TrustState::TrustedUnsigned => b'~',
            };
        }
        out
    }

    /// 全链 Verified 且无断裂才算「安全启动仪式完成」。
    pub fn ceremony_complete(&self) -> bool {
        self.count > 0
            && self.first_break.is_none()
            && (0..self.count).all(|i| self.nodes[i].state == TrustState::Verified)
    }
}

/// 族0008 域自检。
pub fn run_secureboot_checks() -> CheckSet {
    let mut set = CheckSet::new("bootchain.secureboot");
    let mut c = TrustChain::new(SecureBootMode::Strict);
    set.add("enroll chain", {
        c.enroll(b"firmware") && c.enroll(b"shim") && c.enroll(b"loader") && c.enroll(b"kernel") && c.count == 4
    }, "");
    set.add("enroll overflow clamped", {
        let mut c7 = TrustChain::new(SecureBootMode::Audit);
        for i in 0..TRUST_NODE_MAX {
            assert!(c7.enroll(&[b'a' + i as u8]));
        }
        !c7.enroll(b"overflow") && c7.clamped == 1
    }, "");
    let mut c2 = TrustChain::new(SecureBootMode::Strict);
    c2.enroll(b"firmware");
    set.add("measure verified", c2.measure(0, &digest(b"firmware")) && c2.nodes[0].state == TrustState::Verified, "");
    set.add("measure broken sets first_break", {
        c2.measure(0, &[1u8; DIGEST_LEN]);
        c2.first_break == Some(0) && !c2.admit()
    }, "");
    set.add("off mode admits anything", {
        let mut c3 = TrustChain::new(SecureBootMode::Off);
        c3.enroll(b"a");
        c3.measure(0, &[9u8; DIGEST_LEN]);
        c3.admit()
    }, "");
    set.add("mode index roundtrip", SecureBootMode::from_index(2) == SecureBootMode::Relaxed, "");
    set.add("ceremony needs all verified", {
        let mut c4 = TrustChain::new(SecureBootMode::Strict);
        c4.enroll(b"a");
        c4.enroll(b"b");
        c4.measure(0, &digest(b"a"));
        !c4.ceremony_complete()
    }, "");
    set.add("ceremony complete on full verify", {
        let mut c5 = TrustChain::new(SecureBootMode::Strict);
        c5.enroll(b"a");
        c5.enroll(b"b");
        c5.measure(0, &digest(b"a"));
        c5.measure(1, &digest(b"b"));
        let marks = c5.render_marks();
        c5.ceremony_complete() && c5.admit() && marks[0] == b'V' && marks[1] == b'V' && marks[2] == b'-'
    }, "");
    set.add("out-of-range measure clamped", {
        let mut c6 = TrustChain::new(SecureBootMode::Audit);
        let ok = c6.measure(7, &[0u8; DIGEST_LEN]);
        !ok && c6.clamped == 1
    }, "");
    set
}

// ---------------------------------------------------------------------------
// 族0010 固件风格定制：固件皮
// ---------------------------------------------------------------------------

/// 固件皮最大名字长度（字节）。
pub const PERSONA_NAME_MAX: usize = 16;

/// 进度样式档（≥5 档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProgressStyle {
    Bar,
    Ring,
    Dots,
    Marquee,
    Silent,
}

/// 固件皮配置：颜色、徽标、进度样式、静默。
#[derive(Clone, Copy, Debug)]
pub struct FwPersona {
    pub name: [u8; PERSONA_NAME_MAX],
    pub name_len: usize,
    pub accent: (u8, u8, u8),
    pub glyph: u32,
    pub progress: ProgressStyle,
    pub silent: bool,
    pub clamped: u32,
}

impl FwPersona {
    /// 默认档 = 现状（深空蓝 + Bar + 不静默）。
    pub const fn new() -> FwPersona {
        FwPersona {
            name: [0u8; PERSONA_NAME_MAX],
            name_len: 0,
            accent: (0x4c, 0x8b, 0xf0),
            glyph: 0x25cf,
            progress: ProgressStyle::Bar,
            silent: false,
            clamped: 0,
        }
    }

    /// 设置名字（越界钳掉尾巴）。
    pub fn set_name(&mut self, name: &[u8]) {
        if name.is_empty() {
            self.clamped += 1;
            return;
        }
        let n = name.len().min(PERSONA_NAME_MAX);
        self.name[..n].copy_from_slice(&name[..n]);
        self.name_len = n;
    }

    pub fn name_bytes(&self) -> &[u8] {
        &self.name[..self.name_len]
    }

    /// 设置主色：色值无法钳制语义，只记非法（全零 = 视为非法回默认）。
    pub fn set_accent(&mut self, rgb: (u8, u8, u8)) {
        if rgb == (0, 0, 0) {
            self.clamped += 1;
            self.accent = FwPersona::new().accent;
        } else {
            self.accent = rgb;
        }
    }

    /// 设置进度样式（按索引，越界回 Bar）。
    pub fn set_progress_index(&mut self, i: u32) {
        self.progress = match i {
            0 => ProgressStyle::Bar,
            1 => ProgressStyle::Ring,
            2 => ProgressStyle::Dots,
            3 => ProgressStyle::Marquee,
            4 => ProgressStyle::Silent,
            _ => {
                self.clamped += 1;
                ProgressStyle::Bar
            }
        };
    }

    /// 静默档联动：Silent 样式强制 silent = true（体验一致性）。
    pub fn reconcile(&mut self) {
        if self.progress == ProgressStyle::Silent {
            self.silent = true;
        }
    }

    /// 快照：可携带的字节载荷（迁移体系三通道之一）。
    pub fn snapshot(&self) -> [u8; 8] {
        [
            self.name_len as u8,
            self.accent.0,
            self.accent.1,
            self.accent.2,
            (self.glyph & 0xff) as u8,
            match self.progress {
                ProgressStyle::Bar => 0,
                ProgressStyle::Ring => 1,
                ProgressStyle::Dots => 2,
                ProgressStyle::Marquee => 3,
                ProgressStyle::Silent => 4,
            },
            u8::from(self.silent),
            0,
        ]
    }
}

/// 族0010 域自检。
pub fn run_fwpersona_checks() -> CheckSet {
    let mut set = CheckSet::new("bootchain.fwpersona");
    let mut p = FwPersona::new();
    set.add("default persona is stock", p.progress == ProgressStyle::Bar && !p.silent, "");
    set.add("name clamp tail", {
        p.set_name(b"0123456789ABCDEFEXTRA");
        p.name_bytes().len() == PERSONA_NAME_MAX
    }, "");
    set.add("empty name clamped", {
        let mut q = FwPersona::new();
        q.set_name(b"");
        q.clamped == 1
    }, "");
    set.add("zero accent falls back", {
        let mut q = FwPersona::new();
        q.set_accent((0, 0, 0));
        q.accent != (0, 0, 0) && q.clamped == 1
    }, "");
    set.add("progress index roundtrip", {
        let mut q = FwPersona::new();
        q.set_progress_index(3);
        q.progress == ProgressStyle::Marquee
    }, "");
    set.add("progress overflow clamps to bar", {
        let mut q = FwPersona::new();
        q.set_progress_index(9);
        q.progress == ProgressStyle::Bar && q.clamped == 1
    }, "");
    set.add("silent style forces silent", {
        let mut q = FwPersona::new();
        q.set_progress_index(4);
        q.reconcile();
        q.silent
    }, "");
    set.add("snapshot stable 8 bytes", p.snapshot().len() == 8 && p.snapshot()[1] == 0x4c, "");
    set.add("snapshot style byte matches", {
        let mut q = FwPersona::new();
        q.set_progress_index(1);
        q.snapshot()[5] == 1
    }, "");
    set.add("name roundtrip", {
        let mut q = FwPersona::new();
        q.set_name(b"varix-fw");
        q.name_bytes() == b"varix-fw"
    }, "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x00176_chain_end_to_end() {
        let mut c = TrustChain::new(SecureBootMode::Strict);
        for n in [b"firmware".as_slice(), b"shim".as_slice(), b"kernel".as_slice()] {
            assert!(c.enroll(n));
        }
        for i in 0..c.count {
            let expect = c.nodes[i].expect;
            assert!(c.measure(i, &expect));
        }
        assert!(c.ceremony_complete() && c.admit());
    }

    #[test]
    fn x00186_broken_chain_blocked_in_strict() {
        let mut c = TrustChain::new(SecureBootMode::Strict);
        c.enroll(b"loader");
        assert!(c.measure(0, &[7u8; DIGEST_LEN]));
        assert_eq!(c.first_break, Some(0));
        assert!(!c.admit());
        assert_eq!(c.render_marks()[0], b'X');
    }

    #[test]
    fn x00226_persona_default_matches_stock() {
        let p = FwPersona::new();
        assert_eq!(p.accent, (0x4c, 0x8b, 0xf0));
        assert!(!p.silent);
    }

    #[test]
    fn x00231_persona_clamps() {
        let mut p = FwPersona::new();
        p.set_name(&[b'x'; 64]);
        assert_eq!(p.name_bytes().len(), PERSONA_NAME_MAX);
        p.set_progress_index(255);
        assert_eq!(p.progress, ProgressStyle::Bar);
    }

    #[test]
    fn x00008_run_checks_pass() {
        assert!(run_secureboot_checks().all_passed());
        assert!(run_fwpersona_checks().all_passed());
    }
}
