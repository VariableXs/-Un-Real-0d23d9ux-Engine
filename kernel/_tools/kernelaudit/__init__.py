# -*- coding: utf-8 -*-
"""Varix 内核静态审计工具包。

设计目标只有一个：**把模块文档里写着、却从来没人验过的性质，变成可执行断言。**

内核里 150+ 个模块在文档头写着「零 `unwrap()` / `expect()` / `panic!`」「零浮点」
「零真实 IO」「零墙钟」「确定性 LCG」「冻结 FNV-1a」这类承诺。这些承诺此前只
存在于注释中：改代码的人换了实现，注释不会报警。本包把它们逐条对账。

模块划分（每个模块只做一件事，互不依赖对方内部结构）：

    sources.py   源码索引 —— 读文件、剥注释/字符串、切出测试区、定位条目
    claims.py    文档承诺抽取与逐条核验
    hazards.py   panic 面普查（内核路径 / 判据层 / 测试层三分类）
    registry.py  模块声明 ↔ 自检注册表 的闭环对账
    report.py    Markdown / JSON 渲染
    cli.py       命令行入口

用法：
    python -m kernelaudit claims      # 只验文档承诺
    python -m kernelaudit hazards     # 只普查 panic 面
    python -m kernelaudit registry    # 只对账自检注册
    python -m kernelaudit all         # 全跑并输出报告
"""

__all__ = ["sources", "claims", "hazards", "registry", "report", "cli"]
__version__ = "1.0.0"
