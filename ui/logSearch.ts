import type { LogRecord } from "./types";
/** 一条搜索结果及其原始位置。 */
export interface LogRow {
  key: string;
  index: number;
  record: LogRecord;
  matched: boolean;
  separator: boolean;
}
/** 检索日志并合并相邻上下文范围。参数：records 为最近日志，query 为关键字，regex/ignoreCase 为匹配方式，context 为 -C 前后行数。返回：结果、命中数量及表达式错误。
 * 上下文从来源筛选后的最近记录中取，重叠范围只显示一次。 */
export function searchLogs(
  records: LogRecord[],
  query: string,
  regex: boolean,
  ignoreCase: boolean,
  context: number,
): { rows: LogRow[]; matches: number; error: string } {
  let expression: RegExp | undefined;
  try {
    if (query && regex) expression = new RegExp(query, ignoreCase ? "i" : "");
  } catch (error) {
    return { rows: [], matches: 0, error: `正则表达式错误：${String(error)}` };
  }
  const needle = ignoreCase ? query.toLocaleLowerCase() : query;
  const hits = records.map(
    (record) =>
      !query ||
      (expression
        ? expression.test(record.text)
        : (ignoreCase ? record.text.toLocaleLowerCase() : record.text).includes(
            needle,
          )),
  );
  const included = new Set<number>();
  const radius = Math.min(100, Math.max(0, Math.floor(context || 0)));
  hits.forEach((hit, index) => {
    if (!hit) return;
    for (
      let i = Math.max(0, index - radius);
      i <= Math.min(records.length - 1, index + radius);
      i++
    )
      included.add(i);
  });
  let previous = -1;
  const rows: LogRow[] = [];
  for (const index of [...included].sort((a, b) => a - b)) {
    const record = records[index]!;
    rows.push({
      key: `${record.time}-${record.instance}-${index}`,
      index,
      record,
      matched: !!query && hits[index]!,
      separator: previous >= 0 && index > previous + 1,
    });
    previous = index;
  }
  return { rows, matches: query ? hits.filter(Boolean).length : 0, error: "" };
}
