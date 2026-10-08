import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

/** 拆分第三方依赖。参数：id 为模块路径。返回：依赖分组名称或默认分组。 */
function vendorChunk(id) {
  if (!id.includes("node_modules")) return undefined;
  if (/node_modules[/\\](?:@vue|vue)[/\\]/.test(id)) return "vue";
  if (/node_modules[/\\]zrender[/\\]/.test(id)) return "chart-renderer";
  if (/node_modules[/\\]echarts[/\\]/.test(id)) return "charts";
  return "components";
}

// Rust 构建会锁定正在生成的可执行文件，前端开发服务器仅监听界面源文件。
export default defineConfig({
  plugins: [vue()],
  build: { rollupOptions: { output: { manualChunks: vendorChunk } } },
  clearScreen: false,
  server: {
    watch: {
      ignored: ["**/target/**", "**/src-tauri/**", "**/temp/**"],
    },
  },
});
