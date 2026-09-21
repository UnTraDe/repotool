// ==UserScript==
// @name         Check Repo Archive
// @namespace    http://tampermonkey.net/
// @version      0.4
// @description  Checks if the current GitHub or Hugging Face repository is in the archive
// @author       You
// @match        https://github.com/*/*
// @match        https://huggingface.co/*
// @grant        GM_xmlhttpRequest
// ==/UserScript==

function transformGitHubUrl(url) {
  const parsedUrl = new URL(url);
  const pathname = parsedUrl.pathname;
  const pathSegments = pathname.split("/");
  const org = pathSegments[1];
  const repo = pathSegments[2];

  return `${parsedUrl.protocol}//${parsedUrl.host}/${org}/${repo}`;
}

function transformHuggingfaceUrl(url) {
  const parsedUrl = new URL(url);
  const pathname = parsedUrl.pathname;
  const pathSegments = pathname.split("/");
  const org = pathSegments[1];
  const repo = pathSegments[2];

  return `${org}/${repo}`;
}

const ARCHIVE_PATH = "/data/archive.txt";
const GRAB_COMMAND = `sudo docker run --rm --user 1000:3001 -v /mnt/red-cluster1/backup/sources:/data repotool:latest grab --base-dir /data --archive ${ARCHIVE_PATH} github`;

function createCopyButton(label, buildCommand) {
  const btn = document.createElement("button");
  btn.textContent = label;
  btn.style.cssText = `
    margin-top: 8px;
    padding: 8px 12px;
    background: #238636;
    border: 1px solid #2ea043;
    border-radius: 6px;
    color: white;
    font-size: 14px;
    cursor: pointer;
    width: 100%;
    transition: background 0.2s;
  `;
  btn.onmouseover = () => (btn.style.background = "#2ea043");
  btn.onmouseout = () => (btn.style.background = "#238636");
  btn.onclick = () => {
    navigator.clipboard
      .writeText(buildCommand())
      .then(() => {
        btn.textContent = "Copied!";
        setTimeout(() => {
          btn.textContent = label;
        }, 2000);
      })
      .catch((err) => {
        console.error("Failed to copy:", err);
        btn.textContent = "Failed to copy";
        setTimeout(() => {
          btn.textContent = label;
        }, 2000);
      });
  };

  return btn;
}

function createPanel(res, url, hostname) {
  // Remove existing panel if any
  const existingPanel = document.getElementById("repo-checker-panel");
  if (existingPanel) {
    existingPanel.remove();
  }

  const panel = document.createElement("div");
  panel.id = "repo-checker-panel";
  panel.style.cssText = `
    position: fixed;
    bottom: 20px;
    left: 20px;
    background: ${res.exists ? "#1a472a" : "#5c1f1f"};
    color: white;
    border: 2px solid ${res.exists ? "#2ea043" : "#da3633"};
    border-radius: 8px;
    padding: 12px;
    font-size: 14px;
    z-index: 10000;
    min-width: 250px;
    max-width: 500px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
  `;

  const title = document.createElement("div");
  title.style.cssText = `
    font-weight: 600;
    font-size: 16px;
    margin-bottom: 8px;
    display: flex;
    align-items: center;
    gap: 8px;
  `;

  const statusIcon = res.exists ? "✓" : "✗";
  const statusText = res.exists ? "Repository Archived" : "Not Archived";
  title.innerHTML = `<span style="font-size: 20px;">${statusIcon}</span> ${statusText}`;
  panel.appendChild(title);

  // Add copy command buttons for GitHub repos
  if (hostname === "github.com" && url) {
    const org = new URL(url).pathname.split("/")[1];

    if (!res.exists) {
      panel.appendChild(
        createCopyButton(
          "Copy grab command",
          () => `${GRAB_COMMAND} single "${url}.git"`,
        ),
      );
    }

    if (org) {
      const compareLabel = document.createElement("label");
      compareLabel.style.cssText = `
        display: flex;
        align-items: center;
        gap: 6px;
        margin-top: 8px;
        font-size: 13px;
        cursor: pointer;
        user-select: none;
      `;

      const compareCheckbox = document.createElement("input");
      compareCheckbox.type = "checkbox";
      compareCheckbox.style.cssText = "margin: 0; cursor: pointer;";

      compareLabel.appendChild(compareCheckbox);
      compareLabel.appendChild(
        document.createTextNode("--compare-file /data/archive.txt"),
      );
      panel.appendChild(compareLabel);

      panel.appendChild(
        createCopyButton("Copy org grab command", () => {
          const compare = compareCheckbox.checked
            ? ` --compare-file ${ARCHIVE_PATH}`
            : "";
          return `${GRAB_COMMAND} org "${org}"${compare}`;
        }),
      );
    }
  }

  if (res.exists && res.metadata) {
    const metadata = res.metadata;

    const addField = (label, value, monospace = false) => {
      const field = document.createElement("div");
      field.style.cssText = "margin-bottom: 4px;";

      const labelEl = document.createElement("div");
      labelEl.style.cssText =
        "color: #8b949e; font-size: 12px; margin-bottom: 1px;";
      labelEl.textContent = label;

      const valueEl = document.createElement("div");
      valueEl.style.cssText = `
        color: white;
        ${monospace ? "font-family: ui-monospace, SFMono-Regular, 'SF Mono', Menlo, Consolas, 'Liberation Mono', monospace; font-size: 12px;" : ""}
        word-break: break-all;
      `;
      valueEl.textContent = value;

      field.appendChild(labelEl);
      field.appendChild(valueEl);
      panel.appendChild(field);
    };

    addField("Path", metadata.path);
    addField("Last Commit", metadata.commit_hash.substring(0, 12), true);
    addField("Commit Date", metadata.commit_date);
    addField("Last Fetch", metadata.last_fetch);
  }

  // Add close button
  const closeBtn = document.createElement("button");
  closeBtn.textContent = "×";
  closeBtn.style.cssText = `
    position: absolute;
    top: 8px;
    right: 8px;
    background: none;
    border: none;
    color: white;
    font-size: 24px;
    cursor: pointer;
    padding: 0;
    width: 24px;
    height: 24px;
    line-height: 24px;
    text-align: center;
    opacity: 0.6;
    transition: opacity 0.2s;
  `;
  closeBtn.onmouseover = () => (closeBtn.style.opacity = "1");
  closeBtn.onmouseout = () => (closeBtn.style.opacity = "0.6");
  closeBtn.onclick = () => panel.remove();
  panel.appendChild(closeBtn);

  document.body.appendChild(panel);
}

const cache = new Map();

(function () {
  "use strict";

  async function checkRepoInArchive(url) {
    const hostname = new URL(url).hostname;
    let transformedUrl, apiUrl, requestData;

    if (hostname === "github.com") {
      transformedUrl = transformGitHubUrl(url);
      apiUrl = "http://127.0.0.1:8081/has_git_repo";
      requestData = JSON.stringify({ url: transformedUrl });
    } else if (hostname === "huggingface.co") {
      transformedUrl = transformHuggingfaceUrl(url);
      apiUrl = "http://127.0.0.1:8081/has_huggingface_repo";
      requestData = JSON.stringify({ repo: transformedUrl });
    } else {
      return;
    }

    if (cache.has(transformedUrl)) {
      const cached = cache.get(transformedUrl);

      if (cached !== null) {
        createPanel(cache.get(transformedUrl), transformedUrl, hostname);
      }

      return;
    }

    cache.set(transformedUrl, null);

    GM_xmlhttpRequest({
      method: "POST",
      url: apiUrl,
      headers: {
        "Content-Type": "application/json",
      },
      data: requestData,
      onload: (response) => {
        if (response.status === 200) {
          const result = JSON.parse(response.responseText);
          createPanel(result, transformedUrl, hostname);
          cache.set(transformedUrl, result);
        } else {
          console.error("Error checking repository:", response.statusText);
        }
      },
      onerror: (error) => {
        // Show error panel
        createPanel(
          {
            exists: false,
            error: true,
            message: "Connection error",
          },
          transformedUrl,
          hostname,
        );
        console.log("error: " + JSON.stringify(error));
      },
    });
  }

  let lastUrl = window.location.href;
  checkRepoInArchive(lastUrl);

  // Detect URL changes from SPA navigation
  const checkUrlChange = () => {
    const currentUrl = window.location.href;
    if (currentUrl !== lastUrl) {
      lastUrl = currentUrl;
      checkRepoInArchive(currentUrl);
    }
  };

  // Listen for history changes (back/forward buttons)
  window.addEventListener("popstate", checkUrlChange);

  // Intercept pushState and replaceState
  const originalPushState = history.pushState;
  const originalReplaceState = history.replaceState;

  history.pushState = function (...args) {
    originalPushState.apply(this, args);
    checkUrlChange();
  };

  history.replaceState = function (...args) {
    originalReplaceState.apply(this, args);
    checkUrlChange();
  };
})();
