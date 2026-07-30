import type { ConflictState, DetectionStatus } from "./types";

export const detectionLabel: Record<DetectionStatus, string> = {
  installed: "已安装",
  cliOnly: "仅 CLI",
  configOnly: "配置残留",
  unsupportedVersion: "版本过低",
  notInstalled: "未检测到",
};

export const conflictLabel: Record<ConflictState, string> = {
  notInstalled: "新安装",
  identical: "内容相同",
  updateAvailable: "可更新",
  conflict: "存在冲突",
  notWritable: "不可写入",
};

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

export function shortPath(path: string): string {
  const home = path.match(/^\/Users\/[^/]+/);
  return home ? path.replace(home[0], "~") : path;
}
