import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import App from "../src/App";

describe("Windows application shell", () => {
  it("identifies the independent Windows preview and local safety boundary", () => {
    render(<App />);
    expect(screen.getByText("Windows 0.1.0")).toBeInTheDocument();
    expect(screen.getByText("仅在本机处理文件，不执行 Skill 脚本")).toBeInTheDocument();
  });
});
