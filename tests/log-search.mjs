import assert from "node:assert/strict";
import { searchLogs } from "../ui/logSearch.ts";
/** 构建测试日志。参数：text 为行内容，index 为序号。返回：一条日志。 */
function record(text, index) {
  return {
    text,
    time: `2026-10-08T00:00:${String(index).padStart(2, "0")}Z`,
    source: "stdout",
    instance: 1,
  };
}
const records = [
  "before",
  "ERROR one",
  "middle",
  "error two",
  "after",
  "gap",
  "end",
  "ERROR last",
].map(record);
assert.deepEqual(
  searchLogs(records, "error", false, true, 1).rows.map((row) => row.index),
  [0, 1, 2, 3, 4, 6, 7],
);
assert.equal(
  searchLogs(records, "error", false, true, 1).rows[5].separator,
  true,
);
assert.equal(searchLogs(records, "error", false, true, 1).matches, 3);
assert.equal(searchLogs(records, "error", false, false, 0).matches, 1);
assert.equal(
  searchLogs(records, "^ERROR.*(one|last)$", true, false, 0).matches,
  2,
);
assert.ok(searchLogs(records, "[", true, false, 0).error);
assert.equal(searchLogs(records, "[", false, false, 0).error, "");
assert.equal(
  searchLogs(records, "", false, true, 0).rows.length,
  records.length,
);
assert.equal(searchLogs(records, "absent", false, true, 2).rows.length, 0);
console.log(
  "日志搜索验证通过：上下文合并、断组、大小写、正则、非法表达式、空搜索。",
);
