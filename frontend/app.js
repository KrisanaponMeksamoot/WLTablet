const appDiv = document.getElementById("app");
const inputArea = document.getElementById("inputArea");
const overlay = document.getElementById("overlay");
const statusEl = document.getElementById("status");
const pinForm = document.getElementById("pinForm");
const pinInput = document.getElementById("pinInput");
const statusDbg = document.getElementById("statusDebug");

const settingsOverlay = document.getElementById("settingsOverlay");
const settingsButton = document.getElementById("settingsButton");
const canvasWidthInput = document.getElementById("canvasWidth");
const canvasHeightInput = document.getElementById("canvasHeight");
const preserveAspectCheckbox = document.getElementById("preserveAspect");
const rememberPinCheckbox = document.getElementById("rememberPin");
const autoConnectCheckbox = document.getElementById("autoConnect");
const enableWebrtcCheckbox = document.getElementById("enableWebrtc");

const SETTING_KEY = "wltablet-settings";
const PIN_STORAGE_KEY = "wltablet-last-pin";

function bytesToBase64(bytes) {
  const binString = Array.from(bytes, (byte) =>
    String.fromCodePoint(byte),
  ).join("");
  return btoa(binString);
}

function base64ToBytes(base64) {
  const binString = Array.from(atob(base64), (char) =>
    char.codePointAt(0),
  );
  return new Uint8Array(binString);
}

class Settings {
  constructor(options = {}) {
    this.canvasWidth = options.canvasWidth ?? screen.availWidth;
    this.canvasHeight = options.canvasHeight ?? screen.availHeight;
    this.preserveAspectRatio = options.preserveAspectRatio ?? preserveAspectCheckbox.checked;
    this.rememberPin = options.rememberPin ?? rememberPinCheckbox.checked;
    this.autoConnect = options.autoConnect ?? autoConnectCheckbox.checked;
    this.enableWebrtc = options.enableWebrtc ?? enableWebrtcCheckbox.checked;
    this.canvasBackground = options.canvasBackground ?? null;
    this._canvasBackgroundBlob = null;
    this._canvasBackgroundUrl = null;

    if (this.canvasBackground)
      try {
        let {data, type} = this.canvasBackground;
        this._canvasBackgroundBlob = new Blob([base64ToBytes(data)], { type });
        this._canvasBackgroundUrl = URL.createObjectURL(this._canvasBackgroundBlob);
      } catch (e) {
        console.error(e);
      }
  }

  async setCanvasBackground(blob) {
    this._canvasBackgroundBlob = blob;
    if (blob) {
      this.canvasBackground = { data: bytesToBase64(await blob.bytes()), type: blob.type };
      this._canvasBackgroundUrl = URL.createObjectURL(this._canvasBackgroundBlob);
      appDiv.style.backgroundImage = `url("${this._canvasBackgroundUrl}")`;
    } else {
      this.canvasBackground = null;
      this._canvasBackgroundUrl = null;
      appDiv.style.removeProperty("background-image");
    }
  }

  sync() {
    this.canvasWidth = canvasWidthInput.valueAsNumber;
    this.canvasHeight = canvasHeightInput.valueAsNumber;

    this.preserveAspectRatio = preserveAspectCheckbox.checked;
    this.rememberPin = rememberPinCheckbox.checked;
    this.autoConnect = autoConnectCheckbox.checked;
    this.enableWebrtc = enableWebrtcCheckbox.checked;
  }
  
  save() {
    this.sync();
    localStorage.setItem(SETTING_KEY, JSON.stringify(this, (k, v) => {
      if (k.startsWith("_")) return undefined;
      return v;
    }));
  }

  apply() {
    canvasWidthInput.value = this.canvasWidth;
    canvasHeightInput.value = this.canvasHeight;

    inputArea.style.width = inputArea.width = this.canvasWidth;
    inputArea.style.height = inputArea.height = this.canvasHeight;

    preserveAspectCheckbox.checked = this.preserveAspectRatio;

    rememberPinCheckbox.checked = this.rememberPin;
    autoConnectCheckbox.checked = this.autoConnect;
    enableWebrtcCheckbox.checked = this.enableWebrtc;

    this.setCanvasBackground(this._canvasBackgroundBlob);

    if (this.rememberPin) {
      const savedPin = localStorage.getItem(PIN_STORAGE_KEY) || "";
      pinInput.value = savedPin;
    }

    if (this.rememberPin && this.autoConnect && !connection?.is_active()) {
      const savedPin = pinInput.value;
      if (/^\d{6}$/.test(savedPin)) {
        setStatus("Auto connecting...");
        startSession(savedPin);
      }
    }
  }

  static load() {
    try {
      return new Settings(JSON.parse(localStorage.getItem(SETTING_KEY) || "{}"));
    } catch (error) {
      console.error("Could not parse settings", error);
    }
    return new Settings();
  }
}

let settings = Settings.load();

let pc = null;
let socket = null;
let hostScreenWidth = 1920;
let hostScreenHeight = 1080;

function setStatus(message) {
  statusEl.textContent = message;
  updateDebugStatus();
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

let connection = { is_active:()=>false, send(_){} };

async function startSession(pin) {
  if (connection.is_active()) return;
  let wsUrl = new URL("./ws", location);
  wsUrl.protocol = location.protocol === "https:" ? "wss" : "ws";

  socket = new WebSocket(wsUrl);
  socket.binaryType = "arraybuffer";
  connection = {
    socket,
    is_active() {
      return socket.readyState === WebSocket.OPEN;
    },
    send(data) {
      socket.send(data);
    }
  }
  ping_stat.reset();

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
          setStatus(settings.enableWebrtc ?
              "PIN accepted, creating WebRTC handshake..." :
              "PIN accepted. Draw now.");
          overlay.style.display = "none";
          if (settings.enableWebrtc) await buildPeerConnection();
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
    } else {
      processIncomingPacket(event.data);
    }
  });

  socket.addEventListener("close", (ev) => {
    setStatus("Connection closed.");
    if (ev.code >= 4000) {
      setStatus(ev.reason);
    }
    close();
  });

  socket.addEventListener("error", () => {
    setStatus("WebSocket error.");
    close();
  });
}

function close() {
  overlay.style.removeProperty("display");
}

let ping_stat = {
  time: NaN,
  received: 0,
  lost: 0,

  last_ping: [],

  _seq_c: 0,

  send_ping() {
    if (!connection?.is_active()) return;
    
    this._seq_c++;
    this.last_ping.push([this._seq_c, Date.now()]);
    if (this.last_ping.length > 0)
      this.time = Date.now() - this.last_ping[0][1];

    if (this.last_ping.length > 10) {
      this.last_ping.shift();
      this.lost++;
    }

    const buffer = new ArrayBuffer(6);
    const view = new DataView(buffer);
    view.setUint16(0, 0, true);
    view.setUint32(2, this._seq_c, true);

    connection.send(buffer);
  },

  reset() {
    this.time = NaN;
    this.received = 0;
    this.lost = 0;
    this.last_ping = [];
  }
};

function processIncomingPacket(data) {
  const view = new DataView(data);
  const id = view.getUint16(0, true);
  if (id == 0) {
    view.setUint16(0, 8);
    connection.send(view.buffer);
  } else if (id == 8) {
    let seq = view.getUint32(2, true);
    let i = ping_stat.last_ping.findIndex(v => v[0]==seq);
    if (i>=0) {
      ping_stat.lost += i;
      while (i--) ping_stat.last_ping.shift();
      let [_seq, t] = ping_stat.last_ping.shift();
      ping_stat.time = Date.now() - t;
      ping_stat.received++;
    }
    updateDebugStatus();
  }
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

  let dataChannel = pc.createDataChannel("pointer", {
    ordered: false,
    maxRetransmits: 0,
  });

  dataChannel.onopen = () => {
    connection = {
      pc,
      dataChannel,
      is_active() {
        return dataChannel.readyState == "open";
      },
      send(data) {
        dataChannel.send(data);
      }
    }
    overlay.style.display = "none";
    setStatus("Pointer channel open. Draw now.");
  };

  dataChannel.onmessage = (ev) => processIncomingPacket(ev.data);

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

/**
 * @type {CanvasRenderingContext2D}
 */
let ctx = inputArea.getContext("2d");

let lastPointer = null;

inputArea.addEventListener("pointermove", processPointerData, { passive: false });
inputArea.addEventListener("pointerdown", (ev) => {
  inputArea.setPointerCapture(ev.pointerId);
  processPointerData(ev);
}, { passive: false });
inputArea.addEventListener("pointerup", (ev) => {
  processPointerData(ev);
  try { inputArea.releasePointerCapture(ev.pointerId); } catch(e) {}
  lastPointer = null;
}, { passive: false });
inputArea.addEventListener("pointercancel", (ev) => {
  processPointerData(ev);
  try { inputArea.releasePointerCapture(ev.pointerId); } catch(e) {}
  lastPointer = null;
}, { passive: false });
inputArea.addEventListener('touchstart', (e) => {
  if (e.touches.length === 1) {
    e.preventDefault();
  }
}, { passive: false });

function processPointerData(event) {
  event.preventDefault();
  let {clientX, clientY, pressure} = event;

  let bound = inputArea.getBoundingClientRect();

  let x = clientX - bound.left;
  let y = clientY - bound.top;

  if (lastPointer) {
    ctx.beginPath();
    ctx.moveTo(lastPointer.x, lastPointer.y);
    ctx.lineTo(x, y);
    ctx.strokeStyle = `rgba(255, 255, 255, ${(lastPointer.pressure + pressure)/2})`;
    ctx.stroke();
  }

  lastPointer = { x, y, pressure };

  const xRatio = Math.max(0, Math.min(x / inputArea.width, 1.0));
  const yRatio = Math.max(0, Math.min(y / inputArea.height, 1.0));

  sendPointerData(xRatio, yRatio, pressure);
}

let lt = 0;
function animate(t) {
  let dt = t - lt;
  lt = t;

  ctx.globalCompositeOperation = "destination-out"
  ctx.fillStyle = `rgba(0, 0, 0, ${dt/200})`;
  ctx.fillRect(0, 0, inputArea.width, inputArea.height);
  ctx.globalCompositeOperation = "source-over"

  requestAnimationFrame(animate);
}
animate();

function sendPointerData(xRatio, yRatio, pressure) {
  if (!connection?.is_active()) return;

  const buffer = new ArrayBuffer(14);
  const view = new DataView(buffer);
  view.setUint16(0, 2, true);
  view.setFloat32(2, xRatio, true);
  view.setFloat32(6, yRatio, true);
  view.setFloat32(10, pressure, true);

  connection.send(buffer);
}

settings.apply();

function updateDebugStatus() {
  statusDbg.innerText = `connected: ${connection?.is_active()??false}`;
  if (connection?.is_active()) {
    statusDbg.innerText += `\nping: ${ping_stat.time} ms`;
    if (ping_stat.lost) {
      statusDbg.innerText += ` lost: ${Math.round(ping_stat.lost/(ping_stat.lost+ping_stat.received)*100)}%`;
    }
    statusDbg.innerText += `\nhost resolution: ${hostScreenWidth}x${hostScreenHeight}`;
  }
}

setInterval(() => {
  ping_stat.send_ping();
  updateDebugStatus();
}, 1000);
