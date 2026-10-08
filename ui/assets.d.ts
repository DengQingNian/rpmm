/** SVG 文件在前端构建后对应的静态资源地址。 */
declare module "*.svg" {
  const source: string;
  export default source;
}
