import { expect, test } from "bun:test";

test("video worker requests a key frame after overload and acknowledges recovered output", async () => {
  const messages: Array<{ type: string; requestKeyFrame?: boolean }> = [];
  const scope = {
    onmessage: null as ((event: { data: unknown }) => void) | null,
    postMessage: (message: { type: string; requestKeyFrame?: boolean }) => messages.push(message),
  };
  const decoders: TestDecoder[] = [];
  class TestDecoder {
    state = "unconfigured";
    decodeQueueSize = 0;
    decoded = 0;
    constructor(readonly callbacks: VideoDecoderInit) { decoders.push(this); }
    configure() { this.state = "configured"; }
    reset() { this.state = "unconfigured"; }
    close() { this.state = "closed"; }
    decode() {
      if (this.state !== "configured") throw new DOMException("Unconfigured", "InvalidStateError");
      this.decoded += 1;
    }
  }
  const originals = new Map(
    ["self", "VideoDecoder", "EncodedVideoChunk"].map((name) =>
      [name, Object.getOwnPropertyDescriptor(globalThis, name)] as const),
  );
  Object.defineProperty(globalThis, "self", { configurable: true, value: scope });
  Object.defineProperty(globalThis, "VideoDecoder", { configurable: true, value: TestDecoder });
  Object.defineProperty(globalThis, "EncodedVideoChunk", {
    configurable: true,
    value: class { constructor(readonly init: EncodedVideoChunkInit) {} },
  });
  try {
    await import("../src/video-worker");
    scope.onmessage?.({ data: {
      type: "initialize",
      canvas: { getContext: () => ({ drawImage() {} }) },
      codec: "vp09.00.10.08",
      protocol: "vp9",
      width: 1920,
      height: 1080,
    } });
    const decoder = decoders[0];
    const deliver = (key: boolean, pts: bigint) => scope.onmessage?.({ data: {
      type: "frame",
      frame: { vp9s: { frames: [{ key, pts, data: new Uint8Array([1]) }] } },
    } });
    decoder.decodeQueueSize = 12;
    deliver(false, 1n);
    expect(messages).toEqual([{ type: "ack", requestKeyFrame: true }]);
    decoder.decodeQueueSize = 0;
    deliver(false, 2n);
    expect(messages.at(-1)).toEqual({ type: "ack", requestKeyFrame: true });
    deliver(true, 3n);
    expect(decoder.decoded).toBe(1);
    expect(decoder.state).toBe("configured");
    expect(messages).toHaveLength(2);
    decoder.callbacks.output({ close() {} } as VideoFrame);
    expect(messages.at(-1)).toEqual({ type: "ack" });
    expect(messages.some((message) => message.type === "error")).toBe(false);
  } finally {
    scope.onmessage?.({ data: { type: "reset" } });
    for (const [name, descriptor] of originals) {
      if (descriptor) Object.defineProperty(globalThis, name, descriptor);
      else Reflect.deleteProperty(globalThis, name);
    }
  }
});
