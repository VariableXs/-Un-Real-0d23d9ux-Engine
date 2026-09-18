# 任务32 栈安全修复：测试堆构造 + 探针全初始化静态
import io

p = r"kernel/varix/src/shmsrv.rs"
s = io.open(p, encoding="utf-8").read()

# ① 探针：static 直接以 const 值初始化（全零 → .bss，零运行时栈物化），
#    不再走 Option<ShmStore> 运行时赋值路径（debug 构建下 1MiB 值可能栈中转）。
old = """    /// .bss 静态表（≈1 MiB；引导期单核、探针同步执行，static mut 无并发）。
    static mut STORE: Option<ShmStore> = None;

    fn store() -> &'static mut ShmStore {
        let slot = &raw mut STORE;
        // SAFETY: 引导期单核、探针独占运行（inputsvc::target 同范式）。
        unsafe {
            if (*slot).is_none() {
                *slot = Some(ShmStore::new());
            }
            (*slot).as_mut().unwrap()
        }
    }"""
new = """    /// .bss 静态表（≈1 MiB；引导期单核、探针同步执行，static mut 无并发）。
    /// 静态初始化器位置直接 const 求值——全零落 .bss，运行时零栈物化
    /// （Box/Option 中转路径在 debug 构建下会把 1MiB 值打上引导栈，任务56 同类教训）。
    static mut STORE: ShmStore = ShmStore::new();

    fn store() -> &'static mut ShmStore {
        // SAFETY: 引导期单核、探针独占运行（inputsvc::target 同范式）。
        unsafe { &mut *(&raw mut STORE) }
    }"""
assert s.count(old) == 1
s = s.replace(old, new, 1)

# ② 测试：堆构造不走 Box::new(new())（栈中转 1MiB 爆测试线程栈），
#    改 new_uninit + 整块清零（全零位模式 ≡ ShmStore::new()，语义合法）。
old = """    fn store() -> Box<ShmStore> {
        Box::new(ShmStore::new()) // 1 MiB：测试侧走堆，不占栈
    }"""
new = """    /// 堆上构造：`Box::new(ShmStore::new())` 会先在测试线程栈（Windows 默认
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
    }"""
assert s.count(old) == 1
s = s.replace(old, new, 1)

io.open(p, "w", encoding="utf-8", newline="").write(s)

s2 = io.open(p, encoding="utf-8").read()
assert "static mut STORE: ShmStore = ShmStore::new();" in s2
assert "new_uninit" in s2 and "Box::new(ShmStore::new())" not in s2
assert "Some(ShmStore::new())" not in s2
print("stack-safety patched ok")
