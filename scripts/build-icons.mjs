/** 从唯一 SVG 源生成桌面图标，将中间产物保存在 temp/。 */
import { copyFileSync, mkdirSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

/** 构建 Windows 和桌面端图标。参数：无。返回：无，生成失败时抛出错误。 */
function buildIcons() {
  const root = fileURLToPath(new URL("../", import.meta.url));
  const source = join(root, "ui", "assets", "icon.svg");
  const staging = join(root, "temp", "icon-build");
  const output = join(root, "src-tauri", "icons");
  const cli = join(root, "node_modules", "@tauri-apps", "cli", "tauri.js");
  mkdirSync(staging, { recursive: true });
  mkdirSync(output, { recursive: true });
  const generated = spawnSync(
    process.execPath,
    [cli, "icon", source, "--output", staging],
    { stdio: "inherit" },
  );
  if (generated.error) throw generated.error;
  if (generated.status !== 0) throw new Error("桌面图标生成失败");
  // 仅发布顶层桌面资源；移动平台的生成文件留在 temp/，不加入 Windows 项目。
  for (const entry of readdirSync(staging, { withFileTypes: true })) {
    if (entry.isFile())
      copyFileSync(join(staging, entry.name), join(output, entry.name));
  }
  copyFileSync(join(staging, "icon.png"), join(output, "source.png"));
}

buildIcons();
