// SOURCE: koharu feat/openrouter-provider-routing-pr @ dc6a680a, packages/koharu/components/preferences/PipelinePreferences.tsx (OpenRouterHost)
// STATUS: shipped in PR #1534; checked in the debug build UI.
// Port this to the proxy's language for the `hosts` CLI (T7).

type Host = { tag: string; name: string; quantization: string | null }

/** What tells a host's endpoints apart: its quantization and the region or tier the rest of its
 * tag names, as `Sail Research (fp4, us)` for `sail-research/fp4/us`. */
export function label(host: Host): string {
  const parts = [host.quantization, ...host.tag.split('/').slice(1)].filter(
    (part, index, all): part is string => !!part && all.indexOf(part) === index,
  )
  return parts.length > 0 ? `${host.name} (${parts.join(', ')})` : host.name
}

/** Hosts sorted by label, as the dropdown showed them. */
export function sorted(hosts: Host[]): [tag: string, label: string][] {
  return hosts.map((h) => [h.tag, label(h)] as [string, string]).sort(([, a], [, b]) => a.localeCompare(b))
}
