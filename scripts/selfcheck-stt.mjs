#!/usr/bin/env node
import assert from 'node:assert/strict'
import {
  DEFAULT_STT_SETTINGS,
  isWebSpeechSupported,
  isSpeechRecording,
  stopSpeechRecognition,
} from '../src/services/st/stt'

console.log('  — STT 语音识别配置与状态机自检 —')

// 1. 默认设置断言
assert.equal(DEFAULT_STT_SETTINGS.provider, 'webspeech', '默认提供商为 webspeech')
assert.equal(DEFAULT_STT_SETTINGS.lang, 'zh-CN', '默认语言为中文')
assert.ok(DEFAULT_STT_SETTINGS.whisperEndpoint?.includes('transcriptions'), 'Whisper 端点包含 transcriptions')

// 2. Node 环境安全探测
assert.equal(isWebSpeechSupported(), false, 'Node 环境下非浏览器应返回 false 且不崩溃')
assert.equal(isSpeechRecording(), false, '未启动时录音状态为 false')

// 3. 停止录音安全幂等性
stopSpeechRecognition()
assert.equal(isSpeechRecording(), false, '重复停止录音安全幂等')

console.log('✓ STT 语音输入模块自检全部通过')
