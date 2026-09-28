// Gloss 站点脚本：下载直链 + 本机架构识别 + 界面微交互。
// 下载契约：同源 manifest.json（CI 注入）提供 channels.stable[<triple>] 的
// dmg_url / url / size；取不到或校验不过则整体降级到 GitHub Releases。
const RELEASES_LATEST = 'https://github.com/Losmli010/gloss/releases/latest';
const ARCHES = {
  'aarch64-apple-darwin': { label: 'Apple Silicon' },
  'x86_64-apple-darwin': { label: 'Intel 芯片' }
};
let arch = 'aarch64-apple-darwin';
let userPicked = false;
let manifest = null;

const $ = (id) => document.getElementById(id);
const $$ = (sel, root = document) => Array.from(root.querySelectorAll(sel));

function formatSize(bytes) {
  const mb = bytes / (1024 * 1024);
  return mb >= 1 ? `约 ${mb.toFixed(1)} MB` : `约 ${(bytes / 1024).toFixed(0)} KB`;
}

function entryFor(triple) {
  return manifest && manifest.channels && manifest.channels.stable && manifest.channels.stable[triple];
}

function manifestLooksValid(m) {
  if (!m || m.schema !== 1 || typeof m.version !== 'string' || !m.channels || !m.channels.stable) return false;
  return Object.keys(ARCHES).some((key) => {
    const e = m.channels.stable[key];
    return e
      && typeof e.dmg_url === 'string' && e.dmg_url.startsWith('https://')
      && typeof e.url === 'string' && e.url.startsWith('https://')
      && typeof e.size === 'number';
  });
}

function render() {
  const entry = entryFor(arch);
  const dmgHref = (entry && entry.dmg_url) || RELEASES_LATEST;
  const sizeText = entry && typeof entry.size === 'number' && entry.size > 0 ? formatSize(entry.size) : '';

  $('hero-dl').href = dmgHref;
  $('nav-dl').href = dmgHref;
  $('hero-arch-label').textContent = ARCHES[arch].label;
  $$('#hero-split .menu-item, #nav-split .menu-item').forEach((item) => {
    item.setAttribute('aria-checked', String(item.dataset.arch === arch));
    const sizeEl = item.querySelector('.mi-size');
    const e = entryFor(item.dataset.arch);
    sizeEl.textContent = e && typeof e.size === 'number' && e.size > 0 ? formatSize(e.size) : '';
  });

  for (const triple of Object.keys(ARCHES)) {
    const short = triple === 'aarch64-apple-darwin' ? 'aarch64' : 'x86_64';
    const e = entryFor(triple);
    $(`card-dmg-${short}`).href = (e && e.dmg_url) || RELEASES_LATEST;
    $(`card-zip-${short}`).href = (e && e.url) || RELEASES_LATEST;
    $(`card-size-${short}`).textContent =
      e && typeof e.size === 'number' && e.size > 0 ? `安装包 ${formatSize(e.size)}` : '';
  }
}

fetch('manifest.json')
  .then((res) => { if (!res.ok) throw new Error(`HTTP ${res.status}`); return res.json(); })
  .then((m) => {
    if (!manifestLooksValid(m)) throw new Error('manifest 校验不符');
    manifest = m;
    for (const pill of ['hero-version', 'dl-version']) {
      $(pill).textContent = `v${m.version}`;
      $(pill).hidden = false;
    }
    render();
  })
  .catch(() => {
    $('fallback-note').hidden = false;
    render();
  });

// 本机架构识别：WebGL 渲染器名优先（Apple Silicon 报 "Apple M2"/"Apple GPU"，
// Intel Mac 报 "Intel/AMD/NVIDIA"），取不到再走 UA-CH；都取不到保持默认。
async function detectArch() {
  try {
    const canvas = document.createElement('canvas');
    const gl = canvas.getContext('webgl') || canvas.getContext('experimental-webgl');
    if (gl) {
      const ext = gl.getExtension('WEBGL_debug_renderer_info');
      const renderer = ext
        ? gl.getParameter(ext.UNMASKED_RENDERER_WEBGL)
        : gl.getParameter(gl.RENDERER);
      if (/apple\s*(m\d|gpu)/i.test(renderer)) return 'aarch64-apple-darwin';
      if (/intel|nvidia|amd|radeon/i.test(renderer)) return 'x86_64-apple-darwin';
    }
  } catch (err) { /* WebGL 不可用则走下一路 */ }
  try {
    if (navigator.userAgentData && navigator.userAgentData.getHighEntropyValues) {
      const ua = await navigator.userAgentData.getHighEntropyValues(['architecture', 'platform']);
      if (ua.platform === 'macOS') {
        if (ua.architecture === 'arm') return 'aarch64-apple-darwin';
        if (ua.architecture === 'x86') return 'x86_64-apple-darwin';
      }
    }
  } catch (err) { /* UA-CH 不可用则保持默认 */ }
  return null;
}

detectArch().then((detected) => {
  if (detected && !userPicked) {
    arch = detected;
    render();
  }
});

// 分体按钮的架构下拉
for (const splitId of ['hero-split', 'nav-split']) {
  const split = $(splitId);
  const caret = $(splitId === 'hero-split' ? 'hero-caret' : 'nav-caret');
  caret.addEventListener('click', (ev) => {
    ev.stopPropagation();
    const open = split.classList.toggle('open');
    caret.setAttribute('aria-expanded', String(open));
  });
  document.addEventListener('click', () => {
    split.classList.remove('open');
    caret.setAttribute('aria-expanded', 'false');
  });
  $$('.menu-item', split).forEach((item) => {
    item.addEventListener('click', () => {
      userPicked = true;
      arch = item.dataset.arch;
      split.classList.remove('open');
      caret.setAttribute('aria-expanded', 'false');
      render();
    });
  });
}

// 滚动状态与进场动画
const nav = $('nav');
const onScroll = () => nav.classList.toggle('scrolled', window.scrollY > 8);
window.addEventListener('scroll', onScroll, { passive: true });
onScroll();

if ('IntersectionObserver' in window) {
  const io = new IntersectionObserver((entries) => {
    for (const en of entries) {
      if (en.isIntersecting) { en.target.classList.add('in'); io.unobserve(en.target); }
    }
  }, { threshold: 0.12 });
  $$('.reveal').forEach((el) => io.observe(el));
} else {
  $$('.reveal').forEach((el) => el.classList.add('in'));
}
