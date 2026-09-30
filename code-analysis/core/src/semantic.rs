//! AI-07 · 语义系统（#516~#530）。
//!
//! 三层架构：语义词典层（250 概念 × 9 语言 = 2250 条映射）→
//! 语义推理层（消歧 / 领域适配 / 距离计算 / 最佳匹配）→
//! 应用层（翻译 / 命名 / 理解 / 转换 / 比喻修正）。
//! 零 AI：全部为确定性查表与加权距离计算，不调用任何网络模型。

/// 9 种目标语言（与主规格「32. 语义系统」示例表列序一致）。
pub const LANGS: [&str; 9] = ["Python", "Java", "JS", "Go", "Rust", "C++", "C#", "Kotlin", "Swift"];

/// 语义词典条目：(语义ID, 中文概念, [9 语言等价写法], 领域/类别)。
pub type Concept = (&'static str, &'static str, [&'static str; 9], &'static str);

/// 语义词典：#516 250 概念 × 9 语言。
pub const DICT: &[Concept] = &[
    // ── 并发与异步（20）
    ("ASYNC", "异步", ["async/await", "Future/CompletableFuture", "Promise", "goroutine", "async/await", "std::future", "Task/async", "suspend fun", "async/await"], "并发"),
    ("AWAIT", "等待完成", ["await", ".get()", "await", "<-ch", ".await", "co_await", "await", "await", "await"], "并发"),
    ("GOROUTINE", "轻量线程", ["threading.Thread", "Thread", "Worker", "goroutine", "std::thread", "std::thread", "Task", "coroutine", "Task"], "并发"),
    ("THREAD", "线程", ["threading.Thread", "Thread", "Worker", "go f()", "std::thread::spawn", "std::thread", "Thread", "Thread", "Thread"], "并发"),
    ("LOCK", "互斥锁", ["threading.Lock", "synchronized", "Mutex(库)", "sync.Mutex", "Mutex", "std::mutex", "lock/Monitor", "synchronized", "NSLock"], "并发"),
    ("RWLOCK", "读写锁", ["threading.RLock", "ReentrantReadWriteLock", "（无内置）", "sync.RWMutex", "RwLock", "std::shared_mutex", "ReaderWriterLockSlim", "ReentrantReadWriteLock", "pthread_rwlock"], "并发"),
    ("ATOMIC", "原子操作", ["（无内置）", "AtomicInteger", "Atomics", "sync/atomic", "AtomicUsize", "std::atomic", "Interlocked", "AtomicInteger", "OSAtomic"], "并发"),
    ("CHANNEL", "通道", ["queue.Queue", "BlockingQueue", "（无内置）", "chan", "mpsc::channel", "（无内置）", "Channel", "Channel", "DispatchQueue"], "并发"),
    ("SELECT", "多路选择", ["selectors", "Selector", "Promise.race", "select", "tokio::select!", "poll", "Task.WhenAny", "select", "DispatchGroup"], "并发"),
    ("SEMAPHORE", "信号量", ["threading.Semaphore", "Semaphore", "（无内置）", "semaphore.Weighted", "tokio::sync::Semaphore", "std::counting_semaphore", "SemaphoreSlim", "Semaphore", "DispatchSemaphore"], "并发"),
    ("BARRIER", "屏障", ["threading.Barrier", "CyclicBarrier", "（无内置）", "sync.WaitGroup", "std::barrier", "std::barrier", "Barrier", "CyclicBarrier", "DispatchGroup.wait"], "并发"),
    ("CONDVAR", "条件变量", ["threading.Condition", "wait/notify", "（无内置）", "sync.Cond", "Condvar", "std::condition_variable", "Monitor.Wait", "Object.wait", "NSCondition"], "并发"),
    ("THREADPOOL", "线程池", ["ThreadPoolExecutor", "ExecutorService", "Worker Pool", "Worker Pool", "rayon::ThreadPool", "std::async", "ThreadPool", "Dispatchers.Default", "DispatchQueue.concurrent"], "并发"),
    ("FUTURE", "未来值", ["asyncio.Future", "CompletableFuture", "Promise", "（无内置）", "impl Future", "std::future", "Task", "Deferred", "Future"], "并发"),
    ("COROUTINE", "协程", ["async def", "（虚拟线程）", "async function", "goroutine", "async fn", "C++20 coroutine", "async/await", "suspend fun", "async func"], "并发"),
    ("SPAWN", "派生任务", ["asyncio.create_task", "Thread.start", "Promise 构造", "go f()", "tokio::spawn", "std::thread", "Task.Run", "launch{}", "Task.detached"], "并发"),
    ("JOIN", "汇合等待", ["await task", "Future.get", "await Promise.all", "wg.Wait", "handle.await", "thread::join", "Task.WaitAll", "awaitAll", "await"], "并发"),
    ("SCOPE", "结构化并发", ["asyncio.TaskGroup", "StructuredTaskScope", "Promise.all", "errgroup.Group", "JoinSet", "std::jthread", "Task.WhenAll", "coroutineScope", "withTaskGroup"], "并发"),
    ("WORKQUEUE", "工作队列", ["queue.Queue", "BlockingQueue", "async queue", "chan buffer", "crossbeam channel", "std::queue+mutex", "BlockingCollection", "Channel", "OperationQueue"], "并发"),
    ("LOCKFREE", "无锁编程", ["（无内置）", "AtomicReference", "Atomics", "sync/atomic", "Arc<Atomic>", "std::atomic", "Interlocked", "AtomicReference", "OSAtomic"], "并发"),
    // ── 数据结构（20）
    ("LIST", "列表", ["list", "ArrayList", "Array", "slice", "Vec", "std::vector", "List", "MutableList", "Array"], "数据"),
    ("MAP", "键值对", ["dict", "HashMap", "Map", "map", "HashMap", "std::unordered_map", "Dictionary", "MutableMap", "Dictionary"], "数据"),
    ("SET", "集合", ["set", "HashSet", "Set", "map[T]struct{}", "HashSet", "std::unordered_set", "HashSet", "MutableSet", "Set"], "数据"),
    ("QUEUE", "队列", ["collections.deque", "ArrayDeque", "Array 队列", "slice 队列", "VecDeque", "std::deque", "Queue", "ArrayDeque", "Array 队列"], "数据"),
    ("STACK", "栈", ["list 模拟", "ArrayDeque", "Array 栈", "slice 栈", "Vec", "std::stack", "Stack", "ArrayDeque", "Array 栈"], "数据"),
    ("DEQUE", "双端队列", ["collections.deque", "ArrayDeque", "Array 双端", "slice 双端", "VecDeque", "std::deque", "LinkedList", "ArrayDeque", "Deque"], "数据"),
    ("HEAP", "堆/优先队列", ["heapq", "PriorityQueue", "（无内置）", "container/heap", "BinaryHeap", "std::priority_queue", "PriorityQueue", "PriorityQueue", "（无内置）"], "数据"),
    ("TREE", "有序树", ["（自建/类）", "TreeMap", "（对象）", "（自建）", "BTreeMap", "std::map", "SortedDictionary", "TreeMap", "（自建）"], "数据"),
    ("GRAPH", "图", ["networkx", "（自建）", "（对象）", "（自建）", "petgraph", "boost::graph", "（自建）", "（自建）", "（自建）"], "数据"),
    ("TUPLE", "元组", ["tuple", "record", "Array 解构", "多返回值", "(A, B)", "std::tuple", "ValueTuple", "Pair", "tuple"], "数据"),
    ("ARRAY", "定长数组", ["array/bytes", "T[]", "TypedArray", "[N]T", "[T; N]", "std::array", "T[]", "Array", "Array"], "数据"),
    ("STRING", "字符串", ["str", "String", "string", "string", "String", "std::string", "string", "String", "String"], "数据"),
    ("BYTES", "字节序列", ["bytes", "byte[]", "Uint8Array", "[]byte", "Vec<u8>", "std::vector<uint8_t>", "byte[]", "ByteArray", "Data"], "数据"),
    ("BUFFER", "缓冲", ["io.BytesIO", "ByteBuffer", "ArrayBuffer", "bytes.Buffer", "Vec<u8> 缓冲", "std::stringbuf", "MemoryStream", "ByteBuffer", "Data"], "数据"),
    ("BITSET", "位集", ["int 位运算", "BitSet", "（BigInt）", "math/bits", "bitvec", "std::bitset", "BitArray", "（BigInteger）", "OptionSet"], "数据"),
    ("LINKEDLIST", "链表", ["（自建）", "LinkedList", "（对象链）", "container/list", "LinkedList", "std::list", "LinkedList", "LinkedList", "（自建）"], "数据"),
    ("HASHSET", "哈希集合", ["set", "HashSet", "Set", "map[T]struct{}", "HashSet", "std::unordered_set", "HashSet", "HashSet", "Set"], "数据"),
    ("RINGBUFFER", "环形缓冲", ["deque(maxlen)", "（自建）", "（自建）", "ring buffer", "ringbuf", "boost::circular_buffer", "（自建）", "（自建）", "（自建）"], "数据"),
    ("MATRIX", "矩阵", ["list[list]", "double[][]", "Array 嵌套", "[][]float64", "Vec<Vec<T>>", "std::vector<vector>", "T[,]", "Array 嵌套", "[[T]]"], "数据"),
    ("OPTIONAL", "可选容器", ["Optional", "Optional", "undefined", "指针 nil", "Option", "std::optional", "T?", "T?", "T?"], "数据"),
    // ── 错误处理（17）
    ("EXCEPTION", "异常", ["try/except", "try/catch", "try/catch", "if err != nil", "Result/?", "try/catch", "try/catch", "try/catch", "do/catch"], "错误"),
    ("ERROR", "错误值", ["Exception", "Exception", "Error", "error", "Err(E)", "std::error_code", "Exception", "Error", "Error"], "错误"),
    ("NULL", "空值", ["None", "null", "null/undefined", "nil", "None", "std::nullopt", "null", "null", "nil"], "错误"),
    ("RESULT", "结果类型", ["（约定）", "（约定）", "（约定）", "(T, error)", "Result<T,E>", "std::expected", "Result<T>", "Result<T>", "Result"], "错误"),
    ("PANIC", "崩溃", ["raise SystemExit", "RuntimeException", "throw Error", "panic()", "panic!", "std::abort", "Environment.FailFast", "throw", "fatalError"], "错误"),
    ("ASSERT", "断言", ["assert", "assert", "console.assert", "if !cond {}", "assert!", "assert", "Debug.Assert", "assert", "precondition"], "错误"),
    ("TRY", "尝试块", ["try:", "try {", "try {", "defer+recover", "（无）", "try {", "try {", "try {", "do {"], "错误"),
    ("CATCH", "捕获块", ["except:", "catch {", "catch {", "recover()", "（无）", "catch {", "catch {", "catch {", "catch {"], "错误"),
    ("FINALLY", "收尾块", ["finally:", "finally {", "finally {", "defer", "Drop", "RAII", "finally {", "finally {", "defer {"], "错误"),
    ("THROW", "抛出", ["raise", "throw", "throw", "panic()", "panic!", "throw", "throw", "throw", "throw"], "错误"),
    ("DEFER", "延迟执行", ["contextlib", "try-finally", "try-finally", "defer", "Drop", "RAII 析构", "using", "use{}", "defer"], "错误"),
    ("GUARD", "卫语句", ["if not x: return", "if(!x) return", "if(!x) return", "if err != nil", "if x.is_none()", "if (!x)", "if (x==null)", "if(x==null)", "guard else"], "错误"),
    ("UNWRAP", "强制取值", ["list[0]", ".get()", "!", "*p", ".unwrap()", "*ptr", "Value", "!!", "!"], "错误"),
    ("PROPAGATE", "错误传播", ["raise", "throws", "throw", "return err", "?", "throw", "throw", "throw", "throws"], "错误"),
    ("RETRY", "重试", ["tenacity", "（自建）", "（自建）", "（自建）", "（自建）", "（自建）", "Polly.Retry", "（自建）", "（自建）"], "错误"),
    ("FALLBACK", "兜底值", ["except: 默认值", "catch 默认值", "catch 默认值", "if err != nil", "unwrap_or", "value_or", "?? 运算符", "?:", "??"], "错误"),
    ("CLEANUP", "资源清理", ["try/finally", "finally", "finally", "defer", "Drop", "析构", "using", "use", "defer"], "错误"),
    // ── 类型与面向对象（24）
    ("CLASS", "类", ["class", "class", "class", "struct", "struct+impl", "class", "class", "class", "class"], "类型"),
    ("STRUCT", "结构体", ["dataclass", "record", "对象字面量", "struct", "struct", "struct", "struct", "data class", "struct"], "类型"),
    ("INTERFACE", "接口", ["Protocol/ABC", "interface", "鸭子类型", "interface", "trait", "抽象类", "interface", "interface", "protocol"], "类型"),
    ("TRAIT", "特征", ["ABC", "interface", "mixin", "interface", "trait", "concept", "interface", "interface", "protocol"], "类型"),
    ("ENUM", "枚举", ["Enum", "enum", "对象常量", "const+iota", "enum", "enum class", "enum", "enum class", "enum"], "类型"),
    ("ABSTRACT", "抽象声明", ["ABC", "abstract class", "（无）", "interface", "trait", "纯虚函数", "abstract class", "abstract class", "protocol 默认"], "类型"),
    ("FINAL", "最终声明", ["（无）", "final", "const", "（无）", "（无）", "final", "sealed", "final", "final"], "类型"),
    ("STATIC", "静态成员", ["@staticmethod", "static", "static", "包级函数", "关联函数", "static", "static", "companion object", "static"], "类型"),
    ("CONST", "常量", ["大写常量", "static final", "const", "const", "const", "constexpr", "const", "const val", "let"], "类型"),
    ("VIRTUAL", "虚方法", ["普通方法", "非 final 方法", "原型方法", "方法集", "trait 方法", "virtual", "virtual", "open fun", "默认虚"], "类型"),
    ("OVERRIDE", "重写", ["def 同名", "@Override", "原型覆写", "接口实现", "impl", "override", "override", "override", "override"], "类型"),
    ("OVERLOAD", "重载", ["默认参数", "重载", "（无）", "（无）", "（无）", "重载", "重载", "重载", "重载"], "类型"),
    ("TEMPLATE", "模板", ["TypeVar", "泛型", "（无）", "（无）", "泛型", "template", "泛型", "泛型", "泛型"], "类型"),
    ("GENERIC", "泛型", ["TypeVar", "<T>", "<T>", "[T any]", "<T>", "<typename T>", "<T>", "<T>", "<T>"], "类型"),
    ("MIXIN", "混入", ["多继承", "接口默认方法", "原型混入", "嵌入 struct", "trait", "多继承", "接口默认实现", "接口委托", "协议扩展"], "类型"),
    ("EXTEND", "继承", ["class A(B)", "extends", "extends", "嵌入", "impl Trait for", "class A : public B", "class A : B", "class A : B", "class A: B"], "类型"),
    ("IMPL", "实现", ["def", "implements", "鸭子类型", "（隐式）", "impl", "实现", "实现", "实现", "extension"], "类型"),
    ("INHERIT", "继承链", ["MRO", "继承链", "原型链", "嵌入链", "trait 链", "继承链", "继承链", "继承链", "继承链"], "类型"),
    ("NAMESPACE", "命名空间", ["模块", "package", "module", "package", "mod", "namespace", "namespace", "package", "module"], "类型"),
    ("MODULE", "模块", ["module", "module-info", "ES module", "package", "mod", "头文件", "namespace", "package", "module"], "类型"),
    ("PACKAGE", "包", ["package", "package", "package.json", "go.mod", "crate", "CMake target", "csproj", "gradle 模块", "Package.swift"], "类型"),
    ("INSTANCE", "实例", ["对象", "new 对象", "new 对象", "&T{}", "实例", "对象", "new", "实例", "实例"], "类型"),
    ("CONSTRUCTOR", "构造器", ["__init__", "构造方法", "constructor", "NewX()", "new()", "构造函数", "构造函数", "init{}", "init"], "类型"),
    ("DESTRUCTOR", "析构器", ["__del__", "finalize", "（GC）", "SetFinalizer", "Drop", "~析构函数", "Dispose", "（GC）", "deinit"], "类型"),
    // ── 函数式（16）
    ("LAMBDA", "匿名函数", ["lambda", "->", "=>", "func(){}", "|x| {}", "[](){}", "x => x", "{}", "{ in }"], "函数"),
    ("CLOSURE", "闭包", ["嵌套函数", "lambda 捕获", "闭包", "func 字面量", "闭包", "lambda 捕获", "lambda", "lambda", "闭包"], "函数"),
    ("TRANSFORM", "映射变换", ["map()", "Stream.map", "Array.map", "for 循环", "iter().map", "std::transform", "Select", "map{}", "map{}"], "函数"),
    ("FILTER", "过滤", ["filter()", "Stream.filter", "Array.filter", "for+if", "iter().filter", "std::copy_if", "Where", "filter{}", "filter{}"], "函数"),
    ("REDUCE", "归约", ["functools.reduce", "Stream.reduce", "Array.reduce", "for 累加", "iter().fold", "std::accumulate", "Aggregate", "fold{}", "reduce{}"], "函数"),
    ("FOLD", "折叠", ["reduce", "reduce", "reduceRight", "（自定义）", "fold", "std::accumulate", "Aggregate", "fold", "reduce"], "函数"),
    ("CURRY", "柯里化", ["functools.partial", "（无内置）", "bind", "闭包", "闭包", "std::bind", "闭包", "partial 高阶函数", "闭包"], "函数"),
    ("PARTIAL", "偏应用", ["functools.partial", "（无内置）", "bind", "闭包", "闭包", "std::bind_front", "（无内置）", "（无内置）", "闭包"], "函数"),
    ("COMPOSE", "函数组合", ["reduce 组合", "andThen", "链式调用", "嵌套调用", "闭包组合", "std::function", "委托", "andThen", "闭包"], "函数"),
    ("PIPE", "管道", ["toolz.pipe", "Stream 链", "|> 提案", "（无）", "|> 提案", "views 管道", "LINQ 链", "let 链", "链式调用"], "函数"),
    ("GENERATOR", "生成器", ["yield", "（无内置）", "function*", "（无）", "迭代器", "co_yield", "yield return", "yield", "AsyncStream"], "函数"),
    ("YIELD", "产出", ["yield", "（无内置）", "yield", "（无）", "（无）", "co_yield", "yield return", "yield", "yield"], "函数"),
    ("ITERATOR", "迭代器", ["iter()", "Iterator", "Symbol.iterator", "range", "Iterator", "iterator", "IEnumerator", "Iterator", "IteratorProtocol"], "函数"),
    ("MONAD", "单子链", ["（约定）", "Optional 链", "Promise 链", "（约定）", "Option/Result 链", "std::optional 链", "（约定）", "（约定）", "Optional 链"], "函数"),
    ("IMMUTABLE", "不可变", ["tuple/frozenset", "final 不可变类", "Object.freeze", "（约定）", "默认不可变", "const 成员", "record", "val", "let"], "函数"),
    ("PURE", "纯函数", ["无副作用函数", "无副作用方法", "纯函数", "纯函数", "无副作用 fn", "constexpr", "静态纯方法", "纯函数", "纯函数"], "函数"),
    // ── 内存与所有权（14）
    ("POINTER", "指针", ["（无）", "引用", "（无）", "*T", "&T", "T*", "指针", "（无）", "UnsafePointer"], "内存"),
    ("REFERENCE", "引用", ["别名", "引用", "对象引用", "&T", "&", "T&", "ref", "引用", "引用"], "内存"),
    ("ALLOC", "分配", ["对象构造", "new", "new", "new()", "Box::new", "new", "new", "构造", "init"], "内存"),
    ("FREE", "释放", ["（GC）", "（GC）", "（GC）", "（GC）", "drop", "delete", "（GC）", "（GC）", "deinit"], "内存"),
    ("GC", "垃圾回收", ["引用计数+GC", "GC", "GC", "GC", "（无GC）", "（无GC）", "GC", "GC", "ARC"], "内存"),
    ("REFCOUNT", "引用计数", ["sys.getrefcount", "（无）", "（无）", "（无）", "Rc", "shared_ptr", "（无）", "（无）", "ARC"], "内存"),
    ("ARC", "原子引用计数", ["（无）", "（无）", "（无）", "（无）", "Arc", "原子 shared_ptr", "（无）", "（无）", "ARC"], "内存"),
    ("BOX", "堆装箱", ["（自动）", "Integer 装箱", "（自动）", "interface{}", "Box", "unique_ptr", "object", "Any", "AnyObject"], "内存"),
    ("MOVE", "移动语义", ["（无）", "（无）", "（无）", "（无）", "move", "std::move", "（无）", "（无）", "（无）"], "内存"),
    ("BORROW", "借用", ["（无）", "（无）", "（无）", "（无）", "&/&mut", "const&", "ref/in/out", "（无）", "（无）"], "内存"),
    ("LIFETIME", "生命周期", ["作用域", "作用域", "作用域", "作用域", "'a", "RAII 作用域", "using 作用域", "作用域", "ARC 作用域"], "内存"),
    ("WEAK", "弱引用", ["weakref", "WeakReference", "WeakRef", "（无）", "Weak", "weak_ptr", "WeakReference", "WeakReference", "weak"], "内存"),
    ("POOL", "对象池", ["（自建）", "（自建）", "（自建）", "sync.Pool", "（自建）", "（自建）", "ArrayPool", "（自建）", "（自建）"], "内存"),
    ("STACKALLOC", "栈分配", ["局部变量", "栈变量", "栈变量", "栈变量", "栈变量", "std::array", "栈变量", "局部变量", "局部变量"], "内存"),
    // ── IO 与文件（18）
    ("FILE", "文件", ["open()", "File", "File", "os.Open", "File", "std::fstream", "FileStream", "File", "FileHandle"], "IO"),
    ("OPEN", "打开", ["open()", "new FileInputStream", "fs.openSync", "os.Open", "File::open", "std::ifstream", "File.Open", "File()", "FileHandle(forReadingAtPath:)"], "IO"),
    ("CLOSE", "关闭", ["close()", "close()", "close()", "defer f.Close()", "Drop", "close()", "using", "use{}", "defer"], "IO"),
    ("READ", "读取", ["read()", "read()", "fs.readFileSync", "io.ReadAll", "read_to_string", "read", "ReadAllText", "readText", "Data(contentsOf:)"], "IO"),
    ("WRITE", "写入", ["write()", "write()", "fs.writeFileSync", "os.WriteFile", "write_all", "write", "WriteAllText", "writeText", "write(to:)"], "IO"),
    ("PATH", "路径", ["pathlib.Path", "Paths.get", "path", "path/filepath", "PathBuf", "std::filesystem::path", "Path", "File", "URL"], "IO"),
    ("STREAM", "流", ["io.IOBase", "Stream", "ReadableStream", "io.Reader", "Read", "std::istream", "Stream", "Flow", "AsyncSequence"], "IO"),
    ("FLUSH", "刷新", ["flush()", "flush()", "（自动）", "Flush()", "flush()", "flush", "Flush", "flush()", "（自动）"], "IO"),
    ("SEEK", "定位", ["seek()", "seek()", "（无）", "Seek", "seek", "seekg", "Seek", "seek()", "（无）"], "IO"),
    ("MMAP", "内存映射", ["mmap", "MappedByteBuffer", "（无）", "syscall.Mmap", "memmap2", "mmap", "MemoryMappedFile", "（无）", "mmap"], "IO"),
    ("STDOUT", "标准输出", ["sys.stdout", "System.out", "console.log", "os.Stdout", "stdout()", "std::cout", "Console.WriteLine", "println", "print"], "IO"),
    ("STDIN", "标准输入", ["sys.stdin", "System.in", "process.stdin", "os.Stdin", "stdin()", "std::cin", "Console.ReadLine", "readLine", "readLine()"], "IO"),
    ("SOCKET", "套接字", ["socket", "Socket", "WebSocket", "net.Conn", "TcpStream", "socket", "Socket", "Socket", "NWConnection"], "IO"),
    ("HTTP", "HTTP 请求", ["requests", "HttpClient", "fetch", "net/http", "reqwest", "libcurl", "HttpClient", "HttpURLConnection", "URLSession"], "IO"),
    ("SERVER", "服务端", ["http.server", "HttpServer", "http.createServer", "ListenAndServe", "axum", "（框架）", "Kestrel", "Ktor", "Vapor"], "IO"),
    ("CLIENT", "客户端", ["http.client", "HttpClient", "fetch", "http.Client", "reqwest::Client", "（库）", "HttpClient", "OkHttp", "URLSession"], "IO"),
    ("REQUEST", "请求", ["Request", "HttpRequest", "Request", "http.Request", "Request", "（结构）", "HttpRequestMessage", "Request", "URLRequest"], "IO"),
    ("RESPONSE", "响应", ["Response", "HttpResponse", "Response", "http.Response", "Response", "（结构）", "HttpResponseMessage", "Response", "URLResponse"], "IO"),
    // ── 集合操作（20）
    ("PUSH", "追加", ["append", "add", "push", "append", "push", "push_back", "Add", "add", "append"], "集合"),
    ("POP", "弹出", ["pop", "remove", "pop", "切片截断", "pop", "pop_back", "Dequeue", "removeLast", "removeLast"], "集合"),
    ("INSERT", "插入", ["insert", "insert", "splice", "append", "insert", "insert", "Insert", "add", "insert"], "集合"),
    ("REMOVE", "删除", ["remove", "remove", "splice", "delete(m,k)", "remove", "erase", "Remove", "remove", "remove"], "集合"),
    ("FIND", "查找位置", ["index()", "indexOf", "indexOf", "for+if", "position", "find", "IndexOf", "indexOf", "firstIndex"], "集合"),
    ("CONTAINS", "包含判断", ["in", "contains", "includes", "for+if", "contains", "count", "Contains", "contains", "contains"], "集合"),
    ("SORT", "排序", ["sorted", "Collections.sort", "Array.sort", "sort.Slice", "sort()", "std::sort", "OrderBy", "sortedBy", "sorted()"], "集合"),
    ("REVERSE", "反转", ["reverse", "Collections.reverse", "reverse", "for 交换", "reverse", "std::reverse", "Reverse", "reversed", "reversed()"], "集合"),
    ("MERGE", "合并", ["update", "putAll", "Object.assign", "append", "extend", "merge", "Concat", "plus", "merge"], "集合"),
    ("SPLIT", "切分", ["split", "split", "split", "strings.Split", "split", "views::split", "Split", "split", "split"], "集合"),
    ("JOINSTR", "连接", ["join", "Collectors.joining", "join", "strings.Join", "join", "for+append", "Join", "joinToString", "joined"], "集合"),
    ("CONCAT", "拼接", ["+", "+", "concat", "append", "+", "+", "+", "plus", "+"], "集合"),
    ("DEDUP", "去重", ["set()", "distinct", "Set", "map 去重", "dedup", "unique", "Distinct", "distinct", "uniqued"], "集合"),
    ("COUNT", "计数", ["len", "size", "length", "len", "len", "size", "Count", "size", "count"], "集合"),
    ("INDEX", "索引访问", ["a[i]", "get(i)", "a[i]", "a[i]", "a[i]", "a[i]", "a[i]", "a[i]", "a[i]"], "集合"),
    ("FLATTEN", "展平", ["itertools.chain", "flatMap", "flat", "for 展开", "flatten", "for 展开", "SelectMany", "flatten", "flatMap"], "集合"),
    ("GROUP", "分组", ["itertools.groupby", "groupingBy", "reduce 分组", "for+map", "chunk_by", "for 分组", "GroupBy", "groupBy", "Dictionary(grouping:)"], "集合"),
    ("ZIP", "拉链配对", ["zip", "（无内置）", "（无）", "（无）", "zip", "（无）", "Zip", "zip", "zip"], "集合"),
    ("SLICE", "切片", ["a[i:j]", "subList", "slice", "a[i:j]", "&a[i..j]", "substr", "Substring", "subList", "a[i..<j]"], "集合"),
    ("CAPACITY", "容量", ["（自动）", "capacity", "length", "cap()", "capacity", "capacity", "Capacity", "capacity", "capacity"], "集合"),
    // ── 元编程与反射（12）
    ("REFLECTION", "反射", ["getattr/inspect", "反射 API", "Reflect", "reflect", "（无）", "（无 RTTI）", "Reflection", "反射", "Mirror"], "元编程"),
    ("ANNOTATION", "注解", ["装饰器", "@Annotation", "装饰器提案", "结构标签", "属性宏", "[[attribute]]", "Attribute", "@Annotation", "@attribute"], "元编程"),
    ("DECORATOR", "装饰器", ["@decorator", "注解+代理", "装饰器函数", "中间件", "属性宏", "（无）", "Attribute", "注解类", "属性包装"], "元编程"),
    ("MACRO", "宏", ["（无）", "（无）", "（无）", "go:generate", "macro_rules!", "#define", "#define", "（无）", "（无）"], "元编程"),
    ("CODEGEN", "代码生成", ["exec/AST", "注解处理器", "模板字符串", "go generate", "proc-macro", "模板", "Source Generator", "KSP", "Sourcery"], "元编程"),
    ("DYNAMIC", "动态类型", ["动态类型", "（无）", "typeof", "interface{}", "dyn Trait", "std::any", "dynamic", "Any", "Any"], "元编程"),
    ("TYPEOF", "取类型", ["type()", "getClass()", "typeof", "reflect.TypeOf", "type_name", "typeid", "GetType()", "::class", "type(of:)"], "元编程"),
    ("CAST", "类型转换", ["int(x)", "(T) x", "Number(x)", "T(x)", "as", "static_cast", "(T)x", "as", "as!"], "元编程"),
    ("ISCHECK", "类型判断", ["isinstance", "instanceof", "instanceof", "type switch", "matches!", "dynamic_cast", "is", "is", "is"], "元编程"),
    ("TEMPLATE_STR", "模板字符串", ["f\"{x}\"", "String.format", "`${x}`", "fmt.Sprintf", "format!", "std::format", "$\"{x}\"", "\"$x\"", "\\(x)"], "元编程"),
    ("INTERP", "插值", ["f-string", "%s 格式", "模板字面量", "fmt", "format!", "std::format", "插值字符串", "字符串模板", "字符串插值"], "元编程"),
    ("SERIALIZE", "序列化", ["pickle/json", "Serializable", "JSON.stringify", "encoding/json", "serde", "（库）", "JsonSerializer", "kotlinx.serialization", "Codable"], "元编程"),
    // ── 数值与时间（16）
    ("INT", "整数", ["int", "int", "number", "int", "i32/i64", "int", "int", "Int", "Int"], "数值"),
    ("FLOAT", "浮点", ["float", "double", "number", "float64", "f64", "double", "double", "Double", "Double"], "数值"),
    ("BOOL", "布尔", ["bool", "boolean", "boolean", "bool", "bool", "bool", "bool", "Boolean", "Bool"], "数值"),
    ("CHAR", "字符", ["str 单字符", "char", "string 单元", "rune", "char", "char", "char", "Char", "Character"], "数值"),
    ("BYTE", "字节", ["bytes 单元", "byte", "number", "byte", "u8", "uint8_t", "byte", "Byte", "UInt8"], "数值"),
    ("DATE", "日期", ["datetime.date", "LocalDate", "Date", "time.Time", "NaiveDate", "std::chrono", "DateOnly", "LocalDate", "Date"], "数值"),
    ("TIME", "时间", ["datetime.time", "LocalTime", "Date", "time.Time", "NaiveTime", "time_point", "TimeOnly", "LocalTime", "DateComponents"], "数值"),
    ("DURATION", "时长", ["timedelta", "Duration", "毫秒数", "time.Duration", "Duration", "std::chrono::duration", "TimeSpan", "Duration", "TimeInterval"], "数值"),
    ("TIMESTAMP", "时间戳", ["time.time()", "Instant", "Date.now()", "time.Now().Unix()", "SystemTime", "system_clock", "DateTimeOffset", "Instant", "Date()"], "数值"),
    ("RANDOM", "随机数", ["random", "Random", "Math.random", "math/rand", "rand", "std::rand", "Random", "Random", "random()"], "数值"),
    ("MATHFN", "数学函数", ["math", "Math", "Math", "math", "f64 方法", "cmath", "Math", "kotlin.math", "Foundation"], "数值"),
    ("ROUND", "取整", ["round", "Math.round", "Math.round", "math.Round", "round", "std::round", "Math.Round", "roundToInt", "rounded"], "数值"),
    ("MIN", "最小值", ["min", "Math.min", "Math.min", "for 比较", "min", "std::min", "Math.Min", "minOf", "min"], "数值"),
    ("MAX", "最大值", ["max", "Math.max", "Math.max", "for 比较", "max", "std::max", "Math.Max", "maxOf", "max"], "数值"),
    ("ABS", "绝对值", ["abs", "Math.abs", "Math.abs", "math.Abs", "abs", "std::abs", "Math.Abs", "abs", "abs"], "数值"),
    ("SUM", "求和", ["sum", "Stream.sum", "reduce", "for 累加", "sum", "std::accumulate", "Sum", "sum", "reduce"], "数值"),
    // ── 语法糖（12）
    ("SPREAD", "展开", ["*args", "varargs", "...", "...", "（无）", "参数包", "params", "vararg", "可变参数"], "语法"),
    ("REST_PARAM", "剩余参数", ["**kwargs", "varargs", "...rest", "...", "（无）", "（无）", "params", "vararg", "可变参数"], "语法"),
    ("DESTRUCTURE", "解构", ["a, b = t", "record 解构", "const {a,b}", "a, b := f()", "let (a,b)", "结构化绑定", "(a, b) = t", "解构声明", "let (a, b)"], "语法"),
    ("OPTIONAL_CHAIN", "可选链", ["getattr 链", "Optional 链", "a?.b", "if 判断", "a?.b?", "if 判断", "a?.b", "a?.b", "a?.b"], "语法"),
    ("NULL_COALESCE", "空值合并", ["a or b", "requireNonNullElse", "a ?? b", "if 判断", "unwrap_or", "value_or", "a ?? b", "a ?: b", "a ?? b"], "语法"),
    ("TERNARY", "三元表达式", ["a if c else b", "c ? a : b", "c ? a : b", "if 语句", "if c {a} else {b}", "c ? a : b", "c ? a : b", "if (c) a else b", "c ? a : b"], "语法"),
    ("SWITCH", "分支选择", ["match", "switch", "switch", "switch", "match", "switch", "switch", "when", "switch"], "语法"),
    ("PATTERN_MATCH", "模式匹配", ["match", "switch 模式", "（无）", "type switch", "match", "（无）", "switch 模式", "when", "switch case"], "语法"),
    ("COMPREHENSION", "推导式", ["[x for x in y]", "Stream 收集", "map/filter 链", "for 循环", "迭代器链", "for 循环", "LINQ", "map/filter 链", "map/filter 链"], "语法"),
    ("WITHBLOCK", "上下文管理", ["with open()", "try-with-resources", "（无）", "defer", "Drop", "RAII", "using", "use{}", "defer"], "语法"),
    ("CHAIN", "链式调用", ["方法链", "Builder 链", "Promise 链", "链式调用", "迭代器链", "流式", "LINQ 链", "作用域函数", "链式调用"], "语法"),
    ("PIPE_OP", "管道运算符", ["（无）", "（无）", "|> 提案", "（无）", "（无）", "ranges", "（无）", "（无）", "（无）"], "语法"),
    // ── 网络与协议（14）
    ("TCP", "面向连接传输", ["socket(SOCK_STREAM)", "SocketChannel", "net.Socket", "net.Dial(\"tcp\")", "TcpStream", "socket(AF_INET, SOCK_STREAM)", "TcpClient", "Socket", "NWConnection(.tcp)"], "网络"),
    ("UDP", "无连接传输", ["socket(SOCK_DGRAM)", "DatagramChannel", "dgram", "net.Dial(\"udp\")", "UdpSocket", "socket(AF_INET, SOCK_DGRAM)", "UdpClient", "DatagramSocket", "NWConnection(.udp)"], "网络"),
    ("WEBSOCKET", "全双工长连接", ["websockets", "WebSocket", "WebSocket", "gorilla/websocket", "tokio-tungstenite", "（库）", "ClientWebSocket", "okhttp-ws", "URLSessionWebSocketTask"], "网络"),
    ("GRPC", "远程过程调用", ["grpcio", "gRPC-Java", "grpc-js", "google.golang.org/grpc", "tonic", "grpc++", "Grpc.Net", "grpc-kotlin", "grpc-swift"], "网络"),
    ("REST", "表述性状态", ["requests+JSON", "JAX-RS", "fetch+JSON", "net/http+JSON", "axum+JSON", "（库）", "ASP.NET Web API", "Ktor", "URLSession+JSON"], "网络"),
    ("GRAPHQL", "图查询", ["graphene", "graphql-java", "graphql-js", "gqlgen", "async-graphql", "（库）", "HotChocolate", "graphql-kotlin", "Apollo"], "网络"),
    ("URL", "统一资源定位", ["urllib.parse", "URI", "URL", "net/url", "url::Url", "（库）", "Uri", "java.net.URI", "URL"], "网络"),
    ("COOKIE", "会话票据", ["http.cookies", "CookieManager", "document.cookie", "http.Cookie", "cookie crate", "（库）", "CookieContainer", "OkHttp CookieJar", "HTTPCookie"], "网络"),
    ("SESSION", "会话", ["session 对象", "HttpSession", "express-session", "gorilla/sessions", "tower-sessions", "（库）", "ISession", "HttpSession", "URLSession"], "网络"),
    ("TOKEN", "令牌", ["secrets.token_hex", "UUID/Token", "crypto.randomUUID", "uuid.New", "uuid::Uuid", "（库）", "Guid", "UUID", "UUID"], "网络"),
    ("JWT", "签名令牌", ["PyJWT", "jjwt", "jsonwebtoken", "golang-jwt", "jsonwebtoken", "jwt-cpp", "IdentityModel", "java-jwt", "JWTKit"], "网络"),
    ("OAUTH", "授权", ["oauthlib", "Spring Security OAuth", "passport", "golang.org/x/oauth2", "oauth2 crate", "（库）", "IdentityModel", "AppAuth", "AuthenticationServices"], "网络"),
    ("CACHE", "缓存", ["functools.lru_cache", "Caffeine", "Map 缓存", "sync.Map 缓存", "lru crate", "unordered_map 缓存", "MemoryCache", "LruCache", "NSCache"], "网络"),
    ("TLS", "加密传输", ["ssl", "SSLSocket", "tls", "crypto/tls", "rustls", "OpenSSL", "SslStream", "SSLSocket", "NWProtocolTLS"], "网络"),
    // ── 系统与进程（14）
    ("PROCESS", "进程", ["subprocess", "ProcessBuilder", "child_process", "os/exec", "std::process::Command", "std::system", "Process", "ProcessBuilder", "Process"], "系统"),
    ("FORK", "分叉", ["os.fork", "（无）", "（无）", "os.StartProcess", "（无）", "fork()", "（无）", "（无）", "（无）"], "系统"),
    ("EXEC", "执行外部程序", ["subprocess.run", "Runtime.exec", "execSync", "exec.Command", "Command::new", "execvp", "Process.Start", "Runtime.exec", "Process.run"], "系统"),
    ("SIGNAL", "信号", ["signal", "Signal", "process.on(SIGINT)", "os/signal", "signal-hook", "signal()", "（无）", "（无）", "DispatchSourceSignal"], "系统"),
    ("ENV", "环境变量", ["os.environ", "System.getenv", "process.env", "os.Getenv", "std::env::var", "getenv", "Environment", "System.getenv", "ProcessInfo.environment"], "系统"),
    ("CWD", "工作目录", ["os.getcwd", "user.dir", "process.cwd", "os.Getwd", "current_dir", "std::filesystem", "GetCurrentDirectory", "user.dir", "currentDirectoryPath"], "系统"),
    ("SYSCALL", "系统调用", ["ctypes", "JNI/FFM", "（无）", "syscall", "libc crate", "syscall()", "P/Invoke", "JNI", "Darwin.syscall"], "系统"),
    ("DAEMON", "后台常驻", ["守护进程", "（无）", "（无）", "goroutine 常驻", "（无）", "daemon()", "Windows Service", "Service", "launchd"], "系统"),
    ("SERVICE", "后台服务", ["服务类", "Service", "（无）", "service 包", "（无）", "（无）", "BackgroundService", "Service", "（无）"], "系统"),
    ("CRON", "定时任务", ["schedule 库", "ScheduledExecutor", "node-cron", "time.Ticker", "tokio::time::interval", "（无）", "Timer", "Timer", "Timer"], "系统"),
    ("PROC_PIPE", "进程管道", ["subprocess.PIPE", "PipedInputStream", "stream.pipe", "io.Pipe", "Stdio::piped", "pipe()", "AnonymousPipe", "PipedReader", "Pipe"], "系统"),    ("REDIRECT", "重定向", ["stdout 参数", "ProcessBuilder.redirect", "stdio 配置", "cmd.Stdout", "Stdio::piped", "dup2", "RedirectStandardOutput", "ProcessBuilder", "Pipe 配置"], "系统"),
    ("EXIT", "退出码", ["sys.exit", "System.exit", "process.exit", "os.Exit", "process::exit", "std::exit", "Environment.Exit", "exitProcess", "exit()"], "系统"),
    ("PACKAGE_MGR", "包管理器", ["pip", "Maven", "npm", "go mod", "cargo", "vcpkg", "NuGet", "Gradle", "SPM"], "系统"),
    // ── 测试（8）
    ("TEST", "单元测试", ["unittest", "JUnit", "Jest", "testing", "#[test]", "gtest", "xUnit", "kotlin.test", "XCTest"], "测试"),
    ("MOCK", "模拟对象", ["unittest.mock", "Mockito", "jest.mock", "gomock", "mockall", "gmock", "Moq", "MockK", "协议替身"], "测试"),
    ("STUB", "桩对象", ["MagicMock", "stub", "stub", "stub", "stub", "Stub", "Stub", "stub", "Stub"], "测试"),
    ("FIXTURE", "测试夹具", ["pytest.fixture", "@BeforeEach", "beforeEach", "TestMain", "setup 函数", "SetUp", "[SetUp]", "@BeforeTest", "setUp()"], "测试"),
    ("ASSERT_EQ", "相等断言", ["assertEqual", "assertEquals", "expect().toBe", "t.Fatal", "assert_eq!", "EXPECT_EQ", "Assert.Equal", "assertEquals", "XCTAssertEqual"], "测试"),
    ("BENCHMARK", "基准测试", ["timeit", "JMH", "benchmark.js", "testing.B", "criterion", "google/benchmark", "BenchmarkDotNet", "JMH", "measure"], "测试"),
    ("COVERAGE", "覆盖率", ["coverage.py", "JaCoCo", "nyc", "go test -cover", "tarpaulin", "gcov", "Coverlet", "JaCoCo", "xccov"], "测试"),
    ("TDD", "测试驱动", ["unittest 先行", "JUnit 先行", "Jest 先行", "testing 先行", "#[test] 先行", "gtest 先行", "xUnit 先行", "kotlin.test 先行", "XCTest 先行"], "测试"),
    // ── 设计模式（10）
    ("SINGLETON", "单例", ["模块级实例", "enum 单例", "模块导出", "sync.Once", "OnceLock", "函数局部静态", "静态只读字段", "object 声明", "static let"], "模式"),
    ("FACTORY", "工厂", ["工厂函数", "工厂方法", "工厂函数", "NewX 构造", "关联函数", "工厂函数", "工厂方法", "伴生工厂", "init 工厂"], "模式"),
    ("OBSERVER", "观察者", ["回调列表", "Listener", "EventEmitter", "channel 广播", "观察者 trait", "std::function 列表", "event", "Flow", "NotificationCenter"], "模式"),
    ("VISITOR", "访问者", ["singledispatch", "访问者模式", "（无）", "接口实现", "enum match", "std::visit", "访问者模式", "sealed when", "协议实现"], "模式"),
    ("BUILDER", "建造者", ["关键字参数", "Builder 类", "链式对象", "Option 模式", "Builder 结构体", "Builder 类", "Builder 类", "apply{}", "链式 init"], "模式"),
    ("STRATEGY", "策略", ["函数参数", "策略接口", "函数参数", "接口实现", "trait 对象", "std::function", "策略接口", "函数类型", "闭包"], "模式"),
    ("ADAPTER", "适配器", ["包装类", "适配器类", "包装函数", "类型别名", "newtype", "适配器类", "适配器类", "扩展函数", "协议扩展"], "模式"),
    ("PROXY", "代理", ["__getattr__", "动态代理", "Proxy", "中间件", "Deref", "代理类", "DispatchProxy", "by 委托", "NSProxy"], "模式"),
    ("MEDIATOR", "中介者", ["事件总线", "事件总线", "EventEmitter", "channel", "mpsc", "（无）", "MediatR", "SharedFlow", "通知中心"], "模式"),
    ("DI", "依赖注入", ["参数注入", "@Inject", "constructor 注入", "构造函数注入", "参数注入", "构造函数注入", "DI 容器", "Koin", "初始化注入"], "模式"),
    // ── 日志与编解码（15）
    ("LOG", "日志", ["logging", "java.util.logging", "console.log", "log", "log crate", "std::clog", "ILogger", "Log", "os_log"], "工具"),
    ("DEBUG_OUT", "调试输出", ["print", "System.out.println", "console.debug", "fmt.Println", "dbg!", "std::cerr", "Debug.WriteLine", "println", "print"], "工具"),
    ("TRACE", "调用追踪", ["traceback", "StackTrace", "console.trace", "runtime.Stack", "backtrace", "backtrace()", "StackTrace", "stackTrace", "callStackSymbols"], "工具"),
    ("PRINT", "打印", ["print", "System.out.print", "console.log", "fmt.Print", "print!", "std::cout", "Console.Write", "print", "print"], "工具"),
    ("FORMAT", "格式化", ["str.format", "String.format", "模板字面量", "fmt.Sprintf", "format!", "std::format", "string.Format", "String.format", "String(format:)"], "工具"),
    ("PARSE", "解析", ["json.loads", "Integer.parseInt", "JSON.parse", "strconv.Atoi", "parse()", "std::stoi", "Parse", "toInt()", "Int()"], "工具"),
    ("SER_JSON", "JSON 编码", ["json.dumps", "writeValueAsString", "JSON.stringify", "json.Marshal", "serde_json::to_string", "nlohmann::json", "JsonSerializer", "encodeToString", "JSONEncoder"], "工具"),
    ("DESER_JSON", "JSON 解码", ["json.loads", "readValue", "JSON.parse", "json.Unmarshal", "serde_json::from_str", "json::parse", "Deserialize", "decodeFromString", "JSONDecoder"], "工具"),
    ("ENCODE", "编码", ["bytes.encode", "getBytes", "TextEncoder", "[]byte(s)", "as_bytes()", "（无）", "GetBytes", "toByteArray", "data(using:)"], "工具"),
    ("DECODE", "解码", ["bytes.decode", "new String", "TextDecoder", "string(b)", "from_utf8", "（无）", "GetString", "toString(Charsets.UTF_8)", "String(data:encoding:)"], "工具"),
    ("HASH", "哈希", ["hashlib", "MessageDigest", "createHash", "crypto/sha256", "sha2", "std::hash", "SHA256", "MessageDigest", "CryptoKit"], "工具"),
    ("ENCRYPT", "加密", ["cryptography", "Cipher", "crypto", "crypto/aes", "aes-gcm", "OpenSSL", "Aes", "javax.crypto", "CryptoKit"], "工具"),
    ("COMPRESS", "压缩", ["zlib/gzip", "Deflater", "zlib", "compress/gzip", "flate2", "zlib", "GZipStream", "Deflater", "Compression"], "工具"),
    ("BASE64", "Base64 转换", ["base64", "Base64", "btoa/Buffer", "encoding/base64", "base64 crate", "（库）", "ToBase64String", "Base64", "base64EncodedString"], "工具"),
    ("REGEX", "正则表达式", ["re", "Pattern", "RegExp", "regexp", "regex", "std::regex", "Regex", "Regex", "NSRegularExpression"], "工具"),
];
// ── 语义推理层 ────────────────────────────────────────────────

/// 按语义 ID 取条目。
pub fn concept(id: &str) -> Option<&'static Concept> {
    DICT.iter().find(|c| c.0 == id)
}

/// 取某概念在某语言下的等价写法。
pub fn form(id: &str, lang: &str) -> Option<&'static str> {
    let i = LANGS.iter().position(|l| *l == lang)?;
    concept(id).map(|c| c.2[i])
}

/// 词典条目总数（#516）。
pub fn concept_count() -> usize {
    DICT.len()
}

/// 映射条数 = 概念数 × 语言数。
pub fn mapping_count() -> usize {
    DICT.len() * LANGS.len()
}

/// 全词表检索：语义 ID / 中文概念 / 各语言写法 / 类别。
pub fn surface_match(id: &str, needle: &str) -> bool {
    let n = needle.trim().to_lowercase();
    if n.is_empty() {
        return false;
    }
    match concept(id) {
        Some(c) => {
            c.0.to_lowercase().contains(&n)
                || c.1.contains(needle.trim())
                || c.3.contains(needle.trim())
                || c.2.iter().any(|f| f.to_lowercase().contains(&n))
        }
        None => false,
    }
}

/// #516 语义词典查询结果（语义气泡所需字段）。
#[derive(Debug, Clone)]
pub struct SemanticInfo {
    pub id: &'static str,
    pub zh: &'static str,
    pub category: &'static str,
    pub forms: [&'static str; 9],
    pub metaphor: &'static str,
}

/// #516 语义词典查询。
pub fn info(id: &str) -> Option<SemanticInfo> {
    concept(id).map(|c| SemanticInfo {
        id: c.0,
        zh: c.1,
        category: c.3,
        forms: c.2,
        metaphor: metaphor(id),
    })
}

/// 上下文类型（#517 消歧输入：AST 节点类型 + 调用方式）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ctx {
    /// 类型/变量声明位 → 数据结构语义。
    TypeDecl,
    /// 调用/管道位 → 行为语义。
    CallSite,
    /// 未定。
    Unknown,
}

/// #517 上下文消歧：同一词不同上下文显示不同含义（map = 数据结构 vs 变换）。
pub fn disambiguate(word: &str, ctx: Ctx) -> Option<&'static str> {
    let w = word.trim().to_lowercase();
    if w == "map" {
        return Some(match ctx {
            Ctx::TypeDecl => "MAP",
            Ctx::CallSite => "TRANSFORM",
            Ctx::Unknown => "MAP",
        });
    }
    if w == "lock" || w == "mutex" {
        return Some("LOCK");
    }
    if w == "list" || w == "vec" || w == "array" {
        return Some("LIST");
    }
    if w == "function" || w == "func" || w == "fn" {
        return Some("LAMBDA");
    }
    DICT
        .iter()
        .find(|c| {
            c.0.eq_ignore_ascii_case(&w)
                || c.2.iter().any(|f| f.to_lowercase() == w)
                || c.1 == word.trim()
        })
        .map(|c| c.0)
}

/// #518 语义距离：类型 + 行为 + 上下文加权（越小越近）。
pub fn semantic_distance(a: &str, b: &str) -> f64 {
    let (Some(x), Some(y)) = (concept(a), concept(b)) else {
        return f64::MAX;
    };
    let mut d = 0.0f64;
    // 类型权重 0.5：类别相同则近。
    if x.3 != y.3 {
        d += 0.5;
    }
    // 行为权重 0.3：各语言写法是否有交集。
    let share_form = x
        .2
        .iter()
        .any(|f| y.2.iter().any(|g| f.to_lowercase() == g.to_lowercase()));
    if !share_form {
        d += 0.3;
    }
    // 上下文权重 0.2：ID 公共前缀占比。
    let common = x
        .0
        .chars()
        .zip(y.0.chars())
        .take_while(|(p, q)| p == q)
        .count() as f64;
    let maxlen = x.0.len().max(y.0.len()) as f64;
    d += 0.2 * (1.0 - common / maxlen);
    d
}

/// #518 搜索时显示最接近的概念。
pub fn nearest(query: &str) -> Option<&'static str> {
    let q = query.trim();
    if q.is_empty() {
        return None;
    }
    let mut best: Option<(&'static str, f64)> = None;
    for c in DICT {
        // 查询词若非词典 ID，则以单位基准分参与比较，避免 f64::MAX 抹平差异。
        let mut score = if concept(q).is_some() { semantic_distance(c.0, q) } else { 1.0 };
        if c.0.eq_ignore_ascii_case(q) {
            score -= 2.0;
        } else if c.2.iter().any(|f| f.eq_ignore_ascii_case(q)) {
            score -= 1.0;
        } else if c.2.iter().any(|f| f.to_lowercase().contains(&q.to_lowercase())) {
            score -= 0.6;
        } else if c.1 == q {
            score -= 1.5;
        } else {
            score += 0.4;
        }
        if best.map_or(true, |(_, bd)| score < bd) {
            best = Some((c.0, score));
        }
    }
    best.map(|(id, _)| id)
}

/// #519 跨语言等价：语义 ID 匹配 → 目标语言最佳等价写法。
pub fn equivalent(id: &str, lang: &str) -> Option<&'static str> {
    form(id, lang)
}

/// #520 语义搜索：概念级搜索（搜「异步」能匹配 async / goroutine / Promise）。
pub fn search(query: &str) -> Vec<&'static str> {
    let q = query.trim();
    if q.is_empty() {
        return Vec::new();
    }
    DICT
        .iter()
        .filter(|c| c.1.contains(q) || surface_match(c.0, q))
        .map(|c| c.0)
        .collect()
}

/// #521 语义气泡（悬停 1s）：语义 ID + 定义 + 跨语言映射 + 比喻。
pub fn bubble(id: &str, lang: &str) -> Option<String> {
    let i = concept(id)?;
    let f = form(id, lang).unwrap_or("");
    Some(format!(
        "{} · {}：{} = {}；比喻：{}",
        i.0,
        i.1,
        lang,
        f,
        metaphor(id)
    ))
}

/// #522 语义对比：并排两概念在各语言写法 + 差异说明。
pub fn compare(a: &str, b: &str, langs: &[&str]) -> Option<String> {
    let ca = concept(a)?;
    let cb = concept(b)?;
    let mut out = format!("对比 {}（{}）与 {}（{}）：\n", ca.0, ca.1, cb.0, cb.1);
    for l in langs {
        let Some(idx) = LANGS.iter().position(|x| x == l) else { continue };
        out.push_str(&format!("  {}: {} | {}\n", l, ca.2[idx], cb.2[idx]));
    }
    out.push_str(&format!(
        "差异说明：类别 {} vs {}，语义距离 {:.2}",
        ca.3,
        cb.3,
        semantic_distance(a, b)
    ));
    Some(out)
}

/// #523 领域适配矩阵可用领域。
pub const DOMAINS: [&str; 4] = ["game", "ecommerce", "finance", "general"];

/// #523 领域适配：游戏代码用游戏比喻，电商用电商比喻。
pub fn adapt(domain: &str, id: &str) -> &'static str {
    let base = concept(id).map(|c| c.1).unwrap_or("概念");
    match (domain, id) {
        ("game", "LOCK") => "像房间门锁：同一时刻只有一名玩家能开门",
        ("game", "ASYNC") => "像任务系统：接任务不必站着等完成，可继续做别的事",
        ("game", "THREAD") => "像多名 NPC 各自执行行为脚本",
        ("ecommerce", "LOCK") => "像库存扣减上锁：同一件商品不能被两人同时下单",
        ("ecommerce", "ASYNC") => "像下单后异步发货通知，不阻塞结算页",
        ("ecommerce", "CACHE") => "像前端商品列表缓存，避免每次刷新都查库",
        ("finance", "LOCK") => "像账务记账锁：同一账户不能并发改余额",
        ("finance", "RESULT") => "像对账结果对象：要么成功带金额，要么带差错原因",
        ("finance", "IMMUTABLE") => "像已归档凭证，只能新增不能修改",
        ("general", _) => match id {
            "LOCK" => "像一把钥匙：同一时间只允许一人使用",
            "ASYNC" => "像洗衣机：按下启动去做别的事，洗完再回来",
            "LIST" => "像一列排队的人",
            "MAP" => "像电话簿：按姓名查号码",
            _ => base,
        },
        _ => base,
    }
}

/// #524 比喻三层校验：领域 → 语义 → 反向。杜绝「动作系统」被说成「发动机」。
pub fn verify_metaphor(id: &str, text: &str) -> bool {
    let Some(c) = concept(id) else { return false };
    // 跨域禁用词（游戏域的「动作系统」不得被说成机械域的「发动机」）。
    const CROSS_DOMAIN: [&str; 3] = ["发动机", "引擎", "电路板"];
    let domain_ok = !CROSS_DOMAIN.iter().any(|b| text.contains(b));
    // 反向校验：异常不得被描述成正常通过。
    let reverse_ok = !(c.0 == "EXCEPTION" && text.contains("正常通过"));
    // 常识校验：比喻非空且不与概念名完全重合（必须真的换了个说法）。
    let common_ok = !text.trim().is_empty() && text.trim() != c.1;
    domain_ok && reverse_ok && common_ok
}

/// #525 翻译语义联动：由语义 ID → 目标语言写法 + 领域比喻，而非逐字直译。
pub fn translate_semantic(id: &str, target_lang: &str, domain: &str) -> Option<String> {
    let c = concept(id)?;
    let f = form(id, target_lang)?;
    Some(format!("{} → {} [{}]（比喻：{}）", c.1, f, target_lang, adapt(domain, id)))
}

/// #526 命名建议：语义 → 推荐变量名（camel / snake-joined / Pascal）。
pub fn suggest_name(id: &str) -> Vec<String> {
    let Some(c) = concept(id) else { return Vec::new() };
    let lower = c.0.to_lowercase();
    let words: Vec<String> = lower.split('_').map(|s| s.to_string()).collect();
    let pascal: String = words.iter().map(|w| capitalize(w)).collect();
    let camel = {
        let mut s = String::new();
        for (i, w) in words.iter().enumerate() {
            if i == 0 {
                s.push_str(w);
            } else {
                s.push_str(&capitalize(w));
            }
        }
        s
    };
    vec![camel, lower.replace('_', ""), pascal]
}

fn capitalize(w: &str) -> String {
    let mut cs = w.chars();
    match cs.next() {
        Some(f) => f.to_uppercase().collect::<String>() + cs.as_str(),
        None => String::new(),
    }
}

/// #527 代码理解：陌生语言语法识别。
pub fn explain_syntax(lang: &str, snippet: &str) -> Option<&'static str> {
    let t = snippet.trim();
    match (lang, t) {
        ("Go", s) if s.len() <= 5 && s.contains('?') => Some("错误传播：把上一步的 err 原样返回给调用者"),
        ("Rust", "?") => Some("错误传播：失败即提前返回 Err"),
        ("JS", "??") => Some("空值合并：左边为 null/undefined 才取右边"),
        ("JS", "?.") => Some("可选链：左边为空则整条表达式短路为 undefined"),
        ("Python", "async") => Some("异步声明：函数变为协程，需 await 调用"),
        ("Rust", "&mut") => Some("可变借用：临时独占该值的读写权"),
        ("C++", "std::move") => Some("移动语义：把资源所有权转移，源对象不再持有"),
        _ => None,
    }
}

/// #528 跨语言转换：语义等价而非逐行翻译。
pub fn convert(id: &str, from_lang: &str, to_lang: &str) -> Option<String> {
    let a = form(id, from_lang)?;
    let b = form(id, to_lang)?;
    if a == b {
        return Some(format!("{} 与 {} 在该概念上写法相同：{}", from_lang, to_lang, a));
    }
    Some(format!("{} 的 `{}` 语义等价于 {} 的 `{}`", from_lang, a, to_lang, b))
}

/// #529 语义命令 `/semantic`：search / compare / map / translate / explain / suggest / check。
pub fn semantic_command(line: &str) -> String {
    let t = line.trim().strip_prefix("/semantic").unwrap_or(line.trim()).trim();
    let mut it = t.split_whitespace();
    let sub = it.next().unwrap_or("search");
    let args: Vec<&str> = it.collect();
    match sub {
        "search" => {
            let hits = search(args.first().copied().unwrap_or(""));
            if hits.is_empty() {
                "无语义匹配".into()
            } else {
                hits.iter()
                    .filter_map(|id| concept(id).map(|c| format!("{} ({})", c.0, c.1)))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        }
        "compare" => match (args.first(), args.get(1)) {
            (Some(a), Some(b)) => compare(a, b, &LANGS).unwrap_or_else(|| "未找到概念".into()),
            _ => "用法：/semantic compare <A> <B>".into(),
        },
        "map" => match (args.first(), args.get(1), args.get(2)) {
            (Some(a), Some(b), Some(c)) => equivalent(a, b)
                .map(|f| format!("{} 的 `{}` → {}: {}", b, f, c, form(a, c).unwrap_or("")))
                .unwrap_or_else(|| "未找到概念".into()),
            _ => "用法：/semantic map <语义ID> <语言A> <语言B>".into(),
        },
        "translate" => match (args.first(), args.get(1)) {
            (Some(a), Some(b)) => {
                translate_semantic(a, b, "general").unwrap_or_else(|| "未找到概念".into())
            }
            _ => "用法：/semantic translate <语义ID> <目标语言>".into(),
        },
        "explain" => {
            let lang = args.first().copied().unwrap_or("Go");
            let code = args.get(1).copied().unwrap_or("");
            explain_syntax(lang, code).unwrap_or("无对应语义解释").into()
        }
        "suggest" => {
            let names = suggest_name(args.first().copied().unwrap_or(""));
            if names.is_empty() {
                "未找到概念".into()
            } else {
                names.join(", ")
            }
        }
        "check" => {
            let id = args.first().copied().unwrap_or("");
            let text = args.get(1).copied().unwrap_or("");
            if verify_metaphor(id, text) { "比喻通过三层校验".into() } else { "比喻未通过校验".into() }
        }
        _ => format!("未知子命令 {sub}"),
    }
}

/// #530 语义快捷键 Alt+S。
pub fn semantic_shortcut() -> &'static str {
    "Alt+S"
}

/// #530 触发：查询选中词的语义信息。
pub fn on_shortcut(selection: &str) -> Option<SemanticInfo> {
    let id = disambiguate(selection, Ctx::Unknown)?;
    info(id)
}

/// 比喻库（经三层校验的概念 → 生活比喻）。
pub fn metaphor(id: &str) -> &'static str {
    match id {
        "ASYNC" => "像洗衣机：按下启动去做别的事，洗完再回来",
        "LOCK" => "像一把钥匙：同一时间只允许一人使用",
        "MAP" => "像电话簿：按姓名查号码",
        "LIST" => "像一列排队的人",
        "EXCEPTION" => "像保险丝：一出问题就跳闸，防止烧毁整条电路",
        "NULL" => "像空格子：标注「这里本来就没有东西」",
        "CLASS" => "像模具：一个模具能倒出很多相同的零件",
        "INTERFACE" => "像插座标准：只要引脚一样，什么电器都能插",
        "LAMBDA" => "像便签：随手写一小段，用完即弃",
        "ITERATOR" => "像传送带：一个接一个把货送到你手上",
        "CACHE" => "像冰箱：常用的先放手边，不用每次跑超市",
        "CHANNEL" => "像传送管道：一头投递，另一头接收",
        "THREAD" => "像多个人同时干活",
        "GC" => "像清洁工：没人再使用的垃圾自动被清走",
        "TEST" => "像质检关卡：出厂前逐项检查",
        _ => "暂无专属比喻（按语义类别自适应生成）",
    }
}

/// #516~#530 域自检。
pub fn run_semantic_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("semantic");

    // #516 语义词典 250 概念 × 9 语言
    let core_ids = [
        "ASYNC", "NULL", "MAP", "LIST", "EXCEPTION", "LOCK", "CLASS", "INTERFACE", "LAMBDA", "ITERATOR",
    ];
    let core_ok = core_ids.iter().all(|id| concept(id).is_some());
    let unique_ids = {
        let mut ids: Vec<&str> = DICT.iter().map(|c| c.0).collect();
        let before = ids.len();
        ids.sort_unstable();
        ids.dedup();
        ids.len() == before
    };
    let bubble_ok = bubble("ASYNC", "Go").map(|b| b.contains("ASYNC") && b.contains("goroutine")).unwrap_or(false);
    s.add(
        "#516 语义词典",
        concept_count() == 250
            && mapping_count() == 2250
            && LANGS.len() == 9
            && core_ok
            && unique_ids
            && form("ASYNC", "Go") == Some("goroutine")
            && bubble_ok,
        "250概念×9语言=2250条映射，悬停显示语义气泡",
    );

    // #517 上下文消歧
    let as_type = disambiguate("map", Ctx::TypeDecl);
    let as_call = disambiguate("map", Ctx::CallSite);
    s.add(
        "#517 上下文消歧",
        as_type == Some("MAP") && as_call == Some("TRANSFORM") && as_type != as_call,
        "AST 节点类型+调用方式 → map=数据结构 vs 变换",
    );

    // #518 语义距离
    let near = semantic_distance("LIST", "ARRAY");
    let far = semantic_distance("LIST", "OAUTH");
    let n1 = nearest("mutex");
    s.add(
        "#518 语义距离",
        near < far && near < 0.6 && n1 == Some("LOCK") && nearest("").is_none(),
        "类型+行为+上下文加权，搜索显示最接近概念",
    );

    // #519 跨语言等价
    let py = equivalent("ASYNC", "Python");
    let go = equivalent("ASYNC", "Go");
    let java = equivalent("ASYNC", "Java");
    s.add(
        "#519 跨语言等价",
        py == Some("async/await")
            && go == Some("goroutine")
            && java == Some("Future/CompletableFuture")
            && equivalent("ASYNC", "Brainfuck").is_none(),
        "翻译时找到目标语言最佳等价写法",
    );

    // #520 语义搜索
    let hits = search("异步");
    let hit_ok = hits.contains(&"ASYNC") && hits.iter().all(|h| concept(h).is_some());
    let by_goroutine = search("goroutine");
    s.add(
        "#520 语义搜索",
        hit_ok && by_goroutine.contains(&"ASYNC") && search("").is_empty(),
        "搜「异步」匹配 async/goroutine/Promise",
    );

    // #521 语义气泡
    let b = bubble("MAP", "Go").unwrap();
    s.add(
        "#521 语义气泡",
        b.contains("MAP") && b.contains("键值对") && b.contains("map") && b.contains("电话簿"),
        "悬停1s 显示语义ID+定义+跨语言映射+比喻",
    );

    // #522 语义对比
    let cmp = compare("MAP", "SET", &["Python", "Go"]).unwrap();
    s.add(
        "#522 语义对比",
        cmp.contains("Python: dict | set") && cmp.contains("Go: map | map[T]struct{}") && cmp.contains("差异说明"),
        "并排显示两概念各语言写法+差异说明",
    );

    // #523 领域适配
    let g = adapt("game", "LOCK");
    let e = adapt("ecommerce", "LOCK");
    let f = adapt("finance", "LOCK");
    s.add(
        "#523 领域适配",
        DOMAINS.len() == 4 && g.contains("玩家") && e.contains("库存") && f.contains("账务") && g != e && e != f,
        "领域×功能矩阵：游戏/电商/金融各用各的比喻",
    );

    // #524 比喻三层校验
    let ok = verify_metaphor("LOCK", "像一把钥匙：同一时间只允许一人使用");
    let bad_cross = verify_metaphor("LOCK", "像发动机一样驱动");
    let bad_rev = verify_metaphor("EXCEPTION", "这里会正常通过");
    let bad_same = verify_metaphor("LOCK", "互斥锁");
    let bad_id = verify_metaphor("NO_SUCH_ID", "随便什么");
    s.add(
        "#524 比喻三层校验",
        ok && !bad_cross && !bad_rev && !bad_same && !bad_id,
        "领域→语义→反向，确保「动作系统」不会被说成「发动机」",
    );

    // #525 翻译语义联动
    let tr = translate_semantic("LOCK", "Go", "game").unwrap();
    s.add(
        "#525 翻译语义联动",
        tr.contains("sync.Mutex") && tr.contains("比喻") && tr.contains("玩家"),
        "语义ID→比喻库，基于语义而非直译",
    );

    // #526 命名建议
    let names = suggest_name("SER_JSON");
    s.add(
        "#526 命名建议",
        names == vec!["serJson".to_string(), "serjson".to_string(), "SerJson".to_string()]
            && suggest_name("NOPE").is_empty(),
        "根据语义推荐 camel/snake/Pascal 变量名",
    );

    // #527 代码理解
    let go_q = explain_syntax("Go", "?");
    let js_qq = explain_syntax("JS", "??");
    let none = explain_syntax("Go", "struct{}");
    s.add(
        "#527 代码理解",
        go_q.map(|x| x.contains("错误传播")).unwrap_or(false)
            && js_qq.map(|x| x.contains("空值")).unwrap_or(false)
            && none.is_none(),
        "陌生语言语法识别（Go `?` → 错误传播）",
    );

    // #528 跨语言转换
    let cv = convert("ASYNC", "Python", "Go").unwrap();
    s.add(
        "#528 跨语言转换",
        cv.contains("async/await") && cv.contains("goroutine") && !cv.contains("逐行"),
        "Python async → Go goroutine（语义等价）",
    );

    // #529 语义命令
    let c_search = semantic_command("/semantic search 异步");
    let c_compare = semantic_command("/semantic compare MAP SET");
    let c_map = semantic_command("/semantic map ASYNC Python Go");
    let c_translate = semantic_command("/semantic translate LOCK Rust");
    let c_explain = semantic_command("/semantic explain Go ?");
    let c_suggest = semantic_command("/semantic suggest NULL");
    let c_check = semantic_command("/semantic check LOCK 像一把钥匙");
    s.add(
        "#529 语义命令",
        c_search.contains("ASYNC")
            && c_compare.contains("MAP")
            && c_map.contains("goroutine")
            && c_translate.contains("Mutex")
            && c_explain.contains("错误传播")
            && c_suggest.contains("null")
            && c_check.contains("通过"),
        "search/compare/map/translate/explain/suggest/check",
    );

    // #530 语义快捷键
    let hit = on_shortcut("mutex");
    s.add(
        "#530 语义快捷键",
        semantic_shortcut() == "Alt+S"
            && hit.map(|h| h.id == "LOCK").unwrap_or(false)
            && on_shortcut("zzz_bogus").is_none(),
        "Alt+S 查询选中词语义信息",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f516_dict_shape() {
        assert_eq!(concept_count(), 250);
        assert_eq!(mapping_count(), 2250);
        // 每条都必须有 9 个语言写法且非空
        for c in DICT {
            assert!(!c.0.is_empty() && !c.1.is_empty(), "概念 {} 字段为空", c.0);
            assert!(c.2.iter().all(|f| !f.is_empty()), "概念 {} 存在空写法", c.0);
        }
    }

    #[test]
    fn f517_disambiguate_map() {
        assert_eq!(disambiguate("Map", Ctx::TypeDecl), Some("MAP"));
        assert_eq!(disambiguate("map", Ctx::CallSite), Some("TRANSFORM"));
    }

    #[test]
    fn f520_search_is_concept_level() {
        assert!(search("异步").contains(&"ASYNC"));
        assert!(search("goroutine").contains(&"ASYNC"));
        assert!(search("Promise").contains(&"ASYNC"));
    }

    #[test]
    fn f524_metaphor_layers() {
        assert!(verify_metaphor("ASYNC", "像洗衣机"));
        assert!(!verify_metaphor("ASYNC", "像发动机"));
    }

    #[test]
    fn f528_convert_is_semantic() {
        let s = convert("CLASS", "Rust", "Go").unwrap();
        assert!(s.contains("struct+impl") && s.contains("struct"));
    }

    #[test]
    fn f530_shortcut_lookup() {
        assert_eq!(semantic_shortcut(), "Alt+S");
        assert_eq!(on_shortcut("async").map(|i| i.id), Some("ASYNC"));
    }
}

