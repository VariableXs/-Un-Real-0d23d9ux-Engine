import { Component, type ErrorInfo, type ReactNode } from "react";
import { errMessage } from "../lib/ipc";
import { logError } from "../lib/logger";

interface Props {
  children: ReactNode;
}
interface State {
  error: { code: string; message: string } | null;
}

export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(e: unknown): State {
    return { error: errMessage(e) };
  }

  componentDidCatch(e: unknown, info: ErrorInfo): void {
    // AI-20 M-79 + 统一日志门面：console 镜像 + errBoard 环形 + ipc.log（Rust 落盘/applog 实时总线）
    logError("ErrorBoundary", errMessage(e).message, e instanceof Error ? e.stack : info.componentStack);
  }

  render(): ReactNode {
    if (this.state.error) {
      return (
        <div className="fatal-screen">
          <div className="fatal-card">
            <h2>⚠ Variable</h2>
            <p>出现错误，但你的数据是安全的。<br />Something went wrong. Your data is safe.</p>
            <code>{this.state.error.code}: {this.state.error.message}</code>
            <button
              className="btn primary"
              onClick={() => window.location.reload()}
              type="button"
            >
              重新加载应用 / Reload
            </button>
          </div>
        </div>
      );
    }
    return this.props.children;
  }
}