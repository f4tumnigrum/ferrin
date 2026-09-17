# 0023：Google Interactions 与 Live 音频适配器

[English](../../04-decisions/2026-09-17-0023-google-interactions-and-live-audio.md) | **简体中文**

- 状态：proposed
- 日期：2026-09-17
- 关联：[Google 供应商](../providers/google.md)、[ADR 0009](2026-09-13-0009-http-transport-and-secure-url.md)

## 背景

【事实】 本地 Vercel AI SDK Google 适配器包含通用 Interactions、Live 转写和语音翻译；Ferrin 此前排除了这些端点。来源：2026-09-17 检查的 `ai-sdk/packages/google/src/interactions`、`google-transcription-model.ts` 和 `google-speech-translation-model.ts`。该检查确认适配器行为，未验证 Google 在线可用性。

## 决策

【决策】 增加独立的 `GoogleProvider::interactions` 语言模型工厂，`language_model` 仍使用 generateContent。适配器转换 Interactions 步骤、JSON Schema 函数工具、供应商工具、结构化输出和多模态文件，并在标准及自定义供应商键下保留 interaction ID、签名、usage 和服务等级元数据。

【决策】 启用存储时，`previousInteractionId` 压缩匹配的助手步骤。`store: false` 保留完整历史，与先前 ID 合用时给出警告。函数续接所需的工具结果保持可回放。后台调用对原配置源执行有时限且可取消的轮询；后台流读取增量 GET SSE，并通过事件 ID 在有限重试次数和时限内恢复。仅初始响应已终结时合成完整事件。显式取消尝试有界远端清理；drop 不创建任务，并提供显式资源取消。

【决策】 流 EOF 必须有显式终结 interaction 事件。解析错误、供应商错误或提前 EOF 关闭已打开的片段并发出终结错误，不生成可执行的不完整工具调用。服务端 ID 作为路径段编码，文件 URL 保留为供 core 安全下载的引用。

【决策】 可叠加的 `realtime` feature 为 `-live` 模型启用 Live 流式转写，输入为 16 kHz、有符号 16 位单声道 PCM。`setupComplete` 后才发送音频；与参考适配器一致，setup 使用 `inputAudioTranscription` 并省略 `generationConfig`。`GoogleSpeechTranslationModel` 要求目标语言，自动识别源语言，返回 24 kHz PCM 并支持 `echoTargetLanguage`。

【决策】 自有 WebSocket 流根据 `url_policy` 校验等价 HTTP URL、固定解析地址、限制消息大小，并遵守取消、背压和 drop。输入完成后转写在显式空闲/轮次完成或一秒静默后结束。翻译在输入 EOF 后连续一秒 PCM 静音（阈值 128），或轮次完成加一秒宽限期后结束。提前或异常 EOF 作为错误处理；保留 usage 和原始元数据。

## 验证边界

【决策】 fixture 和本地 WebSocket 回归验证转换、流契约与取消，不证明在线端点兼容性；现有 PV-031 录制要求继续开放。新增代码在 crate、模块和根 NOTICE 三处沿用 Vercel AI SDK 归属声明。
