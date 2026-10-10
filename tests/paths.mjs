import assert from "node:assert/strict";
import { isWindowsPath, windowsPath } from "../ui/paths.ts";

// 与后端 rpmm::paths::normalize 保持同一张形式表，避免前后端判断不一致。
const forms = [
  ["C:\\apps\\api.exe", "C:\\apps\\api.exe"],
  ["C:/apps/api.exe", "C:\\apps\\api.exe"],
  ["C:\\\\apps\\\\api.exe", "C:\\apps\\api.exe"],
  ["C:/apps\\api.exe", "C:\\apps\\api.exe"],
  ["/c/apps/api.exe", "C:\\apps\\api.exe"],
  ["/C/apps/api.exe", "C:\\apps\\api.exe"],
  ["/cygdrive/c/apps/api.exe", "C:\\apps\\api.exe"],
  ["/cygdrive/C/apps/api.exe", "C:\\apps\\api.exe"],
  ["\\\\server\\share\\api.exe", "\\\\server\\share\\api.exe"],
  ["//server/share/api.exe", "\\\\server\\share\\api.exe"],
  ["\\\\?\\C:\\apps\\api.exe", "\\\\?\\C:\\apps\\api.exe"],
  ["//?/C:/apps/api.exe", "\\\\?\\C:\\apps\\api.exe"],
  ["  D:/logs/  ", "D:\\logs"],
  ["c:/", "C:\\"],
];
for (const [input, expected] of forms)
  assert.equal(windowsPath(input), expected, input);
// 相对路径、无盘符根路径和单独一个 /c 都不算绝对路径。
for (const invalid of ["", "   ", "relative.exe", "/apps/api.exe", "/c", "C:apps", "C:", "\\", "//"])
  assert.equal(windowsPath(invalid), null, JSON.stringify(invalid));
assert.equal(isWindowsPath("/d/data"), true);
assert.equal(isWindowsPath("logs\\sub"), false);
console.log("路径形式验证通过：盘符、正反斜杠、MSYS、Cygwin、网络路径与扩展前缀。 ");
