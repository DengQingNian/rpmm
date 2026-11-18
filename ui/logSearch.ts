/** 单条日志命中结果。context 表示上下文行，matched 表示命中行。 */
export interface LogRow {
  /** 唯一键，虚拟列表据此复用节点。 */
  key: string;
  /** 原始行号，从 0 开始。 */
  index: number;
  record: { time: string; source: string; text: string };
  matched: boolean;
  /** 上下文片段之间插入的分隔行。 */
  separator: boolean;
}

export interface LogSearchResult {
  rows: LogRow[];
  matches: number;
  error: string;
}

/** 编译用户输入的搜索文本。参数：query 为关键字，regex 表示按正则解释，ignoreCase 表示忽略大小写。
 * 返回：正则对象或 null（空查询），非法正则直接抛出异常。 */
function compile(
  query: string,
  regex: boolean,
  ignoreCase: boolean,
): RegExp | null {
  const text = query.trim();
  if (!text) return null;
  // 关键字按字面匹配，避免用户输入 . * 等字符意外扩大范围。
  const pattern = regex ? text : text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return new RegExp(pattern, ignoreCase ? "i" : "");
}

/** 在日志中搜索并附带前后上下文。参数：records 为原始日志、query 为关键字、regex 与 ignoreCase 为匹配选项、context 为前后行数。
 * 返回：可直接渲染的行集合；正则非法时返回错误说明。 */
export function searchLogs(
  records: { time: string; source: string; text: string }[],
  query: string,
  regex: boolean,
  ignoreCase: boolean,
  context: number,
): LogSearchResult {
  let pattern: RegExp | null;
  try {
    pattern = compile(query, regex, ignoreCase);
  } catch {
    return { rows: [], matches: 0, error: "正则表达式无效" };
  }
  if (!pattern) {
    return {
      rows: records.map((record, index) => ({
        key: `${index}`,
        index,
        record,
        matched: false,
        separator: false,
      })),
      matches: 0,
      error: "",
    };
  }
  const keep = new Set<number>();
  let matches = 0;
  records.forEach((record, index) => {
    if (!pattern!.test(record.text)) return;
    matches += 1;
    for (
      let cursor = Math.max(0, index - context);
      cursor <= Math.min(records.length - 1, index + context);
      cursor += 1
    )
      keep.add(cursor);
  });
  const rows: LogRow[] = [];
  let previous = -2;
  for (const index of [...keep].sort((a, b) => a - b)) {
    rows.push({
      key: `${index}`,
      index,
      record: records[index]!,
      matched: pattern.test(records[index]!.text),
      separator: index !== previous + 1,
    });
    previous = index;
  }
  return { rows, matches, error: "" };
}
