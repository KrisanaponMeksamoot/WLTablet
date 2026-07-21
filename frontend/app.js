const inputArea = document.getElementById("inputArea");
const overlay = document.getElementById("overlay");
const statusEl = document.getElementById("status");
const pinForm = document.getElementById("pinForm");
const pinInput = document.getElementById("pinInput");

const settingsOverlay = document.getElementById("settingsOverlay");
const settingsButton = document.getElementById("settingsButton");
const closeSettingsButton = document.getElementById("closeSettingsButton");
const settingsForm = document.getElementById("settingsForm");
const canvasWidthInput = document.getElementById("canvasWidth");
const canvasHeightInput = document.getElementById("canvasHeight");
const preserveAspectCheckbox = document.getElementById("preserveAspect");
const rememberPinCheckbox = document.getElementById("rememberPin");
const autoConnectCheckbox = document.getElementById("autoConnect");

const STORAGE_KEY = "wltablet-settings";
const PIN_STORAGE_KEY = "wltablet-last-pin";

class Settings {
  constructor(options = {}) {
    this.canvasWidth = options.canvasWidth ?? screen.availWidth;
    this.canvasHeight = options.canvasHeight ?? screen.availHeight;
    this.preserveAspectRatio = options.preserveAspectRatio ?? preserveAspectCheckbox.checked;
    this.rememberPin = options.rememberPin ?? rememberPinCheckbox.checked;
    this.autoConnect = options.autoConnect ?? autoConnectCheckbox.checked;
  }

  sync() {
    this.canvasWidth = canvasWidthInput.valueAsNumber;
    this.canvasHeight = canvasHeightInput.valueAsNumber;

    this.preserveAspectRatio = preserveAspectCheckbox.checked;
    this.rememberPin = rememberPinCheckbox.checked;
    this.autoConnect = autoConnectCheckbox.checked;
  }
  
  save() {
    this.sync();
    localStorage.setItem(STORAGE_KEY, JSON.stringify(this));
  }

  apply() {
    canvasWidthInput.value = this.canvasWidth;
    canvasHeightInput.value = this.canvasHeight;

    inputArea.style.width = inputArea.width = this.canvasWidth;
    inputArea.style.height = inputArea.height = this.canvasHeight;

    preserveAspectCheckbox.checked = this.preserveAspectRatio;

    rememberPinCheckbox.checked = this.rememberPin;
    autoConnectCheckbox.checked = this.autoConnect;

    if (this.rememberPin) {
      const savedPin = localStorage.getItem(PIN_STORAGE_KEY) || "";
      pinInput.value = savedPin;
    }

    if (this.rememberPin && this.autoConnect && !connection_active) {
      const savedPin = pinInput.value;
      if (/^\d{6}$/.test(savedPin)) {
        setStatus("Auto connecting...");
        startSession(savedPin);
      }
    }
  }

  static load() {
    try {
      return new Settings(JSON.parse(localStorage.getItem(STORAGE_KEY) || "{}"));
    } catch (error) {
      console.error("Could not parse settings", error);
    }
    return new Settings();
  }
}

let settings = Settings.load();

let pc = null;
let dataChannel = null;
let socket = null;
let connection_active = false;
let hostScreenWidth = 1920;
let hostScreenHeight = 1080;

function setStatus(message) {
  statusEl.textContent = message;
}

function savePinPreference(pin) {
  if (settings.rememberPin) {
    localStorage.setItem(PIN_STORAGE_KEY, pin);
  } else {
    localStorage.removeItem(PIN_STORAGE_KEY);
  }
}

function syncCanvasSizeInputs(changedId) {
  if (!preserveAspectCheckbox.checked) {
    return;
  }

  let targetAspectRatio = hostScreenWidth / hostScreenHeight;
  
  if (changedId === "canvasWidth") {
    const nextHeight = Math.round(canvasWidthInput.valueAsNumber / targetAspectRatio);
    canvasHeightInput.value = nextHeight;
  } else {
    const nextWidth = Math.round(canvasHeightInput.valueAsNumber * targetAspectRatio);
    canvasWidthInput.value = nextWidth;
  }
}

function openSettingsPanel() {
  settingsOverlay.hidden = false;
  settingsButton.setAttribute("aria-expanded", "true");
}

function closeSettingsPanel() {
  settingsOverlay.hidden = true;
  settingsButton.setAttribute("aria-expanded", "false");
}

settingsButton.addEventListener("click", openSettingsPanel);
closeSettingsButton.addEventListener("click", closeSettingsPanel);
settingsOverlay.addEventListener("click", (event) => {
  if (event.target === settingsOverlay) {
    closeSettingsPanel();
  }
});

document.getElementById("resetSetting").addEventListener("click", () => {
  settings = new Settings();
  settings.apply();
});

canvasWidthInput.addEventListener("input", () => {
  syncCanvasSizeInputs("canvasWidth");
  settings.sync();
  settings.apply();
});

canvasHeightInput.addEventListener("input", () => {
  syncCanvasSizeInputs("canvasHeight");
  settings.sync();
  settings.apply();
});

preserveAspectCheckbox.addEventListener("change", () => {
  syncCanvasSizeInputs("canvasHeight");

  settings.sync();
  settings.apply();
});

settingsForm.addEventListener("submit", (event) => {
  event.preventDefault();

  settings.sync();
  settings.save();

  const pin = pinInput.value.trim();
  savePinPreference(pin);

  closeSettingsPanel();

  if (settings.autoConnect && /^\d{6}$/.test(pin)) {
    setStatus("Auto connecting...");
    startSession(pin);
  } else {
    setStatus("Settings saved.");
  }
});

pinForm.addEventListener("submit", async (event) => {
  event.preventDefault();

  const pin = pinInput.value.trim();
  if (!/^\d{6}$/.test(pin)) {
    setStatus("Enter a valid 6-digit PIN.");
    return;
  }

  savePinPreference(pin);
  setStatus("Connecting...");
  await startSession(pin);
});

async function startSession(pin) {
  if (connection_active) return;
  const protocol = location.protocol === "https:" ? "wss" : "ws";
  const wsUrl = `${protocol}://${location.host}/ws`;

  socket = new WebSocket(wsUrl);
  socket.binaryType = "arraybuffer";
  connection_active = true;

  socket.addEventListener("open", () => {
    socket.send(pin);
    setStatus("Waiting for pairing approval...");
  });

  socket.addEventListener("message", async (event) => {
    if (typeof event.data === "string") {
      try {
        const payload = JSON.parse(event.data);

        if (payload.type === "screen-dimensions") {
          hostScreenWidth = payload.width;
          hostScreenHeight = payload.height;
          setStatus(`Server resolution: ${hostScreenWidth}x${hostScreenHeight}`);
          return;
        }

        if (payload.type === "pin-ok") {
          setStatus("PIN accepted, creating WebRTC handshake...");
          await buildPeerConnection();
          return;
        }

        if (payload.type === "answer" && pc) {
          await pc.setRemoteDescription({
            type: "answer",
            sdp: payload.sdp,
          });
          setStatus("Connected, ready to stream.");
          return;
        }

        if (payload.type === "candidate" && pc) {
          await pc.addIceCandidate(payload.candidate);
        }
      } catch (error) {
        console.error("Invalid server message", error);
      }
    }
  });

  socket.addEventListener("close", (ev) => {
    connection_active = false;
    setStatus("Connection closed.");
    if (ev.code >= 4000) {
      setStatus(ev.reason);
    }
    close();
  });

  socket.addEventListener("error", () => {
    connection_active = false;
    setStatus("WebSocket error.");
    close();
  });
}

function close() {
  overlay.style.removeProperty("display");
}

async function buildPeerConnection() {
  pc = new RTCPeerConnection({ iceServers: [] });

  pc.onicecandidate = (event) => {
    if (event.candidate && socket && socket.readyState === WebSocket.OPEN) {
      socket.send(
        JSON.stringify({
          type: "candidate",
          candidate: event.candidate,
        })
      );
    }
  };

  dataChannel = pc.createDataChannel("pointer", {
    ordered: false,
    maxRetransmits: 0,
  });

  dataChannel.onopen = () => {
    overlay.style.display = "none";
    setStatus("Pointer channel open. Draw now.");
  };

  dataChannel.onclose = () => {
    setStatus("Pointer channel closed.");
  };

  pc.ondatachannel = (event) => {
    event.channel.onopen = () => {
      overlay.style.display = "none";
      setStatus("Pointer channel open. Draw now.");
    };
  };

  const offer = await pc.createOffer();
  await pc.setLocalDescription(offer);
  socket.send(JSON.stringify({ type: "offer", sdp: offer.sdp }));
}

inputArea.addEventListener("pointermove", processPointerData);
inputArea.addEventListener("pointerdown", processPointerData);
inputArea.addEventListener("pointerup", processPointerData);
inputArea.addEventListener("pointercancel", processPointerData);

function processPointerData(event) {
  event.preventDefault();
  let {clientX, clientY, pressure} = event;

  let bound = inputArea.getBoundingClientRect();

  const xRatio = Math.max(0, Math.min((clientX - bound.left) / inputArea.width, 1.0));
  const yRatio = Math.max(0, Math.min((clientY - bound.top) / inputArea.height, 1.0));

  sendPointerData(xRatio, yRatio, pressure);
}

function sendPointerData(xRatio, yRatio, pressure) {
  if (!connection_active || !dataChannel || dataChannel.readyState !== "open") {
    return;
  }

  const buffer = new ArrayBuffer(12);
  const view = new DataView(buffer);
  view.setFloat32(0, xRatio, true);
  view.setFloat32(4, yRatio, true);
  view.setFloat32(8, pressure, true);

  dataChannel.send(buffer);
}

settings.apply();
