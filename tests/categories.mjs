import assert from "node:assert/strict";
import {
  DEFAULT_CATEGORY,
  assignUnit,
  categoryOf,
  defaultCategories,
  groupUnits,
  MAX_CATEGORY_CHARS,
  normalizeCategories,
  validateCategoryName,
  withCategory,
  withoutCategory,
} from "../ui/categories.ts";

// 旧数据没有分类文件：所有子进程都落在默认分类下。
assert.deepEqual(normalizeCategories(undefined), {
  names: [DEFAULT_CATEGORY],
  assignments: {},
});
assert.deepEqual(normalizeCategories({}), normalizeCategories(null));
assert.deepEqual(groupUnits(defaultCategories(), ["a.service"]), [
  { name: DEFAULT_CATEGORY, units: ["a.service"] },
]);

// 手工修改过的文件：补齐默认分类、去重、丢弃非法名称与越界映射。
const repaired = normalizeCategories({
  names: ["Web", "Web", "  ", "x".repeat(MAX_CATEGORY_CHARS + 1), " 运维 ", "默认"],
  assignments: {
    "a.service": "Web",
    "b.service": "已删除",
    "c.service": "默认",
  },
});
assert.deepEqual(repaired.names, [DEFAULT_CATEGORY, "Web", "运维"]);
assert.deepEqual(repaired.assignments, { "a.service": "Web" });
assert.equal(categoryOf(repaired, "b.service"), DEFAULT_CATEGORY);
assert.equal(categoryOf(repaired, "c.service"), DEFAULT_CATEGORY);
assert.equal(categoryOf(repaired, "a.service"), "Web");
assert.equal(categoryOf(repaired, "未记录.service"), DEFAULT_CATEGORY);

// 分组保持分类顺序，空分类也保留为拖动目标。
assert.deepEqual(groupUnits(repaired, ["a.service", "b.service"]), [
  { name: DEFAULT_CATEGORY, units: ["b.service"] },
  { name: "Web", units: ["a.service"] },
  { name: "运维", units: [] },
]);

// 名称校验：空、重复、超长和控制字符都拒绝。
assert.equal(validateCategoryName("", [DEFAULT_CATEGORY]), "请输入分类名称");
assert.ok(validateCategoryName(DEFAULT_CATEGORY, repaired.names));
assert.ok(validateCategoryName("x".repeat(MAX_CATEGORY_CHARS + 1), repaired.names));
assert.ok(validateCategoryName("换\n行", repaired.names));
assert.equal(validateCategoryName("数据库", repaired.names), "");

// 新增分类：重复或非法时不改变原分类表。
assert.deepEqual(withCategory(repaired, "Web"), repaired);
assert.deepEqual(withCategory(repaired, "数据库").names, [
  DEFAULT_CATEGORY,
  "Web",
  "运维",
  "数据库",
]);

// 移动子进程：默认分类不记录映射，未知分类不改变任何数据。
const moved = assignUnit(repaired, "b.service", "Web");
assert.equal(categoryOf(moved, "b.service"), "Web");
assert.equal(categoryOf(assignUnit(moved, "b.service", DEFAULT_CATEGORY), "b.service"), DEFAULT_CATEGORY);
assert.deepEqual(assignUnit(repaired, "a.service", "不存在"), repaired);

// 删除分类：成员回落到默认分类，默认分类本身不能删除。
const removed = withoutCategory(repaired, "Web");
assert.deepEqual(removed.names, [DEFAULT_CATEGORY, "运维"]);
assert.deepEqual(removed.assignments, {});
assert.equal(categoryOf(removed, "a.service"), DEFAULT_CATEGORY);
assert.deepEqual(withoutCategory(repaired, DEFAULT_CATEGORY), repaired);
assert.deepEqual(withoutCategory(repaired, "不存在"), repaired);

// 纯函数不修改入参，界面可以直接用返回值覆盖状态。
assert.deepEqual(repaired.names, [DEFAULT_CATEGORY, "Web", "运维"]);
console.log("分类验证通过：旧数据兼容、归一化、名称校验、分组与移动。 ");
