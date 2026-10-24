// 应用（Gloss）站点脚本：下载直链、本机架构识别与界面微交互。
// 首行标记脚本语言（JS）可用：进场动画的隐藏态只在脚本可用标记下生效，脚本加载失败时内容保持可见。
document.documentElement.classList.add('js');

// 下载契约：同源更新清单由持续集成（CI）注入，提供稳定渠道各架构的镜像地址与体积；
// 取不到或校验不过则整体降级到发布托管（GitHub Releases）。
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

  $('hero-dl').href = dmgHref;
  $('nav-dl').href = dmgHref;
  $('hero-arch-label').textContent = ARCHES[arch].label;
  $$('#hero-split .menu-item').forEach((item) => {
    item.setAttribute('aria-checked', String(item.dataset.arch === arch));
    const e = entryFor(item.dataset.arch);
    // 清单里的体积是压缩包实测值，展示时必须带压缩包标注以免误当镜像大小
    item.querySelector('.mi-size').textContent =
      e && typeof e.size === 'number' && e.size > 0 ? `zip ${formatSize(e.size)}` : '';
  });

  for (const triple of Object.keys(ARCHES)) {
    const short = triple === 'aarch64-apple-darwin' ? 'aarch64' : 'x86_64';
    const e = entryFor(triple);
    $(`card-dmg-${short}`).href = (e && e.dmg_url) || RELEASES_LATEST;
    $(`card-zip-${short}`).href = (e && e.url) || RELEASES_LATEST;
    $(`card-size-${short}`).textContent =
      e && typeof e.size === 'number' && e.size > 0 ? `zip ${formatSize(e.size)}` : '';
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

// 本机架构识别：仅对系统（macOS）访客生效，网页图形（WebGL）渲染器名优先：新架构报苹果芯片，
// 旧架构报芯片厂商（Intel/AMD/NVIDIA）系；该口径天然兼容转译层下浏览器跑在新架构的
// 边界——按图形硬件（GPU）而非浏览器架构选包，取不到再走客户端提示；都取不到保持默认。
async function detectArch() {
  if (!/Mac/i.test(navigator.userAgent)) return null;
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
  } catch (err) { /* 网页图形（WebGL）不可用则走下一路 */ }
  try {
    if (navigator.userAgentData && navigator.userAgentData.getHighEntropyValues) {
      const ua = await navigator.userAgentData.getHighEntropyValues(['architecture', 'platform']);
      if (ua.platform === 'macOS') {
        if (ua.architecture === 'arm') return 'aarch64-apple-darwin';
        if (ua.architecture === 'x86') return 'x86_64-apple-darwin';
      }
    }
  } catch (err) { /* 客户端提示不可用则保持默认 */ }
  return null;
}

detectArch().then((detected) => {
  if (detected && !userPicked) {
    arch = detected;
    render();
  }
});

// 架构下拉（仅首屏一处，导航的下载按钮为跟随当前架构的直链）。
// 键盘契约按无障碍菜单模式：方向键循环、行首行尾键跳转、退出键关闭回焦、
// 制表键走默认顺序并收起菜单；打开即聚焦当前选中项。
const splits = ['hero-split'].map((id) => {
  const root = $(id);
  const caret = $(id === 'hero-split' ? 'hero-caret' : 'nav-caret');
  const menu = root.querySelector('.menu');
  const items = $$('.menu-item', menu);
  return { root, caret, menu, items };
});

function closeSplits(except) {
  for (const s of splits) {
    if (s === except) continue;
    s.root.classList.remove('open');
    s.caret.setAttribute('aria-expanded', 'false');
  }
}

function openSplit(s) {
  closeSplits(s);
  s.root.classList.add('open');
  s.caret.setAttribute('aria-expanded', 'true');
  const current = s.items.find((item) => item.getAttribute('aria-checked') === 'true') || s.items[0];
  current.focus();
}

for (const s of splits) {
  s.caret.addEventListener('click', (ev) => {
    ev.stopPropagation();
    if (s.root.classList.contains('open')) {
      closeSplits();
    } else {
      openSplit(s);
    }
  });
  s.caret.addEventListener('keydown', (ev) => {
    if (ev.key === 'Escape' && s.root.classList.contains('open')) {
      closeSplits();
    } else if (['ArrowDown', 'ArrowUp', 'Enter', ' '].includes(ev.key)) {
      ev.preventDefault();
      openSplit(s);
    }
  });
  s.items.forEach((item, idx) => {
    item.addEventListener('keydown', (ev) => {
      if (ev.key === 'ArrowDown' || ev.key === 'ArrowUp') {
        ev.preventDefault();
        const dir = ev.key === 'ArrowDown' ? 1 : -1;
        s.items[(idx + dir + s.items.length) % s.items.length].focus();
      } else if (ev.key === 'Home') {
        ev.preventDefault();
        s.items[0].focus();
      } else if (ev.key === 'End') {
        ev.preventDefault();
        s.items[s.items.length - 1].focus();
      } else if (ev.key === 'Escape') {
        closeSplits();
        s.caret.focus();
      } else if (ev.key === 'Tab') {
        closeSplits();
      }
    });
    item.addEventListener('click', () => {
      userPicked = true;
      arch = item.dataset.arch;
      closeSplits();
      render();
    });
  });
  s.root.addEventListener('focusout', (ev) => {
    if (!s.root.contains(ev.relatedTarget)) closeSplits();
  });
}
document.addEventListener('click', () => closeSplits());

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
