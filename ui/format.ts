/** 格式化字节数。参数：value 为可空字节数。返回：带单位文本，缺失时为破折号。 */
export function bytes(value: number | null | undefined): string {
  if (value == null) return "—";
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  const index = Math.min(
    4,
    Math.max(0, Math.floor(Math.log2(Math.max(1, value)) / 10)),
  );
  return `${(value / 1024 ** index).toFixed(index ? 1 : 0)} ${units[index]}`;
}
/** 格式化资源百分比。参数：value 为可空百分比。返回：百分比或采样提示。 */
export function percent(value: number | null | undefined): string {
  return value == null ? "采样中" : `${value.toFixed(1)}%`;
}
