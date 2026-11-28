import { invoke } from "@tauri-apps/api/core";
import { expect, it, vi } from "vitest";
import { desktop } from "./bridge";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

it("requests a latency checkup with no download by default", async () => {
  await desktop.nodeCheckup("s2.node", false);
  expect(invoke).toHaveBeenLastCalledWith("node_checkup", { nodeId: "s2.node" });
});

it("requests a capped download only when selected", async () => {
  await desktop.nodeCheckup("s2.node", true);
  expect(invoke).toHaveBeenLastCalledWith("node_checkup", {
    nodeId: "s2.node",
    options: { throughput: { max_bytes: 200_000_000 } },
  });
});
