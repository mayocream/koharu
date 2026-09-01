const dot = document.getElementById("dot");
const statusLabel = document.getElementById("status-label");
const statusNote = document.getElementById("status-note");
const powerButton = document.getElementById("power");

let engineRunning = false;

function refresh() {
  chrome.runtime.sendMessage({ action: "GetStatus" }, (status) => {
    if (chrome.runtime.lastError || !status) {
      render({ connected: false, active: 0, lastError: "Background worker unavailable." });
      return;
    }
    render(status);
  });
}

function render(status) {
  const { connected, active, lastError } = status;

  engineRunning = connected;
  dot.className = "dot";
  statusNote.className = "status-note";
  powerButton.className = connected ? "stop" : "";
  powerButton.disabled = false;

  if (connected) {
    dot.classList.add("online");
    statusLabel.textContent = "Engine running";
    statusNote.textContent = active > 0
      ? `${active} ${active === 1 ? "page" : "pages"} translating`
      : "Models loaded and idle";
    powerButton.textContent = active > 0 ? "Stop engine (cancels work)" : "Stop engine";
  } else if (lastError) {
    dot.classList.add("error");
    statusNote.classList.add("error");
    statusLabel.textContent = "Disconnected";
    statusNote.textContent = lastError;
    powerButton.textContent = "Retry";
  } else {
    statusLabel.textContent = "Engine idle";
    statusNote.textContent = "Starts on the first translation.";
    powerButton.textContent = "Start engine";
  }
}

powerButton.addEventListener("click", () => {
  const action = engineRunning ? "Disconnect" : "Connect";
  powerButton.disabled = true;
  statusNote.className = "status-note";

  if (engineRunning) {
    powerButton.textContent = "Stopping…";
    statusLabel.textContent = "Unloading models…";
    statusNote.textContent = "Frees the GPU memory the host is holding.";
  } else {
    powerButton.textContent = "Starting…";
    statusLabel.textContent = "Loading models…";
    statusNote.textContent = "This takes a few seconds on first start.";
  }

  chrome.runtime.sendMessage({ action }, () => {
    if (chrome.runtime.lastError) {
      console.warn(`${action} failed:`, chrome.runtime.lastError);
    }
    refresh();
  });
});

refresh();
setInterval(refresh, 1000);
