/* Invitation only enters a same-origin POST body. Never stored or put in URLs. */
(function () {
  "use strict";
  const form = document.getElementById("tester-form");
  const input = document.getElementById("invitation");
  const button = document.getElementById("tester-submit");
  const status = document.getElementById("tester-status");
  let busy = false;
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (busy || !form.reportValidity()) return;
    busy = true; button.disabled = true; status.textContent = "Checking invitation…";
    const invitation = input.value.trim();
    input.value = "";
    try {
      const response = await fetch("/client-v2/tester-access", {
        method: "POST", credentials: "same-origin", cache: "no-store",
        headers: { "Content-Type": "application/json", "X-4Play-Client": "v2" },
        body: JSON.stringify({ invitation }),
      });
      const result = await response.json();
      if (!response.ok) throw Error(result.message || "Could not validate the invitation.");
      location.replace("/client-v4");
    } catch (error) {
      status.textContent = error.message || "Could not reach the arcade. Please try again.";
      input.focus();
    } finally {
      busy = false; button.disabled = false;
    }
  });
})();
