// esbuild's "text" loader turns an import of a ".css" file into its contents.
declare module "*.css" {
  const text: string;
  export default text;
}
