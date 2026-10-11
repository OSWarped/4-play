const elements = {
  capabilities: document.querySelector("#capabilities"),
  signalingState: document.querySelector("#signaling-state"),
  iceState: document.querySelector("#ice-state"),
  peerState: document.querySelector("#peer-state"),
  createPeer: document.querySelector("#create-peer"),
  resetPeer: document.querySelector("#reset-peer"),
  remoteVideo: document.querySelector("#remote-video"),
  log: document.querySelector("#log"),
};

let peer = null;

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
  });
  updatePeerState();
  log("Created local recvonly RTCPeerConnection.");
  log("Next step: wire this peer to a runtime-host WebRTC signaling endpoint.");
});

elements.resetPeer.addEventListener("click", resetPeer);

renderCapabilities();
updatePeerState();
log("WebRTC diagnostics loaded.");

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
}

function resetPeer() {
  if (peer) {
    peer.close();
    peer = null;
    log("Closed peer connection.");
  }
  elements.remoteVideo.srcObject = null;
  updatePeerState();
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
