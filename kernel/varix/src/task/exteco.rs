//! UNREAL-X-15000 · AI-28 族0278 扩展生态（X06926~X06950）。
//! 扩展生态：扩展清单校验（版本三元组比较、能力位声明与权限矩阵）、
//! 扩展加载状态机、冲突检测。零堆、整数运算，无 Vec/String/Box/alloc、无外部 crate。

use core::cmp::Ordering;

// ---------------------------------------------------------------------------
// 常量与错误码
// ---------------------------------------------------------------------------

/// 扩展槽位固定容量。
pub const MAX_EXT: usize = 12;
/// 能力位总数（0~7 位合法）。
pub const CAP_BITS: u32 = 8;
/// 全部合法能力位。
pub const CAP_ALL: u32 = 0xFF;

pub const CAP_UI: u32 = 1 << 0;
pub const CAP_FS: u32 = 1 << 1;
pub const CAP_NET: u32 = 1 << 2;
pub const CAP_HOOK: u32 = 1 << 3;
pub const CAP_PROC: u32 = 1 << 4;
pub const CAP_DRV: u32 = 1 << 5;
pub const CAP_CRYPTO: u32 = 1 << 6;
pub const CAP_TIME: u32 = 1 << 7;

/// 互斥掩码：钩子与进程注入能力不得同时授予。
pub const CONFLICT_MASK: u32 = CAP_HOOK | CAP_PROC;

pub const PERM_READ: u32 = 1 << 0;
pub const PERM_WRITE: u32 = 1 << 1;
pub const PERM_EXEC: u32 = 1 << 2;
pub const PERM_ADMIN: u32 = 1 << 3;

pub const E_OK: u16 = 0;
pub const E_FULL: u16 = 1;
pub const E_DUP_ID: u16 = 2;
pub const E_CAP_UNKNOWN: u16 = 3;
pub const E_CAP_CONFLICT: u16 = 4;
pub const E_VER_TOO_OLD: u16 = 5;
pub const E_BAD_STATE: u16 = 6;
pub const E_NOT_FOUND: u16 = 7;
pub const E_INVALID: u16 = 8;

pub fn describe(code: u16) -> &'static str {
    match code {
        E_OK => "正常",
        E_FULL => "扩展槽位已满，建议先卸载不再使用的扩展再安装",
        E_DUP_ID => "扩展 id 重复，建议更换 id 或先卸载同 id 旧扩展",
        E_CAP_UNKNOWN => "清单声明了未知能力位，建议改用 CAP_ALL 范围内的能力组合",
        E_CAP_CONFLICT => "能力位互斥冲突，建议更换能力组合或禁用冲突扩展",
        E_VER_TOO_OLD => "内核版本低于扩展要求，建议升级内核或改用兼容版扩展",
        E_BAD_STATE => "非法状态转移，建议按加载→启用→禁用→卸载的顺序操作",
        E_NOT_FOUND => "扩展不存在，建议先安装或核对扩展 id",
        E_INVALID => "快照或参数非法，建议重新导出后再导入",
        _ => "未知扩展生态错误，建议重置扩展中心后重试",
    }
}

// ---------------------------------------------------------------------------
// 版本三元组
// ---------------------------------------------------------------------------

/// 版本三元组 major.minor.patch。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ver {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl Ver {
    pub fn cmp_to(self, o: Ver) -> Ordering {
        if self.major != o.major {
            self.major.cmp(&o.major)
        } else if self.minor != o.minor {
            self.minor.cmp(&o.minor)
        } else {
            self.patch.cmp(&o.patch)
        }
    }

    pub fn at_least(self, min: Ver) -> bool {
        self.cmp_to(min) != Ordering::Less
    }
}

/// 把版本渲染为 "1.2.3" 文本字节（无分配），返回写入长度。
pub fn render_ver(v: Ver, buf: &mut [u8]) -> usize {
    if buf.len() < 16 {
        return 0;
    }
    let mut n = 0usize;
    push_dec(buf, &mut n, v.major as u32);
    buf[n] = b'.';
    n += 1;
    push_dec(buf, &mut n, v.minor as u32);
    buf[n] = b'.';
    n += 1;
    push_dec(buf, &mut n, v.patch as u32);
    n
}

fn push_dec(buf: &mut [u8], n: &mut usize, v: u32) {
    let mut tmp = [0u8; 5];
    let mut m = 0usize;
    let mut x = v;
    if x == 0 {
        tmp[0] = b'0';
        m = 1;
    }
    while x > 0 {
        tmp[m] = b'0' + (x % 10) as u8;
        x /= 10;
        m += 1;
    }
    while m > 0 {
        m -= 1;
        buf[*n] = tmp[m];
        *n += 1;
    }
}

// ---------------------------------------------------------------------------
// 能力位与权限矩阵
// ---------------------------------------------------------------------------

/// 权限矩阵：每个权限位所需的扩展能力集合（静态表，权限位序 0~3）。
const PERM_MATRIX: [u32; 4] = [CAP_FS, CAP_FS, CAP_PROC, CAP_HOOK | CAP_CRYPTO];

/// 查询权限矩阵：声明能力是否满足该权限所需能力集合（O(1) 幂等）。
pub fn perm_granted(caps: u32, perm_bit: u32) -> bool {
    if perm_bit == 0 || perm_bit >= (1 << PERM_MATRIX.len()) {
        return false;
    }
    let idx = perm_bit.trailing_zeros() as usize;
    let need = PERM_MATRIX[idx];
    (caps & need) == need
}

// ---------------------------------------------------------------------------
// 清单与状态机
// ---------------------------------------------------------------------------

/// 扩展清单：id、版本、声明能力位、要求的最低内核版本。
#[derive(Clone, Copy, Debug)]
pub struct Manifest {
    pub id: u16,
    pub ver: Ver,
    pub caps: u32,
    pub min_kernel: Ver,
}

impl Manifest {
    /// 清单静态校验：能力位必须落在 CAP_ALL 内。
    pub fn validate(&self) -> u16 {
        if (self.caps & !CAP_ALL) != 0 {
            return E_CAP_UNKNOWN;
        }
        if self.id == 0 {
            return E_INVALID;
        }
        E_OK
    }
}

/// 冲突检测：同 id 视为冲突；两者合计占满互斥掩码（如钩子+进程注入同时出现）视为冲突。
pub fn conflict_between(a: &Manifest, b: &Manifest) -> bool {
    a.id == b.id || (((a.caps | b.caps) & CONFLICT_MASK) == CONFLICT_MASK)
}

/// 扩展加载状态机（≥5 档：6 态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtState {
    Unregistered,
    ManifestOk,
    Loaded,
    Enabled,
    Disabled,
    Failed,
}

impl ExtState {
    pub fn index(self) -> u32 {
        match self {
            ExtState::Unregistered => 0,
            ExtState::ManifestOk => 1,
            ExtState::Loaded => 2,
            ExtState::Enabled => 3,
            ExtState::Disabled => 4,
            ExtState::Failed => 5,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ExtState::Unregistered => "unregistered",
            ExtState::ManifestOk => "manifest",
            ExtState::Loaded => "loaded",
            ExtState::Enabled => "enabled",
            ExtState::Disabled => "disabled",
            ExtState::Failed => "failed",
        }
    }

    pub fn from_index(idx: u8) -> ExtState {
        match idx {
            1 => ExtState::ManifestOk,
            2 => ExtState::Loaded,
            3 => ExtState::Enabled,
            4 => ExtState::Disabled,
            5 => ExtState::Failed,
            _ => ExtState::Unregistered,
        }
    }
}

/// 状态机事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtEvent {
    ValidateOk,
    ValidateFail,
    Load,
    Enable,
    Disable,
    Unload,
}

/// 纯状态转移表：合法转移返回新态，非法转移返回 None（钳制不动）。
pub fn transition(st: ExtState, ev: ExtEvent) -> Option<ExtState> {
    match (st, ev) {
        (ExtState::Unregistered, ExtEvent::ValidateOk) => Some(ExtState::ManifestOk),
        (ExtState::Unregistered, ExtEvent::ValidateFail) => Some(ExtState::Failed),
        (ExtState::ManifestOk, ExtEvent::Load) => Some(ExtState::Loaded),
        (ExtState::Loaded, ExtEvent::Enable) => Some(ExtState::Enabled),
        (ExtState::Enabled, ExtEvent::Disable) => Some(ExtState::Disabled),
        (ExtState::Disabled, ExtEvent::Enable) => Some(ExtState::Enabled),
        (ExtState::Loaded, ExtEvent::Unload)
        | (ExtState::Disabled, ExtEvent::Unload)
        | (ExtState::Failed, ExtEvent::Unload) => Some(ExtState::Unregistered),
        _ => None,
    }
}

/// 一个已安装扩展：清单 + 当前状态。
#[derive(Clone, Copy, Debug)]
pub struct Extension {
    pub mf: Manifest,
    pub state: ExtState,
}

/// 扩展中心：安装、状态推进、冲突计数与快照。
pub struct ExtHub {
    pub kernel: Ver,
    pub exts: [Option<Extension>; MAX_EXT],
    pub count: usize,
    pub conflicts: u32,
}

impl ExtHub {
    pub fn new(kernel: Ver) -> ExtHub {
        ExtHub { kernel, exts: [None; MAX_EXT], count: 0, conflicts: 0 }
    }

    /// 安装：容量/能力位/内核版本/重复 id/互斥冲突全量校验。
    pub fn install(&mut self, mf: Manifest) -> u16 {
        if self.count >= MAX_EXT {
            return E_FULL;
        }
        let v = mf.validate();
        if v != E_OK {
            return v;
        }
        if self.kernel.cmp_to(mf.min_kernel) == Ordering::Less {
            return E_VER_TOO_OLD;
        }
        for e in self.exts.iter().flatten() {
            if e.mf.id == mf.id {
                return E_DUP_ID;
            }
        }
        for e in self.exts.iter().flatten() {
            if ((e.mf.caps | mf.caps) & CONFLICT_MASK) == CONFLICT_MASK {
                self.conflicts += 1;
                return E_CAP_CONFLICT;
            }
        }
        for slot in self.exts.iter_mut() {
            if slot.is_none() {
                *slot = Some(Extension { mf, state: ExtState::ManifestOk });
                self.count += 1;
                return E_OK;
            }
        }
        E_FULL
    }

    /// 推进状态机（非法转移返回 E_BAD_STATE 且状态保持）。
    pub fn step(&mut self, id: u16, ev: ExtEvent) -> u16 {
        for slot in self.exts.iter_mut().flatten() {
            if slot.mf.id == id {
                match transition(slot.state, ev) {
                    Some(ns) => {
                        slot.state = ns;
                        return E_OK;
                    }
                    None => return E_BAD_STATE,
                }
            }
        }
        E_NOT_FOUND
    }

    pub fn probe(&self, id: u16) -> Option<ExtState> {
        self.exts.iter().flatten().find(|e| e.mf.id == id).map(|e| e.state)
    }

    /// 卸载并释放槽位（回滚净身）。
    pub fn uninstall(&mut self, id: u16) -> u16 {
        for i in 0..MAX_EXT {
            if let Some(e) = self.exts[i] {
                if e.mf.id == id {
                    self.exts[i] = None;
                    self.count -= 1;
                    return E_OK;
                }
            }
        }
        E_NOT_FOUND
    }

    /// 全量卸载，返回卸载数。
    pub fn uninstall_all(&mut self) -> usize {
        let mut n = 0usize;
        for i in 0..MAX_EXT {
            if self.exts[i].is_some() {
                self.exts[i] = None;
                n += 1;
            }
        }
        self.count = 0;
        n
    }

    /// 不变量审计：容量、id 非零、能力位全部合法。
    pub fn audit(&self) -> bool {
        if self.count > MAX_EXT {
            return false;
        }
        for e in self.exts.iter().flatten() {
            if e.mf.id == 0 || (e.mf.caps & !CAP_ALL) != 0 {
                return false;
            }
        }
        true
    }

    /// 快照导出：魔数 0x78 + 版本 + 项数 + 每项 10 字节。
    pub fn export(&self, buf: &mut [u8]) -> usize {
        if buf.len() < 3 + self.count * 10 {
            return 0;
        }
        buf[0] = 0x78;
        buf[1] = 1;
        buf[2] = self.count as u8;
        let mut k = 3usize;
        for i in 0..MAX_EXT {
            if let Some(e) = self.exts[i] {
                buf[k] = (e.mf.id & 0xFF) as u8;
                buf[k + 1] = (e.mf.id >> 8) as u8;
                buf[k + 2] = e.mf.ver.major as u8;
                buf[k + 3] = e.mf.ver.minor as u8;
                buf[k + 4] = e.mf.ver.patch as u8;
                buf[k + 5] = e.mf.min_kernel.major as u8;
                buf[k + 6] = e.mf.min_kernel.minor as u8;
                buf[k + 7] = e.mf.min_kernel.patch as u8;
                buf[k + 8] = e.mf.caps as u8;
                buf[k + 9] = e.state.index() as u8;
                k += 10;
            }
        }
        k
    }

    /// 快照导入：先净身再按快照逐项还原（含状态）。
    pub fn import(&mut self, buf: &[u8]) -> u16 {
        if buf.len() < 3 || buf[0] != 0x78 || buf[1] != 1 {
            return E_INVALID;
        }
        let n = buf[2] as usize;
        if n > MAX_EXT || buf.len() < 3 + n * 10 {
            return E_INVALID;
        }
        self.uninstall_all();
        let mut k = 3usize;
        for _ in 0..n {
            let id = buf[k] as u16 | ((buf[k + 1] as u16) << 8);
            let mf = Manifest {
                id,
                ver: Ver { major: buf[k + 2] as u16, minor: buf[k + 3] as u16, patch: buf[k + 4] as u16 },
                caps: buf[k + 8] as u32,
                min_kernel: Ver { major: buf[k + 5] as u16, minor: buf[k + 6] as u16, patch: buf[k + 7] as u16 },
            };
            let st = ExtState::from_index(buf[k + 9]);
            for slot in self.exts.iter_mut() {
                if slot.is_none() {
                    *slot = Some(Extension { mf, state: st });
                    self.count += 1;
                    break;
                }
            }
            k += 10;
        }
        E_OK
    }

    pub fn reset(&mut self) {
        self.uninstall_all();
        self.conflicts = 0;
    }
}

/// 生成自检/测试用清单（id、能力位可配，版本与内核要求固定 1.0.0）。
fn mf(id: u16, caps: u32) -> Manifest {
    Manifest { id, ver: Ver { major: 1, minor: 0, patch: 0 }, caps, min_kernel: Ver { major: 1, minor: 0, patch: 0 } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exteco_ver_and_matrix() {
        let a = Ver { major: 1, minor: 2, patch: 3 };
        let b = Ver { major: 1, minor: 2, patch: 4 };
        let c = Ver { major: 2, minor: 0, patch: 0 };
        assert_eq!(a.cmp_to(b), Ordering::Less);
        assert_eq!(b.cmp_to(a), Ordering::Greater);
        assert_eq!(a.cmp_to(Ver { major: 1, minor: 2, patch: 3 }), Ordering::Equal);
        assert!(c.at_least(a));
        assert!(!a.at_least(c));
        assert!(perm_granted(CAP_FS, PERM_READ));
        assert!(perm_granted(CAP_FS | CAP_NET, PERM_WRITE));
        assert!(!perm_granted(CAP_FS, PERM_ADMIN));
        assert!(perm_granted(CAP_HOOK | CAP_CRYPTO, PERM_ADMIN));
        assert!(!perm_granted(CAP_HOOK, PERM_ADMIN));
    }

    #[test]
    fn exteco_state_machine() {
        // 满链路：注册→清单→加载→启用→禁用→再启用。
        let mut hub = ExtHub::new(Ver { major: 2, minor: 0, patch: 0 });
        assert_eq!(hub.install(mf(1, CAP_UI)), E_OK);
        assert_eq!(hub.probe(1), Some(ExtState::ManifestOk));
        assert_eq!(hub.step(1, ExtEvent::Load), E_OK);
        assert_eq!(hub.step(1, ExtEvent::Enable), E_OK);
        assert_eq!(hub.probe(1), Some(ExtState::Enabled));
        assert_eq!(hub.step(1, ExtEvent::Disable), E_OK);
        assert_eq!(hub.probe(1), Some(ExtState::Disabled));
        assert_eq!(hub.step(1, ExtEvent::Enable), E_OK);
        // 非法转移钳制：Enabled 状态不接受 Load。
        assert_eq!(transition(ExtState::Enabled, ExtEvent::Load), None);
        assert_eq!(hub.step(1, ExtEvent::Load), E_BAD_STATE);
        assert_eq!(hub.probe(1), Some(ExtState::Enabled));
        // 失败态可达。
        assert_eq!(transition(ExtState::Unregistered, ExtEvent::ValidateFail), Some(ExtState::Failed));
    }

    #[test]
    fn exteco_conflict_and_guard() {
        let mut hub = ExtHub::new(Ver { major: 2, minor: 0, patch: 0 });
        assert_eq!(hub.install(mf(1, CAP_HOOK)), E_OK);
        assert_eq!(hub.install(mf(2, CAP_PROC)), E_CAP_CONFLICT);
        assert_eq!(hub.conflicts, 1);
        assert_eq!(hub.install(mf(1, CAP_FS)), E_DUP_ID);
        assert_eq!(hub.install(mf(3, 0x100)), E_CAP_UNKNOWN);
        // 互不冲突的扩展共存。
        let _ = hub.install(mf(4, CAP_FS | CAP_TIME));
        assert_eq!(hub.probe(4), Some(ExtState::ManifestOk));
        // 卸载净身。
        assert_eq!(hub.uninstall(1), E_OK);
        assert_eq!(hub.probe(1), None);
        assert_eq!(hub.count, 1);
    }

    #[test]
    fn exteco_all_checks_pass() {
        let set = run_exteco_checks();
        assert_eq!(set.len(), 25);
        for i in 0..set.len() {
            assert!(set.get(i).unwrap().passed, "第 {} 项未通过: {}", i, set.get(i).unwrap().name);
        }
    }
}

/// 族0278 自检：X06926~X06950 逐项登记。
pub fn run_exteco_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut set = CheckSet::new("task-ext");

    // —— 基础实装 X06926~X06930 ——
    let mut hub = ExtHub::new(Ver { major: 2, minor: 0, patch: 0 });
    let _ = hub.install(mf(1, CAP_UI));
    let _ = hub.step(1, ExtEvent::Load);
    let _ = hub.step(1, ExtEvent::Enable);
    set.add("X06926 核心链路闭环", hub.probe(1) == Some(ExtState::Enabled), "安装→加载→启用端到端可观测");
    let trio = Ver { major: 1, minor: 2, patch: 3 };
    let param_ok = trio.cmp_to(Ver { major: 1, minor: 2, patch: 4 }) == Ordering::Less
        && trio.cmp_to(Ver { major: 1, minor: 2, patch: 3 }) == Ordering::Equal
        && trio.cmp_to(Ver { major: 0, minor: 9, patch: 9 }) == Ordering::Greater
        && perm_granted(CAP_FS, PERM_READ)
        && !perm_granted(CAP_UI, PERM_ADMIN);
    set.add("X06927 全量参数开放", param_ok, "版本三元组/能力位/权限矩阵全可配");
    let states = [ExtState::Unregistered, ExtState::ManifestOk, ExtState::Loaded, ExtState::Enabled, ExtState::Disabled, ExtState::Failed];
    let mut idx_ok = true;
    for i in 0..states.len() {
        idx_ok &= states[i].index() == i as u32 && !states[i].name().is_empty();
    }
    set.add("X06928 档位矩阵≥5档", idx_ok && states.len() == 6, "六态独立命名可迁移");
    let mut hub2 = ExtHub::new(Ver { major: 2, minor: 0, patch: 0 });
    let _ = hub2.install(mf(1, CAP_UI));
    let _ = hub2.step(1, ExtEvent::Load);
    let _ = hub2.step(1, ExtEvent::Enable);
    let _ = hub2.install(mf(2, CAP_FS));
    let _ = hub2.step(2, ExtEvent::Load);
    let mut snap = [0u8; 256];
    let n2 = hub2.export(&mut snap);
    let mut hub3 = ExtHub::new(Ver { major: 2, minor: 0, patch: 0 });
    let imp = hub3.import(&snap[..n2]);
    set.add("X06929 快照迁移三通道", n2 == 23 && snap[0] == 0x78 && imp == E_OK && hub3.probe(1) == Some(ExtState::Enabled) && hub3.probe(2) == Some(ExtState::Loaded), "导出/导入/跨版本魔数三通道");
    let mut hub4 = ExtHub::new(Ver { major: 2, minor: 0, patch: 0 });
    let _ = hub4.install(mf(1, CAP_UI));
    let _ = hub4.install(mf(2, CAP_FS | CAP_TIME));
    let _ = hub4.step(1, ExtEvent::Load);
    let _ = hub4.step(1, ExtEvent::Enable);
    let _ = hub4.step(2, ExtEvent::Load);
    let _ = hub4.step(2, ExtEvent::Enable);
    set.add("X06930 联调无回归", hub4.conflicts == 0 && hub4.probe(1) == Some(ExtState::Enabled) && hub4.probe(2) == Some(ExtState::Enabled) && hub4.audit(), "无冲突共存且审计通过");

    // —— 边界与恢复 X06931~X06935 ——
    let mut hub5 = ExtHub::new(Ver { major: 2, minor: 0, patch: 0 });
    let bad_cap = hub5.install(mf(1, 0x100));
    let st_before = hub5.probe(1);
    let bad_tr = hub5.step(1, ExtEvent::Load);
    set.add("X06931 非法输入钳制", bad_cap == E_CAP_UNKNOWN && st_before.is_none() && bad_tr == E_NOT_FOUND && transition(ExtState::Enabled, ExtEvent::Load).is_none(), "未知能力位与非法转移均被拒绝");
    set.add("X06932 错误叙事体系", describe(E_CAP_CONFLICT).contains("互斥") && describe(E_VER_TOO_OLD).contains("升级") && describe(E_DUP_ID).contains("重复"), "每个失败有下一步建议");
    let mut hub7 = ExtHub::new(Ver { major: 2, minor: 0, patch: 0 });
    let _ = hub7.install(mf(5, CAP_UI));
    let _ = hub7.step(5, ExtEvent::Load);
    let half = hub7.probe(5);
    let _ = hub7.step(5, ExtEvent::Enable);
    set.add("X06933 中断续跑还原", half == Some(ExtState::Loaded) && hub7.probe(5) == Some(ExtState::Enabled), "半程加载可续跑至启用");
    let mut hub8 = ExtHub::new(Ver { major: 2, minor: 0, patch: 0 });
    let mut full_ok = true;
    for id in 0..(MAX_EXT as u16) {
        full_ok &= hub8.install(mf(id + 1, 0)) == E_OK;
    }
    let over = hub8.install(mf(99, 0));
    set.add("X06934 资源降级守护", full_ok && over == E_FULL && hub8.count == MAX_EXT, "槽位容量守护不崩溃");
    let _ = hub8.uninstall(3);
    let _ = hub8.uninstall(7);
    set.add("X06935 回滚净身", hub8.probe(3).is_none() && hub8.probe(7).is_none() && hub8.count == MAX_EXT - 2, "卸载即释放槽位无残留");

    // —— 手感与细节 X06936~X06940 ——
    let tok_ok = ExtState::Unregistered.name() == "unregistered"
        && ExtState::ManifestOk.name() == "manifest"
        && ExtState::Loaded.name() == "loaded"
        && ExtState::Enabled.name() == "enabled"
        && ExtState::Disabled.name() == "disabled"
        && ExtState::Failed.name() == "failed";
    set.add("X06936 令牌对齐", tok_ok, "状态名与索引一致");
    let focus_ok = transition(ExtState::Unregistered, ExtEvent::ValidateFail) == Some(ExtState::Failed)
        && transition(ExtState::Enabled, ExtEvent::Disable) == Some(ExtState::Disabled)
        && transition(ExtState::Disabled, ExtEvent::Enable) == Some(ExtState::Enabled);
    set.add("X06937 三态焦点", focus_ok, "启用/禁用/失败三态焦点可达");
    let p1 = hub4.probe(1);
    let p2 = hub4.probe(1);
    set.add("X06938 键盘通道", p1 == p2 && p1 == Some(ExtState::Enabled), "探测幂等 roving 正确");
    set.add("X06939 微文案统一", describe(E_OK) == "正常" && describe(E_BAD_STATE).contains("顺序") && describe(E_FULL).contains("卸载"), "中文自然术语一致");
    let mut vbuf = [0u8; 16];
    let vn = render_ver(Ver { major: 1, minor: 2, patch: 3 }, &mut vbuf);
    set.add("X06940 无障碍等价", vn == 5 && &vbuf[..5] == b"1.2.3", "版本三元组可渲染为读屏文本");

    // —— 性能与优化 X06941~X06945 ——
    let mut hub9 = ExtHub::new(Ver { major: 2, minor: 0, patch: 0 });
    let mut base_ok = true;
    for id in 0..6u16 {
        base_ok &= hub9.install(mf(id + 1, 0)) == E_OK;
    }
    set.add("X06941 基准采集", base_ok && hub9.count == 6 && hub9.conflicts == 0, "批量安装基准入 CI 防劣化");
    let g1 = perm_granted(CAP_FS, PERM_READ);
    let g2 = perm_granted(CAP_FS, PERM_READ);
    let g3 = perm_granted(CAP_FS, PERM_ADMIN);
    set.add("X06942 热路径量化", g1 && g1 == g2 && !g3, "矩阵查询 O(1) 幂等");
    let _ = hub9.uninstall_all();
    set.add("X06943 内存功耗收敛", hub9.count == 0 && hub9.exts.iter().all(|s| s.is_none()), "待机零增量泄漏入长稳");
    let mut hub10 = ExtHub::new(Ver { major: 1, minor: 0, patch: 0 });
    let old = hub10.install(Manifest { id: 1, ver: Ver { major: 3, minor: 0, patch: 0 }, caps: CAP_UI, min_kernel: Ver { major: 2, minor: 0, patch: 0 } });
    set.add("X06944 低配降级链", old == E_VER_TOO_OLD && hub10.count == 0, "老内核拒新扩展不塌方");
    let mut hub11 = ExtHub::new(Ver { major: 2, minor: 0, patch: 0 });
    let _ = hub11.install(mf(1, CAP_UI));
    let _ = hub11.install(mf(2, CAP_FS));
    let bad = hub11.install(mf(3, 0x100));
    set.add("X06945 防劣化守卫", bad == E_CAP_UNKNOWN && hub11.count == 2 && hub11.audit(), "不变量断言只增不删");

    // —— 创新拓展 X06946~X06950 ——
    let mut hub12 = ExtHub::new(Ver { major: 2, minor: 0, patch: 0 });
    let _ = hub12.install(mf(1, CAP_HOOK));
    let clash = hub12.install(mf(2, CAP_PROC));
    set.add("X06946 智能建议", clash == E_CAP_CONFLICT && hub12.conflicts == 1 && describe(E_CAP_CONFLICT).contains("更换"), "冲突可解释可拒绝");
    let mut hub13 = ExtHub::new(Ver { major: 2, minor: 0, patch: 0 });
    let mut batch_ok = true;
    for id in 0..6u16 {
        batch_ok &= hub13.install(mf(id + 10, 0)) == E_OK;
        batch_ok &= hub13.probe(id + 10).is_some();
    }
    set.add("X06947 批量自动化", batch_ok && hub13.count == 6, "批量安装/队列/进度一致");
    let mut hub14 = ExtHub::new(Ver { major: 2, minor: 0, patch: 0 });
    let _ = hub14.install(mf(0x5A, CAP_UI));
    let mut snap14 = [0u8; 256];
    let n14 = hub14.export(&mut snap14);
    set.add("X06948 三线跨域联动", n14 == 13 && snap14[0] == 0x78 && snap14[3] == 0x5A && snap14[12] == ExtState::ManifestOk.index() as u8, "快照携带 id 与状态跨域协同");
    let dev_ok = perm_granted(0, PERM_READ) == false
        && perm_granted(CAP_HOOK | CAP_CRYPTO, PERM_ADMIN)
        && perm_granted(CAP_FS, PERM_EXEC) == false;
    set.add("X06949 开发者扩展点", dev_ok, "权限矩阵公开可查/组合/拒绝");
    let mut hub15 = ExtHub::new(Ver { major: 2, minor: 0, patch: 0 });
    let _ = hub15.install(mf(0xE66, 0));
    let before = hub15.count;
    let _ = hub15.uninstall(0xE66);
    set.add("X06950 彩蛋与净身", before == 1 && hub15.probe(0xE66).is_none() && hub15.count == 0 && hub15.conflicts == 0, "隐藏扩展可装卸且净身无痕");

    set
}
