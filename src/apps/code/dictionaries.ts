/**
 * 第四章 通俗语言翻译字典系统 — 分层词典。
 * L1 通用编程 → L2 语言语法 → L3 框架专属 → L4 项目领域。
 * 全部内置离线；用户补充的解释保存在本地设置（SQLite），见 8.2。
 */

export interface Bilingual {
  zh: string;
  en: string;
  /** 二.5 多风格扩展：故事叙事版 / 工程说明版（ flagship 词条提供，缺失时回退比喻版）。 */
  zhStory?: string;
  zhEng?: string;
  related?: string;
}

type TermTable = Record<string, Bilingual>;

function table(rows: Array<[string, string, string]>): TermTable {
  const out: TermTable = {};
  for (const [term, zh, en] of rows) out[term.toLowerCase()] = { zh, en };
  return out;
}

/** L1 通用编程词典（规范 4.2 首批常用词）。 */
export const L1_UNIVERSAL: TermTable = table([
  ["variable", "一个能装东西的盒子，里面装的东西可以随时换", "A box that holds a value which can be swapped anytime"],
  ["constant", "一个装了东西就再也换不了的盒子", "A sealed box: fill it once, never change it"],
  ["function", "一台机器，你放东西进去，它给你返回加工好的结果", "A machine: put raw material in, get a finished result out"],
  ["parameter", "机器接收的原材料", "The raw material a machine receives"],
  ["return value", "机器加工完给你的成品", "The finished product the machine hands back"],
  ["class", "一张设计图纸，用它可以造出很多个同款物件", "A blueprint for stamping out many identical objects"],
  ["object", "根据图纸造出来的一个具体物件", "A concrete thing built from a blueprint"],
  ["property", "物件身上的一个特征，比如颜色、大小", "A trait of an object, like color or size"],
  ["method", "物件能做的一件事，比如汽车能“跑”“刹车”", "One thing an object can do, like drive or brake"],
  ["array", "一个整整齐齐排好队的清单", "An orderly queue of items"],
  ["dictionary", "一本字典，通过词语查解释", "A lookup book: find meaning by word"],
  ["loop", "重复做同一件事，直到条件满足为止", "Repeat the same job until told to stop"],
  ["if/else", "看情况办事：如果…就…，否则…", "Branch by situation: if… then…, otherwise…"],
  ["event", "有事发生了，比如“按钮被点了”", "Something happened, e.g. a button was clicked"],
  ["callback", "等这件事做完了，再去做那件事", "When this finishes, go do that"],
  ["async", "边做A边做B，不用等A做完再做B", "Do A and B at the same time instead of in turn"],
  ["sync", "一件一件按顺序做，做完一件才做下一件", "One at a time, strictly in order"],
  ["promise", "一张待兑现的取货单，将来某个时刻会拿到东西", "An IOU slip that redeems a value later"],
  ["api", "两个软件之间的对话窗口", "The window where two programs talk"],
  ["interface", "规定了必须要有哪些功能的合同书", "A contract listing required capabilities"],
  ["module", "一个独立的小工具箱", "A self-contained toolbox"],
  ["dependency", "这个工具需要用到的其他工具", "Other tools this tool needs"],
  ["library", "别人写好的工具集合，我们可以直接拿来用", "Prebuilt tools you can use as-is"],
  ["framework", "一整套已经搭好的骨架，我们只需要往里填东西", "A pre-built skeleton you fill in"],
  ["compile", "把人能看懂的代码翻译成电脑能看懂的语言", "Translate human-readable code into machine language"],
  ["interpret", "边翻译边执行，一行一行来", "Translate and run line by line"],
  ["build", "把所有零件装进一个包裹准备发货", "Pack all parts into a shippable bundle"],
  ["deploy", "把做好的软件搬到服务器上让大家能用", "Ship the finished software to where people use it"],
  ["database", "一个巨大的档案柜，专门存资料", "A giant filing cabinet for data"],
  ["sql", "在档案柜里翻找资料的规范语句", "The standard way to ask the cabinet for records"],
  ["frontend", "用户看得见、能点的界面", "The visible, clickable surface"],
  ["backend", "藏在幕后处理数据的大厨房", "The hidden kitchen that processes data"],
  ["server", "一台 7×24 小时不关机、专门给别人提供服务的电脑", "A computer that never sleeps, serving others"],
  ["client", "普通用户手里的手机或电脑", "The phone or computer in the user's hands"],
  ["request", "客户端跟服务器要东西", "The client asking the server for something"],
  ["response", "服务器给客户端回复的内容", "The server's reply"],
  ["json", "一种大家都看得懂的数据交换格式，像一张表格", "A human-readable data format, like a form"],
  ["cache", "把常用的东西放在手边，下次不用重新找", "Keep frequently used items within reach"],
  ["encryption", "把内容变成暗号，只有对的人能看懂", "Turn content into a secret code"],
  ["token", "登录后拿到的通行证", "The pass you get after signing in"],
  ["log", "程序自己写的日记，出问题时可以翻查", "The program's diary for troubleshooting"],
  ["bug", "程序出的毛病", "A flaw in the program"],
  ["debug", "捉虫子，把程序里的毛病找出来修好", "Hunt down and fix flaws"],
  ["refactor", "整理房间，让代码更整洁但功能不变", "Tidy the room without changing what it does"],
  ["git", "给代码存档，随时可以回到过去的版本", "Save checkpoints so you can time-travel back"],
  ["commit", "把这次改动存档", "Snapshot the current changes"],
  ["branch", "开一个平行世界，可以在里面随便改而不影响主线", "A parallel world you can edit freely"],
  ["merge", "把平行世界的改动合并到主线", "Fold parallel edits back into the main line"],
  ["environment", "程序运行的场地：开发场、测试场、正式场", "Where the program runs: dev, staging, production"],
  ["unit test", "给每个小零件单独做质量检查", "Quality-check each part on its own"],
  ["oop", "用“物件+图纸”的思路写程序", "Write programs as objects + blueprints"],
  ["functional programming", "把每件事都变成一台独立的机器", "Turn every job into an independent machine"],
  ["recursion", "自己调用自己，像照镜子里的镜子", "Calling itself, like a mirror facing a mirror"],
  ["data structure", "组织数据的方式，比如清单、字典、树、图", "How data is organized: lists, maps, trees, graphs"],
  ["algorithm", "解决问题的步骤和方法", "The recipe for solving a problem"],
  ["time complexity", "算法运行需要多长时间", "How long the algorithm takes"],
  ["space complexity", "算法运行要占多少内存", "How much memory the algorithm needs"],
  // —— 扩容批次 F：通用编程概念第二批 ——
  ["syntax", "这门语言的语法和用词规矩，写错编译器就不认账", "The grammar rules of a language"],
  ["semantic", "代码实际表达的意思，光语法对还不够", "What the code actually means"],
  ["runtime", "程序真正跑起来的那段时间和环境", "The period and environment when code executes"],
  ["lifecycle", "从出生到销毁的全程：物件也有生老病死", "Birth-to-death stages of a component"],
  ["scope", "势力范围：在这个圈子里起的名字才有效", "Where a name is visible"],
  ["shadowing", "同名遮蔽：外圈的名字被里圈的同名者挡住", "Inner name hides outer name"],
  ["hoisting", "先把声明搬上台面，再开始演戏", "Declarations lifted before execution"],
  ["immutability", "只读不改：要新内容就造新的，旧的留底", "Never mutate; create new instead"],
  ["serialization", "把活物打包成便于运输的干粮", "Turn objects into transportable bytes"],
  ["deserialization", "把干粮还原成活物", "Rebuild objects from bytes"],
  ["concurrency", "多件事在同一个时段内交错推进", "Multiple tasks interleaved in time"],
  ["parallelism", "多件事在同一时刻齐头并进", "Tasks literally running at once"],
  ["thread safety", "好几个工人同时进屋也不会把屋子拆了", "Safe under simultaneous access"],
  ["memory leak", "借了内存不还，房子越堆越满直到爆仓", "Memory borrowed and never returned"],
  ["reference", "门牌号：顺着它能找到原件本身", "A pointer to the original thing"],
  ["value type", "复印件：拿到的就是内容本身", "A copy of the actual content"],
  ["boolean", "是非题：只有成立与不成立两种答案", "True-or-false value"],
  ["integer", "整数：不带小数点的数", "Whole number without decimals"],
  ["float", "浮点数：带小数点的数，偶尔有点不精确", "Decimal number with rounding quirks"],
  ["string", "一串文字，像穿在绳上的字符珠子", "A sequence of characters"],
  ["escape character", "转义符：给特殊字符开的后门通行证", "A marker for special characters"],
  ["regular expression", "文本搜索神器：用一串符号描述要找的花纹", "Pattern language for searching text"],
  ["utf-8", "全世界文字统一的编号方案", "Universal character encoding"],
  ["bytecode", "半成品机器码：给虚拟机看的中间语言", "Intermediate code for a virtual machine"],
  ["virtual machine", "用软件模拟出来的一台电脑", "A computer simulated in software"],
  ["cross-platform", "一份代码，各个系统都能跑", "One codebase, many platforms"],
  ["versioning", "给每次改动编号，随时能认出是哪一版", "Numbered snapshots of changes"],
  ["changelog", "版本日记：每一版改了啥都记着", "A log of what changed per version"],
  ["dependency hell", "依赖打架：各个包要的版本互相矛盾", "Conflicting versions of dependencies"],
  ["monorepo", "所有项目住同一栋楼，统一管理", "All projects in one repository"],
  ["code style", "行文规范：缩进、命名、空格的统一约定", "Consistent formatting conventions"],
  ["naming convention", "起名的规矩：见名知意，风格统一", "Rules for meaningful names"],
  ["magic string", "来历不明的神秘文字，半年后没人看懂", "Unexplained literal string"],
  ["global variable", "公告栏：谁都能改，改完谁也不认账", "Visible and mutable everywhere"],
  ["local variable", "便签纸：只在这一小段里有效", "Only visible in a small block"],
  ["constants file", "把所有定死的值集中放在一个屋子里", "Central place for fixed values"],
  ["boilerplate code", "每次都要抄一遍的开场白", "Intro text you copy every time"],
  ["documentation", "说明书：教后来者怎么用这套东西", "Manuals for future maintainers"],
  ["readme", "门面招牌：项目第一眼看到的介绍", "The project's front-page intro"],
  ["license", "使用许可：这份代码允许别人怎么用", "Legal terms for using the code"],
]);

/** L2 语言语法词典（规范 4.3）。 */
export const L2_LANGUAGE: Record<string, TermTable> = {
  jsts: table([
    ["let / const / var", "定义一个盒子；const 是锁死的，let 可以换内容", "Declare a box; const is locked, let is swappable"],
    ["=>", "简写版的机器（箭头函数）", "Shorthand for a tiny machine (arrow function)"],
    ["async/await", "等一下这个凭证（Promise）兑现了再继续", "Wait for the IOU to redeem, then continue"],
    ["usestate", "让界面能记住某个信息，改了就自动刷新", "UI memory: change it and the screen refreshes"],
    ["useeffect", "界面出现或变化时自动干点啥", "Runs automatically when the UI shows or changes"],
    ["props", "父组件传给子组件的资料袋", "The data bag a parent hands its child"],
    ["jsx", "在 JS 里直接写 HTML 的写法", "Writing HTML right inside JS"],
    ["npm install", "去仓库下载工具包", "Download tool packages from the registry"],
    // 扩容：TS/JS 语法第二批
    ["promise", "一张欠条：现在没有，将来某刻兑现", "An IOU for a future value"],
    ["optional chaining", "门铃按不通就别硬闯，直接走开", "Safe access that yields undefined"],
    ["nullish coalescing", "左边空了就用右边顶着", "Fallback when null/undefined"],
    ["template string", "带填空题的作文纸，变量嵌进句子里", "String with embedded variables"],
    ["spread", "把整袋豆子哗啦倒进锅里", "Expand an array inline"],
    ["destructure", "整箱快递到，开箱按件摆好", "Unpack object into named parts"],
    ["generator", "按需挤牙膏：要一个给一个", "Lazy producer, one value at a time"],
    ["closure", "背包里装着出生时带的工具，走到哪用到哪", "Function keeping its birth scope"],
    ["prototype", "传家宝机制：自己没有就翻祖辈的箱子", "Fallback chain for properties"],
    ["event loop", "单线程的排班表：活儿排进队列挨个做", "Single-threaded task scheduler"],
    ["microtask", "插队小纸条：当前这步做完立刻办", "Jump-the-queue tiny task"],
    ["typeof", "问一句：你是哪种类型", "Ask for the type of a value"],
    ["optional type", "给变量贴上类型标签，错用编译器就骂人", "Type labels checked at compile time"],
    ["enum ts", "一组带编号的固定选项", "Named fixed options"],
    ["generic ts", "万能模具：倒什么料出什么件", "Parameterized type mold"],
    ["readonly", "只读标签：挂上就谁也别想改", "Marked as read-only"],
    ["as const", "彻底焊死：这个值永远不变", "Freeze the value deeply"],
  ]),
  python: table([
    ["def", "定义一台机器（函数）", "Define a machine (function)"],
    ["class", "定义一张图纸（类）", "Define a blueprint (class)"],
    ["self", "指代物件自己", "The object itself"],
    ["pip install", "去 Python 仓库下载工具包", "Download packages from PyPI"],
    ["list / dict / tuple", "清单 / 字典 / 不能改的清单", "List / dict / frozen list"],
    ["for x in ...", "把清单里的东西一个个拿出来处理", "Take items from a list one by one"],
    ["numpy", "数据处理神器", "The number-crunching super tool"],
    ["pandas", "表格数据处理神器", "Super tool for table data"],
    ["torch / tensorflow", "训练 AI 的大工具", "Big toolkits for training AI"],
    // 扩容：Python 语法第二批
    ["indents", "用缩进代替花括号：空格就是代码的围墙", "Indentation defines blocks"],
    ["decorator py", "给函数戴顶帽子：进门前先办点附加事", "Wrap functions with extra behavior"],
    ["list comprehension", "一行写完的迷你流水线：边遍历边加工", "One-line loop with transform"],
    ["virtual env", "每人一个独立实验室，装药互不串味", "Isolated per-project packages"],
    ["dunder", "双下划线魔法方法：物件的开箱说明书", "Dunder methods hook into built-ins"],
    ["typing py", "类型注解：给松散的 Python 系上安全带", "Optional type hints"],
    ["f-string", "带填空题的字符串：变量直接嵌进句子", "Formatted string literal"],
    ["exception py", "出事了就抛出异常，顺着梯子往上找接盘的", "Errors raised and caught up the stack"],
    ["gil", "全局大锁：同一时刻只放一个线程进门", "One thread runs Python at a time"],
    ["asyncio", "异步总管：一件事卡住就先去干别的", "Async scheduling for IO-bound work"],
    ["requests py", "替你去网上办事的跑腿员", "Hassle-free HTTP client"],
    ["fastapi", "自带说明书的后台框架：接口即文档", "API framework with auto docs"],
    ["pytest py", "考官：自动发现并运行你的用例", "Auto-discovering test runner"],
  ]),
  rust: table([
    ["fn", "定义机器（函数）", "Define a machine (function)"],
    ["let mut", "定义一个可以修改内容的盒子", "A box whose contents may change"],
    ["ownership", "谁拥有这个东西，负责最后清理它", "Who owns it, who cleans it up"],
    ["borrow", "临时借用，用完还回去", "Borrow temporarily, return when done"],
    ["cargo", "Rust 项目的管家和工具包下载器", "Rust's butler and package downloader"],
    ["impl", "给某张图纸加上具体功能", "Give a blueprint its actual abilities"],
    ["match", "一个加强版的“看情况办事”，穷举所有情况", "If/else on steroids: cover every case"],
    // 扩容：Rust 语法第二批
    ["lifetime", "寿命标签：保证借来的东西在主人活着时归还", "Borrow must not outlive owner"],
    ["trait rust", "能力证书：谁盖章谁就有这本事", "Shared behavior contract"],
    ["option", "可能没有的答案：Some 是有，None 是没有", "Optional value: Some or None"],
    ["result", "结果的两种下场：Ok 成功，Err 失败", "Ok/Err result type"],
    ["unwrap", "硬拆快递：空包裹直接当场爆炸", "Unwrap or panic on error"],
    ["panic", "立即认输停机：这错我没法处理", "Abort on unrecoverable error"],
    ["clone rust", "深度复印一份，和原件互不相干", "Deep copy, independent"],
    ["move semantics", "搬家不复印：旧主人从此两手空空", "Transfer ownership, no copy"],
    ["shadowing rust", "同名再声明：新的顶替旧的", "New binding replaces old"],
    ["crate", "Rust 的包裹单位：一个可编译的整体", "Rust's compilation unit"],
    ["tokio", "异步大管家：调度成千上万的异步任务", "Async runtime scheduler"],
    ["serde", "打包师傅：物件和文字格式互相转换", "Serialization framework"],
  ]),
  jvm: table([
    ["public / private", "公开的所有人能用，私密的只有自己能用", "Public = everyone; private = only insiders"],
    ["static", "属于图纸本身，不属于任何具体物件", "Belongs to the blueprint, not an instance"],
    ["extends", "继承一张图纸，自动拥有原图纸的所有能力", "Inherit a blueprint and all its abilities"],
    ["interface", "规定必须做到的功能清单", "A list of must-implement abilities"],
    ["maven / gradle", "Java 项目的工具管家", "Java's project tool butler"],
    ["spring", "写后台服务的全套骨架", "A full skeleton for backend services"],
    // 扩容：JVM 语法第二批
    ["garbage collection jvm", "自动保洁：没人用的物件定期清走", "Automatic memory reclamation"],
    ["annotation jvm", "贴在代码上的便利贴，框架会来读", "Metadata read by frameworks"],
    ["abstract", "只画一半的图纸：细节留给子类补齐", "Partially defined base class"],
    ["overload jvm", "同名不同料：按参数挑干法", "Same name, different parameters"],
    ["generics jvm", "万能模具：编译期就定死装的类型", "Compile-time typed molds"],
    ["exception jvm", "受检异常：不接住就别想编译通过", "Checked exceptions must be handled"],
    ["stream jvm", "批量加工流水线：过滤、映射、汇总一步到位", "Declarative collection pipeline"],
    ["optional jvm", "可能空手而归：逼你显式处理空值", "Explicit null handling wrapper"],
    ["synchronized", "上了锁的房间：一次只进一个线程", "One thread at a time"],
    ["jvm", "Java 虚拟机：把字节码翻译成各平台的机器话", "Runs bytecode on any platform"],
    ["spring boot", "开箱即用的 Spring：少配置多干活", "Convention-over-configuration Spring"],
    ["mybatis", "半自动 ORM：SQL 自己写，活儿它来干", "SQL-centric data mapper"],
  ]),
  go: table([
    ["func", "在服务器车间里造一台机器（函数）", "Define a machine (function)"],
    ["goroutine", "一个可以同时干活的小工人，一次可以派几万个", "A tiny worker that runs concurrently — spawn thousands"],
    ["channel", "小工人之间传递消息的管道", "The pipe workers pass messages through"],
    ["defer", "先记下来，最后再执行", "Note it down now, run it last"],
    ["package", "把相关代码打包在一起", "Related code bundled together"],
    // 扩容：Go 语法第二批
    ["struct go", "数据打包盒：一组字段的固定组合", "Typed record of fields"],
    ["interface go", "鸭子判定：会走路会叫就算鸭子", "Implicit behavior matching"],
    ["nil go", "空指针值：还什么都没指到", "The zero reference value"],
    ["error go", "错误是普通返回值：不处理编译器都要提醒", "Errors as ordinary return values"],
    ["sync package", "并发工具箱：锁、等待组都齐备", "Concurrency primitives toolkit"],
    ["waitgroup", "点名册：等所有工人都下班才关门", "Wait for all workers to finish"],
    ["context go", "任务遥控器：取消、超时一句话传达", "Cancellation and timeout carrier"],
    ["defer go", "临走前必办：函数出门最后一件事", "Runs right before function exit"],
    ["slice go", "可伸缩的清单：底层数组的外套", "Growable view over an array"],
    ["map go", "键值对字典：报名字直接取东西", "Key-value lookup"],
    ["go mod", "依赖清单：用哪些包一页纸写清", "Module dependency manifest"],
    ["pprof", "体检仪：找出最耗时的函数", "CPU and memory profiler"],
  ]),
  // —— 扩容批次 M：更多语言的大白话语法映射（第三批语言，全部全新比喻文本） ——
  c_family: table([
    ["include", "把另一份说明书原样贴到这一页前面", "Paste another header's content ahead"],
    ["typedef c", "给复杂类型起个小名，叫起来顺口", "Nickname for a complex type"],
    ["malloc", "向仓库申请一块空地，用完得自己还", "Request raw memory; free it yourself"],
    ["free c", "把借的地还回仓库，忘了还就是内存泄漏", "Return borrowed memory or leak it"],
    ["struct c", "把几样东西捆成一个包裹", "Bundle fields into one record"],
    ["pointer arithmetic", "顺着门牌号往前数几户人家", "Walk addresses like house numbers"],
    ["preprocessor", "开饭前先摆好碗筷的传菜员", "Text-level prep pass before compiling"],
    ["header guard", "说明书上贴个封条，贴两次也不重样", "Include-once guard for headers"],
    ["template cpp", "万能图纸：按材料规格现造一套代码", "C++ generics stamped per type"],
    ["raii", "进门领工牌出门交工牌，谁也不许弄丢", "Resource lifetime tied to object scope"],
    ["virtual destructor", "拆房子前先喊一嗓子，免得楼里还有人", "Tear down derived parts safely"],
    ["namespace cpp", "给同名的工具分仓库贴标签", "Label shelves so names never clash"],
  ]),
  csharp: table([
    ["using csharp", "借来用一下，出了房间自动归还", "Scoped resource disposal"],
    ["property csharp", "带门卫的字段：进出都留登记", "Field guarded by getter/setter"],
    ["linq", "用一句话吩咐仓库：筛出、排好、给我", "Declarative query over collections"],
    ["async csharp", "先去忙别的，好了叫我一声", "Await tasks without blocking"],
    ["interface csharp", "盖章的合同：签字就要兑现", "Contract members must implement"],
    ["delegate csharp", "可以转交别人的任务便签", "Callable handed around as data"],
    ["event csharp", "门铃一按，订过铃的都收到", "Multicast notification hub"],
    ["nuget", "零件超市：报个名就送货上门", "Package manager for .NET"],
    ["garbage collector csharp", "保洁员定时巡逻收走没人认领的物件", "Runtime reclaims unused objects"],
    ["reflection csharp", "不开箱子也能看清里面装了什么", "Inspect types at runtime"],
  ]),
  kotlin_swift: table([
    ["null safety kt", "盒子出厂就写明能不能是空的", "Nullability baked into the type"],
    ["data class kt", "只装数据的卡片盒，盖章就送复印机", "Auto-generated value carriers"],
    ["coroutine kt", "可暂停的小工人，等锅开再去切菜", "Suspending lightweight tasks"],
    ["extension kt", "给邻居家的狗外挂一个握手技能", "Add members to foreign types"],
    ["lateinit kt", "先立字据：内容晚点一定补上", "Promise to initialize later"],
    ["when kt", "加强版看情况办事，条条路都堵死", "Exhaustive switch expression"],
    ["optional swift", "拆礼物前先摇一摇，里头有没有货", "Wrap presence in ? and unwrap safely"],
    ["guard swift", "不对劲就提前离场，别硬着头皮往下演", "Early exit with binding"],
    ["protocol swift", "能力清单：签了就得会这几招", "Swift's behavior contract"],
    ["closure swift", "随身携带上下文的迷你机器", "Closures capture surrounding scope"],
    ["value type swift", "复印件：递出去的是副本不是原件", "Structs copy on assignment"],
    ["arc swift", "计数保洁：最后一个记得它的人走了才打扫", "Reference-counted cleanup"],
  ]),
  dart: table([
    ["widget", "界面的一块积木，大小积木拼成整屏", "Composable UI building block"],
    ["future dart", "一张提货单：货还在路上", "Async value not ready yet"],
    ["async dart", "不等水开就先切菜", "Async without blocking the loop"],
    ["stateless", "只管画，不记事的组件", "Widget without mutable state"],
    ["stateful", "带小本本的组件：改了就重画", "Widget with internal state"],
    ["pubspec", "这份行李清单写明要带哪些包裹", "Dependency manifest for Dart"],
    ["null safety dart", "空盒子也得先声明才许存在", "Sound null safety by default"],
    ["mixin dart", "把别人的绝活直接抄进自己的简历", "Reuse behavior across class trees"],
  ]),
  php_ruby: table([
    ["dollar variable", "每个变量都带一枚硬币当帽子", "Variables prefixed with $"],
    ["array php", "清单和字典是同一个口袋，混着装", "Ordered map doubles as list"],
    ["composer", "PHP 的管家：登个记就把包取回来", "PHP dependency manager"],
    ["associative array", "报名字取东西的口袋", "String-keyed map"],
    ["symbol ruby", "刻在石碑上的名字：只此一份", "Immutable interned name"],
    ["block ruby", "跟着方法走的一小段剧本", "Inline code chunk passed to methods"],
    ["module ruby", "技能包：塞进哪个类哪个就会", "Reusable behavior bundle"],
    ["monkey patch", "趁夜偷偷给别人的机器换个零件", "Reopen and alter existing classes"],
    ["nil ruby", "空无一物的口袋也还是个口袋", "The absence object itself"],
    ["gem", "Ruby 圈的宝石包裹：装上就能发光", "Ruby package"],
  ]),
  sql_shell: table([
    ["join sql", "两张登记表按证件号拼成一张大表", "Combine tables on matching keys"],
    ["group by", "把同样的货物归堆再数件数", "Aggregate rows per bucket"],
    ["index sql", "字典侧边那排字母检索条", "Sorted lookup structure"],
    ["transaction sql", "要么整套都办，要么整单作废", "All-or-nothing work unit"],
    ["view sql", "存好的查询窗口：一推就看到报表", "Saved query presented as a table"],
    ["pipe shell", "上一道工序的出料口对着下一道的进料口", "Chain commands by streaming"],
    ["grep", "拿筛子在水流里捞出带花纹的句子", "Filter lines by pattern"],
    ["cron", "闹钟总管：到点就替你跑一遍活儿", "Scheduled task runner"],
    ["chmod", "给每扇门挂上谁可进谁可动的牌子", "Permission bits on files"],
    ["environment variable", "贴在整个车间墙上的公共告示", "Process-wide settings"],
  ]),
  // —— 第四轮扩容：8 种新语言的大白话语法映射（与既有语言零重复） ——
  lua: table([
    ["local", "只在屋里有效的变量：出了这间房就找不到它", "Block-scoped local variable"],
    ["function lua", "造一台小机器，起个名字随叫随到", "Define a function"],
    ["table lua", "Lua 的万能容器：数组、字典、对象全靠它", "The universal table: array + map + object"],
    ["nil", "什么都没有：盒子是空的，连空气都没装", "The absence of a value"],
    ["metatable", "物件的隐藏说明书：查不到的都翻它", "Fallback table powering OOP and operators"],
    ["colon method", "冒号调用：自动把物件自己塞进第一个参数", "Method call passing self"],
    ["pcall", "保护性调用：出事了不崩盘，只带回坏消息", "Protected call returning errors safely"],
    ["coroutine lua", "可以暂停的流水线：走到哪歇到哪", "Pausable coroutine"],
    ["require lua", "把别的房间里的工具搬进来", "Load a module"],
    ["goto lua", "跳转到贴了标签的那一行", "Jump to a labeled statement"],
  ]),
  perl: table([
    ["use strict", "开工前先立规矩：名字没登记就不许用", "Enforce strict coding rules"],
    ["use warnings", "让解释器多嘴：有问题就嚷嚷", "Enable interpreter warnings"],
    ["scalar sigil", "$ 标量：一件东西的盒子", "Scalar variable sigil"],
    ["array sigil", "@ 数组：一排编号的格子", "Array variable sigil"],
    ["hash sigil", "% 哈希：报名字取东西的抽屉柜", "Hash variable sigil"],
    ["sub", "定义一台小机器（函数）", "Define a subroutine"],
    ["context perl", "语境敏感：同一个表达式在名单和单格之间自动变形", "List vs scalar context coercion"],
    ["regex perl", "正则是 Perl 的母语：模式一写文本秒分", "First-class pattern matching"],
    ["my", "圈地声明：只在当前小院里有效", "Lexically scoped declaration"],
    ["die eval", "die 喊停，eval 接住：一对救命的搭档", "Die/eval error handling pair"],
  ]),
  scala: table([
    ["val scala", "锁死的盒子：赋值一次不再变", "Immutable value binding"],
    ["var scala", "可换内容的盒子", "Mutable variable"],
    ["case class", "数据卡片盒：自带比较、拷贝和拆箱", "Algebraic data class with generated members"],
    ["pattern match scala", "加强版看情况办事：连结构都能对上", "Structural pattern matching"],
    ["option scala", "可能空手的答案：Some 有、None 没有", "Optional value wrapper"],
    ["trait scala2", "能力证书：可以混搭出多重本领", "Mixin-style interface with defaults"],
    ["implicit", "隐式参数：编译器替你递纸条", "Compiler-supplied implicit values"],
    ["companion object", "图纸的孪生管家：static 的替代品", "Singleton paired with a class"],
    ["for comprehension", "flatMap 的糖衣：多层管道写成一句", "Sugar over map/flatMap chains"],
    ["higher-kinded", "高阶类型：模具的模具", "Type constructor abstraction"],
  ]),
  haskell: table([
    ["pure haskell", "纯函数世界：同样的输入永远同样的输出", "Referential transparency"],
    ["type signature", "先写合同的工地：函数名 :: 输入 -> 输出", "Function type declaration"],
    ["lazy haskell", "不催不做：结果要用到才计算", "Lazy evaluation on demand"],
    ["immutable haskell", "万物不变：要新的就造新的", "Values never mutate"],
    ["functor", "会装东西的盒子，还能整盒加工", "Mappable container"],
    ["monad", "带说明书的流水线：一步接一步有章法", "Sequencing abstraction"],
    ["typeclass", "能力证书：声明谁拥有什么本事", "Ad-hoc polymorphism interface"],
    ["currying", "一次只喂一口：喂一个参数还一个半成品", "Partial application chain"],
    ["algebraic data type", "选项拼图：用或和并搭出数据类型", "Sum-of-products data types"],
    ["pattern guard", "看情况办事加守卫：条件对了才进屋", "Guards on pattern equations"],
  ]),
  elixir: table([
    ["immutable elixir", "数据都焊死：改造=生成新的", "Immutable data everywhere"],
    ["pattern match elixir", "等号是配对器：两边对得上才算成立", "Match operator as pattern binding"],
    ["pipe elixir", "|> 传送带：上一站的产出送到下一站", "Pipeline operator"],
    ["process elixir", "小到极点的进程：一人一个信箱互不打扰", "Lightweight isolated process"],
    ["gen server", "服务员模式：收消息、干活、回消息", "Generic server behaviour"],
    ["supervisor", "保姆：孩子进程倒了自己扶起来", "Supervision tree restarts children"],
    ["atom", "名字即值的常量标签", "Constant literal symbol"],
    ["struct elixir", "带了图纸的地图：字段固定好", "Typed map with defaults"],
    ["protocol", "按类型分发的本领清单", "Type-dispatched polymorphism"],
    ["with elixir", "层层把关的流水线：一步失手全盘交代理由", "Match-chaining with else"],
  ]),
  zig: table([
    ["comptime", "编译时是台通用电脑：能算的不留到运行时", "Compile-time code execution"],
    ["no hidden control", "没有暗流：每个分配、每次捕获都写在脸上", "No hidden allocations or control flow"],
    ["defer zig", "出门前必办清单：离开作用域就执行", "Scope-exit execution"],
    ["errunion", "错误也是值：!T 表示要么值要么错", "Error union type"],
    ["try zig", "有错就往上递，没错就拆包用", "Propagate or unpack errors"],
    ["orelse", "空了就换备胎", "Optional coalescing"],
    ["allocator", "内存得自己请管理员：分配器显式传递", "Explicit allocator argument"],
    ["struct zig", "数据打包盒：Zig 唯一的聚合形式", "Struct aggregate type"],
    ["testing zig", "内置考官：test 块一键全跑", "Built-in test blocks"],
    ["unsafe zig", "安全围栏可以拆，但要自己签生死状", "Unchecked operations behind explicit markers"],
  ]),
  julia: table([
    ["multiple dispatch", "按参数类型自动挑干法：同名的机器有好几台", "Methods selected by argument types"],
    ["type julia", "类型是性能说明书：标注得越准跑得越快", "Optional types drive specialization"],
    ["broadcast julia", "点号魔法：. + 让加法逐元素铺满整张表", "Elementwise broadcasting"],
    ["struct julia", "数据打包盒：默认焊死，mutable 才能改", "Immutable-by-default composite"],
    ["range julia", "1:10 就是一条现成的流水线", "Lightweight range abstraction"],
    ["macro julia", "在代码出生前动手术：宏改写表达式", "Compile-time code rewriting"],
    ["vectorized", "别写循环，先想想整列一起算", "Prefer whole-array operations"],
    ["method table", "同一名下多个配方，按入参对号入座", "One function, many methods"],
    ["comprehension julia", "一行流水线造整张清单", "Inline collection builder"],
    ["package julia", "独立实验室式的包管理：环境随项目走", "Per-project package environments"],
  ]),
  r: table([
    ["vector r", "一切的底座：连一个数字都是长度为 1 的向量", "Everything is a vector"],
    ["dataframe", "像 Excel 表格一样的数据框：列是字段行是记录", "Table-like data structure"],
    ["factor", "带档位的分类变量：选项早写死", "Categorical variable levels"],
    ["apply family", "一家人工头：apply/lapply/sapply 批量干活", "Apply family iteration helpers"],
    ["formula r", "波浪号公式：y ~ x 声明模型关系", "Model formula notation"],
    ["na rm", "缺了数据别硬算：先声明怎么处理 NA", "Explicit missing-value handling"],
    ["ggplot", "图层叠画的图表：数据、几何、美学一层层加", "Grammar-of-graphics plotting"],
    ["tidyverse", "一套顺手的全家桶：管道一通到底", "The tidy data toolkit"],
    ["environment r", "变量的户口本：一层层向外找名字", "Lexical environment chain"],
    ["seed r", "抽样前先定种子：随机也可重现", "Set RNG seed for reproducibility"],
  ]),
};

/** L3 框架专属词典。 */
export const L3_FRAMEWORK: TermTable = table([
  ["react", "一个帮你搭界面的骨架：把界面拆成一个个积木（组件）", "UI skeleton: build screens from blocks (components)"],
  ["vue", "和 React 类似的界面骨架，写法更接近普通网页", "A React-like UI skeleton with gentler syntax"],
  ["tauri", "用网页技术做桌面软件的外壳，核心跑得飞快", "A shell for desktop apps built from web tech"],
  ["vite", "开发时的即时热更新服务器 + 打包器", "Instant dev server + bundler"],
  ["webpack", "老牌的零件打包器", "The veteran parts bundler"],
  ["django", "Python 写后台的全套骨架", "Python's batteries-included backend skeleton"],
  ["flask", "轻量的 Python 后台骨架", "A featherweight Python backend skeleton"],
  ["express", "Node 写接口服务的常用小骨架", "The go-to tiny Node server skeleton"],
  // —— 扩容批次 G：框架专属第二批 ——
  ["angular", "Google 出品的全家桶前端框架，规矩多但齐备", "Batteries-included frontend framework"],
  ["svelte", "编译时就干完活的框架：运行时几乎不存在", "Framework that compiles away"],
  ["solid", "信号驱动的前端框架：精确更新不重渲染", "Fine-grained reactivity framework"],
  ["tailwind", "原子样式库：类名就是样式本身", "Utility-first styling"],
  ["element plus", "现成的零件柜：按钮表格弹窗随手取用", "Ready-made UI component kit"],
  ["ant design", "企业级界面零件柜，风格统一", "Enterprise UI component kit"],
  ["nuxt", "Vue 的全栈外壳：页面自动生成与渲染", "Full-stack Vue shell"],
  ["koa", "Express 原班人马的精装版：洋葱式中间件", "Onion-layered Node framework"],
  ["nestjs", "披着 Node 外衣的 Spring：模块化大工程专用", "Angular-style Node backend"],
  ["gin go", "Go 圈的轻量后台骨架：路由中间件齐全", "Lightweight Go HTTP framework"],
  ["actix", "Rust 的高性能后台骨架", "High-performance Rust backend"],
  ["rocket rust", "Rust 圈的易用型后台骨架", "Ergonomic Rust web framework"],
  ["grpc go", "跨语言直呼框架：接口像本地函数一样调用", "Cross-language RPC framework"],
  ["flutter", "一套代码画出苹果安卓两端界面", "One codebase paints both platforms"],
  ["electron", "用网页技术做桌面软件的外壳（个头大些）", "Desktop shell built on web tech"],
  ["unity", "游戏引擎：场景、物体、脚本拼出整个世界", "Game engine: scenes, objects, scripts"],
]);

/** L6 惯用短语词典（规范 8.2 深度扩展）：代码里的“行话”。 */
export const L6_IDIOM: TermTable = table([
  ["boilerplate", "每次都得写一遍的模板代码", "Template code you rewrite every time"],
  ["scaffold", "自动生成的项目骨架", "Auto-generated project skeleton"],
  ["stub", "用假的替身临时顶替", "A fake stand-in for testing"],
  ["fallback", "万一出错时的备用方案", "The backup plan when things fail"],
  ["snapshot", "某一刻的状态存档", "A saved state at one moment"],
  ["circuit breaker", "感觉快挂了就自动切断", "Auto-cuts off before things break"],
  ["rate limiting", "限制单位时间的请求数量", "Cap requests per unit of time"],
  ["tracking", "在代码里种下监测点", "Plant monitoring points in code"],
  ["gray release", "先给一小部分用户试用", "Ship to a small user group first"],
  ["hot reload", "改了代码不用重启就能看到效果", "See changes without restart"],
  ["cold start", "从零开始加载", "Loading from scratch"],
  ["side effect", "除了返回值以外还悄悄干了别的事", "It quietly does more than returning"],
  ["idempotent", "做一次和做十次结果一样", "Once or ten times, same result"],
  ["race condition", "两件事抢着做，谁先谁后不确定", "Two jobs racing, order uncertain"],
  ["deadlock", "互相等对方让路，结果都卡住", "Both wait for each other, all stuck"],
  // —— 扩容批次 I：行话第二批 ——
  ["sugar", "语法糖：同样的药裹了层好吃的糖衣", "Friendlier syntax for the same thing"],
  ["gotcha", "暗坑：看起来没问题，一跑就出事", "Trap that looks fine until it runs"],
  ["footgun", "自爆按钮：随手一写就把自己炸了", "Easy way to shoot yourself"],
  ["plumbing", "水电工程：不起眼但缺了全停的底层活", "Unseen infrastructure wiring"],
  ["scaffolding", "开工前自动搭好的脚手架", "Auto-generated starting skeleton"],
  ["glue", "胶水层：让两个互不相识的系统握上手", "Layer that joins two systems"],
  ["wip", "半成品：还在施工，请勿验收", "Work in progress"],
  ["yak shaving", "剃牦牛：修个bug被迫连修五层前置问题", "Endless prerequisite chain"],
  ["bikeshedding", "自行车棚争论：小事吵翻天，大事没人管", "Endless debates over trivia"],
  ["gold plating", "镀金：功能够用了还一个劲抛光", "Over-polishing finished features"],
  ["premature optimization", "过早优化：还没量体温就开药方", "Optimizing before measuring"],
  ["cargo cult", "拜物教式模仿：抄了仪式没抄原理", "Copying rituals without reasons"],
  ["spike", "探路代码：先糙糙写一版验证可行性", "Throwaway code to test feasibility"],
  ["walkthrough", "走查：拉着大家把代码从头念一遍", "Guided line-by-line review"],
  ["postmortem", "复盘：事故后写检讨找根因不找替罪羊", "Blameless incident review"],
  ["on-call", "值班救火：警报一响就得起身", "Rotating firefighting duty"],
]);

/** L7 项目模式识别字典：架构/风格/流程的通俗描述。 */
export const L7_PATTERN: TermTable = table([
  ["mvc", "MVC：界面、数据、控制分三家管，各司其职", "Model-View-Controller separation"],
  ["mvvm", "MVVM：界面和数据自动同步，中间有个翻译官", "Model-View-ViewModel auto binding"],
  ["redux", "Redux：全项目的状态放进一个大仓库，改东西要打申请", "One big state store with dispatched changes"],
  ["vuex", "Vuex：Vue 版的中央状态仓库", "Vue's central state store"],
  ["microservice", "微服务：大程序拆成很多小服务，各管一摊", "One app split into small services"],
  ["monolith", "单体应用：所有功能装在一个程序里", "Everything in one program"],
  ["rest", "RESTful：用网址+动词约定好的接口风格", "Resource-style HTTP APIs"],
  ["graphql", "GraphQL：想要什么数据自己点菜", "Ask exactly for the data you need"],
  ["websocket", "WebSocket：一条保持接通的电话线，双方随时说话", "A persistent two-way socket"],
  ["cicd", "CI/CD：代码提交后自动检查、自动打包上线", "Auto test & ship pipeline"],
  ["spa", "单页应用：整个网站其实只有一张网页，内容动态换", "One page, dynamic content"],
  // —— 扩容批次 J：架构/流程模式第二批 ——
  ["serverless", "无服务器：代码挂上去就跑，机器的事别人管", "Run code without managing servers"],
  ["faas", "函数即服务：一个函数就是一个部署单元", "Deploy per function"],
  ["event bus", "事件总线：全城广播站，谁有事就在这里喊", "Central channel for events"],
  ["pub sub pattern", "发布订阅：订了频道的都能收到广播", "Broadcast to subscribers"],
  ["state machine", "状态机：角色只能按图走指定的路", "Transitions only along defined edges"],
  ["optimistic ui", "乐观更新：先显示成功，失败了再改口", "Show success first, undo on failure"],
  ["pessimistic lock", "悲观锁：先占住座位再慢慢挑吃的", "Lock first, act second"],
  ["optimistic lock", "乐观锁：先干活，提交时核对有没有人插队", "Check for conflicts at commit"],
  ["read replica", "只读副本：查账去分店，别挤总店", "Reads served by copies"],
  ["write through", "写穿透：改账本的同时把台历也改了", "Write cache and store together"],
  ["eviction policy", "淘汰策略：柜子满了按规矩扔最没用的", "Rules for dropping stale cache"],
  ["ttl", "保质期：到点的缓存自动作废", "Cache expires after a set time"],
  ["feature toggle", "功能开关：新灯装好，拉绳才亮", "Ship dark, enable later"],
  ["ab test", "AB 实验：两版各给一半人看，数据说话", "Split traffic, compare versions"],
  ["dark launch", "暗发布：功能上线但用户看不见，只记录", "Silently run in production"],
  ["blue green", "蓝绿切换：两套环境轮流上岗，换牌不停业", "Parallel environments, switch over"],
  ["zero downtime", "不停机更新：换轮胎不用停车", "Deploy without dropping service"],
  ["data lake", "数据湖：什么格式的原始数据先都倒进来", "Raw data stored as-is"],
  ["etl", "搬运加工队：抽取、清洗、装仓三步走", "Extract, transform, load"],
  ["olap oltp", "记账（OLTP）与算总账（OLAP）分家", "Transaction vs analytics workloads"],
]);

/** L4 项目领域词典（规范 4.4）。 */
export const L4_DOMAIN: TermTable = table([
  ["e-commerce", "网上开店：卖东西、下单、付款、发货", "Online shop: sell, order, pay, ship"],
  ["social", "让人们发消息、点赞、加好友", "Let people message, like, befriend"],
  ["video", "上传、观看、评论视频", "Upload, watch, comment on videos"],
  ["ai", "训练电脑理解和生成人类语言", "Teach computers to understand and generate language"],
  ["vision", "教电脑看图认物", "Teach computers to see"],
  ["crawler", "自动去网页上抓取信息", "Automatically fetch info from web pages"],
  ["data", "从一堆数据里找规律、出报表", "Find patterns and build reports from data"],
  ["game", "画面 + 玩法 + 用户输入", "Graphics + gameplay + user input"],
  ["iot", "让家里的电器和手机对话", "Let appliances and phones talk"],
  ["blockchain", "去中心化的分布式账本", "A decentralized distributed ledger"],
  ["cli", "在黑窗口里输入指令来使用", "Driven by typed commands in a terminal"],
  ["desktop", "装在电脑上双击打开的软件", "Software you double-click on a computer"],
  ["mobile", "装在手机上的软件", "Software installed on a phone"],
  ["web", "在浏览器里访问的服务", "A service you visit in a browser"],
  ["note", "私人记录、写作、笔记类工具", "A private writing / note-taking tool"],
  // —— 扩容批次 H：项目领域第二批 ——
  ["fintech", "把钱搬进代码里：支付、记账、风控", "Money in code: pay, ledger, risk"],
  ["healthtech", "挂号、病历、健康数据的信息化", "Healthcare records and scheduling"],
  ["edutech", "上课、作业、考试搬到线上", "Online classes and grading"],
  ["logistics", "货物从仓到门的全程调度", "Freight from warehouse to door"],
  ["iot platform", "接入成千上万台设备的总控台", "Command center for device fleets"],
  ["chat", "即时通讯：消息秒送、在线状态", "Real-time messaging"],
  ["feed", "信息流：按你的口味排好的内容队列", "Personalized content stream"],
  ["search engine", "全网内容编目，一秒出结果", "Catalog the web, answer in a second"],
  ["recommender", "猜你喜欢：从海量里挑出合口味的几个", "Pick what you will like"],
  ["auth platform", "统一门卫：一个账号走遍全家产品", "One account for every product"],
  ["devtool", "给开发者造的工具：提高写码效率", "Tools that make developers faster"],
  ["analytics", "数数与画图：用户行为全记下", "Counting and charting behavior"],
  ["cms", "内容后台：文章、图片统一发布管理", "Publish and manage content"],
  ["workflow", "审批流：一张单子按流程逐级盖章", "Step-by-step approval flows"],
  ["scheduler app", "日历与排班：把时间切成块来安排", "Calendar and shift planning"],
]);

// ---------- 用户词典（8.2 教学式反馈）与查询 ----------

export interface DictContext {
  /** User-supplied overrides learned in the app (persisted in settings). */
  overrides: Record<string, string>;
  lang: "zh" | "zh-TW" | "en";
  /** 2.5 通俗风格：生活比喻（默认）/ 故事叙事 / 工程说明。 */
  style?: "metaphor" | "story" | "engineering";
}

/** 三.2 旗舰词条的多风格补全（第二批持续扩写至此表）。 */
export const STYLE_EXTRAS: Record<string, { zhStory: string; zhEng: string }> = {
  "variable": { zhStory: "小机器人随身带着一个储物格，需要时就往里放一块记忆碎片", zhEng: "存储数据的命名内存单元，值可随执行改变" },
  "constant": { zhStory: "小机器人立下一块石碑，刻上就再也不改了", zhEng: "只读存储值，初始化后不可重新赋值" },
  "function": { zhStory: "小工人接过任务单，咕噜咕噜加工一阵，然后把成品递回来", zhEng: "可重复调用的代码块，接收输入并返回输出" },
  "class": { zhStory: "一张会说话的图纸，照着它能盖出一整条街的房子", zhEng: "面向对象的类型模板，定义字段与方法" },
  "object": { zhStory: "照着蓝图真的盖起来的那栋房子，门窗俱全", zhEng: "类的运行时实例，持有独立状态" },
  "array": { zhStory: "火车车厢一节接一节，每节都编了号", zhEng: "有序、可索引的线性集合" },
  "loop": { zhStory: "小机器人围着操场一圈圈跑，哨声一响才停下", zhEng: "按条件重复执行的控制结构" },
  "async": { zhStory: "小机器人一边炖汤一边写作业，汤好了闹钟会响", zhEng: "非阻塞执行模型，等待期间可做其他工作" },
  "promise": { zhStory: "快递单握在手里，货到那天凭单取件", zhEng: "异步结果的容器，可链式消费" },
  "recursion": { zhStory: "镜子里还有镜子，一层层照进去直到看见底", zhEng: "函数直接或间接调用自身的求解方式" },
  "closure": { zhStory: "出差时把家里的钥匙也揣进兜里，走到哪都能开门", zhEng: "捕获词法作用域变量的函数值" },
  "event": { zhStory: "门铃一响，小机器人就知道该去开门了", zhEng: "系统中发生的、可被监听处理的信号" },
  "cache": { zhStory: "常用的工具不每次都回仓库拿，就放在工位抽屉里", zhEng: "保存高频数据副本以降低重复获取成本" },
  "token": { zhStory: "进门时发的临时胸牌，凭牌通行、过期作废", zhEng: "携带身份凭证的访问凭据" },
  "api": { zhStory: "两个餐厅之间的小窗口，菜单递进去、菜端出来", zhEng: "程序间约定的调用界面与协议" },
  "database": { zhStory: "一间巨大的档案室，每个格子都有编号，查起来飞快", zhEng: "结构化持久存储与检索系统" },
  "git": { zhStory: "时光机：每次改动拍一张快照，随时能回到过去", zhEng: "分布式版本控制系统" },
  "framework": { zhStory: "房子盖到一半的毛坯房，水电都通，你只管装修", zhEng: "提供骨架与约定、由使用者填充的实现集合" },
  "refactor": { zhStory: "不换家具只打扫房间，住起来舒服多了", zhEng: "在不改变外部行为的前提下改善内部结构" },
  "algorithm": { zhStory: "一份菜谱：几步、几火、几克，照着做就能上桌", zhEng: "解决问题的明确步骤序列" },
  "bug": { zhStory: "藏在墙缝里的小虫子，一开灯就跑出来捣乱", zhEng: "程序行为与预期不符的缺陷" },
  "unit test": { zhStory: "每个零件出厂前都上秤称一称、通个电试一试", zhEng: "针对最小可测单元的自动化验证" },
  "ownership": { zhStory: "东西归谁，谁负责最后收拾干净", zhEng: "Rust 的资源归属与释放责任模型" },
  "goroutine": { zhStory: "一队可以同时干活的小工人，随叫随到", zhEng: "Go 的轻量级并发执行单元" },
  "usestate": { zhStory: "墙上挂了块小黑板，写上去界面立刻跟着变", zhEng: "React 的组件状态钩子" },
};

/** 三.1 十大类扩展包（持续扩写；与 L1-L7 合并查询）。 */
export const EXPANSION_PACKS: Record<string, TermTable> = {
  通用概念扩展: table([
    ["heap", "堆：一个大仓库，放东西不排队，用时自己挑", "Heap: free-form dynamic storage"],
    ["stack", "栈：一摞盘子，后放的先拿", "Stack: LIFO structure"],
    ["pointer", "指针：写着我家住几楼的路牌", "Pointer: address referencing memory"],
    ["reference", "引用：书的借书证，不是书本身", "Reference: alias to a value"],
    ["iteration", "迭代：一格一格往前挪", "Iteration: step-by-step traversal"],
    ["serialization", "序列化：把大件家具拆成能装箱的平板件", "Serialization: object to transportable bytes"],
    ["deserialization", "反序列化：照着说明书把平板件装回家具", "Deserialization: bytes back to object"],
    ["hashtable", "哈希表：报上名字直接跳到对应格子", "Hashtable: O(1) key lookup"],
    ["queue", "队列：排队买奶茶，先来先服务", "Queue: FIFO structure"],
    ["assertion", "断言：中途大喊一声‘这里必须是这样！’", "Assertion: fail-fast invariant check"],
    ["immutability", "不可变：造出来就定型的工艺品", "Immutability: values never mutate"],
    ["garbage collection", "垃圾回收：保洁阿姨自动收走没人要的纸箱", "GC: automatic memory reclamation"],
    ["thread", "线程：一条同时干活的传送带", "Thread: concurrent execution lane"],
    ["process", "进程：一间独立的作坊，和别的作坊互不打扰", "Process: isolated program instance"],
  ]),
  js_ts扩展: table([
    ["destructuring", "解构：整箱快递直接按件拆开放好", "Destructuring: unpack into named parts"],
    ["spread operator", "展开运算符：把整叠牌摊开在桌上", "Spread: expand iterable in place"],
    ["optional chaining", "可选链：先敲门再进屋，没人就不进了", "Optional chaining: safe deep access"],
    ["nullish coalescing", "空值合并：没货就上备货", "Nullish coalescing: default on null/undefined"],
    ["template string", "模板字符串：填空造句", "Template literal: interpolated strings"],
    ["generator", "生成器：按暂停键的流水线，要一个做一个", "Generator: pausable producer"],
    ["iterator", "迭代器：发牌员，一次发一张", "Iterator: sequential access protocol"],
    ["typescript generic", "泛型：能装任何型号的万能盒子图纸", "Generics: parameterized types"],
    ["module", "模块：带门牌的独立房间，屋里东西按需外借", "Module: scoped unit of code"],
    ["type inference", "类型推断：不用开口，编译器自己看懂", "Type inference: compiler derives types"],
  ]),
  python扩展: table([
    ["list comprehension", "列表推导：一行流水线造出整张清单", "List comprehension: inline mapping"],
    ["with statement", "with 语句：借的东西用完自动还", "With: scoped resource management"],
    ["args kwargs", "*args/**kwargs：来者不拒的收货口", "args/kwargs: variadic parameters"],
    ["type hint", "类型注解：盒子上贴的型号标签", "Type hints: optional static annotations"],
    ["dataclass", "dataclass：自动帮你写好图纸里最啰嗦的部分", "Dataclass: generated boilerplate classes"],
    ["slice", "切片：从长条面包上切下想要的那段", "Slicing: range extraction"],
    ["magic method", "魔术方法：物件自带的双下划线暗号", "Dunder methods: protocol hooks"],
    ["virtualenv", "虚拟环境：每人一个独立厨房，互不串味", "Virtualenv: isolated dependency envs"],
    ["gil", "GIL：厨房里只有一把主灶钥匙，同一时刻一人开火", "GIL: global interpreter lock"],
  ]),
  rust_go_c扩展: table([
    ["lifetime", "生命周期：借的东西必须在保质期内用完", "Lifetimes: borrow validity scopes"],
    ["trait", "trait：能力证书，谁拿到谁就会这门手艺", "Trait: shared behavior contract"],
    ["mutex", "互斥锁：公共卫生间一次只进一个人", "Mutex: exclusive access lock"],
    ["unsafe", "unsafe：摘掉手套徒手操作，后果自负", "Unsafe: opt out of safety checks"],
    ["segmentation fault", "段错误：闯进了不属于你的房间", "Segfault: invalid memory access"],
    ["memory leak", "内存泄漏：借了东西不还，仓库迟早爆满", "Leak: memory never released"],
    ["goroutine channel", "channel：小工人之间的传菜口", "Channel: message passing between goroutines"],
    ["header file", "头文件：菜单目录，先看有什么再上菜", "Header: declarations ahead of definitions"],
    ["manual memory", "手动内存管理：水电煤全靠自己抄表关阀", "Manual memory: explicit alloc/free"],
    ["null pointer", "空指针：路牌上写着‘此处无路’还硬闯", "Null pointer: reference to nothing"],
  ]),
  jvm移动扩展: table([
    ["annotation", "注解：贴在代码上的便签，工具会来读", "Annotation: metadata read by tooling"],
    ["type erasure", "泛型擦除：盒子上的型号标签只贴给检查员看", "Erasure: generics erased at runtime"],
    ["coroutine", "协程：会自己让路的工人，不堵车道", "Coroutine: suspensible execution"],
    ["nullable type", "可空类型：盒子里可能啥都没有，先声明免得吓一跳", "Nullable: explicit absence"],
    ["data class", "data class：只为装数据而生的卡片盒", "Data class: state carrier"],
    ["extension function", "扩展函数：给别人的物件外挂一个新技能", "Extension: add methods externally"],
    ["declarative ui", "声明式界面：你只说‘长这样’，怎么画交给框架", "Declarative UI: describe, don't mutate"],
    ["activity lifecycle", "生命周期：从开门营业到打烊的完整流程", "Lifecycle: creation to destroy phases"],
  ]),
  前端框架扩展: table([
    ["virtual dom", "虚拟 DOM：先在草稿纸上排好版再动真墙", "Virtual DOM: diff before paint"],
    ["reactivity", "响应式：牵一发动全身的联动机关", "Reactivity: derived auto-updates"],
    ["computed", "computed：自动算出来的属性，原料变了结果跟着变", "Computed: cached derived value"],
    ["watcher", "watch：专职盯着某个数据的风哨", "Watcher: observe and react"],
    ["directive", "指令：写在标签上的小咒语", "Directive: template behavior hook"],
    ["router", "路由：前台分诊台，不同网址进不同房间", "Router: URL to view mapping"],
    ["state lifting", "状态提升：兄弟吵架找家长主持公道", "Lifting state: shared parent ownership"],
    ["css variable", "CSS 变量：给颜色起个名，全屋统一换装", "CSS custom properties"],
    ["media query", "媒体查询：屏幕窄了就换一套姿势", "Media query: responsive breakpoints"],
    ["code splitting", "代码分割：行李太多就分箱托运", "Code splitting: bundle per route"],
    ["lazy loading", "懒加载：客人点单了才下厨", "Lazy loading: load on demand"],
  ]),
  后端框架扩展: table([
    ["middleware", "中间件：流水线上的质检台，件件过手", "Middleware: layered request handlers"],
    ["routing table", "路由表：总机接线员的话务单", "Routing table: endpoint mapping"],
    ["orm", "ORM：把数据库的表格当物件来使唤", "ORM: rows as objects"],
    ["migration", "迁移：给档案柜加抽屉的施工单", "Migration: versioned schema change"],
    ["dependency injection", "依赖注入：零件不用自己造，直接递到你手上", "DI: dependencies provided externally"],
    ["serializer", "序列化器：把物件翻译成快递单", "Serializer: model to wire format"],
    ["auth middleware", "鉴权中间件：门口查胸牌的保安", "Auth middleware: credential gate"],
    ["task queue", "任务队列：不急的活儿先取号排队", "Task queue: deferred background jobs"],
    ["rpc", "RPC：远程喊一嗓子，对面函数就跑了", "RPC: call remote like local"],
  ]),
  数据库扩展: table([
    ["transaction", "事务：要么整桌菜都上齐，要么全撤单", "Transaction: all-or-nothing unit"],
    ["index", "索引：字典侧边的字母检索条", "Index: fast lookup structure"],
    ["primary key", "主键：每个人的身份证号，绝不重号", "Primary key: unique row identity"],
    ["foreign key", "外键：两张表之间的亲戚关系证明", "Foreign key: cross-table reference"],
    ["view", "视图：常查的报表存成一个虚拟窗口", "View: saved query surface"],
    ["trigger", "触发器：一动这张表，那边自动办事", "Trigger: event-driven procedure"],
    ["connection pool", "连接池：车队待命，用车即提", "Pool: reusable connections"],
    ["replication", "主从复制：正本抄了几份副本随时顶上", "Replication: redundant copies"],
    ["sharding", "分库分表：一个巨柜拆成多个小柜分头管", "Sharding: horizontal partitioning"],
    ["migration script", "迁移脚本：档案柜的改造施工图", "Migration script: schema upgrade"],
  ]),
  devops扩展: table([
    ["container", "容器：随插随用的隔离迷你房间", "Container: isolated runnable unit"],
    ["image", "镜像：房间的全息快照，照着能克隆无数间", "Image: immutable build template"],
    ["orchestration", "编排：乐队指挥统一调度几百个容器", "Orchestration: scheduled fleet management"],
    ["load balancer", "负载均衡：银行叫号机，客多的窗口少派点", "Load balancer: traffic distribution"],
    ["reverse proxy", "反向代理：前台统一收件再分发给后台", "Reverse proxy: unified entry"],
    ["service discovery", "服务发现：通讯录自动更新谁搬了家", "Service discovery: dynamic registry"],
    ["blue-green deploy", "蓝绿部署：新旧两套房，验好再换门牌", "Blue-green: parallel cutover"],
    ["rollback", "回滚：新菜砸了就端回上一道", "Rollback: revert to last good"],
    ["health check", "健康检查：每隔几秒摸摸鼻子看还活着没", "Health check: liveness probe"],
    ["secrets management", "密钥管理：保险柜统一发放钥匙，绝不贴墙上", "Secrets: centralized credential vault"],
  ]),
  俚语扩展: table([
    ["glue code", "胶水代码：把两个不会说话的工具粘到一起传话", "Glue code: integration shims"],
    ["magic number", "魔法数字：来历不明的神秘数字，半年后没人认识", "Magic number: unexplained literal"],
    ["hardcode", "硬编码：把地址直接刻在墙上，搬家就傻眼", "Hardcode: inline fixed values"],
    ["tech debt", "技术债：先欠着快改，利息越滚越大", "Tech debt: deferred cost of shortcuts"],
    ["dead code", "死代码：没人打这扇门，但它还占着走廊", "Dead code: unreachable code"],
    ["syntactic sugar", "语法糖：同样的药，裹了一层好吃的糖衣", "Syntactic sugar: friendlier syntax"],
    ["early return", "早返回：不对劲就先走，别硬撑到最后", "Early return: guard exits"],
    ["defensive programming", "防御式编程：进门先看脚下有没有香蕉皮", "Defensive: validate everything"],
    ["big ball of mud", "大泥球：什么 都缠在一起的大毛线团", "Big ball of mud: tangled architecture"],
    ["reinvent the wheel", "重复造轮子：别人有现成的轮胎你偏自己搓", "Reinventing: rewriting existing solutions"],
  ]),
  // —— 模块三扩容批次 C：多语言语法的大白话映射 ——
  多语言语法扩展: table([
    ["closure", "闭包：背包里装着出门时带的工具，走到哪用到哪", "Closure: function carrying its birth scope"],
    ["recursion", "递归：套娃，一层层打开直到最小的那个", "Recursion: self-calling until base case"],
    ["pointer", "指针：写着别人家门牌号的纸条", "Pointer: reference to a memory address"],
    ["garbage collection", "垃圾回收：有保洁阿姨定时收走没人用的桌子", "GC: automatic memory reclamation"],
    ["type inference", "类型推断：不用报户口，系统一看就知道你是什么", "Type inference: compiler deduces types"],
    ["generic", "泛型：万能模具，倒什么材料出什么零件", "Generic: parameterized types"],
    ["lambda", "匿名函数：不留名字的一次性跑腿员", "Lambda: inline unnamed function"],
    ["iterator", "迭代器：发牌员，一次发一张直到发完", "Iterator: sequential access protocol"],
    ["generator", "生成器：按需挤牙膏，要一点给一点", "Generator: lazy step-by-step producer"],
    ["pattern matching", "模式匹配：看菜下单，什么形状配什么做法", "Pattern matching: shape-based branching"],
    ["trait", "特质：能力证书，谁挂谁就有这本事", "Trait: shared behavior contract"],
    ["mixin", "混入：把别人的技能直接抄进自己简历", "Mixin: behavior merged into a class"],
    ["decorator", "装饰器：出差前给西装多别一枚徽章", "Decorator: wrap to extend behavior"],
    ["annotation", "注解：贴在代码上的便利贴，工具会来读", "Annotation: metadata for tooling"],
    ["macro", "宏：把常用动作录成一个快捷键", "Macro: code that writes code"],
    ["null safety", "空安全：出门前必查钥匙带没带", "Null safety: no accidental undefined"],
    ["ownership", "所有权：每件东西只有一个主人，交接要签字", "Ownership: Rust's single-owner memory rule"],
    ["borrow checker", "借用检查：借书要登记，还书要核对", "Borrow checker: compile-time borrow rules"],
    ["destructure", "解构：快递整箱到，开箱按件摆好", "Destructuring: unpack into named parts"],
    ["spread operator", "展开运算符：把整袋豆子倒进锅里", "Spread: expand iterable inline"],
    ["optional chaining", "可选链：门铃按不通就别硬闯", "Optional chaining: safe property access"],
    ["template literal", "模板字符串：带填空题的作文纸", "Template literal: string interpolation"],
    ["saturating add", "饱和加法：温度计到顶了就贴着顶，不爆表", "Saturating arithmetic: clamped overflow"],
    ["defer", "延迟执行：临走前一定记得关灯", "Defer: run at scope exit"],
    ["goroutine", "协程：轻量工人，一个人能雇几万个", "Goroutine: cheap concurrent task"],
    ["channel", "通道：工人之间传纸条的固定信道", "Channel: message pipe between tasks"],
    ["virtual method", "虚方法：具体干活的由子类说了算", "Virtual method: overridable dispatch"],
    ["operator overloading", "运算符重载：让加号也会加两个矩阵", "Operator overloading: custom operators"],
    ["union type", "联合类型：今天穿A明天穿B，二选一", "Union type: one of several shapes"],
    ["type guard", "类型守卫：先验明正身再放行", "Type guard: runtime narrowing check"],
  ]),
  // —— 模块三扩容批次 D：架构模式的大白话映射 ——
  架构模式扩展: table([
    ["monolith", "单体架构：一家店包办全部业务，柜台厨房都在一处", "Monolith: single deployable app"],
    ["microservices", "微服务：大公司拆成各管一摊的小组", "Microservices: many small services"],
    ["mvc", "MVC：账本（模型）、柜台（控制器）、橱窗（视图）各司其职", "MVC: model-view-controller separation"],
    ["mvvm", "MVVM：橱窗和账本之间装一台自动同步机", "MVVM: view-model binding layer"],
    ["event-driven", "事件驱动：门铃一响才有人动，平时各干各的", "Event-driven: react to published events"],
    ["pub/sub", "发布订阅：广播站喊一嗓子，订了频道的都收到", "Pub/Sub: broadcast via broker"],
    ["message broker", "消息中间件：小区代收驿站，谁有空谁来取", "Message broker: decoupling queue"],
    ["layered architecture", "分层架构：接待、办事、库房一层层往上递", "Layered: stacked responsibilities"],
    ["hexagonal architecture", "六边形架构：核心业务居中，外界全走统一插座", "Hexagonal: ports and adapters"],
    ["clean architecture", "整洁架构：心脏不关心衣服是什么牌子", "Clean: domain at the core"],
    ["cqrs", "CQRS：记账和查账用两套本子，各抄各的", "CQRS: split write and read models"],
    ["event sourcing", "事件溯源：不存结果只存流水账，结果能重放出来", "Event sourcing: state from event log"],
    ["saga", "Saga：长途接力，一棒失败全队按预案退回", "Saga: distributed transaction choreography"],
    ["circuit breaker", "熔断器：电路过载先跳闸，别把整栋楼拖垮", "Circuit breaker: fail fast, then recover"],
    ["bulkhead", "舱壁隔离：一个舱进水别让整艘船沉", "Bulkhead: isolate failure domains"],
    ["sidecar", "边车模式：正驾驶专心开车，副驾驶管导航查表", "Sidecar: helper process beside service"],
    ["service mesh", "服务网格：给每个快递员配一台对讲机和导航", "Service mesh: infra-level traffic layer"],
    ["api gateway", "API 网关：大厦前台，访客登记后再放行上楼", "API gateway: single managed entry"],
    ["bff", "BFF：给手机端配个专属翻译，别让小程序啃大接口", "BFF: backend for frontend"],
    ["strangler fig", "绞杀者模式：新藤蔓慢慢缠满旧树，最后取而代之", "Strangler: gradual legacy replacement"],
    ["adapter pattern", "适配器：港版插头接国标插座的那转换头", "Adapter: interface translation"],
    ["facade", "门面：一站式窗口，复杂流程都藏在后面", "Facade: simplified front to subsystem"],
    ["proxy pattern", "代理：明星的经纪人，见人先过他这一关", "Proxy: stand-in with control"],
    ["observer pattern", "观察者：点了个关注，主播开播自动提醒", "Observer: subscribe for notifications"],
    ["strategy pattern", "策略：导航的路线偏好，随时切换最快/最省/最短", "Strategy: swappable algorithms"],
    ["factory pattern", "工厂：下单只说要什么，造的细节归车间", "Factory: centralized object creation"],
    ["singleton", "单例：全校只有一个广播站，谁都从这儿喊", "Singleton: one shared instance"],
    ["builder pattern", "建造者：点餐式组装，逐项加上想要的配置", "Builder: step-by-step construction"],
    ["dependency inversion", "依赖倒置：插座定标准，电器跟着标准走", "DIP: depend on abstractions"],
    ["graceful degradation", "优雅降级：高速堵了就走国道，服务照常", "Graceful degradation: fallback behavior"],
    ["idempotent", "幂等：同一单重复提交也只扣一次钱", "Idempotency: repeat-safe operations"],
    ["rate limiting", "限流：售票窗口一次只放行这么多黄牛……不，顾客", "Rate limit: cap request throughput"],
    ["backpressure", "背压：下游吃不下就告诉上游慢点倒", "Backpressure: consumer-driven flow control"],
    ["blueprint pattern", "蓝图模式：施工先看图纸，别上来就砌墙", "Blueprint: plan-before-build pattern"],
    ["repository pattern", "仓储模式：取数据只找管理员，别自己翻仓库", "Repository: data access abstraction"],
    ["unit of work", "工作单元：一批改动攒一起，结账时一次提交", "Unit of Work: batched commit"],
  ]),
  // —— 模块三扩容批次 E：质量与工程实践 ——
  工程实践扩展: table([
    ["unit test", "单元测试：给每个零件单独过一遍质检", "Unit test: verify components in isolation"],
    ["integration test", "集成测试：零件装成整机再试一次", "Integration test: verify assembled parts"],
    ["e2e test", "端到端测试：模拟真实顾客从进门到买单走全程", "E2E: full user journey test"],
    ["mock", "Mock：排练时用替身演员顶一阵", "Mock: stand-in for real dependency"],
    ["coverage", "覆盖率：质检员摸过的零件占总数的比例", "Coverage: tested code fraction"],
    ["ci", "持续集成：每次交作业机器都自动批一遍", "CI: automated build-and-verify"],
    ["cd", "持续交付：批完直接装车发货", "CD: automated release pipeline"],
    ["code review", "代码评审：交稿前先让同事挑挑刺", "Code review: peer inspection"],
    ["lint", "静态检查：作文交之前先过一遍错别字扫描", "Lint: static rule check"],
    ["refactoring", "重构：屋子重新收拾，家具一件不换功能", "Refactor: restructure without behavior change"],
    ["smoke test", "冒烟测试：新手机先看能不能开机", "Smoke test: minimal sanity check"],
    ["regression", "回归测试：修完水管再检查全屋有没有漏水", "Regression: verify nothing else broke"],
    ["feature flag", "功能开关：新灯先装上，拉绳才亮", "Feature flag: runtime toggle"],
    ["canary release", "金丝雀发布：先让 5% 的用户尝鲜", "Canary: gradual rollout"],
    ["observability", "可观测性：机舱装满仪表盘，随时看得见转速", "Observability: metrics, logs, traces"],
    ["distributed tracing", "链路追踪：一个快递从下单到签收每站都有记录", "Tracing: request journey across services"],
    ["chaos engineering", "混沌工程：没事拉一次电闸，看看预案灵不灵", "Chaos engineering: deliberate fault drills"],
    ["slo", "SLO：向用户承诺的账单——本月可用 99.9%", "SLO: reliability objective"],
    ["hotfix", "热修复：大火先扑灭，起因回头再查", "Hotfix: urgent production patch"],
    ["pair programming", "结对编程：一人开车一人看路", "Pair programming: two at one keyboard"],
  ]),
  // —— 模块三扩容批次 K：移动端与桌面 ——
  移动端扩展: table([
    ["activity", "安卓的房间：一屏一个房间，进出要登记", "Android screen unit with lifecycle"],
    ["fragment", "可拼装的半屏组件：房间里的活动隔断", "Reusable Android UI section"],
    ["intent", "安卓的快递单：想干什么写在上面交给系统派件", "Android's message for actions"],
    ["viewmodel", "屏幕的记忆管家：转屏重建也不丢数据", "Survives config changes"],
    ["lifecycle", "生命周期：从见面到告别的每一站", "Stage callbacks from create to destroy"],
    ["recycler view", "循环复用列表：滑出屏外的卡片回收再用", "Recycled list item views"],
    ["swift ui", "声明式画界面：说清要什么样，系统自己画", "Declarative iOS UI"],
    ["uikit", "iOS 的老派界面车间：手动摆件手动刷新", "Classic imperative iOS UI"],
    ["app state", "应用的全局记忆：全 App 共用的小账本", "Shared app-level state"],
    ["navigation", "导航栈：页面像书一样一页页压上去", "Stack of pushed screens"],
    ["deep link", "直达链接：一条网址直接跳进 App 内页", "URL that opens an inner page"],
    ["push notification", "推送：App 不开也能收到门铃", "Server-pinged notification"],
    ["responsive layout", "自适应排版：屏幕大小变了姿势跟着变", "Adapts to any screen size"],
    ["hot reload flutter", "改完即见：界面刷新快到不用等", "Instant UI refresh on save"],
    ["app store review", "上架审核：摆好摊还得先过市场管理员", "Platform review before release"],
    ["permission", "权限申请：用摄像头前先征得主人同意", "Runtime permission prompts"],
    ["offline first", "离线优先：没网也能记账，有网再同步", "Works offline, syncs later"],
    ["background task", "后台任务：App 退到幕后仍偷偷干活", "Work continued in background"],
  ]),
  // —— 模块三扩容批次 L：机器学习与数据 ——
  机器学习扩展: table([
    ["model", "模型：练成之后的一台预测机器", "A trained predictor"],
    ["training", "训练：拿题海一遍遍磨这台机器", "Fit parameters on data"],
    ["inference", "推断：出师之后正式上岗答题", "Run the trained model"],
    ["dataset", "题库：所有练习题和标准答案", "The examples to learn from"],
    ["label", "标准答案：每条数据对应的正确结果", "The expected output"],
    ["feature", "特征：题目里的关键线索", "Input signals for the model"],
    ["loss", "扣分项：预测和答案差多少", "How far off the prediction is"],
    ["gradient descent", "下山法：每步往坡最陡的反方向挪", "Step downhill to reduce loss"],
    ["overfitting", "死记硬背：题库全会，新题全崩", "Memorized training data, fails on new"],
    ["underfitting", "没学进去：连题库都答不好", "Too simple to even fit training"],
    ["neural network", "神经网络：一层层叠加的开关网", "Stacked layers of weights"],
    ["transformer", "变换器：靠注意力读全句的大模型骨架", "Attention-based architecture"],
    ["attention", "注意力：读到哪个词就重点看哪个", "Weight which parts matter"],
    ["embedding", "词向量：把词变成坐标，相近的词住得近", "Words as nearby vectors"],
    ["token llm", "词块：文字切成的小段，模型的口粮单位", "Chunk of text fed to a model"],
    ["prompt", "咒语：写给模型的任务说明书", "Instructions given to a model"],
    ["fine tuning", "微调：给通才补几节专业课", "Specialize a pretrained model"],
    ["quantization", "量化：把模型压成小字条省内存", "Shrink weights to fewer bits"],
    ["gpu", "显卡：几千个小算盘同时打的大算场", "Massively parallel compute"],
    ["batch size", "批量大小：一口吞几道题再消化", "Examples processed per step"],
    ["epoch", "轮次：整个题库从头到尾刷一遍", "One full pass over the data"],
    ["tensor", "张量：多维大表格，深度学习的通用货箱", "Multi-dimensional array"],
  ]),
  // —— 模块三扩容批次 N：通用概念第二批（全新比喻，不与既有条目重复） ——
  通用概念扩展二: table([
    ["bit", "一个开关：只有开和关两种姿势", "A single on/off switch"],
    ["endianness", "先寄大件还是先寄小件，各家快递规矩不同", "Byte order conventions"],
    ["checksum", "货单上的封条：对不上就是路上被拆过", "Integrity fingerprint of data"],
    ["idempotence key", "凭票取餐：同一张票只端一次盘", "Repeat-safe request marker"],
    ["circuit switching", "全程包下一条专线直到挂断", "Dedicated end-to-end path"],
    ["packet switching", "拆成一封封信各自投递，到了再拼回原文", "Data in independently routed packets"],
    ["latency", "按铃到开门之间那段干等的时间", "Waiting time before a reply"],
    ["throughput", "传送带每分钟能运多少箱", "Work completed per unit time"],
    ["backwards compatibility", "新钥匙仍能开老锁", "New versions still serve old inputs"],
    ["feature detection", "先问会不会，别猜人家会不会", "Probe capability before using it"],
    ["graceful shutdown", "打烊前把炉火关好、账本收齐再走", "Clean stop that finishes in-flight work"],
    ["watchdog", "巡夜的保安：发现没人动就拉闸重启", "Timer that restarts hung systems"],
  ]),
  // —— 模块三扩容批次 O：云原生与运维第二批 ——
  云原生扩展: table([
    ["sidecar proxy", "挂在主车旁的副驾：替主车接听所有电话", "Helper beside the main container"],
    ["autoscaling", "客人多了自动加桌子，走了自动撤", "Capacity follows demand"],
    ["helm chart", "整套家具的安装说明书：一条命令全装好", "Packaged deployment template"],
    ["config map", "开关全装在一块统一配电板上", "Externalized configuration"],
    ["namespace k8s", "园区里划出的独立院落", "Isolated cluster tenancy"],
    ["ingress", "园区唯一的正门：访客都从这里进出", "Single managed cluster entrance"],
    ["stateful set", "每台机器有固定工位和专属储物柜", "Identity-bound workloads"],
    ["grace period", "搬家前给的宽限期：先收尾再断电", "Shutdown grace before kill"],
    ["log aggregation", "各家日记统一收进档案室方便翻查", "Central log collection"],
    ["chaos monkey", "随机拔网线的捣蛋鬼，逼你练好应急预案", "Random fault injector"],
  ]),
};

export const EXPANSION_COUNT: number = Object.values(EXPANSION_PACKS).reduce((n, t) => n + Object.keys(t).length, 0);

export interface DictContext {
  /** User-supplied overrides learned in the app (persisted in settings). */
  overrides: Record<string, string>;
  lang: "zh" | "zh-TW" | "en";
  /** 2.5 通俗风格切换（缺省 = 生活比喻）。 */
  style?: "metaphor" | "story" | "engineering";
}

export function lookupTerm(term: string, ctx: DictContext): string | null {
  const key = term.trim().toLowerCase();
  if (!key) return null;
  const ov = ctx.overrides[key];
  if (ov) return ov;
  const tables: TermTable[] = [
    L1_UNIVERSAL, L3_FRAMEWORK, L4_DOMAIN, L6_IDIOM, L7_PATTERN,
    ...Object.values(EXPANSION_PACKS),
    L2_LANGUAGE.jsts ?? {}, L2_LANGUAGE.python ?? {}, L2_LANGUAGE.rust ?? {},
    L2_LANGUAGE.jvm ?? {}, L2_LANGUAGE.go ?? {},
    L2_LANGUAGE.c_family ?? {}, L2_LANGUAGE.csharp ?? {}, L2_LANGUAGE.kotlin_swift ?? {},
    L2_LANGUAGE.dart ?? {}, L2_LANGUAGE.php_ruby ?? {}, L2_LANGUAGE.sql_shell ?? {},
    L2_LANGUAGE.lua ?? {}, L2_LANGUAGE.perl ?? {}, L2_LANGUAGE.scala ?? {},
    L2_LANGUAGE.haskell ?? {}, L2_LANGUAGE.elixir ?? {}, L2_LANGUAGE.zig ?? {},
    L2_LANGUAGE.julia ?? {}, L2_LANGUAGE.r ?? {},
  ];
  for (const t of tables) {
    const hit = t[key];
    if (hit) {
      if (ctx.lang === "en") return hit.en;
      const extra = STYLE_EXTRAS[key];
      if (ctx.style === "story") return extra?.zhStory ?? hit.zhStory ?? hit.zh;
      if (ctx.style === "engineering") return extra?.zhEng ?? hit.zhEng ?? hit.zh;
      return hit.zh;
    }
  }
  const extra = STYLE_EXTRAS[key];
  if (extra) {
    if (ctx.lang === "en") return null; // extras 为中文风格表
    if (ctx.style === "story") return extra.zhStory;
    if (ctx.style === "engineering") return extra.zhEng;
    return null;
  }
  return null;
}


/** 8.2：不认识的词返回 null，由界面标注“这个词我还不懂”。 */
export function translateTerm(term: string, ctx: DictContext): string {
  return lookupTerm(term, ctx) ?? term;
}

/**
 * 对一段描述文本做术语通俗化替换（最长优先，避免子串先吞掉长词）。
 * 返回未知词列表供教学式反馈。
 */
export function translateText(text: string, terms: string[], ctx: DictContext): { out: string; unknown: string[] } {
  const known = terms
    .filter((t) => t.trim().length > 0)
    .sort((a, b) => b.length - a.length);
  let out = text;
  const unknown: string[] = [];
  for (const term of known) {
    const hit = lookupTerm(term, ctx);
    if (hit) {
      out = replaceInsensitive(out, term, hit);
    } else if (!unknown.includes(term)) {
      unknown.push(term);
    }
  }
  return { out, unknown };
}

function replaceInsensitive(text: string, term: string, replacement: string): string {
  let out = text;
  let idx = out.toLowerCase().indexOf(term.toLowerCase());
  while (idx !== -1) {
    out = out.slice(0, idx) + replacement + out.slice(idx + term.length);
    idx = out.toLowerCase().indexOf(term.toLowerCase(), idx + replacement.length);
  }
  return out;
}

/** 目录用“厨房/客厅/仓库”式比喻描述（规范 1.x 愿景 + 5.2）。 */
export function describeDir(dirName: string, lang: "zh" | "zh-TW" | "en"): string {
  const n = dirName.toLowerCase();
  const pick = (zh: string, en: string): string => (lang === "en" ? en : zh);
  if (["src", "source", "app", "lib", "core", "internal"].includes(n)) return pick("厨房 —— 核心代码都在这里烹制", "Kitchen — where the core code is cooked");
  if (["components", "widgets", "views", "ui", "pages", "screens", "features"].includes(n)) return pick("预制菜间 —— 界面积木（组件）在这里拼装", "Prep station — UI blocks are assembled here");
  if (["tests", "test", "__tests__", "spec", "e2e"].includes(n)) return pick("质检车间 —— 每个零件在这里做质量检查", "QC workshop — parts get quality-checked here");
  if (["docs", "doc", "documentation"].includes(n)) return pick("说明书柜 —— 项目文档都存在这里", "Manual shelf — project documents live here");
  if (["config", "configs", "conf", "settings"].includes(n)) return pick("配电箱 —— 项目的各种开关都装在这里", "Fuse box — the project's switches");
  if (["scripts", "tools", "bin"].includes(n)) return pick("自动化流水线 —— 一次性的小工具和脚本", "Assembly line — one-off helper scripts");
  if (["assets", "static", "images", "media", "fonts", "icons"].includes(n)) return pick("仓库 —— 图片、字体等静态物料", "Storage room — images, fonts and other assets");
  if (["api", "routes", "server", "controllers", "endpoints"].includes(n)) return pick("前台接待 —— 对外的服务接口在这里值守", "Reception desk — the public service interface");
  if (["db", "database", "models", "migrations", "schema"].includes(n)) return pick("档案柜 —— 数据的存取规矩都在这里", "Filing cabinet — data storage rules");
  if (["styles", "css", "scss", "less"].includes(n)) return pick("装修队 —— 负责外观的样式表", "Decorators — the stylesheets");
  if (["public", "wwwroot", "webroot"].includes(n)) return pick("陈列柜 —— 直接对外的静态样品", "Showcase — public static samples");
  return pick("房间 —— 项目的一个功能分区", "A room — one functional area of the project");
}
