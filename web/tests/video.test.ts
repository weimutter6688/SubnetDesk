import { describe, expect, test } from "bun:test";
import { VideoFrame } from "../src/generated/message";
import {
  closeVideoDecoder,
  decodeVideoBatch,
  initialDecoderRecoveryState,
  initialVideoDecodeState,
  requestDecoderRecovery,
  shouldRequestKeyFrame,
} from "../src/video";

describe("WebCodecs video queue", () => {
  test("allows one decoder rebuild and then stops retrying", () => {
    const first = requestDecoderRecovery(initialDecoderRecoveryState());
    expect(first.shouldRetry).toBe(true);
    expect(first.state.attempts).toBe(1);

    const second = requestDecoderRecovery(first.state);
    expect(second.shouldRetry).toBe(false);
    expect(second.state.attempts).toBe(1);
  });

  test("does not close a decoder that WebCodecs has already closed", () => {
    let closeCalls = 0;
    closeVideoDecoder({
      state: "closed",
      close: () => {
        closeCalls += 1;
      },
    });

    expect(closeCalls).toBe(0);
  });

  test("closes a configured decoder during normal cleanup", () => {
    let closeCalls = 0;
    closeVideoDecoder({
      state: "configured",
      close: () => {
        closeCalls += 1;
      },
    });

    expect(closeCalls).toBe(1);
  });

  test("keeps the decoder open between a key-frame batch and following delta frames", () => {
    const decoded: EncodedVideoChunkInit[] = [];
    let flushCalls = 0;
    const decoder = {
      decode: (chunk: EncodedVideoChunk) => decoded.push(chunk as unknown as EncodedVideoChunkInit),
      flush: async () => {
        flushCalls += 1;
      },
    };
    const createChunk = (init: EncodedVideoChunkInit) => init as unknown as EncodedVideoChunk;

    let state = decodeVideoBatch(
      decoder,
      VideoFrame.create({
        vp9s: { frames: [{ key: true, pts: 1n, data: new Uint8Array([1]) }] },
      }),
      initialVideoDecodeState(),
      createChunk,
    );
    state = decodeVideoBatch(
      decoder,
      VideoFrame.create({
        vp9s: { frames: [{ key: false, pts: 2n, data: new Uint8Array([2]) }] },
      }),
      state,
      createChunk,
    );

    expect(decoded.map(({ type }) => type)).toEqual(["key", "delta"]);
    expect(decoded.map(({ timestamp: value }) => value)).toEqual([1, 2]);
    expect(state.timestamp).toBe(2);
    expect(state.droppedFrames).toBe(0);
    expect(flushCalls).toBe(0);
  });

  test("keeps timestamps increasing when the source timestamp repeats", () => {
    const decoded: EncodedVideoChunkInit[] = [];
    const decoder = {
      decode: (chunk: EncodedVideoChunk) => decoded.push(chunk as unknown as EncodedVideoChunkInit),
    };
    const createChunk = (init: EncodedVideoChunkInit) => init as unknown as EncodedVideoChunk;
    const state = decodeVideoBatch(
      decoder,
      VideoFrame.create({
        vp9s: {
          frames: [
            { key: true, pts: 5n, data: new Uint8Array([1]) },
            { key: false, pts: 5n, data: new Uint8Array([2]) },
          ],
        },
      }),
      { ...initialVideoDecodeState(), timestamp: 4 },
      createChunk,
    );

    expect(decoded.map(({ timestamp: value }) => value)).toEqual([5, 6]);
    expect(state.timestamp).toBe(6);
  });

  test("drops queued delta frames and resumes from a key frame", () => {
    const decoded: EncodedVideoChunkInit[] = [];
    let configured = true;
    const decoder = {
      decodeQueueSize: 12,
      decode: (chunk: EncodedVideoChunk) => {
        if (!configured) throw new DOMException("Decoder is unconfigured", "InvalidStateError");
        decoded.push(chunk as unknown as EncodedVideoChunkInit);
      },
      reset: () => {
        configured = false;
      },
    };
    const createChunk = (init: EncodedVideoChunkInit) => init as unknown as EncodedVideoChunk;

    let state = decodeVideoBatch(
      decoder,
      VideoFrame.create({
        vp9s: { frames: [{ key: false, pts: 1n, data: new Uint8Array([1]) }] },
      }),
      initialVideoDecodeState(),
      createChunk,
    );
    expect(decoded).toHaveLength(0);
    expect(state.awaitingKeyFrame).toBe(true);
    expect(state.droppedFrames).toBe(1);

    decoder.decodeQueueSize = 0;
    state = decodeVideoBatch(
      decoder,
      VideoFrame.create({
        vp9s: {
          frames: [
            { key: false, pts: 2n, data: new Uint8Array([2]) },
            { key: true, pts: 3n, data: new Uint8Array([3]) },
          ],
        },
      }),
      state,
      createChunk,
    );
    expect(decoded.map(({ type }) => type)).toEqual(["key"]);
    expect(configured).toBe(true);
    expect(state.awaitingKeyFrame).toBe(false);
    expect(state.droppedFrames).toBe(2);
  });

  test("keeps backpressure active throughout a large recovery batch", () => {
    let decoded = 0;
    const decoder = {
      decodeQueueSize: 0,
      decode: () => {
        decoded += 1;
        decoder.decodeQueueSize += 1;
      },
    };
    const state = decodeVideoBatch(
      decoder,
      VideoFrame.create({
        vp9s: {
          frames: Array.from({ length: 13 }, (_, index) => ({
            key: index === 0,
            pts: BigInt(index + 1),
            data: new Uint8Array([index]),
          })),
        },
      }),
      { ...initialVideoDecodeState(), awaitingKeyFrame: true },
      (init) => init as unknown as EncodedVideoChunk,
    );
    expect(decoded).toBe(6);
    expect(state.awaitingKeyFrame).toBe(true);
    expect(state.droppedFrames).toBe(7);
  });

  test("does not enqueue a recovery key frame while the decoder is still full", () => {
    let decoded = 0;
    const state = decodeVideoBatch(
      { decodeQueueSize: 6, decode: () => { decoded += 1; } },
      VideoFrame.create({
        vp9s: { frames: [{ key: true, pts: 1n, data: new Uint8Array([1]) }] },
      }),
      { ...initialVideoDecodeState(), awaitingKeyFrame: true },
      (init) => init as unknown as EncodedVideoChunk,
    );
    expect(decoded).toBe(0);
    expect(state.awaitingKeyFrame).toBe(true);
    expect(state.droppedFrames).toBe(1);
  });

  test("requests the first key frame immediately and throttles retries", () => {
    expect(shouldRequestKeyFrame(undefined, 0)).toBe(true);
    expect(shouldRequestKeyFrame(0, 0)).toBe(false);
    expect(shouldRequestKeyFrame(0, 999)).toBe(false);
    expect(shouldRequestKeyFrame(0, 1_000)).toBe(true);
    expect(shouldRequestKeyFrame(1_000, 1_500)).toBe(false);
    expect(shouldRequestKeyFrame(1_000, 2_000)).toBe(true);
  });

  test("uses the browser chunk constructor in the production decode path", () => {
    const decoded: EncodedVideoChunk[] = [];
    const previousConstructor = globalThis.EncodedVideoChunk;
    class TestChunk {
      readonly type: EncodedVideoChunkType;
      readonly timestamp: number;
      readonly data: AllowSharedBufferSource;

      constructor(init: EncodedVideoChunkInit) {
        this.type = init.type;
        this.timestamp = init.timestamp;
        this.data = init.data;
      }
    }
    Object.defineProperty(globalThis, "EncodedVideoChunk", {
      configurable: true,
      value: TestChunk,
    });
    try {
      decodeVideoBatch(
        { decode: (chunk) => decoded.push(chunk) },
        VideoFrame.create({
          h264s: { frames: [{ key: true, pts: 9n, data: new Uint8Array([9]) }] },
        }),
        initialVideoDecodeState("h264"),
      );
      expect(decoded[0]?.type).toBe("key");
      expect(decoded[0]?.timestamp).toBe(9);
    } finally {
      Object.defineProperty(globalThis, "EncodedVideoChunk", {
        configurable: true,
        value: previousConstructor,
      });
    }
  });
});
