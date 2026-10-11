const elements = {
  capabilities: document.querySelector("#capabilities"),
  signalingState: document.querySelector("#signaling-state"),
  iceState: document.querySelector("#ice-state"),
  peerState: document.querySelector("#peer-state"),
  remoteTracks: document.querySelector("#remote-tracks"),
  createPeer: document.querySelector("#create-peer"),
  createOffer: document.querySelector("#create-offer"),
  resetPeer: document.querySelector("#reset-peer"),
  localOffer: document.querySelector("#local-offer"),
  copyOffer: document.querySelector("#copy-offer"),
  remoteAnswer: document.querySelector("#remote-answer"),
  applyAnswer: document.querySelector("#apply-answer"),
  remoteVideo: document.querySelector("#remote-video"),
  stats: document.querySelector("#stats"),
  log: document.querySelector("#log"),
};

let peer = null;
let statsTimer = null;
let remoteTrackCount = 0;

elements.createPeer.addEventListener("click", () => {
  resetPeer();
  if (!window.RTCPeerConnection) {
    log("RTCPeerConnection is not available in this browser.");
    return;
  }
  peer = new RTCPeerConnection({
    iceServers: [],
  });
  peer.addTransceiver("video", { direction: "recvonly" });
  peer.addTransceiver("audio", { direction: "recvonly" });
  peer.addEventListener("signalingstatechange", updatePeerState);
  peer.addEventListener("iceconnectionstatechange", updatePeerState);
  peer.addEventListener("connectionstatechange", updatePeerState);
  peer.addEventListener("track", (event) => {
    const [stream] = event.streams;
    if (stream) {
      elements.remoteVideo.srcObject = stream;
      log(`Attached remote ${event.track.kind} track.`);
    }
    remoteTrackCount += 1;
    updatePeerState();
  });
  updatePeerState();
  log("Created local recvonly RTCPeerConnection.");
  log("Next step: create an SDP offer and send it to a test publisher.");
});

elements.createOffer.addEventListener("click", () => createOffer());
elements.resetPeer.addEventListener("click", resetPeer);
elements.copyOffer.addEventListener("click", () => copyOffer());
elements.applyAnswer.addEventListener("click", () => applyAnswer());
elements.remoteAnswer.addEventListener("input", () => {
  elements.applyAnswer.disabled = !peer || !elements.remoteAnswer.value.trim();
});

renderCapabilities();
updatePeerState();
renderStats(new Map());
log("WebRTC diagnostics loaded.");

async function createOffer() {
  if (!peer) {
    log("Create a peer before creating an offer.");
    return;
  }
  try {
    const offer = await peer.createOffer();
    await peer.setLocalDescription(offer);
    await waitForIceGathering(peer);
    elements.localOffer.value = peer.localDescription.sdp;
    elements.copyOffer.disabled = false;
    elements.applyAnswer.disabled = !elements.remoteAnswer.value.trim();
    startStatsLoop();
    log("Created SDP offer. Send this offer to the test publisher.");
  } catch (error) {
    log(`Failed to create offer: ${error.message}`);
  }
}

async function copyOffer() {
  const offer = elements.localOffer.value.trim();
  if (!offer) {
    return;
  }
  try {
    await navigator.clipboard.writeText(offer);
    log("Copied SDP offer to clipboard.");
  } catch (error) {
    log(`Clipboard copy failed; select and copy the offer manually. ${error.message}`);
  }
}

async function applyAnswer() {
  if (!peer) {
    log("Create a peer before applying an answer.");
    return;
  }
  const sdp = elements.remoteAnswer.value.trim();
  if (!sdp) {
    log("Paste an SDP answer before applying it.");
    return;
  }
  try {
    await peer.setRemoteDescription({ type: "answer", sdp });
    startStatsLoop();
    log("Applied remote SDP answer.");
  } catch (error) {
    log(`Failed to apply answer: ${error.message}`);
  }
}

function waitForIceGathering(connection) {
  if (connection.iceGatheringState === "complete") {
    return Promise.resolve();
  }
  return new Promise((resolve) => {
    const deadline = window.setTimeout(done, 2000);
    connection.addEventListener("icegatheringstatechange", handleStateChange);

    function handleStateChange() {
      if (connection.iceGatheringState === "complete") {
        done();
      }
    }

    function done() {
      window.clearTimeout(deadline);
      connection.removeEventListener("icegatheringstatechange", handleStateChange);
      resolve();
    }
  });
}

function renderCapabilities() {
  const capabilities = [
    ["RTCPeerConnection", Boolean(window.RTCPeerConnection)],
    ["RTCDataChannel", Boolean(window.RTCDataChannel)],
    ["MediaStream", Boolean(window.MediaStream)],
    ["HTMLVideoElement.srcObject", "srcObject" in HTMLMediaElement.prototype],
    ["WebCodecs VideoDecoder", Boolean(window.VideoDecoder)],
    ["MediaSource", Boolean(window.MediaSource)],
    ["Secure context", window.isSecureContext],
  ];
  elements.capabilities.replaceChildren(
    ...capabilities.flatMap(([name, available]) => [
      term(name),
      definition(available ? "available" : "missing", available),
    ]),
  );
}

function updatePeerState() {
  elements.signalingState.textContent = peer?.signalingState ?? "not started";
  elements.iceState.textContent = peer?.iceConnectionState ?? "not started";
  elements.peerState.textContent = peer?.connectionState ?? "not started";
  elements.remoteTracks.textContent = remoteTrackCount.toString();
  elements.createOffer.disabled = !peer;
  elements.applyAnswer.disabled = !peer || !elements.remoteAnswer.value.trim();
}

function resetPeer() {
  stopStatsLoop();
  if (peer) {
    peer.close();
    peer = null;
    log("Closed peer connection.");
  }
  remoteTrackCount = 0;
  elements.localOffer.value = "";
  elements.remoteAnswer.value = "";
  elements.copyOffer.disabled = true;
  elements.applyAnswer.disabled = true;
  elements.remoteVideo.srcObject = null;
  renderStats(new Map());
  updatePeerState();
}

function startStatsLoop() {
  if (statsTimer || !peer) {
    return;
  }
  statsTimer = window.setInterval(updateStats, 1000);
  updateStats();
}

function stopStatsLoop() {
  if (statsTimer) {
    window.clearInterval(statsTimer);
    statsTimer = null;
  }
}

async function updateStats() {
  if (!peer) {
    return;
  }
  try {
    renderStats(await peer.getStats());
  } catch (error) {
    log(`Stats update failed: ${error.message}`);
  }
}

function renderStats(report) {
  const inbound = Array.from(report.values()).filter((item) => item.type === "inbound-rtp");
  const candidatePair = Array.from(report.values()).find(
    (item) => item.type === "candidate-pair" && item.state === "succeeded",
  );
  const selected = [
    ["Video frames decoded", sum(inbound, "framesDecoded")],
    ["Video frames dropped", sum(inbound, "framesDropped")],
    ["Audio samples decoded", sum(inbound, "totalSamplesDecoded")],
    ["Packets lost", sum(inbound, "packetsLost")],
    ["Jitter seconds", formatMax(inbound, "jitter", 3)],
    ["Current RTT seconds", formatNumber(candidatePair?.currentRoundTripTime, 3)],
    ["Bytes received", sum(inbound, "bytesReceived")],
  ];
  elements.stats.replaceChildren(
    ...selected.flatMap(([name, value]) => [term(name), definition(String(value ?? "n/a"), true)]),
  );
}

function sum(items, property) {
  const value = items.reduce((total, item) => total + (Number(item[property]) || 0), 0);
  return value || "n/a";
}

function formatMax(items, property, decimals) {
  const values = items.map((item) => Number(item[property])).filter(Number.isFinite);
  if (!values.length) {
    return "n/a";
  }
  return Math.max(...values).toFixed(decimals);
}

function formatNumber(value, decimals) {
  return Number.isFinite(value) ? Number(value).toFixed(decimals) : "n/a";
}

function term(text) {
  const element = document.createElement("dt");
  element.textContent = text;
  return element;
}

function definition(text, available) {
  const element = document.createElement("dd");
  element.textContent = text;
  element.className = available ? "ok" : "missing";
  return element;
}

function log(message) {
  const timestamp = new Date().toLocaleTimeString();
  elements.log.textContent += `[${timestamp}] ${message}\n`;
  elements.log.scrollTop = elements.log.scrollHeight;
}
