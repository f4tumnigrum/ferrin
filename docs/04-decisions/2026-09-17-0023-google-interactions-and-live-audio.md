# 0023: Google Interactions and Live audio adapters

**English** | [Chinese](../zh-CN/04-decisions/2026-09-17-0023-google-interactions-and-live-audio.md)

- Status: proposed
- Date: 2026-09-17
- Related: [Google provider](../providers/google.md), [ADR 0009](2026-09-13-0009-http-transport-and-secure-url.md)

## Context

[Fact] The local Vercel AI SDK Google adapter includes general Interactions, Live transcription and speech translation; Ferrin previously excluded those endpoints. Source: `ai-sdk/packages/google/src/interactions`, `google-transcription-model.ts` and `google-speech-translation-model.ts`, inspected 2026-09-17. This inspection verifies adapter behavior, not live Google availability.

## Decision

[Decision] Add an explicit `GoogleProvider::interactions` language-model factory, leaving `language_model` on generateContent. The adapter converts Interactions steps, JSON-schema function tools, provider tools, structured output and multimodal files, and preserves interaction IDs, signatures, usage and service-tier metadata under canonical and configured provider keys.

[Decision] `previousInteractionId` compacts matching assistant steps when storage is enabled. `store: false` preserves complete history and warns when combined with a prior ID. Tool results needed for function continuation remain replayable. Background calls use bounded, cancellation-aware polling of the original configured origin; background streams read incremental GET SSE and resume using event IDs, with a bounded retry count and deadline. Only terminal initial responses synthesize complete parts. Explicit cancellation attempts bounded remote cleanup; stream drop remains task-free and explicit resource cancellation is available.

[Decision] Stream EOF requires an explicit terminal interaction event. A parse error, provider error or premature EOF closes open parts and emits a terminal error without an executable incomplete tool call. Server-provided IDs are encoded as path segments, and file URLs remain payload references for core secure downloads.

[Decision] The additive `realtime` feature enables Live streaming transcription for `-live` model IDs with signed 16-bit mono PCM at 16 kHz. `setupComplete` gates audio submission; setup uses `inputAudioTranscription` and omits `generationConfig` as in the reference adapter. `GoogleSpeechTranslationModel` requires a target language, auto-detects source language, returns PCM at 24 kHz and supports `echoTargetLanguage`.

[Decision] Owned WebSocket streams validate the HTTP-equivalent URL using `url_policy`, pin resolved addresses, bound messages and respect cancellation, backpressure and drop. Transcription closes after explicit idle/turn completion or a one-second quiet window after input completion. Translation closes after one second of PCM silence (threshold 128) after input EOF, or turn completion plus a one-second grace period. Premature or abnormal EOF is an error; usage and raw metadata are retained.

## Verification boundary

[Decision] Fixture and local WebSocket regressions verify conversions, stream contracts and cancellation. They do not establish live endpoint compatibility; the existing PV-031 recording requirement remains open. New code keeps the existing Vercel AI SDK attribution at crate, module and root NOTICE levels.
