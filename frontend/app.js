const canvas = document.getElementById("pointerCanvas");
const overlay = document.getElementById("overlay");
const statusEl = document.getElementById("status");
const pinForm = document.getElementById("pinForm");
const pinInput = document.getElementById("pinInput");

let pc = null;
let dataChannel = null;
let socket = null;
let screenWidth = 1920;
let screenHeight = 1080;
let canvasScaleX = 1;
let canvasScaleY = 1;
let canvasOffsetX = 0;
let canvasOffsetY = 0;

function setStatus(message) {
  statusEl.textContent = message;
}

function resizeCanvas() {
  canvas.width = window.innerWidth;
  canvas.height = window.innerHeight;
  updateCanvasAspectRatio();
}

function updateCanvasAspectRatio() {
  const windowAspect = window.innerWidth / window.innerHeight;
  const screenAspect = screenWidth / screenHeight;

  const ctx = canvas.getContext("2d");
  ctx.clearRect(0, 0, canvas.width, canvas.height);

  if (windowAspect > screenAspect) {
    const scaledHeight = window.innerHeight;
    const scaledWidth = scaledHeight * screenAspect;
    canvasScaleX = scaledWidth / screenWidth;
    canvasScaleY = scaledHeight / screenHeight;
    canvasOffsetX = (window.innerWidth - scaledWidth) / 2;
    canvasOffsetY = 0;
  } else {
    const scaledWidth = window.innerWidth;
    const scaledHeight = scaledWidth / screenAspect;
    canvasScaleX = scaledWidth / screenWidth;
    canvasScaleY = scaledHeight / screenHeight;
    canvasOffsetX = 0;
    canvasOffsetY = (window.innerHeight - scaledHeight) / 2;
  }

  ctx.fillStyle = "#070a11";
  ctx.fillRect(0, 0, canvas.width, canvas.height);
}

window.addEventListener("resize", resizeCanvas);
resizeCanvas();

pinForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  const pin = pinInput.value.trim();
  if (!/^\d{6}$/.test(pin)) {
    setStatus("Enter a valid 6-digit PIN.");
    return;
  }
  setStatus("Connecting...");
  await startSession(pin);
});

async function startSession(pin) {
  const protocol = location.protocol === "https:" ? "wss" : "ws";
  const wsUrl = `${protocol}://${location.host}/ws`;

  socket = new WebSocket(wsUrl);
  socket.binaryType = "arraybuffer";

  socket.addEventListener("open", () => {
    socket.send(pin);
    setStatus("Waiting for pairing approval...");
  });

  socket.addEventListener("message", async (event) => {
    if (typeof event.data === "string") {
      try {
        const payload = JSON.parse(event.data);

        if (payload.type === "screen-dimensions") {
          screenWidth = payload.width;
          screenHeight = payload.height;
          updateCanvasAspectRatio();
          setStatus(`Server resolution: ${screenWidth}x${screenHeight}`);
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

  socket.addEventListener("close", () => {
    setStatus("Connection closed.");
  });

  socket.addEventListener("error", () => {
    setStatus("WebSocket error.");
  });
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
    enablePointerCapture();
  };

  dataChannel.onclose = () => {
    setStatus("Pointer channel closed.");
  };

  pc.ondatachannel = (event) => {
    event.channel.onopen = () => {
      overlay.style.display = "none";
      setStatus("Pointer channel open. Draw now.");
      enablePointerCapture();
    };
  };

  const offer = await pc.createOffer();
  await pc.setLocalDescription(offer);
  socket.send(JSON.stringify({ type: "offer", sdp: offer.sdp }));
}

function enablePointerCapture() {
  canvas.addEventListener("pointermove", sendPointerData, { passive: false });
  canvas.addEventListener("pointerdown", (event) => event.preventDefault());
  canvas.addEventListener("pointerup", (event) => event.preventDefault());
  canvas.addEventListener("pointercancel", (event) => event.preventDefault());
}

function sendPointerData(event) {
  if (!dataChannel || dataChannel.readyState !== "open") {
    return;
  }

  event.preventDefault();

  const canvasRect = canvas.getBoundingClientRect();
  const clientX = event.clientX;
  const clientY = event.clientY;

  const xOnScaledArea = Math.max(
    0,
    Math.min(clientX - canvasOffsetX, canvasRect.width - canvasOffsetX * 2)
  );
  const yOnScaledArea = Math.max(
    0,
    Math.min(clientY - canvasOffsetY, canvasRect.height - canvasOffsetY * 2)
  );

  const xRatio = Math.max(
    0,
    Math.min(xOnScaledArea / (canvasScaleX * screenWidth), 1.0)
  );
  const yRatio = Math.max(
    0,
    Math.min(yOnScaledArea / (canvasScaleY * screenHeight), 1.0)
  );
  const pressure =
    typeof event.pressure === "number" ? event.pressure : event.buttons ? 1.0 : 0.0;

  const buffer = new ArrayBuffer(20);
  const view = new DataView(buffer);
  view.setFloat32(0, xRatio, true);
  view.setFloat32(4, yRatio, true);
  view.setUint32(8, screenWidth, true);
  view.setUint32(12, screenHeight, true);
  view.setFloat32(16, pressure, true);

  dataChannel.send(buffer);
}