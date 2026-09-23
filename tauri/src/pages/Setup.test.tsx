// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import Setup from "./Setup";
import { api } from "../api";
import { useIsMobile } from "../hooks/useIsMobile";

vi.mock("../api", () => ({ api: { recoverPeerAuth: vi.fn(), syncAll: vi.fn() } }));
vi.mock("../hooks/useIsMobile", () => ({ useIsMobile: vi.fn() }));
vi.mock("../hooks/useReauthenticate", () => ({ useReauthenticate: () => ({ reauthenticate: vi.fn(), busy: false, error: null }) }));
vi.mock("../components/SyncPanel", () => ({ default: () => <div>Pair this device</div> }));

describe("peer sign-in from setup", () => {
  let container: HTMLDivElement;
  let root: Root;
  const complete = vi.fn();
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    vi.clearAllMocks();
    vi.mocked(api.syncAll).mockResolvedValue({} as never);
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });
  afterEach(() => {
    act(() => root.unmount());
    container.remove();
  });

  it.each([false, true])("offers pairing and session recovery before sign-in (mobile=%s)", async (mobile) => {
    vi.mocked(useIsMobile).mockReturnValue(mobile);
    const auth = { authenticated: true, degraded: false, host: "lms.example.edu", uid: null, user_id: null };
    vi.mocked(api.recoverPeerAuth).mockResolvedValue(auth);
    act(() => root.render(<Setup onComplete={complete} />));
    expect(container.textContent).toContain("Pair this device");
    const button = Array.from(container.querySelectorAll("button")).find(b => b.textContent === "Use paired device")!;
    await act(async () => button.click());
    expect(api.recoverPeerAuth).toHaveBeenCalledOnce();
    expect(complete).toHaveBeenCalledWith(auth);
    expect(api.syncAll).toHaveBeenCalledWith(false);
  });

  it("keeps setup open and displays the error when no peer has a usable session", async () => {
    vi.mocked(api.recoverPeerAuth).mockRejectedValue(new Error("Open a signed-in paired device."));
    act(() => root.render(<Setup onComplete={complete} />));
    const button = Array.from(container.querySelectorAll("button")).find(b => b.textContent === "Use paired device")!;
    await act(async () => button.click());
    expect(container.textContent).toContain("Open a signed-in paired device.");
    expect(complete).not.toHaveBeenCalled();
    expect(api.syncAll).not.toHaveBeenCalled();
    expect(button.disabled).toBe(false);
  });
});
