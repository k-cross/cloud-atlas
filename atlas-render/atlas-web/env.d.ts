// Bun's fullstack dev server lets HTML files be imported as route entries.
declare module "*.html" {
  const index: import("bun").HTMLBundle;
  export default index;
}
