import { renderHook, act } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "../src/api";
import { useInventoryEvents } from "../src/hooks/useInventoryEvents";

vi.mock("../src/api", () => ({
  api: {
    scanEnvironment: vi.fn(),
  },
}));

describe("useInventoryEvents", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.mocked(api.scanEnvironment).mockResolvedValue({ clients: [], inventories: [] });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.clearAllMocks();
  });

  it("does not start the inventory timer while a manage operation is busy", async () => {
    const onEnvironment = vi.fn();
    renderHook(() => useInventoryEvents(false, onEnvironment));

    await act(async () => {
      vi.advanceTimersByTime(15_000);
      await Promise.resolve();
    });

    expect(api.scanEnvironment).not.toHaveBeenCalled();
    expect(onEnvironment).not.toHaveBeenCalled();
  });

  it("polls at most one scan at a time when enabled", async () => {
    const onEnvironment = vi.fn();
    let resolveScan: ((value: { clients: []; inventories: [] }) => void) | undefined;
    vi.mocked(api.scanEnvironment).mockImplementation(
      () => new Promise((resolve) => (resolveScan = resolve)),
    );
    renderHook(() => useInventoryEvents(true, onEnvironment));

    await act(async () => {
      vi.advanceTimersByTime(5_000);
      vi.advanceTimersByTime(5_000);
    });
    expect(api.scanEnvironment).toHaveBeenCalledTimes(1);

    await act(async () => {
      resolveScan?.({ clients: [], inventories: [] });
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(onEnvironment).toHaveBeenCalledTimes(1);
  });
});
