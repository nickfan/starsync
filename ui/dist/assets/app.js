const AUTO_SYNC_KEY = "starsync.autoSyncMs";

const state = {
  page: 1,
  perPage: 25,
  sort: "updated",
  direction: "desc",
  total: 0,
  nextCursor: null,
  searchBusy: false,
  tasks: new Map(),
  eventsPrimed: false,
  seenEvents: new Set(),
  autoTimer: null,
  nextAutoAt: null,
};

const els = {
  status: document.querySelector("#status"),
  serviceVersion: document.querySelector("#serviceVersion"),
  serviceState: document.querySelector("#serviceState"),
  lastEvent: document.querySelector("#lastEvent"),
  nextAutoSync: document.querySelector("#nextAutoSync"),
  syncButton: document.querySelector("#syncButton"),
  enrichButton: document.querySelector("#enrichButton"),
  enrichListsButton: document.querySelector("#enrichListsButton"),
  searchButton: document.querySelector("#searchButton"),
  clearButton: document.querySelector("#clearButton"),
  query: document.querySelector("#query"),
  owner: document.querySelector("#owner"),
  language: document.querySelector("#language"),
  topic: document.querySelector("#topic"),
  tag: document.querySelector("#tag"),
  list: document.querySelector("#list"),
  repoStatus: document.querySelector("#repoStatus"),
  archived: document.querySelector("#archived"),
  activeFilters: document.querySelector("#activeFilters"),
  sortPreset: document.querySelector("#sortPreset"),
  perPage: document.querySelector("#perPage"),
  totalCount: document.querySelector("#totalCount"),
  visibleRange: document.querySelector("#visibleRange"),
  pageNumber: document.querySelector("#pageNumber"),
  prevPage: document.querySelector("#prevPage"),
  nextPage: document.querySelector("#nextPage"),
  results: document.querySelector("#results"),
  autoSync: document.querySelector("#autoSync"),
  taskCount: document.querySelector("#taskCount"),
  taskList: document.querySelector("#taskList"),
  eventList: document.querySelector("#eventList"),
  refreshEvents: document.querySelector("#refreshEvents"),
  toasts: document.querySelector("#toasts"),
};

async function getJson(path, options = {}) {
  const response = await fetch(path, {
    headers: { accept: "application/json", ...(options.headers || {}) },
    ...options,
  });
  if (!response.ok) {
    const text = await response.text();
    let message = text;
    try {
      message = JSON.parse(text).error || text;
    } catch (_error) {
    }
    throw new Error(message || `${response.status} ${response.statusText}`);
  }
  return response.json();
}

function setStatus(text, tone = "neutral") {
  els.status.textContent = text;
  els.status.dataset.tone = tone;
  els.serviceState.textContent = text;
}

function setSearchBusy(value) {
  state.searchBusy = value;
  els.searchButton.disabled = value;
  els.prevPage.disabled = value || state.page <= 1;
  els.nextPage.disabled = value || state.page * state.perPage >= state.total;
}

function formatNumber(value) {
  return new Intl.NumberFormat().format(value || 0);
}

function shortTime(value) {
  if (!value) return "-";
  return new Intl.DateTimeFormat(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  }).format(new Date(value));
}

function toast(message, tone = "neutral") {
  const item = document.createElement("div");
  item.className = "toast";
  item.dataset.tone = tone;
  item.textContent = message;
  els.toasts.append(item);
  window.setTimeout(() => item.remove(), 5200);
}

function syncSortFromPreset() {
  const [sort, direction] = String(els.sortPreset.value || "updated:desc").split(":");
  state.sort = sort || "updated";
  state.direction = direction || "desc";
}

function paramsFromForm() {
  syncSortFromPreset();
  const params = new URLSearchParams();
  const fields = [
    ["q", els.query.value],
    ["owner", els.owner.value],
    ["language", els.language.value],
    ["topic", els.topic.value],
    ["tag", els.tag.value],
    ["list", els.list.value],
    ["status", els.repoStatus.value],
    ["archived", els.archived.value],
    ["sort", state.sort],
    ["direction", state.direction],
  ];
  for (const [key, value] of fields) {
    const trimmed = String(value || "").trim();
    if (trimmed) params.set(key, trimmed);
  }
  params.set("page", String(state.page));
  params.set("per_page", String(state.perPage));
  return params;
}

function repoFromItem(item) {
  return item.repo ? item.repo : item;
}

function repoTitle(repo) {
  return `${repo.owner}/${repo.name}`;
}

function repoDescription(repo) {
  return repo.description || repo.user?.summary || "No description yet.";
}

function repoTags(repo) {
  const seen = new Set();
  const chips = [];
  const push = (kind, value) => {
    const label = String(value || "").trim();
    if (!label) return;
    const key = `${kind}:${label.toLowerCase()}`;
    if (seen.has(key)) return;
    seen.add(key);
    chips.push({ kind, label });
  };
  for (const tag of repo.user?.tags || []) push("tag", tag);
  for (const list of repo.user?.lists || []) push("list", list);
  for (const list of repo.github_lists || []) push("list", list);
  for (const topic of repo.topics || []) push("topic", topic);
  return chips.slice(0, 12);
}

function filterFields() {
  return [
    { key: "owner", label: "owner", el: els.owner },
    { key: "language", label: "language", el: els.language },
    { key: "topic", label: "topic", el: els.topic },
    { key: "tag", label: "tag", el: els.tag },
    { key: "list", label: "list", el: els.list },
    { key: "status", label: "status", el: els.repoStatus },
    { key: "archived", label: "visibility", el: els.archived },
  ];
}

function renderActiveFilters() {
  els.activeFilters.replaceChildren();
  for (const field of filterFields()) {
    const value = String(field.el.value || "").trim();
    if (!value) continue;
    const pill = document.createElement("span");
    pill.className = "filter-pill";
    const label = document.createElement("span");
    label.textContent = `${field.label}:${value}`;
    const remove = document.createElement("button");
    remove.type = "button";
    remove.title = `Remove ${field.label} filter`;
    remove.textContent = "x";
    remove.addEventListener("click", () => {
      field.el.value = "";
      state.page = 1;
      renderActiveFilters();
      search();
    });
    pill.append(label, remove);
    els.activeFilters.append(pill);
  }
}

function applyChipFilter(kind, value) {
  const target = kind === "topic" ? els.topic : kind === "tag" ? els.tag : els.list;
  target.value = value;
  state.page = 1;
  renderActiveFilters();
  search();
}

function renderSummary(payload, itemCount) {
  state.total = payload.total || 0;
  state.nextCursor = payload.next_cursor || null;
  const first = state.total === 0 ? 0 : (state.page - 1) * state.perPage + 1;
  const last = Math.min(state.total, first + itemCount - 1);
  els.totalCount.textContent = formatNumber(state.total);
  els.visibleRange.textContent = state.total === 0 ? "0" : `${first}-${last}`;
  els.pageNumber.textContent = String(state.page);
  els.prevPage.disabled = state.searchBusy || state.page <= 1;
  els.nextPage.disabled = state.searchBusy || state.page * state.perPage >= state.total;
}

function renderResults(payload) {
  const items = payload.items || [];
  renderSummary(payload, items.length);
  els.results.replaceChildren();

  if (items.length === 0) {
    const empty = document.createElement("p");
    empty.className = "empty";
    empty.textContent = "No repositories matched.";
    els.results.append(empty);
    return;
  }

  for (const item of items) {
    const repo = repoFromItem(item);
    const article = document.createElement("article");
    article.className = "repo";

    const header = document.createElement("div");
    header.className = "repo-header";

    const title = document.createElement("a");
    title.href = repo.html_url || "#";
    title.target = "_blank";
    title.rel = "noreferrer";
    title.textContent = repoTitle(repo);

    const meta = document.createElement("span");
    meta.textContent = [
      repo.language,
      repo.user?.status,
      repo.stargazers_count !== null && repo.stargazers_count !== undefined
        ? `${formatNumber(repo.stargazers_count)} stars`
        : "",
      repo.forks_count !== null && repo.forks_count !== undefined
        ? `${formatNumber(repo.forks_count)} forks`
        : "",
    ]
      .filter(Boolean)
      .join(" · ");

    header.append(title, meta);

    const description = document.createElement("p");
    description.textContent = repoDescription(repo);

    const tagWrap = document.createElement("div");
    tagWrap.className = "tags";
    for (const tag of repoTags(repo)) {
      const chip = document.createElement("button");
      chip.type = "button";
      chip.textContent = tag.label;
      chip.title = `Filter by ${tag.kind}:${tag.label}`;
      chip.addEventListener("click", () => applyChipFilter(tag.kind, tag.label));
      tagWrap.append(chip);
    }

    article.append(header, description, tagWrap);
    if (item.snippet) {
      const snippet = document.createElement("blockquote");
      snippet.textContent = item.snippet;
      article.append(snippet);
    }
    els.results.append(article);
  }
}

async function loadHealth() {
  try {
    const health = await getJson("/health");
    els.serviceVersion.textContent = `v${health.version}`;
    setStatus("Ready", "ok");
  } catch (_error) {
    els.serviceVersion.textContent = "-";
    setStatus("Offline", "bad");
  }
}

async function search({ resetPage = false } = {}) {
  if (state.searchBusy) return;
  if (resetPage) state.page = 1;
  setSearchBusy(true);
  try {
    const params = paramsFromForm();
    const path = params.get("q") ? `/search?${params}` : `/repos?${params}`;
    const payload = await getJson(path);
    renderResults(payload);
    setStatus("Ready", "ok");
  } catch (error) {
    setStatus(error.message, "bad");
    toast(error.message, "bad");
  } finally {
    setSearchBusy(false);
  }
}

function summarizeTaskPayload(kind, payload) {
  if (payload?.accepted && payload?.job_id) {
    return `${payload.message || "Queued"} · ${payload.job_id.slice(0, 8)}`;
  }
  if (kind === "sync") return "Sync queued";
  if (kind === "enrich_lists") return "GitHub Lists enrichment queued";
  return "README enrichment queued";
}

function renderTasks() {
  const tasks = Array.from(state.tasks.values()).sort((a, b) => b.createdAt - a.createdAt);
  const running = tasks.filter((task) => task.status === "running").length;
  els.taskCount.textContent = `${running} running`;
  els.taskList.replaceChildren();

  if (tasks.length === 0) {
    const empty = document.createElement("p");
    empty.className = "muted-line";
    empty.textContent = "No task history.";
    els.taskList.append(empty);
    return;
  }

  for (const task of tasks.slice(0, 6)) {
    const row = document.createElement("div");
    row.className = "task-row";
    row.dataset.status = task.status;

    const name = document.createElement("strong");
    name.textContent = task.label;

    const detail = document.createElement("span");
    detail.textContent = task.detail || task.status;

    const time = document.createElement("small");
    time.textContent = shortTime(task.finishedAt || task.createdAt);

    row.append(name, detail, time);
    els.taskList.append(row);
  }
}

function updateTaskButtons() {
  const hasSync = Array.from(state.tasks.values()).some(
    (task) => task.kind === "sync" && task.status === "running",
  );
  const hasEnrich = Array.from(state.tasks.values()).some(
    (task) => isReadmeEnrichKind(task.kind) && task.status === "running",
  );
  const hasListEnrich = Array.from(state.tasks.values()).some(
    (task) => task.kind === "enrich_lists" && task.status === "running",
  );
  els.syncButton.disabled = hasSync;
  els.enrichButton.disabled = hasEnrich;
  els.enrichListsButton.disabled = hasListEnrich;
}

async function runTask(kind, path, label, { quiet = false } = {}) {
  const existing = Array.from(state.tasks.values()).some(
    (task) => task.kind === kind && task.status === "running",
  );
  if (existing) return;

  const id = `${kind}-${Date.now()}`;
  state.tasks.set(id, {
    id,
    kind,
    label,
    status: "running",
    detail: "Running",
    createdAt: Date.now(),
  });
  renderTasks();
  updateTaskButtons();
  if (!quiet) toast(`${label} started`);

  try {
    const payload = await getJson(path, { method: "POST" });
    const jobId = payload.job_id || id;
    const task = state.tasks.get(id);
    task.id = jobId;
    task.status = "running";
    task.detail = summarizeTaskPayload(kind, payload);
    task.jobId = jobId;
    state.tasks.delete(id);
    state.tasks.set(jobId, task);
    renderTasks();
    if (!quiet) toast(`${label} queued`, "ok");
    await loadEvents({ notify: true });
  } catch (error) {
    const task = state.tasks.get(id);
    task.status = "failed";
    task.detail = error.message;
    task.finishedAt = Date.now();
    renderTasks();
    setStatus(error.message, "bad");
    toast(`${label} failed: ${error.message}`, "bad");
  } finally {
    updateTaskButtons();
  }
}

function eventDetails(event) {
  const body = event.event || {};
  const repo = body.repo ? ` · ${body.repo}` : "";
  const summary = body.summary ? ` · ${body.summary}` : "";
  const message = body.message ? ` · ${body.message}` : "";
  return { kind: event.name || body.type || "event", repo: `${repo}${summary}${message}` };
}

function taskLabel(kind) {
  if (kind === "sync") return "Sync Stars";
  if (kind === "auto_sync") return "Auto Sync";
  if (kind === "enrich_readme" || kind === "enrich") return "Enrich README";
  if (kind === "enrich_lists") return "Enrich Lists";
  return kind || "Task";
}

function isReadmeEnrichKind(kind) {
  return kind === "enrich" || kind === "enrich_readme";
}

function applyEventToTasks(event) {
  const body = event.event || {};
  const type = body.type;
  const jobId = body.job_id;
  if (!jobId || !type?.startsWith("task_")) return false;

  let task = state.tasks.get(jobId);
  if (!task) {
    task = {
      id: jobId,
      jobId,
      kind: body.kind,
      label: taskLabel(body.kind),
      createdAt: new Date(event.emitted_at).getTime(),
      status: "running",
      detail: "Running",
    };
    state.tasks.set(jobId, task);
  }

  if (type === "task_started") {
    task.status = "running";
    task.detail = "Running";
  }
  if (type === "task_completed") {
    task.status = "done";
    task.detail = body.summary || "Done";
    task.finishedAt = new Date(event.emitted_at).getTime();
    return true;
  }
  if (type === "task_failed") {
    task.status = "failed";
    task.detail = body.message || "Failed";
    task.finishedAt = new Date(event.emitted_at).getTime();
  }
  return false;
}

function renderEvents(events) {
  els.eventList.replaceChildren();
  if (events.length === 0) {
    const empty = document.createElement("p");
    empty.className = "muted-line";
    empty.textContent = "No events yet.";
    els.eventList.append(empty);
    els.lastEvent.textContent = "-";
    return;
  }

  els.lastEvent.textContent = `${events[0].name} ${shortTime(events[0].emitted_at)}`;
  for (const event of events.slice(0, 8)) {
    const { kind, repo } = eventDetails(event);
    const row = document.createElement("div");
    row.className = "event-row";
    const name = document.createElement("strong");
    name.textContent = kind;
    const detail = document.createElement("span");
    detail.textContent = `${shortTime(event.emitted_at)}${repo}`;
    row.append(name, detail);
    els.eventList.append(row);
  }
}

async function loadEvents({ notify = false } = {}) {
  try {
    const events = await getJson("/events/recent?limit=12");
    renderEvents(events);
    let shouldRefreshResults = false;
    if (state.eventsPrimed && notify) {
      for (const event of events.slice().reverse()) {
        if (!state.seenEvents.has(event.id)) {
          if (applyEventToTasks(event)) {
            shouldRefreshResults = true;
          }
          toast(event.name, "ok");
        }
      }
    }
    if (!state.eventsPrimed) {
      for (const event of events.slice().reverse()) {
        applyEventToTasks(event);
      }
    }
    state.seenEvents = new Set(events.map((event) => event.id));
    state.eventsPrimed = true;
    renderTasks();
    updateTaskButtons();
    if (shouldRefreshResults) {
      await search();
    }
  } catch (error) {
    toast(error.message, "bad");
  }
}

function updateAutoSyncLabel() {
  const interval = Number(els.autoSync.value || 0);
  if (!interval || !state.nextAutoAt) {
    els.nextAutoSync.textContent = "Off";
    return;
  }
  const minutes = Math.max(1, Math.round((state.nextAutoAt - Date.now()) / 60000));
  els.nextAutoSync.textContent = `${minutes} min`;
}

function scheduleAutoSync() {
  if (state.autoTimer) {
    window.clearTimeout(state.autoTimer);
    state.autoTimer = null;
  }
  const interval = Number(els.autoSync.value || 0);
  localStorage.setItem(AUTO_SYNC_KEY, String(interval));
  if (!interval) {
    state.nextAutoAt = null;
    updateAutoSyncLabel();
    return;
  }

  state.nextAutoAt = Date.now() + interval;
  updateAutoSyncLabel();
  state.autoTimer = window.setTimeout(async () => {
    await runTask("sync", "/sync", "Auto Sync", { quiet: true });
    scheduleAutoSync();
  }, interval);
}

function clearFilters() {
  for (const input of [els.query, els.owner, els.language, els.topic, els.tag, els.list]) {
    input.value = "";
  }
  els.repoStatus.value = "";
  els.archived.value = "";
  state.page = 1;
  renderActiveFilters();
  search();
}

for (const input of [els.query, els.owner, els.language, els.topic, els.tag, els.list]) {
  input.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      renderActiveFilters();
      search({ resetPage: true });
    }
  });
}

for (const select of [els.repoStatus, els.archived, els.sortPreset, els.perPage]) {
  select.addEventListener("change", () => {
    if (select === els.sortPreset) syncSortFromPreset();
    if (select === els.perPage) state.perPage = Number(els.perPage.value);
    renderActiveFilters();
    search({ resetPage: true });
  });
}

els.searchButton.addEventListener("click", () => {
  renderActiveFilters();
  search({ resetPage: true });
});
els.clearButton.addEventListener("click", clearFilters);
els.prevPage.addEventListener("click", () => {
  state.page = Math.max(1, state.page - 1);
  search();
});
els.nextPage.addEventListener("click", () => {
  if (state.page * state.perPage < state.total) {
    state.page += 1;
    search();
  }
});
els.syncButton.addEventListener("click", () => runTask("sync", "/sync", "Sync Stars"));
els.enrichButton.addEventListener("click", () =>
  runTask("enrich", "/enrich/readme?limit=50", "Enrich README"),
);
els.enrichListsButton.addEventListener("click", () =>
  runTask("enrich_lists", "/enrich/lists", "Enrich Lists"),
);
els.refreshEvents.addEventListener("click", () => loadEvents({ notify: true }));
els.autoSync.addEventListener("change", scheduleAutoSync);

const storedAutoSync = localStorage.getItem(AUTO_SYNC_KEY);
if (storedAutoSync && [...els.autoSync.options].some((option) => option.value === storedAutoSync)) {
  els.autoSync.value = storedAutoSync;
}
els.sortPreset.value = `${state.sort}:${state.direction}`;
els.perPage.value = String(state.perPage);
renderTasks();
renderActiveFilters();
scheduleAutoSync();
window.setInterval(updateAutoSyncLabel, 30000);
window.setInterval(() => loadEvents({ notify: true }), 5000);

await loadHealth();
await Promise.all([loadEvents(), search()]);
