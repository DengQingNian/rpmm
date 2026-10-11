#!/usr/bin/env node
/**
 * 校验发布版本在全部清单中一致，可选核对发布标签。
 *
 * 用法：
 *   node scripts/check-version.mjs                       # 校验各清单互相一致
 *   node scripts/check-version.mjs --tag v0.3.0          # 额外核对标签
 *   node scripts/check-version.mjs --tag v0.3.0 --json   # 输出机器可读结果，供工作流解析
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

/** 仓库根目录。参数：无。返回：绝对路径字符串。 */
const root = fileURLToPath(new URL("../", import.meta.url));

/**
 * 读取仓库内的文本文件。
 * 参数：file 为相对仓库根目录的路径。
 * 返回：文件内容字符串。
 */
function readText(file) {
  return readFileSync(join(root, file), "utf8");
}

/**
 * 读取并解析仓库内的 JSON 文件。
 * 参数：file 为相对仓库根目录的路径。
 * 返回：解析后的对象。
 */
function readJson(file) {
  return JSON.parse(readText(file));
}

/**
 * 从 Cargo 清单的 [package] 段读取版本号。
 * 参数：text 为清单内容，file 为文件名（用于错误说明）。
 * 返回：版本号字符串；缺少 [package] 段或 version 字段时抛出错误。
 */
function cargoVersion(text, file) {
  // 只取第一个 [package] 段，避免读到后面 [workspace] 等段中的 version；行尾兼容 LF 与 CRLF。
  const section = /\[package\]([\s\S]*?)(?:\r?\n\[|$)/.exec(text);
  const matched = section && /^[ \t]*version[ \t]*=[ \t]*"([^"]+)"/m.exec(section[1]);
  if (!matched) throw new Error(`${file} 缺少 [package] 段或 version 字段`);
  return matched[1];
}

/**
 * 从 Cargo.lock 读取指定包记录的版本号。
 * 参数：text 为锁文件内容，name 为包名。
 * 返回：版本号字符串；锁文件缺少该包时抛出错误。
 */
function lockVersion(text, name) {
  // cargo 固定按 name、version 的顺序输出包记录，按相邻两行匹配即可，无需完整解析 TOML；
  // Windows 上检出仓库时锁文件可能是 CRLF，因此行尾写成可选回车加换行。
  const pattern = new RegExp(`\\[\\[package\\]\\]\\r?\\nname = "${name}"\\r?\\nversion = "([^"]+)"`);
  const matched = pattern.exec(text);
  if (!matched) throw new Error(`Cargo.lock 缺少 ${name}，请先执行 cargo check --workspace 同步锁文件`);
  return matched[1];
}

/**
 * 收集全部需要同步的版本号。
 * 参数：无。
 * 返回：按清单排列的 [清单名, 版本号] 列表。
 */
function collectVersions() {
  const lock = readText("Cargo.lock");
  // package-lock.json 的根包版本写在 packages[""] 中，只有旧格式才使用顶层 version。
  const npmLock = readJson("package-lock.json");
  const npmLockVersion = npmLock.packages?.[""]?.version ?? npmLock.version;
  return [
    ["package.json", readJson("package.json").version],
    ["package-lock.json", npmLockVersion],
    ["Cargo.toml", cargoVersion(readText("Cargo.toml"), "Cargo.toml")],
    ["src-tauri/Cargo.toml", cargoVersion(readText("src-tauri/Cargo.toml"), "src-tauri/Cargo.toml")],
    ["src-tauri/tauri.conf.json", readJson("src-tauri/tauri.conf.json").version],
    ["Cargo.lock rpmm", lockVersion(lock, "rpmm")],
    ["Cargo.lock rpmm-desktop", lockVersion(lock, "rpmm-desktop")],
  ];
}

/**
 * 解析命令行参数。
 * 参数：argv 为 process.argv 去掉前两项后的参数数组。
 * 返回：{ tag: 发布标签或空串, json: 是否输出 JSON }。
 */
function parseArgs(argv) {
  const options = { tag: "", json: false };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    // 只接受已知参数：拼写错误直接报错，避免工作流里少校验一项却仍然显示通过。
    if (arg === "--json") {
      options.json = true;
    } else if (arg === "--tag") {
      options.tag = (argv[index + 1] ?? "").trim();
      index += 1;
      if (!options.tag) throw new Error("--tag 缺少标签值");
    } else {
      throw new Error(`无法识别的参数：${arg}（可用参数：--tag <标签>、--json）`);
    }
  }
  return options;
}

/**
 * 校验各清单与发布标签的版本号，并输出结果。
 * 参数：无。
 * 返回：无；发现不一致时打印全部差异并以退出码 1 结束。
 */
function main() {
  const options = parseArgs(process.argv.slice(2));
  const versions = collectVersions();
  const expected = versions[0][1];
  const problems = [];

  // 清单之间必须完全一致：安装包名、前端包版本和 Cargo 锁文件由不同工具读取，任一处漂移都会让发布内容与标签不符。
  if (versions.some(([, version]) => version !== expected)) {
    problems.push(`清单版本不一致：${versions.map(([name, version]) => `${name}=${version}`).join("，")}`);
  }

  // 标签允许带 v 前缀，比较前统一去掉，v0.3.0 与 0.3.0 两种写法都可以。
  if (options.tag && options.tag.replace(/^v/i, "") !== expected) {
    problems.push(`标签 ${options.tag} 与清单版本 ${expected} 不一致`);
  }

  // 只要有一处不一致就整体失败，避免产出与标签不符的安装包。
  if (problems.length > 0) {
    for (const problem of problems) console.error(problem);
    console.error("请同步 package.json、package-lock.json、Cargo.toml、src-tauri/Cargo.toml、src-tauri/tauri.conf.json 与 Cargo.lock（npm install --package-lock-only、cargo check --workspace）。");
    process.exitCode = 1;
    return;
  }

  // 工作流用 --json 解析版本号，人工使用时输出可读摘要。
  if (options.json) {
    console.log(JSON.stringify({ version: expected, tag: options.tag, sources: Object.fromEntries(versions) }));
    return;
  }
  console.log(`版本校验通过：${expected}`);
  console.log(`已核对：${versions.map(([name]) => name).join("、")}${options.tag ? `；标签 ${options.tag}` : ""}`);
}

// 参数或清单错误只打印一行中文说明，不输出堆栈，便于在工作流日志里直接定位。
try {
  main();
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
}
