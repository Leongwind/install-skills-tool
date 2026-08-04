import type { ConflictState, DetectionStatus, ManagementStatus } from "./types";

export const statusText: Record<DetectionStatus, string> = {
  installed: "已安装",
  cliOnly: "仅 CLI",
  configOnly: "配置残留",
  unsupportedVersion: "版本过低",
  notInstalled: "未安装",
};

export const conflictText: Record<ConflictState, string> = {
  notInstalled: "新安装",
  identical: "内容相同",
  updateAvailable: "可更新",
  conflict: "需覆盖",
  notWritable: "不可写",
};

export const managementText: Record<ManagementStatus, string> = {
  toolManaged: "本工具安装",
  adopted: "已纳管",
  external: "外部",
  modified: "已修改",
  unsafe: "不安全",
  passive: "被动发现",
};

export function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
export function formatWindowsPath(path: string) {
  return path
    .replace(/^\\\\\?\\UNC\\/i, "\\\\")
    .replace(/^\\\\\?\\/i, "")
    .replace(/\//g, "\\");
}
