/**
 * 统一前端日志门面（异常实时分析 L0）单元测试：
 * 级别记录、环形上限、console 桥接（tag 提取/静音防重）、会话 ID 恒定、
 * 落盘/恢复、诊断报告组合。DOM 真链路由实机验收覆盖。
 */
import { describe, it, expect, beforeEach, vi, afterEach } from "vitest";
import {
  logDebug, logInfo, logWarn, logError, logRecords, clearLogTimeline,
  sessionId, captureConsoleMessage, persistLogTail, loadPreviousTail,
  buildDiagnosticReport, formatRecord, _resetLoggerForTest,
} from "../logger";
import { clearErrBoard, boardRecords } from "../errBoard";

describe("统一日志门面：环形缓冲与级别", () => {
  beforeEach(() => {
    _resetLoggerForTest();
    clearLogTimeline();
    clearErrBoard();
    localStorage.clear();
  });

  it("logDebug/logInfo 只进时间线；logWarn/logError 进时间线", () => {
    logDebug("t1", "d");
    logInfo("t1", "i");
    logWarn("t1", "w");
    const recs = logRecords();
    expect(recs.map((r) => r.level)).toEqual(["debug", "info", "warn"]);
    expect(recs[2]!.tag).toBe("t1");
  });

  it("环形上限 600：超出时丢最旧", () => {
    for (let i = 0; i < 605; i++) logInfo("x", `m${i}`);
    const recs = logRecords();
    expect(recs.length).toBe(600);
    expect(recs[0]!.msg).toBe("m5");
    expect(recs[599]!.msg).toBe("m604");
  });

  it("sessionId 恒定（8 位十六进制）", () => {
    const a = sessionId();
    const b = sessionId();
    expect(a).toBe(b);
    expect(a).toMatch(/^[0-9a-f]{8}$/);
  });
});

describe("统一日志门面：console 桥接", () => {
  beforeEach(() => {
    _resetLoggerForTest();
    clearLogTimeline();
    localStorage.clear();
  });

  it("captureConsoleMessage 提取 [tag] 前缀并剥离", () => {
    captureConsoleMessage("error", ["[launcher] 启动失败", 42]);
    const recs = logRecords();
    expect(recs.length).toBe(1);
    expect(recs[0]!.tag).toBe("launcher");
    expect(recs[0]!.msg).toBe("启动失败 42");
    expect(recs[0]!.level).toBe("error");
  });

  it("无前缀消息落到 console 标签；Error 实例取 message", () => {
    captureConsoleMessage("warn", ["plain"]);
    captureConsoleMessage("error", [new Error("boom"), { a: 1 }]);
    const recs = logRecords();
    expect(recs[0]!.tag).toBe("console");
    expect(recs[0]!.msg).toBe("plain");
    expect(recs[1]!.msg).toBe('boom {"a":1}');
  });

  it("logError 三路同步：时间线 + errBoard + console 镜像（不重复入时间线）", () => {
    const errSpy = vi.spyOn(console, "error");
    logError("mindmap", "渲染崩溃", "Error: x\n    at f (src/system/x/Foo.tsx:7:1)");
    const recs = logRecords();
    expect(recs.length).toBe(1); // console.error 镜像被静音，不二次入时间线
    expect(recs[0]!.tag).toBe("mindmap");
    expect(errSpy).toHaveBeenCalledWith("[mindmap]", "渲染崩溃");
    // errBoard 同步进板（含栈顶提取与 PII 清洗）
    const board = boardRecords();
    expect(board.length).toBe(1);
    expect(board[0]!.component).toBe("mindmap");
    expect(board[0]!.stackTop).toBe("src/system/x/Foo.tsx:7");
    errSpy.mockRestore();
  });
});

describe("统一日志门面：落盘与诊断报告", () => {
  beforeEach(() => {
    _resetLoggerForTest();
    clearLogTimeline();
    clearErrBoard();
    localStorage.clear();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("persistLogTail → loadPreviousTail 往返一致（容量裁剪 200）", () => {
    for (let i = 0; i < 230; i++) logInfo("p", `m${i}`);
    persistLogTail();
    const tail = loadPreviousTail();
    expect(tail.length).toBe(200);
    expect(tail[0]!.msg).toBe("m30");
    expect(tail[199]!.msg).toBe("m229");
  });

  it("loadPreviousTail 对损坏数据如实返回空", () => {
    localStorage.setItem("variable:log:v1", "{not json");
    expect(loadPreviousTail()).toEqual([]);
    localStorage.setItem("variable:log:v1", JSON.stringify([{ bad: 1 }, null]));
    expect(loadPreviousTail()).toEqual([]);
  });

  it("formatRecord 输出级别/标签/可选 sid", () => {
    logInfo("tag1", "hello");
    const first = logRecords()[0]!;
    expect(formatRecord(first)).toContain("[INFO] [tag1] hello");
    expect(formatRecord(first, true)).toContain(`[${sessionId()}]`);
  });

  it("诊断报告：无错误也有时间线；含会话 ID 与上一会话尾部", () => {
    logWarn("fe", "磁盘余量低");
    persistLogTail();
    clearLogTimeline(); // 模拟崩溃后新会话
    const report = buildDiagnosticReport([], "9.9.9");
    expect(report).toContain("上一会话日志尾部");
    expect(report).toContain("磁盘余量低");
    expect(report).toContain(`会话 ID：${sessionId()}`);
    expect(report).toContain("本会话暂无日志记录");
    expect(report).toContain("9.9.9");
  });

  it("诊断报告：上一会话尾部与本会话重叠部分被 ts 去重", () => {
    logInfo("a", "old-1");
    logInfo("a", "old-2");
    persistLogTail();
    // 当前缓冲不清空（模拟同页隐藏后恢复导出）
    const report = buildDiagnosticReport([], "9.9.9");
    expect(report.match(/old-1/g)?.length).toBe(1);
    expect(report.match(/old-2/g)?.length).toBe(1);
  });
});
