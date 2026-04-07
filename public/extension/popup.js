const BRIGHTSPACE_DOMAIN = "courses.maine.edu";
const DEFAULT_SERVER = "http://10.1.0.75:4567";
const STORAGE_KEY = "brilliant_server_url";

const serverInput = document.getElementById("server");
const sendBtn = document.getElementById("send");
const statusEl = document.getElementById("status");
const versionEl = document.getElementById("version");

// Show version so we can confirm the right build is loaded
if (versionEl) versionEl.textContent = "v1.1.0";

// Load saved server URL
chrome.storage.local.get(STORAGE_KEY, (result) => {
  serverInput.value = result[STORAGE_KEY] || DEFAULT_SERVER;
});

// Save server URL on change
serverInput.addEventListener("change", () => {
  chrome.storage.local.set({ [STORAGE_KEY]: serverInput.value.trim() });
});

sendBtn.addEventListener("click", async () => {
  sendBtn.disabled = true;
  sendBtn.textContent = "Sending...";
  showStatus("", "hidden");

  try {
    const cookies = await chrome.cookies.getAll({ domain: BRIGHTSPACE_DOMAIN });

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

    // Send via service worker to avoid popup CSP restrictions
    chrome.runtime.sendMessage(
      {
        action: "sendCookies",
        serverUrl,
        host: BRIGHTSPACE_DOMAIN,
        cookies: cookieString,
      },
      (response) => {
        if (chrome.runtime.lastError) {
          showStatus(`Extension error: ${chrome.runtime.lastError.message}`, "error");
          return;
        }
        if (!response) {
          showStatus("No response from background worker. Try reloading the extension.", "error");
          return;
        }
        if (response.success) {
          showStatus("Connected! You can close this popup.", "success");
        } else {
          showStatus(response.error, "error");
        }
      }
    );
  } catch (err) {
    showStatus(`Error: ${err.message}`, "error");
  } finally {
    sendBtn.disabled = false;
    sendBtn.textContent = "Send Cookies to Brilliant";
  }
});

function showStatus(msg, cls) {
  statusEl.textContent = msg;
  statusEl.className = cls;
}
