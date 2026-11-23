import { describe, expect, it } from "vitest";
import { connectPhase } from "./labels";
import { busyOperation, connectionOperation, finishOperation, startOperation, toggleAction } from "./operations";

describe("operations under way", () => {
  it("ends only the operation that finished", () => {
    let operations = startOperation([], "delay");
    operations = startOperation(operations, "capture session");
    // The session operation ends first: the delay test is still under way.
    operations = finishOperation(operations, "capture session");
    expect(operations).toEqual(["delay"]);
    expect(busyOperation(operations)).toBe("delay");
    operations = finishOperation(operations, "delay");
    expect(busyOperation(operations)).toBeNull();
    // Two runs of the same operation: one ending leaves the other.
    expect(finishOperation(["node", "node"], "node")).toEqual(["node"]);
    expect(finishOperation(["node"], "delay")).toEqual(["node"]);
  });

  it("finds the connection operation among others", () => {
    expect(connectionOperation(["delay"])).toBeNull();
    expect(connectionOperation(["delay", "disconnect"])).toBe("disconnect");
    expect(connectPhase("Connected", connectionOperation(["delay", "disconnect"]))).toBe("disconnecting");
    // A delay test alone does not move the switch.
    expect(connectPhase("Connected", connectionOperation(["delay"]))).toBe("on");
  });

  it("lets the switch act whatever else is under way, and asks to wait while the connection changes", () => {
    // During a long delay test the switch is not "busy": the phase comes from the connection alone.
    expect(toggleAction(connectPhase("Connected", connectionOperation(["delay"])))).toBe("disconnect");
    expect(toggleAction(connectPhase("Not connected", connectionOperation(["subscription"])))).toBe("connect");
    expect(toggleAction(connectPhase("Connection failed", null))).toBe("connect");
    expect(toggleAction(connectPhase("Not connected", connectionOperation(["connect"])))).toBe("wait");
    expect(toggleAction(connectPhase(undefined, null))).toBe("wait");
  });
});
