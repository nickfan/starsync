const state = {
  limit: 50,
  busy: false,
};

const els = {
  status: document.querySelector("#status"),
  syncButton: document.querySelector("#syncButton"),
  enrichButton: document.querySelector("#enrichButton"),
  searchButton: document.querySelector("#searchButton"),
  query: document.querySelector("#query"),
  owner: document.querySelector("#owner"),
  language: document.querySelector("#language"),
  tag: document.querySelector("#tag"),
  repoStatus: document.querySelector("#repoStatus"),
  sort: document.querySelector("#sort"),
  direction: document.querySelector("#direction"),
  resultCount: document.querySelector("#resultCount"),
  nextCursor: document.querySelector("#nextCursor"),
  results: document.querySelector("#results"),
};

async function getJson(path, options) {
  const response = await fetch(path, {
    headers: { accept: "application/json" },
    ...options,
  });
  if (!response.ok) {
    const text = await response.text();
    throw new Error(text || `${response.status} ${response.statusText}`);
  }
  return response.json();
}

function setStatus(text, tone = "neutral") {
  els.status.textContent = text;
  els.status.dataset.tone = tone;
}

function setBusy(value) {
  state.busy = value;
  for (const button of [els.syncButton, els.enrichButton, els.searchButton]) {
    button.disabled = value;
  }
}

function paramsFromForm() {
  const params = new URLSearchParams();
  const fields = [
    ["q", els.query.value],
    ["owner", els.owner.value],
    ["language", els.language.value],
    ["tag", els.tag.value],
    ["status", els.repoStatus.value],
    ["sort", els.sort.value],
    ["direction", els.direction.value],
  ];
  for (const [key, value] of fields) {
    const trimmed = value.trim();
    if (trimmed) params.set(key, trimmed);
  }
  params.set("limit", String(state.limit));
  return params;
}

function repoTitle(repo) {
  return `${repo.owner}/${repo.name}`;
}

function repoDescription(repo) {
  return repo.description || repo.user?.summary || "No description yet.";
}

function repoTags(repo) {
  const topics = repo.topics || [];
  const tags = repo.user?.tags || [];
  return [...new Set([...tags, ...topics])].slice(0, 8);
}

function renderResults(payload) {
  const items = payload.items || [];
  els.resultCount.textContent = String(items.length);
  els.nextCursor.textContent = payload.next_cursor || "-";
  els.results.replaceChildren();

  if (items.length === 0) {
    const empty = document.createElement("p");
    empty.className = "empty";
    empty.textContent = "No repositories matched.";
    els.results.append(empty);
    return;
  }

  for (const item of items) {
    const repo = item.repo ? item.repo : item;
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
      repo.stargazers_count ? `${repo.stargazers_count} stars` : "",
    ]
      .filter(Boolean)
      .join(" · ");

    header.append(title, meta);

    const description = document.createElement("p");
    description.textContent = repoDescription(repo);

    const tagWrap = document.createElement("div");
    tagWrap.className = "tags";
    for (const tag of repoTags(repo)) {
      const chip = document.createElement("span");
      chip.textContent = tag;
      tagWrap.append(chip);
    }

    if (item.snippet) {
      const snippet = document.createElement("blockquote");
      snippet.textContent = item.snippet;
      article.append(header, description, tagWrap, snippet);
    } else {
      article.append(header, description, tagWrap);
    }
    els.results.append(article);
  }
}

async function loadHealth() {
  try {
    const health = await getJson("/health");
    setStatus(`Ready v${health.version}`, "ok");
  } catch (error) {
    setStatus("Offline", "bad");
  }
}

async function search() {
  if (state.busy) return;
  setBusy(true);
  try {
    const params = paramsFromForm();
    const path = params.get("q") ? `/search?${params}` : `/repos?${params}`;
    const payload = await getJson(path);
    renderResults(payload);
    setStatus("Ready", "ok");
  } catch (error) {
    setStatus(error.message, "bad");
  } finally {
    setBusy(false);
  }
}

async function runAction(path, label) {
  if (state.busy) return;
  setBusy(true);
  setStatus(`${label} running`, "neutral");
  let refresh = false;
  try {
    await getJson(path, { method: "POST" });
    setStatus(`${label} done`, "ok");
    refresh = true;
  } catch (error) {
    setStatus(error.message, "bad");
  } finally {
    setBusy(false);
  }
  if (refresh) await search();
}

els.searchButton.addEventListener("click", search);
els.query.addEventListener("keydown", (event) => {
  if (event.key === "Enter") search();
});
els.syncButton.addEventListener("click", () => runAction("/sync", "Sync"));
els.enrichButton.addEventListener("click", () =>
  runAction("/enrich/readme?limit=50", "README enrichment"),
);

await loadHealth();
await search();
