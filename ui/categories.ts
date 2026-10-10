/** 界面分类的名称与归属，与后端 state/categories.json 保持一致。 */
export interface Categories {
  names: string[];
  assignments: Record<string, string>;
}
/** 一个分类及其子进程。 */
export interface CategoryGroup {
  name: string;
  units: string[];
}
/** 默认分类名称；旧数据、未分类和已失效映射的子进程都归入该分类。 */
export const DEFAULT_CATEGORY = "默认";
/** 分类名称允许的最大字符数，与后端一致。 */
export const MAX_CATEGORY_CHARS = 32;

/** 创建仅含默认分类的分类表。参数：无。返回：默认分类表。 */
export function defaultCategories(): Categories {
  return { names: [DEFAULT_CATEGORY], assignments: {} };
}

/** 判断名称是否为合法分类名称。参数：name 为已去空白的名称。返回：是否合法。 */
function validName(name: string): boolean {
  return (
    !!name && [...name].length <= MAX_CATEGORY_CHARS && !/\p{Cc}/u.test(name)
  );
}

/** 归一化分类数据，兼容旧数据与手工修改过的文件。参数：value 为后端或本地数据。返回：含默认分类且无重复、无越界映射的分类表。 */
export function normalizeCategories(value?: Partial<Categories> | null): Categories {
  const names = [DEFAULT_CATEGORY];
  for (const item of value?.names ?? []) {
    const name = String(item).trim();
    // 非法、重复的名称直接丢弃，保证界面始终可用。
    if (!validName(name) || names.includes(name)) continue;
    names.push(name);
  }
  const assignments: Record<string, string> = {};
  for (const [unit, category] of Object.entries(value?.assignments ?? {})) {
    // 指向已删除分类或默认分类的映射不记录，读取时回落到默认分类。
    if (category !== DEFAULT_CATEGORY && names.includes(category))
      assignments[unit] = category;
  }
  return { names, assignments };
}

/** 校验新增分类的名称。参数：name 为已去空白的名称，names 为现有分类。返回：错误说明；合法时返回空串。 */
export function validateCategoryName(name: string, names: string[]): string {
  if (!name) return "请输入分类名称";
  if ([...name].length > MAX_CATEGORY_CHARS)
    return `分类名称不能超过 ${MAX_CATEGORY_CHARS} 个字符`;
  if (/\p{Cc}/u.test(name)) return "分类名称不能包含控制字符";
  if (names.includes(name)) return `分类已存在：${name}`;
  return "";
}

/** 查询子进程所属分类。参数：categories 为分类表，unit 为子进程名称。返回：分类名称；未分类时返回默认分类。 */
export function categoryOf(categories: Categories, unit: string): string {
  const name = categories.assignments[unit];
  return name && categories.names.includes(name) ? name : DEFAULT_CATEGORY;
}

/** 按分类分组子进程。参数：categories 为分类表，units 为子进程名称。返回：按分类顺序排列的分组，空分类也会保留。 */
export function groupUnits(
  categories: Categories,
  units: string[],
): CategoryGroup[] {
  return categories.names.map((name) => ({
    name,
    units: units.filter((unit) => categoryOf(categories, unit) === name),
  }));
}

/** 新增分类。参数：categories 为分类表，name 为已去空白的名称。返回：新的分类表；名称非法或重复时原样返回。 */
export function withCategory(categories: Categories, name: string): Categories {
  if (validateCategoryName(name, categories.names)) return categories;
  return { ...categories, names: [...categories.names, name] };
}

/** 删除分类，其成员回落到默认分类。参数：categories 为分类表，name 为分类名称。返回：新的分类表。 */
export function withoutCategory(
  categories: Categories,
  name: string,
): Categories {
  if (name === DEFAULT_CATEGORY || !categories.names.includes(name))
    return categories;
  const assignments: Record<string, string> = {};
  for (const [unit, category] of Object.entries(categories.assignments))
    if (category !== name) assignments[unit] = category;
  return {
    names: categories.names.filter((item) => item !== name),
    assignments,
  };
}

/** 把子进程移动到指定分类。参数：categories 为分类表，unit 为子进程名称，category 为分类名称。返回：新的分类表；目标分类不存在时原样返回。 */
export function assignUnit(
  categories: Categories,
  unit: string,
  category: string,
): Categories {
  if (!categories.names.includes(category)) return categories;
  const assignments = { ...categories.assignments };
  // 默认分类不需要映射，删除映射即可让子进程回落到默认分类。
  if (category === DEFAULT_CATEGORY) delete assignments[unit];
  else assignments[unit] = category;
  return { ...categories, assignments };
}
