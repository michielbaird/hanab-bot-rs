// esbuild's "text" loader turns an import of an ".svg" file into its markup.
declare module "*.svg" {
  const markup: string;
  export default markup;
}
