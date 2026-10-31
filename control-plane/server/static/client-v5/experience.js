/* V5: opt-in floating landscape controls; V4 browsing remains unchanged. */
(() => {
  const body = document.body;
  if (body.dataset.clientGeneration !== "5") return;
  const controls = document.querySelector("#touch-controls");
  if (controls) controls.dataset.touchLayout = "floating";
})();
