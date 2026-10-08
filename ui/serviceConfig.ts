import type { ServiceDraft } from "./types";

/** 创建服务扩展表单。参数：无。返回：不限制资源、无依赖的草稿。 */
export function serviceDefaults(): ServiceDraft {
  return { after: [], before: [], requires: [], wants: [], healthAfter: [], stdoutDirectory: "", memoryMax: null, cpuQuota: null };
}

/** 生成关联及运行选项。参数：draft 为表单。返回：可追加的配置节。 */
export function serviceText(draft: ServiceDraft): string {
  const requires = [...new Set([...draft.requires, ...draft.after])];
  return `[Unit]\nRequires=${requires.join(" ")}\nWants=${draft.wants.join(" ")}\nAfter=${draft.after.join(" ")}\nBefore=${draft.before.join(" ")}\nHealthAfter=\nHealthAfter=${draft.healthAfter.join(" ")}\n\n[Service]\nStandardOutputDirectory=${draft.stdoutDirectory.trim().replaceAll("\\", "/").replaceAll("%", "%%")}\nMemoryMax=${draft.memoryMax === null ? "infinity" : `${draft.memoryMax}M`}\nCPUQuota=${draft.cpuQuota === null ? "" : `${draft.cpuQuota}%`}\n`;
}

/** 判断表单负责的指令。参数：section/key 为节和指令。返回：是否需要替换。 */
function managed(section: string, key: string): boolean {
  return (section === "Unit" && ["Requires", "Wants", "After", "Before", "HealthAfter"].includes(key)) ||
    (section === "Service" && ["StandardOutputDirectory", "MemoryMax", "CPUQuota"].includes(key));
}

/** 将表单写入当前文档并保留其他节。参数：text 为原文，draft 为表单。返回：更新的草稿。
 * 续行指令整体移除，避免遗留的参数被当作其他配置指令。 */
export function applyService(text: string, draft: ServiceDraft): string {
  const original = readService(text);
  const keys: Record<keyof ServiceDraft, string> = { after: "After", before: "Before", requires: "Requires", wants: "Wants", healthAfter: "HealthAfter", stdoutDirectory: "StandardOutputDirectory", memoryMax: "MemoryMax", cpuQuota: "CPUQuota" };
  const changed = new Set(Object.entries(keys).filter(([field]) => JSON.stringify(original[field as keyof ServiceDraft]) !== JSON.stringify(draft[field as keyof ServiceDraft])).map(([, key]) => key));
  if (!changed.size) return text;
  if (changed.has("After")) changed.add("Requires");
  let section = "";
  let skipping = false;
  let retaining = false;
  const lines = text.split(/\r?\n/).filter((line) => {
    if (/^\s*(?:[#;]|$)/.test(line)) return true;
    if (skipping) { skipping = /\\\s*$/.test(line); return false; }
    if (retaining) { retaining = /\\\s*$/.test(line); return true; }
    const heading = line.trim().match(/^\[(.+)\]$/);
    if (heading) section = heading[1] ?? "";
    const key = line.match(/^\s*([A-Za-z]+)\s*=/)?.[1];
    if (key && managed(section, key) && changed.has(key)) { skipping = /\\\s*$/.test(line); return false; }
    retaining = /\\\s*$/.test(line);
    return true;
  });
  const additions = serviceText(draft).split("\n").filter((line) => line.startsWith("[") || changed.has(line.split("=")[0] ?? "")).join("\n");
  return `${lines.join("\n").trimEnd()}\n\n${additions}\n`;
}

/** 合并续行并跳过整行注释。参数：text 为原始配置。返回：逻辑行；未完成的续行留待后端报错。 */
function logicalLines(text: string): string[] {
  const lines: string[] = [];
  let pending = "";
  for (const physical of text.replace(/^\uFEFF/, "").split(/\r?\n/)) {
    const line = physical.trim();
    if (!line || /^[#;]/.test(line)) continue;
    if (line.endsWith("\\")) { pending += `${line.slice(0, -1)} `; continue; }
    lines.push(pending + line);
    pending = "";
  }
  return lines;
}

/** 读取当前文档的服务选项。参数：text 为配置原文。返回：表单草稿；跨文件继承在保存时由后端合并。 */
export function readService(text: string): ServiceDraft {
  const draft = serviceDefaults();
  let section = "";
  for (const line of logicalLines(text)) {
    // 列表按重复指令累积，标量按最后赋值覆盖；HealthAfter 的空值独立重置。
    const heading = line.trim().match(/^\[(.+)\]$/);
    if (heading) section = heading[1] ?? "";
    const assignment = line.match(/^\s*([A-Za-z]+)\s*=(.*)$/);
    if (!assignment || !managed(section, assignment[1] ?? "")) continue;
    const key = assignment[1];
    const value = assignment[2]?.trim() ?? "";
    const fields: Record<string, "after" | "before" | "requires" | "wants" | "healthAfter"> = { After: "after", Before: "before", Requires: "requires", Wants: "wants", HealthAfter: "healthAfter" };
    const field = fields[key ?? ""];
    if (field) {
      if (field === "healthAfter" && !value) draft[field] = [];
      const names = Array.from(value.matchAll(/"([^"]*)"|'([^']*)'|(\S+)/g), (match) => match[1] ?? match[2] ?? match[3] ?? "").filter(Boolean);
      draft[field] = [...new Set([...draft[field], ...names])];
    } else if (key === "StandardOutputDirectory") draft.stdoutDirectory = value.replaceAll("%%", "%");
    else if (key === "CPUQuota") draft.cpuQuota = value ? Number(value.replace("%", "")) : null;
    else if (key === "MemoryMax") {
      const match = value.match(/^(\d+)([KMG]?)$/);
      draft.memoryMax = match ? Number(match[1]) * ({ K: 1 / 1024, M: 1, G: 1024, "": 1 / 1048576 }[match[2] as "M"] ?? 1) : null;
    }
  }
  return draft;
}
