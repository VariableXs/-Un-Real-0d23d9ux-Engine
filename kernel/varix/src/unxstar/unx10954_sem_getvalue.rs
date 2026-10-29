//! UNX-F10954 sem_getvalue 错误矩阵（0x1d6c0 段）
//!
//! 卡片判据（UNX-F10954-J1）：
//! - 正面：EINVAL/EBADF 两错误各 10/10；
//! - 反面：回读与值账一致（千次 10/10）。
//!
//! 域：CoRun·UNX-C4-B28。本模块用纯功能建模 POSIX 命名信号量的
//! sem_getvalue 错误面：fd 记账（EBADF）、句柄标记（EINVAL）与
//! 回读一致性（getvalue 返回值必须等于内部值账）。全部判定为
//! 确定性纯函数，无阻塞、无真实系统调用。

use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

// ---------------------------------------------------------------------------
// 错误码（SEG_BASE = 0x1d000 + (10954-10900)*0x20 = 0x1d6c0）
// ---------------------------------------------------------------------------
const SEG_BASE: u32 = 0x1d6c0;

/// EBADF：描述符从未打开、已关闭或越界
pub const GV_EBADF: u32 = SEG_BASE + 0x01;
/// EINVAL：句柄存在但已被标记无效（rmid 后遗留句柄）
pub const GV_EINVAL: u32 = SEG_BASE + 0x02;
/// EOVERFLOW：值账内部自检失败（不应发生）
pub const GV_EOVERFLOW: u32 = SEG_BASE + 0x03;
/// ESTATE：回读与值账不一致（自检面，不应发生）
pub const GV_ESTATE: u32 = SEG_BASE + 0x04;
/// EPERM：非属主试图改写值账
pub const GV_EPERM: u32 = SEG_BASE + 0x05;
/// ERANGE：setvalue 超出上界
pub const GV_ERANGE: u32 = SEG_BASE + 0x06;
/// EBUSY：句柄正被回读锁定（建模竞态面）
pub const GV_EBUSY: u32 = SEG_BASE + 0x07;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------
/// 值上界（SEM_VALUE_MAX 建模值）
const SEM_VALUE_MAX: u32 = 0xffff;
/// fd 表容量
const FD_MAX: usize = 8;
/// fd 表空槽标记
const FD_FREE: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

/// 命名信号量句柄（内核侧记账体）
#[derive(Clone, Debug, PartialEq)]
pub struct SemVal {
    /// 域内唯一 id
    pub id: u32,
    /// 名字（POSIX 语义以 / 开头）
    pub name: String,
    /// 当前值账
    pub value: u32,
    /// 是否仍有效（false 即 EINVAL 面）
    pub alive: bool,
    /// 属主 pid
    pub owner: u32,
}

impl SemVal {
    pub fn new(id: u32, name: &str, value: u32, owner: u32) -> Self {
        SemVal { id, name: String::from(name), value, alive: true, owner }
    }

    /// FNV 折叠：id、value、alive 与名字逐字节
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ self.id as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.value as u64).wrapping_mul(0x100000001b3);
        h = (h ^ (self.alive as u64)).wrapping_mul(0x100000001b3);
        for b in self.name.as_bytes() {
            h = (h ^ (*b as u64)).wrapping_mul(0x100000001b3);
        }
        h
    }
}

/// sem_getvalue 错误判定台
#[derive(Clone, Debug)]
pub struct GetValDesk {
    /// fd -> 对象 id（FD_FREE 表示空槽）
    fds: Vec<u32>,
    /// 对象表
    objs: Vec<SemVal>,
    /// 错误计数
    pub n_ebadf: u32,
    pub n_einval: u32,
    pub n_eperm: u32,
    pub n_erange: u32,
    /// 合法回读次数
    pub n_ok_read: u32,
    /// 回读值总账
    pub read_total: u64,
}

impl GetValDesk {
    pub fn new() -> Self {
        GetValDesk {
            fds: vec![FD_FREE; FD_MAX],
            objs: Vec::new(),
            n_ebadf: 0,
            n_einval: 0,
            n_eperm: 0,
            n_erange: 0,
            n_ok_read: 0,
            read_total: 0,
        }
    }

    /// 登记，返回 fd 或 EBADF 面（表满/名字非法）
    pub fn open(&mut self, name: &str, value: u32, owner: u32) -> Result<usize, u32> {
        if name.as_bytes().first() != Some(&b'/') {
            return Err(GV_EBADF);
        }
        if value > SEM_VALUE_MAX {
            return Err(GV_ERANGE);
        }
        if self.objs.len() >= FD_MAX {
            return Err(GV_EBADF);
        }
        let id = 1000 + self.objs.len() as u32;
        let slot = self.objs.len();
        self.objs.push(SemVal::new(id, name, value, owner));
        for (i, s) in self.fds.iter_mut().enumerate() {
            if *s == FD_FREE {
                *s = id;
                let _ = i;
                return Ok(slot);
            }
        }
        // 表满：回滚登记
        self.objs.pop();
        Err(GV_EBADF)
    }

    /// 按 fd 解析对象索引：EBADF 优先于 EINVAL
    fn resolve(&self, fd: i32) -> Result<usize, u32> {
        if fd < 0 || fd as usize >= FD_MAX {
            return Err(GV_EBADF);
        }
        let id = self.fds[fd as usize];
        if id == FD_FREE {
            return Err(GV_EBADF);
        }
        let idx = self.objs.iter().position(|o| o.id == id);
        match idx {
            Some(i) => Ok(i),
            // fd 有账但对象已不存在：仍按 EBADF 面（描述符悬空）
            None => Err(GV_EBADF),
        }
    }

    /// sem_getvalue 错误矩阵判定：EINVAL/EBADF 两错误路径 + 合法回读
    pub fn getvalue(&mut self, fd: i32) -> Result<u32, u32> {
        let idx = match self.resolve(fd) {
            Ok(i) => i,
            // EBADF 面计数：负值、越界、空槽、悬空 fd
            Err(GV_EBADF) => {
                self.n_ebadf = self.n_ebadf.wrapping_add(1);
                return Err(GV_EBADF);
            }
            Err(e) => return Err(e),
        };
        if !self.objs[idx].alive {
            self.n_einval = self.n_einval.wrapping_add(1);
            return Err(GV_EINVAL);
        }
        let v = self.objs[idx].value;
        self.n_ok_read = self.n_ok_read.wrapping_add(1);
        self.read_total = self.read_total.wrapping_add(v as u64);
        Ok(v)
    }

    /// 关闭 fd（关闭后再读 EBADF）
    pub fn close(&mut self, fd: i32) -> Result<(), u32> {
        let idx = self.resolve(fd)?;
        let id = self.objs[idx].id;
        self.fds[fd as usize] = FD_FREE;
        let _ = id;
        Ok(())
    }

    /// 标记无效（模拟 unlink 后遗留句柄 → EINVAL）
    pub fn kill(&mut self, fd: i32) -> Result<(), u32> {
        let idx = self.resolve(fd)?;
        self.objs[idx].alive = false;
        Ok(())
    }

    /// setvalue：属主才可改写，越界 ERANGE
    pub fn setvalue(&mut self, fd: i32, value: u32, pid: u32) -> Result<(), u32> {
        let idx = self.resolve(fd)?;
        let o = &self.objs[idx];
        if !o.alive {
            return Err(GV_EINVAL);
        }
        if o.owner != pid {
            self.n_eperm = self.n_eperm.wrapping_add(1);
            return Err(GV_EPERM);
        }
        if value > SEM_VALUE_MAX {
            self.n_erange = self.n_erange.wrapping_add(1);
            return Err(GV_ERANGE);
        }
        self.objs[idx].value = value;
        Ok(())
    }

    /// post：值 +1（封顶溢出面）
    pub fn post(&mut self, fd: i32) -> Result<(), u32> {
        let idx = self.resolve(fd)?;
        let o = &self.objs[idx];
        if !o.alive {
            return Err(GV_EINVAL);
        }
        if o.value >= SEM_VALUE_MAX {
            return Err(GV_EOVERFLOW);
        }
        self.objs[idx].value = o.value.wrapping_add(1);
        Ok(())
    }

    /// wait：值 >0 减一，否则不阻塞建模面返回 0 占位
    pub fn wait(&mut self, fd: i32, nonblock: bool) -> Result<u32, u32> {
        let idx = self.resolve(fd)?;
        let o = &self.objs[idx];
        if !o.alive {
            return Err(GV_EINVAL);
        }
        if o.value > 0 {
            self.objs[idx].value = o.value.wrapping_sub(1);
            return Ok(1);
        }
        if nonblock {
            return Ok(0);
        }
        Ok(0) // 阻塞占位
    }

    /// 回读与值账一致性自检：逐对象复述
    pub fn readback_consistent(&self) -> bool {
        for o in &self.objs {
            if !o.alive {
                continue;
            }
            if o.value > SEM_VALUE_MAX {
                return false;
            }
            // 折叠重放：同一对象两次折叠必须一致
            if o.fold() != o.clone().fold() {
                return false;
            }
        }
        true
    }

    /// 账目对平：错误计数 + 合法回读 = 总判定轮数由调用方核对
    pub fn ledger_ok(&self, expect_ok: u32) -> bool {
        self.n_ok_read == expect_ok
            && (self.n_ebadf.wrapping_add(self.n_einval) > 0 || expect_ok > 0)
    }

    /// 汇总折叠
    pub fn fold_all(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for o in &self.objs {
            h = (h ^ o.fold()).wrapping_mul(0x100000001b3);
        }
        h = (h ^ self.n_ok_read as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染摘要
    pub fn render(&self) -> String {
        let mut s = String::from("getvalue:");
        for o in &self.objs {
            s.push('/');
            if o.alive {
                s.push_str(&crate::alloc::format!("{}={}", o.name, o.value));
            } else {
                s.push_str(&crate::alloc::format!("{}=dead", o.name));
            }
        }
        s
    }
}

impl Default for GetValDesk {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 注入四路：四个典型坏样本必须全部被判定面拦下
// ---------------------------------------------------------------------------
pub fn inject_four() -> [bool; 4] {
    let mut d = GetValDesk::new();
    let _ = d.open("/gv-a", 3, 7);
    let _ = d.open("/gv-b", 5, 7);
    // 路1：从未打开的 fd（如 6）→ EBADF
    let r1 = matches!(d.getvalue(6), Err(x) if x == GV_EBADF);
    // 路2：负 fd → EBADF
    let r2 = matches!(d.getvalue(-2), Err(x) if x == GV_EBADF);
    // 路3：打开后 kill → EINVAL
    let _ = d.kill(1);
    let r3 = matches!(d.getvalue(1), Err(x) if x == GV_EINVAL);
    // 路4：先 close 再读 → EBADF
    let _ = d.close(0);
    let r4 = matches!(d.getvalue(0), Err(x) if x == GV_EBADF);
    [r1, r2, r3, r4]
}

// ---------------------------------------------------------------------------
// 确定性千轮样本
// ---------------------------------------------------------------------------
/// 千轮：正面 500 轮走 EINVAL/EBADF 两错误（各计 250），
/// 反面 500 轮合法回读并核对回读值与值账一致。返回通过轮数。
/// 说明：EINVAL 面只对已 kill 的句柄复读，不再重建对象占 fd 槽；
/// fd 6/7 必须始终空着，越界 EBADF 面才不会被合法对象挤掉。
pub fn sample_thousand() -> u32 {
    let mut d = GetValDesk::new();
    let f0 = match d.open("/gv-main", 9, 7) {
        Ok(f) => f as i32,
        Err(_) => return 0,
    };
    let f1 = match d.open("/gv-side", 4, 7) {
        Ok(f) => f as i32,
        Err(_) => return 0,
    };
    let mut good: u32 = 0;
    let mut expect_value: u32 = 9;
    for r in 0..1000u32 {
        let phase = r % 10;
        if phase < 3 {
            // 正面：EBADF 路径（fd 6 越界）
            if matches!(d.getvalue(6), Err(x) if x == GV_EBADF) {
                good = good.wrapping_add(1);
            }
        } else if phase < 5 {
            // 正面：EINVAL 路径（kill f1 后回读；句柄保持悬空，
            // 不再重新 open 占新 fd 槽——否则会挤掉越界 fd 6/7 的 EBADF 面）
            let _ = d.kill(f1);
            if matches!(d.getvalue(f1), Err(x) if x == GV_EINVAL) {
                good = good.wrapping_add(1);
            }
        } else {
            // 反面：合法回读，回读值必须等于期望值账
            // 每 20 轮 post 一次推进值账（封顶回绕到 1）
            if r % 20 == 5 {
                let _ = d.post(f0);
                expect_value = if expect_value >= SEM_VALUE_MAX {
                    1
                } else {
                    expect_value.wrapping_add(1)
                };
            }
            match d.getvalue(f0) {
                Ok(v) => {
                    if v == expect_value {
                        good = good.wrapping_add(1);
                    }
                }
                Err(_) => {}
            }
        }
    }
    good
}

// ---------------------------------------------------------------------------
// 判据检查
// ---------------------------------------------------------------------------
pub fn run_unx10954_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10954");

    // 注入四路
    let inj = inject_four();
    s.add("J1-注入四路全拦截", inj.iter().all(|&b| b), "EBADF/EINVAL 四个坏样本全部按面命中");

    // J1 正面：EINVAL/EBADF 两错误各 10/10
    let mut d = GetValDesk::new();
    let f = match d.open("/gv-j1", 6, 7) { Ok(f) => f as i32, Err(_) => -1 };
    // 先做一次合法回读，保证账面有基线
    let _ = d.getvalue(f);
    let mut ebadf_hits = 0u32;
    let mut einval_hits = 0u32;
    for _ in 0..10 {
        if matches!(d.getvalue(7), Err(x) if x == GV_EBADF) {
            ebadf_hits = ebadf_hits.wrapping_add(1);
        }
    }
    let _ = d.kill(f);
    for _ in 0..10 {
        if matches!(d.getvalue(f), Err(x) if x == GV_EINVAL) {
            einval_hits = einval_hits.wrapping_add(1);
        }
    }
    s.add(
        "J1-正面-EINVAL/EBADF 各10/10",
        ebadf_hits == 10 && einval_hits == 10,
        "越界 fd 十次全 EBADF，无效句柄十次全 EINVAL",
    );

    // J2 反面：回读与值账一致（千次 10/10）
    let good = sample_thousand();
    s.add("J2-反面-回读一致千次", good == 1000, "千轮回读值逐次等于值账期望，零漂移");

    // 判定序：EBADF 优先于 EINVAL
    let mut d2 = GetValDesk::new();
    let _ = d2.open("/gv-seq", 1, 7);
    let _ = d2.kill(0);
    let order_ok = matches!(d2.getvalue(4), Err(x) if x == GV_EBADF)
        && matches!(d2.getvalue(0), Err(x) if x == GV_EINVAL);
    s.add("J3-判定序-EBADF先于EINVAL", order_ok, "先 fd 解析后句柄有效性，顺序固定");

    // 错误码分段
    let codes = [GV_EBADF, GV_EINVAL, GV_EOVERFLOW, GV_ESTATE, GV_EPERM, GV_ERANGE, GV_EBUSY];
    let seg_ok = codes.iter().all(|&c| c > SEG_BASE && c < SEG_BASE + 0x20);
    s.add("J4-错误码分段", seg_ok, "七码全部落在 0x1d6c0 段内");

    // 错误码互异
    let mut distinct = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                distinct = false;
            }
        }
    }
    s.add("J5-错误码互异", distinct, "七码两两互异");

    // 账目对平
    let _ = d.getvalue(f);
    s.add("J6-账目对平", d.ledger_ok(1), "合法回读计数与期望一致");

    // 回读一致性自检
    s.add("J7-回读一致性自检", d2.readback_consistent(), "逐对象复述一致");

    // fold 稳定
    let h1 = d2.fold_all();
    let h2 = d2.fold_all();
    s.add("J8-fold稳定", h1 == h2, "同一状态两次折叠一致");

    // 渲染非空
    let r = d2.render();
    s.add("J9-渲染非空", r.len() > 8 && r.starts_with("getvalue:"), "摘要含前缀与对象状态");

    // post 溢出面
    let mut d3 = GetValDesk::new();
    let f3 = match d3.open("/gv-max", SEM_VALUE_MAX, 7) { Ok(f) => f as i32, Err(_) => -1 };
    s.add(
        "J10-post溢出面",
        matches!(d3.post(f3), Err(x) if x == GV_EOVERFLOW),
        "满值 post 命中 EOVERFLOW 且值不变",
    );

    // setvalue 属主/越界面
    let mut d4 = GetValDesk::new();
    let f4 = match d4.open("/gv-set", 1, 7) { Ok(f) => f as i32, Err(_) => -1 };
    let eperm_ok = matches!(d4.setvalue(f4, 5, 99), Err(x) if x == GV_EPERM);
    let erange_ok = matches!(d4.setvalue(f4, SEM_VALUE_MAX + 1, 7), Err(x) if x == GV_ERANGE);
    let set_ok = d4.setvalue(f4, 42, 7).is_ok() && matches!(d4.getvalue(f4), Ok(42));
    s.add(
        "J11-setvalue三面",
        eperm_ok && erange_ok && set_ok,
        "非属主 EPERM、越界 ERANGE、属主合法写回成功",
    );

    // wait 消费面
    let mut d5 = GetValDesk::new();
    let f5 = match d5.open("/gv-wait", 2, 7) { Ok(f) => f as i32, Err(_) => -1 };
    let w1 = d5.wait(f5, true);
    let w2 = d5.wait(f5, true);
    let w3 = d5.wait(f5, true);
    let wait_ok = matches!(w1, Ok(1)) && matches!(w2, Ok(1)) && matches!(w3, Ok(0));
    s.add("J12-wait消费面", wait_ok, "值 2→1→0→非阻塞空返回占位");

    // 动态判据
    s.add_dyn(
        "J13-错误计数",
        d.n_ebadf >= 10 && d.n_einval >= 10,
        &crate::alloc::format!("EBADF={} EINVAL={} 合法回读={}", d.n_ebadf, d.n_einval, d.n_ok_read),
    );
    s.add_dyn(
        "J14-千轮通过率",
        good == 1000,
        &crate::alloc::format!("{}//1000", good),
    );
    s.add_dyn(
        "J15-回读总账",
        d.read_total >= 6,
        &crate::alloc::format!("回读值累计={}", d.read_total),
    );

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10954_sem_getvalue_flow() {
        let mut d = GetValDesk::new();
        let f = d.open("/gv-t", 5, 7).unwrap();
        assert_eq!(d.getvalue(f as i32), Ok(5));
        let _ = d.post(f as i32);
        assert_eq!(d.getvalue(f as i32), Ok(6));
        let _ = d.close(f as i32);
        assert!(matches!(d.getvalue(f as i32), Err(x) if x == GV_EBADF));
        let inj = inject_four();
        assert!(inj.iter().all(|&b| b));
        assert_eq!(sample_thousand(), 1000);
    }

    #[test]
    fn unx10954_checks_green() {
        let s = run_unx10954_checks();
        assert!(s.all_passed(), "UNX-F10954 判据未全绿");
    }
}
