/** 正则脚本引擎自检（回归脚本；用法同 selfcheck-macros.mjs） */
const { runRegex, newRegexScript } = await import('../src/services/st/regex.ts')

let fail = 0
const eq = (a, e, n) => {
  if (a !== e) { fail++; console.error(`✗ ${n}\n  期望 ${JSON.stringify(e)} 实得 ${JSON.stringify(a)}`) }
}
const mk = (over) => ({ ...newRegexScript(), ...over })

/* 基本替换 + placement 过滤 */
const s1 = mk({ findRegex: 'foo', replaceString: 'bar', placement: [2] })
eq(runRegex('foo foo', [s1], { placement: 2, isPrompt: false }), 'bar bar', 'placement 命中')
eq(runRegex('foo', [s1], { placement: 1, isPrompt: true }), 'foo', 'placement 不命中')

/* promptOnly / markdownOnly / 皆 false → 双通路 */
const po = mk({ findRegex: 'X', replaceString: 'P', promptOnly: true })
const mo = mk({ findRegex: 'X', replaceString: 'D', markdownOnly: true })
const both = mk({ findRegex: 'X', replaceString: 'B' })
eq(runRegex('X', [po], { placement: 2, isPrompt: true }), 'P', 'promptOnly 提示词生效')
eq(runRegex('X', [po], { placement: 2, isPrompt: false }), 'X', 'promptOnly 显示不生效')
eq(runRegex('X', [mo], { placement: 2, isPrompt: true }), 'X', 'markdownOnly 提示词不生效')
eq(runRegex('X', [mo], { placement: 2, isPrompt: false }), 'D', 'markdownOnly 显示生效')
eq(runRegex('X', [both], { placement: 2, isPrompt: true }), 'B', '皆 false 提示词生效')
eq(runRegex('X', [both], { placement: 2, isPrompt: false }), 'B', '皆 false 显示生效')

/* 深度过滤（仅提示词通路） */
const depth = mk({ findRegex: 'D', replaceString: 'R', minDepth: 1, maxDepth: 2 })
eq(runRegex('D', [depth], { placement: 2, isPrompt: true, depth: 1 }), 'R', '深度在区间内')
eq(runRegex('D', [depth], { placement: 2, isPrompt: true, depth: 0 }), 'D', '深度 < min 跳过')
eq(runRegex('D', [depth], { placement: 2, isPrompt: true, depth: 5 }), 'D', '深度 > max 跳过')
eq(runRegex('D', [depth], { placement: 2, isPrompt: false, depth: 0 }), 'R', '显示通路不做深度过滤')

/* /pattern/flags 写法 + 分组引用 + {{match}} */
const re1 = mk({ findRegex: '/hel(\\w+)/gi', replaceString: '[$1]' })
eq(runRegex('hello HELLO', [re1], { placement: 2, isPrompt: false }), '[lo] [LO]', '/regex/flags + $1')
const re2 = mk({ findRegex: 'ab', replaceString: '<{{match}}>' })
eq(runRegex('ab', [re2], { placement: 2, isPrompt: false }), '<ab>', '{{match}}')

/* substituteRegex = 2（宏替换并转义） */
{
  const { emptyContext } = await import('../src/services/st/macros.ts')
  const c = emptyContext({ charName: '爱.丽丝' })
  const sc = mk({ findRegex: '{{char}}', replaceString: 'R', substituteRegex: 2 })
  eq(runRegex('爱.丽丝', [sc], { placement: 2, isPrompt: true, macroCtx: c }), 'R', 'substituteRegex=2 转义后命中')
  const sc1 = mk({ findRegex: '{{char}}', replaceString: 'R', substituteRegex: 1 })
  eq(runRegex('爱.丽丝', [sc1], { placement: 2, isPrompt: true, macroCtx: c }), 'R', 'substituteRegex=1 原样（. 按正则任意字符命中）')
}

/* trimStrings + disabled */
const ts = mk({ findRegex: 'a', replaceString: 'a', trimStrings: ['---'] })
eq(runRegex('---a---', [ts], { placement: 2, isPrompt: false }), 'a', 'trimStrings')
const dis = mk({ findRegex: 'a', replaceString: 'b', disabled: true })
eq(runRegex('a', [dis], { placement: 2, isPrompt: false }), 'a', 'disabled 跳过')

/* 多脚本按顺序执行 */
const chain = [mk({ findRegex: 'a', replaceString: 'b' }), mk({ findRegex: 'b', replaceString: 'c' })]
eq(runRegex('a', chain, { placement: 2, isPrompt: false }), 'c', '多脚本链式')

console.log(fail ? `${fail} 失败` : '全部通过')
process.exit(fail ? 1 : 0)
