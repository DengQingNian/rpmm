/** 折叠路径分隔符并去掉末尾分隔符。参数：text 为盘符之后的文本。返回：以反斜杠开头的主体。 */
function fold(text: string): string {
  let out = "";
  let previous = false;
  for (const ch of text) {
    if (ch === "/" || ch === "\\") {
      if (!previous) out += "\\";
      previous = true;
      continue;
    }
    previous = false;
    out += ch;
  }
  return out.replace(/\\+$/, "");
}

/** 组装盘符路径。参数：letter 为盘符字母，rest 为冒号之后的文本。返回：C:\xx 形式。 */
function drivePath(letter: string, rest: string): string {
  const drive = letter.toUpperCase() + ":";
  const body = fold(rest);
  return body ? drive + body : drive + "\\";
}

/** 归一化 Windows 绝对路径的各种书写形式。参数：value 为表单或配置中的路径文本。
 * 返回：C:\xx\yy 形式；不是受支持的绝对路径时返回 null，与后端 rpmm::paths 规则一致。 */
export function windowsPath(value: string): string | null {
  const text = value.trim();
  if (!text) return null;
  // 扩展长度与设备前缀整体保留，只统一分隔符方向。
  if (/^[\\/][\\/][?.][\\/]/.test(text)) return text.replaceAll("/", "\\");
  if (/^[\\/]{2}/.test(text)) {
    const parts = text.split(/[\\/]+/).filter(Boolean);
    return parts.length ? "\\\\" + parts.join("\\") : null;
  }
  // /c/xx 与 /cygdrive/c/xx 都按盘符路径处理；单独一个 /c 更像命令开关，不作为盘符。
  const msys = text.match(/^\/(?:cygdrive\/)?([a-zA-Z])\//i);
  if (msys) return drivePath(msys[1] ?? "", text.slice((msys[0] ?? "").length - 1));
  const drive = text.match(/^([a-zA-Z]):[\\/]/);
  return drive ? drivePath(drive[1] ?? "", text.slice(2)) : null;
}

/** 判断文本是否为受支持的 Windows 绝对路径。参数：value 为路径文本。返回：是否绝对。 */
export function isWindowsPath(value: string): boolean {
  return windowsPath(value) !== null;
}
