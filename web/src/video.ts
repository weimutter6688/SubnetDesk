import type { VideoFrame as ProtocolVideoFrame } from "./generated/message";
import type { ProtocolVideoCodec } from "./capabilities";

interface VideoDecodeQueue {
  readonly decodeQueueSize?: number;
  decode(chunk: EncodedVideoChunk): void;
}

interface ClosableVideoDecoder {
  readonly state: string;
  close(): void;
}

type EncodedVideoChunkFactory = (init: EncodedVideoChunkInit) => EncodedVideoChunk;

export interface VideoDecodeState {
  timestamp: number;
  awaitingKeyFrame: boolean;
  droppedFrames: number;
  codec: ProtocolVideoCodec;
}

export interface DecoderRecoveryState {
  attempts: number;
  maxAttempts: number;
}

const MAX_DECODE_QUEUE_SIZE = 6;
const KEY_FRAME_REQUEST_INTERVAL_MS = 1_000;

export function shouldRequestKeyFrame(lastRequestedAt: number | undefined, now: number): boolean {
  return lastRequestedAt === undefined || now - lastRequestedAt >= KEY_FRAME_REQUEST_INTERVAL_MS;
}

export function initialDecoderRecoveryState(maxAttempts = 1): DecoderRecoveryState {
  return { attempts: 0, maxAttempts };
}

export function requestDecoderRecovery(previous: DecoderRecoveryState): {
  state: DecoderRecoveryState;
  shouldRetry: boolean;
} {
  if (previous.attempts >= previous.maxAttempts) {
    return { state: previous, shouldRetry: false };
  }
  return {
    state: { ...previous, attempts: previous.attempts + 1 },
    shouldRetry: true,
  };
}

export function closeVideoDecoder(decoder: ClosableVideoDecoder | undefined): void {
  if (decoder && decoder.state !== "closed") decoder.close();
}

export function initialVideoDecodeState(codec: ProtocolVideoCodec = "vp9"): VideoDecodeState {
  return {
    timestamp: 0,
    awaitingKeyFrame: false,
    droppedFrames: 0,
    codec,
  };
}

function encodedFrames(frame: ProtocolVideoFrame, codec: ProtocolVideoCodec) {
  if (codec === "h264") return frame.h264s?.frames;
  if (codec === "av1") return frame.av1s?.frames;
  return frame.vp9s?.frames;
}

export function decodeVideoBatch(
  decoder: VideoDecodeQueue,
  frame: ProtocolVideoFrame,
  previousState: VideoDecodeState,
  createChunk: EncodedVideoChunkFactory = (init) => new EncodedVideoChunk(init),
): VideoDecodeState {
  const frames = encodedFrames(frame, previousState.codec);
  if (!frames) return previousState;

  let timestamp = previousState.timestamp;
  let awaitingKeyFrame = previousState.awaitingKeyFrame;
  let droppedFrames = previousState.droppedFrames;
  for (const encoded of frames) {
    const sourceTimestamp = Number(encoded.pts);
    timestamp = Number.isSafeInteger(sourceTimestamp) && sourceTimestamp > timestamp
      ? sourceTimestamp
      : timestamp + 1;
    if ((decoder.decodeQueueSize ?? 0) >= MAX_DECODE_QUEUE_SIZE) {
      awaitingKeyFrame = true;
      droppedFrames += 1;
      continue;
    }
    if (awaitingKeyFrame && !encoded.key) {
      droppedFrames += 1;
      continue;
    }
    // A key frame restores references without reset(), which would unconfigure WebCodecs.
    awaitingKeyFrame = false;
    decoder.decode(
      createChunk({
        type: encoded.key ? "key" : "delta",
        timestamp,
        data: encoded.data,
      }),
    );
  }
  return { timestamp, awaitingKeyFrame, droppedFrames, codec: previousState.codec };
}
