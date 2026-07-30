import { describe, expect, it } from "vitest";
import { conflictLabel, detectionLabel, formatBytes, shortPath } from "../src/ui";

describe("UI presentation helpers", () => {
  it("presents detection and conflict states in Chinese", () => {
    expect(detectionLabel.unsupportedVersion).toBe("版本过低");
    expect(conflictLabel.updateAvailable).toBe("可更新");
  });

  it("formats sizes and redacts a macOS home prefix", () => {
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(shortPath("/Users/test/.trae/skills/demo")).toBe("~/.trae/skills/demo");
  });
});
