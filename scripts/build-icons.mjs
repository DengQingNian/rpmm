/** 从规范化 SVG 生成桌面图标，将中间产物保存在 temp/。 */
import { copyFileSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

/** 执行 Tauri 图标转换。参数：cli 为工具路径，source 为源图，output 为输出目录，sizes 为可选 PNG 尺寸。返回：无，失败时抛出错误。 */
function renderIcons(cli, source, output, sizes = []) {
  const args = [cli, "icon", source, "--output", output];
  for (const size of sizes) args.push("--png", String(size));
  const generated = spawnSync(process.execPath, args, { stdio: "inherit" });
  // 先报告启动错误，再检查退出状态，确保工具失败时不会继续发布不完整的资源。
  if (generated.error) throw generated.error;
  if (generated.status !== 0) throw new Error("桌面图标生成失败");
}

/** 将 PNG 帧封装为 Windows ICO。参数：output 为图标目录，sizes 为有序尺寸列表。返回：无，写入 icon.ico。 */
function writeWindowsIcon(output, sizes) {
  const header = Buffer.alloc(6 + sizes.length * 16);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(sizes.length, 4);
  const frames = [];
  let offset = header.length;
  // 每帧独立设置尺寸和数据偏移；ICO 使用零表示 256 px，内部保留 PNG 的透明通道。
  for (const [index, size] of sizes.entries()) {
    const png = readFileSync(join(output, `${size}x${size}.png`));
    const entry = 6 + index * 16;
    header[entry] = size === 256 ? 0 : size;
    header[entry + 1] = size === 256 ? 0 : size;
    header.writeUInt16LE(1, entry + 4);
    header.writeUInt16LE(32, entry + 6);
    header.writeUInt32LE(png.length, entry + 8);
    header.writeUInt32LE(offset, entry + 12);
    frames.push(png);
    offset += png.length;
  }
  writeFileSync(join(output, "icon.ico"), Buffer.concat([header, ...frames]));
}

/** 构建 Windows 和桌面端图标。参数：无。返回：无，生成失败时抛出错误。 */
function buildIcons() {
  const root = fileURLToPath(new URL("../", import.meta.url));
  const source = join(root, "ui", "assets", "icon.svg");
  const smallSource = join(root, "ui", "assets", "icon-small.svg");
  const staging = join(root, "temp", "icon-build");
  const largeStaging = join(staging, "large");
  const smallStaging = join(staging, "small");
  const output = join(root, "src-tauri", "icons");
  const cli = join(root, "node_modules", "@tauri-apps", "cli", "tauri.js");
  const smallSizes = [16, 24, 30, 32, 44, 48];
  const largeSizes = [64, 128, 256, 1024];
  mkdirSync(staging, { recursive: true });
  mkdirSync(output, { recursive: true });
  renderIcons(cli, source, staging);
  renderIcons(cli, source, largeStaging, largeSizes);
  renderIcons(cli, smallSource, smallStaging, smallSizes);
  // 仅发布桌面资源；移动平台文件留在 temp/。小尺寸使用同轮廓的单色、加粗版本。
  for (const entry of readdirSync(staging, { withFileTypes: true })) {
    if (entry.isFile()) copyFileSync(join(staging, entry.name), join(output, entry.name));
  }
  for (const size of largeSizes) {
    copyFileSync(join(largeStaging, `${size}x${size}.png`), join(output, `${size}x${size}.png`));
  }
  for (const size of smallSizes) {
    copyFileSync(join(smallStaging, `${size}x${size}.png`), join(output, `${size}x${size}.png`));
  }
  copyFileSync(join(smallStaging, "30x30.png"), join(output, "Square30x30Logo.png"));
  copyFileSync(join(smallStaging, "44x44.png"), join(output, "Square44x44Logo.png"));
  copyFileSync(join(largeStaging, "1024x1024.png"), join(output, "source.png"));
  writeWindowsIcon(output, [16, 24, 32, 48, 64, 128, 256]);
}

buildIcons();
