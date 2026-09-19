// TS 侧注释掩码单一维护点（issue #1481）：TS/Vue 源码里「注释 vs 真实代码」的
// 词法切分——把注释内容掩为等长空白（保留换行与列位），守门脚本在掩码文本上匹配
// 靶形态。此前实现住在 check-frontend-structure.ts，issue #1471 让 check-commands.ts
// 复用同一函数后，掩码改动同时影响命令一致性守门与前端结构守门，改动面靠注释
// 「两侧一并核对」人工维持——本模块把出口上收，两个消费者只消费同一份实现。
//
// 消费方：check-frontend-structure.ts（import 说明符扫描）、check-commands.ts
// （TS 调用面识别，注释掉的 invoke 不算真实调用）。掩码规则改动只动本文件，
// 消费者随引用自动跟随。
//
// 边界：本模块只承载 TS/Vue 注释掩码。面向 Rust 源码的词法掩码（check-structure.ts
// 的 maskNonCode 与 Rust 侧 test_support::scan、check-test-support.ts 的 maskComments）
// 是另一族，是否随同收敛见 #1466 的 Out of Scope 记录与 #1433；本票不动。

/** 若 text[start] 起是字符串/模板字面量，返回结束引号后的下标；未闭合则返回文末。
 *  否则返回 -1。只用于跳过字面量内容以识别注释起点，字面量本身不掩码。 */
function stringLiteralEnd(text: string, start: number): number {
  const quote = text[start];
  if (quote !== "'" && quote !== '"' && quote !== "`") return -1;
  let i = start + 1;
  while (i < text.length) {
    if (text[i] === "\\") {
      i += 2;
      continue;
    }
    if (text[i] === quote) return i + 1;
    i++;
  }
  return text.length;
}

/** 掩码 TS/Vue 源文本中的注释（行注释与块注释）：内容替换为等长空白（保留换行与
 *  列位），使 import / invoke 扫描只落在真实代码上。字面量内容不掩码——import 说明符
 *  本身是字符串、invoke 命令名也是字符串，掩码会连靶一起抹掉；但识别注释起点前先
 *  跳过字符串/模板字面量，否则字面量里的 // 或 /* 会被误当注释、吞掉同行与前后的
 *  真实代码（把真实 import 或 invoke 藏出检测面，issue #1471）。正则字面量体内未转义
 *  的 / 会终止字面量，故体内 / 必为 \/，紧邻的反斜杠即「正则内斜杠」信号——带转义的
 *  / 不作注释起点，正则字面量的 \/\/ 形态同样不吞其后的真实代码。正则字面量整体词法
 *  不做（残余：字符类内相邻斜杠如 /[//]/、模板 ${} 插值——本仓零现役命中，评审兜底）；
 *  字符串内的伪 import 仍靠关键词上下文排除。
 *  单一维护点（issue #1481）：check-frontend-structure.ts 与 check-commands.ts 消费
 *  本出口，掩码规则改动只动本文件。 */
export function maskComments(text: string): string {
  const out = text.split("");
  const n = text.length;
  const blank = (from: number, to: number): void => {
    for (let k = from; k < to && k < n; k++) if (out[k] !== "\n") out[k] = " ";
  };
  let i = 0;
  while (i < n) {
    // text[i - 1] === '\\'：正则字面量体内的转义斜杠，不是注释起点（issue #1471）。
    if (text[i] === "/" && text[i - 1] !== "\\" && text[i + 1] === "/") {
      const stop = text.indexOf("\n", i) === -1 ? n : text.indexOf("\n", i);
      blank(i, stop);
      i = stop;
    } else if (text[i] === "/" && text[i - 1] !== "\\" && text[i + 1] === "*") {
      const end = text.indexOf("*/", i + 2);
      const stop = end === -1 ? n : end + 2;
      blank(i, stop);
      i = stop;
    } else {
      const literalEnd = stringLiteralEnd(text, i);
      i = literalEnd === -1 ? i + 1 : literalEnd;
    }
  }
  return out.join("");
}
