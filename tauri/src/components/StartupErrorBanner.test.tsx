// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import StartupErrorBanner from "./StartupErrorBanner";
import { api } from "../api";

vi.mock("../api", () => ({ api: { startupError: vi.fn() } }));

let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  vi.resetAllMocks();
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

it("reports a runtime panic without claiming the app never opened", async () => {
  vi.mocked(api.startupError).mockResolvedValue({ setup: null, previous_panic: "late iOS event" });
  await act(async () => root.render(<StartupErrorBanner />));
  expect(container.textContent).toContain("The previous session ended unexpectedly.");
  expect(container.textContent).toContain("late iOS event");
  expect(container.textContent).not.toContain("before the app opened");
});

it("prioritizes a current setup failure over the previous panic", async () => {
  vi.mocked(api.startupError).mockResolvedValue({ setup: "database failed", previous_panic: "old panic" });
  await act(async () => root.render(<StartupErrorBanner />));
  expect(container.textContent).toContain("Brilliant started without its backend.");
  expect(container.textContent).toContain("database failed");
  expect(container.textContent).not.toContain("old panic");
});
