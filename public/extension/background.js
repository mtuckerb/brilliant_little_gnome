// Service worker handles network requests (popup CSP blocks cross-origin fetch in MV3)
chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (msg.action !== "sendCookies") return;

  fetch(`${msg.serverUrl}/api/v1/auth/cookies`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ host: msg.host, cookies: msg.cookies }),
  })
    .then(async (res) => {
      if (res.ok) {
        sendResponse({ success: true });
      } else {
        const body = await res.text();
        sendResponse({ success: false, error: `Server error (${res.status}): ${body}` });
      }
    })
    .catch((err) => {
      sendResponse({ success: false, error: `Connection failed: ${err.message}` });
    });

  return true; // keep message channel open for async response
});
