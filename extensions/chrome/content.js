// Store active overlays keyed by a unique ID assigned to original images
const activeOverlays = new Map();

// CSS insertion
const style = document.createElement("style");
style.textContent = `
  .koharu-translate-btn {
    position: absolute;
    z-index: 10000;
    background: #6366f1;
    color: white;
    border: none;
    padding: 6px 12px;
    border-radius: 4px;
    cursor: pointer;
    font-weight: bold;
    font-size: 12px;
    box-shadow: 0 4px 6px -1px rgba(0,0,0,0.1);
    transition: background 0.2s;
    font-family: system-ui, sans-serif;
  }
  .koharu-translate-btn:hover {
    background: #4f46e5;
  }
  .koharu-spinner {
    position: absolute;
    background: rgba(0, 0, 0, 0.4);
    display: flex;
    align-items: center;
    justify-content: center;
    color: white;
    font-weight: bold;
    font-size: 14px;
    z-index: 10001;
    pointer-events: none;
  }
`;
document.head.appendChild(style);

// Hover handler to spawn the Translate button
document.addEventListener("mouseover", (e) => {
  const target = e.target;
  if (target.tagName === "IMG" && !target.dataset.koharuId) {
    setupImage(target);
  }
});

function setupImage(img) {
  const id = "img_" + Math.random().toString(36).substring(2, 9);
  img.dataset.koharuId = id;

  const btn = document.createElement("button");
  btn.className = "koharu-translate-btn";
  btn.textContent = "Translate";
  document.body.appendChild(btn);

  const updatePosition = () => {
    const rect = img.getBoundingClientRect();
    btn.style.top = `${rect.top + window.scrollY + 10}px`;
    btn.style.left = `${rect.left + window.scrollX + 10}px`;
  };

  updatePosition();
  
  // Reposition on scroll/resize
  window.addEventListener("scroll", updatePosition);
  window.addEventListener("resize", updatePosition);

  // Hide button if not hovering
  let hoverTimer;
  const onLeave = () => {
    hoverTimer = setTimeout(() => {
      btn.remove();
      window.removeEventListener("scroll", updatePosition);
      window.removeEventListener("resize", updatePosition);
    }, 1500);
  };
  const onEnter = () => {
    clearTimeout(hoverTimer);
  };

  img.addEventListener("mouseleave", onLeave);
  btn.addEventListener("mouseenter", onEnter);
  btn.addEventListener("mouseleave", onLeave);

  btn.addEventListener("click", () => {
    btn.remove();
    startTranslation(img, id);
  });
}

async function startTranslation(img, id) {
  // Show spinner
  const rect = img.getBoundingClientRect();
  const spinner = document.createElement("div");
  spinner.className = "koharu-spinner";
  spinner.textContent = "Processing...";
  spinner.style.top = `${rect.top + window.scrollY}px`;
  spinner.style.left = `${rect.left + window.scrollX}px`;
  spinner.style.width = `${rect.width}px`;
  spinner.style.height = `${rect.height}px`;
  document.body.appendChild(spinner);

  activeOverlays.set(id, { spinner, img });

  chrome.runtime.sendMessage({
    action: "TranslateImage",
    payload: { url: img.src }
  }, (resp) => {
    if (resp && resp.transferId) {
      console.log("Translation started, transferId:", resp.transferId);
      const state = activeOverlays.get(id);
      state.transferId = resp.transferId;
      activeOverlays.set(resp.transferId, { id, ...state });
    }
  });
}

chrome.runtime.onMessage.addListener((message) => {
  if (message.action === "TranslationResult") {
    const { transferId, inpaintedImage, texts } = message.payload;
    const state = activeOverlays.get(transferId);
    if (!state) return;

    // Remove spinner
    state.spinner.remove();

    renderOverlay(state.img, inpaintedImage, texts);
  } else if (message.action === "Error") {
    const { transferId, message: errMsg } = message.payload;
    const state = activeOverlays.get(transferId);
    if (!state) return;

    state.spinner.remove();
    alert("Translation failed: " + errMsg);
  }
});

function renderOverlay(img, inpaintedBase64, texts) {
  if (inpaintedBase64) {
    img.src = "data:image/png;base64," + inpaintedBase64;
    img.removeAttribute("srcset");
  }
}
