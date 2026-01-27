<script>
  let currentPage = 1;
  const PER_PAGE = 25;

  // Helper: Course Pill Style
  function getCoursePillStyle(name) {
    const hash = name.split('').reduce((acc, char) => char.charCodeAt(0) + ((acc << 5) - acc), 0);
    const color = `hsl(${Math.abs(hash % 360)}, 60%, 45%)`;
    return `background-color: ${color}; color: white;`;
  }

  async function refreshData() {
    const btn = document.getElementById('refresh-btn');
    btn.classList.add('is-loading');
    await loadNotifications();
    btn.classList.remove('is-loading');
  }

  function runFilter() {
    currentPage = 1;
    loadNotifications();
  }

  async function markAllRead() {
    if (!confirm("Mark all notifications as read?")) return;
    try {
      await fetch('/notifications/mark_all_read', { method: 'POST' });
      loadNotifications();
    } catch (err) {
      console.error("Failed to mark all read", err);
    }
  }

  async function toggleRead(id, isRead) {
    const endpoint = isRead ? `/notifications/${id}/mark_unread` : `/notifications/${id}/mark_read`;
    try {
      const resp = await fetch(endpoint, { method: 'POST', headers: { 'X-Requested-With': 'XMLHttpRequest' } });
      const data = await resp.json();
      if (data.status === 'ok') {
        const item = document.getElementById(`notif-${id}`);
        if (item) {
          if (data.is_read) {
            item.classList.add('is-read');
            item.querySelector('.read-toggle-btn').innerHTML = '<i class="far fa-circle"></i>';
            item.querySelector('.read-toggle-btn').title = "Mark as unread";
          } else {
            item.classList.remove('is-read');
            item.querySelector('.read-toggle-btn').innerHTML = '<i class="fas fa-circle"></i>';
            item.querySelector('.read-toggle-btn').title = "Mark as read";
          }
        }
      }
    } catch (err) {
      console.error("Toggle read failed", err);
    }
  }

  async function loadNotifications() {
    const form = document.getElementById('filter-form');
    const formData = new FormData(form);
    const params = new URLSearchParams(formData);

    // Only sync on initial load
    if (!window.hasInitiallyLoaded) params.set('sync', 'true');

    params.set('limit', PER_PAGE);
    params.set('offset', (currentPage - 1) * PER_PAGE);

    const container = document.getElementById('notifications-container');
    container.style.opacity = '0.5';

    try {
      const response = await fetch(`/api/v1/notifications?${params.toString()}`);
      const data = await response.json();
      renderNotifications(data.notifications, data.total);
      renderPagination(data.total);
      window.hasInitiallyLoaded = true;
    } catch (err) {
      container.innerHTML = `<div class="notification is-danger">Failed to load notifications.</div>`;
    } finally {
      container.style.opacity = '1';
    }
  }

  function renderNotifications(items, total) {
    const container = document.getElementById('notifications-container');
    if (!items || items.length === 0) {
      container.innerHTML = '<div class="box has-text-centered py-6 has-text-grey">No notifications found.</div>';
      return;
    }

    container.innerHTML = items.map(n => {
      const urgencyClass = n.urgency === 3 ? 'has-text-danger' : (n.urgency === 2 ? 'has-text-warning' : 'has-text-info');
      const isReadClass = n.is_read ? 'is-read' : '';
      const dotIcon = n.is_read ? 'far fa-circle' : 'fas fa-circle';

      return `
        <div class="box p-3 mb-3 notification-card ${isReadClass}" id="notif-${n.id}">
          <div class="columns is-mobile is-vcentered">
            <div class="column is-narrow">
               <button class="button is-ghost p-0 read-toggle-btn" onclick="toggleRead(${n.id}, ${n.is_read})" title="${n.is_read ? 'Mark as unread' : 'Mark as read'}">
                 <i class="${dotIcon} ${urgencyClass} is-size-7"></i>
               </button>
            </div>
            <div class="column">
              <div class="is-flex is-justify-content-space-between">
                <div class="is-flex is-align-items-center">
                  ${n.course_prefix ? `<span class="px-1 mr-2" style="border-radius: 3px; font-size: 0.65rem; font-weight: bold; ${n.pill_style || getCoursePillStyle(n.course_name)}">${n.course_prefix}</span>` : ''}
                  <span class="has-text-weight-bold has-text-dark is-size-6 mr-2">${n.course_short_name || n.course_name || 'General'}</span>
                  ${n.semester || n.course_semester ? `<span class="tag is-white is-small has-text-grey" style="font-size: 0.65rem;">${n.semester || n.course_semester}</span>` : ''}
                </div>
                <span class="is-size-7 has-text-grey">${n.simple_date}</span>
              </div>
              <h5 class="title is-6 mb-1">
                <a href="/notifications/${n.id}/view" class="has-text-dark">${n.display_title}</a>
              </h5>
            </div>
          </div>
        </div>
      `;
    }).join('');
  }

  function renderPagination(total) {
    const totalPages = Math.ceil(total / PER_PAGE);
    const container = document.getElementById('pagination-container');
    if (totalPages <= 1) {
      container.innerHTML = '';
      return;
    }

    let html = `<nav class="pagination is-small is-centered" role="navigation" aria-label="pagination">`;
    html += `<a class="pagination-previous" ${currentPage === 1 ? 'disabled' : `onclick="changePage(${currentPage - 1})"`}>Previous</a>`;
    html += `<a class="pagination-next" ${currentPage === totalPages ? 'disabled' : `onclick="changePage(${currentPage + 1})"`}>Next page</a>`;
    html += `<ul class="pagination-list">`;
    
    for (let i = 1; i <= totalPages; i++) {
        if (i === 1 || i === totalPages || (i >= currentPage - 1 && i <= currentPage + 1)) {
            html += `<li><a class="pagination-link ${i === currentPage ? 'is-current' : ''}" onclick="changePage(${i})">${i}</a></li>`;
        } else if (i === currentPage - 2 || i === currentPage + 2) {
            html += `<li><span class="pagination-ellipsis">&hellip;</span></li>`;
        }
    }
    
    html += `</ul></nav>`;
    container.innerHTML = html;
  }
</script>
<script>
  // Setup SSE Debounce to prevent flickering
  let sseTimer = null;
  const SSE_DEBOUNCE_MS = 2000;

  function changePage(p) {
    currentPage = p;
    loadNotifications();
    window.scrollTo({ top: 0, behavior: 'smooth' });
  }

  document.addEventListener('DOMContentLoaded', loadNotifications);

  window.addEventListener('brilliant:notification_received', (e) => {
    console.log("[SSE] Notification received, refreshing list...");
    // Debounce to prevent multiple refreshes during bulk sync
    clearTimeout(sseTimer);
    sseTimer = setTimeout(() => {
      loadNotifications();
    }, SSE_DEBOUNCE_MS);
  });

  window.addEventListener('brilliant:notifications_synced', (e) => {
    console.log("[SSE] Notifications synced, refreshing list...");
    // If we've already set a timer for persistent updates, clear it.
    // A full sync completion event should trigger regardless but respect debounce.
    clearTimeout(sseTimer);
    sseTimer = setTimeout(() => {
      loadNotifications();
    }, 500); // 500ms after final sync is usually safe
  });
</script>
