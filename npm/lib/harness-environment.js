/** npm exec exports its invocation selectors to children. They belong to Martty's
 * launcher, not a nested Harness runner (npx otherwise treats a package as a bin).
 * Keep registry/proxy/cache/auth settings and explicit Harness overrides intact.
 */
export function harnessEnvironment(overrides = {}, inherited = process.env) {
  const env = Object.fromEntries(Object.entries(inherited).filter(([key]) =>
    !/^npm_config_(package|call|workspace|workspaces|include_workspace_root)$/i.test(key)))
  return { ...env, ...overrides }
}
