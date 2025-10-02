// ==UserScript==
// @name         Check GitHub Repo Archive
// @namespace    http://tampermonkey.net/
// @version      0.1
// @description  Checks if the current GitHub repository is in the archive
// @author       You
// @match        https://github.com/*/*
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

function notify(res) {
  if (res.exists) {
    document.body.style.border = "2px solid green";
  } else {
    document.body.style.border = "2px solid red";
  }
}

const cache = new Map();

(function () {
  "use strict";

  async function checkRepoInArchive(url) {
    url = transformGitHubUrl(url);
    const apiUrl = "http://127.0.0.1:8081/has_git_repo";

    if (cache.has(url)) {
      const cached = cache.get(url);

      if (cached !== null) {
        notify(cache.get(url));
      }

      return;
    }

    cache.set(url, null);

    GM_xmlhttpRequest({
      method: "POST",
      url: apiUrl,
      headers: {
        "Content-Type": "application/json",
      },
      data: JSON.stringify({ url }),
      onload: (response) => {
        if (response.status === 200) {
          const result = JSON.parse(response.responseText);
          notify(result);
          cache.set(url, result);
        } else {
          console.error("Error checking repository:", response.statusText);
        }
      },
      onerror: (error) => {
        document.body.style.border = "2px solid yellow";
        console.log("error: " + JSON.stringify(error));
      },
    });
  }

  checkRepoInArchive(window.location.href);

  const observer = new MutationObserver((mutations) => {
    mutations.forEach((mutation) => {
      if (mutation.type === "childList" || mutation.type === "attributes") {
        if (window.location.href !== document.referrer) {
          checkRepoInArchive(window.location.href);
        }
      }
    });
  });

  observer.observe(document.body, {
    childList: true,
    subtree: true,
    attributes: true,
  });
})();
