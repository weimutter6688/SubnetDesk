import { expect, test } from "bun:test";
import { VideoFrame } from "../src/generated/message";
import { decodeVideoBatch, initialVideoDecodeState, KeyFrameRecovery } from "../src/video";

function clock() {
  let now = 0;
  let nextId = 0;
  const timers = new Map<number, { at: number; run: () => void }>();
  return {
    now: () => now,
    schedule(run: () => void, delay: number) {
      const id = ++nextId;
      timers.set(id, { at: now + delay, run });
      return () => { timers.delete(id); };
    },
    advance(to: number) {
      for (;;) {
        const next = [...timers.entries()].sort((a, b) => a[1].at - b[1].at)[0];
        if (!next || next[1].at > to) break;
        timers.delete(next[0]);
        now = next[1].at;
        next[1].run();
      }
      now = to;
    },
    pending: () => timers.size,
  };
}

test("a dropped recovery key frame is retried on an idle desktop", () => {
  const time = clock();
  const requests: number[] = [];
  const recovery = new KeyFrameRecovery(() => requests.push(time.now()), time.schedule, time.now);
  let state = initialVideoDecodeState();
  let decoded = 0;
  const decoder = { decodeQueueSize: 6, decode() { decoded++; } };
  function deliver(key: boolean) {
    state = decodeVideoBatch(decoder, VideoFrame.create({
      vp9s: { frames: [{ key, pts: BigInt(time.now() + 1), data: new Uint8Array([1]) }] },
    }), state, (chunk) => chunk as unknown as EncodedVideoChunk);
    recovery.setAwaitingKeyFrame(state.awaitingKeyFrame);
  }
  deliver(false);
  expect(requests).toEqual([0]);
  time.advance(100);
  deliver(true); // Decoder is still full, and the retry is rate limited.
  expect(decoded).toBe(0);
  expect(time.pending()).toBe(1);
  decoder.decodeQueueSize = 0;
  time.advance(1_000); // No new frame or worker event arrives in this interval.
  expect(requests).toEqual([0, 1_000]);
  deliver(true);
  expect(decoded).toBe(1);
  expect(time.pending()).toBe(0);
  time.advance(5_000);
  expect(requests).toEqual([0, 1_000]);
});

test("disconnect or decoder replacement cancels retries and resets their budget", () => {
  const time = clock();
  const requests: number[] = [];
  const recovery = new KeyFrameRecovery(() => requests.push(time.now()), time.schedule, time.now);
  recovery.setAwaitingKeyFrame(true);
  time.advance(100);
  recovery.setAwaitingKeyFrame(true);
  expect(time.pending()).toBe(1);
  recovery.reset();
  time.advance(5_000);
  expect(requests).toEqual([0]);
  recovery.setAwaitingKeyFrame(true);
  expect(requests).toEqual([0, 5_000]);
  recovery.setAwaitingKeyFrame(false);
  time.advance(5_100);
  recovery.setAwaitingKeyFrame(true);
  expect(requests).toEqual([0, 5_000]);
  time.advance(6_000);
  expect(requests).toEqual([0, 5_000, 6_000]);
  recovery.reset();
});
