const BRIGHTSPACE_DOMAIN = "courses.maine.edu";
const DEFAULT_SERVER = "http://10.1.0.75:4567";
const STORAGE_KEY = "brilliant_server_url";

const serverInput = document.getElementById("server");
const sendBtn = document.getElementById("send");
const statusEl = document.getElementById("status");
const versionEl = document.getElementById("version");

if (versionEl) versionEl.textContent = "v1.0.0-firefox";

// Load saved server URL
browser.storage.local.get(STORAGE_KEY).then((result) => {
  serverInput.value = result[STORAGE_KEY] || DEFAULT_SERVER;
});

// Save server URL on change
serverInput.addEventListener("change", () => {
  browser.storage.local.set({ [STORAGE_KEY]: serverInput.value.trim() });
});

sendBtn.addEventListener("click", async () => {
  sendBtn.disabled = true;
  sendBtn.textContent = "Sending...";
  showStatus("", "hidden");

  try {
    const cookies = await browser.cookies.getAll({ domain: BRIGHTSPACE_DOMAIN });

    if (!cookies.length) {
      showStatus("No Brightspace cookies found. Are you logged in at courses.maine.edu?", "error");
      return;
    }

    const hasSession = cookies.some((c) => c.name === "d2lSecureSessionVal");
    if (!hasSession) {
      showStatus(
        "Session cookie not found. Please log into Brightspace first.",
        "error"
      );
      return;
    }

    const cookieString = cookies.map((c) => `${c.name}=${c.value}`).join("; ");
    const serverUrl = serverInput.value.trim().replace(/\/+$/, "");

    showStatus("Sending to Brilliant server...", "info");

    const response = await fetch(`${serverUrl}/api/v1/auth/cookies`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ host: BRIGHTSPACE_DOMAIN, cookies: cookieString }),
    });

    if (response.ok) {
      showStatus("Connected! You can close this popup.", "success");
    } else {
      const body = await response.text();
      showStatus(`Server error (${response.status}): ${body}`, "error");
    }
  } catch (err) {
    showStatus(`Connection failed: ${err.message}`, "error");
  } finally {
    sendBtn.disabled = false;
    sendBtn.textContent = "Send Cookies to Brilliant";
  }
});

function showStatus(msg, cls) {
  statusEl.textContent = msg;
  statusEl.className = cls;
}
