/* V5: a distinct presentation and idle attract mode over the proven V2-V4 stack. */
(() => {
  const body = document.body;
  if (body.dataset.clientGeneration !== "5") return;
  const controls = document.querySelector("#touch-controls");
  if (controls) controls.dataset.touchLayout = "floating";

  const logoLoaders = new Map();
  const logoQueue = [];
  let activeLogoLoads = 0;
  const games = new Map();
  function pumpLogoQueue() {
    while (activeLogoLoads < 2 && logoQueue.length) {
      const task = logoQueue.shift();
      if (!task.card.isConnected || logoLoaders.get(task.card) !== task.loader) continue;
      activeLogoLoads += 1;
      task.loader.load(task.game, window.FourPlayV2.headers, url => {
        if (!url || logoLoaders.get(task.card) !== task.loader) return;
        task.logo.onload = () => {
          task.logo.hidden = false;
          task.card.classList.add("has-game-logo");
        };
        task.logo.onerror = () => {
          task.logo.hidden = true;
          task.card.classList.remove("has-game-logo");
        };
        task.logo.src = url;
      }, "logo").finally(() => {
        activeLogoLoads -= 1;
        pumpLogoQueue();
      });
    }
  }
  function addGameLogos(selection, cards) {
    const connected = new Set(cards);
    for (const [card, loader] of logoLoaders) {
      if (!connected.has(card)) {
        loader.clear();
        logoLoaders.delete(card);
        card.querySelector(".v5-game-logo")?.remove();
        card.classList.remove("has-game-logo");
      }
    }
    cards.forEach((card, index) => {
      const game = selection.games[index];
      if (game) games.set(game.id, game);
      if (!game?.metadata?.logo_path || logoLoaders.has(card)) return;
      const logo = document.createElement("img");
      logo.className = "v5-game-logo"; logo.alt = ""; logo.hidden = true;
      card.append(logo);
      const loader = new window.FourPlayCarousel.ArtworkLoader(window.FourPlayV2.artworkEnvironment);
      logoLoaders.set(card, loader);
      logoQueue.push({ card, game, loader, logo });
    });
    pumpLogoQueue();
  }
  if (window.FourPlayV2) {
    const originalUpdateCovers = window.FourPlayV2.updateCovers;
    window.FourPlayV2.updateCovers = (selection, cards) => {
      const result = originalUpdateCovers?.(selection, cards);
      addGameLogos(selection, cards);
      return result;
    };
  }

  const toolbar = document.querySelector(".v4-toolbar");
  const brand = document.createElement("div");
  brand.className = "v5-brand";
  brand.innerHTML = '<img src="/client-v5/app-logo.png" alt="4-Play" />';
  toolbar?.prepend(brand);

  const attract = document.createElement("section");
  attract.className = "v5-attract";
  attract.hidden = true;
  attract.setAttribute("aria-label", "4-Play attract mode");
  attract.innerHTML = `
    <img class="v5-attract-art" alt="" />
    <div class="v5-attract-wash"></div>
    <div class="v5-attract-copy">
      <img class="v5-attract-game-logo" alt="" hidden />
      <h1>4-Play Arcade</h1>
      <p class="v5-attract-meta"></p>
    </div>
    <div class="v5-attract-progress" aria-hidden="true"></div>`;
  body.append(attract);

  const art = attract.querySelector(".v5-attract-art");
  const title = attract.querySelector("h1");
  const gameLogo = attract.querySelector(".v5-attract-game-logo");
  const meta = attract.querySelector(".v5-attract-meta");
  let idleTimer = null, rotation = null, slide = 0, visible = false, gamepadState = "";
  const idleMs = new URLSearchParams(location.search).get("attract") === "1" ? 1500 : 45000;

  function slides() {
    return [...document.querySelectorAll("#games .game-cover")].map(card => {
      const game = games.get(card.dataset.gameId);
      return {
        title: card.querySelector(".cover-name")?.textContent?.trim() || "4-Play Arcade",
        image: card.querySelector(".cover-art:not([hidden])")?.src || "",
        logo: card.querySelector(".v5-game-logo:not([hidden])")?.src || "",
        live: card.dataset.live === "true",
        description: game?.metadata?.description?.trim() || "",
      };
    }).sort((a, b) => Number(b.live) - Number(a.live));
  }

  function renderSlide() {
    const available = slides();
    if (!available.length) return exitAttract();
    const value = available[slide++ % available.length];
    art.hidden = !value.image;
    if (value.image) art.src = value.image;
    title.textContent = value.title;
    title.hidden = Boolean(value.logo);
    gameLogo.hidden = !value.logo;
    gameLogo.alt = value.logo ? value.title : "";
    if (value.logo) gameLogo.src = value.logo;
    meta.textContent = value.description;
    meta.hidden = !value.description;
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
    if (visible) event?.preventDefault?.();
    exitAttract();
  }
  for (const name of ["pointerdown", "keydown", "touchstart", "wheel"]) {
    window.addEventListener(name, activity, { capture: true, passive: name !== "keydown" });
  }
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
