const navButtons = [...document.querySelectorAll("[data-view]")];
const panels = [...document.querySelectorAll("[data-view-panel]")];
const viewTitle = document.getElementById("viewTitle");
const quicklook = document.getElementById("quicklook");
const palette = document.getElementById("palette");
const onboarding = document.getElementById("onboarding");
const rows = [...document.querySelectorAll(".component-row")];
const search = document.getElementById("inventorySearch");
const resultCount = document.getElementById("resultCount");

const titles = {
  inventory: "Inventory",
  map: "Map",
  editor: "Editor",
  health: "Health",
};

function setView(view) {
  navButtons.forEach((button) => {
    button.classList.toggle("active", button.dataset.view === view);
  });

  panels.forEach((panel) => {
    panel.classList.toggle("active", panel.dataset.viewPanel === view);
  });

  viewTitle.textContent = titles[view] || "Inventory";
}

function openQuicklook(row) {
  rows.forEach((item) => item.classList.toggle("selected", item === row));
  quicklook.classList.add("open");

  document.getElementById("quickKind").textContent = `${row.dataset.kind}: ${row.dataset.name}`;
  document.getElementById("quickTitle").textContent = row.dataset.name;
  document.getElementById("quickDesc").textContent = row.dataset.desc;
  document.getElementById("quickTool").textContent = row.dataset.tool;
  document.getElementById("quickScope").textContent = row.dataset.scope;
  document.getElementById("quickPath").textContent = row.dataset.path;
  document.getElementById("quickBody").textContent = row.dataset.body;
  document.getElementById("quickRelations").textContent = row.dataset.relations;
}

function togglePalette(force) {
  const shouldOpen = typeof force === "boolean" ? force : !palette.classList.contains("open");
  palette.classList.toggle("open", shouldOpen);
  palette.setAttribute("aria-hidden", String(!shouldOpen));
  if (shouldOpen) {
    document.getElementById("paletteInput").focus();
  }
}

function toggleOnboarding(force) {
  const shouldOpen = typeof force === "boolean" ? force : !onboarding.classList.contains("open");
  onboarding.classList.toggle("open", shouldOpen);
  onboarding.setAttribute("aria-hidden", String(!shouldOpen));
}

function filterRows() {
  const normalize = (value) => value.toLowerCase().replace(/-/g, " ").trim();
  const query = normalize(search.value);
  let visible = 0;

  rows.forEach((row) => {
    const haystack = [
      row.dataset.name,
      row.dataset.kind,
      row.dataset.tool,
      row.dataset.scope,
      row.dataset.desc,
      row.dataset.path,
    ]
      .join(" ")
      .toLowerCase()
      .replace(/-/g, " ");

    const terms = query
      .split(/\s+/)
      .map((term) => term.replace(/^(type|tool|scope):/, ""))
      .filter(Boolean);

    const match = terms.every((term) => haystack.includes(term));
    row.hidden = !match;
    if (match) visible += 1;
  });

  resultCount.textContent = `${visible} components visible`;
}

navButtons.forEach((button) => {
  button.addEventListener("click", () => setView(button.dataset.view));
});

document.querySelectorAll("[data-view-jump]").forEach((button) => {
  button.addEventListener("click", () => setView(button.dataset.viewJump));
});

rows.forEach((row) => {
  row.addEventListener("click", () => openQuicklook(row));
  row.addEventListener("dblclick", () => setView("editor"));
});

document.querySelectorAll(".chip").forEach((chip) => {
  chip.addEventListener("click", () => chip.classList.toggle("selected"));
});

document.querySelectorAll("[data-chip-target]").forEach((button) => {
  button.addEventListener("click", () => {
    const target = button.dataset.chipTarget;
    if (!target) return;
    search.value = target;
    setView("inventory");
    filterRows();
  });
});

search.addEventListener("input", filterRows);

document.getElementById("paletteTrigger").addEventListener("click", () => togglePalette());
document.getElementById("refreshButton").addEventListener("click", () => {
  resultCount.textContent = "scan completed just now";
});
document.getElementById("closeQuicklook").addEventListener("click", () => quicklook.classList.remove("open"));
document.getElementById("openEditorFromQuick").addEventListener("click", () => setView("editor"));
document.getElementById("themeToggle").addEventListener("click", () => document.body.classList.toggle("light"));
document.getElementById("densityToggle").addEventListener("click", () => document.body.classList.toggle("compact"));
document.getElementById("showOnboarding").addEventListener("click", () => toggleOnboarding(true));
document.getElementById("finishOnboarding").addEventListener("click", () => toggleOnboarding(false));
document.getElementById("cancelOnboarding").addEventListener("click", () => toggleOnboarding(false));

document.querySelectorAll("[data-palette-open]").forEach((row) => {
  row.addEventListener("click", () => {
    setView(row.dataset.paletteOpen);
    togglePalette(false);
  });
});

palette.addEventListener("click", (event) => {
  if (event.target === palette) togglePalette(false);
});

onboarding.addEventListener("click", (event) => {
  if (event.target === onboarding) toggleOnboarding(false);
});

document.addEventListener("keydown", (event) => {
  const modifier = event.metaKey || event.ctrlKey;

  if (modifier && event.key.toLowerCase() === "k") {
    event.preventDefault();
    togglePalette();
  }

  if (modifier && ["1", "2", "3"].includes(event.key)) {
    event.preventDefault();
    setView({ 1: "inventory", 2: "map", 3: "editor" }[event.key]);
  }

  if (event.key === "Escape") {
    togglePalette(false);
    toggleOnboarding(false);
    quicklook.classList.remove("open");
  }

  if (event.key === " " && document.activeElement?.classList.contains("component-row")) {
    event.preventDefault();
    openQuicklook(document.activeElement);
  }
});

filterRows();
