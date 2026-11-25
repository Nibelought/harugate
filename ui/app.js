/* Provider pins: control panel for harugate. Vanilla JS, no dependencies.
   All text from the API is written with textContent, never innerHTML. */
(function () {
  'use strict';

  const $ = (id) => document.getElementById(id);
  const OFFICIAL_UPSTREAM = 'https://openrouter.ai/api/v1';
  const DEFAULT_LISTEN = '127.0.0.1:8787';
  const TABS = ['routing', 'requests', 'advanced', 'raw'];

  const state = {
    status: null,
    saved: null,        // last config the server confirmed
    draft: null,        // what the form shows
    configError: null,
    raw: { loaded: false, path: '', saved: '' },
    editing: null,      // model id whose tag is being edited inline
    hosts: null,        // {model, loading, error, data}
    tab: 'routing',
    busy: false,
  };
  let hostsToken = 0;

  /* ---------- small helpers ---------- */

  // Build an element. attrs: class, text, on<event> handlers, anything else is an attribute.
  function h(tag, attrs, ...kids) {
    const e = document.createElement(tag);
    for (const [k, v] of Object.entries(attrs || {})) {
      if (v == null || v === false) continue;
      if (k === 'class') e.className = v;
      else if (k === 'text') e.textContent = v;
      else if (k.startsWith('on')) e.addEventListener(k.slice(2), v);
      else e.setAttribute(k, v === true ? '' : v);
    }
    for (const kid of kids) if (kid != null) e.append(kid);
    return e;
  }
  const code = (t) => h('code', { text: t });
  const clone = (o) => JSON.parse(JSON.stringify(o));

  class ApiError extends Error {
    constructor(message, code, status) { super(message); this.code = code; this.status = status; }
  }

  async function api(method, path, body) {
    const init = { method, headers: {} };
    if (method !== 'GET') {
      init.headers['Content-Type'] = 'application/json';
      init.headers['X-Proxy-UI'] = '1';
      init.body = JSON.stringify(body === undefined ? {} : body);
    }
    let res;
    try {
      res = await fetch(path, init);
    } catch (e) {
      throw new ApiError("Can't reach the proxy. Is it still running?", 'network', 0);
    }
    let data = null;
    try { data = await res.json(); } catch (e) { /* not JSON */ }
    if (!res.ok) {
      const err = data && data.error;
      throw new ApiError((err && err.message) || ('The proxy answered with status ' + res.status + '.'),
        err && err.code, res.status);
    }
    return data;
  }
  const get = (p) => api('GET', p);
  const put = (p, b) => api('PUT', p, b);

  function announce(text) { $('sr-status').textContent = ''; setTimeout(() => { $('sr-status').textContent = text; }, 30); }

  /* ---------- risk rules (same as the server) ---------- */

  function upstreamRisky(u) {
    try { const url = new URL(u); return !(url.protocol === 'https:' && url.hostname === 'openrouter.ai'); }
    catch (e) { return true; }
  }
  function listenRisky(l) {
    let host = String(l).trim().toLowerCase();
    if (host.startsWith('[')) host = host.slice(0, host.indexOf(']') + 1);
    else if (host.includes(':')) host = host.slice(0, host.lastIndexOf(':'));
    return !(host === 'localhost' || host === '[::1]' || /^127(\.\d{1,3}){3}$/.test(host));
  }

  /* ---------- draft vs saved ---------- */

  const sortedPins = (pins) => Object.entries(pins).sort((a, b) => a[0].localeCompare(b[0]));

  function dirtyParts() {
    if (!state.saved || !state.draft) return { routing: false, advanced: false, raw: false };
    const s = state.saved, d = state.draft;
    return {
      routing: (s.routing || null) !== (d.routing || null)
        || !!s.require_parameters !== !!d.require_parameters
        || JSON.stringify(sortedPins(s.pins)) !== JSON.stringify(sortedPins(d.pins)),
      advanced: s.upstream !== d.upstream || s.listen !== d.listen,
      raw: state.raw.loaded && $('raw-text').value !== state.raw.saved,
    };
  }

  function refreshDirty() {
    const p = dirtyParts();
    $('dot-routing').hidden = !p.routing;
    $('dot-advanced').hidden = !p.advanced;
    $('dot-raw').hidden = !p.raw;
    $('raw-chip').hidden = !p.raw;
    $('raw-save').disabled = state.busy || !p.raw;
    const struct = p.routing || p.advanced;
    $('chip-struct').hidden = !struct;
    $('chip-raw').hidden = !p.raw;
    $('bar-actions').hidden = !struct;
    $('save-btn').disabled = state.busy;
    $('discard-btn').disabled = state.busy;
    $('savebar').hidden = !(struct || p.raw || !$('fb').hidden);
    return p;
  }

  /* ---------- feedback ---------- */

  let fbTimer = 0;
  function say(kind, text) {
    clearTimeout(fbTimer);
    $('fb').hidden = false;
    $('savebar').hidden = false;
    $('live-ok').textContent = '';
    $('live-err').textContent = '';
    // Unhide first, then fill, so screen readers announce the change.
    setTimeout(() => {
      (kind === 'error' ? $('live-err') : $('live-ok')).textContent = text;
    }, 50);
    if (kind !== 'error') fbTimer = setTimeout(clearFeedback, 9000);
  }
  function clearFeedback() {
    clearTimeout(fbTimer);
    $('fb').hidden = true;
    $('live-ok').textContent = '';
    $('live-err').textContent = '';
    refreshDirty();
  }

  /* ---------- confirmation dialog ---------- */

  function confirmDialog({ title, paragraphs = [], items = [], detail, confirmLabel }) {
    const dlg = $('confirm-dialog');
    const opener = document.activeElement;
    $('dlg-title').textContent = title;
    const body = $('dlg-body');
    body.replaceChildren(...paragraphs.map((t) => h('p', { text: t })));
    if (items.length) body.append(h('ul', null, ...items.map((t) => h('li', { text: t }))));
    if (detail) body.append(h('pre', { class: 'errtext', text: detail }));
    $('dlg-confirm').textContent = confirmLabel;
    return new Promise((resolve) => {
      let answer = false;
      const onClose = () => {
        dlg.removeEventListener('close', onClose);
        $('dlg-cancel').onclick = $('dlg-confirm').onclick = null;
        if (opener && opener.focus) opener.focus();
        resolve(answer);
      };
      $('dlg-cancel').onclick = () => dlg.close();
      $('dlg-confirm').onclick = () => { answer = true; dlg.close(); };
      dlg.addEventListener('close', onClose);
      dlg.showModal();
      $('dlg-cancel').focus();
    });
  }

  /* ---------- rendering: status and connect ---------- */

  const ROUTING_NAMES = { price: 'Price (cheapest first)', throughput: 'Throughput (fastest tokens per second)',
    latency: 'Latency (quickest first token)' };

  function renderStatus() {
    const s = state.status;
    if (!s) return;
    $('version').textContent = 'Version ' + s.version;
    $('st-path').textContent = s.config_path;
    $('st-listen').textContent = s.listen_running || s.listen;
    $('st-upstream').textContent = s.upstream;
    $('st-routing').textContent = ROUTING_NAMES[s.routing] || 'Balanced (OpenRouter default)';
    $('st-pins').textContent = String(s.pins);
    $('st-key').replaceChildren(s.api_key_set
      ? h('span', { class: 'flag flag-ok', text: '\u{2713} Set' })
      : h('span', { class: 'flag', text: 'Not set' }));

    $('base-url').textContent = s.base_url;
    $('key-hint').replaceChildren(h('strong', { text: 'API key: ' }), s.api_key_set
      ? 'leave it empty. The proxy already has an OpenRouter key (OPENROUTER_API_KEY) and uses it instead of whatever Koharu sends.'
      : "paste your OpenRouter key. The proxy passes it through. (Or set OPENROUTER_API_KEY for the proxy and leave Koharu's field empty.)");

    state.configError = s.config_error || null;
    $('config-error').hidden = !state.configError;
    $('config-error-msg').textContent = state.configError || '';

    $('restart-notice').hidden = !s.restart_required;
    if (s.restart_required) {
      $('restart-text').textContent = 'The listen address changed'
        + (s.listen_running ? ' (running on ' + s.listen_running + ', set to ' + s.listen + ')' : '')
        + '. It applies after you restart the proxy.';
    }

    const warn = [];
    if (!s.upstream_is_official) warn.push('The upstream is not openrouter.ai, so it receives your OpenRouter API key.');
    if (!s.listen_is_loopback) warn.push('The proxy listens on an address other than this computer. Anyone on your network could use your key.');
    $('status-warnings').replaceChildren(...warn.map((t) =>
      h('div', { class: 'banner banner-warn' }, h('p', { class: 'banner-title', text: 'Warning' }), h('p', { text: t }))));
  }

  /* ---------- rendering: routing form ---------- */

  function renderRouting() {
    const val = state.draft.routing || 'balanced';
    document.querySelectorAll('input[name="routing"]').forEach((r) => { r.checked = r.value === val; });
    $('require-params').checked = !!state.draft.require_parameters;
  }

  function renderAdvanced() {
    $('in-upstream').value = state.draft.upstream;
    $('in-listen').value = state.draft.listen;
    updateWarnings();
  }
  function updateWarnings() {
    $('warn-upstream').hidden = !upstreamRisky(state.draft.upstream.trim());
    $('warn-listen').hidden = !listenRisky(state.draft.listen.trim());
  }

  /* ---------- rendering: pins ---------- */

  const rowButton = (act, model) => $('pins-body').querySelector('button[data-act="' + act + '"][data-model="' + CSS.escape(model) + '"]');

  function renderPins() {
    const entries = sortedPins(state.draft.pins);
    $('pins-empty').hidden = entries.length > 0;
    $('pins-table-wrap').hidden = entries.length === 0;
    $('pins-body').replaceChildren(...entries.map(([model, tag]) => {
      const savedTag = state.saved.pins[model];
      const badge = savedTag === undefined ? 'New' : savedTag !== tag ? 'Changed' : '';
      const editing = state.editing === model;
      const tagCell = editing
        ? h('td', null, h('input', { type: 'text', class: 'tag-input', value: tag, 'aria-label': 'Tag for ' + model,
            spellcheck: 'false', autocomplete: 'off', 'data-edit-input': '1',
            onkeydown: (ev) => {
              if (ev.key === 'Enter') { ev.preventDefault(); finishEdit(model, ev.target.value, true); }
              else if (ev.key === 'Escape') { ev.preventDefault(); finishEdit(model, null, false); }
            } }))
        : h('td', null, code(tag), badge ? h('span', { class: 'badge', text: badge }) : null);
      const actions = editing
        ? [h('button', { type: 'button', class: 'btn btn-small', text: 'Done', 'aria-label': 'Save tag for ' + model,
              onclick: () => finishEdit(model, document.querySelector('[data-edit-input]').value, true) }),
           h('button', { type: 'button', class: 'btn btn-small', text: 'Cancel', 'aria-label': 'Cancel editing ' + model,
              onclick: () => finishEdit(model, null, false) })]
        : [h('button', { type: 'button', class: 'btn btn-small', 'data-act': 'edit', 'data-model': model, text: 'Edit',
              'aria-label': 'Edit pin for ' + model, onclick: () => startEdit(model) }),
           h('button', { type: 'button', class: 'btn btn-small', text: 'Find hosts', 'aria-label': 'Find hosts for ' + model,
              onclick: () => findHosts(model) }),
           h('button', { type: 'button', class: 'btn btn-small', 'data-act': 'remove', 'data-model': model, text: 'Remove',
              'aria-label': 'Remove pin for ' + model, onclick: () => removePin(model) })];
      return h('tr', null, h('td', null, code(model)), tagCell, h('td', null, h('div', { class: 'cell-actions' }, ...actions)));
    }));
    refreshDirty();
  }

  function startEdit(model) {
    state.editing = model;
    renderPins();
    const input = document.querySelector('[data-edit-input]');
    if (input) { input.focus(); input.select(); }
  }
  function finishEdit(model, value, apply) {
    const tag = (value || '').trim();
    if (apply && tag) { state.draft.pins[model] = tag; announce('Pin for ' + model + ' set to ' + tag + '. Not saved yet.'); }
    state.editing = null;
    renderPins();
    renderHosts();
    const btn = rowButton('edit', model);
    if (btn) btn.focus();
  }
  function removePin(model) {
    const keys = sortedPins(state.draft.pins).map((e) => e[0]);
    const i = keys.indexOf(model);
    delete state.draft.pins[model];
    renderPins();
    renderHosts();
    announce('Removed the pin for ' + model + '. Not saved yet.');
    const next = keys[i + 1] || keys[i - 1];
    const target = next ? rowButton('remove', next) : $('add-model');
    if (target) target.focus();
  }

  function addPin(ev) {
    ev.preventDefault();
    const model = $('add-model').value.trim();
    const tag = $('add-tag').value.trim();
    const err = $('add-error');
    if (!model || !tag) { err.textContent = 'Enter both a model id and a tag.'; (model ? $('add-tag') : $('add-model')).focus(); return; }
    if (!/^[^\s/]+\/[^\s/]+$/.test(model)) { err.textContent = 'Model ids look like author/slug, for example deepseek/deepseek-v4.1-flash-1017.'; $('add-model').focus(); return; }
    err.textContent = '';
    const replaced = model in state.draft.pins;
    state.draft.pins[model] = tag;
    $('add-model').value = '';
    $('add-tag').value = '';
    renderPins();
    renderHosts();
    announce((replaced ? 'Updated' : 'Added') + ' the pin for ' + model + '. Not saved yet.');
    $('add-model').focus();
  }

  /* ---------- hosts ---------- */

  async function findHosts(model) {
    const token = ++hostsToken;
    state.hosts = { model, loading: true };
    renderHosts();
    $('hosts-panel').scrollIntoView({ block: 'nearest' });
    $('hosts-title').focus({ preventScroll: true });
    try {
      const data = await get('/api/hosts?model=' + encodeURIComponent(model));
      if (token !== hostsToken) return;
      state.hosts = { model, data };
    } catch (e) {
      if (token !== hostsToken) return;
      state.hosts = { model, error: e.message };
    }
    renderHosts();
  }

  const yesNo = (flag) => h('span', { class: flag ? 'yes' : 'no', text: flag ? 'Yes' : 'No' });

  function renderHosts() {
    const hs = state.hosts;
    $('hosts-panel').hidden = !hs;
    if (!hs) return;
    $('hosts-title').textContent = 'Hosts for ' + hs.model;
    const status = $('hosts-status'), err = $('hosts-error'), wrap = $('hosts-wrap');
    err.hidden = true; wrap.hidden = true; status.textContent = '';
    if (hs.loading) { status.textContent = 'Asking OpenRouter...'; return; }
    if (hs.error) { err.hidden = false; err.textContent = hs.error; return; }
    const hosts = hs.data.hosts || [];
    if (!hosts.length) {
      status.textContent = "OpenRouter lists no pinnable hosts for this model. (Model ids with a ~ alias can't be pinned.)";
      return;
    }
    const current = state.draft.pins[hs.model];
    status.textContent = hosts.length + ' hosts listed. ' + (current
      ? 'Pinned to ' + current + (current !== state.saved.pins[hs.model] ? ' (not saved yet).' : '.')
      : 'This model has no pin.');
    wrap.hidden = false;
    $('hosts-body').replaceChildren(...hosts.map((host) => {
      const pinned = current === host.tag;
      const cell = h('td', null, host.label || host.name);
      if (!host.tools) {
        cell.append(h('span', { class: 'caveat', text: "No tools support. If you pin this host, every request from Koharu's in-app agent fails with a 404." }));
      }
      return h('tr', { class: pinned ? 'is-pinned' : '' },
        cell, h('td', null, code(host.tag)),
        h('td', { text: host.quantization || 'Not stated' }),
        h('td', null, yesNo(host.tools)), h('td', null, yesNo(host.structured_outputs)),
        h('td', null, h('button', { type: 'button', class: 'btn btn-small', 'data-tag': host.tag,
          'aria-disabled': pinned ? 'true' : null,
          'aria-label': (pinned ? 'Pinned: ' : 'Pin this: ') + (host.label || host.tag),
          text: pinned ? 'Pinned' : 'Pin this',
          onclick: () => pinHost(hs.model, host.tag, pinned) })));
    }));
  }

  function pinHost(model, tag, already) {
    if (already) return;
    state.draft.pins[model] = tag;
    renderPins();
    renderHosts();
    announce('Pinned ' + model + ' to ' + tag + '. Not saved yet.');
    const btn = $('hosts-body').querySelector('button[data-tag="' + CSS.escape(tag) + '"]');
    if (btn) btn.focus();
  }

  /* ---------- models datalist ---------- */

  async function loadModels() {
    try {
      const data = await get('/api/models');
      const frag = document.createDocumentFragment();
      for (const id of data.models || []) frag.append(h('option', { value: id }));
      $('model-list').replaceChildren(frag);
    } catch (e) {
      $('models-note').textContent = "Model suggestions aren't available right now. You can still type an id, like author/slug.";
    }
  }

  /* ---------- requests ---------- */

  function humanRouting(r) {
    if (r.startsWith('pin:')) return 'Pinned to ' + r.slice(4);
    if (r.startsWith('sort:')) return 'Sorted by ' + r.slice(5);
    if (r === 'balanced') return 'Balanced';
    if (r === 'none') return 'Not routed';
    return r;
  }
  const fmtLatency = (ms) => ms < 1000 ? ms + ' ms' : (ms / 1000).toFixed(ms < 10000 ? 2 : 1) + ' s';
  function fmtAgo(ms) {
    const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
    if (s < 5) return 'just now';
    if (s < 60) return s + ' s ago';
    if (s < 3600) return Math.floor(s / 60) + ' min ago';
    return Math.floor(s / 3600) + ' h ago';
  }

  function renderRequests(data) {
    const list = data.requests || [];
    $('req-intro').textContent = 'The last ' + (data.capacity || 100) + ' requests from Koharu, newest first. No keys, prompts or images are stored. The list is kept in memory only and clears when the proxy restarts.';
    $('req-empty').hidden = list.length > 0;
    $('req-wrap').hidden = list.length === 0;
    $('req-body').replaceChildren(...list.map((r) => {
      const ok = r.status >= 200 && r.status < 300;
      const bad = r.status >= 400;
      const when = new Date(r.time_ms);
      return h('tr', { class: bad ? 'row-err' : '' },
        h('td', { title: when.toLocaleString() }, when.toLocaleTimeString(), h('span', { class: 'sub', text: fmtAgo(r.time_ms) })),
        h('td', null, r.model ? code(r.model) : h('span', { class: 'sub', text: 'No model' }), h('span', { class: 'sub', text: r.method + ' ' + r.path })),
        h('td', { text: humanRouting(r.routing) }),
        h('td', null, h('span', { class: 'st ' + (ok ? 'st-ok' : bad ? 'st-err' : 'st-other'), text: String(r.status) },
          h('span', { class: 'sr-only', text: ok ? ' (success)' : bad ? ' (error)' : '' }))),
        h('td', { text: fmtLatency(r.latency_ms) }));
    }));
  }

  let reqBusy = false, statBusy = false, reqTimer = 0, statTimer = 0;
  async function fetchRequests() {
    if (reqBusy) return;
    reqBusy = true;
    try { renderRequests(await get('/api/requests')); $('req-note').hidden = true; }
    catch (e) { $('req-note').hidden = false; $('req-note').textContent = "Can't refresh the list right now. " + e.message; }
    finally { reqBusy = false; }
  }
  async function fetchStatus() {
    if (statBusy) return;
    statBusy = true;
    try { state.status = await get('/api/status'); renderStatus(); $('conn-note').hidden = true; }
    catch (e) { $('conn-note').hidden = false; $('conn-note').textContent = 'Lost connection to the proxy. Trying again...'; }
    finally { statBusy = false; }
  }
  function updatePolling() {
    clearInterval(reqTimer); clearInterval(statTimer);
    if (document.hidden || !state.status) return;
    statTimer = setInterval(fetchStatus, 10000);
    if (state.tab === 'requests') { fetchRequests(); reqTimer = setInterval(fetchRequests, 2000); }
  }

  /* ---------- raw file ---------- */

  async function loadRaw() {
    $('raw-loading').hidden = false;
    $('raw-error').hidden = true;
    try {
      const data = await get('/api/config/raw');
      state.raw = { loaded: true, path: data.path, saved: data.text };
      $('raw-path').textContent = data.path;
      $('raw-text').value = data.text;
    } catch (e) {
      $('raw-error').hidden = false;
      $('raw-error').textContent = e.message;
    } finally { $('raw-loading').hidden = true; refreshDirty(); }
  }

  async function reloadRaw() {
    if (dirtyParts().raw && !(await confirmDialog({ title: 'Reload the file from disk?',
      paragraphs: ['Your edits in the box will be lost.'], confirmLabel: 'Reload and lose edits' }))) return;
    await loadRaw();
    announce('Reloaded the file from disk.');
  }

  // Run a save; if the server asks for a risk confirmation, ask the user and resend once.
  async function withConfirm(doSend, askAgain) {
    try { return await doSend(false); }
    catch (e) {
      if (e.code !== 'confirmation_required') throw e;
      if (!(await askAgain(e))) return null;
      return doSend(true);
    }
  }

  function setBusy(on) { state.busy = on; refreshDirty(); $('raw-reload').disabled = on; }

  async function saveRaw() {
    if (state.busy) return;
    const p = dirtyParts();
    if (p.routing || p.advanced) {
      const ok = await confirmDialog({ title: 'Replace unsaved settings?',
        paragraphs: ['Saving the file reloads all settings from it. Your unsaved changes in Routing & pins and Advanced will be lost.'],
        confirmLabel: 'Save file and lose those changes' });
      if (!ok) return;
    }
    const text = $('raw-text').value;
    $('raw-error').hidden = true;
    setBusy(true);
    try {
      const res = await withConfirm(
        (c) => put('/api/config/raw', c ? { text, confirm_risky: true } : { text }),
        (e) => confirmDialog({ title: 'This file has risky settings', confirmLabel: 'Save anyway', detail: e.message,
          paragraphs: ['The file points the proxy somewhere that could expose your OpenRouter key. Only continue if you know exactly why.'] }));
      if (!res) { say('info', 'Not saved.'); return; }
      state.raw.saved = res.text;
      $('raw-text').value = res.text;
      state.saved = clone(res.config);
      state.draft = clone(res.config);
      state.editing = null;
      if (state.status) state.status.restart_required = !!res.restart_required;
      renderAll();
      fetchStatus();
      say('ok', 'Saved. New requests use it now.' + (res.restart_required ? ' A changed listen address applies after you restart the proxy.' : ''));
    } catch (e) {
      $('raw-error').hidden = false;
      $('raw-error').textContent = e.message;
      $('raw-error').scrollIntoView({ block: 'nearest' });
    } finally { setBusy(false); }
  }

  /* ---------- saving structured settings ---------- */

  async function saveSettings() {
    if (state.busy) return;
    const d = state.draft;
    const body = { listen: d.listen.trim(), upstream: d.upstream.trim(), routing: d.routing || null,
      require_parameters: !!d.require_parameters, pins: d.pins };
    if (!body.listen || !body.upstream) { say('error', "Upstream and listen address can't be empty. Open the Advanced tab to fix them."); return; }

    if (dirtyParts().raw) {
      const ok = await confirmDialog({ title: 'Replace edits in the Raw file tab?',
        paragraphs: ['Saving rewrites the config file, so your unsaved edits in the Raw file tab will be lost.'],
        confirmLabel: 'Save and lose file edits' });
      if (!ok) return;
    }

    // Only a risky value that differs from the saved one needs a fresh confirmation.
    const risks = [];
    if (upstreamRisky(body.upstream) && body.upstream !== state.saved.upstream)
      risks.push('Upstream: "' + body.upstream + '" is not https://openrouter.ai. That server will receive your OpenRouter API key with every request.');
    if (listenRisky(body.listen) && body.listen !== state.saved.listen)
      risks.push('Listen address: "' + body.listen + '" is not limited to this computer. Anyone who can reach it on your network could use your OpenRouter key and spend your credit.');
    const riskDialog = (items, detail) => confirmDialog({ title: 'Are you sure about this change?', items, detail,
      paragraphs: ['You are about to save a setting that can expose your OpenRouter key.'], confirmLabel: 'Yes, save this risky setting' });
    if (risks.length && !(await riskDialog(risks))) { say('info', 'Not saved. Nothing was changed.'); return; }

    setBusy(true);
    try {
      const res = await withConfirm(
        (c) => put('/api/config', risks.length || c ? { ...body, confirm_risky: true } : body),
        (e) => riskDialog([], e.message));
      if (!res) { say('info', 'Not saved. Nothing was changed.'); return; }
      const listenChanged = state.saved.listen !== res.config.listen;
      state.saved = clone(res.config);
      state.draft = clone(res.config);
      state.editing = null;
      if (state.status) state.status.restart_required = !!res.restart_required;
      state.raw.loaded = false;                  // the file text changed on disk: reload when the tab opens
      if (state.tab === 'raw') await loadRaw();
      renderAll();
      fetchStatus();
      say('ok', 'Saved. New requests use it now.' + (listenChanged || res.restart_required ? ' The listen address changes after you restart the proxy.' : ''));
    } catch (e) {
      say('error', e.message);
    } finally { setBusy(false); }
  }

  function discard() {
    state.draft = clone(state.saved);
    state.editing = null;
    renderAll();
    announce('Changes discarded.');
  }

  /* ---------- tabs ---------- */

  function selectTab(name, focus) {
    state.tab = name;
    for (const t of TABS) {
      const tab = $('tab-' + t), on = t === name;
      tab.setAttribute('aria-selected', String(on));
      tab.tabIndex = on ? 0 : -1;
      $('panel-' + t).hidden = !on;
      if (on && focus) tab.focus();
    }
    if (name === 'raw' && !state.raw.loaded) loadRaw();
    updatePolling();
  }

  function onTabKey(ev) {
    const i = TABS.indexOf(state.tab);
    let n = -1;
    if (ev.key === 'ArrowRight') n = (i + 1) % TABS.length;
    else if (ev.key === 'ArrowLeft') n = (i + TABS.length - 1) % TABS.length;
    else if (ev.key === 'Home') n = 0;
    else if (ev.key === 'End') n = TABS.length - 1;
    if (n < 0) return;
    ev.preventDefault();
    selectTab(TABS[n], true);
  }

  /* ---------- copy ---------- */

  async function copyBaseUrl() {
    const text = $('base-url').textContent;
    let ok = false;
    try { await navigator.clipboard.writeText(text); ok = true; } catch (e) { /* use the fallback */ }
    if (!ok) {
      const ta = h('textarea', { class: 'offscreen', 'aria-hidden': 'true', tabindex: '-1', readonly: true });
      ta.value = text;
      document.body.append(ta);
      ta.select();
      try { ok = document.execCommand('copy'); } catch (e) { ok = false; }
      ta.remove();
      $('copy-btn').focus();
    }
    $('copy-note').textContent = ok ? 'Copied.' : 'Copy failed. Select the address and copy it by hand.';
    setTimeout(() => { $('copy-note').textContent = ''; }, 4000);
  }

  /* ---------- wiring ---------- */

  function renderAll() {
    renderStatus();
    renderRouting();
    renderAdvanced();
    renderPins();
    renderHosts();
    refreshDirty();
  }

  function bind() {
    document.querySelectorAll('input[name="routing"]').forEach((r) => r.addEventListener('change', () => {
      state.draft.routing = r.value === 'balanced' ? null : r.value; refreshDirty();
    }));
    $('require-params').addEventListener('change', (e) => { state.draft.require_parameters = e.target.checked; refreshDirty(); });
    $('in-upstream').addEventListener('input', (e) => { state.draft.upstream = e.target.value; updateWarnings(); refreshDirty(); });
    $('in-listen').addEventListener('input', (e) => { state.draft.listen = e.target.value; updateWarnings(); refreshDirty(); });
    $('reset-upstream').addEventListener('click', () => { state.draft.upstream = OFFICIAL_UPSTREAM; renderAdvanced(); refreshDirty(); $('in-upstream').focus(); });
    $('reset-listen').addEventListener('click', () => { state.draft.listen = DEFAULT_LISTEN; renderAdvanced(); refreshDirty(); $('in-listen').focus(); });
    $('add-form').addEventListener('submit', addPin);
    $('find-btn').addEventListener('click', () => {
      const m = $('add-model').value.trim();
      if (!m) { $('add-error').textContent = 'Enter a model id first, then find its hosts.'; $('add-model').focus(); return; }
      $('add-error').textContent = '';
      findHosts(m);
    });
    $('save-btn').addEventListener('click', saveSettings);
    $('discard-btn').addEventListener('click', discard);
    $('raw-save').addEventListener('click', saveRaw);
    $('raw-reload').addEventListener('click', reloadRaw);
    $('raw-text').addEventListener('input', refreshDirty);
    $('fb-dismiss').addEventListener('click', clearFeedback);
    $('copy-btn').addEventListener('click', copyBaseUrl);
    $('goto-raw').addEventListener('click', () => selectTab('raw', true));
    $('retry-btn').addEventListener('click', init);
    for (const t of TABS) {
      $('tab-' + t).addEventListener('click', () => selectTab(t, false));
      $('tab-' + t).addEventListener('keydown', onTabKey);
    }
    document.addEventListener('visibilitychange', () => { updatePolling(); if (!document.hidden && state.status) fetchStatus(); });
    window.addEventListener('beforeunload', (e) => {
      const p = dirtyParts();
      if (p.routing || p.advanced || p.raw) { e.preventDefault(); e.returnValue = ''; }
    });
  }

  async function init() {
    $('load-error').hidden = true;
    $('loading').hidden = false;
    try {
      const [status, cfg] = await Promise.all([get('/api/status'), get('/api/config')]);
      if (cfg.config_error && !status.config_error) status.config_error = cfg.config_error;
      state.status = status;
      state.saved = clone(cfg.config);
      state.draft = clone(cfg.config);
    } catch (e) {
      $('loading').hidden = true;
      $('load-error').hidden = false;
      $('load-error-msg').textContent = e.message;
      return;
    }
    $('loading').hidden = true;
    $('app').hidden = false;
    renderAll();
    selectTab(state.tab, false);
    loadModels();
  }

  bind();
  init();
})();
