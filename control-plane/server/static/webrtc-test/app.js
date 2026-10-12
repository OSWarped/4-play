const elements = {
  capabilities: document.querySelector("#capabilities"),
  signalingState: document.querySelector("#signaling-state"),
  iceGatheringState: document.querySelector("#ice-gathering-state"),
  iceState: document.querySelector("#ice-state"),
  peerState: document.querySelector("#peer-state"),
  remoteTracks: document.querySelector("#remote-tracks"),
  includeAudio: document.querySelector("#include-audio"),
  createPeer: document.querySelector("#create-peer"),
  createOffer: document.querySelector("#create-offer"),
  resetPeer: document.querySelector("#reset-peer"),
  localOffer: document.querySelector("#local-offer"),
  copyOffer: document.querySelector("#copy-offer"),
  remoteAnswer: document.querySelector("#remote-answer"),
  seatId: document.querySelector("#seat-id"),
  apiToken: document.querySelector("#api-token"),
  publisherUrl: document.querySelector("#publisher-url"),
  signalingMode: document.querySelector("#signaling-mode"),
  useControlPlane: document.querySelector("#use-control-plane"),
  useDirectPublisher: document.querySelector("#use-direct-publisher"),
  signalPublisher: document.querySelector("#signal-publisher"),
  applyAnswer: document.querySelector("#apply-answer"),
  refreshStatus: document.querySelector("#refresh-status"),
  controlPlaneStatus: document.querySelector("#control-plane-status"),
  remoteVideo: document.querySelector("#remote-video"),
  stats: document.querySelector("#stats"),
  log: document.querySelector("#log"),
};

let peer = null;
let statsTimer = null;
let remoteTrackCount = 0;

const storedToken = window.localStorage.getItem("fourplay.webrtcTestApiToken");
if (storedToken) {
  elements.apiToken.value = storedToken;
}
elements.apiToken.addEventListener("change", () => {
  const token = elements.apiToken.value.trim();
  if (token) {
    window.localStorage.setItem("fourplay.webrtcTestApiToken", token);
  } else {
    window.localStorage.removeItem("fourplay.webrtcTestApiToken");
  }
});

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
  if (elements.includeAudio.checked) {
    peer.addTransceiver("audio", { direction: "recvonly" });
  }
  peer.addEventListener("signalingstatechange", updatePeerState);
  peer.addEventListener("icegatheringstatechange", updatePeerState);
  peer.addEventListener("iceconnectionstatechange", updatePeerState);
  peer.addEventListener("connectionstatechange", updatePeerState);
  peer.addEventListener("icecandidateerror", (event) => {
    log(`ICE candidate error url=${event.url || "n/a"} code=${event.errorCode} text=${event.errorText}`);
  });
  peer.addEventListener("icecandidate", (event) => {
    if (event.candidate) {
      log(`Local ICE candidate gathered: ${event.candidate.type}/${event.candidate.protocol}`);
    } else {
      log("Local ICE gathering complete.");
    }
  });
  peer.addEventListener("track", (event) => {
    const [stream] = event.streams;
    if (stream) {
      elements.remoteVideo.srcObject = stream;
      elements.remoteVideo
        .play()
        .catch((error) => log(`Remote media play() did not start automatically: ${error.message}`));
      log(`Attached remote ${event.track.kind} track.`);
    }
    event.track.addEventListener("mute", () => log(`Remote ${event.track.kind} track muted.`));
    event.track.addEventListener("unmute", () => log(`Remote ${event.track.kind} track unmuted.`));
    event.track.addEventListener("ended", () => log(`Remote ${event.track.kind} track ended.`));
    remoteTrackCount += 1;
    updatePeerState();
  });
  updatePeerState();
  log(
    `Created local recvonly RTCPeerConnection with ${
      elements.includeAudio.checked ? "video + audio" : "video only"
    }.`,
  );
  log("Next step: create an SDP offer and send it to a test publisher.");
});

elements.createOffer.addEventListener("click", () => createOffer());
elements.resetPeer.addEventListener("click", resetPeer);
elements.copyOffer.addEventListener("click", () => copyOffer());
elements.signalPublisher.addEventListener("click", () => signalPublisher());
elements.applyAnswer.addEventListener("click", () => applyAnswer());
elements.refreshStatus.addEventListener("click", () => loadControlPlaneStatus());
elements.remoteAnswer.addEventListener("input", () => {
  elements.applyAnswer.disabled = !peer || !elements.remoteAnswer.value.trim();
});
elements.publisherUrl.addEventListener("input", updateSignalingMode);
elements.useControlPlane.addEventListener("click", () => {
  elements.publisherUrl.value = controlPlanePublisherUrl();
  updateSignalingMode();
});
elements.useDirectPublisher.addEventListener("click", () => {
  elements.publisherUrl.value = directPublisherUrl();
  updateSignalingMode();
});

elements.publisherUrl.value = controlPlanePublisherUrl();
renderCapabilities();
renderControlPlaneStatus(null);
updatePeerState();
renderStats(new Map());
updateSignalingMode();
loadControlPlaneStatus();
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
    elements.signalPublisher.disabled = false;
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

async function signalPublisher() {
  if (!peer || !elements.localOffer.value.trim()) {
    log("Create an SDP offer before signaling the publisher.");
    return;
  }
  const url = elements.publisherUrl.value.trim();
  if (!url) {
    log("Enter the publisher /offer URL before signaling.");
    return;
  }
  try {
    log(`Sending offer to ${url}`);
    const useControlPlane = isControlPlanePublisherUrl(url);
    const response = useControlPlane
      ? await signalControlPlanePublisher(url)
      : await signalRawPublisher(url);
    const responseText = await response.text();
    if (!response.ok) {
      throw new Error(responseText || `HTTP ${response.status}`);
    }
    const answer = useControlPlane ? parseControlPlaneAnswer(responseText) : responseText;
    elements.remoteAnswer.value = answer;
    log("Received SDP answer from publisher.");
    await applyAnswer();
  } catch (error) {
    log(`Publisher signaling failed: ${error.message}`);
  }
}

async function signalRawPublisher(url) {
  return fetch(url, {
    method: "POST",
    mode: "cors",
    headers: {
      "content-type": "application/sdp; charset=utf-8",
    },
    body: elements.localOffer.value,
  });
}

async function signalControlPlanePublisher(url) {
  const token = elements.apiToken.value.trim();
  const seatId = elements.seatId.value.trim() || "webrtc-test-seat";
  if (!token) {
    throw new Error("Control-plane signaling requires an API token.");
  }
  window.localStorage.setItem("fourplay.webrtcTestApiToken", token);
  return fetch(url, {
    method: "POST",
    headers: {
      authorization: `Bearer ${token}`,
      "content-type": "application/json",
    },
    body: JSON.stringify({
      seat_id: seatId,
      role: "spectator",
      sdp_offer: elements.localOffer.value,
    }),
  });
}

function parseControlPlaneAnswer(text) {
  const payload = JSON.parse(text);
  if (!payload.sdp_answer) {
    throw new Error("Control-plane response did not include sdp_answer.");
  }
  log(`Control-plane returned ${payload.media_transport || "unknown"} answer for ${payload.session_id || "unknown"}.`);
  return payload.sdp_answer;
}

function controlPlanePublisherUrl() {
  return `${location.origin}/api/v1/experimental/webrtc/test-pattern/offer`;
}

function controlPlaneStatusUrl() {
  return `${location.origin}/api/v1/experimental/webrtc/status`;
}

function directPublisherUrl() {
  return `http://${location.hostname}:18080/offer`;
}

function isControlPlanePublisherUrl(url) {
  return url.includes("/api/v1/experimental/webrtc/");
}

function updateSignalingMode() {
  const url = elements.publisherUrl.value.trim();
  if (isControlPlanePublisherUrl(url)) {
    elements.signalingMode.textContent =
      "Mode: authenticated control-plane proxy. Requires the seat API token and the local publisher service.";
  } else {
    elements.signalingMode.textContent =
      "Mode: direct publisher. Sends raw SDP to the publisher URL and bypasses the control plane.";
  }
}

async function loadControlPlaneStatus() {
  const token = elements.apiToken.value.trim();
  if (!token) {
    renderControlPlaneStatus({
      unavailable: "enter a seat API token to inspect control-plane WebRTC readiness",
    });
    return;
  }
  window.localStorage.setItem("fourplay.webrtcTestApiToken", token);
  try {
    const response = await fetch(controlPlaneStatusUrl(), {
      headers: {
        authorization: `Bearer ${token}`,
      },
    });
    const text = await response.text();
    if (!response.ok) {
      throw new Error(text || `HTTP ${response.status}`);
    }
    const status = JSON.parse(text);
    renderControlPlaneStatus(status);
    log("Loaded control-plane WebRTC readiness status.");
  } catch (error) {
    renderControlPlaneStatus({ unavailable: error.message });
    log(`Control-plane WebRTC readiness failed: ${error.message}`);
  }
}

function renderControlPlaneStatus(status) {
  const values = status
    ? [
        ["Test-pattern proxy", status.test_pattern_proxy_configured ? "configured" : "not configured"],
        ["Test-pattern offer path", status.test_pattern_offer_path || "n/a"],
        [
          "Session offer endpoint",
          status.session_offer_endpoint_available ? "available" : "unavailable",
        ],
        ["Session offer path", status.session_offer_path_template || "n/a"],
        [
          "Session publisher",
          status.session_publisher_available ? "available" : "not available yet",
        ],
        status.unavailable ? ["Status detail", status.unavailable] : null,
      ].filter(Boolean)
    : [["Status", "not loaded"]];
  elements.controlPlaneStatus.replaceChildren(
    ...values.flatMap(([name, value]) => [
      term(name),
      definition(String(value), !String(value).includes("not configured") && !String(value).includes("not available") && !String(value).includes("unavailable")),
    ]),
  );
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
    await peer.setRemoteDescription({ type: "answer", sdp: sanitizeSdp(sdp) });
    startStatsLoop();
    log("Applied remote SDP answer.");
  } catch (error) {
    log(`Failed to apply answer: ${error.message}`);
  }
}

function sanitizeSdp(text) {
  const lines = text
    .replace(/\u2010|\u2011|\u2012|\u2013|\u2014|\u2212/g, "-")
    .replace(/\u200b|\u200c|\u200d|\ufeff/g, "")
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line && !line.startsWith("-----BEGIN") && !line.startsWith("-----END"))
    .map((line) =>
      line.replace(
        /^a=fingerprint:([A-Za-z0-9-]+)\s+([0-9A-Fa-f:]+)$/,
        (_match, algorithm, fingerprint) =>
          `a=fingerprint:${algorithm.toLowerCase()} ${fingerprint.toLowerCase()}`,
      ),
    );
  return `${lines.join("\r\n")}\r\n`;
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
  elements.iceGatheringState.textContent = peer?.iceGatheringState ?? "not started";
  elements.iceState.textContent = peer?.iceConnectionState ?? "not started";
  elements.peerState.textContent = peer?.connectionState ?? "not started";
  elements.remoteTracks.textContent = remoteTrackCount.toString();
  elements.createOffer.disabled = !peer;
  elements.signalPublisher.disabled = !peer || !elements.localOffer.value.trim();
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
  elements.signalPublisher.disabled = true;
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
  const values = Array.from(report.values());
  const inbound = values.filter((item) => item.type === "inbound-rtp");
  const candidatePair = Array.from(report.values()).find(
    (item) => item.type === "candidate-pair" && item.state === "succeeded",
  );
  const anyCandidatePair = values.find((item) => item.type === "candidate-pair");
  const selectedPair = candidatePair ?? anyCandidatePair;
  const selected = [
    ["Inbound RTP reports", inbound.length || "n/a"],
    ["Candidate pairs", values.filter((item) => item.type === "candidate-pair").length || "n/a"],
    ["Best candidate state", selectedPair?.state ?? "n/a"],
    ["Best candidate nominated", selectedPair?.nominated ?? "n/a"],
    ["Video frames decoded", sum(inbound, "framesDecoded")],
    ["Video frames dropped", sum(inbound, "framesDropped")],
    ["Audio samples decoded", sum(inbound, "totalSamplesDecoded")],
    ["Packets lost", sum(inbound, "packetsLost")],
    ["Jitter seconds", formatMax(inbound, "jitter", 3)],
    ["Current RTT seconds", formatNumber(selectedPair?.currentRoundTripTime, 3)],
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
