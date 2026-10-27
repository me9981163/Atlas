(function () {
  "use strict";

  const SVG_NS = "http://www.w3.org/2000/svg";
  const NODE_HEIGHT = 46;
  const MAX_RESULTS = 40;

  const $ = (id) => document.getElementById(id);

  const state = {
    lang: readStorage("atlas.lang") === "ar" ? "ar" : "en",
    status: { key: "statusReady", vars: {}, busy: false },
    repo: null,
    stats: null,
    results: [],
    resultsMode: "browse",
    resultsQuery: "",
    searchMode:
      readStorage("atlas.searchMode") === "semantic" ? "semantic" : "lexical",
    searchMessage: null,
    searching: false,
    requestSeq: 0,
    semantic: null,
    semanticSeq: 0,
    semanticBusy: false,
    semanticProgress: null,
    selectionSeq: 0,
    selectedId: null,
    details: null,
    graph: null,
    graphById: new Map(),
    nodePositions: new Map(),
    graphTransform: { x: 0, y: 0, k: 1 },
    graphHistory: [],
    callers: [],
    callees: [],
    impact: [],
    impactDepth: 5,
    graphDepth: 2,
    graphLimit: 80,
    tab: "callers",
    busy: false,
    indexing: false,
    lastWarnings: [],
    drag: null,
  };

  const tauri = window.__TAURI__;
  const invoke =
    tauri && tauri.core && typeof tauri.core.invoke === "function"
      ? tauri.core.invoke.bind(tauri.core)
      : null;
  const dialog = tauri && tauri.dialog ? tauri.dialog : null;

  const dom = {
    openForm: $("open-form"),
    repoPath: $("repo-path"),
    browseBtn: $("browse-btn"),
    openBtn: $("open-btn"),
    indexBtn: $("index-btn"),
    langBtn: $("lang-btn"),
    repoSummary: $("repo-summary"),
    repoRoot: $("repo-root"),
    repoDb: $("repo-db"),
    appStatus: $("app-status"),
    closeBtn: $("close-btn"),
    indexState: $("index-state"),
    notIndexed: $("not-indexed"),
    statFiles: $("stat-files"),
    statSymbols: $("stat-symbols"),
    statEdges: $("stat-edges"),
    statResolved: $("stat-resolved"),
    accuracyText: $("accuracy-text"),
    barResolved: $("bar-resolved"),
    barExternal: $("bar-external"),
    barUnresolved: $("bar-unresolved"),
    legendResolved: $("legend-resolved"),
    legendExternal: $("legend-external"),
    legendUnresolved: $("legend-unresolved"),
    warningsBox: $("warnings-box"),
    warningsSummary: $("warnings-summary"),
    warningsList: $("warnings-list"),
    searchForm: $("search-form"),
    searchInput: $("search-input"),
    searchBtn: $("search-btn"),
    searchCount: $("search-count"),
    searchState: $("search-state"),
    searchResults: $("search-results"),
    searchHint: $("search-hint"),
    modeLexical: $("mode-lexical"),
    modeSemantic: $("mode-semantic"),
    semanticPanel: $("semantic-panel"),
    semanticModel: $("semantic-model"),
    semanticDetail: $("semantic-detail"),
    semanticPill: $("semantic-pill"),
    semanticProgress: $("semantic-progress"),
    semanticProgressFill: $("semantic-progress-fill"),
    semanticProgressText: $("semantic-progress-text"),
    downloadModelBtn: $("download-model-btn"),
    embedBtn: $("embed-btn"),
    confirmModal: $("confirm-modal"),
    confirmTitle: $("confirm-title"),
    confirmMessage: $("confirm-message"),
    confirmCancel: $("confirm-cancel"),
    confirmOk: $("confirm-ok"),
    graphDepth: $("graph-depth"),
    graphLimit: $("graph-limit"),
    graphBack: $("graph-back"),
    graphFit: $("graph-fit"),
    zoomIn: $("zoom-in"),
    zoomOut: $("zoom-out"),
    graph: $("graph"),
    graphViewport: $("graph-viewport"),
    graphEdges: $("graph-edges"),
    graphNodes: $("graph-nodes"),
    graphEmpty: $("graph-empty"),
    graphLoading: $("graph-loading"),
    graphTruncated: $("graph-truncated"),
    graphMeta: $("graph-meta"),
    detailEmpty: $("detail-empty"),
    detail: $("detail"),
    detailKind: $("detail-kind"),
    detailName: $("detail-name"),
    detailQName: $("detail-qname"),
    detailLocation: $("detail-location"),
    detailSignatureBox: $("detail-signature-box"),
    detailSignature: $("detail-signature"),
    detailDocBox: $("detail-doc-box"),
    detailDoc: $("detail-doc"),
    tabCountCallers: $("tab-count-callers"),
    tabCountCallees: $("tab-count-callees"),
    tabCountImpact: $("tab-count-impact"),
    tabs: Array.from(document.querySelectorAll(".tab")),
    tabContent: $("tab-content"),
    impactControls: $("impact-controls"),
    impactDepth: $("impact-depth"),
    toastContainer: $("toast-container"),
  };

  function t(key, vars) {
    if (!window.ATLAS_I18N) return key;
    return window.ATLAS_I18N.t(state.lang, key, vars);
  }

  function readStorage(key) {
    try {
      return window.localStorage.getItem(key);
    } catch {
      return null;
    }
  }

  function writeStorage(key, value) {
    try {
      window.localStorage.setItem(key, value);
    } catch {
      // Persistence is a convenience; the app works without it.
    }
  }

  function normalizeError(error) {
    if (typeof error === "string") return error;
    if (error && typeof error.message === "string") return error.message;
    try {
      return JSON.stringify(error);
    } catch {
      return String(error);
    }
  }

  function formatNumber(value) {
    const number = Number(value);
    if (!Number.isFinite(number)) return "—";
    return new Intl.NumberFormat(state.lang === "ar" ? "ar" : "en-US").format(
      number,
    );
  }

  function clamp(value, minimum, maximum) {
    return Math.min(maximum, Math.max(minimum, value));
  }

  function formatBytes(bytes) {
    const value = Number(bytes) || 0;
    let formatted;
    if (value >= 1024 * 1024) {
      formatted = `${(value / (1024 * 1024)).toFixed(1)} MB`;
    } else if (value >= 1024) {
      formatted = `${(value / 1024).toFixed(1)} KB`;
    } else {
      formatted = `${value} B`;
    }
    // Keep the number and unit in left-to-right order inside RTL text.
    return `\u2068${formatted}\u2069`;
  }

  function setButtonLabel(button, key) {
    const label = button.querySelector("span");
    if (label) label.textContent = t(key);
  }

  function el(tag, className, text) {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text !== undefined) node.textContent = text;
    return node;
  }

  function svgEl(tag, attrs) {
    const node = document.createElementNS(SVG_NS, tag);
    if (attrs) {
      for (const [key, value] of Object.entries(attrs)) {
        node.setAttribute(key, String(value));
      }
    }
    return node;
  }

  function clearNode(node) {
    while (node.firstChild) node.removeChild(node.firstChild);
  }

  function firstLine(text) {
    return String(text || "")
      .split(/\r?\n/)[0]
      .trim();
  }

  function truncateText(text, maximum) {
    const value = String(text || "");
    if (value.length <= maximum) return value;
    return `${value.slice(0, maximum - 1)}…`;
  }

  function normalizedKind(kind) {
    return String(kind || "unknown")
      .toLowerCase()
      .replace(/[^a-z0-9]/g, "");
  }

  function kindClass(kind) {
    return `kind-${normalizedKind(kind) || "unknown"}`;
  }

  function kindLabel(kind) {
    if (!kind) return t("unknown");
    const key = `kind_${normalizedKind(kind)}`;
    const label = t(key);
    return label === key ? String(kind) : label;
  }

  function resolutionClass(resolution) {
    if (!resolution) return "unresolved";
    if (
      resolution === "import" ||
      resolution === "same_file" ||
      resolution === "unique_name" ||
      resolution === "external"
    ) {
      return resolution;
    }
    return "unique_name";
  }

  function resolutionLabel(resolution) {
    if (!resolution) return t("unresolvedCall");
    if (resolution === "external") return t("externalCall");
    const key = `resolution_${resolution}`;
    const label = t(key);
    return label === key ? resolution : label;
  }

  function sideLabel(side) {
    const keys = {
      root: "sideRoot",
      caller: "sideCaller",
      callee: "sideCallee",
      both: "sideBoth",
    };
    return t(keys[side] || "unknown");
  }

  function showToast(message, type = "info", timeout = 5200) {
    if (!message) return;
    const toast = el("div", `toast ${type}`, message);
    toast.setAttribute("role", type === "error" ? "alert" : "status");
    dom.toastContainer.appendChild(toast);
    while (dom.toastContainer.children.length > 4) {
      dom.toastContainer.removeChild(dom.toastContainer.firstChild);
    }
    window.setTimeout(() => {
      if (toast.parentNode) toast.parentNode.removeChild(toast);
    }, timeout);
  }

  function handleError(prefixKey, error) {
    showToast(t(prefixKey, { error: normalizeError(error) }), "error", 8000);
  }

  function setStatus(key, vars = {}, busy = false) {
    state.status = { key, vars, busy };
    renderStatus();
  }

  function renderStatus() {
    dom.appStatus.textContent = t(state.status.key, state.status.vars);
    dom.appStatus.classList.toggle("busy", Boolean(state.status.busy));
  }

  function setBusy(busy, statusKey = "statusLoading", vars = {}) {
    state.busy = busy;
    if (busy) {
      setStatus(statusKey, vars, true);
    } else {
      setStatus("statusReady");
    }
    updateControls();
  }

  function applyLanguage(lang, options = {}) {
    const { persist = true, render = true } = options;
    state.lang = lang === "ar" ? "ar" : "en";
    if (persist) writeStorage("atlas.lang", state.lang);

    document.documentElement.lang = state.lang;
    document.documentElement.dir = state.lang === "ar" ? "rtl" : "ltr";
    document.title = `${t("appName")} — ${t("tagline")}`;

    document.querySelectorAll("[data-i18n]").forEach((node) => {
      node.textContent = t(node.dataset.i18n);
    });
    document
      .querySelectorAll("[data-i18n-placeholder]")
      .forEach((node) => {
        node.placeholder = t(node.dataset.i18nPlaceholder);
      });
    document
      .querySelectorAll("[data-i18n-aria-label]")
      .forEach((node) => {
        node.setAttribute("aria-label", t(node.dataset.i18nAriaLabel));
      });

    dom.langBtn.textContent = t("switchLanguage");
    dom.graph.setAttribute("aria-label", t("graphTitle"));
    dom.searchBtn.setAttribute("aria-label", t("search"));
    renderStatus();

    if (!render) return;
    renderRepository();
    renderStats();
    renderSearch();
    renderSearchMode();
    renderDetails();
    renderTabs();
    if (state.graph) renderGraph({ preserveZoom: true });
    updateControls();
  }

  function updateControls() {
    const hasRepo = Boolean(state.repo);
    const indexed = Boolean(state.repo && state.repo.indexed);
    const hasSelection = state.selectedId !== null;

    dom.openBtn.disabled = state.busy;
    dom.browseBtn.disabled = state.busy;
    dom.indexBtn.disabled = state.busy || !hasRepo;
    dom.closeBtn.disabled = state.busy || !hasRepo;
    dom.searchInput.disabled = state.busy || !indexed;
    dom.searchBtn.disabled = state.busy || !indexed;
    dom.modeLexical.disabled = state.busy;
    dom.modeSemantic.disabled = state.busy;
    dom.graphDepth.disabled = !hasSelection;
    dom.graphLimit.disabled = !hasSelection;
    dom.graphBack.disabled = !hasSelection || state.graphHistory.length === 0;
    dom.graphFit.disabled = !state.graph;
    dom.zoomIn.disabled = !state.graph;
    dom.zoomOut.disabled = !state.graph;

    const label = dom.indexBtn.querySelector("span");
    if (label) label.textContent = state.indexing ? t("indexing") : t("index");
    renderSemantic();
  }

  function renderRepository() {
    const repo = state.repo;
    dom.repoSummary.classList.toggle("hidden", !repo);
    if (!repo) {
      dom.indexState.textContent = "—";
      dom.indexState.className = "pill muted-pill";
      dom.notIndexed.classList.add("hidden");
      return;
    }

    dom.repoRoot.textContent = repo.root;
    dom.repoRoot.title = repo.root;
    dom.repoDb.textContent = repo.db_path;
    dom.repoDb.title = repo.db_path;
    dom.indexState.textContent = repo.indexed
      ? t("indexedState")
      : t("notIndexed");
    dom.indexState.className = `pill ${
      repo.indexed ? "ok-pill" : "warn-pill"
    }`;
    dom.notIndexed.classList.toggle("hidden", repo.indexed);
  }

  function renderStats() {
    const stats = state.stats;
    if (!stats) {
      dom.statFiles.textContent = "—";
      dom.statSymbols.textContent = "—";
      dom.statEdges.textContent = "—";
      dom.statResolved.textContent = "—";
      dom.accuracyText.textContent = "—";
      dom.accuracyText.removeAttribute("title");
      dom.barResolved.style.width = "0%";
      dom.barExternal.style.width = "0%";
      dom.barUnresolved.style.width = "0%";
      dom.legendResolved.textContent = "—";
      dom.legendExternal.textContent = "—";
      dom.legendUnresolved.textContent = "—";
      renderWarnings();
      return;
    }

    const edges = Number(stats.edge_count) || 0;
    const resolved = Number(stats.resolved_edge_count) || 0;
    const external = Number(stats.external_edge_count) || 0;
    const unresolved = Number(stats.unresolved_edge_count) || 0;
    const resolvedPercent = edges ? (resolved / edges) * 100 : 0;
    const externalPercent = edges ? (external / edges) * 100 : 0;
    const unresolvedPercent = edges ? (unresolved / edges) * 100 : 0;

    dom.statFiles.textContent = formatNumber(stats.file_count);
    dom.statSymbols.textContent = formatNumber(stats.symbol_count);
    dom.statEdges.textContent = formatNumber(edges);
    dom.statResolved.textContent = formatNumber(resolved);
    dom.accuracyText.textContent = edges
      ? `${resolvedPercent.toFixed(1)}%`
      : "—";
    dom.accuracyText.title = t("accuracyBar", {
      resolved: resolvedPercent.toFixed(1),
      external: externalPercent.toFixed(1),
      unresolved: unresolvedPercent.toFixed(1),
    });
    dom.barResolved.style.width = `${resolvedPercent}%`;
    dom.barExternal.style.width = `${externalPercent}%`;
    dom.barUnresolved.style.width = `${unresolvedPercent}%`;
    dom.legendResolved.textContent = formatNumber(resolved);
    dom.legendExternal.textContent = formatNumber(external);
    dom.legendUnresolved.textContent = formatNumber(unresolved);
    renderWarnings();
  }

  function renderWarnings() {
    const warnings = state.lastWarnings || [];
    if (!warnings.length) {
      dom.warningsBox.classList.add("hidden");
      clearNode(dom.warningsList);
      return;
    }
    dom.warningsBox.classList.remove("hidden");
    dom.warningsSummary.textContent = `${t("indexWarningsTitle")} (${formatNumber(
      warnings.length,
    )})`;
    clearNode(dom.warningsList);
    for (const warning of warnings) {
      const item = el("li");
      const path = el("code", null, warning.path || "");
      path.setAttribute("dir", "ltr");
      item.appendChild(path);
      item.appendChild(document.createTextNode(` — ${warning.message || ""}`));
      dom.warningsList.appendChild(item);
    }
  }

  function renderSemanticProgress() {
    const progress = state.semanticProgress;
    dom.semanticProgress.classList.toggle("hidden", !progress);
    if (!progress) {
      dom.semanticProgressFill.style.width = "0%";
      dom.semanticProgressText.textContent = "";
      return;
    }
    const total = Number(progress.total) || 0;
    const done = Number(progress.done) || 0;
    const percent = total > 0 ? clamp((done / total) * 100, 0, 100) : 0;
    dom.semanticProgressFill.style.width = `${percent}%`;
    dom.semanticProgressText.textContent = progress.label || "";
  }

  function renderSemantic() {
    const status = state.semantic;
    const hasRepo = Boolean(state.repo);
    const indexed = Boolean(state.repo && state.repo.indexed);
    const busy = state.semanticBusy || state.busy;

    dom.semanticPill.className = "pill muted-pill";
    dom.semanticPill.textContent = "—";
    dom.semanticModel.textContent = status
      ? status.model
      : "minishlab/potion-code-16M-v2";
    dom.downloadModelBtn.disabled = true;
    dom.embedBtn.disabled = true;
    setButtonLabel(dom.downloadModelBtn, "downloadModel");
    setButtonLabel(dom.embedBtn, "buildSemanticIndex");

    if (!hasRepo) {
      dom.semanticDetail.textContent = t("openToBegin");
    } else if (!status) {
      dom.semanticDetail.textContent = t("semanticStatusUnavailable");
    } else {
      const total = Number(status.total_symbols) || 0;
      const embedded = Number(status.embedded_symbols) || 0;
      const current = Number(status.current_symbols) || 0;
      const stale = Number(status.stale_symbols) || 0;

      if (!status.model_installed) {
        dom.semanticDetail.textContent = t("semanticDetailMissing", {
          size: formatBytes(status.model_size_bytes || 33554431),
        });
        dom.semanticPill.classList.add("missing");
        dom.semanticPill.textContent = t("semanticPillMissing");
        dom.downloadModelBtn.disabled = busy;
      } else if (!indexed || total === 0) {
        dom.semanticDetail.textContent = t("semanticDetailNoSymbols");
        dom.semanticPill.textContent = t("semanticPillNotBuilt");
      } else if (stale === 0 && current === total) {
        dom.semanticDetail.textContent = t("semanticDetailReady", {
          current: formatNumber(current),
          total: formatNumber(total),
        });
        dom.semanticPill.classList.add("ready");
        dom.semanticPill.textContent = t("semanticPillReady");
        // Everything is current: re-embedding would be a no-op.
        dom.embedBtn.disabled = true;
      } else if (embedded === 0) {
        dom.semanticDetail.textContent = t("semanticDetailNotBuilt", {
          total: formatNumber(total),
        });
        dom.semanticPill.textContent = t("semanticPillNotBuilt");
        dom.embedBtn.disabled = busy;
      } else {
        dom.semanticDetail.textContent = t("semanticDetailStale", {
          current: formatNumber(current),
          total: formatNumber(total),
          stale: formatNumber(stale),
        });
        dom.semanticPill.classList.add("stale");
        dom.semanticPill.textContent = t("semanticPillStale");
        dom.embedBtn.disabled = busy;
        setButtonLabel(dom.embedBtn, "refreshSemanticIndex");
      }
    }
    renderSemanticProgress();
  }

  function renderSearchMode() {
    const semantic = state.searchMode === "semantic";
    dom.modeLexical.classList.toggle("active", !semantic);
    dom.modeSemantic.classList.toggle("active", semantic);
    dom.modeLexical.setAttribute("aria-pressed", String(!semantic));
    dom.modeSemantic.setAttribute("aria-pressed", String(semantic));
    dom.semanticPanel.classList.toggle("hidden", !semantic);
    dom.searchHint.textContent = t(semantic ? "semanticHint" : "searchHint");
    dom.searchInput.placeholder = t(
      semantic ? "semanticPlaceholder" : "searchPlaceholder",
    );
    renderSemantic();
  }

  async function refreshSemanticStatus() {
    if (!invoke || !state.repo) {
      state.semantic = null;
      renderSemantic();
      return;
    }
    const seq = ++state.semanticSeq;
    try {
      const status = await invoke("semantic_status");
      if (seq !== state.semanticSeq) return;
      state.semantic = status;
    } catch (error) {
      if (seq !== state.semanticSeq) return;
      state.semantic = null;
      if (state.searchMode === "semantic") {
        showToast(
          t("semanticStatusError", { error: normalizeError(error) }),
          "warning",
          7000,
        );
      }
    }
    renderSemantic();
  }

  async function setSearchMode(mode) {
    const next = mode === "semantic" ? "semantic" : "lexical";
    if (next === state.searchMode) return;
    state.searchMode = next;
    writeStorage("atlas.searchMode", next);
    state.searchMessage = null;
    renderSearchMode();
    if (next === "semantic") {
      await refreshSemanticStatus();
    }
    const query = dom.searchInput.value.trim();
    if (query) {
      await performSearch(query);
    } else if (state.repo && state.repo.indexed) {
      await loadBrowse({ resetInput: false });
    } else {
      renderSearch();
    }
  }

  let confirmResolve = null;
  let confirmPreviousFocus = null;

  function closeConfirm(result) {
    if (!confirmResolve) return;
    const resolve = confirmResolve;
    confirmResolve = null;
    dom.confirmModal.classList.add("hidden");
    if (confirmPreviousFocus && typeof confirmPreviousFocus.focus === "function") {
      confirmPreviousFocus.focus();
    }
    confirmPreviousFocus = null;
    resolve(result);
  }

  function confirmAction(message, title) {
    // Use an in-app dialog so the confirmation follows the UI language,
    // direction and font shaping instead of a native GTK dialog.
    if (confirmResolve) closeConfirm(false);
    confirmPreviousFocus = document.activeElement;
    dom.confirmTitle.textContent = title || t("downloadModel");
    dom.confirmMessage.textContent = message;
    dom.confirmModal.classList.remove("hidden");
    // Focus the dialog itself, not a button: the event that opened the
    // dialog must not also activate Continue.
    dom.confirmModal.querySelector(".modal-card").focus();
    return new Promise((resolve) => {
      confirmResolve = resolve;
    });
  }

  async function downloadModel() {
    if (!invoke || state.semanticBusy || state.busy) return;
    const size = formatBytes(
      (state.semantic && state.semantic.model_size_bytes) || 33554431,
    );
    const confirmed = await confirmAction(
      t("semanticDownloadConfirm", { size }),
      t("downloadModel"),
    );
    if (!confirmed) return;

    state.semanticBusy = true;
    state.semanticProgress = {
      done: 0,
      total: 1,
      label: t("semanticDownloadStarting"),
    };
    setBusy(true, "statusDownloadingModel");
    renderSemantic();
    try {
      await invoke("download_model");
      showToast(t("semanticDownloadDone"), "success", 7000);
      await refreshSemanticStatus();
    } catch (error) {
      handleError("semanticDownloadError", error);
    } finally {
      state.semanticBusy = false;
      state.semanticProgress = null;
      setBusy(false);
      renderSemantic();
    }
  }

  async function embedRepository() {
    if (!invoke || !state.repo || state.semanticBusy || state.busy) return;
    state.semanticBusy = true;
    state.semanticProgress = {
      done: 0,
      total: 1,
      label: t("semanticEmbedStarting"),
    };
    setBusy(true, "statusEmbedding");
    renderSemantic();
    try {
      const stats = await invoke("embed_repository");
      showToast(
        t("semanticEmbedDone", {
          current: formatNumber(stats.current_symbols),
          total: formatNumber(stats.total_symbols),
        }),
        "success",
        7000,
      );
      await refreshSemanticStatus();
      const query = dom.searchInput.value.trim();
      if (state.searchMode === "semantic" && query) {
        await performSearch(query);
      }
    } catch (error) {
      handleError("semanticEmbedError", error);
    } finally {
      state.semanticBusy = false;
      state.semanticProgress = null;
      setBusy(false);
      renderSemantic();
    }
  }

  function renderSearch() {
    clearNode(dom.searchResults);
    const hasRepo = Boolean(state.repo);
    const indexed = Boolean(state.repo && state.repo.indexed);
    dom.searchCount.textContent = state.results.length
      ? t("resultCount", { count: formatNumber(state.results.length) })
      : "";

    let message = null;
    let type = "info";
    if (state.searching) {
      message = t("searching");
    } else if (state.searchMessage) {
      message = t(state.searchMessage.key, state.searchMessage.vars);
      type = state.searchMessage.type || "info";
    } else if (!hasRepo) {
      message = t("openToBegin");
    } else if (!indexed) {
      message = t("notIndexedHint");
    } else if (!state.results.length) {
      if (state.resultsMode === "semantic") {
        message = t("noSemanticResults");
      } else if (state.resultsMode === "search") {
        message = t("noResults");
      } else {
        message = t("noSymbols");
      }
    }

    if (message) {
      dom.searchState.textContent = message;
      dom.searchState.className = `search-state${
        type === "error" ? " error" : ""
      }`;
      dom.searchState.classList.remove("hidden");
    } else {
      dom.searchState.classList.add("hidden");
    }

    for (const result of state.results) {
      const item = el("li");
      const button = el("button", "result-item");
      button.type = "button";
      if (result.id === state.selectedId) button.classList.add("active");

      const nameRow = el("span", "result-name");
      nameRow.setAttribute("dir", "auto");
      nameRow.appendChild(el("span", null, result.name));
      nameRow.appendChild(
        el(
          "span",
          `kind-pill ${kindClass(result.kind)}`,
          kindLabel(result.kind),
        ),
      );

      const qname = el("span", "result-qname", result.qualified_name);
      qname.setAttribute("dir", "auto");

      const snippetText = firstLine(result.signature || result.doc || "");
      const snippet = snippetText
        ? el("span", "result-snippet", snippetText)
        : null;
      if (snippet) snippet.setAttribute("dir", "auto");

      const meta = el("span", "result-meta");
      meta.setAttribute("dir", "ltr");
      meta.appendChild(el("span", null, `${result.file}:${result.line}`));
      if (
        state.resultsMode === "semantic" &&
        Number.isFinite(Number(result.similarity))
      ) {
        meta.appendChild(
          el(
            "span",
            "similarity-pill",
            `${Math.round(Number(result.similarity) * 100)}%`,
          ),
        );
      }

      button.appendChild(nameRow);
      button.appendChild(qname);
      if (snippet) button.appendChild(snippet);
      button.appendChild(meta);
      button.addEventListener("click", () => {
        selectSymbol(result.id, { history: "reset" });
      });

      item.appendChild(button);
      dom.searchResults.appendChild(item);
    }
  }

  async function loadBrowse(options = {}) {
    const { resetInput = true } = options;
    if (!invoke || !state.repo || !state.repo.indexed) {
      state.results = [];
      state.resultsMode = "browse";
      state.searchMessage = null;
      state.searching = false;
      renderSearch();
      return;
    }
    if (resetInput) dom.searchInput.value = "";
    const seq = ++state.requestSeq;
    state.resultsMode = "browse";
    state.resultsQuery = "";
    state.searchMessage = null;
    state.searching = false;
    try {
      const results = await invoke("list_symbols", { limit: MAX_RESULTS });
      if (seq !== state.requestSeq) return;
      state.results = Array.isArray(results) ? results : [];
      renderSearch();
    } catch (error) {
      if (seq !== state.requestSeq) return;
      state.results = [];
      state.searchMessage = {
        key: "searchError",
        vars: { error: normalizeError(error) },
        type: "error",
      };
      renderSearch();
    }
  }

  async function performSearch(rawQuery) {
    const query = String(rawQuery || "").trim();
    if (!query) {
      await loadBrowse();
      return;
    }
    if (!invoke || !state.repo || !state.repo.indexed) return;

    const semantic = state.searchMode === "semantic";
    if (semantic) {
      if (!state.semantic) {
        await refreshSemanticStatus();
      }
      const status = state.semantic;
      if (!status || !status.model_installed) {
        state.requestSeq += 1;
        state.results = [];
        state.resultsMode = "semantic";
        state.searching = false;
        state.searchMessage = { key: "semanticModelMissing", type: "warning" };
        renderSearch();
        return;
      }
      if (!Number(status.embedded_symbols)) {
        state.requestSeq += 1;
        state.results = [];
        state.resultsMode = "semantic";
        state.searching = false;
        state.searchMessage = { key: "semanticNotBuilt", type: "warning" };
        renderSearch();
        return;
      }
    }

    const seq = ++state.requestSeq;
    state.resultsMode = semantic ? "semantic" : "search";
    state.resultsQuery = query;
    state.results = [];
    state.searchMessage = null;
    state.searching = true;
    renderSearch();

    try {
      const results = await invoke(
        semantic ? "semantic_search" : "search_symbols",
        {
          query,
          limit: MAX_RESULTS,
        },
      );
      if (seq !== state.requestSeq) return;
      state.results = Array.isArray(results) ? results : [];
    } catch (error) {
      if (seq !== state.requestSeq) return;
      state.results = [];
      state.searchMessage = {
        key: "searchError",
        vars: { error: normalizeError(error) },
        type: "error",
      };
    } finally {
      if (seq === state.requestSeq) {
        state.searching = false;
        renderSearch();
      }
    }
  }

  function setRepository(info) {
    state.repo = info;
    state.stats = info.stats;
    state.lastWarnings = [];
    state.results = [];
    state.resultsMode = "browse";
    state.resultsQuery = "";
    state.searchMessage = null;
    state.searching = false;
    state.semantic = null;
    state.semanticProgress = null;
    state.semanticSeq += 1;
    clearSelection();
    renderRepository();
    renderStats();
    renderSearchMode();
    renderSearch();
    updateControls();
    refreshSemanticStatus();
  }

  async function openRepository(path) {
    if (!invoke || state.busy) return;
    const value = String(path !== undefined ? path : dom.repoPath.value).trim();
    if (!value) {
      showToast(t("openHint"), "warning");
      dom.repoPath.focus();
      return;
    }

    setBusy(true, "statusLoading");
    try {
      const info = await invoke("open_repository", { path: value });
      setRepository(info);
      dom.repoPath.value = info.root;
      writeStorage("atlas.repoPath", info.root);
      showToast(t("repoOpened", { path: info.root }), "success");
      if (info.indexed) {
        await loadBrowse({ resetInput: true });
      } else {
        renderSearch();
      }
    } catch (error) {
      handleError("openError", error);
    } finally {
      setBusy(false);
    }
  }

  async function browseRepository() {
    if (state.busy) return;
    if (!dialog || typeof dialog.open !== "function") {
      showToast(t("browseUnavailable"), "warning");
      return;
    }
    try {
      const selected = await dialog.open({
        directory: true,
        multiple: false,
        title: t("chooseFolder"),
      });
      const path =
        typeof selected === "string"
          ? selected
          : Array.isArray(selected)
            ? selected[0]
            : null;
      if (typeof path === "string" && path.trim()) {
        dom.repoPath.value = path;
        await openRepository(path);
      }
    } catch (error) {
      handleError("openError", error);
    }
  }

  async function indexRepository() {
    if (!invoke || !state.repo || state.busy) return;
    state.indexing = true;
    setBusy(true, "statusIndexing");
    try {
      const outcome = await invoke("index_repository");
      state.stats = outcome.stats;
      state.repo = {
        ...state.repo,
        stats: outcome.stats,
        indexed: Number(outcome.stats.file_count) > 0,
      };
      state.lastWarnings = Array.isArray(outcome.summary.warnings)
        ? outcome.summary.warnings
        : [];
      clearSelection();
      renderRepository();
      renderStats();
      await refreshSemanticStatus();
      if (state.repo.indexed) {
        await loadBrowse({ resetInput: true });
      } else {
        renderSearch();
      }
      showToast(
        t("indexDone", {
          scanned: formatNumber(outcome.summary.files_scanned),
          indexed: formatNumber(outcome.summary.indexed),
          symbols: formatNumber(outcome.summary.symbols),
          ms: formatNumber(outcome.summary.elapsed_time_ms),
        }),
        "success",
        7000,
      );
      if (state.lastWarnings.length) {
        showToast(
          t("indexWarnings", {
            count: formatNumber(state.lastWarnings.length),
          }),
          "warning",
          7000,
        );
      }
    } catch (error) {
      handleError("indexError", error);
    } finally {
      state.indexing = false;
      setBusy(false);
      updateControls();
    }
  }

  async function closeRepository() {
    if (!invoke || state.busy) return;
    try {
      await invoke("close_repository");
    } catch (error) {
      showToast(normalizeError(error), "error");
      return;
    }
    state.repo = null;
    state.stats = null;
    state.lastWarnings = [];
    state.results = [];
    state.resultsMode = "browse";
    state.resultsQuery = "";
    state.searchMessage = null;
    state.searching = false;
    state.semantic = null;
    state.semanticProgress = null;
    state.semanticSeq += 1;
    clearSelection();
    renderRepository();
    renderStats();
    renderSearchMode();
    renderSearch();
    setStatus("statusReady");
    updateControls();
    showToast(t("repoClosed"), "info");
  }

  function showGraphLoading(show) {
    dom.graphLoading.classList.toggle("hidden", !show);
    if (show) {
      dom.graphEmpty.classList.add("hidden");
      dom.graphTruncated.classList.add("hidden");
    } else if (!state.graph) {
      dom.graphEmpty.classList.remove("hidden");
    }
  }

  function showDetailLoading() {
    dom.detailEmpty.classList.add("hidden");
    dom.detail.classList.remove("hidden");
    dom.detailKind.textContent = "";
    dom.detailKind.className = "detail-kind";
    dom.detailName.textContent = "…";
    dom.detailQName.textContent = t("statusLoading");
    dom.detailLocation.textContent = "";
    dom.detailSignatureBox.classList.add("hidden");
    dom.detailDocBox.classList.add("hidden");
    dom.impactControls.classList.add("hidden");
    clearNode(dom.tabContent);
    dom.tabCountCallers.textContent = "0";
    dom.tabCountCallees.textContent = "0";
    dom.tabCountImpact.textContent = "0";
  }

  function clearSelection() {
    state.selectionSeq += 1;
    state.selectedId = null;
    state.details = null;
    state.graph = null;
    state.graphById = new Map();
    state.nodePositions = new Map();
    state.callers = [];
    state.callees = [];
    state.impact = [];
    state.graphHistory = [];
    state.drag = null;
    clearNode(dom.graphNodes);
    clearNode(dom.graphEdges);
    dom.graphMeta.textContent = "";
    dom.graphTruncated.classList.add("hidden");
    dom.graphLoading.classList.add("hidden");
    dom.graphEmpty.classList.remove("hidden");
    applyGraphTransform({ x: 0, y: 0, k: 1 });
    dom.detail.classList.add("hidden");
    dom.detailEmpty.classList.remove("hidden");
    renderTabs();
    renderSearch();
    updateControls();
  }

  async function selectSymbol(id, options = {}) {
    if (!invoke || !state.repo || !state.repo.indexed || id === null) return;
    const history = options.history || "reset";
    if (
      history === "push" &&
      state.selectedId !== null &&
      state.selectedId !== id
    ) {
      state.graphHistory.push(state.selectedId);
    } else if (history === "reset") {
      state.graphHistory = [];
    }

    state.selectedId = id;
    state.details = null;
    state.graph = null;
    state.graphById = new Map();
    state.nodePositions = new Map();
    state.callers = [];
    state.callees = [];
    state.impact = [];
    state.drag = null;
    clearNode(dom.graphNodes);
    clearNode(dom.graphEdges);
    dom.graphMeta.textContent = "";
    dom.graphTruncated.classList.add("hidden");
    dom.graphEmpty.classList.add("hidden");
    showDetailLoading();
    showGraphLoading(true);
    renderSearch();
    updateControls();

    const seq = ++state.selectionSeq;
    try {
      const [details, graph, callers, callees, impact] = await Promise.all([
        invoke("symbol_details", { id }),
        invoke("symbol_graph", {
          id,
          depth: state.graphDepth,
          limit: state.graphLimit,
        }),
        invoke("callers", { id }),
        invoke("callees", { id }),
        invoke("impact", { id, depth: state.impactDepth }),
      ]);
      if (seq !== state.selectionSeq) return;
      state.details = details;
      state.graph = graph;
      state.callers = Array.isArray(callers) ? callers : [];
      state.callees = Array.isArray(callees) ? callees : [];
      state.impact = Array.isArray(impact) ? impact : [];
      renderDetails();
      renderTabs();
      renderGraph();
      renderSearch();
    } catch (error) {
      if (seq !== state.selectionSeq) return;
      handleError("detailError", error);
      clearSelection();
    } finally {
      if (seq === state.selectionSeq) showGraphLoading(false);
    }
  }

  function renderDetails() {
    const details = state.details;
    if (!details) {
      dom.detail.classList.add("hidden");
      dom.detailEmpty.classList.remove("hidden");
      return;
    }

    dom.detailEmpty.classList.add("hidden");
    dom.detail.classList.remove("hidden");
    dom.detailKind.className = `detail-kind ${kindClass(details.kind)}`;
    dom.detailKind.textContent = kindLabel(details.kind);
    dom.detailName.textContent = details.name;
    dom.detailName.title = details.name;
    dom.detailQName.textContent = details.qualified_name;
    dom.detailLocation.textContent = `${details.file}:${details.line}`;
    dom.detailLocation.title = `${details.file}:${details.line}`;

    if (details.signature) {
      dom.detailSignatureBox.classList.remove("hidden");
      dom.detailSignature.textContent = details.signature;
    } else {
      dom.detailSignatureBox.classList.add("hidden");
      dom.detailSignature.textContent = "";
    }

    if (details.doc) {
      dom.detailDocBox.classList.remove("hidden");
      dom.detailDoc.textContent = details.doc;
      dom.detailDoc.setAttribute("dir", "auto");
    } else {
      dom.detailDocBox.classList.add("hidden");
      dom.detailDoc.textContent = "";
    }
  }

  function emptyTab(titleKey, hintKey) {
    const container = el("div", "empty-tab");
    container.appendChild(el("strong", null, t(titleKey)));
    if (hintKey) container.appendChild(el("span", null, t(hintKey)));
    return container;
  }

  function relatedList(items, role) {
    const list = el("ul", "related-list");
    for (const item of items) {
      const entry = el("li");
      const clickable = item.symbol_id !== null && item.symbol_id !== undefined;
      const node = document.createElement(clickable ? "button" : "div");
      node.className = "related-item";
      if (clickable) {
        node.type = "button";
        node.addEventListener("click", () => {
          selectSymbol(item.symbol_id, { history: "push" });
        });
      }

      const name = el(
        "span",
        "related-name",
        item.qualified_name || item.name || t("unknown"),
      );
      name.setAttribute("dir", "auto");

      const meta = el("div", "related-meta");
      meta.setAttribute("dir", "ltr");
      const line = role === "caller" ? item.call_line : item.line;
      meta.appendChild(el("span", null, `${item.file}:${line}`));
      meta.appendChild(
        el(
          "span",
          `resolution ${resolutionClass(item.resolution)}`,
          resolutionLabel(item.resolution),
        ),
      );

      node.appendChild(name);
      node.appendChild(meta);
      entry.appendChild(node);
      list.appendChild(entry);
    }
    return list;
  }

  function impactList(entries) {
    const groups = new Map();
    for (const entry of entries) {
      const depth = Number(entry.depth) || 0;
      if (!groups.has(depth)) groups.set(depth, []);
      groups.get(depth).push(entry);
    }

    const fragment = document.createDocumentFragment();
    for (const depth of Array.from(groups.keys()).sort((a, b) => a - b)) {
      const group = el("div", "impact-group");
      group.appendChild(
        el(
          "div",
          "impact-depth",
          t("depthValue", { depth: formatNumber(depth) }),
        ),
      );
      const list = el("ul", "related-list");
      for (const entry of groups.get(depth)) {
        const item = el("li");
        const button = el("button", "related-item");
        button.type = "button";
        const name = el(
          "span",
          "related-name",
          entry.symbol_qualified_name,
        );
        name.setAttribute("dir", "auto");
        const meta = el("div", "related-meta");
        meta.setAttribute("dir", "ltr");
        meta.appendChild(el("span", null, `${entry.file}:${entry.line}`));
        button.appendChild(name);
        button.appendChild(meta);
        if (entry.symbol_id !== null && entry.symbol_id !== undefined) {
          button.addEventListener("click", () => {
            selectSymbol(entry.symbol_id, { history: "push" });
          });
        }
        item.appendChild(button);
        list.appendChild(item);
      }
      group.appendChild(list);
      fragment.appendChild(group);
    }
    return fragment;
  }

  function renderTabs() {
    const hasDetails = Boolean(state.details);
    dom.tabCountCallers.textContent = formatNumber(state.callers.length);
    dom.tabCountCallees.textContent = formatNumber(state.callees.length);
    dom.tabCountImpact.textContent = formatNumber(state.impact.length);

    for (const tab of dom.tabs) {
      const active = tab.dataset.tab === state.tab;
      tab.classList.toggle("active", active);
      tab.setAttribute("aria-selected", active ? "true" : "false");
      tab.disabled = !hasDetails;
    }

    dom.impactControls.classList.toggle(
      "hidden",
      !hasDetails || state.tab !== "impact",
    );
    clearNode(dom.tabContent);
    if (!hasDetails) return;

    if (state.tab === "callers") {
      dom.tabContent.appendChild(
        state.callers.length
          ? relatedList(state.callers, "caller")
          : emptyTab("noCallers", "noCallersHint"),
      );
    } else if (state.tab === "callees") {
      dom.tabContent.appendChild(
        state.callees.length
          ? relatedList(state.callees, "callee")
          : emptyTab("noCallees", null),
      );
    } else {
      dom.tabContent.appendChild(
        state.impact.length
          ? impactList(state.impact)
          : emptyTab("noImpact", "impactNote"),
      );
    }
  }

  async function refreshGraph() {
    if (!invoke || state.selectedId === null) return;
    const id = state.selectedId;
    showGraphLoading(true);
    try {
      const graph = await invoke("symbol_graph", {
        id,
        depth: state.graphDepth,
        limit: state.graphLimit,
      });
      if (state.selectedId !== id) return;
      state.graph = graph;
      renderGraph();
    } catch (error) {
      if (state.selectedId !== id) return;
      handleError("graphError", error);
      showGraphLoading(false);
    }
  }

  async function refreshImpact() {
    if (!invoke || state.selectedId === null) return;
    const id = state.selectedId;
    dom.tabContent.textContent = t("statusLoading");
    try {
      const impact = await invoke("impact", {
        id,
        depth: state.impactDepth,
      });
      if (state.selectedId !== id) return;
      state.impact = Array.isArray(impact) ? impact : [];
      renderTabs();
    } catch (error) {
      if (state.selectedId !== id) return;
      handleError("detailError", error);
      renderTabs();
    }
  }

  function nodeWidth(node) {
    const label = truncateText(node.name || node.qualified_name || "", 30);
    let textWidth = label.length * 7;
    if (!nodeWidth.context) {
      const canvas = document.createElement("canvas");
      nodeWidth.context = canvas.getContext("2d");
      if (nodeWidth.context) {
        nodeWidth.context.font =
          '600 11.5px "SFMono-Regular", Menlo, Consolas, monospace';
      }
    }
    if (nodeWidth.context) {
      textWidth = nodeWidth.context.measureText(label).width;
    }
    return clamp(Math.ceil(textWidth) + 46, 124, 246);
  }

  function layoutGraph(data) {
    const positions = new Map();
    const root = data.nodes.find((node) => node.side === "root") || data.nodes[0];
    if (!root) {
      state.nodePositions = positions;
      return;
    }

    const rtl = document.documentElement.dir === "rtl";
    const callerSign = rtl ? 1 : -1;
    const calleeSign = rtl ? -1 : 1;
    const columnGap = 245;
    const rowGap = 66;

    positions.set(root.id, {
      x: 0,
      y: 0,
      w: nodeWidth(root),
      h: NODE_HEIGHT,
    });

    const columns = new Map();
    for (const node of data.nodes) {
      if (node.id === root.id) continue;
      const side =
        node.side === "both"
          ? "both"
          : node.side === "caller"
            ? "caller"
            : "callee";
      const key = side === "both" ? "both" : `${side}:${node.depth}`;
      if (!columns.has(key)) columns.set(key, []);
      columns.get(key).push(node);
    }

    for (const [key, nodes] of columns) {
      nodes.sort(
        (a, b) =>
          String(a.qualified_name).localeCompare(String(b.qualified_name)) ||
          a.id - b.id,
      );
      let x = 0;
      let startY = 0;
      if (key === "both") {
        startY = 115;
      } else {
        const [side, depthText] = key.split(":");
        const depth = Number(depthText) || 1;
        x = (side === "caller" ? callerSign : calleeSign) * depth * columnGap;
        startY = -((nodes.length - 1) * rowGap) / 2;
      }
      nodes.forEach((node, index) => {
        positions.set(node.id, {
          x,
          y: startY + index * rowGap,
          w: nodeWidth(node),
          h: NODE_HEIGHT,
        });
      });
    }

    state.nodePositions = positions;
  }

  function edgePath(from, to) {
    if (from === to) {
      const x = from.x + from.w / 2;
      return `M ${x} ${from.y - 10} C ${x + 48} ${from.y - 52}, ${
        x + 48
      } ${from.y + 52}, ${x} ${from.y + 10}`;
    }

    if (Math.abs(to.x - from.x) <= 4) {
      const direction = to.y >= from.y ? 1 : -1;
      const sx = from.x;
      const sy = from.y + direction * (from.h / 2);
      const tx = to.x;
      const ty = to.y - direction * (to.h / 2);
      const middleY = (sy + ty) / 2;
      return `M ${sx} ${sy} C ${sx} ${middleY}, ${tx} ${middleY}, ${tx} ${ty}`;
    }

    const direction = to.x >= from.x ? 1 : -1;
    const sx = from.x + direction * (from.w / 2);
    const tx = to.x - direction * (to.w / 2);
    const sy = from.y;
    const ty = to.y;
    const curve = Math.max(35, Math.min(170, Math.abs(tx - sx) * 0.5));
    return `M ${sx} ${sy} C ${sx + direction * curve} ${sy}, ${
      tx - direction * curve
    } ${ty}, ${tx} ${ty}`;
  }

  function renderGraphEdges(data) {
    clearNode(dom.graphEdges);
    for (const edge of data.edges) {
      const from = state.nodePositions.get(edge.from);
      const to = state.nodePositions.get(edge.to);
      if (!from || !to) continue;
      const path = svgEl("path", {
        class: `graph-edge res-${edge.resolution || "unresolved"}`,
        d: edgePath(from, to),
        "marker-end": "url(#arrow)",
      });
      const fromNode = state.graphById.get(edge.from);
      const toNode = state.graphById.get(edge.to);
      const title = svgEl("title");
      title.textContent = `${fromNode ? fromNode.qualified_name : edge.from} → ${
        toNode ? toNode.qualified_name : edge.to
      }\n${resolutionLabel(edge.resolution)} · ${edge.file}:${edge.line}`;
      path.appendChild(title);
      dom.graphEdges.appendChild(path);
    }
  }

  function renderGraphNodes(data) {
    clearNode(dom.graphNodes);
    for (const node of data.nodes) {
      const position = state.nodePositions.get(node.id);
      if (!position) continue;
      const group = svgEl("g", {
        class: `graph-node ${kindClass(node.kind)}${
          node.side === "root" ? " root" : ""
        }`,
        transform: `translate(${position.x},${position.y})`,
        "data-id": String(node.id),
        tabindex: "0",
        role: "button",
      });
      group.setAttribute(
        "aria-label",
        `${node.qualified_name}, ${kindLabel(node.kind)}, ${node.file}:${
          node.line
        }`,
      );

      const rect = svgEl("rect", {
        x: -position.w / 2,
        y: -position.h / 2,
        width: position.w,
        height: position.h,
        rx: 10,
      });
      const dot = svgEl("circle", {
        cx: -position.w / 2 + 13,
        cy: -4,
        r: 3.5,
        class: "node-dot",
      });
      const label = svgEl("text", { x: 0, y: -3, class: "node-label" });
      label.textContent = truncateText(node.name, 30);
      const kind = svgEl("text", { x: 0, y: 12, class: "node-kind" });
      kind.textContent = `${kindLabel(node.kind)} · ${sideLabel(node.side)}`;
      const title = svgEl("title");
      title.textContent = `${node.qualified_name}\n${kindLabel(node.kind)} · ${
        node.file
      }:${node.line}`;

      group.appendChild(rect);
      group.appendChild(dot);
      group.appendChild(label);
      group.appendChild(kind);
      group.appendChild(title);
      dom.graphNodes.appendChild(group);
    }
  }

  function renderGraph(options = {}) {
    const { preserveZoom = false } = options;
    const previousZoom = state.graphTransform.k;
    const data = state.graph;
    clearNode(dom.graphNodes);
    clearNode(dom.graphEdges);
    dom.graphTruncated.classList.toggle(
      "hidden",
      !(data && data.truncated),
    );

    if (!data || !Array.isArray(data.nodes) || data.nodes.length === 0) {
      dom.graphEmpty.classList.remove("hidden");
      dom.graphLoading.classList.add("hidden");
      dom.graphMeta.textContent = "";
      state.graphById = new Map();
      state.nodePositions = new Map();
      updateControls();
      return;
    }

    dom.graphEmpty.classList.add("hidden");
    dom.graphLoading.classList.add("hidden");
    state.graphById = new Map(data.nodes.map((node) => [node.id, node]));
    layoutGraph(data);
    renderGraphEdges(data);
    renderGraphNodes(data);
    dom.graphMeta.textContent = t("graphMeta", {
      nodes: formatNumber(data.nodes.length),
      edges: formatNumber(data.edges.length),
    });
    fitGraph();
    if (
      preserveZoom &&
      Number.isFinite(previousZoom) &&
      previousZoom > 0 &&
      Math.abs(previousZoom / state.graphTransform.k - 1) > 0.001
    ) {
      const rect = dom.graph.getBoundingClientRect();
      zoomAt(
        rect.left + rect.width / 2,
        rect.top + rect.height / 2,
        previousZoom / state.graphTransform.k,
      );
    }
    updateControls();
  }

  function applyGraphTransform(transform) {
    state.graphTransform = {
      x: transform.x,
      y: transform.y,
      k: transform.k,
    };
    dom.graphViewport.setAttribute(
      "transform",
      `translate(${transform.x} ${transform.y}) scale(${transform.k})`,
    );
  }

  function fitGraph() {
    const positions = state.nodePositions;
    if (!positions || positions.size === 0) return;
    const rect = dom.graph.getBoundingClientRect();
    const width = rect.width || 800;
    const height = rect.height || 600;

    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    for (const position of positions.values()) {
      minX = Math.min(minX, position.x - position.w / 2);
      maxX = Math.max(maxX, position.x + position.w / 2);
      minY = Math.min(minY, position.y - position.h / 2);
      maxY = Math.max(maxY, position.y + position.h / 2);
    }

    const padding = 70;
    const graphWidth = Math.max(1, maxX - minX);
    const graphHeight = Math.max(1, maxY - minY);
    const scale = clamp(
      Math.min(
        (width - padding * 2) / graphWidth,
        (height - padding * 2) / graphHeight,
      ),
      0.15,
      1.25,
    );
    applyGraphTransform({
      x: width / 2 - ((minX + maxX) / 2) * scale,
      y: height / 2 - ((minY + maxY) / 2) * scale,
      k: scale,
    });
  }

  function zoomAt(clientX, clientY, factor) {
    const rect = dom.graph.getBoundingClientRect();
    const px = clientX - rect.left;
    const py = clientY - rect.top;
    const { x, y, k } = state.graphTransform;
    const nextK = clamp(k * factor, 0.15, 4);
    const worldX = (px - x) / k;
    const worldY = (py - y) / k;
    applyGraphTransform({
      x: px - worldX * nextK,
      y: py - worldY * nextK,
      k: nextK,
    });
  }

  function zoomBy(factor) {
    const rect = dom.graph.getBoundingClientRect();
    zoomAt(
      rect.left + rect.width / 2,
      rect.top + rect.height / 2,
      factor,
    );
  }

  function bindGraphEvents() {
    dom.graph.addEventListener(
      "wheel",
      (event) => {
        if (!state.graph) return;
        event.preventDefault();
        zoomAt(
          event.clientX,
          event.clientY,
          event.deltaY < 0 ? 1.12 : 0.89,
        );
      },
      { passive: false },
    );

    dom.graph.addEventListener("pointerdown", (event) => {
      if (event.button !== 0 || !state.graph) return;
      const nodeElement = event.target.closest(".graph-node");
      if (nodeElement) {
        const id = Number(nodeElement.dataset.id);
        const position = state.nodePositions.get(id);
        if (!position) return;
        state.drag = {
          type: "node",
          id,
          startX: event.clientX,
          startY: event.clientY,
          original: { ...position },
          moved: false,
        };
      } else {
        state.drag = {
          type: "pan",
          startX: event.clientX,
          startY: event.clientY,
          origin: { ...state.graphTransform },
          moved: false,
        };
        dom.graph.classList.add("panning");
      }
      dom.graph.setPointerCapture(event.pointerId);
      event.preventDefault();
    });

    dom.graph.addEventListener("pointermove", (event) => {
      const drag = state.drag;
      if (!drag) return;
      const dx = event.clientX - drag.startX;
      const dy = event.clientY - drag.startY;
      if (Math.abs(dx) > 3 || Math.abs(dy) > 3) drag.moved = true;

      if (drag.type === "pan") {
        applyGraphTransform({
          x: drag.origin.x + dx,
          y: drag.origin.y + dy,
          k: drag.origin.k,
        });
        return;
      }

      const position = state.nodePositions.get(drag.id);
      if (!position) return;
      position.x = drag.original.x + dx / state.graphTransform.k;
      position.y = drag.original.y + dy / state.graphTransform.k;
      const nodeElement = dom.graphNodes.querySelector(
        `[data-id="${drag.id}"]`,
      );
      if (nodeElement) {
        nodeElement.setAttribute(
          "transform",
          `translate(${position.x},${position.y})`,
        );
      }
      renderGraphEdges(state.graph);
    });

    const finishPointer = (event) => {
      const drag = state.drag;
      if (!drag) return;
      state.drag = null;
      dom.graph.classList.remove("panning");
      try {
        dom.graph.releasePointerCapture(event.pointerId);
      } catch {
        // The pointer may already have been released.
      }
      if (drag.type === "node" && !drag.moved) {
        selectSymbol(drag.id, { history: "push" });
      }
    };

    dom.graph.addEventListener("pointerup", finishPointer);
    dom.graph.addEventListener("pointercancel", (event) => {
      state.drag = null;
      dom.graph.classList.remove("panning");
      try {
        dom.graph.releasePointerCapture(event.pointerId);
      } catch {
        // Ignore.
      }
    });

    dom.graph.addEventListener("keydown", (event) => {
      if (event.key !== "Enter" && event.key !== " ") return;
      const nodeElement = event.target.closest(".graph-node");
      if (!nodeElement) return;
      event.preventDefault();
      selectSymbol(Number(nodeElement.dataset.id), { history: "push" });
    });
  }

  function bindEvents() {
    dom.openForm.addEventListener("submit", (event) => {
      event.preventDefault();
      openRepository();
    });
    dom.browseBtn.addEventListener("click", browseRepository);
    dom.indexBtn.addEventListener("click", indexRepository);
    dom.closeBtn.addEventListener("click", closeRepository);
    dom.langBtn.addEventListener("click", () => {
      applyLanguage(state.lang === "en" ? "ar" : "en");
    });
    dom.modeLexical.addEventListener("click", () => setSearchMode("lexical"));
    dom.modeSemantic.addEventListener("click", () => setSearchMode("semantic"));
    dom.downloadModelBtn.addEventListener("click", downloadModel);
    dom.embedBtn.addEventListener("click", embedRepository);
    dom.confirmOk.addEventListener("click", () => closeConfirm(true));
    dom.confirmCancel.addEventListener("click", () => closeConfirm(false));
    dom.confirmModal.addEventListener("click", (event) => {
      if (event.target === dom.confirmModal) closeConfirm(false);
    });
    document.addEventListener("keydown", (event) => {
      if (event.key === "Escape" && confirmResolve) {
        event.preventDefault();
        closeConfirm(false);
      }
    });

    if (tauri && tauri.event && typeof tauri.event.listen === "function") {
      tauri.event.listen("semantic-download-progress", (event) => {
        const payload = event && event.payload ? event.payload : {};
        const total = Number(payload.total) || 0;
        const downloaded = Number(payload.downloaded) || 0;
        const percent = total
          ? ((downloaded / total) * 100).toFixed(0)
          : "0";
        state.semanticProgress = {
          done: downloaded,
          total,
          label: t("semanticDownloadProgress", {
            index: formatNumber(payload.file_index || 1),
            count: formatNumber(payload.file_count || 1),
            file: payload.file || "",
            downloaded: formatBytes(downloaded),
            total: formatBytes(total),
            percent,
          }),
        };
        renderSemanticProgress();
      });
      tauri.event.listen("semantic-embed-progress", (event) => {
        const payload = event && event.payload ? event.payload : {};
        const total = Number(payload.total) || 0;
        const done = Number(payload.done) || 0;
        const percent = total ? ((done / total) * 100).toFixed(0) : "0";
        state.semanticProgress = {
          done,
          total,
          label: t("semanticEmbedProgress", {
            done: formatNumber(done),
            total: formatNumber(total),
            percent,
          }),
        };
        renderSemanticProgress();
      });
    }

    let searchTimer = null;
    dom.searchForm.addEventListener("submit", (event) => {
      event.preventDefault();
      window.clearTimeout(searchTimer);
      performSearch(dom.searchInput.value);
    });

    dom.searchInput.addEventListener("input", () => {
      window.clearTimeout(searchTimer);
      const query = dom.searchInput.value.trim();
      if (!query) {
        searchTimer = window.setTimeout(() => {
          if (state.repo && state.repo.indexed) {
            loadBrowse({ resetInput: false });
          }
        }, 250);
        return;
      }
      if (query.length < 2) return;
      searchTimer = window.setTimeout(() => performSearch(query), 350);
    });

    dom.searchInput.addEventListener("keydown", (event) => {
      if (event.key === "Escape") {
        dom.searchInput.value = "";
        if (state.repo && state.repo.indexed) {
          loadBrowse({ resetInput: false });
        }
      }
    });

    for (const tab of dom.tabs) {
      tab.addEventListener("click", () => {
        if (!state.details) return;
        state.tab = tab.dataset.tab;
        renderTabs();
      });
    }

    dom.graphDepth.addEventListener("change", () => {
      state.graphDepth = Number(dom.graphDepth.value) || 2;
      refreshGraph();
    });
    dom.graphLimit.addEventListener("change", () => {
      state.graphLimit = Number(dom.graphLimit.value) || 80;
      refreshGraph();
    });
    dom.impactDepth.addEventListener("change", () => {
      state.impactDepth = Number(dom.impactDepth.value) || 5;
      refreshImpact();
    });
    dom.graphBack.addEventListener("click", () => {
      if (!state.graphHistory.length) return;
      const previous = state.graphHistory.pop();
      selectSymbol(previous, { history: "keep" });
    });
    dom.graphFit.addEventListener("click", fitGraph);
    dom.zoomIn.addEventListener("click", () => zoomBy(1.18));
    dom.zoomOut.addEventListener("click", () => zoomBy(0.85));

    bindGraphEvents();

    window.addEventListener("keydown", (event) => {
      if (
        event.key === "/" &&
        !event.metaKey &&
        !event.ctrlKey &&
        !event.altKey
      ) {
        const target = event.target;
        if (
          target &&
          (target.tagName === "INPUT" ||
            target.tagName === "TEXTAREA" ||
            target.tagName === "SELECT")
        ) {
          return;
        }
        event.preventDefault();
        dom.searchInput.focus();
      }
    });

    let resizeTimer = null;
    window.addEventListener("resize", () => {
      window.clearTimeout(resizeTimer);
      resizeTimer = window.setTimeout(() => {
        if (state.graph) fitGraph();
      }, 150);
    });

    window.addEventListener("error", (event) => {
      if (event.message) showToast(event.message, "error", 8000);
    });
    window.addEventListener("unhandledrejection", (event) => {
      showToast(normalizeError(event.reason), "error", 8000);
    });
  }

  function bootstrap() {
    applyLanguage(state.lang, { persist: false, render: false });
    dom.graphDepth.value = String(state.graphDepth);
    dom.graphLimit.value = String(state.graphLimit);
    dom.impactDepth.value = String(state.impactDepth);
    bindEvents();

    const lastPath = readStorage("atlas.repoPath");
    if (lastPath) dom.repoPath.value = lastPath;

    renderRepository();
    renderStats();
    renderSearch();
    renderSearchMode();
    renderDetails();
    renderTabs();
    renderStatus();
    updateControls();

    if (!invoke) {
      showToast(t("tauriMissing"), "error", 12000);
      return;
    }

    invoke("repository_info")
      .then((info) => {
        if (!info) return;
        setRepository(info);
        dom.repoPath.value = info.root;
        if (info.indexed) {
          loadBrowse({ resetInput: true });
        } else {
          renderSearch();
        }
      })
      .catch((error) => {
        showToast(normalizeError(error), "error", 8000);
      });
  }

  bootstrap();
})();
