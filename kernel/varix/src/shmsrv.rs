//! 共享内存块原语 + 授权模型 — AI-S（双域总案·阶段4 任务32）。
//!
//! 总案施工步骤 4 口径：
//! - **三语义**：`create` / `map` / `revoke`（另配 `destroy`/`unmap`/`access` 完成闭环）；
//! - **授权模型**：句柄不可传递默认（拿到句柄号 ≠ 有权映射）；创建者显式授权
//!   （只有 owner 能 grant，且被授权者**不得转授**）；revoke 即时失效授权与其映射；
//! - **块销毁后映射失效**：destroy 升代（generation），旧句柄/旧映射令牌全部变陈旧；
//!   句柄号永不复用同代（代际校验防"销毁后重放旧句柄"）。
//!
//! 实现形态：内核侧注册表模型（无 MMU 用户态前，"映射"=登记 + 令牌纪元校验；
//! 未来 ring3 落地后由 vmm 把 `access` 换成真实页映射，语义面不变）。
//!
//! **栈戒律**（任务56 实机教训）：`ShmStore` ≈1 MiB（16 块 × 64 KiB），禁止栈上/
//! Box 中转物化——实机探针走 `.bss` static；`create` 重置块必须逐字段原地写 +
//! `write_bytes` 清零，绝不构造整块临时值。

/// 块上限：16。定容免分配。
pub const SHM_MAX_BLOCKS: usize = 16;
/// 单块容量：64 KiB（页对齐倍数；未来 vmm 映射的粒度上限）。
pub const SHM_SLOT_BYTES: usize = 65_536;
/// 每块授权槽：4（创建者显式授权名单）。
pub const GRANTS_PER_BLOCK: usize = 4;
/// 每块映射槽：8（活动映射登记）。
pub const MAPS_PER_BLOCK: usize = 8;

/// 错误码：全部分支显式，绝不静默。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShmError {
    /// 句柄不存在 / 代际不符（含销毁后旧句柄重放）。
    NotFound,
    /// 未授权 / 非 owner 操作 / 越权写。默认拒绝的唯一出口。
    PermissionDenied,
    /// 块表满（17 块请求）。
    NoBlocks,
    /// 授权名单满（第 5 个被授权者）。
    NoGrantSlot,
    /// 映射槽满（第 9 个映射）。
    NoMapSlot,
    /// 尺寸超块容量。
    TooBig,
    /// 零尺寸 / 重复授权 / 撤销未授权者等参数错。
    BadArgs,
    /// 映射令牌已陈旧（被 revoke / destroy / 代际更替）。
    StaleMapping,
}

/// 授权槽。
#[derive(Clone, Copy, PartialEq, Eq)]
struct GrantSlot {
    live: bool,
    pid: u32,
}

/// 映射槽：登记一次活动映射。
#[derive(Clone, Copy, PartialEq, Eq)]
struct MapSlot {
    live: bool,
    pid: u32,
    /// 块内映射序号（map 时递增；令牌携带，access 校验）。
    seq: u32,
    writable: bool,
}

/// 块：缓冲 + 授权名单 + 映射登记 + 代。
#[derive(Clone, Copy)]
struct Block {
    used: bool,
    /// 代数：destroy 时 +1；句柄携带创建时代数，代不符即 NotFound（防重放）。
    gen: u32,
    owner: u32,
    size: u32,
    buf: [u8; SHM_SLOT_BYTES],
    grants: [GrantSlot; GRANTS_PER_BLOCK],
    maps: [MapSlot; MAPS_PER_BLOCK],
    map_seq: u32,
}

impl Block {
    const EMPTY: Block = Block {
        used: false,
        gen: 0,
        owner: 0,
        size: 0,
        buf: [0; SHM_SLOT_BYTES],
        grants: [GrantSlot { live: false, pid: 0 }; GRANTS_PER_BLOCK],
        maps: [MapSlot { live: false, pid: 0, seq: 0, writable: false }; MAPS_PER_BLOCK],
        map_seq: 0,
    };
}

/// 映射令牌：map 的返回物。值本身也不可传递——`access` 还要核对 pid。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mapping {
    pub handle: u64,
    pub seq: u32,
    pub writable: bool,
}

/// 共享内存块表。≈1 MiB，只许 static/Box 承载，禁止栈上物化。
#[derive(Clone, Copy)]
pub struct ShmStore {
    blocks: [Block; SHM_MAX_BLOCKS],
}

impl ShmStore {
    /// const 构造（全零 → static 落 .bss，不占镜像）。
    pub const fn new() -> Self {
        ShmStore {
            blocks: [Block::EMPTY; SHM_MAX_BLOCKS],
        }
    }

    /// 句柄编码：高 32 位 = 创建代数，低 32 位 = 块号。
    pub const fn encode(idx: usize, gen: u32) -> u64 {
        ((gen as u64) << 32) | (idx as u64)
    }

    fn resolve_mut(&mut self, h: u64) -> Result<&mut Block, ShmError> {
        let idx = (h & 0xFFFF_FFFF) as usize;
        let gen = (h >> 32) as u32;
        if idx >= SHM_MAX_BLOCKS {
            return Err(ShmError::NotFound);
        }
        let b = &mut self.blocks[idx];
        if !b.used || b.gen != gen {
            return Err(ShmError::NotFound);
        }
        Ok(b)
    }

    /// 创建块：owner 独占；尺寸 1..=SHM_SLOT_BYTES。逐字段原地写 + write_bytes 清零
    /// （绝不构造整块临时值——64 KiB 临时会碎在引导栈上）。
    pub fn create(&mut self, owner: u32, size: u32) -> Result<u64, ShmError> {
        if size == 0 {
            return Err(ShmError::BadArgs);
        }
        if size as usize > SHM_SLOT_BYTES {
            return Err(ShmError::TooBig);
        }
        let slot = (0..SHM_MAX_BLOCKS).find(|&i| !self.blocks[i].used).ok_or(ShmError::NoBlocks)?;
        let b = &mut self.blocks[slot];
        // SAFETY: b.buf 可写；write_bytes 原地清零，无栈中转。
        unsafe { core::ptr::write_bytes(b.buf.as_mut_ptr(), 0, SHM_SLOT_BYTES) };
        b.used = true;
        b.owner = owner;
        b.size = size;
        for g in b.grants.iter_mut() {
            g.live = false;
        }
        for m in b.maps.iter_mut() {
            m.live = false;
        }
        b.map_seq = 0;
        Ok(Self::encode(slot, b.gen))
    }

    /// 创建者显式授权。被授权者不得转授（本函数只认 owner）；重复授权拒绝。
    pub fn grant(&mut self, h: u64, owner: u32, grantee: u32) -> Result<(), ShmError> {
        let b = self.resolve_mut(h)?;
        if b.owner != owner || grantee == owner {
            return Err(ShmError::PermissionDenied);
        }
        if b.grants.iter().any(|g| g.live && g.pid == grantee) {
            return Err(ShmError::BadArgs);
        }
        let slot = b.grants.iter_mut().find(|g| !g.live).ok_or(ShmError::NoGrantSlot)?;
        *slot = GrantSlot { live: true, pid: grantee };
        Ok(())
    }

    /// 撤销授权：授权位即时失效，**该被授权者的既有映射一并失效**。
    pub fn revoke(&mut self, h: u64, owner: u32, grantee: u32) -> Result<(), ShmError> {
        let b = self.resolve_mut(h)?;
        if b.owner != owner {
            return Err(ShmError::PermissionDenied);
        }
        let had = b.grants.iter_mut().any(|g| g.live && g.pid == grantee);
        if !had {
            return Err(ShmError::BadArgs);
        }
        for g in b.grants.iter_mut() {
            if g.live && g.pid == grantee {
                g.live = false;
            }
        }
        for m in b.maps.iter_mut() {
            if m.live && m.pid == grantee {
                m.live = false; // 映射随授权失效
            }
        }
        Ok(())
    }

    /// 映射：owner 隐式有权；被授权者经名单放行；其余默认拒绝。
    /// 返回令牌（值同样不可传递——access 复核 pid）。
    pub fn map(&mut self, h: u64, requester: u32, writable: bool) -> Result<Mapping, ShmError> {
        let b = self.resolve_mut(h)?;
        let authorized = requester == b.owner || b.grants.iter().any(|g| g.live && g.pid == requester);
        if !authorized {
            return Err(ShmError::PermissionDenied);
        }
        b.map_seq = b.map_seq.wrapping_add(1);
        let seq = b.map_seq;
        let slot = b.maps.iter_mut().find(|m| !m.live).ok_or(ShmError::NoMapSlot)?;
        *slot = MapSlot { live: true, pid: requester, seq, writable };
        Ok(Mapping { handle: h, seq, writable })
    }

    /// 经令牌访问：核对块存活 + 代 + 映射槽（pid/seq/writable 全对上）。
    /// 返回块可见区（`buf[..size]`）。写访问须令牌可写。
    pub fn access(&mut self, m: &Mapping, pid: u32, write: bool) -> Result<&mut [u8], ShmError> {
        if write && !m.writable {
            return Err(ShmError::PermissionDenied);
        }
        let b = self.resolve_mut(m.handle)?;
        let ok = b.maps.iter().any(|s| s.live && s.pid == pid && s.seq == m.seq);
        if !ok {
            return Err(ShmError::StaleMapping);
        }
        let size = b.size as usize;
        Ok(&mut b.buf[..size])
    }

    /// 解除单个映射。
    pub fn unmap(&mut self, m: &Mapping, pid: u32) -> Result<(), ShmError> {
        let b = self.resolve_mut(m.handle)?;
        let mut found = false;
        for s in b.maps.iter_mut() {
            if s.live && s.pid == pid && s.seq == m.seq {
                s.live = false;
                found = true;
            }
        }
        if found {
            Ok(())
        } else {
            Err(ShmError::StaleMapping)
        }
    }

    /// 销毁块（仅 owner）：升代使句柄与全部映射/授权即刻失效。
    pub fn destroy(&mut self, h: u64, owner: u32) -> Result<(), ShmError> {
        let idx = (h & 0xFFFF_FFFF) as usize;
        let gen = (h >> 32) as u32;
        if idx >= SHM_MAX_BLOCKS {
            return Err(ShmError::NotFound);
        }
        let b = &mut self.blocks[idx];
        if !b.used || b.gen != gen {
            return Err(ShmError::NotFound);
        }
        if b.owner != owner {
            return Err(ShmError::PermissionDenied);
        }
        b.used = false;
        b.gen = b.gen.wrapping_add(1);
        for g in b.grants.iter_mut() {
            g.live = false;
        }
        for m in b.maps.iter_mut() {
            m.live = false;
        }
        b.map_seq = 0;
        Ok(())
    }

    /// 统计快照：(存活块, 活动授权, 活动映射)。
    pub fn stats(&self) -> (usize, usize, usize) {
        let mut used = 0;
        let mut grants = 0;
        let mut maps = 0;
        for b in self.blocks.iter() {
            if !b.used {
                continue;
            }
            used += 1;
            grants += b.grants.iter().filter(|g| g.live).count();
            maps += b.maps.iter().filter(|m| m.live).count();
        }
        (used, grants, maps)
    }
}

// ---------------------------------------------------------------------------
// 实机探针（target_os = "none"）：句柄传递攻击全矩阵，纯计算不碰设备。
// ---------------------------------------------------------------------------

#[cfg(target_os = "none")]
pub mod target {
    use super::{Mapping, ShmError, ShmStore};

    /// .bss 静态表（≈1 MiB；引导期单核、探针同步执行，static mut 无并发）。
    /// 静态初始化器位置直接 const 求值——全零落 .bss，运行时零栈物化
    /// （Box/Option 中转路径在 debug 构建下会把 1MiB 值打上引导栈，任务56 同类教训）。
    static mut STORE: ShmStore = ShmStore::new();

    fn store() -> &'static mut ShmStore {
        // SAFETY: 引导期单核、探针独占运行（inputsvc::target 同范式）。
        unsafe { &mut *(&raw mut STORE) }
    }

    /// 单步断言打印：失败即 kwarn 并返回 false（探针壳统一汇总）。
    macro_rules! step {
        ($ok:expr, $($arg:tt)*) => {{
            let ok: bool = $ok;
            if ok {
                crate::kinfo!("shm-probe: {} verdict=ok", format_args!($($arg)*));
            } else {
                crate::kwarn!("shm-probe: {} verdict=FAIL", format_args!($($arg)*));
            }
            ok
        }};
    }

    /// 任务32 实机探针：授权模型全矩阵（默认拒绝/显式授权/转授拒绝/句柄传递
    /// 攻击被拒/revoke 即时失效/destroy 升代防重放）。
    pub fn shm_probe() {
        let s = store();
        const OWNER: u32 = 101;
        const A: u32 = 202;
        const B: u32 = 303;

        // ① 创建 + owner 映射读写回环。
        let h = match s.create(OWNER, 4096) {
            Ok(x) => x,
            Err(_) => {
                crate::kwarn!("shm-probe: create failed");
                return;
            }
        };
        let m_own = match s.map(h, OWNER, true) {
            Ok(x) => x,
            Err(_) => {
                crate::kwarn!("shm-probe: owner map failed");
                return;
            }
        };
        let ok1 = step!(s.access(&m_own, OWNER, true).map(|b| b[0] = 0x5A).is_ok()
            && s.access(&m_own, OWNER, false).map(|b| b[0] == 0x5A).unwrap_or(false),
            "owner roundtrip");

        // ② 句柄不可传递默认：A 拿到句柄号也不能映射。
        let ok2 = step!(s.map(h, A, true) == Err(ShmError::PermissionDenied), "no-pass default-deny");

        // ③ 创建者显式授权 → A 可映射可写；A 转授 B 拒绝。
        let g = s.grant(h, OWNER, A).is_ok();
        let m_a = s.map(h, A, true).ok();
        let ok3 = step!(g && m_a.is_some() && s.grant(h, A, B) == Err(ShmError::PermissionDenied),
            "grant-and-no-regrant");

        // ④ 句柄传递攻击：A 把句柄号+令牌都交给 B，B 两条路都被拒。
        let stolen = m_a.unwrap_or(Mapping { handle: 0, seq: 0, writable: true });
        let ok4 = step!(s.map(h, B, true) == Err(ShmError::PermissionDenied)
            && s.access(&stolen, B, false) == Err(ShmError::StaleMapping),
            "handle-passing-attack denied");

        // ⑤ revoke 即时失效：A 的授权位与既有映射同时死。
        let rv = s.revoke(h, OWNER, A).is_ok();
        let ok5 = step!(rv
            && s.map(h, A, true) == Err(ShmError::PermissionDenied)
            && s.access(&stolen, A, false) == Err(ShmError::StaleMapping),
            "revoke invalidates grant+mapping");

        // ⑥ destroy 升代：全部失效；同号旧句柄重放 NotFound；重建后新代可用。
        let dst = s.destroy(h, OWNER).is_ok();
        let re = s.create(OWNER, 4096);
        let ok6 = step!(dst
            && s.access(&stolen, OWNER, false) == Err(ShmError::NotFound)
            && s.map(h, OWNER, false) == Err(ShmError::NotFound)
            && re.is_ok()
            && re.unwrap_or(0) != h,
            "destroy bumps gen (handle never reused same-gen)");

        // ⑦ 汇总：块表/授权/映射计数归位。
        let (used, grants, maps) = s.stats();
        let ok7 = step!(used == 1 && grants == 0 && maps == 0, "stats after drill used={} g={} m={}", used, grants, maps);
        crate::kinfo!(
            "shm-probe: verdict={} matrix=[roundtrip,default-deny,grant,revoke,attack,destroy-gen]",
            ok1 && ok2 && ok3 && ok4 && ok5 && ok6 && ok7
        );
    }
}

// ---------------------------------------------------------------------------
// 宿主测试（ktest）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 堆上构造：`Box::new(ShmStore::new())` 会先在测试线程栈（Windows 默认
    /// 1 MiB）物化 1 MiB 临时值 → STATUS_STACK_OVERFLOW。改 new_uninit + 整块
    /// 原地清零——全零位模式与 `ShmStore::new()` 逐字段等价（无引用/枚举陷阱）。
    fn store() -> Box<ShmStore> {
        let mut b = Box::<ShmStore>::new_uninit();
        // SAFETY: 清零后 assume_init 合法——ShmStore 全部字段零值即 EMPTY 态。
        unsafe {
            core::ptr::write_bytes(
                b.as_mut_ptr() as *mut u8,
                0,
                core::mem::size_of::<ShmStore>(),
            );
            b.assume_init()
        }
    }

    #[test]
    fn create_map_roundtrip_owner_rw() {
        let mut s = store();
        let h = s.create(7, 4096).unwrap();
        let m = s.map(h, 7, true).unwrap();
        s.access(&m, 7, true).unwrap()[..4].copy_from_slice(&[1, 2, 3, 4]);
        assert_eq!(&s.access(&m, 7, false).unwrap()[..4], &[1, 2, 3, 4]);
        // 可见区受 size 约束：越界写入不存在（切片到 size）。
        assert_eq!(s.access(&m, 7, false).unwrap().len(), 4096);
    }

    #[test]
    fn map_without_grant_denied() {
        let mut s = store();
        let h = s.create(7, 64).unwrap();
        assert_eq!(s.map(h, 8, false), Err(ShmError::PermissionDenied), "句柄不可传递默认");
        assert_eq!(s.map(0xDEAD_BEEF, 8, false), Err(ShmError::NotFound));
    }

    #[test]
    fn grant_enables_map_and_forbids_regrant() {
        let mut s = store();
        let h = s.create(7, 64).unwrap();
        s.grant(h, 7, 8).unwrap();
        let m8 = s.map(h, 8, true).unwrap();
        s.access(&m8, 8, true).unwrap()[0] = 9;
        assert_eq!(s.access(&m8, 8, false).unwrap()[0], 9);
        // 被授权者不得转授（创建者显式授权唯一出口）。
        assert_eq!(s.grant(h, 8, 9), Err(ShmError::PermissionDenied));
        assert_eq!(s.map(h, 9, false), Err(ShmError::PermissionDenied));
        // 重复授权拒绝；非 owner 授权拒绝。
        assert_eq!(s.grant(h, 7, 8), Err(ShmError::BadArgs));
        assert_eq!(s.grant(h, 9, 8), Err(ShmError::PermissionDenied));
    }

    #[test]
    fn handle_passing_attack_denied() {
        let mut s = store();
        let h = s.create(7, 64).unwrap();
        s.grant(h, 7, 8).unwrap();
        let m8 = s.map(h, 8, true).unwrap();
        // 8 把"句柄号 + 令牌值"都泄露给 9（模拟句柄传递攻击）。
        assert_eq!(s.map(h, 9, false), Err(ShmError::PermissionDenied), "句柄号泄露 ≠ 有权");
        assert_eq!(
            s.access(&m8, 9, false),
            Err(ShmError::StaleMapping),
            "令牌值泄露 ≠ 有权（pid 复核）"
        );
        // 写方向同样被拒（令牌可写性成立，但 pid 复核先死于槽位）。
        assert_eq!(s.access(&m8, 9, true), Err(ShmError::StaleMapping));
    }

    #[test]
    fn revoke_invalidates_grant_and_mapping() {
        let mut s = store();
        let h = s.create(7, 64).unwrap();
        s.grant(h, 7, 8).unwrap();
        let m8 = s.map(h, 8, true).unwrap();
        s.revoke(h, 7, 8).unwrap();
        assert_eq!(s.map(h, 8, false), Err(ShmError::PermissionDenied), "授权位已死");
        assert_eq!(s.access(&m8, 8, false), Err(ShmError::StaleMapping), "既有映射一并失效");
        // 撤销未授权者 / 非 owner 撤销：显式报错。
        assert_eq!(s.revoke(h, 7, 8), Err(ShmError::BadArgs));
        assert_eq!(s.revoke(h, 8, 7), Err(ShmError::PermissionDenied));
    }

    #[test]
    fn destroy_invalidates_all_and_handles_never_reused() {
        let mut s = store();
        let h = s.create(7, 64).unwrap();
        s.grant(h, 7, 8).unwrap();
        let m8 = s.map(h, 8, false).unwrap();
        assert_eq!(s.destroy(h, 8), Err(ShmError::PermissionDenied), "非 owner 不得销毁");
        s.destroy(h, 7).unwrap();
        assert_eq!(s.access(&m8, 8, false), Err(ShmError::NotFound), "块已不存在");
        assert_eq!(s.map(h, 7, false), Err(ShmError::NotFound), "旧句柄（旧代）重放被拒");
        // 同槽重建 → 新代新句柄；旧句柄与新句柄号必不相同。
        let h2 = s.create(9, 64).unwrap();
        assert_eq!((h2 >> 32) as u32, 1, "gen 已升到 1");
        assert_ne!(h, h2);
        assert_eq!(s.map(h2, 9, false).is_ok(), true);
        assert_eq!(s.map(h, 7, false), Err(ShmError::NotFound), "销毁前旧句柄永远无效");
    }

    #[test]
    fn grant_and_map_slots_exhausted_then_reused() {
        let mut s = store();
        let h = s.create(7, 64).unwrap();
        for pid in 100..104u32 {
            s.grant(h, 7, pid).unwrap();
        }
        assert_eq!(s.grant(h, 7, 104), Err(ShmError::NoGrantSlot), "第 5 个授权明确拒绝");
        for _ in 0..4usize {
            assert!(s.map(h, 7, false).is_ok(), "owner 可重复映射（占满槽位）");
        }
        for pid in 100..104u32 {
            assert!(s.map(h, pid, false).is_ok(), "授权者可映射");
        }
        assert_eq!(s.map(h, 7, false), Err(ShmError::NoMapSlot), "第 9 个映射明确拒绝");
        // unmap 释放槽位后可复用（seq 1 = owner 首次映射）。
        let first = Mapping { handle: h, seq: 1, writable: false };
        s.unmap(&first, 7).unwrap();
        assert!(s.map(h, 7, false).is_ok());
        assert_eq!(s.unmap(&first, 7), Err(ShmError::StaleMapping));
    }

    #[test]
    fn size_limits_and_capacity() {
        let mut s = store();
        assert_eq!(s.create(7, 0), Err(ShmError::BadArgs));
        assert_eq!(s.create(7, SHM_SLOT_BYTES as u32 + 1), Err(ShmError::TooBig));
        assert!(s.create(7, SHM_SLOT_BYTES as u32).is_ok(), "满容量单块合法");
        for _ in 1..SHM_MAX_BLOCKS {
            assert!(s.create(7, 64).is_ok());
        }
        assert_eq!(s.create(7, 64), Err(ShmError::NoBlocks), "第 17 块明确拒绝");
        assert_eq!(s.stats(), (SHM_MAX_BLOCKS, 0, 0));
    }
}
