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
  home: { image: 'home.png', alt: 'Emulsion Home with five workspace shortcuts, a project card and its action menu, the projects sidebar, and a grid of recent files', copy: 'Pick up a recent file or start in Photo, Paint, Library, Design, or Diagram. Organize work into projects, search your files, and use each project’s menu to rename it or remove the grouping while keeping its files.', tag: 'HOME / PROJECTS & RECENT FILES' },
  editor: { image: 'photo-portrait.png', alt: 'Emulsion Photo displaying a blue-haired portrait, recipe preview thumbnails with Faithful selected, and editable adjustment layers', copy: 'Preview recipes on your photograph, compare different looks, and apply a treatment as editable adjustment layers. Keep the portrait at the centre while refining colour, curves, and white balance.', tag: 'PHOTO / PORTRAITS & RECIPES' },
  drawing: { image: 'painting.png', alt: 'Emulsion Paint workspace with a layered Mount Fuji landscape painting, the brush panel with size, hardness and pressure controls, a preset library, and the layers panel', copy: 'Paint with pressure-sensitive brushes and build a scene across editable layers. Tune size, hardness, flow, and tilt, or pick from 55 built-in brushes, from inking pens to manga screentones, and import your own.', tag: 'PAINT / BRUSHES & LAYERS' },
  library: { image: 'library-panel.png', alt: 'Emulsion Library with a photo grid, folders and collections, a metadata panel, export settings, and a filmstrip', copy: 'Browse folders and collections, find photographs with ratings and filters, and inspect metadata beside the grid. Switch to Develop to import presets, compare Before / After, and refine local adjustments before exporting.', tag: 'LIBRARY / COLLECTIONS, PRESETS & DEVELOP', guide: '#guide-library' },
  design: { image: 'design-poster.png', alt: 'Emulsion Design workspace displaying a neon OMARCHY poster with a sunset, mountains, perspective grid, editable lettering, a template browser, and the Layers panel', copy: 'Start with an editable template or an empty document. Arrange text, shapes, and images across pages, then save your project, present it, or export your artwork.', tag: 'DESIGN / TEMPLATES, LAYOUTS & PAGES', guide: '#guide-design' },
  diagram: { image: 'diagram.png', alt: 'Emulsion Diagram workspace showing a credit approval process with swimlanes, editable shapes, bound connectors, the shape library, and object properties', copy: 'Start empty or choose a diagram template. Build flowcharts and concept maps with editable shapes, labels, containers, and connectors that follow their objects.', tag: 'DIAGRAM / SHAPES & CONNECTIONS', guide: '#guide-diagram' },
};
function selectView(name) {
  const view = workspaceViews[name];
  if (!view) return;
  document.querySelectorAll('[data-tab]').forEach(tab => {
    const active = tab.dataset.tab === name;
    tab.setAttribute('aria-selected', String(active));
    tab.tabIndex = active ? 0 : -1;
  });
  const img = document.querySelector('#workspace-image');
  const pending = document.querySelector('#workspace-placeholder');
  img.hidden = !view.image;
  pending.hidden = Boolean(view.image);
  document.querySelector('.expand-image').hidden = !view.image;
  if (view.image) {
    img.src = `/assets/${view.image}`;
    img.alt = view.alt;
  } else {
    pending.querySelector('use').setAttribute('href', `/icons.svg#${view.icon}`);
    document.querySelector('#workspace-pending-title').textContent = view.title;
    document.querySelector('#workspace-pending-copy').textContent = 'Explore the available tools in the guide below. An application screenshot will be added here.';
    const guide = document.querySelector('#workspace-pending-guide');
    guide.href = view.guide;
    guide.textContent = `Read the ${view.title} guide →`;
  }
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
  macos: { requirements: 'Download the latest Emulsion release for macOS on Apple silicon. Open the disk image and drag Emulsion to Applications. No Rust toolchain needed.', download: 'https://github.com/jeremielumandong/emulsion/releases/latest/download/Emulsion-macos-arm64.dmg', label: 'Download for macOS (Apple silicon)' },
  windows: { requirements: 'Download the latest Emulsion release for Windows x64, then run the setup program. No Rust compiler or Visual Studio Build Tools needed.', download: 'https://github.com/jeremielumandong/emulsion/releases/latest/download/Emulsion-windows-x64-setup.exe', label: 'Download for Windows (x64)' },
};
function selectPlatform(name) {
  const platform = platforms[name];
  document.querySelectorAll('[data-platform]').forEach(button => button.setAttribute('aria-pressed', String(button.dataset.platform === name)));
  document.querySelector('#install-requirements').textContent = platform.requirements;
  document.querySelector('#install-dialog .command-block').hidden = Boolean(platform.download);
  document.querySelector('#install-command').textContent = platform.download ? '' : name === 'linux' ? platform.command : `git clone https://github.com/jeremielumandong/emulsion.git\ncd emulsion\n${platform.command}`;
  document.querySelector('#install-guide').href = platform.download || `https://github.com/jeremielumandong/emulsion#${platform.anchor}`;
  document.querySelector('#install-guide-label').textContent = platform.download ? platform.label : 'Open installation guide';
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
  if (source.hidden) return;
  const image = document.querySelector('#image-dialog img');
  image.src = source.src; image.alt = source.alt;
  document.querySelector('#image-dialog').showModal();
});
