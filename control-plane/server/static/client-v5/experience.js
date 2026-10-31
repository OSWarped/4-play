/* V5: a distinct presentation and idle attract mode over the proven V2-V4 stack. */
(() => {
  const body = document.body;
  if (body.dataset.clientGeneration !== "5") return;
  const controls = document.querySelector("#touch-controls");
  if (controls) controls.dataset.touchLayout = "floating";

  const toolbar = document.querySelector(".v4-toolbar");
  const brand = document.createElement("div");
  brand.className = "v5-brand";
  brand.innerHTML = '<span aria-hidden="true">4</span><strong>PLAY</strong>';
  toolbar?.prepend(brand);

  const attract = document.createElement("section");
  attract.className = "v5-attract";
  attract.hidden = true;
  attract.setAttribute("aria-label", "4-Play attract mode");
  attract.innerHTML = `
    <img class="v5-attract-art" alt="" />
    <div class="v5-attract-wash"></div>
    <header><div class="v5-attract-brand"><span>4</span><strong>PLAY</strong></div><small>CONNECTED ARCADE</small></header>
    <div class="v5-attract-copy">
      <p class="v5-attract-kicker">FEATURED GAME</p>
      <h1>4-Play Arcade</h1>
      <p class="v5-attract-meta"></p>
      <button type="button">Tap to browse</button>
    </div>
    <div class="v5-attract-progress" aria-hidden="true"></div>`;
  body.append(attract);

  const art = attract.querySelector(".v5-attract-art");
  const title = attract.querySelector("h1");
  const kicker = attract.querySelector(".v5-attract-kicker");
  const meta = attract.querySelector(".v5-attract-meta");
  let idleTimer = null, rotation = null, slide = 0, visible = false, gamepadState = "";
  const idleMs = new URLSearchParams(location.search).get("attract") === "1" ? 1500 : 45000;

  function slides() {
    return [...document.querySelectorAll("#games .game-cover")].map(card => ({
      title: card.querySelector(".cover-name")?.textContent?.trim() || "4-Play Arcade",
      image: card.querySelector(".cover-art:not([hidden])")?.src || "",
      live: card.dataset.live === "true",
      details: [...card.querySelectorAll(".cover-footer .pill")].map(node => node.textContent.trim()).filter(Boolean).join(" · "),
    })).sort((a, b) => Number(b.live) - Number(a.live));
  }

  function renderSlide() {
    const available = slides();
    if (!available.length) return exitAttract();
    const value = available[slide++ % available.length];
    art.hidden = !value.image;
    if (value.image) art.src = value.image;
    kicker.textContent = value.live ? "PLAYING NOW" : "FEATURED GAME";
    kicker.classList.toggle("is-live", value.live);
    title.textContent = value.title;
    meta.textContent = value.live ? `${value.details || "Live session"} · Join the game` : value.details;
    attract.classList.remove("is-changing");
    requestAnimationFrame(() => attract.classList.add("is-changing"));
  }

  function canAttract() {
    return !document.hidden && !body.classList.contains("browser-playing")
      && !document.querySelector("dialog[open]") && slides().length > 0;
  }
  function enterAttract() {
    if (!canAttract() || visible) return;
    visible = true; slide = 0; attract.hidden = false; body.classList.add("v5-attracting");
    renderSlide(); rotation = setInterval(renderSlide, 10000);
  }
  function exitAttract() {
    if (visible) {
      visible = false; attract.hidden = true; body.classList.remove("v5-attracting"); clearInterval(rotation);
    }
    clearTimeout(idleTimer);
    idleTimer = setTimeout(enterAttract, idleMs);
  }
  function activity(event) {
    if (visible && event?.target !== attract.querySelector("button")) event?.preventDefault?.();
    exitAttract();
  }
  for (const name of ["pointerdown", "keydown", "touchstart", "wheel"]) {
    window.addEventListener(name, activity, { capture: true, passive: name !== "keydown" });
  }
  attract.querySelector("button").addEventListener("click", exitAttract);
  document.addEventListener("visibilitychange", () => document.hidden ? exitAttract() : activity());

  // Browsers do not emit ordinary DOM events for controller activity.
  setInterval(() => {
    const pads = [...(navigator.getGamepads?.() || [])].filter(Boolean);
    const state = pads.map(pad => `${pad.index}:${pad.buttons.some(button => button.pressed)}:${pad.axes.some(axis => Math.abs(axis) > .45)}`).join("|");
    if (state !== gamepadState && /:true:|:false:true/.test(state)) activity();
    gamepadState = state;
  }, 500);

  const originalClose = window.FourPlayV4?.closeForPlay;
  const originalRestore = window.FourPlayV4?.restoreBrowse;
  if (window.FourPlayV4) {
    window.FourPlayV4.closeForPlay = (...args) => { exitAttract(); return originalClose?.(...args); };
    window.FourPlayV4.restoreBrowse = (...args) => { const result = originalRestore?.(...args); activity(); return result; };
  }
  exitAttract();
})();
