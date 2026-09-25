import {
  ControlKey,
  ClipboardFormat,
  ImageQuality,
  KeyboardMode,
  Message,
  OptionMessage_BoolOption,
  SupportedDecoding_PreferCodec,
  type DisplayInfo,
  type KeyEvent,
  type PeerInfo,
} from "./generated/message";
import { LanCryptoSession } from "./crypto";
import {
  chooseVideoCodec,
  coalescePointerSample,
  initialPointerSample,
  type PointerSample,
  type SelectedVideoCodec,
} from "./capabilities";
import { mapCanvasPoint, mouseMask, normalizeFingerprint } from "./protocol";
import { localizeDocument, translate, type TranslationKey } from "./i18n";
import {
  closeVideoDecoder,
  decodeVideoBatch,
  initialDecoderRecoveryState,
  initialVideoDecodeState,
  requestDecoderRecovery,
  KeyFrameRecovery,
  type DecoderRecoveryState,
  type VideoDecodeState,
} from "./video";

interface ServerInfo {
  app_name: string;
  device_name: string;
  fingerprint: string;
  version: string;
  secure: boolean;
  certificate_mode: "local-ca" | "custom";
  ca_certificate_url: string;
  video_worker_url: string;
  permission_profile: "view-only" | "control" | "collaboration";
}

interface Credentials {
  username: string;
  password: string;
}

const MAX_PASSWORD_LENGTH = 256;
const encoder = new TextEncoder();
const locale = localizeDocument();
const t = (key: TranslationKey): string => translate(locale, key);

function requiredElement<T extends HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (!element) throw new Error(`页面元素缺失：${id}`);
  return element as T;
}

const connectPanel = requiredElement<HTMLElement>("connect-panel");
const viewerPanel = requiredElement<HTMLElement>("viewer-panel");
const loginForm = requiredElement<HTMLFormElement>("login-form");
const usernameInput = requiredElement<HTMLInputElement>("username");
const passwordInput = requiredElement<HTMLInputElement>("password");
const connectButton = requiredElement<HTMLButtonElement>("connect");
const disconnectButton = requiredElement<HTMLButtonElement>("disconnect");
const fullscreenButton = requiredElement<HTMLButtonElement>("fullscreen");
const canvas = requiredElement<HTMLCanvasElement>("screen");
const deviceName = requiredElement<HTMLElement>("device-name");
const fingerprintElement = requiredElement<HTMLElement>("fingerprint");
const statusElement = requiredElement<HTMLElement>("status");
const viewerStatus = requiredElement<HTMLElement>("viewer-status");
const connectionStats = requiredElement<HTMLElement>("connection-stats");
const trustPanel = requiredElement<HTMLElement>("trust-panel");
const caCertificateLink = requiredElement<HTMLAnchorElement>("ca-certificate");
const displaySelect = requiredElement<HTMLSelectElement>("display");
const qualitySelect = requiredElement<HTMLSelectElement>("quality");
const clipboardReadButton = requiredElement<HTMLButtonElement>("clipboard-read");
const clipboardWriteButton = requiredElement<HTMLButtonElement>("clipboard-write");
const textInput = requiredElement<HTMLInputElement>("text-input");
const sendTextButton = requiredElement<HTMLButtonElement>("send-text");

let serverInfo: ServerInfo;
let socket: WebSocket | undefined;
let cryptoSession: LanCryptoSession | undefined;
let secured = false;
let authenticated = false;
let remoteDisplay: DisplayInfo | undefined;
let peerInfo: PeerInfo | undefined;
let decoder: VideoDecoder | undefined;
let videoWorker: Worker | undefined;
let canvasTransferred = false;
let selectedCodec: SelectedVideoCodec | undefined;
let videoDecodeState: VideoDecodeState = initialVideoDecodeState();
let pendingPointerSample = initialPointerSample();
let pointerAnimationFrame: number | undefined;
let activePointerButton = 0;
let lastFrameRenderedAt = 0;
let renderedFrames = 0;
let displayedFps = 0;
let videoAckPending = false;
const keyFrameRecovery = new KeyFrameRecovery(() => {
  if (!authenticated) return;
  try {
    send(Message.create({ misc: { refresh_video: true } }));
  } catch {
    // A connection close can race the recovery timer.
  }
});
let lastRemoteClipboard = "";
let decoderRecoveryState: DecoderRecoveryState = initialDecoderRecoveryState();
let firstDecoderError = "";

function setStatus(message: string, error = false): void {
  statusElement.textContent = message;
  statusElement.dataset.error = error ? "true" : "false";
}

function resetConnection(message = t("disconnected")): void {
  secured = false;
  authenticated = false;
  cryptoSession = undefined;
  connectButton.disabled = false;
  socket = undefined;
  closeVideoDecoder(decoder);
  decoder = undefined;
  videoWorker?.postMessage({ type: "reset" });
  remoteDisplay = undefined;
  peerInfo = undefined;
  selectedCodec = undefined;
  videoDecodeState = initialVideoDecodeState();
  pendingPointerSample = undefined;
  activePointerButton = 0;
  if (pointerAnimationFrame !== undefined) cancelAnimationFrame(pointerAnimationFrame);
  pointerAnimationFrame = undefined;
  renderedFrames = 0;
  displayedFps = 0;
  lastFrameRenderedAt = 0;
  videoAckPending = false;
  keyFrameRecovery.reset();
  lastRemoteClipboard = "";
  decoderRecoveryState = initialDecoderRecoveryState();
  firstDecoderError = "";
  viewerPanel.hidden = true;
  connectPanel.hidden = false;
  viewerStatus.textContent = "";
  setStatus(message);
}

function closeConnection(message = t("disconnected")): void {
  const active = socket;
  socket = undefined;
  if (active && active.readyState < WebSocket.CLOSING) active.close(1000, "client closed");
  resetConnection(message);
}

function sendRaw(payload: Uint8Array): void {
  if (!socket || socket.readyState !== WebSocket.OPEN) throw new Error("连接已经关闭");
  socket.send(payload);
}

function send(message: Message): void {
  if (!cryptoSession || !secured) throw new Error("安全通道尚未建立");
  sendRaw(cryptoSession.encrypt(message));
}

function currentModifiers(
  event: Pick<KeyboardEvent, "altKey" | "ctrlKey" | "shiftKey" | "metaKey">,
): ControlKey[] {
  const modifiers: ControlKey[] = [];
  if (event.altKey) modifiers.push(ControlKey.Alt);
  if (event.ctrlKey) modifiers.push(ControlKey.Control);
  if (event.shiftKey) modifiers.push(ControlKey.Shift);
  if (event.metaKey) modifiers.push(ControlKey.Meta);
  return modifiers;
}

const controlKeys: Record<string, ControlKey> = {
  Alt: ControlKey.Alt,
  Backspace: ControlKey.Backspace,
  CapsLock: ControlKey.CapsLock,
  Control: ControlKey.Control,
  Delete: ControlKey.Delete,
  ArrowDown: ControlKey.DownArrow,
  End: ControlKey.End,
  Escape: ControlKey.Escape,
  Home: ControlKey.Home,
  ArrowLeft: ControlKey.LeftArrow,
  Meta: ControlKey.Meta,
  PageDown: ControlKey.PageDown,
  PageUp: ControlKey.PageUp,
  Enter: ControlKey.Return,
  ArrowRight: ControlKey.RightArrow,
  Shift: ControlKey.Shift,
  " ": ControlKey.Space,
  Tab: ControlKey.Tab,
  ArrowUp: ControlKey.UpArrow,
  Insert: ControlKey.Insert,
};
for (let index = 1; index <= 12; index += 1) {
  controlKeys[`F${index}`] = ControlKey[`F${index}` as keyof typeof ControlKey] as ControlKey;
}

function sendKey(event: KeyboardEvent, release = false): void {
  if (!authenticated || serverInfo.permission_profile === "view-only") return;
  const controlKey = controlKeys[event.key];
  if (release && controlKey === undefined) return;
  const keyEvent: Partial<KeyEvent> = {
    down: !release,
    press: !release,
    modifiers: currentModifiers(event),
    mode: KeyboardMode.Legacy,
  };
  if (controlKey !== undefined) {
    keyEvent.control_key = controlKey;
  } else {
    const codePoint = Array.from(event.key)[0]?.codePointAt(0);
    if (codePoint === undefined || event.key.length > 2) return;
    keyEvent.unicode = codePoint;
  }
  event.preventDefault();
  send(Message.create({ key_event: keyEvent }));
}

function sendText(value: string): void {
  if (!authenticated || serverInfo.permission_profile === "view-only" || !value) return;
  for (const character of Array.from(value)) {
    const codePoint = character.codePointAt(0);
    if (codePoint === undefined) continue;
    send(
      Message.create({
        key_event: {
          down: false,
          press: true,
          modifiers: [],
          mode: KeyboardMode.Legacy,
          unicode: codePoint,
        },
      }),
    );
  }
}

function canvasCoordinates(event: Pick<PointerSample, "clientX" | "clientY">): { x: number; y: number } {
  if (!remoteDisplay) return { x: 0, y: 0 };
  const bounds = canvas.getBoundingClientRect();
  return mapCanvasPoint(
    event.clientX - bounds.left,
    event.clientY - bounds.top,
    bounds.width,
    bounds.height,
    remoteDisplay.width,
    remoteDisplay.height,
    remoteDisplay.x,
    remoteDisplay.y,
  );
}

function sendMouse(event: PointerSample, type: number): void {
  if (!authenticated || serverInfo.permission_profile === "view-only") return;
  const point = canvasCoordinates(event);
  send(
    Message.create({
      mouse_event: {
        mask: mouseMask(type, event.button, event.buttons),
        x: point.x,
        y: point.y,
        modifiers: currentModifiers(event),
      },
    }),
  );
}

function pointerSample(event: PointerEvent): PointerSample {
  return {
    clientX: event.clientX,
    clientY: event.clientY,
    button: event.button,
    buttons: event.buttons,
    altKey: event.altKey,
    ctrlKey: event.ctrlKey,
    shiftKey: event.shiftKey,
    metaKey: event.metaKey,
  };
}

function queuePointerMove(event: PointerEvent): void {
  pendingPointerSample = coalescePointerSample(pendingPointerSample, pointerSample(event));
  if (pointerAnimationFrame !== undefined) return;
  pointerAnimationFrame = requestAnimationFrame(() => {
    pointerAnimationFrame = undefined;
    const sample = pendingPointerSample;
    pendingPointerSample = undefined;
    if (sample) sendMouse(sample, 0);
  });
}

function sendVideoAck(requestKeyFrame = false): void {
  if (!authenticated) return;
  try {
    send(Message.create({ misc: { video_received: true } }));
    keyFrameRecovery.setAwaitingKeyFrame(requestKeyFrame);
  } catch {
    // A close event can race the decoder's final output callback.
  }
}

function decoderErrorMessage(message: string, mode: "worker" | "main"): string {
  const codec = selectedCodec?.codec ?? "unknown";
  const dimensions = remoteDisplay ? `${remoteDisplay.width}x${remoteDisplay.height}` : "unknown";
  return `${message} [codec=${codec}, size=${dimensions}, mode=${mode}]`;
}

function recoverDecoder(message: string, mode: "worker" | "main"): void {
  const detailed = decoderErrorMessage(message, mode);
  if (!firstDecoderError) firstDecoderError = detailed;
  const recovery = requestDecoderRecovery(decoderRecoveryState);
  decoderRecoveryState = recovery.state;
  if (!recovery.shouldRetry || !peerInfo) {
    const history = firstDecoderError === detailed ? detailed : `${firstDecoderError}; retry: ${detailed}`;
    closeConnection(`${t("decoderFailed")}: ${history}`);
    return;
  }
  viewerStatus.textContent = `${t("decoderRecovering")}: ${detailed}`;
  try {
    configureDecoder(peerInfo, true);
    sendVideoAck(true);
  } catch (error) {
    const recoveryError = error instanceof Error ? error.message : t("unknownError");
    closeConnection(`${t("decoderFailed")}: ${firstDecoderError}; recovery: ${recoveryError}`);
  }
}

function configureVideoWorker(codec: SelectedVideoCodec, display: DisplayInfo): boolean {
  if (
    !serverInfo.video_worker_url ||
    typeof canvas.transferControlToOffscreen !== "function" ||
    typeof Worker === "undefined"
  ) {
    return false;
  }
  let transferredCanvas: OffscreenCanvas | undefined;
  if (!videoWorker) {
    transferredCanvas = canvas.transferControlToOffscreen();
    canvasTransferred = true;
    videoWorker = new Worker(serverInfo.video_worker_url, { type: "module" });
    videoWorker.addEventListener("message", (event: MessageEvent<{
      type: "ack" | "stats" | "error";
      requestKeyFrame?: boolean;
      fps?: number;
      droppedFrames?: number;
      message?: string;
    }>) => {
      if (event.data.type === "ack") {
        sendVideoAck(event.data.requestKeyFrame);
      } else if (event.data.type === "stats") {
        connectionStats.textContent =
          `${event.data.fps ?? 0} FPS · ${event.data.droppedFrames ?? 0} dropped · ${selectedCodec?.protocol.toUpperCase() ?? ""}`;
      } else {
        recoverDecoder(event.data.message ?? t("unknownError"), "worker");
      }
    });
    videoWorker.addEventListener("error", () => recoverDecoder(t("unknownError"), "worker"));
  }
  const message = {
    type: "initialize",
    canvas: transferredCanvas,
    codec: codec.codec,
    protocol: codec.protocol,
    width: display.width,
    height: display.height,
  };
  if (transferredCanvas) {
    videoWorker.postMessage(message, [transferredCanvas]);
  } else {
    videoWorker.postMessage(message);
  }
  return true;
}

function videoFrameTransferables(frame: NonNullable<Message["video_frame"]>): Transferable[] {
  const frames = frame.vp9s?.frames ?? frame.h264s?.frames ?? frame.av1s?.frames ?? [];
  const buffers = new Set<ArrayBuffer>();
  for (const encoded of frames) {
    if (encoded.data.buffer instanceof ArrayBuffer) buffers.add(encoded.data.buffer);
  }
  return [...buffers];
}

function configureDecoder(peer: PeerInfo, recovering = false): void {
  keyFrameRecovery.reset();
  if (!selectedCodec) throw new Error(t("noCodec"));
  const codec = selectedCodec;
  peerInfo = peer;
  const display = peer.displays[peer.current_display] ?? peer.displays[0];
  if (!display) throw new Error(t("noDisplay"));
  if (!recovering) {
    decoderRecoveryState = initialDecoderRecoveryState();
    firstDecoderError = "";
  }
  remoteDisplay = display;
  if (!canvasTransferred) {
    canvas.width = display.width;
    canvas.height = display.height;
  }
  closeVideoDecoder(decoder);
  decoder = undefined;
  videoAckPending = false;
  videoDecodeState = initialVideoDecodeState(codec.protocol);
  if (configureVideoWorker(codec, display)) {
    connectionStats.textContent = `0 FPS · 0 dropped · ${codec.protocol.toUpperCase()}`;
  } else {
  const context = canvas.getContext("2d", { alpha: false });
  if (!context) throw new Error(t("canvasUnavailable"));
  decoder = new VideoDecoder({
    output: (frame) => {
      context.drawImage(frame, 0, 0, canvas.width, canvas.height);
      frame.close();
      renderedFrames += 1;
      const now = performance.now();
      if (lastFrameRenderedAt === 0) lastFrameRenderedAt = now;
      if (now - lastFrameRenderedAt >= 1_000) {
        displayedFps = Math.round((renderedFrames * 1_000) / (now - lastFrameRenderedAt));
        renderedFrames = 0;
        lastFrameRenderedAt = now;
        connectionStats.textContent =
          `${displayedFps} FPS · ${videoDecodeState.droppedFrames} dropped · ${codec.protocol.toUpperCase()}`;
      }
      if (videoAckPending && authenticated) {
        videoAckPending = false;
        sendVideoAck(videoDecodeState.awaitingKeyFrame);
      }
    },
    error: (error) => recoverDecoder(error.message, "main"),
  });
  decoder.configure({
    codec: codec.codec,
    codedWidth: display.width,
    codedHeight: display.height,
    optimizeForLatency: true,
    hardwareAcceleration: "prefer-hardware",
  });
  }
  displaySelect.replaceChildren(
    ...peer.displays.map((item, index) => {
      const option = document.createElement("option");
      option.value = index.toString();
      option.textContent = `${t("display")} ${index + 1} · ${item.width}×${item.height}`;
      option.selected = item === display;
      return option;
    }),
  );
  displaySelect.hidden = peer.displays.length <= 1;
  viewerStatus.textContent = `${peer.hostname || serverInfo.device_name} · ${display.width}×${display.height}`;
}

function authenticate(username: string, password: string): void {
  if (!selectedCodec) throw new Error(t("noCodec"));
  const prefer =
    selectedCodec.protocol === "h264"
      ? SupportedDecoding_PreferCodec.H264
      : selectedCodec.protocol === "av1"
        ? SupportedDecoding_PreferCodec.AV1
        : SupportedDecoding_PreferCodec.VP9;
  const passwordBytes = encoder.encode(password);
  try {
    send(
      Message.create({
        login_request: {
          my_id: cryptoSession?.clientIdentifier() ?? "web",
          my_name:
            (navigator as Navigator & { userAgentData?: { platform?: string } }).userAgentData
              ?.platform || navigator.platform || "Browser",
          my_platform: "Web",
          version: serverInfo.version,
          video_ack_required: true,
          session_id: 0n,
          lan_login: {
            access_username: username,
            access_password: passwordBytes,
            credential_revision_hint: 0n,
          },
          option: {
            disable_audio: OptionMessage_BoolOption.Yes,
            disable_clipboard:
              serverInfo.permission_profile === "collaboration"
                ? OptionMessage_BoolOption.No
                : OptionMessage_BoolOption.Yes,
            show_remote_cursor: OptionMessage_BoolOption.No,
            supported_decoding: {
              ability_vp9: selectedCodec.protocol === "vp9" ? 1 : 0,
              ability_h264: selectedCodec.protocol === "h264" ? 1 : 0,
              ability_av1: selectedCodec.protocol === "av1" ? 1 : 0,
              prefer,
            },
          },
        },
      }),
    );
  } finally {
    passwordBytes.fill(0);
  }
}

async function handleMessage(payload: Uint8Array, credentials: Credentials): Promise<void> {
  if (!cryptoSession) throw new Error("连接状态无效");
  if (!secured) {
    const message = Message.decode(payload);
    if (!message.lan_server_hello) throw new Error("服务器没有返回 LAN 安全握手");
    const result = await cryptoSession.acceptServerHello(message.lan_server_hello);
    if (normalizeFingerprint(result.fingerprint) !== normalizeFingerprint(serverInfo.fingerprint)) {
      throw new Error("网页公布的设备指纹与握手签名不一致");
    }
    sendRaw(result.keyMessage);
    secured = true;
    setStatus(t("secureChannel"));
    authenticate(credentials.username, credentials.password);
    credentials.password = "";
    return;
  }

  const message = cryptoSession.decrypt(payload);
  if (message.login_response) {
    if (message.login_response.error) {
      const retry = message.login_response.retry_after_seconds;
      throw new Error(`${message.login_response.error}${retry ? `（${retry} 秒后重试）` : ""}`);
    }
    if (!message.login_response.peer_info) throw new Error("服务器没有返回桌面信息");
    configureDecoder(message.login_response.peer_info);
    authenticated = true;
    connectPanel.hidden = true;
    viewerPanel.hidden = false;
    canvas.focus();
    setStatus(t("connected"));
  }
  if (message.peer_info) configureDecoder(message.peer_info);
  if (message.video_frame) {
    if (videoWorker) {
      videoWorker.postMessage(
        { type: "frame", frame: message.video_frame },
        videoFrameTransferables(message.video_frame),
      );
    } else if (decoder) {
      const previousDropped = videoDecodeState.droppedFrames;
      videoDecodeState = decodeVideoBatch(decoder, message.video_frame, videoDecodeState);
      if (
        videoDecodeState.awaitingKeyFrame ||
        videoDecodeState.droppedFrames > previousDropped
      ) {
        sendVideoAck(videoDecodeState.awaitingKeyFrame);
      } else {
        videoAckPending = true;
      }
    } else {
      sendVideoAck();
    }
  }
  if (message.test_delay && !message.test_delay.from_client) {
    send(Message.create({ test_delay: message.test_delay }));
  }
  if (message.misc?.close_reason) closeConnection(message.misc.close_reason);
  if (message.message_box?.text) viewerStatus.textContent = message.message_box.text;
  if (
    message.clipboard &&
    serverInfo.permission_profile === "collaboration" &&
    !message.clipboard.compress &&
    message.clipboard.format === ClipboardFormat.Text
  ) {
    const text = new TextDecoder().decode(message.clipboard.content);
    lastRemoteClipboard = text;
    void navigator.clipboard.writeText(text).catch(() => {
      viewerStatus.textContent = t("remoteClipboardPermission");
    });
  }
}

async function connect(username: string, password: string): Promise<void> {
  if (!("VideoDecoder" in window) || !("EncodedVideoChunk" in window)) {
    throw new Error(t("webCodecsUnsupported"));
  }
  selectedCodec = await chooseVideoCodec(async (codec) => {
    try {
      const result = await VideoDecoder.isConfigSupported({
        codec,
        codedWidth: 1920,
        codedHeight: 1080,
        optimizeForLatency: true,
      });
      return result.supported === true;
    } catch {
      return false;
    }
  });
  if (!selectedCodec) {
    throw new Error(t("codecUnsupported"));
  }
  cryptoSession = await LanCryptoSession.create();
  const scheme = location.protocol === "https:" ? "wss:" : "ws:";
  const nextSocket = new WebSocket(`${scheme}//${location.host}/ws`);
  const credentials: Credentials = { username, password };
  let receiveQueue = Promise.resolve();
  nextSocket.binaryType = "arraybuffer";
  socket = nextSocket;

  nextSocket.addEventListener("open", () => {
    if (socket !== nextSocket || !cryptoSession) return;
    setStatus(t("verifyingIdentity"));
    sendRaw(cryptoSession.clientHello());
  });
  nextSocket.addEventListener("message", (event) => {
    if (socket !== nextSocket || !(event.data instanceof ArrayBuffer)) return;
    receiveQueue = receiveQueue
      .then(() => handleMessage(new Uint8Array(event.data), credentials))
      .catch((error: unknown) => {
        closeConnection(error instanceof Error ? error.message : t("connectionFailed"));
        statusElement.dataset.error = "true";
      });
  });
  nextSocket.addEventListener("error", () => {
    credentials.password = "";
    if (socket === nextSocket) closeConnection(t("networkFailed"));
    statusElement.dataset.error = "true";
  });
  nextSocket.addEventListener("close", () => {
    credentials.password = "";
    if (socket === nextSocket) {
      resetConnection(authenticated ? t("remoteDisconnected") : t("connectionClosed"));
    }
  });
}

loginForm.addEventListener("submit", (event) => {
  event.preventDefault();
  const username = usernameInput.value.trim();
  const password = passwordInput.value;
  if (!username || !password) {
    setStatus(t("enterCredentials"), true);
    return;
  }
  if (encoder.encode(password).length > MAX_PASSWORD_LENGTH) {
    setStatus(t("passwordTooLong"), true);
    return;
  }
  connectButton.disabled = true;
  setStatus(t("connecting"));
  passwordInput.value = "";
  void connect(username, password).catch((error: unknown) => {
    resetConnection(error instanceof Error ? error.message : t("connectionFailed"));
    statusElement.dataset.error = "true";
  });
});

disconnectButton.addEventListener("click", () => closeConnection());
fullscreenButton.addEventListener("click", () => void viewerPanel.requestFullscreen());
canvas.addEventListener("contextmenu", (event) => event.preventDefault());
canvas.addEventListener("pointermove", queuePointerMove);
canvas.addEventListener("pointerdown", (event) => {
  canvas.focus();
  canvas.setPointerCapture(event.pointerId);
  activePointerButton = event.button;
  sendMouse(pointerSample(event), 1);
});
canvas.addEventListener("pointerup", (event) => {
  if (canvas.hasPointerCapture(event.pointerId)) canvas.releasePointerCapture(event.pointerId);
  sendMouse(pointerSample(event), 2);
  activePointerButton = 0;
});
canvas.addEventListener("pointercancel", (event) => {
  sendMouse({ ...pointerSample(event), button: activePointerButton, buttons: 0 }, 2);
  activePointerButton = 0;
});
canvas.addEventListener(
  "wheel",
  (event) => {
    if (!authenticated || serverInfo.permission_profile === "view-only") return;
    event.preventDefault();
    send(
      Message.create({
        mouse_event: {
          mask: 3,
          x: Math.round(event.deltaX),
          y: Math.round(event.deltaY),
          modifiers: currentModifiers(event),
        },
      }),
    );
  },
  { passive: false },
);
canvas.addEventListener("keydown", (event) => sendKey(event));
canvas.addEventListener("keyup", (event) => sendKey(event, true));
sendTextButton.addEventListener("click", () => {
  sendText(textInput.value);
  textInput.value = "";
  canvas.focus();
});
textInput.addEventListener("keydown", (event) => {
  if (event.key === "Enter") {
    event.preventDefault();
    sendTextButton.click();
  }
});
displaySelect.addEventListener("change", () => {
  const display = peerInfo?.displays[Number(displaySelect.value)];
  if (!display) return;
  send(
    Message.create({
      misc: {
        switch_display: {
          display: Number(displaySelect.value),
          x: display.x,
          y: display.y,
          width: display.width,
          height: display.height,
          cursor_embedded: false,
        },
      },
    }),
  );
});
qualitySelect.addEventListener("change", () => {
  const quality = Number(qualitySelect.value) as ImageQuality;
  send(Message.create({ misc: { option: { image_quality: quality } } }));
});
clipboardReadButton.addEventListener("click", () => {
  void navigator.clipboard
    .readText()
    .then((text) => {
      send(
        Message.create({
          clipboard: {
            compress: false,
            content: encoder.encode(text),
            width: 0,
            height: 0,
            format: ClipboardFormat.Text,
            special_name: "",
          },
        }),
      );
    })
    .catch(() => {
      viewerStatus.textContent = t("clipboardReadPermission");
    });
});
clipboardWriteButton.addEventListener("click", () => {
  void navigator.clipboard.writeText(lastRemoteClipboard).catch(() => {
    viewerStatus.textContent = t("clipboardWritePermission");
  });
});

void fetch("/api/info", { cache: "no-store", credentials: "same-origin" })
  .then(async (response) => {
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    serverInfo = (await response.json()) as ServerInfo;
    deviceName.textContent = serverInfo.device_name;
    fingerprintElement.textContent = serverInfo.fingerprint.match(/.{1,4}/g)?.join(" ") ?? serverInfo.fingerprint;
    if (serverInfo.certificate_mode === "local-ca" && serverInfo.ca_certificate_url) {
      trustPanel.hidden = false;
      caCertificateLink.href = serverInfo.ca_certificate_url;
    }
    const collaboration = serverInfo.permission_profile === "collaboration";
    clipboardReadButton.hidden = !collaboration;
    clipboardWriteButton.hidden = !collaboration;
    const viewOnly = serverInfo.permission_profile === "view-only";
    textInput.hidden = viewOnly;
    sendTextButton.hidden = viewOnly;
    canvas.dataset.viewOnly = viewOnly ? "true" : "false";
    connectButton.disabled = false;
    setStatus(serverInfo.secure ? t("httpsEnabled") : t("insecureHttp"), !serverInfo.secure);
  })
  .catch((error: unknown) => {
    connectButton.disabled = true;
    setStatus(
      `${t("cannotReadDeviceInfo")}: ${error instanceof Error ? error.message : t("unknownError")}`,
      true,
    );
  });
