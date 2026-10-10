const params = new URLSearchParams(window.location.search);

const game = params.get("game") || "Production Feed";
const session = params.get("session") || "";
const port = params.get("port") || "";
const grant = params.get("grant") || "";

const receiverUrl = port
  ? `udp://0.0.0.0:${port}?fifo_size=1000000&overrun_nonfatal=1`
  : "Waiting for media port...";

document.querySelector("#capture-title").textContent = game;
document.querySelector("#capture-session").textContent = session
  ? `Session ${session}`
  : "No session selected.";
document.querySelector("#capture-url").textContent = receiverUrl;
document.querySelector("#capture-port").textContent = port ? `media ${port}` : "media unknown";
document.querySelector("#capture-grant").textContent = grant ? `grant ${grant}` : "grant unknown";
