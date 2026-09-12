/* Desktop presentation layer. Storage operations remain in the existing UI client. */
let selectedFileName = null;
let desktopTab = 'files';
let inspectorRequest = 0;

Object.assign(i18n.en, {
  tab_files: 'All files', tab_overview: 'Overview', tab_snapshots: 'Snapshots',
  tab_upload: 'Import files', tab_watcher: 'Auto-Vault', tab_maintenance: 'Maintenance',
  files_title: 'All files', files_search_placeholder: 'Search files and folders...',
  files_empty: 'No files in this vault', search_empty: 'No matching files',
  snapshots_title: 'Snapshots', snapshot_input_placeholder: 'Snapshot name',
  btn_capture_snapshot: 'Create snapshot', upload_title: 'Import files',
  btn_select_files: 'Select files', btn_select_folder: 'Select folder',
  watcher_title: 'Auto-Vault', watcher_card_title: 'Watched folder',
  btn_start_watcher: 'Start Auto-Vault', retention_title: 'Version retention',
  versions_table_header_title: 'Version history', action_versions: 'Version history',
  workspace: 'Workspace', local_vault: 'Local vault', on_device: 'On this device',
  nav_workspace: 'Workspace', nav_tools: 'Tools', local_storage: 'Local storage',
  files_description: 'Your files, with every version in place.',
  import_files: 'Import files', storage_used: 'Storage used', saved_by_dedup: 'Space saved',
  versions_total: 'Versions', file_details: 'File details', size_label: 'Size',
  type_label: 'Type', updated_label: 'Updated', location_label: 'Location',
  object_label: 'Object ID', recent_versions: 'Recent versions',
  all_versions: 'All versions', current_version: 'Current version',
  name_label: 'Name', version_label: 'Version', sort_name: 'Name: A to Z',
  sort_recent: 'Recently updated', sort_size: 'Largest files',
  vault_root: 'Vault root', storage_distribution: 'Storage distribution',
  vault_summary: 'Vault summary', engine_label: 'Storage engine', no_selection: 'Select a file',
  local_only: 'Stored locally', items_shown: 'files shown', watcher_not_running: 'Auto-Vault is off',
  latest_snapshot: 'Snapshots', total_logical: 'Logical data', view_overview: 'View overview',
  snapshot_create: 'Create snapshot', refresh_label: 'Refresh', close_label: 'Close',
  filter_label: 'Sort files', overview_description: 'Storage, snapshots and vault activity.',
  diff_mode_diff: 'Compare', diff_mode_selected: 'Selected version', diff_mode_current: 'Current version',
  diff_copy_content: 'Copy', action_diff_inspect: 'Inspect', modal_versions_title: 'Version history',
  diff_section_title: 'Changes', btn_prune: 'Prune versions',
  theme_toggle: 'Toggle theme', theme_light: 'Switch to light mode', theme_dark: 'Switch to dark mode',
});
Object.assign(i18n.vi, {
  tab_files: 'Tất cả tập tin', tab_overview: 'Tổng quan', tab_snapshots: 'Snapshot',
  tab_upload: 'Nhập tập tin', tab_watcher: 'Auto-Vault', tab_maintenance: 'Bảo trì',
  files_title: 'Tất cả tập tin', files_search_placeholder: 'Tìm tập tin và thư mục...',
  files_empty: 'Kho chưa có tập tin', search_empty: 'Không có tập tin phù hợp',
  snapshots_title: 'Snapshot', snapshot_input_placeholder: 'Tên snapshot',
  btn_capture_snapshot: 'Tạo snapshot', upload_title: 'Nhập tập tin',
  btn_select_files: 'Chọn tập tin', btn_select_folder: 'Chọn thư mục',
  watcher_title: 'Auto-Vault', watcher_card_title: 'Thư mục theo dõi',
  btn_start_watcher: 'Bắt đầu Auto-Vault', retention_title: 'Lưu giữ phiên bản',
  versions_table_header_title: 'Lịch sử phiên bản', action_versions: 'Lịch sử phiên bản',
  workspace: 'Không gian làm việc', local_vault: 'Kho cục bộ', on_device: 'Trên thiết bị này',
  nav_workspace: 'Không gian làm việc', nav_tools: 'Công cụ', local_storage: 'Lưu trữ cục bộ',
  files_description: 'Tập tin của bạn và lịch sử của từng phiên bản.',
  import_files: 'Nhập tập tin', storage_used: 'Dung lượng đã dùng', saved_by_dedup: 'Tiết kiệm',
  versions_total: 'Phiên bản', file_details: 'Chi tiết tập tin', size_label: 'Kích thước',
  type_label: 'Loại', updated_label: 'Cập nhật', location_label: 'Vị trí',
  object_label: 'Object ID', recent_versions: 'Phiên bản gần đây',
  all_versions: 'Tất cả phiên bản', current_version: 'Phiên bản hiện tại',
  name_label: 'Tên', version_label: 'Phiên bản', sort_name: 'Tên: A đến Z',
  sort_recent: 'Cập nhật gần đây', sort_size: 'Lớn nhất',
  vault_root: 'Thư mục gốc', storage_distribution: 'Phân bổ lưu trữ',
  vault_summary: 'Thông tin kho', engine_label: 'Storage engine', no_selection: 'Chọn tập tin',
  local_only: 'Lưu trên thiết bị', items_shown: 'tập tin hiển thị', watcher_not_running: 'Auto-Vault đang tắt',
  latest_snapshot: 'Snapshot', total_logical: 'Dữ liệu logic', view_overview: 'Xem tổng quan',
  snapshot_create: 'Tạo snapshot', refresh_label: 'Làm mới', close_label: 'Đóng',
  filter_label: 'Sắp xếp tập tin', overview_description: 'Dung lượng, snapshot và hoạt động của kho.',
  diff_mode_diff: 'So sánh', diff_mode_selected: 'Bản đang chọn', diff_mode_current: 'Bản hiện tại',
  diff_copy_content: 'Sao chép', action_diff_inspect: 'Xem', modal_versions_title: 'Lịch sử phiên bản',
  diff_section_title: 'Thay đổi', btn_prune: 'Dọn phiên bản',
  theme_toggle: 'Chuyển giao diện', theme_light: 'Chuyển sang chế độ sáng', theme_dark: 'Chuyển sang chế độ tối',
});

function desktopIcon(name) {
  if (!window.lucide) return '<span aria-hidden="true">&#183;</span>';
  const key = name.replace(/(^|-)(\w)/g, (_, dash, c) => c.toUpperCase());
  const node = lucide.icons[key];
  if (!node) return '';
  const element = lucide.createElement(node);
  element.classList.add('lucide');
  element.setAttribute('aria-hidden', 'true');
  return element.outerHTML;
}

function fileType(name) {
  const ext = name.split('.').pop().toLowerCase();
  if (['png', 'jpg', 'jpeg', 'webp', 'gif', 'bmp'].includes(ext)) return ['image', 'image'];
  if (['rs', 'js', 'ts', 'py', 'go', 'html', 'css', 'json', 'toml', 'yaml'].includes(ext)) return ['code', 'file-code-2'];
  if (['zip', 'tar', 'gz', '7z', 'rar'].includes(ext)) return ['archive', 'file-archive'];
  if (['md', 'txt', 'pdf', 'doc', 'docx', 'csv'].includes(ext)) return ['document', 'file-text'];
  return ['file', 'file'];
}

function getFileIcon(name) {
  const [type, icon] = fileType(name);
  return `<span class="file-type-icon ${type}">${desktopIcon(icon)}</span>`;
}

function fileParts(name) {
  const parts = name.replace(/\\/g, '/').split('/');
  const basename = parts.pop();
  return [basename, parts.join('/') || t('vault_root')];
}

function sortedFiles(files) {
  const sort = document.getElementById('file-sort').value;
  return [...files].sort((a, b) => sort === 'recent' ? b.created_at - a.created_at
    : sort === 'size' ? b.size_bytes - a.size_bytes : a.name.localeCompare(b.name));
}

function actionIcon(action, file, icon, title, extra = '') {
  return `<button type="button" class="icon-button ${extra}" data-file-action="${action}"
    data-file="${escapeHtml(file.name)}" title="${escapeHtml(title)}" aria-label="${escapeHtml(title)}">${desktopIcon(icon)}</button>`;
}

function fileActions(file) {
  return `<div class="row-actions">${actionIcon('details', file, 'panel-right', t('file_details'))}
    ${actionIcon('versions', file, 'history', t('action_versions'))}
    <a class="icon-button" href="/api/download?target=${encodeURIComponent(file.name)}" title="${escapeHtml(t('action_download'))}" aria-label="${escapeHtml(t('action_download'))}">${desktopIcon('download')}</a>
    ${actionIcon('delete', file, 'trash-2', t('action_delete'), 'danger')}</div>`;
}

function emptyFiles(files) {
  const query = document.getElementById('file-search-input').value.trim();
  return `<div class="empty-state">${desktopIcon(query ? 'search-x' : 'folder-open')}
    <h3>${escapeHtml(t(query ? 'search_empty' : 'files_empty'))}</h3>
    ${query ? '' : `<button class="primary-button" onclick="switchTab('upload')">${desktopIcon('plus')}${t('import_files')}</button>`}</div>`;
}

function renderFilesList(files) {
  const tbody = document.getElementById('files-table-body');
  tbody.innerHTML = files.length ? sortedFiles(files).map(file => {
    const [name, path] = fileParts(file.name);
    return `<tr tabindex="0" data-file="${escapeHtml(file.name)}" data-file-action="details" class="${selectedFileName === file.name ? 'selected' : ''}" aria-selected="${selectedFileName === file.name}">
      <td><div class="file-name-cell">${getFileIcon(file.name)}<div><span class="file-name" title="${escapeHtml(name)}">${escapeHtml(name)}</span><span class="file-path">${escapeHtml(path)}</span></div></div></td>
      <td><button class="version-tag" data-file-action="versions" data-file="${escapeHtml(file.name)}" title="${escapeHtml(t('action_versions'))}">${desktopIcon('history')}v${file.latest_version}</button></td>
      <td class="file-size">${formatBytes(file.size_bytes)}</td><td>${fileActions(file)}</td></tr>`;
  }).join('') : `<tr><td colspan="4">${emptyFiles(files)}</td></tr>`;
}

function renderFilesGrid(files) {
  document.getElementById('files-grid-container').innerHTML = files.length ? sortedFiles(files).map(file => {
    const [name, path] = fileParts(file.name);
    return `<article class="card p-4" tabindex="0" data-file-action="details" data-file="${escapeHtml(file.name)}">
      <div class="flex justify-between items-center mb-4">${getFileIcon(file.name)}<span class="version-tag">v${file.latest_version}</span></div>
      <div class="file-name" title="${escapeHtml(name)}">${escapeHtml(name)}</div><div class="file-path">${escapeHtml(path)}</div>
      <div class="flex items-center justify-between mt-4"><span class="file-size">${formatBytes(file.size_bytes)}</span>${fileActions(file)}</div></article>`;
  }).join('') : `<div class="col-span-full">${emptyFiles(files)}</div>`;
}

function decorateActionButtons(root) {
  if (!root) return;
  const actions = {
    openPreviewModal: ['eye', 'action_preview'], openVersionsModal: ['history', 'action_versions'],
    createShareLink: ['share-2', 'Share'], confirmDeleteFile: ['trash-2', 'action_delete'],
    confirmDeleteFolder: ['trash-2', 'action_delete'], closePreviewModal: ['x', 'close_label'],
    closeVersionsModal: ['x', 'close_label'], closeConfirmModal: ['x', 'close_label'],
  };
  root.querySelectorAll('button[onclick]').forEach(button => {
    const handler = button.getAttribute('onclick');
    const key = Object.keys(actions).find(key => handler && handler.includes(key + '('));
    if (!key || button.classList.contains('version-tag')) return;
    const [icon, label] = actions[key];
    button.innerHTML = desktopIcon(icon);
    button.title = t(label);
    button.setAttribute('aria-label', t(label));
    button.className = 'icon-button' + (key.includes('Delete') ? ' danger' : '');
  });
  root.querySelectorAll('a[href*="/api/download"]').forEach(link => {
    link.innerHTML = desktopIcon('download');
    link.title = t('action_download');
    link.setAttribute('aria-label', t('action_download'));
    link.className = 'icon-button';
  });
}

const baseRenderTree = renderFilesTree;
renderFilesTree = function(files) {
  baseRenderTree(files);
  decorateActionButtons(document.getElementById('files-tree-body'));
};

const baseRenderFiles = renderFiles;
renderFiles = function(files) {
  baseRenderFiles(files);
  document.getElementById('files-shown').textContent = `${files.length} ${t('items_shown')}`;
  if (selectedFileName && !cachedFilesList.some(file => file.name === selectedFileName)) closeFileDetails();
};

function closeFileDetails() {
  selectedFileName = null;
  inspectorRequest++;
  document.getElementById('file-inspector').hidden = true;
  document.getElementById('files-layout').classList.remove('has-selection');
  filterFilesTable();
}

async function selectFileDetails(name) {
  const file = cachedFilesList.find(file => file.name === name);
  if (!file) return;
  selectedFileName = name;
  const request = ++inspectorRequest;
  const [basename, path] = fileParts(name);
  const panel = document.getElementById('file-inspector');
  panel.hidden = false;
  document.getElementById('files-layout').classList.add('has-selection');
  panel.innerHTML = `<div class="inspector-title">${t('file_details')}<button class="icon-button" onclick="closeFileDetails()" title="${t('close_label')}" aria-label="${t('close_label')}">${desktopIcon('x')}</button></div>
    <div class="inspector-preview">${fileType(name)[0] === 'image' ? `<img src="/api/download?target=${encodeURIComponent(name)}" alt="${escapeHtml(basename)}">` : getFileIcon(name)}</div>
    <h3 class="inspector-name">${escapeHtml(basename)}</h3><p class="inspector-subtitle">${escapeHtml(path)}</p>
    <dl class="inspector-details">
      <div><dt>${t('size_label')}</dt><dd>${formatBytes(file.size_bytes)}</dd></div>
      <div><dt>${t('type_label')}</dt><dd>${escapeHtml(name.split('.').pop().toUpperCase())}</dd></div>
      <div><dt>${t('updated_label')}</dt><dd>${escapeHtml(formatTimestamp(file.created_at))}</dd></div>
      <div><dt>${t('version_label')}</dt><dd>v${file.latest_version}</dd></div>
      <div><dt>${t('object_label')}</dt><dd title="${escapeHtml(file.object_id)}">${escapeHtml(file.object_id.slice(0, 14))}...</dd></div>
    </dl>
    <div class="inspector-actions"><button class="primary-button" data-file-action="preview" data-file="${escapeHtml(name)}">${desktopIcon('eye')}${t('action_preview')}</button>
    <button class="secondary-button" data-file-action="share" data-file="${escapeHtml(name)}">${desktopIcon('share-2')}Share</button></div>
    <div class="inspector-history"><h4>${t('recent_versions')}</h4><div id="inspector-versions" aria-live="polite">...</div>
    <button class="secondary-button w-full mt-3" data-file-action="versions" data-file="${escapeHtml(name)}">${desktopIcon('history')}${t('all_versions')}</button></div>`;
  filterFilesTable();
  panel.querySelector('img')?.addEventListener('error', event => { event.target.parentElement.innerHTML = getFileIcon(name); });
  try {
    const response = await fetch('/api/versions?name=' + encodeURIComponent(name));
    if (!response.ok) throw new Error('Could not load versions');
    const versions = await response.json();
    if (request !== inspectorRequest) return;
    document.getElementById('inspector-versions').innerHTML = versions.slice(-3).reverse().map(version =>
      `<div class="history-item">${desktopIcon('circle-check')}<div>v${version.version}${version.version === file.latest_version ? ' · ' + t('current_version') : ''}<small>${escapeHtml(formatTimestamp(version.created_at))}</small></div></div>`).join('');
  } catch (error) {
    if (request === inspectorRequest) document.getElementById('inspector-versions').textContent = error.message;
  }
}

document.getElementById('files-layout').addEventListener('click', event => {
  if (event.target.closest('a')) return;
  const target = event.target.closest('[data-file-action]');
  if (!target) return;
  const file = cachedFilesList.find(file => file.name === target.dataset.file);
  if (!file) return;
  const encoded = encodeURIComponent(file.name);
  switch (target.dataset.fileAction) {
    case 'details': selectFileDetails(file.name); break;
    case 'preview': openPreviewModal(encoded, file.size_bytes); break;
    case 'versions': openVersionsModal(encoded); break;
    case 'share': createShareLink(encoded); break;
    case 'delete': confirmDeleteFile(encoded); break;
  }
});
document.getElementById('files-layout').addEventListener('keydown', event => {
  if (['Enter', ' '].includes(event.key) && event.target.matches('tr[data-file], article[data-file]')) {
    event.preventDefault(); event.target.click();
  }
});

let currentTheme = localStorage.getItem('oos_theme') ||
  (window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light');

function applyTheme(theme) {
  currentTheme = theme;
  localStorage.setItem('oos_theme', theme);
  const isDark = theme === 'dark';
  document.body.classList.toggle('theme-dark', isDark);
  document.documentElement.classList.toggle('dark', isDark);

  const themeBtn = document.getElementById('theme-toggle-btn');
  const themeIcon = document.getElementById('theme-icon');
  if (themeBtn) {
    themeBtn.title = isDark ? t('theme_light') : t('theme_dark');
    themeBtn.setAttribute('aria-label', themeBtn.title);
  }
  if (themeIcon) {
    themeIcon.setAttribute('data-lucide', isDark ? 'sun' : 'moon');
    if (window.lucide) lucide.createIcons();
  }
}

function toggleTheme() {
  applyTheme(currentTheme === 'dark' ? 'light' : 'dark');
}

if (window.matchMedia) {
  window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', e => {
    if (!localStorage.getItem('oos_theme')) {
      applyTheme(e.matches ? 'dark' : 'light');
    }
  });
}

function hookMicroInteractions() {
  const refreshBtn = document.getElementById('refresh-btn');
  if (refreshBtn) {
    refreshBtn.addEventListener('click', () => {
      const icon = document.getElementById('refresh-icon');
      if (!icon) return;
      icon.classList.remove('spin-animate');
      void icon.offsetWidth;
      icon.classList.add('spin-animate');
      setTimeout(() => icon.classList.remove('spin-animate'), 650);
    });
  }
}

function syncDesktopLabels() {
  document.getElementById('breadcrumb-current').textContent = t('tab_' + desktopTab);
  document.querySelectorAll('[data-i18n-title]').forEach(element => {
    element.title = t(element.dataset.i18nTitle);
    element.setAttribute('aria-label', element.title);
  });
  const themeBtn = document.getElementById('theme-toggle-btn');
  if (themeBtn) {
    themeBtn.title = currentTheme === 'dark' ? t('theme_light') : t('theme_dark');
    themeBtn.setAttribute('aria-label', themeBtn.title);
  }
  document.querySelectorAll('.view-segments button').forEach(button => {
    button.setAttribute('aria-pressed', String(button.id === 'btn-view-' + currentViewMode));
  });
  if (window.lucide) lucide.createIcons();
  document.documentElement.lang = currentLang;
}

const baseSwitchTab = switchTab;
switchTab = function(tab) {
  desktopTab = tab;
  baseSwitchTab(tab);
  document.body.classList.remove('nav-open');
  syncDesktopLabels();
  document.querySelectorAll('.app-nav button').forEach(button => button.setAttribute('aria-current', button.id === 'tab-' + tab ? 'page' : 'false'));
};
const baseSetViewMode = setViewMode;
setViewMode = function(mode) { baseSetViewMode(mode); filterFilesTable(); syncDesktopLabels(); };
const baseSetLanguage = setLanguage;
setLanguage = function(lang) {
  baseSetLanguage(lang); syncDesktopLabels();
  if (selectedFileName) selectFileDetails(selectedFileName);
};

function updateDesktopStats(data) {
  document.getElementById('metric-files').textContent = data.total_objects;
  document.getElementById('metric-storage').textContent = formatBytes(data.physical_disk_bytes);
  document.getElementById('metric-saved').textContent = (data.space_savings_pct || 0).toFixed(1) + '%';
  document.getElementById('metric-versions').textContent = data.total_snapshots;
  document.getElementById('sidebar-used').textContent = formatBytes(data.physical_disk_bytes);
  document.getElementById('sidebar-dedup').textContent = (data.dedup_ratio || 1).toFixed(2) + 'x';
  document.getElementById('sidebar-meter-fill').style.width = (100 - Math.max(0, Math.min(100, data.space_savings_pct || 0))) + '%';
}

document.getElementById('mobile-menu-btn').addEventListener('click', () => {
  document.body.classList.toggle('nav-open');
  document.getElementById('mobile-menu-btn').setAttribute('aria-expanded', String(document.body.classList.contains('nav-open')));
});
document.addEventListener('click', event => {
  if (!event.target.closest('.app-sidebar, #mobile-menu-btn')) document.body.classList.remove('nav-open');
});
document.addEventListener('keydown', event => {
  if (event.key === 'Escape') {
    document.body.classList.remove('nav-open');
    for (const [id, close] of [['confirm-modal', closeConfirmModal], ['preview-modal', closePreviewModal], ['versions-modal', closeVersionsModal]]) {
      if (!document.getElementById(id).classList.contains('hidden')) { close(); break; }
    }
  }
  if (event.key === 'Tab') {
    const modal = [...document.querySelectorAll('[role="dialog"]')].reverse().find(el => !el.classList.contains('hidden'));
    if (!modal) return;
    const focusable = [...modal.querySelectorAll('button:not(:disabled), a[href], input:not(:disabled), select:not(:disabled), [tabindex="0"]')].filter(el => el.getClientRects().length);
    const first = focusable[0], last = focusable.at(-1);
    if (event.shiftKey && (document.activeElement === first || !modal.contains(document.activeElement))) {
      event.preventDefault(); last?.focus();
    } else if (!event.shiftKey && (document.activeElement === last || !modal.contains(document.activeElement))) {
      event.preventDefault(); first?.focus();
    }
  }
});

document.querySelectorAll('body > [id$="-modal"]').forEach(modal => {
  modal.setAttribute('role', 'dialog');
  modal.setAttribute('aria-modal', 'true');
  const title = modal.querySelector('h3, h2');
  if (title?.id) modal.setAttribute('aria-labelledby', title.id);
});

applyTheme(currentTheme);
hookMicroInteractions();
applyTranslations();
syncDesktopLabels();
decorateActionButtons(document.querySelector('body'));
setViewMode('list');
switchTab('files');
refreshAll().then(handleUrlAction);
setInterval(fetchMountStatus, 4000);
setInterval(fetchWatcherStatus, 4000);
setInterval(pollUiActions, 1000);
