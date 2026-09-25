import './style.css';

const navToggle = document.querySelector('.menu-toggle');
navToggle.addEventListener('click', () => {
  const open = navToggle.getAttribute('aria-expanded') !== 'true';
  navToggle.setAttribute('aria-expanded', String(open));
  navToggle.querySelector('use').setAttribute('href', `/icons.svg#${open ? 'close' : 'menu'}`);
  navToggle.setAttribute('aria-label', open ? 'Close menu' : 'Open menu');
  document.querySelector('.header').classList.toggle('menu-open', open);
});
document.querySelectorAll('.header nav a').forEach(link => link.addEventListener('click', () => {
  navToggle.setAttribute('aria-expanded', 'false');
  navToggle.querySelector('use').setAttribute('href', '/icons.svg#menu');
  navToggle.setAttribute('aria-label', 'Open menu');
  document.querySelector('.header').classList.remove('menu-open');
}));

const workspaceViews = {
  editor: { image: 'editor.png', alt: 'Emulsion Photo workspace with a portrait, navigation tools, layer properties, and layers panel', copy: 'A dedicated photo workspace with the canvas at its centre. Navigate your image and keep layer properties, masks, opacity, and blend controls close at hand.', tag: 'PHOTO / LAYERS & PROPERTIES' },
  drawing: { image: 'drawing.png', alt: 'Emulsion Draw workspace with a layered squirrel painting, photograph reference, brush presets, and assistant conversation', copy: 'Paint from a reference with brushes and editable layers. Keep your source image beside the canvas and continue working on an assistant-assisted drawing with the same tools.', tag: 'DRAW / REFERENCE / ASSISTANT' },
  home: { image: 'home.png', alt: 'Emulsion home library showing recent photos, illustrations, and a selected squirrel project with layer and history details', copy: 'Browse recent photographs and illustrations, inspect a project’s layers and history count, or start with an open file, a new canvas, or a batch folder.', tag: 'HOME / YOUR PROJECT LIBRARY' },
};
function selectView(name) {
  const view = workspaceViews[name];
  document.querySelectorAll('[data-tab]').forEach(tab => {
    const active = tab.dataset.tab === name;
    tab.setAttribute('aria-selected', String(active));
    tab.tabIndex = active ? 0 : -1;
  });
  const img = document.querySelector('#workspace-image');
  img.src = `/assets/${view.image}`;
  img.alt = view.alt;
  document.querySelector('#workspace-copy').textContent = view.copy;
  document.querySelector('#workspace-tag').textContent = view.tag;
  document.querySelector('#workspace-panel').setAttribute('aria-labelledby', `tab-${name}`);
}
document.querySelectorAll('[data-tab]').forEach((tab, index, tabs) => {
  tab.addEventListener('click', () => selectView(tab.dataset.tab));
  tab.addEventListener('keydown', event => {
    let next;
    if (event.key === 'ArrowRight') next = (index + 1) % tabs.length;
    if (event.key === 'ArrowLeft') next = (index - 1 + tabs.length) % tabs.length;
    if (event.key === 'Home') next = 0;
    if (event.key === 'End') next = tabs.length - 1;
    if (next !== undefined) { event.preventDefault(); selectView(tabs[next].dataset.tab); tabs[next].focus(); }
  });
});
document.querySelectorAll('[data-select]').forEach(link => link.addEventListener('click', () => selectView(link.dataset.select)));
document.querySelectorAll('[data-look]').forEach(button => button.addEventListener('click', () => {
  document.querySelectorAll('[data-look]').forEach(other => { const active = other === button; other.classList.toggle('active', active); other.setAttribute('aria-pressed', String(active)); });
  document.querySelector('.portrait-demo').dataset.look = button.dataset.look;
  document.querySelector('.demo-badge').textContent = { original: 'Original direction', mono: 'Monochrome direction', warm: 'Warm study direction' }[button.dataset.look];
}));

const installDialog = document.querySelector('#install-dialog');
const platforms = {
  linux: { requirements: 'Downloads the latest Linux x86_64 release and installs an AppImage with a desktop launcher. Requires a working graphics driver; no Rust toolchain needed.', command: 'curl -fsSL https://emulsion.pro/install | sh', anchor: 'install-linux' },
  macos: { requirements: 'Requires Rust and the standard macOS iconutil and sips tools. The script builds a local .app and .dmg, signed ad hoc for local use.', command: 'scripts/build-macos.sh', anchor: 'build-macos' },
  windows: { requirements: 'Download Emulsion 0.0.1 for Windows x64, then run the setup program. No Rust compiler or Visual Studio Build Tools needed.', download: 'https://releases.emulsion.pro/emulsion/emulsion_0.0.1_x64-setup.exe', anchor: 'build-windows' },
};
function selectPlatform(name) {
  const platform = platforms[name];
  document.querySelectorAll('[data-platform]').forEach(button => button.setAttribute('aria-pressed', String(button.dataset.platform === name)));
  document.querySelector('#install-requirements').textContent = platform.requirements;
  document.querySelector('#install-dialog .command-block').hidden = Boolean(platform.download);
  document.querySelector('#install-command').textContent = platform.download ? '' : name === 'linux' ? platform.command : `git clone https://github.com/jeremielumandong/emulsion.git\ncd emulsion\n${platform.command}`;
  document.querySelector('#install-guide').href = platform.download || `https://github.com/jeremielumandong/emulsion#${platform.anchor}`;
  document.querySelector('#install-guide-label').textContent = platform.download ? 'Download for Windows (x64)' : 'Open installation guide';
  document.querySelector('#copy-command span').textContent = 'Copy';
  document.querySelector('#copy-command use').setAttribute('href', '/icons.svg#copy');
  document.querySelector('#copy-status').textContent = '';
}
selectPlatform(/Win/.test(navigator.platform) ? 'windows' : /Mac/.test(navigator.platform) ? 'macos' : 'linux');
document.querySelectorAll('[data-platform]').forEach(button => button.addEventListener('click', () => selectPlatform(button.dataset.platform)));
document.querySelectorAll('[data-install]').forEach(button => button.addEventListener('click', () => installDialog.showModal()));
document.querySelector('#copy-command').addEventListener('click', async event => {
  try { await navigator.clipboard.writeText(document.querySelector('#install-command').textContent); document.querySelector('#copy-command span').textContent = 'Copied!'; document.querySelector('#copy-command use').setAttribute('href', '/icons.svg#check'); document.querySelector('#copy-status').textContent = 'Installation commands copied.'; }
  catch { document.querySelector('#copy-status').textContent = 'Clipboard unavailable. Select and copy the commands above.'; }
});
document.querySelectorAll('dialog').forEach(dialog => {
  dialog.querySelector('.close-dialog').addEventListener('click', () => dialog.close());
  dialog.addEventListener('click', event => { if (event.target === dialog) { const bounds = dialog.getBoundingClientRect(); if (event.clientX < bounds.left || event.clientX > bounds.right || event.clientY < bounds.top || event.clientY > bounds.bottom) dialog.close(); } });
});
document.querySelector('.expand-image').addEventListener('click', () => {
  const source = document.querySelector('#workspace-image');
  const image = document.querySelector('#image-dialog img');
  image.src = source.src; image.alt = source.alt;
  document.querySelector('#image-dialog').showModal();
});
